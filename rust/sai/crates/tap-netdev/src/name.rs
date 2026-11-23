//! Interface names as the kernel accepts them (`dev_valid_name` in `net/core/dev.c`).
use core::fmt;

/// `IFNAMSIZ`: a name is at most this many bytes including its terminating NUL.
pub const IFNAMSIZ: usize = 16;

/// Why a string is not an interface name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NameError {
    /// The empty string.
    Empty,
    /// 16 bytes or more (no room for the NUL).
    TooLong,
    /// `.` or `..`.
    Reserved,
    /// A byte the kernel refuses (`/`, `:`, NUL, white space), or `%`, which would make the
    /// kernel pick a numbered name instead of this one.
    BadByte(u8),
}

impl fmt::Display for NameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => f.write_str("empty interface name"),
            Self::TooLong => write!(f, "interface name longer than {} bytes", IFNAMSIZ - 1),
            Self::Reserved => f.write_str("interface name . or .."),
            Self::BadByte(b) => write!(f, "byte 0x{b:02x} in an interface name"),
        }
    }
}

impl std::error::Error for NameError {}

/// The kernel's `isspace` (`lib/ctype.c`): ASCII white space and Latin-1 NBSP.
const fn kernel_space(b: u8) -> bool {
    matches!(b, b'\t' | b'\n' | 0x0b | 0x0c | b'\r' | b' ' | 0xa0)
}

/// A valid interface name: 1..=15 bytes, none of them refused by the kernel.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct IfName(String);

impl IfName {
    /// Checks `name`.
    pub fn new(name: &str) -> Result<Self, NameError> {
        if name.is_empty() {
            return Err(NameError::Empty);
        }
        if name.len() >= IFNAMSIZ {
            return Err(NameError::TooLong);
        }
        if name == "." || name == ".." {
            return Err(NameError::Reserved);
        }
        if let Some(b) = name
            .bytes()
            .find(|b| matches!(b, b'/' | b':' | b'%' | 0) || kernel_space(*b))
        {
            return Err(NameError::BadByte(b));
        }
        Ok(Self(name.to_owned()))
    }

    /// The name.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The name as the kernel's `char ifr_name[IFNAMSIZ]`: its bytes, then NULs.
    pub(crate) fn to_c(&self) -> [libc::c_char; IFNAMSIZ] {
        let mut out = [0; IFNAMSIZ];
        for (slot, byte) in out.iter_mut().zip(self.0.bytes()) {
            // c_char is i8 on x86_64 and u8 on powerpc64: the same byte either way.
            *slot = libc::c_char::from_ne_bytes([byte]);
        }
        out
    }
}

impl fmt::Display for IfName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn names_the_kernel_takes_and_refuses() {
        for ok in ["Ethernet0", "eth1", "a", "Ethernet124", "x-y_z.1", "123456789012345"] {
            assert_eq!(IfName::new(ok).unwrap().as_str(), ok);
        }
        assert_eq!(IfName::new(""), Err(NameError::Empty));
        assert_eq!(IfName::new("1234567890123456"), Err(NameError::TooLong));
        assert_eq!(IfName::new("."), Err(NameError::Reserved));
        assert_eq!(IfName::new(".."), Err(NameError::Reserved));
        assert_eq!(IfName::new("..."), Ok(IfName("...".into())));
        assert_eq!(IfName::new("a/b"), Err(NameError::BadByte(b'/')));
        assert_eq!(IfName::new("a:1"), Err(NameError::BadByte(b':')));
        assert_eq!(IfName::new("tap%d"), Err(NameError::BadByte(b'%')));
        assert_eq!(IfName::new("a b"), Err(NameError::BadByte(b' ')));
        assert_eq!(IfName::new("a\u{b}b"), Err(NameError::BadByte(0x0b)));
        assert_eq!(IfName::new("a\0"), Err(NameError::BadByte(0)));
        // 15 bytes of UTF-8 is fine; NBSP (C2 A0) has a byte the kernel calls white space
        assert!(IfName::new("ééééééé").is_ok());
        assert_eq!(IfName::new("a\u{a0}"), Err(NameError::BadByte(0xa0)));
    }

    #[test]
    fn the_c_form_is_the_bytes_then_nul() {
        let c = IfName::new("Ethernet16").unwrap().to_c();
        let bytes: Vec<u8> = c.iter().map(|b| b.to_ne_bytes()[0]).collect();
        assert_eq!(&bytes[..10], b"Ethernet16");
        assert!(bytes[10..].iter().all(|b| *b == 0));
    }

    proptest! {
        #[test]
        fn a_valid_name_fits_its_c_form_with_a_nul(s in "\\PC{0,20}") {
            if let Ok(name) = IfName::new(&s) {
                prop_assert!(name.as_str().len() < IFNAMSIZ);
                let c = name.to_c();
                prop_assert_eq!(c[IFNAMSIZ - 1], 0);
                let bytes: Vec<u8> = c.iter().map(|b| b.to_ne_bytes()[0]).take_while(|b| *b != 0).collect();
                prop_assert_eq!(bytes.as_slice(), s.as_bytes());
            } else {
                prop_assert!(s.is_empty() || s.len() >= IFNAMSIZ || s == "." || s == ".."
                    || s.bytes().any(|b| matches!(b, b'/' | b':' | b'%' | 0) || kernel_space(b)));
            }
        }
    }
}
