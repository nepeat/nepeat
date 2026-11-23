// SPDX-License-Identifier: GPL-2.0-only
//! What the AS7760 fan CPLD's registers mean: pure functions, no kernel types,
//! unit-tested on the build host (rustc --test). The C driver
//! (../src/as7760_fan.c) is the reference; every function here names the C
//! lines it mirrors. See devices/edge-core-7760-32x/thermal.md.

/// errno values the driver returns (negated by the caller).
pub(crate) const EIO: i32 = 5;
pub(crate) const EINVAL: i32 = 22;
pub(crate) const EOPNOTSUPP: i32 = 95;

/// Registers: PWM duty (0..=15), front/rear tach base, presence bits, the
/// register the vendor clears before a duty write.
pub(crate) const REG_DUTY: u8 = 0x11;
pub(crate) const REG_FRONT: u8 = 0x12;
pub(crate) const REG_REAR: u8 = 0x22;
pub(crate) const REG_PRESENT: u8 = 0x0f;
pub(crate) const REG_PRE_WRITE: u8 = 0x33;
pub(crate) const DUTY_MAX: u8 = 15;
/// Six dual-rotor fans: channels 0..12, even = front, odd = rear.
pub(crate) const CHANNELS: usize = 12;


/// pwm1 from the duty register: raw * 17 (0..=255); a value > 15 is -EIO.
pub(crate) fn pwm_from_duty(raw: u8) -> Result<i64, i32> {
    if raw > DUTY_MAX { Err(EIO) } else { Ok(i64::from(raw) * 17) }
}

/// Tach register of a channel: (odd ? 0x22 : 0x12) + channel / 2.
pub(crate) fn tach_reg(channel: usize) -> u8 {
    (if channel % 2 == 1 { REG_REAR } else { REG_FRONT }) + (channel / 2) as u8
}

/// fanN_input in RPM: raw * 100.
pub(crate) fn rpm(raw: u8) -> i64 {
    i64::from(raw) * 100
}

/// fanN_fault: the fan module is absent (presence bit set) or the rotor reads 0.
pub(crate) fn fault(present: u8, raw: u8, channel: usize) -> i64 {
    i64::from(present & (1 << (channel / 2)) != 0 || raw == 0)
}

/// Duty for a pwm1 write: only 128..=255 (bring-up guard: no fan-off or very
/// low duty), rounded up: DIV_ROUND_UP(val * 15, 255).
pub(crate) fn duty_from_pwm(val: i64) -> Result<u8, i32> {
    if !(128..=255).contains(&val) {
        return Err(EINVAL);
    }
    Ok(((val * 15 + 254) / 255) as u8)
}

/// Probe accepts only a duty register that reads 0..=15.
pub(crate) fn probe_duty_ok(raw: u8) -> bool {
    raw <= DUTY_MAX
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pwm_matches_c() {
        for raw in 0..=255u8 {
            let c = if raw > 15 { Err(EIO) } else { Ok(i64::from(raw) * 17) };
            assert_eq!(pwm_from_duty(raw), c);
        }
    }

    #[test]
    fn duty_matches_c_div_round_up() {
        for val in -1000i64..=1000 {
            let c = if val < 128 || val > 255 { Err(EINVAL) } else { Ok(((val * 15 + 255 - 1) / 255) as u8) };
            assert_eq!(duty_from_pwm(val), c, "val {val}");
        }
        assert_eq!(duty_from_pwm(128), Ok(8));
        assert_eq!(duty_from_pwm(255), Ok(15));
    }

    #[test]
    fn registers_and_faults() {
        let regs: [u8; CHANNELS] = [0x12, 0x22, 0x13, 0x23, 0x14, 0x24, 0x15, 0x25, 0x16, 0x26, 0x17, 0x27];
        for ch in 0..CHANNELS {
            assert_eq!(tach_reg(ch), regs[ch]);
            assert_eq!(fault(0, 1, ch), 0);
            assert_eq!(fault(0, 0, ch), 1);
            assert_eq!(fault(1 << (ch / 2), 50, ch), 1);
            assert_eq!(fault(!(1 << (ch / 2)), 50, ch), 0);
        }
        assert_eq!(rpm(255), 25500);
        assert!((0..=15).all(probe_duty_ok) && !(16..=255).any(probe_duty_ok));
    }
}
