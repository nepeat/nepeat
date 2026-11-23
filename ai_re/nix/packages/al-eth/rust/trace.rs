// SPDX-License-Identifier: GPL-2.0

//! MMIO trace of the Rust al_eth, for diffing it against the C driver on the
//! hardware (../trace: al_eth_trace, the C recorder; ../trace/parse.py,
//! ../difftest --hw). Built only with `--cfg al_eth_trace` (Kbuild
//! AL_ETH_TRACE=1, module name al_eth_rs); otherwise every call here is an
//! empty inline function and [Handle] a zero-sized type.
//!
//! Records are the C recorder's: 16 bytes (op, bar, ctx, cpu, pid, off, val),
//! claimed with an atomic counter, written with relaxed atomic stores (any
//! context, no lock, no allocation). Read back from debugfs
//! `al_eth_rs/trace` as binary: a 32-byte header ("ALRT", version 1,
//! records, dropped, the three BAR lengths), then the records;
//! ../trace/rs2txt.py turns that into the C's text format. Context ([Ctx]) is
//! chosen by the driver per register window (the kernel has no binding for
//! in_interrupt()): the port's windows are process context, the datapath ones
//! (NAPI, xmit, rx mode under the address lock, management IRQ) are " @".
//! Per device: created at probe, gone when the device goes (copy it first, as
//! with the C's /proc/al_eth_trace).

/// Where an access happens (" [pid]" or " @" in the text form).
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Ctx {
    Task,
    Atomic,
}

/// Entry marks, numbered as the C's (al_eth_trace.h).
#[derive(Clone, Copy)]
#[allow(dead_code)]
pub(crate) enum Mark {
    Probe = 0,
    Open = 1,
    Close = 2,
    Remove = 3,
    RxMode = 4,
    Mtu = 5,
    SetMac = 6,
    Stats = 7,
    Bars = 8,
}

pub(crate) use imp::{Files, Handle};

#[cfg(al_eth_trace)]
mod imp {
    use super::{Ctx, Mark};
    use kernel::{
        debugfs::{BinaryWriter, Dir, File},
        fs::file,
        prelude::*,
        sync::{
            atomic::{ordering::Relaxed, Atomic},
            Arc,
        },
        uaccess::UserSliceWriter,
    };

    const RECORDS: usize = 1 << 20; // 16 MiB
    const HEADER: usize = 32;

    pub(crate) struct Trace {
        recs: KVVec<[Atomic<u32>; 4]>,
        n: Atomic<u32>,
        dropped: Atomic<u32>,
        bar_len: [Atomic<u32>; 3],
    }

    impl Trace {
        fn put(&self, op: u8, bar: u8, off: u32, val: u32, ctx: Ctx) {
            let i = self.n.fetch_add(1, Relaxed) as usize;
            let Some(r) = self.recs.get(i) else {
                self.dropped.fetch_add(1, Relaxed);
                return;
            };
            let cpu = u32::from(kernel::cpu::CpuId::current()) & 0xff;
            let (c, pid) = match ctx {
                Ctx::Task => (0u32, kernel::current!().pid() as u32),
                Ctx::Atomic => (1u32, 0),
            };
            r[1].store(pid, Relaxed);
            r[2].store(off, Relaxed);
            r[3].store(val, Relaxed);
            r[0].store(u32::from(op) | u32::from(bar) << 8 | c << 16 | cpu << 24, Relaxed);
        }

        fn count(&self) -> usize {
            (self.n.load(Relaxed) as usize).min(self.recs.len())
        }
    }

    /// The read side: header + records, at any offset (debugfs read).
    impl BinaryWriter for Trace {
        fn write_to_slice(&self, w: &mut UserSliceWriter, off: &mut file::Offset) -> Result<usize> {
            let n = self.count();
            let total = HEADER + n * 16;
            let mut done = 0;
            let mut chunk = [0u8; 256];
            while !w.is_empty() {
                let pos = usize::try_from(*off).map_err(|_| EINVAL)?;
                if pos >= total {
                    break;
                }
                let (base, len) = if pos < HEADER {
                    let h = [
                        u32::from_le_bytes(*b"ALRT"),
                        1,
                        n as u32,
                        self.dropped.load(Relaxed),
                        self.bar_len[0].load(Relaxed),
                        self.bar_len[1].load(Relaxed),
                        self.bar_len[2].load(Relaxed),
                        0,
                    ];
                    for (d, v) in chunk.chunks_exact_mut(4).zip(h) {
                        d.copy_from_slice(&v.to_le_bytes());
                    }
                    (0, HEADER)
                } else {
                    let first = (pos - HEADER) / 16;
                    let cnt = (n - first).min(chunk.len() / 16);
                    for (j, d) in chunk.chunks_exact_mut(16).take(cnt).enumerate() {
                        for (k, w4) in d.chunks_exact_mut(4).enumerate() {
                            w4.copy_from_slice(&self.recs[first + j][k].load(Relaxed).to_le_bytes());
                        }
                    }
                    (HEADER + first * 16, cnt * 16)
                };
                let start = pos - base;
                let take = (len - start).min(w.len());
                w.write_slice(&chunk[start..start + take])?;
                *off += take as file::Offset;
                done += take;
            }
            Ok(done)
        }
    }

    /// A device's trace (cheap to clone: shared).
    #[derive(Clone)]
    pub(crate) struct Handle(Arc<Trace>);

    impl Handle {
        pub(crate) fn new() -> Result<Self> {
            let mut recs = KVVec::with_capacity(RECORDS, GFP_KERNEL)?;
            for _ in 0..RECORDS {
                recs.push([Atomic::new(0), Atomic::new(0), Atomic::new(0), Atomic::new(0)], GFP_KERNEL)?;
            }
            let t = Trace { recs, n: Atomic::new(0), dropped: Atomic::new(0), bar_len: [Atomic::new(0), Atomic::new(0), Atomic::new(0)] };
            Ok(Self(Arc::new(t, GFP_KERNEL)?))
        }
        #[inline]
        pub(crate) fn rw(&self, write: bool, bar: u8, off: usize, val: u32, ctx: Ctx) {
            self.0.put(if write { b'W' } else { b'R' }, bar, off as u32, val, ctx);
        }
        #[inline]
        pub(crate) fn cfg(&self, write: bool, off: u16, val: u32) {
            self.0.put(if write { b'c' } else { b'C' }, b'p', u32::from(off), val, Ctx::Task);
        }
        #[inline]
        pub(crate) fn delay(&self, us: u32) {
            self.0.put(b'D', b'?', 0, us, Ctx::Task);
        }
        #[inline]
        pub(crate) fn mark(&self, m: Mark, ctx: Ctx) {
            self.0.put(b'K', b'?', 0, m as u32, ctx);
        }
        pub(crate) fn bar_lens(&self, lens: [u64; 3]) {
            for (a, l) in self.0.bar_len.iter().zip(lens) {
                a.store(l as u32, Relaxed);
            }
        }
        /// The debugfs file of this trace.
        pub(crate) fn files(&self) -> Result<Files> {
            let dir = Dir::new(c"al_eth_rs");
            let file = KBox::pin_init(dir.read_binary_file(c"trace", self.0.clone()), GFP_KERNEL)?;
            Ok(Files { _file: file, _dir: dir })
        }
    }

    /// The debugfs entries (removed when dropped).
    pub(crate) struct Files {
        _file: Pin<KBox<File<Arc<Trace>>>>,
        _dir: Dir,
    }
}

#[cfg(not(al_eth_trace))]
mod imp {
    use super::{Ctx, Mark};
    use kernel::prelude::*;

    #[derive(Clone)]
    pub(crate) struct Handle;

    impl Handle {
        #[inline(always)]
        pub(crate) fn new() -> Result<Self> {
            Ok(Self)
        }
        #[inline(always)]
        pub(crate) fn rw(&self, _write: bool, _bar: u8, _off: usize, _val: u32, _ctx: Ctx) {}
        #[inline(always)]
        pub(crate) fn cfg(&self, _write: bool, _off: u16, _val: u32) {}
        #[inline(always)]
        pub(crate) fn delay(&self, _us: u32) {}
        #[inline(always)]
        pub(crate) fn mark(&self, _m: Mark, _ctx: Ctx) {}
        #[inline(always)]
        pub(crate) fn bar_lens(&self, _lens: [u64; 3]) {}
        #[inline(always)]
        pub(crate) fn files(&self) -> Result<Files> {
            Ok(Files)
        }
    }

    pub(crate) struct Files;
}
