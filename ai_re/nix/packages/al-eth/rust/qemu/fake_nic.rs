// SPDX-License-Identifier: GPL-2.0

//! fake_nic: a loopback Ethernet NIC on QEMU's "edu" PCI device, built only on
//! knet (no al_eth hal), to run knet's netdev/NAPI/skb/DMA code under the debug
//! kernel in QEMU (KFENCE, slub_debug, lockdep, DMA-API debug, kmemleak).
//!
//! The "wire" is edu's DMA engine (one transfer at a time, ~100 ms each, MSI on
//! completion) and its 4 KiB buffer: a transmitted frame's head and fragments
//! are DMA'd from their streaming mappings into the edu buffer, then from there
//! into one or more mapped RX buffers, and the frame is received on the same
//! interface. So TX mapping (head + page fragments), BQL, the TX queue lock,
//! RX build_skb / copy-small / multi-buffer frames, NAPI with a real interrupt
//! and real device DMA are all exercised; nothing touches a mapped buffer.
//!
//! QEMU: -device edu,dma_mask=0xffffffff (the default 28-bit mask cannot reach
//! virt's RAM at 1 GiB; 32-bit BAR accesses limit us to 32-bit addresses).

use core::sync::atomic::{AtomicU64, Ordering};
use kernel::{
    device::{Bound, Core, Device},
    dma::{Device as _, DmaMask},
    io::Io,
    pci,
    prelude::*,
    time::{delay::fsleep, Delta},
};

#[path = "../knet/mod.rs"]
mod knet;

use knet::{
    dma::{RxBuf, RxFrag, TxMapped},
    ether::MacAddr,
    ethtool::{DrvInfo, LinkKsettings, Port},
    napi::{Bh, Kind, NapiCtx, NapiIrq, Poll},
    netdev::{AddrLists, Config, Features, NetDev, Ops as NetOps, Registration, Stats64, TxGuard, TxLocked, Unregistered, Xmit},
    skb::SkBuff,
};

// edu registers (qemu hw/misc/edu.c)
const EDU_ID: usize = 0x00;
const EDU_IRQ_ACK: usize = 0x64;
const EDU_DMA_SRC: usize = 0x80;
const EDU_DMA_DST: usize = 0x88;
const EDU_DMA_CNT: usize = 0x90;
const EDU_DMA_CMD: usize = 0x98;
const DMA_RUN: u64 = 1;
const DMA_TO_PCI: u64 = 2;
const DMA_IRQ: u64 = 4;
const DMA_IRQ_STATUS: u32 = 0x100;
const EDU_BUF: u64 = 0x40000;
const EDU_BUF_LEN: u32 = 4096;
const BAR_SIZE: usize = 0x100;

const TX_RING: usize = 16;
const RX_RING: usize = 16;
const RX_BUF: u32 = 2048;
const RX_HEADROOM: u32 = 64; // NET_SKB_PAD-like headroom for build_skb
const SMALL_COPY: u32 = 128;
const MAX_MTU: u32 = 4000;

type Bar<'b> = pci::Bar<'b, BAR_SIZE>;

kernel::pci_device_table!(
    PCI_TABLE,
    <FakeNic as pci::Driver>::IdInfo,
    // No QEMU vendor (0x1234) constant in pci::Vendor: match edu's class code
    // 0x00ff00 and check 1234:11e8 in probe.
    [(pci::DeviceId::from_class(0x00ff00, 0xffff00), ())]
);

/// Packets on their way through the wire (owned by the TX queue's lock).
struct TxRing<'b> {
    slots: KVec<Option<TxMapped<'b>>>,
    head: usize, // next free
    tail: usize, // oldest in flight
    used: usize,
}

impl<'b> TxRing<'b> {
    fn new() -> Result<Self> {
        let mut slots = KVec::with_capacity(TX_RING, GFP_KERNEL)?;
        for _ in 0..TX_RING {
            slots.push(None, GFP_KERNEL)?;
        }
        Ok(Self { slots, head: 0, tail: 0, used: 0 })
    }
    fn full(&self) -> bool {
        self.used == TX_RING
    }
    fn push(&mut self, m: TxMapped<'b>) {
        self.slots[self.head] = Some(m);
        self.head = (self.head + 1) % TX_RING;
        self.used += 1;
    }
    fn oldest(&self) -> Option<&TxMapped<'b>> {
        self.slots[self.tail].as_ref()
    }
    fn pop(&mut self) -> Option<TxMapped<'b>> {
        let m = self.slots[self.tail].take()?;
        self.tail = (self.tail + 1) % TX_RING;
        self.used -= 1;
        Some(m)
    }
    /// Device idle: drop (unmap + free) everything.
    fn drain(&mut self) -> usize {
        let mut n = 0;
        while self.pop().is_some() {
            n += 1;
        }
        self.head = 0;
        self.tail = 0;
        n
    }
}

/// Where the engine is with the packet at the TX tail.
#[derive(Clone, Copy)]
enum Step {
    Idle,
    /// DMA of segment seg (from the packet) into the edu buffer at off is running.
    Gather { seg: usize, off: u32 },
    /// DMA of edu buffer bytes [off, off + n) into RX buffer i is running.
    Scatter { i: usize, off: u32, len: u32 },
}

/// The NAPI context's state: the RX ring and the wire engine.
struct Engine<'b> {
    dev: &'b Device<Bound>,
    rx: KVec<Option<RxBuf<'b>>>,
    rx_next: usize,
    step: Step,
    /// frames > SMALL_COPY seen: odd ones take the GRO frags path
    frags_seen: u64,
}

impl<'b> Engine<'b> {
    fn new(dev: &'b Device<Bound>) -> Result<Self> {
        let mut rx = KVec::with_capacity(RX_RING, GFP_KERNEL)?;
        for _ in 0..RX_RING {
            rx.push(None, GFP_KERNEL)?;
        }
        Ok(Self { dev, rx, rx_next: 0, step: Step::Idle, frags_seen: 0 })
    }

    /// Fills empty RX slots; bh: inside the NAPI poll (the NAPI fragment cache).
    fn refill(&mut self, bh: Option<Bh<'_>>) {
        for slot in self.rx.iter_mut().filter(|s| s.is_none()) {
            let frag = match bh {
                Some(bh) => RxFrag::alloc_bh(RX_BUF, RX_HEADROOM, bh),
                None => RxFrag::alloc(RX_BUF, RX_HEADROOM),
            };
            let Ok(frag) = frag else { return };
            match RxBuf::map(self.dev, frag) {
                Ok(b) => *slot = Some(b),
                Err(_) => return,
            }
        }
    }

    fn rx_slot(&self, i: usize) -> usize {
        (self.rx_next + i) % RX_RING
    }

    /// Whether RX buffers for a frame of len bytes are on the ring.
    fn rx_ready(&self, len: u32) -> bool {
        let n = len.div_ceil(RX_BUF) as usize;
        n <= RX_RING && (0..n).all(|i| self.rx[self.rx_slot(i)].is_some())
    }

    /// Takes a frame of len bytes (> SMALL_COPY) out of the RX ring into the
    /// context's GRO frags skb and hands it to GRO (the al_eth RX path). Every
    /// 8th such frame first drops an empty guard: the free-and-clear path and a
    /// fresh napi_get_frags run under the checkers too. False if dropped.
    fn receive_frags(&mut self, napi: &mut NapiCtx<'_>, len: u32) -> bool {
        self.frags_seen += 1;
        if self.frags_seen % 8 == 0 {
            drop(napi.frags());
        }
        let Some(mut g) = napi.frags() else { return false };
        let mut off = 0;
        let mut i = 0;
        while off < len {
            let n = (len - off).min(RX_BUF);
            let slot = self.rx_slot(i);
            let Some(buf) = self.rx[slot].take() else { return false };
            if g.add_rx_frag(buf.unmap(), n).is_err() {
                return false;
            }
            off += n;
            i += 1;
        }
        self.rx_next = (self.rx_next + i) % RX_RING;
        g.record_rx_queue(0);
        g.gro()
    }

    /// Takes the frame of len bytes out of the RX ring as an skb.
    fn receive(&mut self, napi: &mut NapiCtx<'_>, len: u32) -> Option<SkBuff> {
        let first = self.rx_slot(0);
        let n0 = len.min(RX_BUF);
        let mut skb = if len <= SMALL_COPY {
            // Copy and keep the buffer on the ring.
            let mut skb = napi.alloc_skb(len)?;
            let buf = self.rx[first].as_mut()?;
            let view = buf.sync_for_cpu(len);
            skb.put(len).ok()?.copy_from_slice(view.bytes());
            drop(view);
            self.rx_next = (self.rx_next + 1) % RX_RING;
            return Some(skb);
        } else {
            let frag = self.rx[first].take()?.unmap();
            match frag.into_skb(n0) {
                Ok(s) => s,
                Err(_frag) => return None,
            }
        };
        let mut off = n0;
        let mut i = 1;
        while off < len {
            let n = (len - off).min(RX_BUF);
            let slot = self.rx_slot(i);
            let frag = self.rx[slot].take()?.unmap();
            if skb.add_rx_frag(frag, 0, n).is_err() {
                return None;
            }
            off += n;
            i += 1;
        }
        self.rx_next = (self.rx_next + i) % RX_RING;
        Some(skb)
    }
}

fn dma_busy(bar: &Bar<'_>) -> bool {
    u64::from(bar.read32(EDU_DMA_CMD)) & DMA_RUN != 0
}

/// Starts one edu DMA. The BAR allows 32-bit accesses only, which set the whole
/// edu register (zero-extended): the DMA mask is 32 bits for that reason.
fn dma_start(bar: &Bar<'_>, src: u64, dst: u64, len: u32, to_pci: bool) {
    bar.write32(src as u32, EDU_DMA_SRC);
    bar.write32(dst as u32, EDU_DMA_DST);
    bar.write32(len, EDU_DMA_CNT);
    bar.write32((DMA_RUN | DMA_IRQ | if to_pci { DMA_TO_PCI } else { 0 }) as u32, EDU_DMA_CMD);
}

/// What the engine did with the packet at the TX tail.
enum Action {
    /// A DMA is running; its completion interrupt schedules NAPI again.
    Wait,
    /// The frame is through the wire (true) or dropped (false): complete it.
    Finish(bool),
}

impl Engine<'_> {
    /// Advances the engine for the oldest TX packet (whose DMA, if any, is done).
    fn advance(&mut self, bar: &Bar<'_>, pkt: &TxMapped<'_>, bh: Option<Bh<'_>>) -> Action {
        let len = pkt.meta().len;
        let segs = pkt.segs();
        match self.step {
            Step::Idle => {
                let Some(s) = segs.first() else { return Action::Finish(false) };
                if !(14..=EDU_BUF_LEN).contains(&len) {
                    return Action::Finish(false);
                }
                dma_start(bar, s.0, EDU_BUF, s.1, false);
                self.step = Step::Gather { seg: 0, off: 0 };
                Action::Wait
            }
            Step::Gather { seg, off } => {
                let off = off + segs.get(seg).map_or(0, |s| s.1);
                if let Some(s) = segs.get(seg + 1) {
                    dma_start(bar, s.0, EDU_BUF + u64::from(off), s.1, false);
                    self.step = Step::Gather { seg: seg + 1, off };
                    return Action::Wait;
                }
                if !self.rx_ready(len) {
                    self.refill(bh);
                }
                let Some(buf) = self.rx[self.rx_slot(0)].as_ref().filter(|_| self.rx_ready(len)) else {
                    return Action::Finish(false);
                };
                let n = len.min(RX_BUF);
                dma_start(bar, EDU_BUF, buf.dma_addr(), n, true);
                self.step = Step::Scatter { i: 0, off: 0, len: n };
                Action::Wait
            }
            Step::Scatter { i, off, len: n } => {
                let off = off + n;
                if off >= len {
                    return Action::Finish(true);
                }
                let Some(buf) = self.rx[self.rx_slot(i + 1)].as_ref() else { return Action::Finish(false) };
                let n = (len - off).min(RX_BUF);
                dma_start(bar, EDU_BUF + u64::from(off), buf.dma_addr(), n, true);
                self.step = Step::Scatter { i: i + 1, off, len: n };
                Action::Wait
            }
        }
    }
}

impl<'b> Poll for Engine<'b> {
    type Dev = Priv<'b>;

    fn poll(&mut self, dev: &NetDev<Priv<'b>>, napi: &mut NapiCtx<'_>, budget: u32) -> u32 {
        let bh = napi.bh();
        let p = dev.data();
        p.bar.write32(DMA_IRQ_STATUS, EDU_IRQ_ACK);
        let mut work = 0;
        while work < budget && !dma_busy(&p.bar) {
            let Some(mut tx) = napi.tx_lock(dev, 0) else { break };
            let Some(ring) = p.tx.get(&mut tx) else { break };
            let Some(pkt) = ring.oldest() else { break };
            let len = pkt.meta().len;
            let delivered = match self.advance(&p.bar, pkt, bh) {
                Action::Wait => break,
                Action::Finish(d) => d,
            };
            self.step = Step::Idle;
            // TX completion: the device has read the whole packet.
            let Some(pkt) = ring.pop() else { break };
            if module_parameters::inject.value() == 1 {
                // Test of the test: leak the packet and its mappings (safe code
                // can leak). kmemleak and DMA-API debug must report it.
                tx.completed(1, len);
                core::mem::forget(pkt);
                drop(tx);
                work += 1;
                continue;
            }
            let skb = pkt.unmap();
            let bytes = skb.len();
            tx.completed(1, bytes);
            if tx.queue().stopped() {
                tx.queue().wake();
            }
            drop(tx);
            p.tx_packets.fetch_add(1, Ordering::Relaxed);
            p.tx_bytes.fetch_add(u64::from(bytes), Ordering::Relaxed);
            napi.consume_skb(skb);
            work += 1;
            if !delivered {
                p.rx_dropped.fetch_add(1, Ordering::Relaxed);
                continue;
            }
            // RX: the device has written the frame into the ring's buffers.
            // Large frames alternate between the build_skb path and GRO frags.
            if len > SMALL_COPY && self.frags_seen % 2 == 0 {
                if self.receive_frags(napi, len) {
                    p.rx_packets.fetch_add(1, Ordering::Relaxed);
                    p.rx_bytes.fetch_add(u64::from(len), Ordering::Relaxed);
                } else {
                    p.rx_dropped.fetch_add(1, Ordering::Relaxed);
                    self.rx_next = (self.rx_next + len.div_ceil(RX_BUF) as usize) % RX_RING;
                }
                self.refill(bh);
                continue;
            }
            if len > SMALL_COPY {
                self.frags_seen += 1;
            }
            match self.receive(napi, len) {
                Some(mut skb) => {
                    if skb.eth_type_trans(dev).is_ok() {
                        skb.record_rx_queue(0);
                        p.rx_packets.fetch_add(1, Ordering::Relaxed);
                        p.rx_bytes.fetch_add(u64::from(len), Ordering::Relaxed);
                        skb.gro_receive(napi);
                    } else {
                        p.rx_dropped.fetch_add(1, Ordering::Relaxed);
                    }
                }
                None => {
                    p.rx_dropped.fetch_add(1, Ordering::Relaxed);
                    self.rx_next = (self.rx_next + len.div_ceil(RX_BUF) as usize) % RX_RING;
                }
            }
            self.refill(bh);
        }
        if work < budget {
            napi.complete_done(work);
        }
        work
    }
}

/// The device's private data.
#[pin_data]
struct Priv<'b> {
    // First: dropped first (frees the IRQ, then deletes the NAPI context).
    #[pin]
    irq: NapiIrq<'b, Engine<'b>>,
    tx: TxLocked<TxRing<'b>>,
    bar: Bar<'b>,
    dev: &'b Device<Bound>,
    tx_packets: AtomicU64,
    tx_bytes: AtomicU64,
    rx_packets: AtomicU64,
    rx_bytes: AtomicU64,
    rx_dropped: AtomicU64,
    tx_dropped: AtomicU64,
    uc: AtomicU64,
    mc: AtomicU64,
}

/// Waits (sleeping) until the edu DMA engine is idle: before buffers are freed.
fn wait_dma_idle(bar: &Bar<'_>) -> bool {
    for _ in 0..200 {
        if !dma_busy(bar) {
            return true;
        }
        fsleep(Delta::from_millis(10));
    }
    false
}

#[vtable]
impl<'b> NetOps for Priv<'b> {
    fn open(dev: &NetDev<Self>) -> Result {
        let p = dev.data();
        let napi = p.irq.napi();
        let mut st = napi.parked().ok_or(EBUSY)?;
        st.step = Step::Idle;
        st.rx_next = 0;
        st.refill(None);
        st.enable();
        dev.carrier_on();
        dev.tx_start_all();
        Ok(())
    }

    fn stop(dev: &NetDev<Self>) -> Result {
        let p = dev.data();
        dev.tx_disable();
        dev.carrier_off();
        let Some(mut st) = p.irq.napi().disable() else { return Ok(()) };
        // The device must be done with every buffer before any is freed.
        if !wait_dma_idle(&p.bar) {
            pr_err!("fake_nic: DMA engine stuck, leaking buffers\n");
            core::mem::forget(st); // keeps NAPI disabled and the buffers mapped
            return Err(EBUSY);
        }
        st.step = Step::Idle;
        for slot in st.rx.iter_mut() {
            *slot = None; // unmap + free
        }
        drop(st);
        if let Some(mut tx) = dev.tx_lock_bh(0) {
            if let Some(ring) = p.tx.get(&mut tx) {
                let n = ring.drain();
                p.tx_dropped.fetch_add(n as u64, Ordering::Relaxed);
            }
            tx.reset();
        }
        Ok(())
    }

    fn start_xmit(dev: &NetDev<Self>, mut skb: SkBuff, txq: &mut TxGuard<'_>) -> Xmit {
        let p = dev.data();
        // "Hardware" checksum: in software before mapping (we advertise HW_CSUM
        // so that the stack hands us fragmented skbs).
        if skb.checksum_help().is_err() {
            p.tx_dropped.fetch_add(1, Ordering::Relaxed);
            return Xmit::Ok;
        }
        let Some(ring) = p.tx.get(txq) else {
            p.tx_dropped.fetch_add(1, Ordering::Relaxed);
            return Xmit::Ok;
        };
        if ring.full() {
            txq.queue().stop();
            return Xmit::Busy(skb);
        }
        let len = skb.len();
        skb.tx_timestamp();
        match TxMapped::map(p.irq_dev(), skb) {
            Ok(m) => ring.push(m),
            Err((_, skb)) => {
                drop(skb);
                p.tx_dropped.fetch_add(1, Ordering::Relaxed);
                return Xmit::Ok;
            }
        }
        let full = ring.full();
        txq.sent(len);
        if full {
            txq.queue().stop();
        }
        // Doorbell: the engine runs in NAPI context.
        p.irq.napi().schedule();
        Xmit::Ok
    }

    fn get_stats64(dev: &NetDev<Self>, s: &mut Stats64) {
        let p = dev.data();
        s.tx_packets = p.tx_packets.load(Ordering::Relaxed);
        s.tx_bytes = p.tx_bytes.load(Ordering::Relaxed);
        s.rx_packets = p.rx_packets.load(Ordering::Relaxed);
        s.rx_bytes = p.rx_bytes.load(Ordering::Relaxed);
        s.rx_dropped = p.rx_dropped.load(Ordering::Relaxed);
        s.tx_dropped = p.tx_dropped.load(Ordering::Relaxed);
    }

    fn set_rx_mode(dev: &NetDev<Self>, addrs: &AddrLists<'_>) {
        let p = dev.data();
        let (mut uc, mut mc) = (0u64, 0u64);
        addrs.for_each_uc(|_| uc += 1);
        addrs.for_each_mc(|a: MacAddr| {
            if a.is_multicast() {
                mc += 1
            }
        });
        p.uc.store(uc, Ordering::Relaxed);
        p.mc.store(mc, Ordering::Relaxed);
    }

    fn change_mtu(_dev: &NetDev<Self>, new_mtu: u32, _old: u32) -> Result {
        if new_mtu > MAX_MTU {
            return Err(EINVAL);
        }
        Ok(())
    }

    fn set_mac_address(_dev: &NetDev<Self>, _addr: MacAddr) -> Result {
        Ok(())
    }

    fn tx_timeout(_dev: &NetDev<Self>, queue: u32) {
        pr_warn!("fake_nic: tx timeout on queue {}\n", queue);
    }

    fn set_features(_dev: &NetDev<Self>, _f: Features) -> Result {
        Ok(())
    }

    fn get_drvinfo(_dev: &NetDev<Self>, info: &mut DrvInfo<'_>) {
        info.set_driver("fake_nic");
        info.set_version("0.1");
        info.set_bus_info("edu");
    }

    fn get_link_ksettings(_dev: &NetDev<Self>, ks: &mut LinkKsettings<'_>) -> Result {
        ks.set_speed_duplex(1000, true);
        ks.set_port(Port::Tp);
        ks.set_autoneg(false);
        ks.add_link_mode(kernel::bindings::ethtool_link_mode_bit_indices_ETHTOOL_LINK_MODE_1000baseT_Full_BIT);
        Ok(())
    }
}

impl<'b> Priv<'b> {
    fn irq_dev(&self) -> &'b Device<Bound> {
        self.dev
    }
}

/// The driver's data: the net device, then the interrupt vectors its IRQ uses.
#[pin_data]
struct Data<'b> {
    // First: dropped first (the IRQ inside must go before the vectors).
    netdev: Registration<Priv<'b>>,
    vectors: pci::IrqVectorRegistration<'b>,
}

struct FakeNic;

impl pci::Driver for FakeNic {
    type IdInfo = ();
    type Data<'b> = Data<'b>;
    const ID_TABLE: pci::IdTable<Self::IdInfo> = &PCI_TABLE;

    fn probe<'b>(pdev: &'b pci::Device<Core<'_>>, _info: Option<&'b ()>) -> impl PinInit<Data<'b>, Error> + 'b {
        let r = (|| -> Result<_> {
            if pdev.vendor_id().as_raw() != 0x1234 || pdev.device_id() != 0x11e8 {
                return Err(ENODEV);
            }
            pdev.enable_device_mem()?;
            pdev.set_master();
            // SAFETY: probe, before any DMA allocation or mapping for this device.
            unsafe { pdev.dma_set_mask_and_coherent(DmaMask::new::<32>())? };
            let vectors = pdev.alloc_irq_vectors(1, 1, pci::IrqTypes::default().with(pci::IrqType::Msi))?;
            Ok(vectors)
        })();
        let bdev: &'b Device<Bound> = pdev.as_ref();
        let ppdev: &'b pci::Device<Bound> = pdev;
        try_pin_init!(&this in Data {
            vectors: r?,
            netdev: {
                // SAFETY: vectors is initialised (above), pinned with Data and never
                // moved or mutated, and dropped after netdev (field order), so a
                // reference to it is valid for everything netdev holds. Nothing
                // escapes netdev: its IRQ registration only keeps the IRQ number.
                let vectors: &'b pci::IrqVectorRegistration<'b> = unsafe { &*(&raw const (*this.as_ptr()).vectors) };
                let irq = vectors.index(0)?;
                let bar = ppdev.iomap_region_sized::<BAR_SIZE>(0, c"fake_nic")?;
                if bar.read32(EDU_ID) >> 16 != 0x0100 {
                    return Err(ENODEV);
                }
                Registration::<Priv<'b>>::new(bdev, Config { tx_queues: 1, rx_queues: 1 }, |u: &Unregistered<Priv<'b>>| {
                    u.set_random_mac_addr();
                    let f = Features::bit(kernel::bindings::NETIF_F_SG_BIT)
                        .with(Features::bit(kernel::bindings::NETIF_F_HW_CSUM_BIT));
                    u.set_features(f, f, Features::default());
                    u.set_mtu(1500, 68, MAX_MTU)?;
                    u.set_watchdog_timeo(kernel::time::msecs_to_jiffies(60_000) as i32);
                    let engine = Engine::new(bdev)?;
                    let tx = TxLocked::new(u, 0, TxRing::new()?)?;
                    // SAFETY: placed in the private data of u's Registration, which
                    // drops it before freeing the device; never forgotten.
                    let irq = unsafe { NapiIrq::new(u, Kind::Rx, engine, irq.into(), c"fake_nic") };
                    Ok(try_pin_init!(Priv {
                        irq <- irq,
                        tx,
                        bar,
                        dev: bdev,
                        tx_packets: AtomicU64::new(0),
                        tx_bytes: AtomicU64::new(0),
                        rx_packets: AtomicU64::new(0),
                        rx_bytes: AtomicU64::new(0),
                        rx_dropped: AtomicU64::new(0),
                        tx_dropped: AtomicU64::new(0),
                        uc: AtomicU64::new(0),
                        mc: AtomicU64::new(0),
                    }? Error))
                })?
            },
        })
    }
}

kernel::module_pci_driver! {
    type: FakeNic,
    name: "fake_nic",
    authors: ["ai_re"],
    description: "Loopback test NIC on QEMU edu, for knet",
    license: "GPL",
    params: {
        inject: i32 {
            default: 0,
            description: "test of the test only: 1 = leak every transmitted packet and its DMA mappings",
        },
    },
}
