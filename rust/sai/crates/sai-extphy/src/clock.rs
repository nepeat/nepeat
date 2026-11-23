//! Time for the runtime: the link gate's poll period and hold-off, the duration counters and the
//! waits between the reads of a `P` operation. Injected, so that tests run on a clock they move
//! themselves and never sleep.
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// A monotonic clock in microseconds, and a way to wait.
pub trait Clock: Send + Sync {
    /// Microseconds since some fixed start.
    fn now_us(&self) -> u64;
    /// Waits `us` microseconds (the interval of a `P` operation).
    fn wait_us(&self, us: u32);
}

/// The process's monotonic clock. Waits of up to 2000 us are spun, longer ones slept: the
/// appliance kernels round a sleep up to a 4 ms tick (the C++ `wait_us`).
#[derive(Clone, Copy, Debug)]
pub struct SystemClock {
    start: Instant,
}

impl SystemClock {
    /// A clock that starts now.
    pub fn new() -> Self {
        Self { start: Instant::now() }
    }
}

impl Default for SystemClock {
    fn default() -> Self {
        Self::new()
    }
}

/// Waits shorter than this are spun.
const SPIN_MAX_US: u32 = 2000;

impl Clock for SystemClock {
    fn now_us(&self) -> u64 {
        u64::try_from(self.start.elapsed().as_micros()).unwrap_or(u64::MAX)
    }

    fn wait_us(&self, us: u32) {
        if us == 0 {
            return;
        }
        if us > SPIN_MAX_US {
            std::thread::sleep(Duration::from_micros(u64::from(us)));
            return;
        }
        let end = self.now_us().saturating_add(u64::from(us));
        while self.now_us() < end {
            std::hint::spin_loop();
        }
    }
}

/// A clock that moves only when told to (and by the waits it is asked for): for tests. Clones
/// share one time.
#[derive(Clone, Debug, Default)]
pub struct ManualClock(Arc<AtomicU64>);

impl ManualClock {
    /// A clock at 0.
    pub fn new() -> Self {
        Self::default()
    }

    /// Moves the clock forward.
    pub fn advance_us(&self, us: u64) {
        self.0.fetch_add(us, Ordering::SeqCst);
    }

    /// Moves the clock forward by milliseconds.
    pub fn advance_ms(&self, ms: u64) {
        self.advance_us(ms.saturating_mul(1000));
    }
}

impl Clock for ManualClock {
    fn now_us(&self) -> u64 {
        self.0.load(Ordering::SeqCst)
    }

    fn wait_us(&self, us: u32) {
        self.advance_us(u64::from(us));
    }
}
