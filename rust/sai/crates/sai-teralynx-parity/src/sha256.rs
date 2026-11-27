//! SHA-256 (FIPS 180-4) in safe Rust: the digests of the golden files (`golden`). The crate has no
//! hashing dependency; the input is a few hundred megabytes at most, so a plain implementation will do.
#![forbid(unsafe_code)]

use std::fmt::Write as _;

const K: [u32; 64] = [
    0x428a_2f98,
    0x7137_4491,
    0xb5c0_fbcf,
    0xe9b5_dba5,
    0x3956_c25b,
    0x59f1_11f1,
    0x923f_82a4,
    0xab1c_5ed5,
    0xd807_aa98,
    0x1283_5b01,
    0x2431_85be,
    0x550c_7dc3,
    0x72be_5d74,
    0x80de_b1fe,
    0x9bdc_06a7,
    0xc19b_f174,
    0xe49b_69c1,
    0xefbe_4786,
    0x0fc1_9dc6,
    0x240c_a1cc,
    0x2de9_2c6f,
    0x4a74_84aa,
    0x5cb0_a9dc,
    0x76f9_88da,
    0x983e_5152,
    0xa831_c66d,
    0xb003_27c8,
    0xbf59_7fc7,
    0xc6e0_0bf3,
    0xd5a7_9147,
    0x06ca_6351,
    0x1429_2967,
    0x27b7_0a85,
    0x2e1b_2138,
    0x4d2c_6dfc,
    0x5338_0d13,
    0x650a_7354,
    0x766a_0abb,
    0x81c2_c92e,
    0x9272_2c85,
    0xa2bf_e8a1,
    0xa81a_664b,
    0xc24b_8b70,
    0xc76c_51a3,
    0xd192_e819,
    0xd699_0624,
    0xf40e_3585,
    0x106a_a070,
    0x19a4_c116,
    0x1e37_6c08,
    0x2748_774c,
    0x34b0_bcb5,
    0x391c_0cb3,
    0x4ed8_aa4a,
    0x5b9c_ca4f,
    0x682e_6ff3,
    0x748f_82ee,
    0x78a5_636f,
    0x84c8_7814,
    0x8cc7_0208,
    0x90be_fffa,
    0xa450_6ceb,
    0xbef9_a3f7,
    0xc671_78f2,
];

const H0: [u32; 8] = [
    0x6a09_e667,
    0xbb67_ae85,
    0x3c6e_f372,
    0xa54f_f53a,
    0x510e_527f,
    0x9b05_688c,
    0x1f83_d9ab,
    0x5be0_cd19,
];

/// An incremental SHA-256.
#[derive(Clone, Debug)]
pub struct Sha256 {
    state: [u32; 8],
    buf: Vec<u8>,
    len: u64,
}

impl Default for Sha256 {
    fn default() -> Self {
        Self::new()
    }
}

#[allow(
    clippy::many_single_char_names,
    reason = "FIPS 180-4's names: the message schedule w, the working variables a to h"
)]
fn compress(state: &mut [u32; 8], block: &[u8]) {
    let mut w = [0u32; 64];
    for (i, c) in block.chunks_exact(4).enumerate() {
        let b = |k: usize| c.get(k).copied().map_or(0, u32::from);
        if let Some(slot) = w.get_mut(i) {
            *slot = (b(0) << 24) | (b(1) << 16) | (b(2) << 8) | b(3);
        }
    }
    for i in 16..64 {
        let at = |k: usize| w.get(i - k).copied().unwrap_or(0);
        let s0 = at(15).rotate_right(7) ^ at(15).rotate_right(18) ^ (at(15) >> 3);
        let s1 = at(2).rotate_right(17) ^ at(2).rotate_right(19) ^ (at(2) >> 10);
        let v = at(16).wrapping_add(s0).wrapping_add(at(7)).wrapping_add(s1);
        if let Some(slot) = w.get_mut(i) {
            *slot = v;
        }
    }
    let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = *state;
    for (k, wi) in K.iter().zip(w.iter()) {
        let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
        let ch = (e & f) ^ (!e & g);
        let t1 = h.wrapping_add(s1).wrapping_add(ch).wrapping_add(*k).wrapping_add(*wi);
        let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
        let maj = (a & b) ^ (a & c) ^ (b & c);
        let t2 = s0.wrapping_add(maj);
        h = g;
        g = f;
        f = e;
        e = d.wrapping_add(t1);
        d = c;
        c = b;
        b = a;
        a = t1.wrapping_add(t2);
    }
    for (s, v) in state.iter_mut().zip([a, b, c, d, e, f, g, h]) {
        *s = s.wrapping_add(v);
    }
}

impl Sha256 {
    /// A fresh hash.
    pub fn new() -> Self {
        Self {
            state: H0,
            buf: Vec::with_capacity(64),
            len: 0,
        }
    }

    /// Feeds bytes.
    pub fn update(&mut self, data: &[u8]) {
        self.len = self.len.wrapping_add(data.len() as u64);
        let mut rest = data;
        if !self.buf.is_empty() {
            let take = (64 - self.buf.len()).min(rest.len());
            let (head, tail) = rest.split_at(take);
            self.buf.extend_from_slice(head);
            rest = tail;
            if self.buf.len() == 64 {
                compress(&mut self.state, &self.buf);
                self.buf.clear();
            }
        }
        let mut blocks = rest.chunks_exact(64);
        for b in &mut blocks {
            compress(&mut self.state, b);
        }
        self.buf.extend_from_slice(blocks.remainder());
    }

    /// The digest as 64 lower-case hex digits.
    pub fn finish_hex(mut self) -> String {
        let bits = self.len.wrapping_mul(8);
        let pad = if self.buf.len() < 56 {
            56 - self.buf.len()
        } else {
            120 - self.buf.len()
        };
        let mut tail = vec![0u8; pad];
        if let Some(first) = tail.first_mut() {
            *first = 0x80;
        }
        tail.extend_from_slice(&bits.to_be_bytes());
        let len = self.len;
        self.update(&tail);
        self.len = len;
        self.state.iter().fold(String::new(), |mut s, word| {
            let _ = write!(s, "{word:08x}");
            s
        })
    }
}

/// The SHA-256 of a byte string, in hex.
pub fn hex(data: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(data);
    h.finish_hex()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_fips_vectors() {
        assert_eq!(
            hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            hex(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"),
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
        );
        let million = vec![b'a'; 1_000_000];
        assert_eq!(
            hex(&million),
            "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0"
        );
    }

    #[test]
    fn chunked_updates_equal_one_update() {
        let data: Vec<u8> = (0..1000u32).map(|i| (i % 251) as u8).collect();
        let mut h = Sha256::new();
        for c in data.chunks(7) {
            h.update(c);
        }
        assert_eq!(h.finish_hex(), hex(&data));
    }
}
