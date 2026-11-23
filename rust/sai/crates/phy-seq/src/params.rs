//! The parameters of one run: key material. Never printed, wiped when dropped.

use std::fmt;

use zeroize::Zeroize;

use crate::op::ParamName;

const CAP: usize = 32;

#[derive(Clone)]
struct Slot {
    len: u8,
    bytes: [u8; CAP],
}

/// The bytes of the parameters a sequence may name (SAK, hash key, SCI, salt, ...).
///
/// * [`fmt::Debug`] shows which parameters are set and how long they are, never a byte, and
///   there is no other way to print one.
/// * The bytes are overwritten with zeros (volatile writes, the `zeroize` crate) when the value
///   is dropped or [`Params::wipe`]d. They live in one heap allocation that is never
///   reallocated, so moving a `Params` does not leave a copy behind. A clone is a second copy
///   with the same guarantees.
///
/// What this type cannot do: the bus words made from the bytes pass through the executor's
/// stack and the [`crate::Bus`] implementation on their way to the chip.
pub struct Params {
    slots: Box<[Slot; 8]>,
}

/// More bytes than the parameter can have ([`ParamName::max_len`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParamTooLong {
    /// The parameter.
    pub name: ParamName,
    /// The length offered.
    pub len: usize,
}

impl fmt::Display for ParamTooLong {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "parameter {}: {} bytes, at most {}",
            self.name,
            self.len,
            self.name.max_len()
        )
    }
}

impl std::error::Error for ParamTooLong {}

impl Params {
    /// No parameter set.
    pub fn new() -> Self {
        const EMPTY: Slot = Slot {
            len: 0,
            bytes: [0; CAP],
        };
        Self {
            slots: Box::new([EMPTY; 8]),
        }
    }

    /// Sets (replaces) a parameter. The previous bytes are wiped.
    pub fn set(&mut self, name: ParamName, bytes: &[u8]) -> Result<(), ParamTooLong> {
        let too_long = ParamTooLong { name, len: bytes.len() };
        let len = u8::try_from(bytes.len())
            .ok()
            .filter(|&n| n <= name.max_len())
            .ok_or(too_long)?;
        let Some(slot) = self.slots.get_mut(name.index()) else {
            return Err(too_long);
        };
        slot.bytes.zeroize();
        for (to, from) in slot.bytes.iter_mut().zip(bytes) {
            *to = *from;
        }
        slot.len = len;
        Ok(())
    }

    /// Sets a parameter from bytes that may be longer than it can be: the bytes beyond
    /// [`ParamName::max_len`] are dropped. No table can name them, so no sequence could read
    /// them (the C++ engine keeps them in its map, unread).
    pub fn set_cut(&mut self, name: ParamName, bytes: &[u8]) {
        let cut = bytes.get(..usize::from(name.max_len())).unwrap_or(bytes);
        // cannot fail: cut is at most max_len bytes
        let _ = self.set(name, cut);
    }

    /// Byte `index` of a parameter, `None` when it is not set or shorter.
    pub fn byte(&self, name: ParamName, index: u8) -> Option<u8> {
        let slot = self.slots.get(name.index())?;
        if index < slot.len {
            slot.bytes.get(usize::from(index)).copied()
        } else {
            None
        }
    }

    /// How many bytes a parameter has (0: not set).
    pub fn len(&self, name: ParamName) -> usize {
        self.slots.get(name.index()).map_or(0, |s| usize::from(s.len))
    }

    /// Whether no parameter is set.
    pub fn is_empty(&self) -> bool {
        self.slots.iter().all(|s| s.len == 0)
    }

    /// Overwrites every byte with zero and forgets every parameter (the C++ `ms_wipe`).
    pub fn wipe(&mut self) {
        for slot in &mut *self.slots {
            slot.bytes.zeroize();
            slot.len.zeroize();
        }
    }
}

impl Default for Params {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for Params {
    fn clone(&self) -> Self {
        Self {
            slots: self.slots.clone(),
        }
    }
}

impl Drop for Params {
    fn drop(&mut self) {
        self.wipe();
    }
}

/// Names and lengths only.
impl fmt::Debug for Params {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut map = f.debug_map();
        for name in ParamName::ALL {
            let len = self.len(name);
            if len > 0 {
                map.entry(&name.name(), &format_args!("<{len} bytes>"));
            }
        }
        map.finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_get_wipe() {
        let mut p = Params::new();
        assert!(p.is_empty());
        p.set(ParamName::Key, &[0xc0; 16]).unwrap();
        assert_eq!(p.len(ParamName::Key), 16);
        assert_eq!(p.byte(ParamName::Key, 15), Some(0xc0));
        assert_eq!(p.byte(ParamName::Key, 16), None);
        assert_eq!(p.byte(ParamName::Hkey, 0), None);
        // a shorter value replaces a longer one completely
        p.set(ParamName::Key, &[0xc1; 32]).unwrap();
        p.set(ParamName::Key, &[0xc2; 2]).unwrap();
        assert_eq!(p.byte(ParamName::Key, 2), None);
        assert!(p.slots[ParamName::Key.index()].bytes[2..].iter().all(|&b| b == 0));
        p.wipe();
        assert!(p.is_empty());
        assert!(p.slots.iter().all(|s| s.len == 0 && s.bytes.iter().all(|&b| b == 0)));
    }

    #[test]
    fn too_long_is_refused_or_cut() {
        let mut p = Params::new();
        assert_eq!(
            p.set(ParamName::Key, &[1; 33]),
            Err(ParamTooLong {
                name: ParamName::Key,
                len: 33
            })
        );
        assert_eq!(
            p.set(ParamName::Ssci, &[1; 5]).map_err(|e| e.to_string()).unwrap_err(),
            "parameter ssci: 5 bytes, at most 4"
        );
        assert!(p.is_empty());
        p.set_cut(ParamName::Ssci, &[1, 2, 3, 4, 5, 6]);
        assert_eq!(p.len(ParamName::Ssci), 4);
        assert_eq!(p.byte(ParamName::Ssci, 3), Some(4));
    }

    #[test]
    fn debug_never_shows_a_byte() {
        let mut p = Params::new();
        p.set(ParamName::Key, &[0xc7; 32]).unwrap();
        p.set(ParamName::Sci, &[0xc7; 8]).unwrap();
        let text = format!("{p:?} {:#?}", p.clone());
        assert!(text.contains("\"key\": <32 bytes>"), "{text}");
        assert!(!text.to_lowercase().contains("c7") && !text.contains("199"), "{text}");
    }
}
