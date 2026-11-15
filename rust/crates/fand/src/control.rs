//! Control building blocks: smoothing, hysteresis, PI(D), ramp limiting and
//! the failsafe guard. All of them take explicit time so they are testable
//! with simulated clocks.

/// Exponential moving average with a time constant (seconds). tau <= 0
/// disables smoothing.
#[derive(Debug, Clone)]
pub struct Ema {
    pub tau: f64,
    value: Option<f64>,
}

impl Ema {
    pub fn new(tau: f64) -> Self {
        Ema { tau, value: None }
    }

    pub fn update(&mut self, x: f64, dt: f64) -> f64 {
        let v = match self.value {
            Some(prev) if self.tau > 0.0 && dt >= 0.0 => {
                let alpha = 1.0 - (-dt / self.tau).exp();
                prev + alpha * (x - prev)
            }
            _ => x,
        };
        self.value = Some(v);
        v
    }

    pub fn reset(&mut self) {
        self.value = None;
    }
}

/// Input deadband for curves: rising temperatures pass straight through, a
/// falling temperature only takes effect once it has dropped more than
/// `band` below the level that is currently held. This stops a temperature
/// that jitters across a curve point from making the fans hunt.
#[derive(Debug, Clone)]
pub struct Hysteresis {
    pub band: f64,
    held: Option<f64>,
}

impl Hysteresis {
    pub fn new(band: f64) -> Self {
        Hysteresis { band, held: None }
    }

    pub fn apply(&mut self, t: f64) -> f64 {
        let out = match self.held {
            Some(h) if t <= h && t >= h - self.band => h,
            Some(h) if t < h - self.band => t + self.band,
            _ => t,
        };
        self.held = Some(out);
        out
    }

    pub fn reset(&mut self) {
        self.held = None;
    }
}

/// PI(D) controller holding a temperature setpoint. The output is a duty in
/// percent; a positive error (too hot) raises it. The integral term carries
/// the operating point (it starts at the initial duty) and is clamped to the
/// output range; the derivative acts on the (filtered) measurement, not the
/// error.
///
/// Anti-windup is back-calculation ([`Pid::track`]): the duty the fans
/// actually ran at is fed back, and the integral moves by
/// `(applied - output) * dt / Tt` with the tracking time `Tt = Ti = kp/ki`.
/// That choice makes the correction cancel the proportional (and
/// derivative) part exactly, so while something else holds the fans
/// (another zone winning the max, a ramp or dwell, an override, the output
/// clamp) the integral relaxes toward the applied duty with the integral
/// time instead of integrating an error the fans never acted on. While the
/// controller's own output is what runs, the correction is zero and it is a
/// plain PI(D). A faster `Tt` would also absorb the P term, so a zone sitting
/// below its setpoint would wind *up* toward the applied duty and push the
/// fans up before it is warm; a slower one leaves part of the windup.
#[derive(Debug, Clone)]
pub struct Pid {
    pub setpoint: f64,
    pub kp: f64,
    pub ki: f64,
    pub kd: f64,
    /// Derivative filter time constant (seconds).
    pub d_tau: f64,
    integral: Option<f64>,
    prev_pv: Option<f64>,
    d_filtered: f64,
    /// Unclamped output of the last update and the duty that actually ran
    /// (`track`); the next update consumes both.
    last_out: Option<f64>,
    applied: Option<f64>,
}

impl Pid {
    pub fn new(setpoint: f64, kp: f64, ki: f64, kd: f64, d_tau: f64) -> Self {
        Pid {
            setpoint,
            kp,
            ki,
            kd,
            d_tau,
            integral: None,
            prev_pv: None,
            d_filtered: 0.0,
            last_out: None,
            applied: None,
        }
    }

    /// Bumpless (re)start: make the next output continue from `duty`.
    pub fn reset_to(&mut self, duty: f64, pv: Option<f64>) {
        let p = pv.map_or(0.0, |pv| self.kp * (pv - self.setpoint));
        self.integral = Some(duty - p);
        self.prev_pv = pv;
        self.d_filtered = 0.0;
        self.last_out = None;
        self.applied = None;
    }

    pub fn initialized(&self) -> bool {
        self.integral.is_some()
    }

    /// Advance by `dt` seconds with measurement `pv`; output is within [lo, hi].
    pub fn update(&mut self, pv: f64, dt: f64, lo: f64, hi: f64) -> f64 {
        let (lo, hi) = if lo <= hi { (lo, hi) } else { (hi, lo) };
        let e = pv - self.setpoint;
        let p = self.kp * e;
        let mut integral = self.integral.unwrap_or(lo).clamp(lo, hi);
        let mut d = 0.0;
        if dt > 0.0 {
            if let Some(prev) = self.prev_pv {
                let raw = (pv - prev) / dt;
                let alpha = if self.d_tau > 0.0 {
                    1.0 - (-dt / self.d_tau).exp()
                } else {
                    1.0
                };
                self.d_filtered += alpha * (raw - self.d_filtered);
                d = self.kd * self.d_filtered;
            }
            let mut di = self.ki * e * dt;
            // Back-calculation with Tt = Ti = kp/ki (see the type's docs);
            // dt/Ti is capped at 1, and kp = 0 (pure I, Ti = 0) tracks fully.
            if let (Some(a), Some(out)) = (self.applied, self.last_out) {
                if self.ki > 0.0 {
                    let k = if self.kp > 0.0 {
                        (dt * self.ki / self.kp).min(1.0)
                    } else {
                        1.0
                    };
                    di += k * (a - out);
                }
            }
            // The integral (the operating point) also never leaves the
            // actuator range, so P reacts at once after a saturation.
            integral = (integral + di).clamp(lo, hi);
        }
        self.applied = None;
        self.integral = Some(integral);
        self.prev_pv = Some(pv);
        let out = p + integral + d;
        self.last_out = Some(out);
        out.clamp(lo, hi)
    }

    /// Back-calculation input: the last update's output actually ran at
    /// `applied` (after ramp, max over zones and the output clamp). The next
    /// update folds the correction into its integration step, before the
    /// range clamp, so the two cancel exactly where they should. One value
    /// per update; ignored after a reset until the next update.
    pub fn track(&mut self, applied: f64) {
        if self.last_out.is_some() && self.applied.is_none() && applied.is_finite() {
            self.applied = Some(applied);
        }
    }
}

/// Slew-rate limiter with a minimum dwell before decreasing.
#[derive(Debug, Clone)]
pub struct Ramp {
    /// Max increase, %/s (<= 0 means unlimited).
    pub up: f64,
    /// Max decrease, %/s (<= 0 means unlimited).
    pub down: f64,
    /// Seconds after the last increase before a decrease may start.
    pub dwell: f64,
    last_increase: Option<f64>,
}

impl Ramp {
    pub fn new(up: f64, down: f64, dwell: f64) -> Self {
        Ramp {
            up,
            down,
            dwell,
            last_increase: None,
        }
    }

    /// Record an externally forced increase (failsafe, override).
    pub fn mark_increase(&mut self, now: f64) {
        self.last_increase = Some(now);
    }

    pub fn step(&mut self, prev: f64, target: f64, now: f64, dt: f64) -> f64 {
        let dt = dt.max(0.0);
        if target > prev {
            self.last_increase = Some(now);
            if self.up > 0.0 {
                target.min(prev + self.up * dt)
            } else {
                target
            }
        } else if target < prev {
            if self.last_increase.is_some_and(|t| now - t < self.dwell) {
                return prev;
            }
            if self.down > 0.0 {
                target.max(prev - self.down * dt)
            } else {
                target
            }
        } else {
            prev
        }
    }
}

/// What a guard update did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transition {
    None,
    Entered,
    Recovering,
    Left,
}

/// Failsafe state machine: any fault enters (or keeps) failsafe; leaving
/// requires `calm` seconds without any fault.
#[derive(Debug, Clone)]
pub struct FailGuard {
    pub calm: f64,
    state: Option<FailState>,
}

#[derive(Debug, Clone)]
pub struct FailState {
    pub since: f64,
    pub calm_since: Option<f64>,
    pub reasons: Vec<String>,
}

impl FailGuard {
    pub fn new(calm: f64) -> Self {
        FailGuard { calm, state: None }
    }

    pub fn active(&self) -> bool {
        self.state.is_some()
    }

    pub fn state(&self) -> Option<&FailState> {
        self.state.as_ref()
    }

    pub fn update(&mut self, faults: &[String], now: f64) -> Transition {
        if !faults.is_empty() {
            match &mut self.state {
                Some(s) => {
                    s.calm_since = None;
                    s.reasons = faults.to_vec();
                    Transition::None
                }
                None => {
                    self.state = Some(FailState {
                        since: now,
                        calm_since: None,
                        reasons: faults.to_vec(),
                    });
                    Transition::Entered
                }
            }
        } else if let Some(s) = &mut self.state {
            let started = s.calm_since.is_none();
            let calm_since = *s.calm_since.get_or_insert(now);
            if now - calm_since >= self.calm {
                self.state = None;
                Transition::Left
            } else if started {
                Transition::Recovering
            } else {
                Transition::None
            }
        } else {
            Transition::None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ema_converges_and_tau_zero_passes_through() {
        let mut e = Ema::new(0.0);
        assert_eq!(e.update(10.0, 1.0), 10.0);
        assert_eq!(e.update(20.0, 1.0), 20.0);
        let mut e = Ema::new(5.0);
        assert_eq!(e.update(30.0, 1.0), 30.0);
        let v = e.update(40.0, 5.0);
        assert!((v - (30.0 + 10.0 * (1.0 - (-1.0f64).exp()))).abs() < 1e-9);
        for _ in 0..100 {
            e.update(40.0, 1.0);
        }
        assert!((e.update(40.0, 1.0) - 40.0).abs() < 1e-3);
    }

    #[test]
    fn hysteresis_holds_small_drops() {
        let mut h = Hysteresis::new(2.0);
        assert_eq!(h.apply(50.0), 50.0);
        assert_eq!(h.apply(49.0), 50.0); // within band: hold
        assert_eq!(h.apply(48.0), 50.0); // at band edge: hold
        assert_eq!(h.apply(47.0), 49.0); // dropped past band: follow, offset by band
        assert_eq!(h.apply(52.0), 52.0); // rises pass straight through
        assert_eq!(h.apply(51.5), 52.0);
    }

    #[test]
    fn pid_moves_towards_setpoint_and_saturates_without_windup() {
        let mut pid = Pid::new(45.0, 4.0, 0.1, 0.0, 0.0);
        pid.reset_to(50.0, Some(45.0));
        // Too hot by 5 C: P alone adds 20 %, the integral keeps climbing.
        let a = pid.update(50.0, 1.0, 30.0, 100.0);
        let b = pid.update(50.0, 1.0, 30.0, 100.0);
        assert!(a > 50.0 && b > a, "{a} {b}");
        // Very hot for a long time: saturates at hi.
        for _ in 0..1000 {
            pid.update(80.0, 1.0, 30.0, 100.0);
        }
        assert_eq!(pid.update(80.0, 1.0, 30.0, 100.0), 100.0);
        // Anti-windup: once cool again the output must leave 100 % quickly,
        // not after unwinding a huge integral.
        let mut out = 100.0;
        for _ in 0..10 {
            out = pid.update(40.0, 1.0, 30.0, 100.0);
        }
        assert!(out < 100.0 - 15.0, "output stuck high after saturation: {out}");
    }

    #[test]
    fn pid_respects_floor_and_unwinds_from_it() {
        let mut pid = Pid::new(45.0, 4.0, 0.2, 0.0, 0.0);
        pid.reset_to(60.0, Some(30.0));
        for _ in 0..500 {
            assert!(pid.update(30.0, 1.0, 40.0, 100.0) >= 40.0);
        }
        assert_eq!(pid.update(30.0, 1.0, 40.0, 100.0), 40.0);
        // Integral sits at the floor, so heating reacts immediately.
        let out = pid.update(47.0, 1.0, 40.0, 100.0);
        assert!(out > 40.0 + 8.0 - 1e-9, "{out}");
    }

    #[test]
    fn pid_tracking_follows_the_applied_duty_without_absorbing_p() {
        // Not in control (applied above its output, too cold): the integral
        // relaxes toward the applied duty with Ti = kp/ki = 40 s, so the
        // output sits at applied + P, below applied while cold.
        let mut pid = Pid::new(45.0, 4.0, 0.1, 0.0, 0.0);
        pid.reset_to(30.0, Some(40.0));
        let mut out = 0.0;
        for _ in 0..400 {
            out = pid.update(40.0, 1.0, 20.0, 100.0);
            pid.track(80.0);
        }
        assert!((out - (80.0 - 20.0)).abs() < 0.1, "{out}");
        // Without tracking the same history sinks to the floor.
        let mut plain = Pid::new(45.0, 4.0, 0.1, 0.0, 0.0);
        plain.reset_to(30.0, Some(40.0));
        for _ in 0..200 {
            out = plain.update(40.0, 1.0, 20.0, 100.0);
        }
        assert_eq!(out, 20.0);
        // In control (applied == its output): tracking changes nothing.
        let mut a = Pid::new(45.0, 4.0, 0.1, 0.0, 0.0);
        let mut b = a.clone();
        a.reset_to(50.0, Some(46.0));
        b.reset_to(50.0, Some(46.0));
        for k in 0..50 {
            let pv = 46.0 + f64::from(k % 7) * 0.3;
            let oa = a.update(pv, 1.0, 20.0, 100.0);
            a.track(oa);
            assert_eq!(oa, b.update(pv, 1.0, 20.0, 100.0));
        }
        // At the floor while cold: the integral settles at the floor (it
        // starts at 80 here: bumpless at 20 % with P = -60), it does not
        // climb by the P term. Back at the setpoint the output is within
        // one tick's lag (ki * 15 C * 1 s) of the floor, not at 80 %.
        let mut f = Pid::new(45.0, 4.0, 0.1, 0.0, 0.0);
        f.reset_to(20.0, Some(30.0));
        for _ in 0..200 {
            let o = f.update(30.0, 1.0, 20.0, 100.0);
            f.track(o);
        }
        let o = f.update(45.0, 1.0, 20.0, 100.0);
        assert!(o < 20.0 + 1.5 + 0.5, "{o}");
    }

    #[test]
    fn pid_derivative_reacts_to_rising_measurement() {
        let mut with_d = Pid::new(45.0, 2.0, 0.0, 10.0, 0.0);
        let mut without = Pid::new(45.0, 2.0, 0.0, 0.0, 0.0);
        with_d.reset_to(50.0, Some(44.0));
        without.reset_to(50.0, Some(44.0));
        let a = with_d.update(46.0, 1.0, 0.0, 100.0);
        let b = without.update(46.0, 1.0, 0.0, 100.0);
        assert!(a > b + 10.0, "{a} {b}");
    }

    #[test]
    fn ramp_limits_rate_and_dwells_before_decreasing() {
        let mut r = Ramp::new(10.0, 1.0, 5.0);
        assert_eq!(r.step(50.0, 100.0, 0.0, 1.0), 60.0);
        assert_eq!(r.step(60.0, 100.0, 1.0, 1.0), 70.0);
        // Now the target drops: dwell 5 s after the last increase at t=1.
        assert_eq!(r.step(70.0, 40.0, 2.0, 1.0), 70.0);
        assert_eq!(r.step(70.0, 40.0, 5.9, 1.0), 70.0);
        assert_eq!(r.step(70.0, 40.0, 6.0, 1.0), 69.0);
        assert_eq!(r.step(69.0, 40.0, 8.0, 2.0), 67.0);
        assert_eq!(r.step(41.0, 40.0, 9.0, 2.0), 40.0);
        let mut unlimited = Ramp::new(0.0, 0.0, 0.0);
        assert_eq!(unlimited.step(30.0, 100.0, 0.0, 1.0), 100.0);
        assert_eq!(unlimited.step(100.0, 30.0, 1.0, 1.0), 30.0);
    }

    #[test]
    fn guard_enters_immediately_and_leaves_after_calm() {
        let mut g = FailGuard::new(60.0);
        let f = vec!["sensor x: gone".to_string()];
        assert_eq!(g.update(&[], 0.0), Transition::None);
        assert_eq!(g.update(&f, 1.0), Transition::Entered);
        assert!(g.active());
        assert_eq!(g.update(&f, 2.0), Transition::None);
        assert_eq!(g.update(&[], 3.0), Transition::Recovering);
        assert_eq!(g.update(&[], 40.0), Transition::None);
        // A fault during the calm period restarts it.
        assert_eq!(g.update(&f, 41.0), Transition::None);
        assert_eq!(g.update(&[], 42.0), Transition::Recovering);
        assert_eq!(g.update(&[], 101.9), Transition::None);
        assert!(g.active());
        assert_eq!(g.update(&[], 102.0), Transition::Left);
        assert!(!g.active());
    }
}
