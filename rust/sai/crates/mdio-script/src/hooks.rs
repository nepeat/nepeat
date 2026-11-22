//! Time and callbacks of a run.

use std::ops::ControlFlow;
use std::time::{Duration, Instant};

use crate::types::{Label, RegAddr, Stats, Until};

/// One assertion of the verification section, as read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Check<'a> {
    /// Script line.
    pub line: u64,
    /// `P`: want `(got & mask) == want`; `N`: `!=`.
    pub until: Until,
    /// The register read.
    pub at: RegAddr,
    /// The value compared with.
    pub want: u16,
    /// The bits compared.
    pub mask: u16,
    /// The value read (0 when the read failed).
    pub got: u16,
    /// The assertion holds.
    pub ok: bool,
    /// The read itself failed.
    pub io_error: bool,
    /// The label in force.
    pub label: &'a Label,
}

/// What a run needs besides the bus. Every method has a default, so `impl Hooks for T {}` is a
/// silent run on real time.
pub trait Hooks {
    /// Waits `us` microseconds (never called with 0). Default: [`SpinSleeper`].
    fn sleep_us(&mut self, us: u64) {
        SpinSleeper.sleep_us(us);
    }

    /// A label when its step starts, and the first 20 `E` mismatches. Default: silent.
    fn log(&mut self, msg: &str) {
        let _ = msg;
    }

    /// Whether [`Hooks::tick`] is to be called at all (C: `io->tick != NULL`). When it is, long
    /// delays are slept in pieces of 0.5 s with a tick between them.
    fn wants_tick(&self) -> bool {
        false
    }

    /// Called from the running thread before every operation, between the reads of a poll and
    /// every 0.5 s during a delay: a place to publish `progress` or do periodic work on the same
    /// bus master without a second thread. [`ControlFlow::Break`] stops the script
    /// ([`crate::RunErrorKind::Abort`]).
    fn tick(&mut self, progress: &Stats) -> ControlFlow<()> {
        let _ = progress;
        ControlFlow::Continue(())
    }

    /// Every assertion of the verification section, with the value read.
    fn verify(&mut self, check: &Check<'_>) {
        let _ = check;
    }
}

impl<H: Hooks + ?Sized> Hooks for &mut H {
    fn sleep_us(&mut self, us: u64) {
        (**self).sleep_us(us);
    }
    fn log(&mut self, msg: &str) {
        (**self).log(msg);
    }
    fn wants_tick(&self) -> bool {
        (**self).wants_tick()
    }
    fn tick(&mut self, progress: &Stats) -> ControlFlow<()> {
        (**self).tick(progress)
    }
    fn verify(&mut self, check: &Check<'_>) {
        (**self).verify(check);
    }
}

/// Hooks that do nothing but sleep on real time.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoHooks;

impl Hooks for NoHooks {}

/// The default sleeper: delays below 20 ms are spun on the monotonic clock, longer ones slept.
///
/// A kernel without high-resolution timers rounds every sleep up to a tick (4 ms on the T2080
/// appliance kernel, which made 90,000 delays of 30-200 us take 6 minutes).
#[derive(Debug, Clone, Copy, Default)]
pub struct SpinSleeper;

impl SpinSleeper {
    /// Delays shorter than this are busy-waited.
    pub const SPIN_BELOW_US: u64 = 20_000;

    /// Waits at least `us` microseconds.
    pub fn sleep_us(self, us: u64) {
        if us == 0 {
            return;
        }
        let wait = Duration::from_micros(us);
        if us < Self::SPIN_BELOW_US {
            let start = Instant::now();
            while start.elapsed() < wait {
                std::hint::spin_loop();
            }
        } else {
            std::thread::sleep(wait);
        }
    }
}
