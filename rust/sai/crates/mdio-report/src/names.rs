//! Names for PHYs and registers in messages and tables.
//!
//! One entry per line, `#` comments (in column 1):
//!
//! ```text
//! phy <address word, hex> <name>          e.g.  phy 24 cage5
//! reg <dev>.<reg, hex> <name>             e.g.  reg 1.8215 fw
//! ```

use std::fmt;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

use crate::report::MAX_PHYS;
use crate::text::{is_space, low32, strerror, strtoul16};

const MAX_REGS: usize = 32;
/// C reads the file with `fgets(line, 200)`: a longer line continues as the next "line".
const CHUNK: usize = 199;

/// Names of PHYs (by address word) and registers (by device and register).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Names {
    phys: Vec<(u32, Vec<u8>)>,
    regs: Vec<(u32, u32, Vec<u8>)>,
}

/// Why a names file was not taken: the message the C tool prints.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NamesError(pub Vec<u8>);

impl fmt::Display for NamesError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&String::from_utf8_lossy(&self.0))
    }
}

impl std::error::Error for NamesError {}

/// The next `sscanf("%<max>s")` field: skips white space, takes at most `max` bytes of the word
/// and leaves the rest of it, if any, for the next field (as C does).
fn field<'a>(rest: &mut &'a [u8], max: usize) -> Option<&'a [u8]> {
    let start = rest.iter().position(|&b| !is_space(b))?;
    let text = rest.get(start..).unwrap_or(&[]);
    let len = text.iter().take_while(|&&b| !is_space(b)).count().min(max);
    let (word, tail) = text.split_at_checked(len)?;
    *rest = tail;
    Some(word)
}

/// `%*s`: skips one whole word.
fn skip_word(rest: &mut &[u8]) -> Option<()> {
    field(rest, usize::MAX).map(|_| ())
}

enum Entry {
    Phy(u32, Vec<u8>),
    Reg(u32, u32, Vec<u8>),
}

/// One line (not a comment, not blank). `None`: a bad line.
fn entry(line: &[u8], names: &Names) -> Option<Entry> {
    let mut rest = line;
    // "%7s": a longer first word is neither "phy" nor "reg".
    let kind = field(&mut rest, 7)?;
    let mut rest = line;
    skip_word(&mut rest)?;
    let args = field(&mut rest, 31).zip(field(&mut rest, 63));
    match (kind, args) {
        (b"phy", Some((a, name))) if names.phys.len() < MAX_PHYS => {
            let (phy, end) = strtoul16(a);
            end.is_empty()
                .then(|| Entry::Phy(low32(phy), name.iter().copied().take(23).collect()))
        }
        (b"reg", Some((a, name))) if names.regs.len() < MAX_REGS => {
            let (dev, end) = strtoul16(a);
            let (reg, end) = strtoul16(end.strip_prefix(b".")?);
            end.is_empty()
                .then(|| Entry::Reg(low32(dev), low32(reg), name.iter().copied().take(11).collect()))
        }
        _ => None,
    }
}

impl Names {
    /// Reads a names file. The error is one line naming the file (and the bad line).
    pub fn load(path: &Path) -> Result<Self, NamesError> {
        let shown = path.as_os_str().as_bytes();
        let data = std::fs::read(path).map_err(|e| {
            let mut m = shown.to_vec();
            m.extend_from_slice(format!(": {}", strerror(&e)).as_bytes());
            NamesError(m)
        })?;
        Self::parse(&data).map_err(|line| {
            let mut m = shown.to_vec();
            m.extend_from_slice(format!(":{line}: want 'phy <hex> <name>' or 'reg <dev>.<reg> <name>'").as_bytes());
            NamesError(m)
        })
    }

    /// Parses the text of a names file; the error is the number of the bad line.
    pub fn parse(data: &[u8]) -> Result<Self, u64> {
        let mut names = Self::default();
        let mut rest = data;
        let mut ln = 0u64;
        while !rest.is_empty() {
            // What fgets returns next: up to and including a line feed, at most CHUNK bytes.
            let window = rest.get(..CHUNK).unwrap_or(rest);
            let len = window
                .iter()
                .position(|&b| b == b'\n')
                .map_or(window.len(), |i| i.saturating_add(1));
            let (chunk, tail) = rest.split_at_checked(len).unwrap_or((rest, &[]));
            rest = tail;
            ln = ln.saturating_add(1);
            // A C string: it ends at a NUL byte.
            let line = chunk.split(|&b| b == 0).next().unwrap_or(chunk);
            if line.first() == Some(&b'#') || line.iter().all(|&b| is_space(b)) {
                continue;
            }
            match entry(line, &names) {
                Some(Entry::Phy(phy, name)) => names.phys.push((phy, name)),
                Some(Entry::Reg(dev, reg, name)) => names.regs.push((dev, reg, name)),
                None => return Err(ln),
            }
        }
        Ok(names)
    }

    /// The name of the PHY at address word `phy`.
    pub fn phy(&self, phy: u32) -> Option<&[u8]> {
        self.phys
            .iter()
            .find(|(p, _)| *p == phy)
            .map(|(_, name)| name.as_slice())
    }

    /// The name of register `dev.reg`.
    pub fn reg(&self, dev: u32, reg: u32) -> Option<&[u8]> {
        self.regs
            .iter()
            .find(|(d, r, _)| *d == dev && *r == reg)
            .map(|(_, _, name)| name.as_slice())
    }

    /// How many PHYs and registers have a name.
    pub fn len(&self) -> (usize, usize) {
        (self.phys.len(), self.regs.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entries_comments_and_lookups() {
        let n =
            Names::parse(b"# names\nphy 0 cage1\nphy 24 cage5\n\n  \t\nphy 25 cage6\nreg 1.8b00 id\nreg 1.8215 fw\n")
                .unwrap();
        assert_eq!(n.len(), (3, 2));
        assert_eq!(n.phy(0x24), Some(&b"cage5"[..]));
        assert_eq!(n.phy(0x26), None);
        assert_eq!(n.reg(1, 0x8215), Some(&b"fw"[..]));
        assert_eq!(n.reg(1, 0x8216), None);
        assert_eq!(Names::parse(b"").unwrap().len(), (0, 0));
        // no line feed at the end, CR LF, 0x prefix, extra words ignored, first entry wins
        let n = Names::parse(b"phy 0x17c  bus1-cage9 ignored\r\nreg 0x1f.0xffff x\r\nphy 17c other").unwrap();
        assert_eq!(n.phy(0x17c), Some(&b"bus1-cage9"[..]));
        assert_eq!(n.reg(0x1f, 0xffff), Some(&b"x"[..]));
    }

    #[test]
    fn bad_lines_are_named() {
        assert_eq!(Names::parse(b"phy zz cage1\n"), Err(1));
        assert_eq!(Names::parse(b"phy 0 a\nphy 1\n"), Err(2));
        assert_eq!(Names::parse(b"phy 0 a\n\nreg 1 fw\n"), Err(3));
        assert_eq!(Names::parse(b"reg 1.82g5 fw\n"), Err(1));
        assert_eq!(Names::parse(b"cage 1 x\n"), Err(1));
        assert_eq!(Names::parse(b" # not a comment: '#' is not in column 1\n"), Err(1));
        assert_eq!(Names::parse(b"physical 0 x\n"), Err(1));
    }

    #[test]
    #[allow(clippy::format_collect, reason = "test input")]
    fn c_quirks_are_kept() {
        // names are cut to 23 and 11 bytes
        let n = Names::parse(b"phy 1 abcdefghijklmnopqrstuvwxyz\nreg 1.2 abcdefghijklmnop\n").unwrap();
        assert_eq!(n.phy(1), Some(&b"abcdefghijklmnopqrstuvw"[..]));
        assert_eq!(n.reg(1, 2), Some(&b"abcdefghijk"[..]));
        // "reg 1. x" is register 1.0; a sign is taken by strtoul; the value is cut to 32 bits
        let n = Names::parse(b"reg 1. x\nphy -1 neg\nphy 100000024 wide\n").unwrap();
        assert_eq!(n.reg(1, 0), Some(&b"x"[..]));
        assert_eq!(n.phy(u32::MAX), Some(&b"neg"[..]));
        assert_eq!(n.phy(0x24), Some(&b"wide"[..]));
        // the 257th PHY and the 33rd register are bad lines
        let many: String = (0..257).map(|i| format!("phy {i:x} p{i}\n")).collect();
        assert_eq!(Names::parse(many.as_bytes()), Err(257));
        let many: String = (0..33).map(|i| format!("reg 1.{i:x} r{i}\n")).collect();
        assert_eq!(Names::parse(many.as_bytes()), Err(33));
        // a line of more than 199 bytes continues as the next line (here: a bad one)
        let long = format!("phy 1 a{}\n", " ".repeat(200));
        assert_eq!(
            Names::parse(long.as_bytes()),
            Ok(Names {
                phys: vec![(1, b"a".to_vec())],
                regs: vec![]
            })
        );
        let long = format!("phy 1 a{}x\n", " ".repeat(200));
        assert_eq!(Names::parse(long.as_bytes()), Err(2));
        // a NUL ends the line
        assert_eq!(Names::parse(b"phy 2 b\0 garbage\n").unwrap().phy(2), Some(&b"b"[..]));
    }

    #[test]
    fn load_reports_the_file() {
        let e = Names::load(Path::new("/nonexistent/names")).unwrap_err();
        assert_eq!(e.to_string(), "/nonexistent/names: No such file or directory");
    }
}
