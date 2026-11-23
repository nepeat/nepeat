//! The GHASH key of an SA: `hkey = AES-ECB(SAK, 0^128)`.
//!
//! The C++ does no AES: it passes `SAI_MACSEC_SA_ATTR_AUTH_KEY` to the sequence as `hkey`
//! and trusts the caller (wpa_supplicant's SONiC driver computes it) to have derived it from
//! the SAK. [`AuthKeyPolicy::Trust`] keeps that; [`AuthKeyPolicy::Verify`] recomputes the key
//! and refuses an SA whose AUTH_KEY is not the SAK's (a PHY given a wrong hash key fails the
//! ICV of every frame, in both directions, with nothing in the SAI layer to show why).
//!
//! AES is the RustCrypto `aes` crate. On targets without the AES instructions it knows
//! (x86 AES-NI, `ARMv8` AES) it uses its fixsliced software implementation: bitsliced boolean
//! arithmetic, no table lookups and no branches on key or data, so constant-time by
//! construction. Big-endian ppc64 takes that path (the crate has no POWER code); the FIPS-197
//! vectors below are part of the test suite that runs there. Feature `zeroize`: the expanded
//! round keys are wiped when the cipher is dropped.
use crate::secret::Secret;
use aes::cipher::{Array, BlockCipherEncrypt, KeyInit};
use zeroize::Zeroize;

/// What to do with `SAI_MACSEC_SA_ATTR_AUTH_KEY`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AuthKeyPolicy {
    /// Use it as the hash key without looking (the C++ behaviour).
    #[default]
    Trust,
    /// Refuse the SA (`INVALID_ATTR_VALUE` at the attribute) unless it equals AES-ECB(SAK, 0^128).
    Verify,
}

/// AES-ECB(`sak`, 0^128) for a 16- or 32-byte SAK; `None` for another length.
pub fn derive_hkey(sak: &[u8]) -> Option<Secret<16>> {
    let mut block = Array::default();
    match sak.len() {
        16 => aes::Aes128::new_from_slice(sak).ok()?.encrypt_block(&mut block),
        32 => aes::Aes256::new_from_slice(sak).ok()?.encrypt_block(&mut block),
        _ => return None,
    }
    let hkey = Secret::from_slice(block.as_slice());
    block.as_mut_slice().zeroize();
    hkey
}

/// Whether `auth_key` is the hash key of `sak`. Every byte is compared whatever the
/// earlier ones were.
pub fn auth_key_matches(sak: &[u8], auth_key: &[u8; 16]) -> bool {
    derive_hkey(sak).is_some_and(|hkey| {
        hkey.expose()
            .iter()
            .zip(auth_key)
            .fold(0u8, |acc, (a, b)| acc | (a ^ b))
            == 0
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(s: &str) -> Vec<u8> {
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
            .collect()
    }

    /// AES itself, on whatever implementation this target selects: FIPS-197 appendix C.1 and
    /// C.3 (key 00 01 02 ..., plaintext 00 11 22 ... ff).
    #[test]
    fn fips_197_example_vectors() {
        let mut block = Array::default();
        let plain = hex("00112233445566778899aabbccddeeff");
        block.as_mut_slice().copy_from_slice(&plain);
        aes::Aes128::new_from_slice(&hex("000102030405060708090a0b0c0d0e0f"))
            .unwrap()
            .encrypt_block(&mut block);
        assert_eq!(block.as_slice(), hex("69c4e0d86a7b0430d8cdb78070b4c55a"));
        block.as_mut_slice().copy_from_slice(&plain);
        aes::Aes256::new_from_slice(&hex("000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f"))
            .unwrap()
            .encrypt_block(&mut block);
        assert_eq!(block.as_slice(), hex("8ea2b7ca516745bfeafc49904b496089"));
    }

    /// The hash subkey H = AES_K(0^128) of published GCM test cases: NIST GCM specification
    /// (McGrew, Viega) test cases 1 (AES-128, zero key), 3 (AES-128), 13 (AES-256, zero key)
    /// and 15 (AES-256).
    #[test]
    fn gcm_hash_subkeys() {
        let cases = [
            ("00000000000000000000000000000000", "66e94bd4ef8a2c3b884cfa59ca342b2e"),
            ("feffe9928665731c6d6a8f9467308308", "b83b533708bf535d0aa6e52980d53b78"),
            (
                "0000000000000000000000000000000000000000000000000000000000000000",
                "dc95c078a2408989ad48a21492842087",
            ),
            (
                "feffe9928665731c6d6a8f9467308308feffe9928665731c6d6a8f9467308308",
                "acbef20579b4b8ebce889bac8732dad7",
            ),
        ];
        for (key, h) in cases {
            let hkey = derive_hkey(&hex(key)).unwrap();
            assert_eq!(hkey.expose()[..], hex(h)[..], "key {key}");
            let mut auth = [0u8; 16];
            auth.copy_from_slice(&hex(h));
            assert!(auth_key_matches(&hex(key), &auth));
            auth[15] ^= 1;
            assert!(!auth_key_matches(&hex(key), &auth));
        }
    }

    #[test]
    fn only_128_and_256_bit_keys() {
        assert!(derive_hkey(&[0; 24]).is_none());
        assert!(derive_hkey(&[]).is_none());
        assert!(!auth_key_matches(&[0; 24], &[0; 16]));
    }
}
