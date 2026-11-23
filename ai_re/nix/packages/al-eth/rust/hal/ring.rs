// SPDX-License-Identifier: GPL-2.0

//! Who owns a buffer while the hardware has it. A slot holds the buffer
//! (`B`: on the kernel side a DMA-mapped skb or page fragment that unmaps
//! itself when dropped or unmapped) from the moment its descriptor is written
//! until the completion that releases it; the only ways out are a completion
//! (in ring order) or a drain after the engine was stopped. So a buffer
//! cannot be handed back twice, used after it was handed back, or reused
//! while the device may still write it: there is no API that does any of
//! that. Storage is any `DerefMut<Target = [_]>` (KVec in the kernel, Vec in
//! tests), so a 1024-entry ring is never built on the stack.

use core::ops::DerefMut;

use crate::hal::desc::MAX_BUFS;
use crate::hal::{Error, Result};

/// Fields al_eth_start_xmit keeps in the slot's al_eth_pkt between packets:
/// set only for checksum offload, they carry over to the next packet of the
/// same slot otherwise (the C behaviour, kept so the descriptors match).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct TxSticky {
    pub l3_proto_idx: u32,
    pub l4_proto_idx: u32,
    pub source_vlan_count: u8,
}

/// One Tx slot.
#[derive(Debug)]
pub struct TxSlot<B> {
    buf: Option<B>,
    descs: u32,
    len: u32,
    sticky: TxSticky,
}

impl<B> Default for TxSlot<B> {
    fn default() -> Self {
        TxSlot { buf: None, descs: 0, len: 0, sticky: TxSticky::default() }
    }
}

/// The software Tx ring (tx_buffer_info + next_to_use/next_to_clean).
pub struct TxRing<B, S: DerefMut<Target = [TxSlot<B>]>> {
    slots: S,
    ntu: usize,
    ntc: usize,
}

impl<B, S: DerefMut<Target = [TxSlot<B>]>> TxRing<B, S> {
    /// `slots`: a power-of-two number of empty slots.
    pub fn new(slots: S) -> Result<Self> {
        if !slots.len().is_power_of_two() || slots.iter().any(|s| s.buf.is_some()) {
            return Err(Error::Invalid);
        }
        Ok(Self { slots, ntu: 0, ntc: 0 })
    }

    fn mask(&self) -> usize {
        self.slots.len() - 1
    }

    /// The sticky packet fields of the slot the next push uses.
    pub fn next_sticky(&mut self) -> &mut TxSticky {
        let i = self.ntu;
        &mut self.slots[i].sticky
    }

    /// Whether the next slot is free.
    pub fn has_room(&self) -> bool {
        self.slots[self.ntu].buf.is_none()
    }

    /// Takes ownership of a sent packet's buffer: `descs` descriptors,
    /// `len` bytes. A full ring gives the buffer back (the C would overwrite
    /// the slot and leak the skb).
    pub fn push(&mut self, buf: B, descs: u32, len: u32) -> core::result::Result<(), B> {
        let i = self.ntu;
        let s = &mut self.slots[i];
        if s.buf.is_some() || descs == 0 {
            return Err(buf);
        }
        s.buf = Some(buf);
        s.descs = descs;
        s.len = len;
        self.ntu = (i + 1) & self.mask();
        Ok(())
    }

    /// The oldest packet if all its descriptors are among `done` completed
    /// ones (al_eth_tx_poll): returns it with its length and takes its
    /// descriptors off `done`.
    pub fn complete(&mut self, done: &mut u32) -> Option<(B, u32)> {
        let i = self.ntc;
        let s = &mut self.slots[i];
        if *done == 0 || s.descs > *done {
            return None;
        }
        let b = s.buf.take()?;
        *done -= s.descs;
        let len = s.len;
        self.ntc = (i + 1) & self.mask();
        Some((b, len))
    }

    /// Packets the device has not completed yet.
    pub fn in_flight(&self) -> usize {
        self.slots.iter().filter(|s| s.buf.is_some()).count()
    }

    /// After the engine is stopped: every remaining buffer; the ring is
    /// reset (sticky fields kept, like the C keeps them until the next up).
    pub(crate) fn drain(&mut self, mut f: impl FnMut(B)) -> u32 {
        let mut n = 0;
        for s in self.slots.iter_mut() {
            if let Some(b) = s.buf.take() {
                f(b);
                n += 1;
            }
        }
        self.ntu = 0;
        self.ntc = 0;
        n
    }

    /// A new ring (al_eth_setup_tx_resources zeroes tx_buffer_info).
    pub(crate) fn reset(&mut self) -> Result {
        if self.slots.iter().any(|s| s.buf.is_some()) {
            return Err(Error::Invalid);
        }
        for s in self.slots.iter_mut() {
            *s = TxSlot::default();
        }
        self.ntu = 0;
        self.ntc = 0;
        Ok(())
    }
}

/// The software Rx ring: slot i holds the buffer of Rx descriptor i. A
/// The buffers of one multi-buffer Rx packet, in order, with their data
/// lengths: a cursor over ring slots the ring has already moved past (so
/// they belong to the packet, not the ring). Yields each buffer once; the
/// ones not taken are dropped with the cursor.
pub struct RxFrags<'a, B> {
    slots: &'a mut [Option<B>],
    lens: [u32; MAX_BUFS],
    pos: usize,
    left: usize,
    i: usize,
}

impl<B> Iterator for RxFrags<'_, B> {
    type Item = (B, u32);
    fn next(&mut self) -> Option<(B, u32)> {
        while self.left > 0 {
            let mask = self.slots.len() - 1;
            let b = self.slots[self.pos].take();
            self.pos = (self.pos + 1) & mask;
            self.left -= 1;
            let len = self.lens.get(self.i).copied().unwrap_or(0);
            self.i += 1;
            if let Some(b) = b {
                return Some((b, len));
            }
        }
        None
    }
}

impl<B> Drop for RxFrags<'_, B> {
    fn drop(&mut self) {
        for _ in self.by_ref() {}
    }
}

/// slot keeps its buffer after a packet was copied out of it (small packets,
/// errors) and the buffer is posted again.
pub struct RxRing<B, S: DerefMut<Target = [Option<B>]>> {
    slots: S,
    ntu: usize,
    ntc: usize,
}

impl<B, S: DerefMut<Target = [Option<B>]>> RxRing<B, S> {
    pub fn new(slots: S) -> Result<Self> {
        if !slots.len().is_power_of_two() || slots.iter().any(|s| s.is_some()) {
            return Err(Error::Invalid);
        }
        Ok(Self { slots, ntu: 0, ntc: 0 })
    }

    fn mask(&self) -> usize {
        self.slots.len() - 1
    }

    /// Refill one slot (al_eth_alloc_rx_frag + al_eth_rx_buffer_add): keep its
    /// buffer or allocate one, then `post` its descriptor. False when the
    /// allocation or the post failed (the buffer stays in the slot).
    pub(crate) fn refill_one(&mut self, alloc: &mut impl FnMut() -> Option<B>, post: &mut impl FnMut(&B) -> Result) -> bool {
        let i = self.ntu;
        if self.slots[i].is_none() {
            match alloc() {
                Some(b) => self.slots[i] = Some(b),
                None => return false,
            }
        }
        let ok = match &self.slots[i] {
            Some(b) => post(b).is_ok(),
            None => false,
        };
        if ok {
            self.ntu = (i + 1) & self.mask();
        }
        ok
    }

    /// The buffer at the head, staying in the ring (a small packet is
    /// copied out of it).
    pub(crate) fn head(&self) -> Option<&B> {
        self.slots[self.ntc].as_ref()
    }

    /// The same, mutably (the kernel side syncs it for the CPU and back).
    pub(crate) fn head_mut(&mut self) -> Option<&mut B> {
        self.slots[self.ntc].as_mut()
    }

    /// Takes the head buffer out (its data now belongs to an skb).
    pub(crate) fn take_head(&mut self) -> Option<B> {
        let i = self.ntc;
        let b = self.slots[i].take();
        self.ntc = (i + 1) & self.mask();
        b
    }

    /// The next `n` buffers as a cursor ([RxFrags]): the ring moves past them
    /// now, the cursor hands them out (with `lens`) and takes the ones not
    /// asked for when dropped. If one of the `n` slots is empty (the hardware
    /// reported more descriptors than were posted), the walk takes and drops
    /// the buffers up to it, moves past it too, and returns how many it took.
    pub(crate) fn take_frags(&mut self, n: usize, lens: &[u32; MAX_BUFS]) -> core::result::Result<RxFrags<'_, B>, usize> {
        let mask = self.mask();
        let start = self.ntc;
        let n = n.min(MAX_BUFS);
        if let Some(missing) = (0..n).find(|i| self.slots[(start + i) & mask].is_none()) {
            for _ in 0..=missing {
                drop(self.take_head());
            }
            return Err(missing);
        }
        self.ntc = (start + n) & mask;
        Ok(RxFrags { slots: &mut self.slots[..], lens: *lens, pos: start, left: n, i: 0 })
    }

    /// Skips `n` slots, buffers staying (error packets, copied packets).
    pub(crate) fn skip(&mut self, n: usize) {
        self.ntc = (self.ntc + n) & self.mask();
    }

    pub fn filled(&self) -> usize {
        self.slots.iter().filter(|s| s.is_some()).count()
    }

    /// After the engine is stopped: every buffer.
    pub(crate) fn drain(&mut self, mut f: impl FnMut(B)) -> u32 {
        let mut n = 0;
        for s in self.slots.iter_mut() {
            if let Some(b) = s.take() {
                f(b);
                n += 1;
            }
        }
        self.ntu = 0;
        self.ntc = 0;
        n
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    extern crate std;
    use std::vec::Vec;

    #[derive(Debug)]
    struct X(u64);

    fn xs(s: &mut u32) -> u32 {
        *s ^= *s << 13;
        *s ^= *s >> 17;
        *s ^= *s << 5;
        *s
    }

    #[test]
    fn tx_full_empty_wrap() {
        let mut r: TxRing<X, Vec<TxSlot<X>>> = TxRing::new((0..4).map(|_| TxSlot::default()).collect()).unwrap();
        for i in 0..4 {
            assert!(r.push(X(i), 1, 10).is_ok());
        }
        // full: the buffer comes back
        let back = r.push(X(9), 1, 10).unwrap_err();
        assert_eq!(back.0, 9);
        let mut done = 2;
        assert_eq!(r.complete(&mut done).unwrap().0 .0, 0);
        assert_eq!(r.complete(&mut done).unwrap().0 .0, 1);
        assert!(r.complete(&mut done).is_none());
        for i in 4..6 {
            assert!(r.push(X(i), 1, 10).is_ok());
        }
        let mut done = 100;
        let got: Vec<u64> = core::iter::from_fn(|| r.complete(&mut done).map(|(b, _)| b.0)).collect();
        assert_eq!(got, [2, 3, 4, 5]);
        assert_eq!(done, 96);
        assert_eq!(r.in_flight(), 0);
    }

    #[test]
    fn tx_partial_completion() {
        let mut r: TxRing<X, Vec<TxSlot<X>>> = TxRing::new((0..8).map(|_| TxSlot::default()).collect()).unwrap();
        r.push(X(1), 3, 1).unwrap();
        r.push(X(2), 2, 1).unwrap();
        let mut done = 2; // fewer than the first packet's 3
        assert!(r.complete(&mut done).is_none());
        assert_eq!(done, 2);
        let mut done = 4;
        assert_eq!(r.complete(&mut done).unwrap().0 .0, 1);
        assert!(r.complete(&mut done).is_none()); // 1 left, packet 2 needs 2
        assert_eq!(done, 1);
    }

    /// Model check against a VecDeque: random pushes / completions /
    /// drains; every buffer comes out exactly once, in order.
    #[test]
    fn tx_random_model() {
        let mut seed = 0x1234_5678;
        let mut r: TxRing<u64, Vec<TxSlot<u64>>> = TxRing::new((0..16).map(|_| TxSlot::default()).collect()).unwrap();
        let mut model = std::collections::VecDeque::new();
        let mut next = 0u64;
        let mut seen = std::collections::BTreeSet::new();
        for _ in 0..200_000 {
            match xs(&mut seed) % 3 {
                0 | 1 => {
                    let d = 1 + xs(&mut seed) % 4;
                    match r.push(next, d, 0) {
                        Ok(()) => model.push_back((next, d)),
                        Err(b) => {
                            assert_eq!(b, next);
                            assert_eq!(model.len(), 16);
                        }
                    }
                    next += 1;
                }
                _ => {
                    let mut done = xs(&mut seed) % 10;
                    let mut mdone = done;
                    while let Some((b, _)) = r.complete(&mut done) {
                        let (m, d) = model.pop_front().unwrap();
                        assert_eq!(b, m);
                        assert!(d <= mdone);
                        mdone -= d;
                        assert!(seen.insert(b));
                    }
                    assert_eq!(done, mdone);
                    if let Some(&(_, d)) = model.front() {
                        assert!(mdone == 0 || d > mdone);
                    }
                }
            }
            if xs(&mut seed) % 5000 == 0 {
                let mut out = Vec::new();
                r.drain(|b| out.push(b));
                let mut m: Vec<u64> = model.drain(..).map(|(b, _)| b).collect();
                out.sort();
                m.sort();
                assert_eq!(out, m);
                for b in out {
                    assert!(seen.insert(b));
                }
            }
        }
    }

    #[test]
    fn rx_refill_take_skip() {
        let mut r: RxRing<X, Vec<Option<X>>> = RxRing::new((0..4).map(|_| None).collect()).unwrap();
        let mut n = 0;
        let mut alloc = || {
            n += 1;
            Some(X(n))
        };
        let mut posted = Vec::new();
        let mut post = |b: &X| {
            posted.push(b.0);
            Ok(())
        };
        for _ in 0..3 {
            assert!(r.refill_one(&mut alloc, &mut post));
        }
        assert_eq!(r.head().unwrap().0, 1);
        r.skip(1); // small packet: slot 0 keeps buffer 1
        assert_eq!(r.take_head().unwrap().0, 2);
        assert!(r.refill_one(&mut alloc, &mut post)); // slot 3: new buffer 4
        assert!(r.refill_one(&mut alloc, &mut post)); // slot 0: buffer 1 again
        assert!(r.refill_one(&mut alloc, &mut post)); // slot 1: new buffer 5
        assert_eq!(posted, [1, 2, 3, 4, 1, 5]);
        let mut out = Vec::new();
        assert_eq!(r.drain(|b| out.push(b.0)), 4);
        out.sort();
        assert_eq!(out, [1, 3, 4, 5]);
    }

    #[test]
    fn rx_alloc_failure_keeps_position() {
        let mut r: RxRing<X, Vec<Option<X>>> = RxRing::new((0..4).map(|_| None).collect()).unwrap();
        let mut alloc = || None;
        let mut post = |_: &X| Ok(());
        assert!(!r.refill_one(&mut alloc, &mut post));
        let mut alloc = || Some(X(7));
        let mut post = |_: &X| Err(Error::NoSpace);
        assert!(!r.refill_one(&mut alloc, &mut post));
        assert_eq!(r.filled(), 1); // the buffer stays in the slot, nothing leaks
    }

    #[test]
    fn rejects_bad_storage() {
        assert!(TxRing::<X, Vec<TxSlot<X>>>::new((0..3).map(|_| TxSlot::default()).collect()).is_err());
        assert!(RxRing::<X, Vec<Option<X>>>::new(vec![Some(X(1)), None]).is_err());
    }
}
