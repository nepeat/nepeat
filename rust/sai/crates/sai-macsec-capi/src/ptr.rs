//! The pointer helpers: the places where a pointer from C becomes something Rust uses.
#![allow(unsafe_code, reason = "the C-ABI rim: see the crate documentation")]

/// `n` elements at `p` as a slice. `n == 0` gives the empty slice whatever `p` is; a null or
/// misaligned `p` with `n > 0`, or a size no allocation can have, gives `None`.
///
/// # Safety
/// When `n > 0` and `p` is non-null: `p` points to `n` initialised `T` in one allocation that
/// nothing writes for `'a`.
pub(crate) unsafe fn in_slice<'a, T>(p: *const T, n: usize) -> Option<&'a [T]> {
    if n == 0 {
        return Some(&[]);
    }
    let too_big = size_of::<T>()
        .checked_mul(n)
        .is_none_or(|b| isize::try_from(b).is_err());
    if p.is_null() || !p.is_aligned() || too_big {
        return None;
    }
    // SAFETY: non-null, aligned and of a size that fits isize (checked); n initialised elements
    // that nothing writes for 'a (the caller's contract).
    Some(unsafe { std::slice::from_raw_parts(p, n) })
}

/// `n` elements at `p` for reading and writing; the same checks as [`in_slice`].
///
/// # Safety
/// When `n > 0` and `p` is non-null: `p` points to `n` initialised `T` in one allocation that
/// nothing else accesses for `'a`.
pub(crate) unsafe fn inout_slice<'a, T>(p: *mut T, n: usize) -> Option<&'a mut [T]> {
    if n == 0 {
        return Some(&mut []);
    }
    let too_big = size_of::<T>()
        .checked_mul(n)
        .is_none_or(|b| isize::try_from(b).is_err());
    if p.is_null() || !p.is_aligned() || too_big {
        return None;
    }
    // SAFETY: non-null, aligned and of a size that fits isize (checked); n initialised elements,
    // not accessed otherwise for 'a (the caller's contract).
    Some(unsafe { std::slice::from_raw_parts_mut(p, n) })
}

/// The value at `p`; `None` for a null or misaligned pointer.
///
/// # Safety
/// A non-null `p` points to an initialised `T` that nothing writes for `'a`.
pub(crate) unsafe fn in_ref<'a, T>(p: *const T) -> Option<&'a T> {
    if !p.is_aligned() {
        return None;
    }
    // SAFETY: as_ref checks for null; aligned (checked); initialised, unwritten (the contract).
    unsafe { p.as_ref() }
}

/// The value at `p` for reading and writing; `None` for a null or misaligned pointer.
///
/// # Safety
/// A non-null `p` points to an initialised `T` that nothing else accesses for `'a`.
pub(crate) unsafe fn inout_ref<'a, T>(p: *mut T) -> Option<&'a mut T> {
    if !p.is_aligned() {
        return None;
    }
    // SAFETY: as_mut checks for null; aligned (checked); initialised, not accessed otherwise (the contract).
    unsafe { p.as_mut() }
}

/// Writes `value` to `p` without reading or dropping what was there; `false` (nothing written)
/// for a null or misaligned pointer.
///
/// # Safety
/// A non-null `p` points to storage for one `T`, valid for a write, not accessed otherwise
/// during the call.
pub(crate) unsafe fn out<T>(p: *mut T, value: T) -> bool {
    if p.is_null() || !p.is_aligned() {
        return false;
    }
    // SAFETY: non-null and aligned (checked), valid for a write (the contract).
    unsafe { p.write(value) };
    true
}

/// SAI's list convention for an output list of `count` / `list`: `count` is the capacity on the
/// way in and the number of items on the way out; too small a capacity, or a null list for a
/// non-empty result, writes nothing but the count and is `BUFFER_OVERFLOW` (the C++ `fill_list`).
/// The error is a SAI status.
///
/// # Safety
/// A non-null `list` points to `*count` elements of writable storage, not accessed otherwise
/// during the call.
pub(crate) unsafe fn fill_list<T: Copy>(count: &mut u32, list: *mut T, items: &[T]) -> Result<(), i32> {
    let Ok(n) = u32::try_from(items.len()) else {
        return Err(sai_sys::SAI_STATUS_FAILURE);
    };
    if *count < n || (n > 0 && list.is_null()) {
        *count = n;
        return Err(sai_sys::SAI_STATUS_BUFFER_OVERFLOW);
    }
    // SAFETY: list has room for *count >= n elements (the contract; null only when n == 0,
    // which inout_slice accepts without looking at it).
    let Some(slots) = (unsafe { inout_slice(list, items.len()) }) else {
        return Err(sai_sys::SAI_STATUS_INVALID_PARAMETER);
    };
    slots.copy_from_slice(items);
    *count = n;
    Ok(())
}
