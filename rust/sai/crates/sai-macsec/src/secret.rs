//! Key material and the parameters / results of a sequence run.
//!
//! [`Secret`] and [`Params`] hold key bytes (SAK, hash key, salt, nonce). Both
//! - never print their bytes: `Debug` shows sizes only, there is no `Display`;
//! - keep the bytes on the heap and overwrite them when dropped or replaced (the `zeroize`
//!   crate: volatile writes the compiler may not remove; this crate has no `unsafe` of its own).
//!
//! Limit, stated plainly: this covers the copies these types own. A copy the caller made
//! before handing the bytes over (the C caller's `sai_attribute_t`, a stack array passed to
//! [`Secret::from_slice`]) is the caller's to wipe.
use std::collections::BTreeMap;
use std::fmt;
use zeroize::{Zeroize, Zeroizing};

/// `N` secret bytes.
pub struct Secret<const N: usize>(Box<[u8; N]>);

impl<const N: usize> Secret<N> {
    /// Copies `bytes` (exactly `N` of them) into a new secret.
    pub fn from_slice(bytes: &[u8]) -> Option<Self> {
        if bytes.len() != N {
            return None;
        }
        let mut owned = Box::new([0u8; N]);
        owned.copy_from_slice(bytes);
        Some(Self(owned))
    }

    /// The bytes. Named to be easy to find in a review: every use is a place key bytes flow to.
    pub fn expose(&self) -> &[u8; N] {
        &self.0
    }
}

impl<const N: usize> Clone for Secret<N> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<const N: usize> Secret<N> {
    /// Overwrites the bytes with zeros (what dropping does).
    fn wipe(&mut self) {
        self.0.zeroize();
    }
}

impl<const N: usize> Drop for Secret<N> {
    fn drop(&mut self) {
        self.wipe();
    }
}

impl<const N: usize> fmt::Debug for Secret<N> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Secret<{N}>(redacted)")
    }
}

/// Names of the parameters a sequence can use (`ext_macsec.h`), bytes index 0 first.
pub mod param {
    /// SAK, 16 or 32 bytes.
    pub const KEY: &str = "key";
    /// GHASH key AES-ECB(SAK, 0^128), 16 bytes.
    pub const HKEY: &str = "hkey";
    /// SCI, 8 bytes as on the wire.
    pub const SCI: &str = "sci";
    /// XPN salt, 12 bytes.
    pub const SALT: &str = "salt";
    /// XPN short SCI, 4 bytes, most significant byte first (as on the wire).
    pub const SSCI: &str = "ssci";
    /// The fixed part of the XPN nonce: the salt with the SSCI XOR-ed into bytes 0..3, 12 bytes.
    pub const IV: &str = "iv";
    /// Packet number, 8 bytes, least significant byte first (egress: next PN; ingress: lowest acceptable).
    pub const PN: &str = "pn";
    /// Replay window, 4 bytes, least significant byte first.
    pub const WINDOW: &str = "window";
}

/// The parameters of one sequence run: byte strings by name. Treated as key material as a
/// whole (see the module documentation).
#[derive(Default)]
pub struct Params(BTreeMap<String, Zeroizing<Vec<u8>>>);

impl Params {
    /// No parameters.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets parameter `name`; a previous value is wiped.
    pub fn set(&mut self, name: &str, bytes: &[u8]) {
        self.0.insert(name.to_owned(), Zeroizing::new(bytes.to_vec()));
    }

    /// The bytes of parameter `name`.
    pub fn get(&self, name: &str) -> Option<&[u8]> {
        self.0.get(name).map(|v| v.as_slice())
    }

    /// The names, sorted.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.0.keys().map(String::as_str)
    }

    /// Whether there is no parameter.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Wipes and removes every parameter (`ms_wipe`).
    pub fn wipe(&mut self) {
        self.0.clear(); // dropping a value zeroes it
    }

    /// A second copy with its own storage, wiped on its own drop.
    #[must_use]
    pub fn duplicate(&self) -> Self {
        Self(
            self.0
                .iter()
                .map(|(k, v)| (k.clone(), Zeroizing::new(v.to_vec())))
                .collect(),
        )
    }
}

impl fmt::Debug for Params {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut map = f.debug_map();
        for (name, value) in &self.0 {
            map.entry(name, &format_args!("{} bytes (redacted)", value.len()));
        }
        map.finish()
    }
}

/// What a sequence read and kept under a name (`X ... <result>[<word>]`): 16-bit words,
/// least significant word first.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Results(BTreeMap<String, Vec<u16>>);

impl Results {
    /// No results.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets word `index` of result `name`; words below it that were never set read 0.
    pub fn set_word(&mut self, name: &str, index: usize, value: u16) {
        let words = self.0.entry(name.to_owned()).or_default();
        if words.len() <= index {
            words.resize(index + 1, 0);
        }
        if let Some(slot) = words.get_mut(index) {
            *slot = value;
        }
    }

    /// Result `name` as a number: words 0..=3, least significant first; 0 when absent
    /// (`ms_result_u64`).
    pub fn u64(&self, name: &str) -> u64 {
        self.0.get(name).map_or(0, |words| {
            words
                .iter()
                .take(4)
                .zip([0u32, 16, 32, 48])
                .fold(0, |acc, (word, shift)| acc | (u64::from(*word) << shift))
        })
    }

    /// The names, sorted.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.0.keys().map(String::as_str)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_never_shows_bytes() {
        let key = Secret::<16>::from_slice(&[0xc0; 16]).unwrap();
        let text = format!("{key:?} {:?}", Some(key.clone()));
        assert!(!text.contains("c0") && !text.contains("192"), "{text}");
        let mut p = Params::new();
        p.set(param::KEY, &[0xc0, 0xc1, 0xc2]);
        p.set(param::PN, &[0xc7; 8]);
        let text = format!("{p:?} {p:#?}");
        assert!(text.contains("key") && text.contains("3 bytes"), "{text}");
        for needle in ["c0", "c1", "c7", "192", "193", "199"] {
            assert!(!text.contains(needle), "{needle} in {text}");
        }
    }

    #[test]
    fn the_drop_path_zeroes_the_bytes() {
        let mut key = Secret::<32>::from_slice(&[0xc0; 32]).unwrap();
        key.wipe(); // exactly what Drop calls; after a real drop the memory cannot be looked at
        assert_eq!(key.expose(), &[0; 32]);
    }

    #[test]
    fn secrets_have_a_fixed_length() {
        assert!(Secret::<16>::from_slice(&[0; 15]).is_none());
        assert!(Secret::<16>::from_slice(&[0; 17]).is_none());
        assert_eq!(Secret::<12>::from_slice(&[7; 12]).unwrap().expose(), &[7; 12]);
    }

    #[test]
    fn params_set_get_wipe() {
        let mut p = Params::new();
        assert!(p.is_empty());
        p.set("key", &[1, 2]);
        p.set("key", &[3]);
        assert_eq!(p.get("key"), Some(&[3u8][..]));
        let q = p.duplicate();
        p.wipe();
        assert!(p.is_empty() && p.get("key").is_none());
        assert_eq!(q.get("key"), Some(&[3u8][..]));
        assert_eq!(q.names().collect::<Vec<_>>(), ["key"]);
    }

    #[test]
    fn results_are_words_least_significant_first() {
        let mut r = Results::new();
        r.set_word("pn", 2, 1);
        r.set_word("pn", 0, 0xbeef);
        assert_eq!(r.u64("pn"), 0x1_0000_beef); // the C++ golden test's value
        r.set_word("pn", 3, 0xffff);
        r.set_word("pn", 4, 0x1234); // beyond 64 bits: ignored
        assert_eq!(r.u64("pn"), 0xffff_0001_0000_beef);
        assert_eq!(r.u64("absent"), 0);
    }
}
