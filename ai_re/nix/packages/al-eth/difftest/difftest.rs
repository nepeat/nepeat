// SPDX-License-Identifier: GPL-2.0

//! Replays the C difftest trace (scenarios.c over the vendor HAL) against the
//! Rust HAL: the inputs (I lines) drive the same mock register bank, every
//! output line (register accesses, PCI config accesses, delays, barriers,
//! descriptor ring changes, results) must be identical. Built and run by
//! ../rust.nix:  difftest <trace.txt>
//! Differences that are allowed are listed in ../rust/hal/README.md and
//! counted here (none at the time of writing).

#![allow(dead_code)]

#[path = "../rust/hal/mod.rs"]
mod hal;

use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt::Write as _;
use std::rc::Rc;

use hal::io::{Bars, CDescMem, Delay, DescMem, PciCfg, RegIo};
use hal::mac::{self, MacMode};
use hal::port::{self, DmaBuf, Port, RxEvent, RxQueue, TxInfo, TxQueue, NUM_QUEUES, RING_SIZE};
use hal::ring::TxSlot;
use hal::Error;

const BAR_SIZE: [usize; 4] = [0x20000, 0x4000, 0x1000, 0x1000];
const BAR_NAME: [char; 4] = ['u', 'e', 'm', 'p'];

fn mock_hash(seed: u32, bar: u32, off: u32) -> u32 {
    if seed == 0 {
        return 0;
    }
    let mut h = seed ^ bar.wrapping_mul(0x9E37_79B9) ^ off.wrapping_mul(0x85EB_CA6B);
    h ^= h >> 16;
    h = h.wrapping_mul(0x7FEB_352D);
    h ^= h >> 15;
    h = h.wrapping_mul(0x846C_A68B);
    h ^= h >> 16;
    h
}

struct Ring {
    name: String,
    mem: Vec<[u32; 4]>,
    shadow: Vec<[u32; 4]>,
}

struct Model {
    /// --hw: ring bus addresses the hardware run got from dma_alloc_coherent
    /// (from its queue base register writes): (tx/rx/rc, queue) -> address
    hw_dma: HashMap<(u8, usize), u64>,
    /// --hw: the reads of the current call as the hardware answered them
    hw_reads: Option<std::collections::VecDeque<(usize, usize, u32)>>,
    bars: [Vec<u32>; 4],
    pins: HashMap<(usize, usize), u32>,
    out: Vec<String>,
    rings: Vec<Ring>,
    rmb: u64,
}

type M = Rc<RefCell<Model>>;

impl Model {
    fn new() -> Self {
        Model { hw_dma: HashMap::new(), hw_reads: None, bars: BAR_SIZE.map(|s| vec![0; s / 4]), pins: HashMap::new(), out: Vec::new(), rings: Vec::new(), rmb: 0 }
    }
    fn log(&mut self, s: String) {
        self.out.push(s);
    }
    fn dump_rings(&mut self) {
        let mut lines = Vec::new();
        for r in self.rings.iter_mut() {
            for (i, (m, s)) in r.mem.iter().zip(r.shadow.iter_mut()).enumerate() {
                if m != s {
                    lines.push(format!("DESC {} {} {:08x} {:08x} {:08x} {:08x}", r.name, i, m[0], m[1], m[2], m[3]));
                    *s = *m;
                }
            }
        }
        self.out.extend(lines);
    }
    fn ring(&mut self, name: &str) -> usize {
        self.rings.push(Ring { name: name.into(), mem: vec![[0; 4]; RING_SIZE], shadow: vec![[0; 4]; RING_SIZE] });
        self.rings.len() - 1
    }
}

struct Bar {
    m: M,
    b: usize,
}

impl RegIo for Bar {
    fn r32(&self, off: usize) -> u32 {
        let mut m = self.m.borrow_mut();
        assert!(off % 4 == 0 && off < BAR_SIZE[self.b], "read outside bar {} at {:#x}", BAR_NAME[self.b], off);
        let mut v = *m.pins.get(&(self.b, off)).unwrap_or(&m.bars[self.b][off / 4]);
        if let Some(q) = m.hw_reads.as_mut() {
            // the hardware's answer, if the Rust reads what the C read next
            v = match q.front() {
                Some(&(b, o, val)) if b == self.b && o == off => {
                    q.pop_front();
                    val
                }
                _ => 0xdead_beef,
            };
        }
        m.log(format!("R {} {:05x} {:08x}", BAR_NAME[self.b], off, v));
        v
    }
    fn w32(&self, off: usize, val: u32) {
        let mut m = self.m.borrow_mut();
        assert!(off % 4 == 0 && off < BAR_SIZE[self.b], "write outside bar {} at {:#x}", BAR_NAME[self.b], off);
        m.bars[self.b][off / 4] = val;
        m.log(format!("W {} {:05x} {:08x}", BAR_NAME[self.b], off, val));
    }
}

struct Dly(M);
impl Delay for Dly {
    fn udelay(&self, us: u32) {
        self.0.borrow_mut().log(format!("D {}", us));
    }
}

struct Pci(M);
impl PciCfg for Pci {
    fn read32(&self, off: u16) -> u32 {
        let mut m = self.0.borrow_mut();
        let mut v = m.bars[3][off as usize / 4];
        if let Some(q) = m.hw_reads.as_mut() {
            v = match q.front() {
                Some(&(3, o, val)) if o == off as usize => {
                    q.pop_front();
                    val
                }
                _ => 0xdead_beef,
            };
        }
        m.log(format!("CR {:03x} {:08x}", off, v));
        v
    }
    fn write32(&self, off: u16, val: u32) {
        let mut m = self.0.borrow_mut();
        m.bars[3][off as usize / 4] = val;
        m.log(format!("CW {:03x} {:08x}", off, val));
    }
}

struct Desc {
    m: M,
    ring: usize,
    dma: u64,
}
impl DescMem for Desc {
    fn write(&mut self, idx: usize, w: [u32; 4]) {
        self.m.borrow_mut().rings[self.ring].mem[idx] = w;
    }
    fn read(&self, idx: usize) -> [u32; 4] {
        self.m.borrow().rings[self.ring].mem[idx]
    }
    fn len(&self) -> usize {
        RING_SIZE
    }
    fn dma_addr(&self) -> u64 {
        self.dma
    }
    fn publish(&self) {
        self.m.borrow_mut().log("MB".into());
    }
}

struct CDesc {
    m: M,
    ring: usize,
    dma: u64,
}
impl CDescMem for CDesc {
    fn ctrl(&self, idx: usize) -> u32 {
        self.m.borrow().rings[self.ring].mem[idx][0]
    }
    fn read(&self, idx: usize) -> [u32; 4] {
        self.m.borrow().rings[self.ring].mem[idx]
    }
    fn len(&self) -> usize {
        RING_SIZE
    }
    fn dma_addr(&self) -> u64 {
        self.dma
    }
    fn dma_rmb(&self) {
        // the C has no barrier here (README.md): counted, not traced
        self.m.borrow_mut().rmb += 1;
    }
}

#[derive(Debug)]
struct RxBuf {
    addr: u64,
    len: u32,
}
impl DmaBuf for RxBuf {
    fn dma_addr(&self) -> u64 {
        self.addr
    }
    fn dma_len(&self) -> u32 {
        self.len
    }
}

/// --board: a recording from the hardware, where allocations did not fail
/// (the simulated ones fail every 61st time, glue.c)
static HW: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// glue.c's rx_alloc_fails: every 61st attempt (not in --hw)
fn rx_alloc_fails(attempt: u32) -> bool {
    !HW.load(std::sync::atomic::Ordering::Relaxed) && attempt % 61 == 60
}

/// glue.c's rx_buf_addr
fn rx_buf_addr(q: usize, n: u32) -> u64 {
    0x20_0000_0000 + ((q as u64) << 28) + (n % 0x10000) as u64 * 0x1000
}

type Txq = TxQueue<u32, Vec<TxSlot<u32>>, Desc>;
type Rxq = RxQueue<RxBuf, Vec<Option<RxBuf>>, Desc, CDesc>;

struct Sim {
    m: M,
    port: Option<Port<Bar>>,
    txq: Option<[Txq; NUM_QUEUES]>,
    rxq: Option<[Rxq; NUM_QUEUES]>,
    allocs: [u32; NUM_QUEUES],
    attempts: [u32; NUM_QUEUES],
    stopped: [bool; NUM_QUEUES],
    tok: u32,
}

fn errno(e: Error) -> i32 {
    match e {
        Error::Invalid => -22,
        Error::Timeout => -110,
        Error::Unsupported => -1,
        _ => -5,
    }
}

impl Sim {
    fn new(m: M) -> Self {
        Sim { m, port: None, txq: None, rxq: None, allocs: [0; NUM_QUEUES], attempts: [0; NUM_QUEUES], stopped: [false; NUM_QUEUES], tok: 0 }
    }
    fn ret(&self, s: String) {
        self.m.borrow_mut().log(format!("RET {}", s));
    }
    fn bars(&self) -> Bars<Bar> {
        Bars { udma: Bar { m: self.m.clone(), b: 0 }, ec: Bar { m: self.m.clone(), b: 1 }, mac: Bar { m: self.m.clone(), b: 2 } }
    }
    fn port(&mut self) -> &mut Port<Bar> {
        self.port.as_mut().expect("no port (probe failed)")
    }

    /// glue.c alloc_rings: new zeroed rings, registered in the same order
    fn new_rings(&mut self) {
        let m = &self.m;
        let hwd = m.borrow().hw_dma.clone();
        let hw = |kind: u8, q: usize, dflt: u64| *hwd.get(&(kind, q)).unwrap_or(&dflt);
        m.borrow_mut().rings.clear();
        let tx: Vec<Desc> = (0..NUM_QUEUES)
            .map(|q| Desc { m: m.clone(), ring: m.borrow_mut().ring(&format!("tx{q}")), dma: hw(0, q, 0x10_0000_0000 + q as u64 * 0x10_0000) })
            .collect();
        let mut rx = Vec::new();
        for q in 0..NUM_QUEUES {
            let d = Desc { m: m.clone(), ring: m.borrow_mut().ring(&format!("rx{q}")), dma: hw(1, q, 0x10_0080_0000 + q as u64 * 0x10_0000) };
            let c = CDesc { m: m.clone(), ring: m.borrow_mut().ring(&format!("rc{q}")), dma: hw(2, q, 0x10_0100_0000 + q as u64 * 0x10_0000) };
            rx.push((d, c));
        }
        match (self.txq.as_mut(), self.rxq.as_mut()) {
            (Some(t), Some(r)) => {
                for (q, d) in t.iter_mut().zip(tx) {
                    q.replace_mem(d).ok().expect("tx ring busy");
                }
                for (q, (d, c)) in r.iter_mut().zip(rx) {
                    q.replace_mem(d, c).ok().expect("rx ring busy");
                }
            }
            _ => {
                let mut tx = tx.into_iter();
                let mut rx = rx.into_iter();
                self.txq = Some(core::array::from_fn(|q| {
                    TxQueue::new(q, (0..RING_SIZE).map(|_| TxSlot::default()).collect(), tx.next().unwrap()).unwrap()
                }));
                self.rxq = Some(core::array::from_fn(|q| {
                    let (d, c) = rx.next().unwrap();
                    RxQueue::new(q, (0..RING_SIZE).map(|_| None).collect(), d, c).unwrap()
                }));
            }
        }
    }

    fn call(&mut self, a: &[&str]) {
        let num = |s: &str| -> i64 { s.parse().unwrap_or_else(|_| panic!("number: {s}")) };
        let hex = |s: &str| -> u64 { u64::from_str_radix(s, 16).unwrap() };
        let dly = Dly(self.m.clone());
        let pci = Pci(self.m.clone());
        match a[0] {
            "probe" => match Port::probe(self.bars(), &dly, &pci, num(a[1]) as u8) {
                Ok(p) => {
                    let c = p.cfg;
                    let mac = p.mac_addr();
                    self.ret(format!(
                        "probe mode {} mac {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x} speed {} duplex {} autoneg {}",
                        c.mac_mode.raw(), mac[0], mac[1], mac[2], mac[3], mac[4], mac[5], c.speed, c.full_duplex as i32, c.autoneg as i32
                    ));
                    self.port = Some(p);
                    self.txq = None;
                    self.rxq = None;
                    self.allocs = [0; NUM_QUEUES];
                    self.attempts = [0; NUM_QUEUES];
                    self.stopped = [false; NUM_QUEUES];
                }
                Err(_) => {
                    self.port = None;
                    self.ret("probe failed".into());
                }
            },
            "up" => {
                let mut key = [0u32; 10];
                for (k, s) in key.iter_mut().zip(&a[1..]) {
                    *k = hex(s) as u32;
                }
                // rings are allocated after hw_init in C; the allocation itself touches no register
                self.new_rings();
                self.stopped = [false; NUM_QUEUES];
                let mut txq = self.txq.take().unwrap();
                let mut rxq = self.rxq.take().unwrap();
                let mtu = self.port().mtu();
                let allocs = &mut self.allocs;
                let attempts = &mut self.attempts;
                let port = self.port.as_mut().unwrap();
                let r = port.up(&dly, &pci, &key, txq.each_mut(), rxq.each_mut(), |q| {
                    let a = attempts[q];
                    attempts[q] += 1;
                    if rx_alloc_fails(a) {
                        return None;
                    }
                    let n = allocs[q];
                    allocs[q] += 1;
                    Some(RxBuf { addr: rx_buf_addr(q, n), len: port::rx_buf_size(mtu) })
                });
                self.txq = Some(txq);
                self.rxq = Some(rxq);
                match r {
                    Ok(()) => self.ret("up ok".into()),
                    Err(e) => self.ret(format!("up hw_init {}", errno(e))),
                }
            }
            "down" => {
                let mut txq = self.txq.take().unwrap();
                let mut rxq = self.rxq.take().unwrap();
                let freed = self.port().down(&dly, txq.each_mut(), rxq.each_mut(), |_| {}, |_| {});
                self.txq = Some(txq);
                self.rxq = Some(rxq);
                for (q, (t, r)) in freed.iter().enumerate() {
                    self.ret(format!("down q {q} tx_freed {t} rx_freed {r}"));
                }
            }
            "stats" => match self.port().stats() {
                None => self.ret("stats down".into()),
                Some(s) => {
                    let len_err = (s.undersize_pkts as u32)
                        .wrapping_add(s.fragments as u32)
                        .wrapping_add(s.jabbers as u32)
                        .wrapping_add(s.oversize_pkts as u32);
                    self.ret(format!(
                        "stats rx_packets {} tx_packets {} rx_bytes {} tx_bytes {} multicast {} rx_length_errors {} rx_crc_errors {} rx_frame_errors {} rx_fifo_errors {} rx_errors {} tx_errors {}",
                        s.frames_received_ok, s.frames_transmitted_ok, s.octets_received_ok, s.octets_transmitted_ok,
                        s.if_in_multicast_pkts, len_err, s.fcs_errors, s.alignment_errors, s.drop_events, s.if_in_errors, s.if_out_errors
                    ))
                }
            },
            "link" => {
                let up = self.port().link_up();
                self.ret(format!("link rc 0 up {}", up as i32));
            }
            "mgmt" => {
                let c = self.port().mgmt_causes();
                self.ret(format!("mgmt {:08x} {:08x} {:08x}", c[0], c[1], c[2]));
            }
            "rx_mode" => {
                let p = self.port();
                // the C programs its own address for each unicast entry (README.md)
                let uc = vec![p.mac_addr(); num(a[4]) as usize];
                p.set_rx_mode(num(a[1]) != 0, num(a[2]) != 0, num(a[3]) as usize, &uc);
            }
            "set_mac" => {
                let mut b = [0u8; 6];
                for (x, s) in b.iter_mut().zip(a[1].split(':')) {
                    *x = u8::from_str_radix(s, 16).unwrap();
                }
                self.port().set_mac_addr(b);
            }
            "mtu" => {
                let v = num(a[1]);
                let r = if (0..=u32::MAX as i64).contains(&v) { self.port().change_mtu(v as u32) } else { Err(Error::Invalid) };
                self.ret(if r.is_ok() { "change_mtu ok".into() } else { "change_mtu einval".into() });
            }
            "mac_config" => {
                let mode = MacMode::from_raw(num(a[1]) as u32).unwrap();
                let bars = self.bars();
                mac::config(&bars.mac, mode);
                self.ret("mac_config 0".into());
                let r = mac::link_config(&bars.mac, mode, num(a[2]) != 0, num(a[3]) != 0, num(a[4]) as u32, num(a[5]) != 0);
                self.ret(format!("link_config {}", r.map_or_else(errno, |_| 0)));
            }
            "flr" => {
                let _ = self.port().function_reset(&dly, &pci);
            }
            "xmit" => self.xmit(a),
            "tx_poll" => {
                let q = num(a[1]) as usize;
                let bars = self.bars();
                let t = &mut self.txq.as_mut().unwrap()[q];
                let c = t.clean(&bars, |_, _| {});
                if self.stopped[q] && c.available > port::TX_STOP_THRESH {
                    self.stopped[q] = false;
                }
                t.unmask(&bars);
                self.ret(format!("tx_poll q {q} pkts {} bytes {} stopped {}", c.pkts, c.bytes, self.stopped[q] as i32));
            }
            "rx_poll" => {
                let q = num(a[1]) as usize;
                let budget = num(a[2]) as u32;
                let bars = self.bars();
                let mtu = self.port().mtu();
                let m = self.m.clone();
                let allocs = &mut self.allocs;
                let attempts = &mut self.attempts;
                let r = &mut self.rxq.as_mut().unwrap()[q];
                let res = r
                    .poll(
                        &bars,
                        budget,
                        |ev| {
                            let line = match ev {
                                RxEvent::Small { info, .. } => format!(
                                    "RET rx q {q} descs {} len {} small 1 csum {} hash {:08x} l4 {}",
                                    info.pkt.descs, info.len, info.csum_ok as i32, info.hash, info.l4_hash as i32
                                ),
                                RxEvent::Frags { info, bufs } => {
                                    let n = bufs.into_iter().count();
                                    assert_eq!(n as u32, info.pkt.descs);
                                    format!(
                                        "RET rx q {q} descs {} len {} small 0 csum {} hash {:08x} l4 {}",
                                        info.pkt.descs, info.len, info.csum_ok as i32, info.hash, info.l4_hash as i32
                                    )
                                }
                                RxEvent::Dropped { flags, descs } => format!("RET rx q {q} error flags {flags:08x} descs {descs}"),
                            };
                            m.borrow_mut().log(line);
                        },
                        || {
                            let a = attempts[q];
                            attempts[q] += 1;
                            if rx_alloc_fails(a) {
                                return None;
                            }
                            let n = allocs[q];
                            allocs[q] += 1;
                            Some(RxBuf { addr: rx_buf_addr(q, n), len: port::rx_buf_size(mtu) })
                        },
                    )
                    .expect("rx poll");
                if res.resched {
                    self.ret(format!("rx_poll q {q} resched"));
                } else if res.complete {
                    r.complete(&bars);
                }
                self.ret(format!("rx_poll q {q} work {}", res.work));
            }
            other => panic!("unknown call {other}"),
        }
        self.m.borrow_mut().dump_rings();
    }

    fn xmit(&mut self, a: &[&str]) {
        let num = |s: &str| -> u64 { s.parse().unwrap() };
        let q = num(a[1]) as usize;
        let mut len = num(a[2]) as u32;
        let mut headlen = num(a[3]) as u32;
        let mut nfr = num(a[4]) as usize;
        let mut i = 5;
        let frags: Vec<u32> = (0..nfr).map(|k| num(a[i + k]) as u32).collect();
        i += nfr;
        let info_raw: Vec<u64> = (0..9).map(|k| num(a[i + k])).collect();
        i += 9;
        let addrs: Vec<u64> = a[i..].iter().map(|s| u64::from_str_radix(s, 16).unwrap()).collect();
        if len < 60 {
            // eth_skb_pad: linear and padded
            len = 60;
            headlen = 60;
            nfr = 0;
        }
        let mut segs = vec![(addrs[0], headlen)];
        for k in 0..nfr {
            segs.push((addrs[k + 1], frags[k]));
        }
        let info = TxInfo {
            len,
            partial: info_raw[0] != 0,
            gso_size: info_raw[1] as u32,
            ip_version: info_raw[2] as u8,
            l4proto: info_raw[3] as u8,
            nh_off: info_raw[4] as u32,
            nh_len: info_raw[5] as u32,
            transport_off: info_raw[6] as u32,
            doff: info_raw[7] as u8,
            vlan: info_raw[8] != 0,
        };
        let bars = self.bars();
        let tok = self.tok;
        self.tok += 1;
        let t = &mut self.txq.as_mut().unwrap()[q];
        match t.offload(&info) {
            Err(_) => self.ret("xmit drop mss".into()),
            Ok(o) => match t.xmit(&bars, &o, &segs, len, tok) {
                Ok(r) => {
                    if r.stop {
                        self.stopped[q] = true;
                    }
                    self.ret(format!("xmit q {q} descs {} gso {} stopped {}", r.descs, o.gso_size, self.stopped[q] as i32));
                }
                Err(_) => self.ret("xmit no room".into()),
            },
        }
    }
}


/// One scenario at a time: replay, compare per call.
struct Replay {
    hw: bool,
    sim: Sim,
    scenario: String,
    call: String,
    expect: Vec<String>,
    calls: u64,
    lines: u64,
    errors: u32,
}

impl Replay {
    fn finish_call(&mut self) {
        if self.call.is_empty() {
            return;
        }
        let mut got: Vec<String> = std::mem::take(&mut self.sim.m.borrow_mut().out);
        if self.hw {
            // the hardware trace has no descriptor dumps, results or barriers
            got.retain(|l| !(l.starts_with("DESC ") || l.starts_with("RET ") || l == "MB"));
            if let Some(q) = self.sim.m.borrow_mut().hw_reads.take() {
                if !q.is_empty() {
                    eprintln!("hw: {} hardware reads not made by the Rust in '{}'", q.len(), self.call);
                }
            }
        }
        self.calls += 1;
        self.lines += self.expect.len() as u64;
        if got != self.expect {
            self.errors += 1;
            let i = got.iter().zip(self.expect.iter()).position(|(a, b)| a != b).unwrap_or(got.len().min(self.expect.len()));
            let mut msg = String::new();
            let _ = writeln!(msg, "MISMATCH scenario {} call '{}' at output line {} (C {} lines, Rust {})", self.scenario, self.call, i, self.expect.len(), got.len());
            if self.hw {
                for (k, (c, r)) in self.expect.iter().zip(got.iter()).enumerate() {
                    if c != r {
                        let _ = writeln!(msg, "  line {k}: C {c} | Rust {r}");
                    }
                }
            }
            for k in i.saturating_sub(3)..(i + 4) {
                let _ = writeln!(msg, "  C:    {}", self.expect.get(k).map_or("<end>", |s| s.as_str()));
                let _ = writeln!(msg, "  Rust: {}", got.get(k).map_or("<end>", |s| s.as_str()));
            }
            eprint!("{msg}");
        }
        self.expect.clear();
        self.call.clear();
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let hw = args.iter().any(|a| a == "--hw");
    HW.store(args.iter().any(|a| a == "--board"), std::sync::atomic::Ordering::Relaxed);
    let path = args.iter().skip(1).find(|a| !a.starts_with("--")).expect("usage: difftest [--hw [--board]] TRACE");
    let text = std::fs::read_to_string(path).expect("trace");
    let all: Vec<&str> = text.lines().collect();
    let m: M = Rc::new(RefCell::new(Model::new()));
    let mut rp = Replay { hw, sim: Sim::new(m.clone()), scenario: String::new(), call: String::new(), expect: Vec::new(), calls: 0, lines: 0, errors: 0 };
    let mut scenarios = 0;
    for (ln, line) in all.iter().enumerate() {
        let line = *line;
        if line.starts_with('#') || line.is_empty() {
            continue;
        }
        let f: Vec<&str> = line.split_whitespace().collect();
        match f[0] {
            "S" => {
                rp.finish_call();
                scenarios += 1;
                rp.scenario = f[1].into();
                *m.borrow_mut() = Model::new();
                rp.sim = Sim::new(m.clone());
            }
            "I" => {
                rp.finish_call();
                let mut mm = m.borrow_mut();
                let bar = |s: &str| BAR_NAME.iter().position(|c| s.starts_with(*c)).unwrap();
                let h = |s: &str| u32::from_str_radix(s, 16).unwrap();
                match f[1] {
                    "FILL" => {
                        let seed: u32 = f[2].parse().unwrap();
                        for (b, v) in mm.bars.iter_mut().enumerate() {
                            for (i, w) in v.iter_mut().enumerate() {
                                *w = mock_hash(seed, b as u32, (i * 4) as u32);
                            }
                        }
                    }
                    "PIN" => {
                        let k = (bar(f[2]), h(f[3]) as usize);
                        mm.pins.insert(k, h(f[4]));
                    }
                    "SET" => {
                        let (b, o) = (bar(f[2]), h(f[3]) as usize);
                        mm.bars[b][o / 4] = h(f[4]);
                    }
                    "CDESC" => {
                        let idx: usize = f[3].parse().unwrap();
                        let w = [h(f[4]), h(f[5]), h(f[6]), h(f[7])];
                        let r = mm.rings.iter_mut().find(|r| r.name == f[2]).expect("cdesc ring");
                        r.mem[idx] = w;
                        r.shadow[idx] = w;
                    }
                    "CALL" => {
                        if hw {
                            // this call's reads, as the hardware answered them
                            let mut q = std::collections::VecDeque::new();
                            for l in all[ln + 1..].iter().take_while(|l| !l.starts_with("I ") && !l.starts_with("S ")) {
                                let p: Vec<&str> = l.split_whitespace().collect();
                                match p.as_slice() {
                                    ["R", b, o, v] => q.push_back((bar(b), h(o) as usize, h(v))),
                                    ["CR", o, v] => q.push_back((3, h(o) as usize, h(v))),
                                    _ => {}
                                }
                            }
                            mm.hw_reads = Some(q);
                            // ring addresses: inputs of the hardware run (dma_alloc_coherent)
                            use hal::regs::off;
                            let mut lo: HashMap<(u8, usize), u32> = HashMap::new();
                            let mut hi: HashMap<(u8, usize), u32> = HashMap::new();
                            for l in all[ln + 1..].iter().take_while(|l| !l.starts_with("I ") && !l.starts_with("S ")) {
                                let p: Vec<&str> = l.split_whitespace().collect();
                                if let ["W", "u", o, v] = p.as_slice() {
                                    let o = h(o) as usize;
                                    for q in 0..NUM_QUEUES {
                                        let t = off::U_M2S_Q0 + q * off::U_M2S_Q_STRIDE;
                                        let r = off::U_S2M_Q0 + q * off::U_S2M_Q_STRIDE;
                                        let k = match o {
                                            x if x == t + off::Q_DRBP_LOW => Some((0, q, true)),
                                            x if x == t + off::Q_DRBP_HIGH => Some((0, q, false)),
                                            x if x == r + off::Q_DRBP_LOW => Some((1, q, true)),
                                            x if x == r + off::Q_DRBP_HIGH => Some((1, q, false)),
                                            x if x == r + off::Q_CRBP_LOW => Some((2, q, true)),
                                            x if x == r + off::Q_CRBP_HIGH => Some((2, q, false)),
                                            _ => None,
                                        };
                                        if let Some((kind, q, low)) = k {
                                            if low { lo.insert((kind, q), h(v)); } else { hi.insert((kind, q), h(v)); }
                                        }
                                    }
                                }
                            }
                            for (k, l) in lo {
                                let hv = *hi.get(&k).unwrap_or(&0);
                                mm.hw_dma.insert(k, ((hv as u64) << 32) | l as u64);
                            }
                        }
                        drop(mm);
                        rp.call = f[2..].join(" ");
                        let args: Vec<&str> = f[2..].to_vec();
                        rp.sim.call(&args);
                    }
                    other => panic!("unknown input {other}"),
                }
            }
            _ => {
                if rp.call.is_empty() {
                    panic!("output outside a call in scenario {}: {line}", rp.scenario);
                }
                rp.expect.push(line.to_string());
            }
        }
    }
    rp.finish_call();
    let rmb = m.borrow().rmb;
    println!(
        "difftest: {} scenarios, {} calls, {} output lines compared, {} mismatching calls (dma_rmb in the last scenario: {})",
        scenarios, rp.calls, rp.lines, rp.errors, rmb
    );
    if rp.errors != 0 {
        std::process::exit(1);
    }
}
