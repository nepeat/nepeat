// SPDX-License-Identifier: GPL-2.0

//! al_eth: the Annapurna Labs Alpine integrated Ethernet port, in Rust.
//!
//! The DMC-12's mode (RGMII, fixed 1000/full link, no PHY, no SerDes, PCI
//! 1c36:0001 revision 2): everything the C driver does on that board, on top of
//! [hal] (the hardware sequences, safe `core`-only Rust, register for register
//! equal to the vendor C: hal/README.md) and [knet] (the kernel networking
//! interfaces, where the unsafe code lives). This file glues the two: which
//! kernel object owns which hal object, and when. The CS8210's modes (SGMII +
//! MDIO/phylib, 10G serial) and PCI revision 4 are refused at probe (the C
//! module drives them).
//!
//! Ownership (who has what, when):
//! - [Priv] (the net_device's private data, owned by a knet Registration) holds
//!   the control state [hal::port::Port] behind a mutex (open/stop/ndo calls,
//!   all under RTNL), one TX queue per netdev TX queue behind that queue's own
//!   `_xmit_lock` ([TxLocked]: start_xmit and the TX completion NAPI), and one
//!   NAPI context per RX queue owning its [RxQueue] (only its poll function, or
//!   open/stop while the context is disabled, can reach it).
//! - Packet buffers are knet's mapped types ([RxBuf], [TxMapped]: buffer + DMA
//!   mapping, unmapped on drop) and live in hal's ring slots from the moment
//!   they are posted until hal hands them back (completion, or the drain in
//!   `Port::down` after the DMA engine is stopped). There is no other way to get
//!   one out of a ring, so a buffer cannot be freed twice or while the device
//!   may still use it.
//! - Open takes the queues out of their homes (NAPI disabled, TX queues
//!   stopped), hands all eight to `Port::up` (fresh zeroed descriptor rings each
//!   time, as the C allocates them), and puts them back before enabling NAPI;
//!   stop does the reverse around `Port::down`. A failed up is followed by a down
//!   before any buffer can be freed.
//! - The BARs are mapped once and shared (an [Arc]) by the port, the queues'
//!   register access and the management interrupt.

use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use kernel::{
    bindings,
    device::{Bound, Core, Device},
    dma::{Device as _, DmaMask},
    io::Io,
    irq::{self, IrqReturn},
    new_mutex, new_spinlock_irq,
    pci::{self, ConfigSpaceSize},
    prelude::*,
    str::CStr,
    sync::{Arc, Mutex, SpinLockIrq},
    time::{delay::fsleep, Delta},
};

mod hal;
mod knet;
mod trace;

use trace::{Ctx, Mark};

use hal::{
    io::{Bars, CDescMem, Delay, DescMem, PciCfg, RegIo},
    iofic::{self, Group},
    mac::{self, MacMode, MacStats, MacTotals},
    port::{self, BoardCfg, DmaBuf, Port, RxEvent, RxQueue, TxInfo, TxQueue, NUM_QUEUES, RING_SIZE, TX_STOP_THRESH},
    ring::TxSlot,
};
use knet::{
    dma::{DescRing, RxBuf, RxFrag, TxMapped},
    ether::MacAddr,
    ethtool::{Coalesce, DrvInfo, LinkKsettings, Port as EthPort, Rings},
    napi::{Bh, Kind, NapiCtx, NapiIrq, NapiOff, Poll},
    netdev::{AddrLists, Config, Features, NetDev, Ops as NetOps, Registration, Stats64, TxGuard, TxLocked, Unregistered, Xmit},
    skb::SkBuff,
};

/// Module and PCI driver name: "al_eth" (the board's driver), or "al_eth_rs"
/// (same code, built with `--cfg al_eth_rs`), which can be loaded on a system
/// whose al_eth is the C one, after `rmmod al_eth`, without shadowing it.
#[cfg(not(al_eth_rs))]
const NAME: &CStr = c"al_eth";
#[cfg(al_eth_rs)]
const NAME: &CStr = c"al_eth_rs";
const NAME_STR: &str = if cfg!(al_eth_rs) { "al_eth_rs" } else { "al_eth" };

const VENDOR_ANNAPURNA: u16 = 0x1c36;
const DEVICE_ETH: u16 = 0x0001; // PCI_DEVICE_ID_AL_ETH, the integrated standard unit
const REV_2: u8 = 2;
/// BARs of the integrated unit (al_eth.h: AL_ETH_UDMA_BAR, _MAC_BAR, _EC_BAR).
const BAR_UDMA: u32 = 0;
const BAR_MAC: u32 = 2;
const BAR_EC: u32 = 4;
/// MSI-X table entries as al_eth_enable_msix requests them: management at 2,
/// RX queue i at 3 + i, TX queue i at 7 + i (3 + AL_ETH_MAX_HW_QUEUES + i).
/// Entries 0 and 1 are allocated too (vectors are allocated from entry 0) and
/// never requested.
const MSIX_VECS: u32 = 11;
const MSIX_MGMT: usize = 2;
const MSIX_RX: usize = 3;
const MSIX_TX: usize = 7;
/// Headroom in front of an RX buffer's data (AL_ETH_RX_OFFSET = NET_SKB_PAD +
/// NET_IP_ALIGN: 64 + 0 on arm64), so a buffer can become an skb head (build_skb).
const RX_OFFSET: u32 = 64;
/// ETH_ZLEN: frames are padded with zeros to this (eth_skb_pad in the C).
const ETH_ZLEN: u32 = 60;
const ETH_P_8021Q: u16 = 0x8100;
const TX_TIMEOUT_MS: u32 = 30_000;

/// ethtool -S, in [Priv::get_stats]'s order: the driver's counters (64-bit,
/// software); raw hardware registers for locating RX loss (the UDMA S2M drop
/// counter; per queue the S2M packet counter and descriptor ring head/tail
/// pointers; the EC's receive-path counters, hal::port::EC_RX_STATS); then the
/// MAC's (hal::mac::MacStats, the counters al_eth_mac_stats_get reads, as 64-bit
/// totals: MacTotals) without the 10G MAC's (always 0 here). The raw registers
/// are 32-bit values as read (not known whether any clears on read: compare
/// two reads).
const DRV_NAMES: [&str; 7 + 4 * NUM_QUEUES] = [
    "rx_dropped", "rx_error_frames", "rx_refill_short", "rx_copied", "rx_gro_frags", "tx_dropped", "tx_timeouts",
    "rx0_packets", "rx1_packets", "rx2_packets", "rx3_packets", "rx0_bytes", "rx1_bytes", "rx2_bytes", "rx3_bytes",
    "tx0_packets", "tx1_packets", "tx2_packets", "tx3_packets", "tx0_bytes", "tx1_bytes", "tx2_bytes", "tx3_bytes",
];
const HW_NAMES: [&str; 1 + 3 * NUM_QUEUES] = [
    "udma_s2m_drop_pkt",
    "rx0_hw_pkt", "rx1_hw_pkt", "rx2_hw_pkt", "rx3_hw_pkt",
    "rx0_hw_desc_head", "rx1_hw_desc_head", "rx2_hw_desc_head", "rx3_hw_desc_head",
    "rx0_hw_desc_tail", "rx1_hw_desc_tail", "rx2_hw_desc_tail", "rx3_hw_desc_tail",
];
const MAC_NAMES: [&str; 32] = [
    "mac_rx_ucast", "mac_rx_mcast", "mac_rx_bcast", "mac_rx_pkts", "mac_tx_ucast", "mac_tx_mcast", "mac_tx_bcast",
    "mac_rx_errors", "mac_tx_errors", "mac_rx_frames_ok", "mac_tx_frames_ok", "mac_rx_octets_ok", "mac_tx_octets_ok",
    "mac_rx_undersize", "mac_rx_fragments", "mac_rx_jabbers", "mac_rx_oversize", "mac_rx_fcs_errors",
    "mac_rx_alignment_errors", "mac_rx_drop_events", "mac_tx_pause", "mac_rx_pause", "mac_octets", "mac_pkts_64",
    "mac_pkts_65_127", "mac_pkts_128_255", "mac_pkts_256_511", "mac_pkts_512_1023", "mac_pkts_1024_1518",
    "mac_pkts_1519_max", "mac_eee_in", "mac_eee_out",
];
const fn concat<const A: usize, const B: usize, const N: usize>(a: [&'static str; A], b: [&'static str; B]) -> [&'static str; N] {
    assert!(A + B == N);
    let mut out = [""; N];
    let mut i = 0;
    while i < N {
        out[i] = if i < A { a[i] } else { b[i - A] };
        i += 1;
    }
    out
}
const N_DRV_HW: usize = DRV_NAMES.len() + HW_NAMES.len();
const N_EC: usize = N_DRV_HW + port::EC_RX_STAT_NAMES.len();
const STATS_N: usize = N_EC + MAC_NAMES.len();
const STATS_NAMES: [&str; STATS_N] =
    concat::<N_EC, 32, STATS_N>(concat::<N_DRV_HW, 21, N_EC>(concat(DRV_NAMES, HW_NAMES), port::EC_RX_STAT_NAMES), MAC_NAMES);
/// Priv::moder: adaptive RX moderation on; else bits 0..7 = the fixed interval.
const MODER_ADAPTIVE: u32 = 1 << 8;
// hal sizes its TX packets (and the stop threshold) for this kernel's MAX_SKB_FRAGS.
const _: () = assert!(bindings::MAX_SKB_FRAGS as u32 == port::MAX_SKB_FRAGS);

const RX_IRQ_NAMES: [&CStr; NUM_QUEUES] = [c"al-eth-rx-comp-0", c"al-eth-rx-comp-1", c"al-eth-rx-comp-2", c"al-eth-rx-comp-3"];
const TX_IRQ_NAMES: [&CStr; NUM_QUEUES] = [c"al-eth-tx-comp-0", c"al-eth-tx-comp-1", c"al-eth-tx-comp-2", c"al-eth-tx-comp-3"];

kernel::pci_device_table!(
    PCI_TABLE,
    <AlEth as pci::Driver>::IdInfo,
    // pci::Vendor has no Annapurna Labs constant (not in pci_ids.h): match the
    // Ethernet class (0x020000, as 1c36:0001 reports it) and check vendor,
    // device and revision in probe.
    [(pci::DeviceId::from_class(0x020000, 0xffffff), ())]
);

fn hal_err(e: hal::Error) -> Error {
    match e {
        hal::Error::Invalid => EINVAL,
        hal::Error::Unsupported => EOPNOTSUPP,
        hal::Error::Timeout => ETIMEDOUT,
        hal::Error::NoSpace => ENOSPC,
        hal::Error::Hardware => EIO,
        hal::Error::NoBoardParams => ENODEV,
    }
}

/// The three register windows, mapped once at probe (and the device's MMIO
/// trace: a no-op unless built with --cfg al_eth_trace).
struct BarSet<'b> {
    udma: pci::Bar<'b>,
    mac: pci::Bar<'b>,
    ec: pci::Bar<'b>,
    tr: trace::Handle,
}

#[derive(Clone, Copy)]
enum Which {
    Udma,
    Mac,
    Ec,
}

/// One register window for hal: 32-bit little-endian readl/writel with a
/// runtime bounds check. An offset outside the BAR reads all ones and writes
/// nothing: hal's offsets are constants inside the windows, so this is a
/// guard, not a path.
/// `ctx` says in which context this window is used (the trace's " [pid]" or
/// " @"): the port's windows are process context, the datapath's are not.
#[derive(Clone)]
struct Reg<'b> {
    set: Arc<BarSet<'b>>,
    which: Which,
    ctx: Ctx,
}

impl<'b> Reg<'b> {
    fn bar(&self) -> (&pci::Bar<'b>, u8) {
        match self.which {
            Which::Udma => (&self.set.udma, b'u'),
            Which::Mac => (&self.set.mac, b'm'),
            Which::Ec => (&self.set.ec, b'e'),
        }
    }
}

impl RegIo for Reg<'_> {
    fn r32(&self, off: usize) -> u32 {
        let (b, n) = self.bar();
        let v = b.try_read32(off).unwrap_or(u32::MAX);
        self.set.tr.rw(false, n, off, v, self.ctx);
        v
    }
    fn w32(&self, off: usize, val: u32) {
        let (b, n) = self.bar();
        self.set.tr.rw(true, n, off, val, self.ctx);
        let _ = b.try_write32(val, off);
    }
}

fn bars<'b>(set: &Arc<BarSet<'b>>, ctx: Ctx) -> Bars<Reg<'b>> {
    Bars {
        udma: Reg { set: set.clone(), which: Which::Udma, ctx },
        ec: Reg { set: set.clone(), which: Which::Ec, ctx },
        mac: Reg { set: set.clone(), which: Which::Mac, ctx },
    }
}

/// hal's al_udelay, in process context only (probe, open, stop, MTU).
struct Sleep<'a>(&'a trace::Handle);

impl Delay for Sleep<'_> {
    fn udelay(&self, us: u32) {
        self.0.delay(us);
        fsleep(Delta::from_micros(i64::from(us)));
    }
}

/// The function's PCI configuration space (FLR). Probe checks it is the
/// extended (4 KiB) one: the FLR sequence reaches offset 0x1e0.
struct CfgSpace<'b>(&'b pci::Device<Bound>, trace::Handle);

impl PciCfg for CfgSpace<'_> {
    fn read32(&self, off: u16) -> u32 {
        let v = self.0.config_space_extended().and_then(|c| c.try_read32(usize::from(off))).unwrap_or(u32::MAX);
        self.1.cfg(false, off, v);
        v
    }
    fn write32(&self, off: u16, val: u32) {
        self.1.cfg(true, off, val);
        if let Ok(c) = self.0.config_space_extended() {
            let _ = c.try_write32(val, usize::from(off));
        }
    }
    /// The FLR reset MSI-X Enable and the vector table, but this driver's
    /// vectors exist from probe on (the C allocates them after the FLR, at
    /// open): write the kernel's cached MSI-X state back (2026-11-23 on u01,
    /// without this: no interrupt on any vector after open).
    fn after_flr(&self) {
        // SAFETY: `pci::Device` is a transparent wrapper of `struct pci_dev`
        // (rust/kernel/pci.rs), so the cast gives this bound device's pci_dev;
        // pci_restore_msi_state only rewrites the cached MSI/MSI-X state of
        // that device (a no-op without vectors). Process context (probe, open:
        // the only callers of the FLR).
        unsafe { bindings::pci_restore_msi_state(core::ptr::from_ref(self.0).cast::<bindings::pci_dev>().cast_mut()) };
    }
}

/// A descriptor ring for hal (submission or completion): zeroed coherent
/// memory, 16-byte entries, bounds-checked volatile accesses.
struct Desc(DescRing);

impl Desc {
    fn new(dev: &Device<Bound>) -> Result<Self> {
        Ok(Self(DescRing::new(dev, RING_SIZE)?))
    }
}

impl DescMem for Desc {
    fn write(&mut self, idx: usize, words: [u32; 4]) {
        let _ = self.0.write(idx, words);
    }
    fn read(&self, idx: usize) -> [u32; 4] {
        self.0.read(idx).unwrap_or([0; 4])
    }
    fn len(&self) -> usize {
        self.0.len()
    }
    fn dma_addr(&self) -> u64 {
        self.0.dma_addr()
    }
    /// Descriptor writes visible to the device before the doorbell.
    fn publish(&self) {
        knet::dma::dma_wmb();
    }
}

impl CDescMem for Desc {
    fn ctrl(&self, idx: usize) -> u32 {
        self.0.read_word(idx, 0).unwrap_or(0)
    }
    fn read(&self, idx: usize) -> [u32; 4] {
        self.0.read(idx).unwrap_or([0; 4])
    }
    fn len(&self) -> usize {
        self.0.len()
    }
    fn dma_addr(&self) -> u64 {
        self.0.dma_addr()
    }
    /// The rest of a completion after its control word said it is new.
    fn dma_rmb(&self) {
        knet::dma::dma_rmb();
    }
}

impl DmaBuf for RxBuf<'_> {
    fn dma_addr(&self) -> u64 {
        RxBuf::dma_addr(self)
    }
    fn dma_len(&self) -> u32 {
        RxBuf::len(self)
    }
}

type TxQ<'b> = TxQueue<TxMapped<'b>, KVVec<TxSlot<TxMapped<'b>>>, Desc>;
type RxQ<'b> = RxQueue<RxBuf<'b>, KVVec<Option<RxBuf<'b>>>, Desc, Desc>;
// Queues and the port live on the heap and move as pointers (open/stop hand
// them around; the probe initialisers carry them): the kernel stack is 16 KiB.
type TxBox<'b> = KBox<TxQ<'b>>;
type RxBox<'b> = KBox<RxQ<'b>>;

#[inline(never)]
fn tx_queue<'b>(dev: &'b Device<Bound>, qid: usize) -> Result<TxBox<'b>> {
    let mut slots = KVVec::with_capacity(RING_SIZE, GFP_KERNEL)?;
    for _ in 0..RING_SIZE {
        slots.push(TxSlot::default(), GFP_KERNEL)?;
    }
    Ok(KBox::new(TxQueue::new(qid, slots, Desc::new(dev)?).map_err(hal_err)?, GFP_KERNEL)?)
}

#[inline(never)]
fn rx_queue<'b>(dev: &'b Device<Bound>, qid: usize) -> Result<RxBox<'b>> {
    let mut slots = KVVec::with_capacity(RING_SIZE, GFP_KERNEL)?;
    for _ in 0..RING_SIZE {
        slots.push(None, GFP_KERNEL)?;
    }
    Ok(KBox::new(RxQueue::new(qid, slots, Desc::new(dev)?, Desc::new(dev)?).map_err(hal_err)?, GFP_KERNEL)?)
}

/// al_eth's probe-time hardware init (Port::probe), its result boxed in a
/// frame of its own so the port is never on the stack below Registration::new.
#[inline(never)]
fn probe_port<'b>(set: &Arc<BarSet<'b>>, cfg: &CfgSpace<'b>) -> Result<KBox<Port<Reg<'b>>>> {
    let port = Port::new(bars(set, Ctx::Task), REV_2).map_err(|e| {
        if e == hal::Error::NoBoardParams {
            pr_err!("{}: no board parameters (the boot loader left none)\n", NAME_STR);
        }
        hal_err(e)
    })?;
    check_mode(&port.cfg)?;
    let mut port = KBox::new(port, GFP_KERNEL)?;
    port.reset_init(&Sleep(&set.tr), cfg).map_err(|e| {
        pr_err!("{}: hardware init failed: {:?}\n", NAME_STR, e);
        hal_err(e)
    })?;
    Ok(port)
}

/// A fresh mapped RX buffer of size data bytes (al_eth_alloc_rx_frag).
fn rx_buf<'b>(dev: &'b Device<Bound>, size: u32) -> Option<RxBuf<'b>> {
    let frag = RxFrag::alloc(size, RX_OFFSET).ok()?;
    RxBuf::map(dev, frag).ok()
}

/// [rx_buf] inside a NAPI poll (the per-CPU NAPI fragment cache).
fn rx_buf_bh<'b>(dev: &'b Device<Bound>, size: u32, bh: Option<Bh<'_>>) -> Option<RxBuf<'b>> {
    let frag = match bh {
        Some(bh) => RxFrag::alloc_bh(size, RX_OFFSET, bh),
        None => RxFrag::alloc(size, RX_OFFSET),
    };
    RxBuf::map(dev, frag.ok()?).ok()
}

/// An RX queue's NAPI context: the queue, while the interface is up.
struct RxCtx<'b> {
    qid: usize,
    dev: &'b Device<Bound>,
    q: Option<RxBox<'b>>,
}

/// One RX queue's counters (64-bit, software: the MAC's are 32-bit and wrap).
/// Written once per poll by that queue's NAPI poll only, read in any context
/// (get_stats64, ethtool -S).
#[derive(Default)]
struct RxStats {
    /// packets handed to the stack, and their bytes (frame length, no FCS)
    packets: AtomicU64,
    bytes: AtomicU64,
    /// dropped by the driver (no skb, impossible layout, runt)
    dropped: AtomicU64,
    /// dropped for a hardware-flagged error (Ethernet or DMA)
    errors: AtomicU64,
    /// polls whose refill came short (buffer allocation or mapping failed)
    refill_short: AtomicU64,
    /// packets copied into a new skb (the buffer stays posted)
    copied: AtomicU64,
    /// packets handed to GRO as page fragments
    frags: AtomicU64,
}

/// One TX queue's counters: completed packets and bytes (written by that
/// queue's completion NAPI under the queue lock).
#[derive(Default)]
struct TxStats {
    packets: AtomicU64,
    bytes: AtomicU64,
}

/// The counters, on the heap (probe's initialiser stays small).
#[derive(Default)]
struct Counters {
    rx: [RxStats; NUM_QUEUES],
    tx: [TxStats; NUM_QUEUES],
}

#[inline(never)]
fn counters() -> Result<KBox<Counters>> {
    Ok(KBox::new(Counters::default(), GFP_KERNEL)?)
}

#[inline(never)]
fn mac_totals() -> Result<KBox<MacTotals>> {
    Ok(KBox::new(MacTotals::new(), GFP_KERNEL)?)
}

/// What one RX poll counted, added to its [RxStats] at the end of the poll.
#[derive(Default)]
struct PollCount {
    packets: u64,
    bytes: u64,
    dropped: u64,
    errors: u64,
    copied: u64,
    frags: u64,
}

impl RxStats {
    fn add(&self, c: &PollCount, refill_short: bool) {
        for (a, v) in [
            (&self.packets, c.packets),
            (&self.bytes, c.bytes),
            (&self.dropped, c.dropped),
            (&self.errors, c.errors),
            (&self.copied, c.copied),
            (&self.frags, c.frags),
            (&self.refill_short, u64::from(refill_short)),
        ] {
            if v != 0 {
                a.fetch_add(v, Ordering::Relaxed);
            }
        }
    }
}

/// The RX features that change what deliver does, read once per poll.
#[derive(Clone, Copy)]
struct RxFeat {
    csum: bool,
    hash: bool,
}

impl<'b> RxCtx<'b> {
    /// Hands one received packet to the stack (al_eth_rx_skb + rx_poll's tail).
    /// Small packets are copied (the buffer stays posted); the others go to GRO
    /// as page fragments of the context's frags skb, as the C does
    /// (napi_get_frags/napi_gro_frags): no skb allocated per packet that GRO merges.
    fn deliver(dev: &NetDev<Priv<'b>>, napi: &mut NapiCtx<'_>, qid: usize, f: RxFeat, c: &mut PollCount, ev: RxEvent<'_, RxBuf<'b>>) {
        match ev {
            RxEvent::Small { buf, info } => {
                let Some(mut skb) = napi.alloc_skb(info.len) else {
                    c.dropped += 1;
                    return;
                };
                // The CPU's window on the received bytes only; back to the device when the view drops.
                let view = buf.sync_for_cpu(info.len);
                match skb.put(info.len) {
                    Ok(dst) if dst.len() == view.bytes().len() => dst.copy_from_slice(view.bytes()),
                    _ => {
                        c.dropped += 1;
                        return;
                    }
                }
                drop(view);
                if skb.eth_type_trans(dev).is_err() {
                    c.dropped += 1;
                    return;
                }
                if f.csum && info.csum_ok {
                    skb.set_csum_unnecessary();
                }
                if f.hash {
                    skb.set_hash(info.hash, info.l4_hash);
                }
                skb.record_rx_queue(qid as u16);
                c.copied += 1;
                c.packets += 1;
                c.bytes += u64::from(info.len);
                skb.gro_receive(napi);
            }
            RxEvent::Frags { bufs, info } => {
                // More buffers than an skb has fragment slots cannot happen with
                // our buffer sizes (>= 600 bytes, frames <= 9238): drop such a packet
                // whole (the buffers are freed with `bufs`) before touching the
                // context's skb, so filling it cannot fail halfway.
                if info.pkt.descs > port::MAX_SKB_FRAGS {
                    c.dropped += 1;
                    return;
                }
                let Some(mut g) = napi.frags() else {
                    c.dropped += 1;
                    return;
                };
                for (b, len) in bufs {
                    if g.add_rx_frag(b.unmap(), len).is_err() {
                        // a length beyond its buffer (impossible hardware): g frees
                        // the fragments added so far, `bufs` the rest
                        c.dropped += 1;
                        return;
                    }
                }
                if f.csum && info.csum_ok {
                    g.set_csum_unnecessary();
                }
                if f.hash {
                    g.set_hash(info.hash, info.l4_hash);
                }
                g.record_rx_queue(qid as u16);
                if g.gro() {
                    c.frags += 1;
                    c.packets += 1;
                    c.bytes += u64::from(info.len);
                } else {
                    c.dropped += 1;
                }
            }
            RxEvent::Dropped { .. } => {
                c.errors += 1;
            }
        }
    }
}

impl<'b> Poll for RxCtx<'b> {
    type Dev = Priv<'b>;

    /// al_eth_rx_poll.
    fn poll(&mut self, dev: &NetDev<Priv<'b>>, napi: &mut NapiCtx<'_>, budget: u32) -> u32 {
        let p = dev.data();
        let qid = self.qid;
        let bdev = self.dev;
        if budget == 0 {
            return 0; // netpoll: TX completions only
        }
        let Some(q) = self.q.as_mut() else {
            // not up (cannot happen: enabled only with the queue in place)
            napi.complete_done(0);
            return 0;
        };
        let size = port::rx_buf_size(dev.mtu());
        let m = p.moder.load(Ordering::Relaxed);
        q.set_moderation(if m & MODER_ADAPTIVE != 0 { None } else { Some(m as u8) });
        let ft = dev.features();
        let f = RxFeat {
            csum: ft.contains(Features::bit(bindings::NETIF_F_RXCSUM_BIT)),
            hash: ft.contains(Features::bit(bindings::NETIF_F_RXHASH_BIT)),
        };
        let bh = napi.bh();
        let mut c = PollCount::default();
        let r = q.poll(&p.bars, budget, |ev| Self::deliver(dev, napi, qid, f, &mut c, ev), || rx_buf_bh(bdev, size, bh));
        p.cnt.rx[qid].add(&c, matches!(r, Ok(ref r) if r.resched));
        match r {
            // the budget used up (more may be waiting): the core polls again,
            // the refill has already run; or the refill came short and the
            // ring is nearly empty (no interrupt can come from a starved ring)
            Ok(r) if r.repoll(budget, q.posted()) => budget,
            // done (or short, with enough buffers posted for the next interrupt)
            Ok(r) => {
                if napi.complete_done(r.work) {
                    q.complete(&p.bars); // moderation step + unmask
                }
                r.work
            }
            Err(e) => {
                // A completion the hardware cannot have written: leave this queue's
                // interrupt masked (the queue stops) rather than guess.
                pr_err!("{}: rx queue {}: {:?}, queue stopped\n", NAME_STR, qid, e);
                napi.complete_done(0);
                0
            }
        }
    }
}

/// A TX queue's completion NAPI context (its queue lives in [Priv::txq]).
struct TxCtx<'b> {
    qid: u32,
    _b: core::marker::PhantomData<&'b ()>,
}

impl<'b> Poll for TxCtx<'b> {
    type Dev = Priv<'b>;

    /// al_eth_tx_poll.
    fn poll(&mut self, dev: &NetDev<Priv<'b>>, napi: &mut NapiCtx<'_>, budget: u32) -> u32 {
        let p = dev.data();
        let i = self.qid as usize;
        let Some(mut g) = napi.tx_lock(dev, self.qid) else { return 0 };
        let c = match p.txq[i].get(&mut g).and_then(|q| q.as_mut()) {
            Some(q) => q.clean(&p.bars, |b, _len| napi.consume_skb(b.unmap())),
            None => return 0,
        };
        g.completed(c.pkts, c.bytes);
        if c.pkts != 0 {
            let s = &p.cnt.tx[i];
            s.packets.fetch_add(u64::from(c.pkts), Ordering::Relaxed);
            s.bytes.fetch_add(u64::from(c.bytes), Ordering::Relaxed);
        }
        if g.queue().stopped() && c.available > TX_STOP_THRESH {
            g.queue().wake();
        }
        if budget == 0 {
            return 0; // netpoll: completions only
        }
        if napi.complete_done(0) {
            if let Some(q) = p.txq[i].get(&mut g).and_then(|q| q.as_ref()) {
                q.unmask(&p.bars);
            }
        }
        0
    }
}

/// The management interrupt (al_eth_intr_msix_mgmt): UDMA error/hint
/// aggregates. Reports the causes (clear-on-read), as the C does.
struct Mgmt<'b> {
    udma: Reg<'b>,
    count: AtomicU32,
}

impl irq::Handler for Mgmt<'_> {
    fn handle(&self) -> IrqReturn {
        let u = &self.udma;
        let d = iofic::UDMA_PRIMARY.read_cause(u, Group::D);
        let a = iofic::UDMA_SECONDARY.read_cause(u, Group::A);
        let b = iofic::UDMA_SECONDARY.read_cause(u, Group::B);
        if self.count.fetch_add(1, Ordering::Relaxed) < 20 {
            pr_err!("{}: management interrupt: primary D {:#x}, M2S errors {:#x}, S2M errors {:#x}\n", NAME_STR, d, a, b);
        }
        IrqReturn::Handled
    }
}

/// What set_rx_mode last asked for, programmed while the port is up.
struct RxMode {
    up: bool,
    promisc: bool,
    allmulti: bool,
    mc: usize,
    uc: [[u8; 6]; port::UC_MAX + 1],
    uc_n: usize,
}

impl RxMode {
    fn apply(&self, b: &Bars<Reg<'_>>) {
        if self.up {
            port::rx_mode(b, self.promisc, self.allmulti, self.mc, &self.uc[..self.uc_n]);
        }
    }
}

/// The net_device's private data.
#[pin_data]
struct Priv<'b> {
    // NAPI contexts with their interrupts first: dropped first (each frees its
    // IRQ, then deletes its context), after unregister_netdev ran stop.
    #[pin]
    rx0: NapiIrq<'b, RxCtx<'b>>,
    #[pin]
    rx1: NapiIrq<'b, RxCtx<'b>>,
    #[pin]
    rx2: NapiIrq<'b, RxCtx<'b>>,
    #[pin]
    rx3: NapiIrq<'b, RxCtx<'b>>,
    #[pin]
    tx0: NapiIrq<'b, TxCtx<'b>>,
    #[pin]
    tx1: NapiIrq<'b, TxCtx<'b>>,
    #[pin]
    tx2: NapiIrq<'b, TxCtx<'b>>,
    #[pin]
    tx3: NapiIrq<'b, TxCtx<'b>>,
    #[pin]
    mgmt: irq::Registration<'b, Mgmt<'b>>,
    /// Control state (probe/open/stop/MTU/MAC: RTNL, may sleep).
    #[pin]
    port: Mutex<KBox<Port<Reg<'b>>>>,
    /// The MAC filter set_rx_mode asked for (atomic context, also softirq).
    #[pin]
    rxmode: SpinLockIrq<RxMode>,
    txq: [TxLocked<Option<TxBox<'b>>>; NUM_QUEUES],
    /// Datapath windows (NAPI, xmit, rx mode, management IRQ).
    bars: Bars<Reg<'b>>,
    /// The MAC window for get_stats64 (process context).
    stats_mac: Reg<'b>,
    mac_mode: MacMode,
    cfg: CfgSpace<'b>,
    dev: &'b Device<Bound>,
    /// Software counters per queue.
    cnt: KBox<Counters>,
    /// The MAC counters as 64-bit totals (live from open to stop).
    #[pin]
    mac_tot: SpinLockIrq<KBox<MacTotals>>,
    tx_dropped: AtomicU64,
    tx_timeouts: AtomicU64,
    /// IRQ numbers: management, RX queues 0-3, TX queues 0-3.
    irqs: [u32; 1 + 2 * NUM_QUEUES],
    /// RX interrupt moderation for the RX polls: MODER_ADAPTIVE (ethtool
    /// adaptive-rx on, the default, as the C), else the fixed interval
    /// (register units) set with ethtool -C.
    moder: AtomicU32,
    /// Intervals set with ethtool -C (µs; u32::MAX: never set), re-applied at
    /// open (Port::up programs the C's defaults).
    want_tx_usecs: AtomicU32,
    want_rx_usecs: AtomicU32,
}

impl<'b> Priv<'b> {
    /// al_eth_setup_int_mode's affinity: RX and TX queue i on the i-th online
    /// CPU, management on the first (the C sets these as hints; set directly
    /// here, so free_irq has no hint to complain about). Failures are ignored.
    fn spread_irqs(&self) {
        let Ok(mut mask) = kernel::cpumask::CpumaskVar::new_zero(GFP_KERNEL) else { return };
        for (n, &irq) in self.irqs.iter().enumerate() {
            let i = if n == 0 { 0 } else { (n - 1) % NUM_QUEUES } as u32;
            // SAFETY: no preconditions (returns an online CPU number).
            let cpu = unsafe { bindings::cpumask_local_spread(i, -1) };
            let Some(id) = kernel::cpu::CpuId::from_u32(cpu) else { continue };
            for c in 0..kernel::cpu::nr_cpu_ids() {
                if let Some(c) = kernel::cpu::CpuId::from_u32(c) {
                    mask.clear(c);
                }
            }
            mask.set(id);
            // SAFETY: `irq` is one of this device's requested vectors (the IRQ
            // registrations live as long as `self`), `mask` a valid cpumask for
            // the call; irq_set_affinity copies it and checks the IRQ.
            let _ = unsafe { bindings::irq_set_affinity(irq, mask.as_raw()) };
        }
    }

    fn tr(&self) -> &trace::Handle {
        &self.bars.udma.set.tr
    }

    /// The MAC counters' 64-bit totals, folded with a fresh reading while up.
    /// Any context (get_stats64 can be called in atomic context).
    #[inline(never)]
    fn mac_stats(&self) -> MacStats {
        let mut t = self.mac_tot.lock();
        if t.live() {
            t.fold(&mac::stats(&self.stats_mac, self.mac_mode), self.mac_mode);
        }
        t.totals()
    }

    /// The MAC counters from here on (open, after Port::up: whatever up reset them to).
    #[inline(never)]
    fn mac_start(&self) {
        let raw = mac::stats(&self.stats_mac, self.mac_mode);
        self.mac_tot.lock().start(&raw);
    }

    /// The MAC counters' last reading (stop, before Port::down).
    #[inline(never)]
    fn mac_stop(&self) {
        let mut t = self.mac_tot.lock();
        t.stop(&mac::stats(&self.stats_mac, self.mac_mode), self.mac_mode);
    }

    /// Sum of one counter over the RX queues.
    fn rx_sum(&self, f: impl Fn(&RxStats) -> &AtomicU64) -> u64 {
        self.cnt.rx.iter().map(|s| f(s).load(Ordering::Relaxed)).fold(0, u64::wrapping_add)
    }
    fn tx_sum(&self, f: impl Fn(&TxStats) -> &AtomicU64) -> u64 {
        self.cnt.tx.iter().map(|s| f(s).load(Ordering::Relaxed)).fold(0, u64::wrapping_add)
    }
    fn rx(&self, i: usize) -> &NapiIrq<'b, RxCtx<'b>> {
        match i {
            0 => &self.rx0,
            1 => &self.rx1,
            2 => &self.rx2,
            _ => &self.rx3,
        }
    }
    fn tx(&self, i: usize) -> &NapiIrq<'b, TxCtx<'b>> {
        match i {
            0 => &self.tx0,
            1 => &self.tx1,
            2 => &self.tx2,
            _ => &self.tx3,
        }
    }

    /// The four RX contexts' state, all disabled (disable: from enabled,
    /// waiting for a running poll; else parked ones).
    fn rx_off(&self, disable: bool) -> Option<[NapiOff<'_, RxCtx<'b>>; NUM_QUEUES]> {
        let g = |i| if disable { self.rx(i).napi().disable() } else { self.rx(i).napi().parked() };
        let [Some(a), Some(b), Some(c), Some(d)] = [g(0), g(1), g(2), g(3)] else { return None };
        Some([a, b, c, d])
    }
    fn tx_off(&self, disable: bool) -> Option<[NapiOff<'_, TxCtx<'b>>; NUM_QUEUES]> {
        let g = |i| if disable { self.tx(i).napi().disable() } else { self.tx(i).napi().parked() };
        let [Some(a), Some(b), Some(c), Some(d)] = [g(0), g(1), g(2), g(3)] else { return None };
        Some([a, b, c, d])
    }

    /// Takes the TX queues out of their locks (the netdev queues are stopped:
    /// start_xmit does not run, or finds none and drops).
    fn take_tx(&self, dev: &NetDev<Self>) -> Option<[TxBox<'b>; NUM_QUEUES]> {
        let t = |i: usize| -> Option<TxBox<'b>> {
            let mut g = dev.tx_lock_bh(i as u32)?;
            self.txq[i].get(&mut g)?.take()
        };
        let [Some(a), Some(b), Some(c), Some(d)] = [t(0), t(1), t(2), t(3)] else {
            // A queue missing is a bug (they are always put back); the ones taken
            // are dropped here, with empty rings.
            return None;
        };
        Some([a, b, c, d])
    }
    fn put_tx(&self, dev: &NetDev<Self>, qs: [TxBox<'b>; NUM_QUEUES]) {
        for (i, q) in qs.into_iter().enumerate() {
            if let Some(mut g) = dev.tx_lock_bh(i as u32) {
                if let Some(s) = self.txq[i].get(&mut g) {
                    *s = Some(q);
                }
                g.reset(); // BQL: nothing in flight
            }
        }
    }
}

/// The Toeplitz key (get_random_bytes in al_eth_config_rx_fwd).
fn random_key() -> [u32; 10] {
    // SAFETY: get_random_u32 has no preconditions and may be called in any context.
    core::array::from_fn(|_| unsafe { bindings::get_random_u32() })
}

/// Stops the hardware and frees every buffer (al_eth_down), with the queues
/// out of their homes and every NAPI context disabled. Returns the TX and RX
/// buffers that were still in the rings.
fn down<'b>(tr: &trace::Handle, port: &mut Port<Reg<'b>>, tx: &mut [TxBox<'b>; NUM_QUEUES], rx: &mut [RxBox<'b>; NUM_QUEUES]) -> (u32, u32) {
    let freed = port.down(&Sleep(tr), tx.each_mut().map(|q| &mut **q), rx.each_mut().map(|q| &mut **q), drop, drop);
    freed.iter().fold((0, 0), |(t, r), (a, b)| (t + a, r + b))
}

#[vtable]
impl<'b> NetOps for Priv<'b> {
    /// al_eth_open (fixed link, no PHY): al_eth_up, then carrier on.
    fn open(dev: &NetDev<Self>) -> Result {
        let p = dev.data();
        p.tr().mark(Mark::Open, Ctx::Task);
        let mut port = p.port.lock();
        if port.is_up() {
            return Err(EBUSY);
        }
        let mut rxg = p.rx_off(false).ok_or(EBUSY)?;
        let txg = p.tx_off(false).ok_or(EBUSY)?;
        let taken = [rxg[0].q.take(), rxg[1].q.take(), rxg[2].q.take(), rxg[3].q.take()];
        let [Some(a), Some(b), Some(c), Some(d)] = taken else {
            pr_err!("{}: open: an rx queue is missing\n", NAME_STR);
            return Err(EBUSY);
        };
        let mut rx = [a, b, c, d];
        let Some(mut tx) = p.take_tx(dev) else {
            for (g, q) in rxg.iter_mut().zip(rx) {
                g.q = Some(q);
            }
            pr_err!("{}: open: a tx queue is missing\n", NAME_STR);
            return Err(EBUSY);
        };
        // Fresh zeroed rings (al_eth_setup_*_resources); the old ones are freed here,
        // with the engine stopped since the last down (or never started).
        let mut r = (|| -> Result {
            for q in tx.iter_mut() {
                drop(q.replace_mem(Desc::new(p.dev)?).map_err(|_| EBUSY)?);
            }
            for q in rx.iter_mut() {
                drop(q.replace_mem(Desc::new(p.dev)?, Desc::new(p.dev)?).map_err(|_| EBUSY)?);
            }
            Ok(())
        })();
        if r.is_ok() {
            let a = dev.mac_addr().0;
            if a != port.mac_addr() {
                port.set_mac_addr(a);
            }
            if port.mtu() != dev.mtu() {
                r = port.change_mtu(dev.mtu()).map_err(hal_err);
            }
        }
        if r.is_ok() {
            let size = port::rx_buf_size(dev.mtu());
            let (t, x) = (tx.each_mut().map(|q| &mut **q), rx.each_mut().map(|q| &mut **q));
            r = port.up(&Sleep(p.tr()), &p.cfg, &random_key(), t, x, |_| rx_buf(p.dev, size)).map_err(hal_err);
            if r.is_err() {
                // Whatever up got to: stop the engine before any buffer can go.
                down(p.tr(), &mut port, &mut tx, &mut rx);
            }
        }
        for (g, q) in rxg.iter_mut().zip(rx) {
            g.q = Some(q);
        }
        p.put_tx(dev, tx);
        if let Err(e) = r {
            pr_err!("{}: open failed: {:?}\n", NAME_STR, e);
            return Err(e);
        }
        for g in rxg {
            g.enable();
        }
        for g in txg {
            g.enable();
        }
        // An interrupt between the unmask (in up) and the enables went nowhere:
        // one poll of each queue handles what came and unmasks it.
        for i in 0..NUM_QUEUES {
            p.rx(i).napi().schedule();
            p.tx(i).napi().schedule();
        }
        p.spread_irqs();
        // ethtool -C settings survive down/up (the C's open puts its defaults back).
        let (tx, rx) = (p.want_tx_usecs.load(Ordering::Relaxed), p.want_rx_usecs.load(Ordering::Relaxed));
        if tx != u32::MAX && rx != u32::MAX {
            let _ = port.apply_coalesce(tx, rx);
        }
        p.mac_start();
        // The filters themselves: the core calls ndo_set_rx_mode right after
        // ndo_open (as for the C, whose open writes none of them).
        p.rxmode.lock().up = true;
        drop(port);
        dev.tx_start_all();
        dev.carrier_on();
        Ok(())
    }

    /// al_eth_close: al_eth_down.
    fn stop(dev: &NetDev<Self>) -> Result {
        let p = dev.data();
        p.tr().mark(Mark::Close, Ctx::Task);
        dev.carrier_off();
        dev.tx_disable();
        p.rxmode.lock().up = false;
        let mut port = p.port.lock();
        if !port.is_up() {
            return Ok(());
        }
        p.mac_stop();
        // Disable waits for running polls.
        let (Some(mut rxg), Some(txg)) = (p.rx_off(true), p.tx_off(true)) else {
            pr_err!("{}: stop: NAPI state unexpected, hardware left running\n", NAME_STR);
            return Err(EBUSY);
        };
        let taken = [rxg[0].q.take(), rxg[1].q.take(), rxg[2].q.take(), rxg[3].q.take()];
        let ([Some(a), Some(b), Some(c), Some(d)], Some(mut tx)) = (taken, p.take_tx(dev)) else {
            pr_err!("{}: stop: a queue is missing, hardware left running\n", NAME_STR);
            return Err(EBUSY);
        };
        let mut rx = [a, b, c, d];
        let (t, _r) = down(p.tr(), &mut port, &mut tx, &mut rx);
        p.tx_dropped.fetch_add(u64::from(t), Ordering::Relaxed);
        for (g, q) in rxg.iter_mut().zip(rx) {
            g.q = Some(q);
        }
        p.put_tx(dev, tx);
        drop(txg); // parked, for the next open
        Ok(())
    }

    /// al_eth_start_xmit.
    fn start_xmit(dev: &NetDev<Self>, mut skb: SkBuff, txq: &mut TxGuard<'_>) -> Xmit {
        let p = dev.data();
        let qi = txq.queue().index() as usize;
        // eth_skb_pad: the MAC pads short frames, but not with zeros. A short
        // frame with paged data or no tailroom goes as it is (the C reallocates).
        if skb.len() < ETH_ZLEN && skb.headlen() == skb.len() {
            if let Ok(pad) = skb.put(ETH_ZLEN - skb.len()) {
                pad.fill(0);
            }
        }
        let info = tx_info(&skb);
        let Some(q) = p.txq.get(qi).and_then(|l| l.get(txq)).and_then(|q| q.as_mut()) else {
            p.tx_dropped.fetch_add(1, Ordering::Relaxed);
            return Xmit::Ok;
        };
        let Ok(o) = q.offload(&info) else {
            // a TSO MSS the hardware cannot do (the C drops it too)
            p.tx_dropped.fetch_add(1, Ordering::Relaxed);
            return Xmit::Ok;
        };
        skb.tx_timestamp();
        let len = skb.len();
        let m = match TxMapped::map(p.dev, skb) {
            Ok(m) => m,
            Err((_, skb)) => {
                drop(skb);
                p.tx_dropped.fetch_add(1, Ordering::Relaxed);
                return Xmit::Ok;
            }
        };
        // The mapping goes straight into its ring slot; a rejected one comes
        // back unmapped, as the skb (no room: the stack retries after a wake).
        let mut back: Option<SkBuff> = None;
        match q.xmit_with(&p.bars, &o, len, m, |m| m.segs(), |m| back = Some(m.unmap())) {
            Some(r) => {
                txq.sent(len);
                if r.stop {
                    txq.queue().stop();
                }
                Xmit::Ok
            }
            None => {
                txq.queue().stop();
                match back {
                    Some(skb) => Xmit::Busy(skb),
                    None => Xmit::Ok,
                }
            }
        }
    }

    /// al_eth_get_stats64, but monotonic: packets and bytes are the driver's
    /// 64-bit counters (the C reports the MAC's 32-bit ones, which wrap: rx_bytes
    /// fell on the DMC-12 every 4 GiB), the error counters the MAC's as 64-bit
    /// totals ([MacTotals]); all of them kept across down/up.
    fn get_stats64(dev: &NetDev<Self>, s: &mut Stats64) {
        let p = dev.data();
        p.tr().mark(Mark::Stats, Ctx::Task);
        s.rx_packets = p.rx_sum(|r| &r.packets);
        s.rx_bytes = p.rx_sum(|r| &r.bytes);
        s.tx_packets = p.tx_sum(|t| &t.packets);
        s.tx_bytes = p.tx_sum(|t| &t.bytes);
        s.rx_dropped = p.rx_sum(|r| &r.dropped);
        s.tx_dropped = p.tx_dropped.load(Ordering::Relaxed);
        let m = p.mac_stats();
        s.multicast = m.if_in_multicast_pkts;
        s.rx_length_errors = m.undersize_pkts + m.fragments + m.jabbers + m.oversize_pkts;
        s.rx_crc_errors = m.fcs_errors;
        s.rx_frame_errors = m.alignment_errors;
        s.rx_fifo_errors = m.drop_events;
        s.rx_errors = m.if_in_errors;
        s.tx_errors = m.if_out_errors;
    }

    /// al_eth_set_rx_mode.
    fn set_rx_mode(dev: &NetDev<Self>, addrs: &AddrLists<'_>) {
        let p = dev.data();
        p.tr().mark(Mark::RxMode, Ctx::Atomic);
        let f = dev.flags();
        let mut mc = 0usize;
        addrs.for_each_mc(|_| mc += 1);
        let mut uc = [[0u8; 6]; port::UC_MAX + 1];
        let mut uc_n = 0usize;
        addrs.for_each_uc(|a: MacAddr| {
            if let Some(s) = uc.get_mut(uc_n) {
                *s = a.0;
                uc_n += 1;
            }
        });
        let mut m = p.rxmode.lock();
        m.promisc = f & bindings::net_device_flags_IFF_PROMISC != 0;
        m.allmulti = f & bindings::net_device_flags_IFF_ALLMULTI != 0;
        m.mc = mc;
        m.uc = uc;
        m.uc_n = uc_n;
        m.apply(&p.bars);
    }

    /// al_eth_change_mtu (68..=9216); RX buffers follow on refill.
    fn change_mtu(dev: &NetDev<Self>, new_mtu: u32, _old: u32) -> Result {
        dev.data().tr().mark(Mark::Mtu, Ctx::Task);
        dev.data().port.lock().change_mtu(new_mtu).map_err(hal_err)
    }

    /// al_eth_set_mac_addr.
    fn set_mac_address(dev: &NetDev<Self>, addr: MacAddr) -> Result {
        dev.data().tr().mark(Mark::SetMac, Ctx::Task);
        dev.data().port.lock().set_mac_addr(addr.0);
        Ok(())
    }

    /// al_eth_tx_timeout. The C schedules a reset task (down + up under RTNL);
    /// not ported: counted and logged.
    fn tx_timeout(dev: &NetDev<Self>, queue: u32) {
        let n = dev.data().tx_timeouts.fetch_add(1, Ordering::Relaxed);
        if n < 10 {
            pr_err!("{}: transmit timed out on queue {}\n", NAME_STR, queue);
        }
    }

    /// al_set_features: nothing to program (RXCSUM/RXHASH are read per packet).
    fn set_features(_dev: &NetDev<Self>, _f: Features) -> Result {
        Ok(())
    }

    /// al_eth_min_get_drvinfo (no bus_info: no device name accessor in the kernel crate).
    fn get_drvinfo(_dev: &NetDev<Self>, info: &mut DrvInfo<'_>) {
        info.set_driver(NAME_STR);
        info.set_version("0.2-rust");
    }

    /// al_eth_min_get_link_ksettings without a PHY: fixed 1000/full.
    fn get_link_ksettings(dev: &NetDev<Self>, ks: &mut LinkKsettings<'_>) -> Result {
        ks.set_port(EthPort::Other);
        ks.set_autoneg(false);
        if dev.carrier_ok() {
            ks.set_speed_duplex(1000, true);
        } else {
            ks.set_speed_duplex(u32::MAX, false); // SPEED_UNKNOWN
        }
        Ok(())
    }

    /// ethtool -c: the programmed intervals (rounded to the hardware's
    /// resolution) and adaptive RX.
    fn get_coalesce(dev: &NetDev<Self>, c: &mut Coalesce) -> Result {
        let p = dev.data();
        let (tx, rx) = p.port.lock().coalesce();
        c.tx_usecs = tx;
        c.rx_usecs = rx;
        c.adaptive_rx = p.moder.load(Ordering::Relaxed) & MODER_ADAPTIVE != 0;
        Ok(())
    }

    /// ethtool -C [rx-usecs N] [tx-usecs N] [adaptive-rx on|off] (the core
    /// merges unchanged values from get_coalesce). tx-usecs: the TX completion
    /// interval (the C's 196 us by default). rx-usecs: the RX interval while
    /// adaptive-rx is off; with it on, the RX interval follows the load (the C's
    /// table: 0 at low load, up to ~190 us) and rx-usecs only seeds it. Both
    /// 0..=max_coalesce_usecs (255 hardware units); kept across down/up.
    fn set_coalesce(dev: &NetDev<Self>, c: &Coalesce) -> Result {
        let p = dev.data();
        let mut port = p.port.lock();
        let max = port.max_coalesce_usecs();
        if c.tx_usecs > max || c.rx_usecs > max {
            return Err(EINVAL);
        }
        let (_, ri) = port.apply_coalesce(c.tx_usecs, c.rx_usecs).map_err(hal_err)?;
        // The RX polls pick this up at their next completion; one that raced
        // with this wrote an adaptive interval, which the next one corrects
        // (RxQueue::set_moderation).
        p.moder.store(if c.adaptive_rx { MODER_ADAPTIVE } else { u32::from(ri) }, Ordering::Relaxed);
        p.want_tx_usecs.store(c.tx_usecs, Ordering::Relaxed);
        p.want_rx_usecs.store(c.rx_usecs, Ordering::Relaxed);
        Ok(())
    }

    /// ethtool -g (fixed rings).
    fn get_ringparam(_dev: &NetDev<Self>, r: &mut Rings) {
        *r = Rings { rx_max: RING_SIZE as u32, rx: RING_SIZE as u32, tx_max: RING_SIZE as u32, tx: RING_SIZE as u32 };
    }

    fn stats_names(_dev: &NetDev<Self>) -> &'static [&'static str] {
        &STATS_NAMES
    }

    /// ethtool -S: driver counters, hardware RX-path registers, the MAC totals
    /// (written straight into `out`, in STATS_NAMES order).
    fn get_stats(dev: &NetDev<Self>, out: &mut [u64]) {
        let p = dev.data();
        let ld = |a: &AtomicU64| a.load(Ordering::Relaxed);
        let mut w = out.iter_mut();
        let mut put = |v: u64| {
            if let Some(o) = w.next() {
                *o = v;
            }
        };
        put(p.rx_sum(|r| &r.dropped));
        put(p.rx_sum(|r| &r.errors));
        put(p.rx_sum(|r| &r.refill_short));
        put(p.rx_sum(|r| &r.copied));
        put(p.rx_sum(|r| &r.frags));
        put(ld(&p.tx_dropped));
        put(ld(&p.tx_timeouts));
        p.cnt.rx.iter().for_each(|s| put(ld(&s.packets)));
        p.cnt.rx.iter().for_each(|s| put(ld(&s.bytes)));
        p.cnt.tx.iter().for_each(|s| put(ld(&s.packets)));
        p.cnt.tx.iter().for_each(|s| put(ld(&s.bytes)));
        let u = &p.bars.udma;
        put(u64::from(u.r32(hal::regs::off::U_S2M_STAT_DROP_PKT)));
        let qr = [0, 1, 2, 3].map(|q| port::rx_queue_regs(u, q));
        qr.iter().for_each(|r| put(u64::from(r.rx_pkt)));
        qr.iter().for_each(|r| put(u64::from(r.head)));
        qr.iter().for_each(|r| put(u64::from(r.tail)));
        for o in port::EC_RX_STATS {
            put(u64::from(p.bars.ec.r32(o)));
        }
        let m = p.mac_stats();
        for v in [
            m.if_in_ucast_pkts, m.if_in_multicast_pkts, m.if_in_broadcast_pkts, m.ether_stats_pkts,
            m.if_out_ucast_pkts, m.if_out_multicast_pkts, m.if_out_broadcast_pkts, m.if_in_errors, m.if_out_errors,
            m.frames_received_ok, m.frames_transmitted_ok, m.octets_received_ok, m.octets_transmitted_ok,
            m.undersize_pkts, m.fragments, m.jabbers, m.oversize_pkts, m.fcs_errors, m.alignment_errors, m.drop_events,
            m.pause_tx, m.pause_rx, m.octets, m.pkts_64, m.pkts_65_127, m.pkts_128_255, m.pkts_256_511,
            m.pkts_512_1023, m.pkts_1024_1518, m.pkts_1519_max, m.eee_in, m.eee_out,
        ] {
            put(v);
        }
    }
}

/// What al_eth_tx_csum reads from the skb's headers, from its linear part (an
/// offset outside it reads 0).
fn tx_info(skb: &SkBuff) -> TxInfo {
    let m = skb.tx_meta();
    let h = skb.head();
    let at = |o: u32| h.get(o as usize).copied().unwrap_or(0);
    let nh = m.network_offset;
    let ip_version = at(nh) >> 4;
    let l4proto = match ip_version {
        4 => at(nh.saturating_add(9)),
        6 => at(nh.saturating_add(6)),
        _ => 0,
    };
    TxInfo {
        len: m.len,
        partial: m.csum_partial,
        gso_size: u32::from(m.gso_size),
        ip_version,
        l4proto,
        nh_off: nh,
        nh_len: m.transport_offset.wrapping_sub(nh),
        transport_off: m.transport_offset,
        doff: at(m.transport_offset.saturating_add(12)) >> 4,
        vlan: m.protocol == ETH_P_8021Q,
    }
}

/// The driver's data: the net device, then the interrupt vectors its IRQs use.
#[pin_data]
struct Data<'b> {
    // First: dropped first (the IRQs inside must go before the vectors).
    netdev: Registration<Priv<'b>>,
    vectors: pci::IrqVectorRegistration<'b>,
    /// The MMIO trace's debugfs file (nothing unless built with al_eth_trace).
    _trace: trace::Files,
}

struct AlEth;

/// The board parameters must say the DMC-12's mode: RGMII without a PHY.
/// Anything else (SGMII + PHY, 10G serial: the CS8210) is the C driver's.
/// Checked on Port::new's decode, before any hardware write (and without
/// register reads the C does not make).
fn check_mode(c: &BoardCfg) -> Result {
    if c.mac_mode == MacMode::Rgmii && !c.phy_exist {
        return Ok(());
    }
    pr_info!("{}: board mode {:?} (PHY {}): not the RGMII fixed-link mode this driver runs\n", NAME_STR, c.mac_mode, c.phy_exist);
    Err(ENODEV)
}


/// What probe hands to the private data's initialiser.
struct Setup<'b> {
    bdev: &'b Device<Bound>,
    vectors: &'b pci::IrqVectorRegistration<'b>,
    port: KBox<Port<Reg<'b>>>,
    mac_addr: MacAddr,
    set: Arc<BarSet<'b>>,
    mac_mode: MacMode,
    cfg: CfgSpace<'b>,
}

/// The device setup and the initialiser of its private data (al_eth_probe's
/// netdev part). A function of its own, not inlined, so its working values
/// are not on the stack while Registration::new allocates and registers.
#[inline(never)]
fn priv_init<'b>(u: &Unregistered<Priv<'b>>, s: Setup<'b>) -> Result<impl PinInit<Priv<'b>, Error> + 'b> {
    let Setup { bdev, vectors, port, mac_addr, set, mac_mode, cfg } = s;
    let b = bars(&set, Ctx::Atomic);
    let stats_mac = Reg { set, which: Which::Mac, ctx: Ctx::Task };
    let irq = |i: usize| -> Result<irq::IrqRequest<'b>> { Ok(vectors.index(i)?.into()) };
    let mut irqs = [0u32; 1 + 2 * NUM_QUEUES];
    for (n, v) in irqs.iter_mut().enumerate() {
        let i = if n == 0 { MSIX_MGMT } else if n <= NUM_QUEUES { MSIX_RX + n - 1 } else { MSIX_TX + n - 1 - NUM_QUEUES };
        *v = irq(i)?.irq();
    }
    if u.set_mac_addr(mac_addr).is_err() {
        u.set_random_mac_addr(); // open programs it into the MAC table
    }
    let bit = Features::bit;
    let f = bit(bindings::NETIF_F_SG_BIT)
        .with(bit(bindings::NETIF_F_IP_CSUM_BIT))
        .with(bit(bindings::NETIF_F_IPV6_CSUM_BIT))
        .with(bit(bindings::NETIF_F_TSO_BIT))
        .with(bit(bindings::NETIF_F_TSO_ECN_BIT))
        .with(bit(bindings::NETIF_F_TSO6_BIT))
        .with(bit(bindings::NETIF_F_RXCSUM_BIT))
        .with(bit(bindings::NETIF_F_RXHASH_BIT))
        .with(bit(bindings::NETIF_F_HIGHDMA_BIT));
    u.set_features(f, f, f);
    u.set_mtu(1500, port::MIN_MTU, port::MAX_MTU)?;
    u.set_watchdog_timeo(kernel::time::msecs_to_jiffies(TX_TIMEOUT_MS) as i32);
    let rxc = |i: usize| -> Result<RxCtx<'b>> { Ok(RxCtx { qid: i, dev: bdev, q: Some(rx_queue(bdev, i)?) }) };
    let txc = |i: usize| TxCtx { qid: i as u32, _b: core::marker::PhantomData };
    let txl = |i: usize| -> Result<TxLocked<Option<TxBox<'b>>>> { TxLocked::new(u, i as u32, Some(tx_queue(bdev, i)?)) };
    // SAFETY (each NapiIrq::new): placed in the private data of u's
    // Registration, which drops it before freeing the device; never forgotten.
    let rx0 = unsafe { NapiIrq::new(u, Kind::Rx, rxc(0)?, irq(MSIX_RX)?, RX_IRQ_NAMES[0]) };
    // SAFETY: as rx0.
    let rx1 = unsafe { NapiIrq::new(u, Kind::Rx, rxc(1)?, irq(MSIX_RX + 1)?, RX_IRQ_NAMES[1]) };
    // SAFETY: as rx0.
    let rx2 = unsafe { NapiIrq::new(u, Kind::Rx, rxc(2)?, irq(MSIX_RX + 2)?, RX_IRQ_NAMES[2]) };
    // SAFETY: as rx0.
    let rx3 = unsafe { NapiIrq::new(u, Kind::Rx, rxc(3)?, irq(MSIX_RX + 3)?, RX_IRQ_NAMES[3]) };
    // SAFETY: as rx0.
    let tx0 = unsafe { NapiIrq::new(u, Kind::Tx, txc(0), irq(MSIX_TX)?, TX_IRQ_NAMES[0]) };
    // SAFETY: as rx0.
    let tx1 = unsafe { NapiIrq::new(u, Kind::Tx, txc(1), irq(MSIX_TX + 1)?, TX_IRQ_NAMES[1]) };
    // SAFETY: as rx0.
    let tx2 = unsafe { NapiIrq::new(u, Kind::Tx, txc(2), irq(MSIX_TX + 2)?, TX_IRQ_NAMES[2]) };
    // SAFETY: as rx0.
    let tx3 = unsafe { NapiIrq::new(u, Kind::Tx, txc(3), irq(MSIX_TX + 3)?, TX_IRQ_NAMES[3]) };
    let mgmt_h = Mgmt { udma: b.udma.clone(), count: AtomicU32::new(0) };
    // SAFETY: a field of the pinned private data, dropped (never
    // forgotten) with it, before the vectors it was resolved from.
    let mgmt = unsafe {
        irq::Registration::new(irq(MSIX_MGMT)?, irq::Flags::TRIGGER_NONE, c"al-eth-msix-mgmt", Ok::<Mgmt<'b>, Error>(mgmt_h))
    };
    let txq = [txl(0)?, txl(1)?, txl(2)?, txl(3)?];
    let cnt = counters()?;
    let mac_tot = mac_totals()?;
    let rxmode = RxMode { up: false, promisc: false, allmulti: false, mc: 0, uc: [[0; 6]; port::UC_MAX + 1], uc_n: 0 };
    Ok(try_pin_init!(Priv {
        rx0 <- rx0,
        rx1 <- rx1,
        rx2 <- rx2,
        rx3 <- rx3,
        tx0 <- tx0,
        tx1 <- tx1,
        tx2 <- tx2,
        tx3 <- tx3,
        mgmt <- mgmt,
        port <- new_mutex!(port),
        rxmode <- new_spinlock_irq!(rxmode),
        txq,
        bars: b,
        stats_mac,
        mac_mode,
        cfg,
        dev: bdev,
        cnt,
        mac_tot <- new_spinlock_irq!(mac_tot),
        tx_dropped: AtomicU64::new(0),
        tx_timeouts: AtomicU64::new(0),
        irqs,
        moder: AtomicU32::new(MODER_ADAPTIVE),
        want_tx_usecs: AtomicU32::new(u32::MAX),
        want_rx_usecs: AtomicU32::new(u32::MAX),
    }? Error))
}

impl pci::Driver for AlEth {
    type IdInfo = ();
    type Data<'b> = Data<'b>;
    const ID_TABLE: pci::IdTable<Self::IdInfo> = &PCI_TABLE;

    /// al_eth_remove's mark (teardown itself is Data's drop order).
    fn unbind<'b>(_dev: &'b pci::Device<Core<'_>>, this: Pin<&Data<'b>>) {
        this.netdev.dev().data().tr().mark(Mark::Remove, Ctx::Task);
    }

    /// al_eth_probe.
    fn probe<'b>(pdev: &'b pci::Device<Core<'_>>, _info: Option<&'b ()>) -> impl PinInit<Data<'b>, Error> + 'b {
        let r = (|| -> Result<_> {
            if pdev.vendor_id().as_raw() != VENDOR_ANNAPURNA || pdev.device_id() != DEVICE_ETH {
                return Err(ENODEV);
            }
            let tr = trace::Handle::new()?;
            let files = tr.files()?;
            tr.mark(Mark::Probe, Ctx::Task);
            if pdev.revision_id() != REV_2 {
                pr_info!("{}: PCI revision {}: only revision 2 (Alpine V2) is supported\n", NAME_STR, pdev.revision_id());
                return Err(ENODEV);
            }
            if pdev.cfg_size() != ConfigSpaceSize::Extended {
                pr_err!("{}: no extended config space (the FLR needs it)\n", NAME_STR);
                return Err(ENODEV);
            }
            pdev.enable_device_mem()?;
            pdev.set_master();
            // SAFETY: probe, before any DMA allocation or mapping for this device.
            unsafe { pdev.dma_set_mask_and_coherent(DmaMask::new::<40>())? };
            let vectors = pdev.alloc_irq_vectors(MSIX_VECS, MSIX_VECS, pci::IrqTypes::default().with(pci::IrqType::MsiX))?;
            Ok((vectors, tr, files))
        })();
        let (r, tf) = match r {
            Ok((v, t, f)) => (Ok(v), Some((t, f))),
            Err(e) => (Err(e), None),
        };
        let bdev: &'b Device<Bound> = pdev.as_ref();
        let ppdev: &'b pci::Device<Bound> = pdev;
        try_pin_init!(&this in Data {
            vectors: r?,
            netdev: {
                let tr = tf.as_ref().map(|(t, _)| t.clone()).ok_or(EINVAL)?;
                // SAFETY: vectors is initialised (above), pinned with Data and never
                // moved or mutated, and dropped after netdev (field order), so a
                // reference to it is valid for everything netdev holds. Nothing
                // escapes netdev: its IRQ registrations only keep IRQ numbers.
                let vectors: &'b pci::IrqVectorRegistration<'b> = unsafe { &*(&raw const (*this.as_ptr()).vectors) };
                let set = Arc::new(
                    BarSet {
                        udma: ppdev.iomap_region(BAR_UDMA, NAME)?,
                        mac: ppdev.iomap_region(BAR_MAC, NAME)?,
                        ec: ppdev.iomap_region(BAR_EC, NAME)?,
                        tr: tr.clone(),
                    },
                    GFP_KERNEL,
                )?;
                tr.bar_lens([ppdev.resource_len(BAR_UDMA)?, ppdev.resource_len(BAR_MAC)?, ppdev.resource_len(BAR_EC)?]);
                tr.mark(Mark::Bars, Ctx::Task);
                let cfg = CfgSpace(ppdev, tr);
                let port = probe_port(&set, &cfg)?;
                let mac_addr = MacAddr(port.mac_addr());
                let mac_mode = port.cfg.mac_mode;
                let queues = Config { tx_queues: NUM_QUEUES as u32, rx_queues: NUM_QUEUES as u32 };
                Registration::<Priv<'b>>::new(bdev, queues, |u: &Unregistered<Priv<'b>>| priv_init(u, Setup { bdev, vectors, port, mac_addr, set, mac_mode, cfg }))?
            },
            _trace: tf.map(|(_, f)| f).ok_or(EINVAL)?,
        })
    }
}

#[cfg(not(al_eth_rs))]
kernel::module_pci_driver! {
    type: AlEth,
    name: "al_eth",
    authors: ["ai_re"],
    description: "Annapurna Labs Alpine integrated Ethernet, RGMII fixed link (Rust)",
    license: "GPL",
}

#[cfg(al_eth_rs)]
kernel::module_pci_driver! {
    type: AlEth,
    name: "al_eth_rs",
    authors: ["ai_re"],
    description: "Annapurna Labs Alpine integrated Ethernet, RGMII fixed link (Rust, test name)",
    license: "GPL",
}
