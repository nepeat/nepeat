//! The C structs this crate hands libifcs, kept as their bytes: `ifcs-sys`'s byte twins (its docs,
//! "By-value structs"), zeroed when made, every field written at the offset the compiler gives
//! (`offset_of!`) as its value's native-endian bytes. No padding byte of anything libifcs receives is
//! left undefined, and a copy keeps every byte (a twin is a `[u8; N]`).
//!
//! [`put_field!`] / [`get_field!`] name the field through a closure over the C struct, so the value
//! has the field's type and size (another type does not compile) and the offset is the field's.
#![forbid(unsafe_code)]

use core::mem::size_of;

pub(crate) use ifcs_sys::CBytes;

/// A field's value: its native-endian bytes.
pub(crate) trait Field: Copy {
    /// Its size in bytes (the field's).
    const SIZE: usize;
    /// Writes its bytes to `out` (`SIZE` bytes).
    fn put(self, out: &mut [u8]);
    /// Reads it from `b` (`SIZE` bytes; missing ones read as zero).
    fn get(b: &[u8]) -> Self;
}

macro_rules! int_fields {
    ($($t:ty),*) => {$(
        impl Field for $t {
            const SIZE: usize = size_of::<$t>();
            fn put(self, out: &mut [u8]) {
                for (d, s) in out.iter_mut().zip(self.to_ne_bytes()) {
                    *d = s;
                }
            }
            fn get(b: &[u8]) -> Self {
                let mut a = [0u8; size_of::<$t>()];
                for (d, s) in a.iter_mut().zip(b) {
                    *d = *s;
                }
                <$t>::from_ne_bytes(a)
            }
        }
    )*};
}
int_fields!(u8, i8, u16, i16, u32, i32, u64, i64);

impl<const N: usize> Field for [u8; N] {
    const SIZE: usize = N;
    fn put(self, out: &mut [u8]) {
        for (d, s) in out.iter_mut().zip(self) {
            *d = s;
        }
    }
    fn get(b: &[u8]) -> Self {
        let mut a = [0u8; N];
        for (d, s) in a.iter_mut().zip(b) {
            *d = *s;
        }
        a
    }
}

/// Writes `v` at `off` of `bytes` (nothing if it does not fit: offsets are the compiler's).
pub(crate) fn put<T, F: Field>(bytes: &mut [u8], off: usize, _field: fn(&T) -> &F, v: F) {
    if let Some(d) = bytes.get_mut(off..off.saturating_add(F::SIZE)) {
        v.put(d);
    }
}

/// Reads the field at `off` of `bytes`.
pub(crate) fn get<T, F: Field>(bytes: &[u8], off: usize, _field: fn(&T) -> &F) -> F {
    F::get(bytes.get(off..off.saturating_add(F::SIZE)).unwrap_or(&[]))
}

/// `put_field!(bytes, base, T, field.path, value)`: writes `value` (of the field's type) into the
/// `T` whose bytes start at `base` of `bytes` (a `&mut [u8]`).
macro_rules! put_field {
    ($b:expr, $base:expr, $t:ty, $($f:ident).+, $v:expr) => {
        $crate::bytes::put::<$t, _>($b, $base + core::mem::offset_of!($t, $($f).+), |s: &$t| &s.$($f).+, $v)
    };
}

/// `get_field!(bytes, base, T, field.path)`: the field of the `T` whose bytes start at `base`.
macro_rules! get_field {
    ($b:expr, $base:expr, $t:ty, $($f:ident).+) => {
        $crate::bytes::get::<$t, _>($b, $base + core::mem::offset_of!($t, $($f).+), |s: &$t| &s.$($f).+)
    };
}

pub(crate) use {get_field, put_field};

/// A pointer to the `M` whose bytes start at `off` of `t`'s (null if it does not fit). The callers'
/// offsets are compile-time checked to be multiples of `M`'s alignment, and `t` is aligned for its
/// struct, whose alignment is at least `M`'s.
pub(crate) fn member_ptr<T: CBytes, M>(t: &mut T, off: usize) -> *mut M {
    match t.as_bytes_mut().get_mut(off..) {
        Some(s) if s.len() >= size_of::<M>() => s.as_mut_ptr().cast(),
        _ => core::ptr::null_mut(),
    }
}

/// The `M` (a byte twin) whose bytes start at `off` of `bytes`, copied (the part that fits).
pub(crate) fn twin_at<M: CBytes>(bytes: &[u8], off: usize) -> M {
    let mut m = M::ZERO;
    for (d, s) in m.as_bytes_mut().iter_mut().zip(bytes.get(off..).unwrap_or(&[])) {
        *d = *s;
    }
    m
}

#[cfg(test)]
mod tests {
    use super::*;
    use ifcs_sys as sys;

    #[test]
    fn fields_go_to_their_offsets_and_nothing_else_changes() {
        type P = sys::ifcs_ip_prefix_t;
        let mut p = sys::ifcs_ip_prefix_t_bytes::ZERO;
        put_field!(p.as_bytes_mut(), 0, P, addr_family, 1);
        put_field!(p.as_bytes_mut(), 0, P, addr.blob, [0xab; 16]);
        put_field!(p.as_bytes_mut(), 0, P, dirty, 7);
        assert_eq!(get_field!(p.as_bytes(), 0, P, addr_family), 1);
        assert_eq!(get_field!(p.as_bytes(), 0, P, dirty), 7);
        let b = p.as_bytes();
        assert_eq!(b.len(), size_of::<P>());
        assert_eq!(b[..4], 1i32.to_ne_bytes());
        assert!(b[4..20].iter().all(|x| *x == 0xab));
        assert!(b[20..36].iter().all(|x| *x == 0)); // mask
        assert_eq!(b[36], 7);
        assert_eq!(b[37..], [0; 3]); // the padding: zero
        let m: sys::ifcs_ip_prefix_t_bytes = twin_at(b, 0);
        assert_eq!(m, p);
        let short: sys::ifcs_ip_prefix_t_bytes = twin_at(&b[30..], 0);
        assert_eq!(short.as_bytes()[..10], b[30..]);
        assert!(short.as_bytes()[10..].iter().all(|x| *x == 0));
        let mut k = sys::ifcs_route_entry_key_t_bytes::ZERO;
        let off = core::mem::offset_of!(sys::ifcs_route_entry_key_t, key);
        let mp = member_ptr::<_, sys::ifcs_route_entry_key_ip_dest_l3vni_t>(&mut k, off);
        assert_eq!(mp.cast::<u8>(), k.as_bytes_mut()[off..].as_mut_ptr());
        assert!(member_ptr::<_, sys::ifcs_route_entry_key_ip_dest_l3vni_t>(&mut k, 20).is_null());
    }
}
