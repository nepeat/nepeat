//! The pointer helpers: every C pointer becomes a reference, a slice or a write-only slot
//! here, or the call fails with `SAI_STATUS_INVALID_PARAMETER`. Nothing else in the crate
//! dereferences a pointer it got from C (callbacks aside).
//!
//! What cannot be checked (that a non-null pointer really points to live memory of the
//! right size) is each function's "# Safety" contract; it is the contract the SAI headers
//! give the C caller, passed on unchanged by the exported functions.
use crate::Status;
use core::marker::PhantomData;
use core::ptr::NonNull;
use sai_sys as sys;

const INVALID: Status = sys::SAI_STATUS_INVALID_PARAMETER;

/// An input the callee only reads.
///
/// # Safety
/// `p` is null, or points to an initialised `T` that is not written to for `'a` (the duration
/// of the C call that passed it).
pub(crate) unsafe fn in_ref<'a, T>(p: *const T) -> Result<&'a T, Status> {
    if !p.is_aligned() {
        return Err(INVALID);
    }
    // SAFETY: aligned (checked); non-null is checked by as_ref; initialised and not written
    // to for 'a by this function's contract.
    unsafe { p.as_ref() }.ok_or(INVALID)
}

/// An input array given as pointer and count. A zero count yields the empty slice whatever
/// the pointer is (C callers pass NULL, or anything, with a zero count); a null pointer with
/// a non-zero count is an error.
///
/// # Safety
/// When `count` is not zero and `p` is not null, `p` points to `count` initialised `T`s that
/// are not written to for `'a`.
pub(crate) unsafe fn in_slice<'a, T>(p: *const T, count: u32) -> Result<&'a [T], Status> {
    if count == 0 {
        return Ok(&[]);
    }
    if p.is_null() || !p.is_aligned() {
        return Err(INVALID);
    }
    let len = usize::try_from(count).map_err(|_| INVALID)?;
    // SAFETY: non-null and aligned (checked); `len` initialised elements, unwritten for 'a,
    // by this function's contract; an existing C array is at most isize::MAX bytes.
    Ok(unsafe { core::slice::from_raw_parts(p, len) })
}

/// An array the callee reads and writes in place (the attribute list of a get call).
/// Zero count and null are treated as by [`in_slice`].
///
/// # Safety
/// When `count` is not zero and `p` is not null, `p` points to `count` `T`s that are valid for
/// reads and writes and that nothing else reads or writes for `'a`; every bit pattern the
/// caller left there is a valid `T`.
pub(crate) unsafe fn inout_slice<'a, T>(p: *mut T, count: u32) -> Result<&'a mut [T], Status> {
    if count == 0 {
        return Ok(&mut []);
    }
    if p.is_null() || !p.is_aligned() {
        return Err(INVALID);
    }
    let len = usize::try_from(count).map_err(|_| INVALID)?;
    // SAFETY: non-null and aligned (checked); `len` valid elements with exclusive access for
    // 'a by this function's contract.
    Ok(unsafe { core::slice::from_raw_parts_mut(p, len) })
}

/// A result slot: can be written once and never read, so memory C left uninitialised is
/// never looked at.
pub(crate) struct Out<'a, T> {
    slot: NonNull<T>,
    _exclusive: PhantomData<&'a mut T>,
}

impl<T> Out<'_, T> {
    /// Stores the result.
    pub(crate) fn write(self, value: T) {
        // SAFETY: `slot` is non-null, aligned and valid for a write of T, and nothing else
        // accesses it meanwhile: the conditions [`out`] checked or was promised when it made
        // this value, which is the only way to make one.
        unsafe { self.slot.as_ptr().write(value) }
    }
}

/// An output parameter.
///
/// # Safety
/// `p` is null, or valid for a write of a `T`, and nothing else reads or writes it for `'a`.
pub(crate) unsafe fn out<'a, T>(p: *mut T) -> Result<Out<'a, T>, Status> {
    if !p.is_aligned() {
        return Err(INVALID);
    }
    NonNull::new(p)
        .map(|slot| Out {
            slot,
            _exclusive: PhantomData,
        })
        .ok_or(INVALID)
}

/// SAI's list convention for results: `count` holds the buffer's capacity on entry and the
/// number of items on return. When the items do not fit, nothing is written, `count` is set
/// to the number needed and the answer is `SAI_STATUS_BUFFER_OVERFLOW`, so the caller can
/// come back with a larger buffer. An empty result needs no buffer (`list` may be null).
///
/// # Safety
/// When `*count` is not zero and `list` is not null, `list` is valid for writes of `*count`
/// `T`s and nothing else accesses them during the call.
pub(crate) unsafe fn write_list<T: Copy>(count: &mut u32, list: *mut T, items: &[T]) -> Result<(), Status> {
    let capacity = *count;
    let needed = u32::try_from(items.len()).map_err(|_| sys::SAI_STATUS_FAILURE)?;
    *count = needed;
    if needed == 0 {
        return Ok(());
    }
    if capacity < needed || list.is_null() {
        return Err(sys::SAI_STATUS_BUFFER_OVERFLOW);
    }
    if !list.is_aligned() {
        return Err(INVALID);
    }
    // SAFETY: `list` is non-null and aligned (checked) and valid for `capacity` >= items.len()
    // writes by this function's contract; `items` is a Rust slice, which cannot overlap a
    // buffer the C caller has exclusive use of.
    unsafe { core::ptr::copy_nonoverlapping(items.as_ptr(), list, items.len()) };
    Ok(())
}

#[cfg(test)]
// The unsafe blocks of the tests pass null, zero counts or pointers to live locals of the
// stated length; one comment here instead of one per assertion.
#[allow(clippy::undocumented_unsafe_blocks, clippy::cast_ptr_alignment)]
mod tests {
    use super::*;
    use core::ptr;

    #[test]
    fn inputs() {
        let x = 7u32;
        // SAFETY (all blocks of this test): pointers are null, dangling with a zero count, or
        // to live locals of the right type and length.
        assert_eq!(unsafe { in_ref(&raw const x) }, Ok(&7));
        assert_eq!(unsafe { in_ref::<u32>(ptr::null()) }, Err(INVALID));
        let a = [1u64, 2, 3];
        assert_eq!(unsafe { in_slice(a.as_ptr(), 3) }, Ok(&a[..]));
        assert_eq!(unsafe { in_slice(a.as_ptr(), 2) }, Ok(&a[..2]));
        // zero count: the pointer is not looked at
        assert_eq!(unsafe { in_slice::<u64>(ptr::null(), 0) }, Ok(&[][..]));
        assert_eq!(unsafe { in_slice::<u64>(ptr::dangling(), 0) }, Ok(&[][..]));
        assert_eq!(unsafe { in_slice::<u64>(ptr::without_provenance(0x1), 0) }, Ok(&[][..]));
        // null with a count
        assert_eq!(unsafe { in_slice::<u64>(ptr::null(), 3) }, Err(INVALID));
        // misaligned
        let bytes = [0u8; 16];
        let odd = bytes.as_ptr().wrapping_add(1).cast::<u64>();
        if !odd.is_aligned() {
            assert_eq!(unsafe { in_slice(odd, 1) }, Err(INVALID));
            assert_eq!(unsafe { in_ref(odd) }, Err(INVALID));
        }
    }

    #[test]
    fn outputs() {
        let mut slot = core::mem::MaybeUninit::<u64>::uninit();
        // SAFETY: a live local, written before it is read.
        unsafe { out(slot.as_mut_ptr()) }.unwrap().write(42);
        // SAFETY: written just above.
        assert_eq!(unsafe { slot.assume_init() }, 42);
        // SAFETY: null is allowed by the contract (and refused).
        assert!(unsafe { out::<u64>(ptr::null_mut()) }.is_err());

        let mut a = [9u32; 4];
        // SAFETY: a live local array of 4.
        let s = unsafe { inout_slice(a.as_mut_ptr(), 4) }.unwrap();
        s[1] = 1;
        assert_eq!(a, [9, 1, 9, 9]);
        // SAFETY: zero count / null, as the contract allows.
        assert_eq!(unsafe { inout_slice::<u32>(ptr::null_mut(), 0) }.unwrap(), []);
        // SAFETY: as above.
        assert_eq!(unsafe { inout_slice::<u32>(ptr::null_mut(), 1) }.unwrap_err(), INVALID);
    }

    #[test]
    fn list_results_follow_the_buffer_overflow_convention() {
        let items = [10u64, 20, 30];
        let mut buf = [0u64; 4];

        // fits: written, count = number of items
        let mut count = 4;
        // SAFETY (all blocks of this test): `buf` is a live local of at least `count` elements,
        // or the pointer is null.
        assert_eq!(unsafe { write_list(&mut count, buf.as_mut_ptr(), &items) }, Ok(()));
        assert_eq!((count, buf), (3, [10, 20, 30, 0]));

        // exact fit
        let (mut count, mut buf3) = (3, [0u64; 3]);
        assert_eq!(unsafe { write_list(&mut count, buf3.as_mut_ptr(), &items) }, Ok(()));
        assert_eq!((count, buf3), (3, items));

        // too small: nothing written, count = needed
        let (mut count, mut small) = (2, [7u64; 2]);
        assert_eq!(
            unsafe { write_list(&mut count, small.as_mut_ptr(), &items) },
            Err(sys::SAI_STATUS_BUFFER_OVERFLOW)
        );
        assert_eq!((count, small), (3, [7, 7]));

        // the size query: zero capacity and a null buffer
        let mut count = 0;
        assert_eq!(
            unsafe { write_list(&mut count, ptr::null_mut(), &items) },
            Err(sys::SAI_STATUS_BUFFER_OVERFLOW)
        );
        assert_eq!(count, 3);

        // a capacity with a null buffer cannot be written to
        let mut count = 8;
        assert_eq!(
            unsafe { write_list(&mut count, ptr::null_mut(), &items) },
            Err(sys::SAI_STATUS_BUFFER_OVERFLOW)
        );
        assert_eq!(count, 3);

        // an empty result needs no buffer and reports zero
        let mut count = 5;
        assert_eq!(unsafe { write_list::<u64>(&mut count, ptr::null_mut(), &[]) }, Ok(()));
        assert_eq!(count, 0);
    }
}
