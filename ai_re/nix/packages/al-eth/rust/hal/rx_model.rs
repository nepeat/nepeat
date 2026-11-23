// SPDX-License-Identifier: GPL-2.0

//! A model of the S2M UDMA under the real RxQueue (poll, refill, ring): the
//! device consumes only descriptors the driver posted and doorbelled, checks
//! each one (fresh ring id, a live buffer of the posted length), writes the
//! packet into the buffers and multi-descriptor completions; buffer
//! allocation fails at high rates for a while, then recovers. Every delivered
//! packet must be the one the device wrote, into the buffers it wrote, and
//! after recovery the ring must be full again (all descriptors posted, every
//! posted one owning its buffer).

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet, VecDeque};
use std::rc::Rc;

use super::*;
use crate::hal::io::{Bars, CDescMem, DescMem, RegIo};
use crate::hal::regs::{k, off};
use crate::hal::udma::{Dir, Queue, Udma};

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

#[derive(Clone)]
struct MReg {
    tail: Rc<Cell<u64>>,
    drtp: usize,
}
impl RegIo for MReg {
    fn r32(&self, _off: usize) -> u32 {
        0
    }
    fn w32(&self, off: usize, val: u32) {
        if off == self.drtp {
            self.tail.set(self.tail.get() + u64::from(val));
        }
    }
}

type Mem = Rc<RefCell<Vec<[u32; 4]>>>;
struct VDesc(Mem, u64);
impl DescMem for VDesc {
    fn write(&mut self, i: usize, w: [u32; 4]) {
        self.0.borrow_mut()[i] = w;
    }
    fn read(&self, i: usize) -> [u32; 4] {
        self.0.borrow()[i]
    }
    fn len(&self) -> usize {
        self.0.borrow().len()
    }
    fn dma_addr(&self) -> u64 {
        self.1
    }
    fn publish(&self) {}
}
impl CDescMem for VDesc {
    fn ctrl(&self, i: usize) -> u32 {
        self.0.borrow()[i][0]
    }
    fn read(&self, i: usize) -> [u32; 4] {
        self.0.borrow()[i]
    }
    fn len(&self) -> usize {
        self.0.borrow().len()
    }
    fn dma_addr(&self) -> u64 {
        self.1
    }
    fn dma_rmb(&self) {}
}

/// A buffer: its address is unique; the live set catches double frees and
/// descriptors pointing at freed buffers.
struct TBuf {
    addr: u64,
    len: u32,
    live: Rc<RefCell<HashSet<u64>>>,
}
impl DmaBuf for TBuf {
    fn dma_addr(&self) -> u64 {
        self.addr
    }
    fn dma_len(&self) -> u32 {
        self.len
    }
}
impl Drop for TBuf {
    fn drop(&mut self) {
        assert!(self.live.borrow_mut().remove(&self.addr), "buffer {:#x} freed twice", self.addr);
    }
}

struct Hw {
    desc: Mem,
    cdesc: Mem,
    tail: Rc<Cell<u64>>,
    /// descriptors consumed / completions written, in total
    dpos: u64,
    cpos: u64,
    /// what the device wrote into each buffer: (packet tag, bytes)
    data: HashMap<u64, (u64, u32)>,
    dropped: u64,
}

const SIZE: usize = RING_SIZE;

impl Hw {
    fn ring_id(pos: u64) -> u32 {
        (k::AL_UDMA_INITIAL_RING_ID + (pos / SIZE as u64) as u32) & k::DMA_RING_ID_MASK
    }
    /// Receives one frame of len bytes; false if the ring had too few
    /// descriptors (the device drops the frame, as when software starves it).
    fn receive(&mut self, len: u32, tag: u64, live: &HashSet<u64>) -> bool {
        // enough posted descriptors?
        let (mut need, mut have) = (len, 0u64);
        while need > 0 {
            if self.dpos + have >= self.tail.get() {
                self.dropped += 1;
                return false;
            }
            let w = self.desc.borrow()[((self.dpos + have) % SIZE as u64) as usize];
            need = need.saturating_sub(w[0] & k::AL_S2M_DESC_LEN_MASK);
            have += 1;
        }
        let mut left = len;
        for i in 0..have {
            let pos = self.dpos;
            let w = self.desc.borrow()[(pos % SIZE as u64) as usize];
            let rid = (w[0] & k::AL_M2S_DESC_RING_ID_MASK) >> k::AL_M2S_DESC_RING_ID_SHIFT;
            assert_eq!(rid, Self::ring_id(pos), "descriptor {} not written by the driver (stale ring id)", pos);
            let addr = u64::from(w[2]) | u64::from(w[3]) << 32;
            assert!(live.contains(&addr), "descriptor {} points at a buffer the driver does not own ({:#x})", pos, addr);
            let blen = w[0] & k::AL_S2M_DESC_LEN_MASK;
            let chunk = left.min(blen);
            left -= chunk;
            self.data.insert(addr, (tag, chunk));
            let mut c = Self::ring_id(self.cpos) << k::AL_M2S_DESC_RING_ID_SHIFT | k::AL_UDMA_CDESC_BUF1_USED;
            if i == 0 {
                c |= k::AL_UDMA_CDESC_FIRST;
            }
            if i == have - 1 {
                c |= k::AL_UDMA_CDESC_LAST;
            }
            self.cdesc.borrow_mut()[(self.cpos % SIZE as u64) as usize] = [c, chunk, 0, 0];
            self.cpos += 1;
            self.dpos += 1;
        }
        true
    }
}

fn run(seed: u64, pressure_rounds: usize, fail_per_mille: u64) {
    let tail = Rc::new(Cell::new(0));
    let live: Rc<RefCell<HashSet<u64>>> = Rc::new(RefCell::new(HashSet::new()));
    let desc: Mem = Rc::new(RefCell::new(vec![[0; 4]; SIZE]));
    let cdesc: Mem = Rc::new(RefCell::new(vec![[0; 4]; SIZE]));
    let probe = MReg { tail: tail.clone(), drtp: usize::MAX };
    let mut udma = Udma::init(&probe, Dir::Rx);
    let q = Queue::init(&probe, &mut udma, 0, SIZE as u32, 0x1000_0000, Some((0x2000_0000, 16))).unwrap();
    let reg = MReg { tail: tail.clone(), drtp: q.base() + off::Q_DRTP_INC };
    let bars = Bars { udma: reg.clone(), ec: reg.clone(), mac: reg.clone() };
    let mut slots = Vec::new();
    slots.resize_with(SIZE, || None);
    let mut r: RxQueue<TBuf, Vec<Option<TBuf>>, VDesc, VDesc> =
        RxQueue::new(0, slots, VDesc(desc.clone(), 0x1000_0000), VDesc(cdesc.clone(), 0x2000_0000)).unwrap();
    r.q = Some(q);
    let rng = Rc::new(RefCell::new(Rng(seed)));
    let next_addr = Rc::new(Cell::new(0x4000_0000u64));
    let failing = Rc::new(Cell::new(false));
    let fails = Rc::new(Cell::new(0u64));
    let mut alloc = {
        let (rng, next_addr, live, failing, fails) = (rng.clone(), next_addr.clone(), live.clone(), failing.clone(), fails.clone());
        move || -> Option<TBuf> {
            if failing.get() && rng.borrow_mut().below(1000) < fail_per_mille {
                fails.set(fails.get() + 1);
                return None;
            }
            let a = next_addr.get();
            next_addr.set(a + 0x1000);
            live.borrow_mut().insert(a);
            // sizes as rx_buf_size gives them for MTU 68..576 and >= 1514
            let len = if rng.borrow_mut().below(8) == 0 { 600 } else { 1536 };
            Some(TBuf { addr: a, len, live: live.clone() })
        }
    };
    assert_eq!(r.refill(&bars, (SIZE - 1) as u32, &mut alloc).unwrap(), (SIZE - 1) as u32);
    let mut hw = Hw { desc, cdesc, tail: tail.clone(), dpos: 0, cpos: 0, data: HashMap::new(), dropped: 0 };
    let expect: RefCell<VecDeque<(u64, u32)>> = RefCell::new(VecDeque::new());
    let (mut tag, mut delivered, mut resched_polls) = (0u64, 0u64, 0u64);
    for round in 0..pressure_rounds * 2 + 50 {
        failing.set(round < pressure_rounds);
        // a burst of frames: small, MTU sized, jumbo
        let n = rng.borrow_mut().below(200);
        for _ in 0..n {
            let kind = rng.borrow_mut().below(10);
            let len = match kind {
                0..=3 => 60 + rng.borrow_mut().below(69) as u32,
                4..=8 => 129 + rng.borrow_mut().below(1386) as u32,
                _ => 1515 + rng.borrow_mut().below(7500) as u32,
            };
            tag += 1;
            if hw.receive(len, tag, &live.borrow()) {
                expect.borrow_mut().push_back((tag, len));
            }
        }
        // NAPI: poll until complete
        for _ in 0..10_000 {
            let data = &hw.data;
            let p = r
                .poll(&bars, 64, |ev| {
                    let (t, len) = expect.borrow_mut().pop_front().expect("a packet the device did not send");
                    match ev {
                        RxEvent::Small { buf, info } => {
                            assert_eq!(info.len, len);
                            assert_eq!(data.get(&buf.addr).map(|d| d.0), Some(t), "small packet from the wrong buffer");
                        }
                        RxEvent::Frags { bufs, info } => {
                            assert_eq!(info.len, len);
                            let mut sum = 0;
                            for (b, l) in bufs {
                                assert_eq!(data.get(&b.addr), Some(&(t, l)), "fragment from the wrong buffer");
                                sum += l;
                            }
                            assert_eq!(sum, len);
                        }
                        RxEvent::Dropped { .. } => panic!("no error frames sent"),
                    }
                    delivered += 1;
                }, &mut alloc)
                .unwrap();
            if p.resched {
                resched_polls += 1;
            }
            if p.complete {
                break;
            }
        }
        assert!(expect.borrow().is_empty() || failing.get(), "packets left undelivered after a complete poll");
    }
    // recovered: the ring is full again and every posted descriptor owns its buffer
    let q = r.q.as_ref().unwrap();
    assert_eq!(q.available(), 0, "ring not refilled after the allocation failures ended");
    assert_eq!(hw.tail.get() - hw.dpos, (SIZE - 1) as u64, "posted descriptors != ring size - 1");
    assert_eq!(r.ring.filled(), live.borrow().len(), "buffers outside the ring");
    let l = live.borrow();
    for pos in hw.dpos..hw.tail.get() {
        let w = hw.desc.borrow()[(pos % SIZE as u64) as usize];
        assert!(l.contains(&(u64::from(w[2]) | u64::from(w[3]) << 32)), "posted descriptor without a buffer");
    }
    println!("rx model seed {seed:#x}: {tag} frames, {delivered} delivered, {} dropped by the starved ring, {} allocation failures, {resched_polls} short-refill polls",
        hw.dropped, fails.get());
    assert!(fails.get() > 0 || fail_per_mille == 0);
}

#[test]
fn rx_ring_survives_allocation_failure() {
    run(0x9e3779b97f4a7c15, 0, 0);
    run(0x1234_5678_9abc_def1, 100, 500);
    run(0xdead_beef_cafe_f00d, 200, 950);
    run(0x0bad_5eed_0bad_5eed, 300, 999);
}

/// Records every register write.
#[derive(Clone, Default)]
struct WReg(Rc<RefCell<Vec<(usize, u32)>>>);
impl RegIo for WReg {
    fn r32(&self, _off: usize) -> u32 {
        0
    }
    fn w32(&self, off: usize, val: u32) {
        self.0.borrow_mut().push((off, val));
    }
}

#[test]
fn rx_moderation_fixed_and_adaptive() {
    let w = WReg::default();
    let bars = Bars { udma: w.clone(), ec: w.clone(), mac: w.clone() };
    let mem = || VDesc(Rc::new(RefCell::new(vec![[0; 4]; SIZE])), 0);
    let mut slots = Vec::new();
    slots.resize_with(SIZE, || None);
    let mut r: RxQueue<TBuf, Vec<Option<TBuf>>, VDesc, VDesc> = RxQueue::new(1, slots, mem(), mem()).unwrap();
    r.modtab = [
        Moder { interval: 0, pkts: 3, bytes: 3048 },
        Moder { interval: 31, pkts: 12, bytes: 16384 },
        Moder { interval: 78, pkts: 48, bytes: 65536 },
        Moder { interval: 125, pkts: 96, bytes: 131072 },
        Moder { interval: 187, pkts: 128, bytes: 196608 },
    ];
    let unmask = (iofic::UDMA_PRIMARY.unmask_offset(Group::B), !(1u32 << 1));
    let take = || core::mem::take(&mut *w.0.borrow_mut());
    // fixed: the interval once, then only the unmask
    r.set_moderation(Some(7));
    r.complete(&bars);
    let first = take();
    assert_eq!(first.len(), 2);
    assert_eq!(first[1], unmask);
    let interval_reg = first[0].0;
    r.complete(&bars);
    assert_eq!(take(), [unmask]);
    // adaptive with load: steps up and writes its own interval
    r.set_moderation(None);
    r.moder.packets = 100;
    r.moder.bytes = 100_000;
    r.complete(&bars);
    let a = take();
    assert_eq!(a.len(), 2);
    assert_eq!(a[0].0, interval_reg);
    assert_ne!(a[0].1, first[0].1);
    // an adaptive step raced with "adaptive-rx off": the next completion puts the fixed value back
    r.set_moderation(Some(7));
    r.complete(&bars);
    assert_eq!(take(), first);
    r.complete(&bars);
    assert_eq!(take(), [unmask]);
}

/// NAPI under sustained load: frames arrive faster than one budget per poll
/// (150 per interrupt, budget 64); the CPU can run several polls between
/// arrivals. With the driver's decision (RxPoll::repoll) the ring never runs
/// dry; completing a poll that used its whole budget (and so waiting for the
/// next interrupt) starves it within a few interrupts, which the model must
/// catch.
fn napi_load(correct: bool) -> (u64, u64) {
    let tail = Rc::new(Cell::new(0));
    let live: Rc<RefCell<HashSet<u64>>> = Rc::new(RefCell::new(HashSet::new()));
    let desc: Mem = Rc::new(RefCell::new(vec![[0; 4]; SIZE]));
    let cdesc: Mem = Rc::new(RefCell::new(vec![[0; 4]; SIZE]));
    let probe = MReg { tail: tail.clone(), drtp: usize::MAX };
    let mut udma = Udma::init(&probe, Dir::Rx);
    let q = Queue::init(&probe, &mut udma, 0, SIZE as u32, 0x1000_0000, Some((0x2000_0000, 16))).unwrap();
    let reg = MReg { tail: tail.clone(), drtp: q.base() + off::Q_DRTP_INC };
    let bars = Bars { udma: reg.clone(), ec: reg.clone(), mac: reg.clone() };
    let mut slots = Vec::new();
    slots.resize_with(SIZE, || None);
    let mut r: RxQueue<TBuf, Vec<Option<TBuf>>, VDesc, VDesc> =
        RxQueue::new(0, slots, VDesc(desc.clone(), 0x1000_0000), VDesc(cdesc.clone(), 0x2000_0000)).unwrap();
    r.q = Some(q);
    r.set_moderation(None);
    let next_addr = Cell::new(0x4000_0000u64);
    let mut alloc = || -> Option<TBuf> {
        let a = next_addr.get();
        next_addr.set(a + 0x1000);
        live.borrow_mut().insert(a);
        Some(TBuf { addr: a, len: 1536, live: live.clone() })
    };
    assert_eq!(r.refill(&bars, (SIZE - 1) as u32, &mut alloc).unwrap(), (SIZE - 1) as u32);
    let mut hw = Hw { desc, cdesc, tail: tail.clone(), dpos: 0, cpos: 0, data: HashMap::new(), dropped: 0 };
    let mut rng = Rng(0x5eed_1234_abcd_0001);
    let (mut tag, mut delivered) = (0u64, 0u64);
    const BUDGET: u32 = 64;
    for _ in 0..2000 {
        // an interrupt's worth of frames (MTU sized and small), then the IRQ
        for _ in 0..150 {
            tag += 1;
            let len = if rng.below(4) == 0 { 60 + rng.below(60) as u32 } else { 1000 + rng.below(514) as u32 };
            hw.receive(len, tag, &live.borrow());
        }
        let mut scheduled = true;
        // CPU time for up to 8 polls before the next frames arrive
        for _ in 0..8 {
            if !scheduled {
                break;
            }
            let p = r.poll(&bars, BUDGET, |_ev| delivered += 1, &mut alloc).unwrap();
            let again = if correct { p.repoll(BUDGET, r.posted()) } else { p.resched && r.posted() < RX_LOW_WATER };
            if !again {
                r.complete(&bars); // napi_complete_done + unmask: idle until the next interrupt
                scheduled = false;
            }
        }
    }
    (hw.dropped, delivered)
}

#[test]
fn rx_napi_repolls_under_sustained_load() {
    let (dropped, delivered) = napi_load(true);
    println!("napi model: {delivered} delivered, {dropped} dropped by a dry ring");
    assert_eq!(dropped, 0, "the ring ran dry although the CPU had time to poll");
    assert_eq!(delivered, 2000 * 150);
    let (dropped_bad, _) = napi_load(false);
    assert!(dropped_bad > 0, "the model does not catch completing a full-budget poll");
}

#[test]
fn rx_repoll_decision() {
    let p = |work, resched| RxPoll { work, resched, complete: false };
    assert!(p(64, false).repoll(64, 1023)); // budget used up: always poll again
    assert!(p(64, true).repoll(64, 1023));
    assert!(!p(63, false).repoll(64, 0)); // done
    assert!(p(10, true).repoll(64, RX_LOW_WATER - 1)); // short refill, nearly starved
    assert!(!p(10, true).repoll(64, RX_LOW_WATER)); // short refill, enough posted
}
