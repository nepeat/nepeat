//! The pointer helpers: all the places where a pointer from C becomes something Rust uses.
#![allow(unsafe_code, reason = "the C-ABI rim: see the crate documentation")]

use std::ffi::c_char;
use std::mem::MaybeUninit;

/// `n` elements at `p` as a slice. `n == 0` gives the empty slice whatever `p` is; a null or
/// misaligned `p` with `n > 0`, or a size beyond what an allocation can have, gives `None`.
///
/// # Safety
/// When `n > 0` and `p` is non-null: `p` points to `n` initialised `T` in one allocation, and
/// nothing writes them for `'a`.
pub(crate) unsafe fn in_slice<'a, T>(p: *const T, n: usize) -> Option<&'a [T]> {
    if n == 0 {
        return Some(&[]);
    }
    let too_big = size_of::<T>()
        .checked_mul(n)
        .is_none_or(|bytes| isize::try_from(bytes).is_err());
    if p.is_null() || !p.is_aligned() || too_big {
        return None;
    }
    // SAFETY: non-null and aligned (checked), n elements valid and unwritten for 'a (the
    // caller's contract), and the size fits isize (checked).
    Some(unsafe { std::slice::from_raw_parts(p, n) })
}

/// The value at `p`; `None` for a null or misaligned pointer.
///
/// # Safety
/// A non-null `p` points to an initialised `T` that nothing writes for `'a`.
pub(crate) unsafe fn in_ref<'a, T>(p: *const T) -> Option<&'a T> {
    if !p.is_aligned() {
        return None;
    }
    // SAFETY: as_ref checks for null; aligned (checked); initialised and unwritten for 'a (the
    // caller's contract).
    unsafe { p.as_ref() }
}

/// Writes `value` to `p` without reading or dropping what was there. `false` (nothing written)
/// for a null or misaligned pointer.
///
/// # Safety
/// A non-null `p` points to storage for one `T` that is valid for a write and that nothing else
/// accesses during the call.
pub(crate) unsafe fn out<T>(p: *mut T, value: T) -> bool {
    if p.is_null() || !p.is_aligned() {
        return false;
    }
    // SAFETY: non-null and aligned (checked), valid for a write (the caller's contract).
    unsafe { p.write(value) };
    true
}

/// Writes `text` to the C buffer `buf` of `cap` bytes as a NUL-terminated string: cut at the
/// first NUL of `text` and to `cap - 1` bytes. Nothing is written when `buf` is null or `cap` 0.
///
/// # Safety
/// A non-null `buf` points to `cap` bytes of writable storage (initialised or not) in one
/// allocation that nothing else accesses during the call.
pub(crate) unsafe fn out_text(buf: *mut c_char, cap: usize, text: &[u8]) {
    if buf.is_null() || cap == 0 || isize::try_from(cap).is_err() {
        return;
    }
    // SAFETY: non-null (checked; c_char has alignment 1), cap bytes writable and not accessed
    // otherwise (the caller's contract); MaybeUninit<u8> makes no claim that they are
    // initialised.
    let space = unsafe { std::slice::from_raw_parts_mut(buf.cast::<MaybeUninit<u8>>(), cap) };
    let Some((last, room)) = space.split_last_mut() else {
        return;
    };
    let text = text.split(|&b| b == 0).next().unwrap_or(text);
    let mut end = Some(last);
    let mut room = room.iter_mut();
    for &b in text {
        match room.next() {
            Some(slot) => {
                slot.write(b);
            }
            None => break,
        }
    }
    // the terminator goes right after the text, or into the last byte when the text was cut
    if let Some(slot) = room.next().or_else(|| end.take()) {
        slot.write(0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slices() {
        let data = [1u16, 2, 3];
        // SAFETY: data is three initialised u16, alive and unwritten.
        assert_eq!(unsafe { in_slice(data.as_ptr(), 3) }, Some(&data[..]));
        // SAFETY: n == 0 never looks at the pointer.
        assert_eq!(unsafe { in_slice::<u16>(std::ptr::null(), 0) }, Some(&[][..]));
        // SAFETY: a null pointer is refused before it is used.
        assert_eq!(unsafe { in_slice::<u16>(std::ptr::null(), 3) }, None);
        // SAFETY: a misaligned pointer is refused before it is used.
        assert_eq!(unsafe { in_slice(data.as_ptr().wrapping_byte_add(1), 1) }, None);
        // SAFETY: a size beyond isize is refused before the pointer is used.
        assert_eq!(unsafe { in_slice(data.as_ptr(), usize::MAX / 2) }, None);
        // SAFETY: a null pointer is refused.
        assert!(unsafe { in_ref::<u32>(std::ptr::null()) }.is_none());
        let mut slot = 0u32;
        // SAFETY: slot is a live u32.
        assert!(unsafe { out(&raw mut slot, 7) });
        assert_eq!(slot, 7);
        // SAFETY: a null pointer is refused.
        assert!(!unsafe { out::<u32>(std::ptr::null_mut(), 7) });
    }

    #[test]
    fn text_is_terminated_and_cut() {
        let write = |cap: usize, text: &[u8]| {
            let mut buf = [0x55 as c_char; 8];
            // SAFETY: buf has 8 bytes and cap <= 8.
            unsafe { out_text(buf.as_mut_ptr(), cap, text) };
            buf.map(|c| c.to_ne_bytes()[0])
        };
        assert_eq!(write(8, b"abc"), *b"abc\0UUUU");
        assert_eq!(write(8, b"abcdefg"), *b"abcdefg\0");
        assert_eq!(write(8, b"abcdefgh"), *b"abcdefg\0");
        assert_eq!(write(4, b"abcdefgh"), *b"abc\0UUUU");
        assert_eq!(write(1, b"abc"), *b"\0UUUUUUU");
        assert_eq!(write(0, b"abc"), *b"UUUUUUUU");
        assert_eq!(write(8, b"ab\0cd"), *b"ab\0UUUUU");
        assert_eq!(write(8, b""), *b"\0UUUUUUU");
        // SAFETY: a null buffer is refused before it is used.
        unsafe { out_text(std::ptr::null_mut(), 8, b"abc") };
    }
}
