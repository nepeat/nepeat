//! IFCS object handles: 31..25 module, 24..21 object class, 20..0 value (the header's
//! `IFCS_HANDLE` macros, which bindgen cannot translate).
#![forbid(unsafe_code)]

use ifcs_sys as sys;

/// An IFCS object handle (`ifcs_handle_t`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Handle(pub sys::ifcs_handle_t);

impl Handle {
    /// `IFCS_NULL_HANDLE`.
    pub const NULL: Self = Self(sys::IFCS_NULL_HANDLE);

    /// `IFCS_HANDLE(mod, cls, val)`.
    pub const fn new(module: u32, class: u32, value: u32) -> Self {
        Self(
            ((module & 127) << sys::IFCS_HANDLE_MODULE_SHIFT)
                | ((class & 15) << sys::IFCS_HANDLE_OBJECT_SHIFT)
                | (value & sys::IFCS_HANDLE_VALUE_MAX),
        )
    }

    /// `IFCS_HANDLE_VALUE(h)`.
    pub const fn value(self) -> u32 {
        self.0 & sys::IFCS_HANDLE_VALUE_MAX
    }

    /// The module bits (`h >> IFCS_HANDLE_MODULE_SHIFT`, as `tl_l2.cpp` reads an L2VNI).
    pub const fn module(self) -> u32 {
        self.0 >> sys::IFCS_HANDLE_MODULE_SHIFT
    }

    /// `IFCS_HANDLE_SYSPORT(v)`.
    pub const fn sysport(v: u32) -> Self {
        Self::new(sys::IFCS_MOD_SYSPORT, sys::IFCS_API_CLASS_SYSPORT, v)
    }

    /// `IFCS_HANDLE_L2VNI(v)`.
    pub const fn l2vni(v: u32) -> Self {
        Self::new(sys::IFCS_MOD_L2VNI, sys::IFCS_API_CLASS_L2VNI, v)
    }

    /// `IFCS_HANDLE_L3VNI(v)`.
    pub const fn l3vni(v: u32) -> Self {
        Self::new(sys::IFCS_MOD_L3VNI, sys::IFCS_API_CLASS_L3VNI, v)
    }

    /// `IFCS_HANDLE_HOSTIF_TRAP(v)`.
    pub const fn hostif_trap(v: u32) -> Self {
        Self::new(sys::IFCS_MOD_HOSTIF, sys::IFCS_API_CLASS_HOSTIF_TRAP, v)
    }

    /// Whether this is the null handle.
    pub const fn is_null(self) -> bool {
        self.0 == sys::IFCS_NULL_HANDLE
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The values the C macros give, as the C++ tests and the mock see them.
    #[test]
    fn handles_are_the_macros() {
        assert_eq!(Handle::sysport(1).0, 0x3e20_0001); // test_ifcs: "sp=0x3e200001"
        assert_eq!(Handle::sysport(1000).0, 0x3e20_03e8); // the CPU sysport
        assert_eq!(Handle::hostif_trap(23).0, 0x1820_0017); // L3MTU_CHECK_FAIL
        assert_eq!(Handle::l2vni(1).0, 0x2420_0001);
        assert_eq!(Handle::l3vni(256).0, 0x2620_0100);
        assert_eq!(Handle::l2vni(4094).value(), 4094);
        assert_eq!(Handle::l2vni(5).module(), sys::IFCS_MOD_L2VNI);
        assert_eq!(
            Handle::new(0xc8, 0x14, 0x3f_ffff).0,
            Handle::new(0xc8 & 0x7f, 0x14 & 0xf, 0x1f_ffff).0
        );
        assert!(Handle::NULL.is_null());
    }
}
