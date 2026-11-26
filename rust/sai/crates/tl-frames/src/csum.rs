//! The Internet checksum as the C tools compute it (RFC 1071: one's complement of the folded
//! sum of big-endian 16-bit words, an odd last byte as the high half of a word).

/// Adds the 16-bit words of `b` to `s` (no folding).
pub fn sum16(b: &[u8], s: u32) -> u32 {
    let mut s = s;
    let words = b.chunks_exact(2);
    let tail = words.remainder();
    for w in words {
        if let [hi, lo] = *w {
            s = s.wrapping_add(u32::from(hi) << 8 | u32::from(lo));
        }
    }
    if let [last] = *tail {
        s = s.wrapping_add(u32::from(last) << 8);
    }
    s
}

/// Folds the carries and complements.
pub fn fold(s: u32) -> u16 {
    let mut s = s;
    while s >> 16 != 0 {
        s = (s & 0xffff) + (s >> 16);
    }
    !u16::try_from(s).unwrap_or(0xffff)
}

/// `csum(p, n)` of the C tools.
pub fn csum(b: &[u8]) -> u16 {
    fold(sum16(b, 0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rfc1071_example() {
        // RFC 1071 section 3: 00 01 f2 03 f4 f5 f6 f7 sums to ddf2 (the complement is 220d)
        let b = [0x00, 0x01, 0xf2, 0x03, 0xf4, 0xf5, 0xf6, 0xf7];
        assert_eq!(csum(&b), 0x220d);
    }

    #[test]
    fn odd_tail_is_the_high_byte() {
        assert_eq!(sum16(&[0x12, 0x34, 0x56], 0), 0x1234 + 0x5600);
    }

    #[test]
    fn a_header_with_its_checksum_verifies_to_zero() {
        let mut ip = [0x45, 0, 0, 0x1c, 0, 0, 0, 0, 0x40, 1, 0, 0, 10, 0, 0, 2, 10, 99, 0, 1];
        let c = csum(&ip);
        ip[10..12].copy_from_slice(&c.to_be_bytes());
        assert_eq!(csum(&ip), 0);
    }
}
