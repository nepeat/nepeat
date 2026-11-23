// SPDX-License-Identifier: GPL-2.0

//! The DMC-12 CPLD board-reset sequence, as plain logic over a byte-write
//! trait: no kernel types, so it also compiles and is unit-tested on the build
//! host (`rustc --test sequence.rs`, run by the package's check phase).
//!
//! Hardware facts (ai_re devices/dmc-12-48r-as0/vendor-tooling.md): the CPLD is
//! at 0x68; writing 0x29 <- 1 and then 0x28 <- 1 resets the whole board, the
//! sequence of the factory kernel's al_dmc12_restart().

/// 7-bit I2C address of the board CPLD.
pub(crate) const CPLD_ADDR: u16 = 0x68;

/// `(register, value)` writes that reset the board, in order.
pub(crate) const RESET_SEQUENCE: [(u8, u8); 2] = [(0x29, 0x01), (0x28, 0x01)];

/// How long the CPLD gets to pull the reset before the caller gives up.
pub(crate) const RESET_WAIT_MS: u32 = 2000;

/// An SMBus "write byte data" capable bus.
pub(crate) trait ByteWrite {
    /// Transfer error.
    type Error;

    /// Writes `val` to register `reg` of the device at `addr`.
    fn write_byte_data(&mut self, addr: u16, reg: u8, val: u8) -> Result<(), Self::Error>;
}

/// Writes the reset sequence; stops at the first failed write.
pub(crate) fn board_reset<B: ByteWrite>(bus: &mut B) -> Result<(), B::Error> {
    for (reg, val) in RESET_SEQUENCE {
        bus.write_byte_data(CPLD_ADDR, reg, val)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Records writes; fails the `fail_at`-th one (0-based).
    struct Fake {
        writes: Vec<(u16, u8, u8)>,
        fail_at: Option<usize>,
    }

    impl ByteWrite for Fake {
        type Error = i32;

        fn write_byte_data(&mut self, addr: u16, reg: u8, val: u8) -> Result<(), i32> {
            if self.fail_at == Some(self.writes.len()) {
                return Err(-6);
            }
            self.writes.push((addr, reg, val));
            Ok(())
        }
    }

    fn run(fail_at: Option<usize>) -> (Result<(), i32>, Vec<(u16, u8, u8)>) {
        let mut f = Fake { writes: Vec::new(), fail_at };
        let r = board_reset(&mut f);
        (r, f.writes)
    }

    #[test]
    fn writes_the_factory_sequence_in_order() {
        let (r, w) = run(None);
        assert_eq!(r, Ok(()));
        assert_eq!(w, [(0x68, 0x29, 0x01), (0x68, 0x28, 0x01)]);
    }

    #[test]
    fn first_write_failing_writes_nothing_more() {
        let (r, w) = run(Some(0));
        assert_eq!(r, Err(-6));
        assert!(w.is_empty());
    }

    #[test]
    fn waits_as_long_as_the_c_module() {
        assert_eq!(RESET_WAIT_MS, 2000); // mdelay(2000) in dmc12_cpld_reset.c
    }

    #[test]
    fn second_write_failing_is_reported() {
        let (r, w) = run(Some(1));
        assert_eq!(r, Err(-6));
        assert_eq!(w, [(0x68, 0x29, 0x01)]);
    }
}
