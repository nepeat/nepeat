//! Byte-buffer helpers that never index or panic (the workspace denies both): a write or read
//! outside a buffer is ignored / reads 0, which the callers' fixed-size buffers never reach.

/// Copies `src` into `buf` at `at` (nothing when it does not fit).
pub fn put(buf: &mut [u8], at: usize, src: &[u8]) {
    if let Some(dst) = at.checked_add(src.len()).and_then(|end| buf.get_mut(at..end)) {
        dst.copy_from_slice(src);
    }
}

/// Writes one byte.
pub fn put_u8(buf: &mut [u8], at: usize, v: u8) {
    put(buf, at, &[v]);
}

/// Writes a 16-bit value in network byte order (`htons` + `memcpy`).
pub fn put_be16(buf: &mut [u8], at: usize, v: u16) {
    put(buf, at, &v.to_be_bytes());
}

/// Reads one byte (0 outside the buffer).
pub fn at(buf: &[u8], i: usize) -> u8 {
    buf.get(i).copied().unwrap_or(0)
}

/// Reads `N` bytes (zeros outside the buffer).
pub fn array<const N: usize>(buf: &[u8], from: usize) -> [u8; N] {
    let mut out = [0u8; N];
    if let Some(src) = from.checked_add(N).and_then(|end| buf.get(from..end)) {
        out.copy_from_slice(src);
    }
    out
}

/// Reads a native-endian `u32` (`memcpy(&v, p, 4)`).
pub fn ne32(buf: &[u8], from: usize) -> u32 {
    u32::from_ne_bytes(array(buf, from))
}

/// Reads a native-endian `u16`.
pub fn ne16(buf: &[u8], from: usize) -> u16 {
    u16::from_ne_bytes(array(buf, from))
}

/// Reads a big-endian `u32`.
pub fn be32(buf: &[u8], from: usize) -> u32 {
    u32::from_be_bytes(array(buf, from))
}

/// `memmem(3)`: the offset of the first occurrence of `needle` in `hay`.
pub fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn put_ignores_what_does_not_fit() {
        let mut b = [0u8; 4];
        put(&mut b, 3, &[1, 2]);
        assert_eq!(b, [0; 4]);
        put(&mut b, 2, &[1, 2]);
        assert_eq!(b, [0, 0, 1, 2]);
        put(&mut b, usize::MAX, &[9]);
        assert_eq!(b, [0, 0, 1, 2]);
    }

    #[test]
    fn reads_are_zero_outside() {
        let b = [1u8, 2, 3];
        assert_eq!(at(&b, 2), 3);
        assert_eq!(at(&b, 3), 0);
        assert_eq!(array::<2>(&b, 2), [0, 0]);
        assert_eq!(be32(&[0, 0, 1, 2], 0), 258);
    }

    #[test]
    fn memmem() {
        assert_eq!(find(b"xxTAGyTAG", b"TAG"), Some(2));
        assert_eq!(find(b"xx", b"TAG"), None);
    }
}
