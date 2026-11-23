//! The command line tools `openbcm-ext-macsec-rs` and `openbcm-ext-phy-rs`: the same arguments,
//! output (byte for byte) and exit codes as sai-openbcm's `openbcm-ext-macsec`
//! (`ext_macsec_check.cpp`) and `openbcm-ext-phy` (`ext_phy_check.cpp`).
//!
//! ```text
//! openbcm-ext-macsec-rs --check FILE                       parse; print what it holds
//! openbcm-ext-macsec-rs --render FILE SEQ PHY [PARAMS|-]   the bus operations of one sequence for PHY (hex):
//!                                                          "W phy dev reg val", "R phy dev reg [want mask]", "D usec".
//!                                                          Reads that are compared return the expected value, others 0.
//!                                                          PARAMS: lines "<param> <hex bytes>"; "-" = none.
//! openbcm-ext-macsec-rs --render FILE SEQ PHY PARAMS READS the n-th read returns the n-th hex value of READS
//! openbcm-ext-phy-rs --check FILE
//! ```
//!
//! **The output of `--render` contains the key bytes** (it is the bus trace), as the C++ tool's.
//!
//! Differences from the C++ tools:
//! * PHY must be a hex number up to ffff (exit 2 otherwise). C++ takes whatever `strtoul` makes
//!   of the word (garbage is PHY 0) and passes a wider value on.
//! * A value in READS is cut to 16 bits. C++ keeps 32, which only shows in the "read 0x..." of
//!   a mismatch message.
//! * A parameter in PARAMS that no table can name is ignored, and bytes beyond the longest a
//!   table can index are dropped; neither can change the output.

use std::ffi::OsString;
use std::io::Write;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

use crate::exec::{Bus, run};
use crate::lex::{Radix, num16, strtoul_hex, words};
use crate::op::{Dev, Op, OpKind, ParamName};
use crate::params::Params;
use crate::{ExtPhyTable, MacsecTable, Section};

/// The exit code of a run that did what was asked.
pub const EXIT_OK: u8 = 0;
/// A file cannot be read, a table is refused, a sequence is missing or stopped.
pub const EXIT_FAILED: u8 = 1;
/// Wrong arguments.
pub const EXIT_USAGE: u8 = 2;

/// Writes `parts` and a line feed. Errors of the stream are ignored, as `printf` ignores them.
fn say(to: &mut dyn Write, parts: &[&[u8]]) {
    let mut line = Vec::new();
    for part in parts {
        line.extend_from_slice(part);
    }
    line.push(b'\n');
    let _ = to.write_all(&line);
}

/// What `printf("%s")` prints of a message: the bytes before its first NUL. (A table word with a
/// NUL byte can end up in a message; the C++ tools print the message that way.)
fn c_str(message: &[u8]) -> &[u8] {
    message.split(|&b| b == 0).next().unwrap_or(message)
}

/// A file as `std::ifstream` + `rdbuf` read it: `None` when it cannot be opened; a directory can
/// be opened and reads as empty.
fn slurp(path: &[u8]) -> Option<Vec<u8>> {
    let path = Path::new(std::ffi::OsStr::from_bytes(path));
    match std::fs::read(path) {
        Ok(text) => Some(text),
        Err(_) if path.is_dir() => Some(Vec::new()),
        Err(_) => None,
    }
}

/// The renderer's bus: prints every access, answers reads from the recording or with what the
/// table expects.
struct Render<'a> {
    out: &'a mut dyn Write,
    /// The recorded read values, when replaying.
    reads: Option<std::vec::IntoIter<u16>>,
    /// The comparison of the operation being run, if it compares.
    compare: Option<(u16, u16)>,
}

impl Bus for Render<'_> {
    type Error = std::convert::Infallible;

    fn begin(&mut self, _index: usize, op: &Op) {
        self.compare = match &op.kind {
            OpKind::Expect { cmp, .. } | OpKind::Poll { cmp, .. } => Some((cmp.want(), cmp.mask())),
            _ => None,
        };
    }

    fn read(&mut self, phy: u16, dev: Dev, reg: u16) -> Result<u16, Self::Error> {
        let at = format!("R {phy:03x} {:x} {reg:04x}", dev.get());
        match (&mut self.reads, self.compare) {
            (Some(reads), _) => {
                say(self.out, &[at.as_bytes()]);
                Ok(reads.next().unwrap_or(0))
            }
            (None, Some((want, mask))) => {
                say(self.out, &[format!("{at} {want:04x} {mask:04x}").as_bytes()]);
                Ok(want)
            }
            (None, None) => {
                say(self.out, &[at.as_bytes()]);
                Ok(0)
            }
        }
    }

    fn write(&mut self, phy: u16, dev: Dev, reg: u16, value: u16) -> Result<(), Self::Error> {
        say(
            self.out,
            &[format!("W {phy:03x} {:x} {reg:04x} {value:04x}", dev.get()).as_bytes()],
        );
        Ok(())
    }

    fn delay_us(&mut self, us: u32) {
        say(self.out, &[format!("D {us}").as_bytes()]);
    }
}

/// PARAMS: pairs of words "<param> <hex bytes>"; every two characters are one byte, read as
/// `strtoul` reads them; a later line replaces an earlier one.
fn read_params(text: &[u8]) -> Params {
    let mut params = Params::new();
    let all = words(text);
    for pair in all.chunks_exact(2) {
        let [name, hex] = pair else { continue };
        let Some(name) = ParamName::from_name(name) else {
            continue;
        };
        // the low 8 bits of each pair, as the C++ "(uint8_t)strtoul(...)"
        let mut bytes: Vec<u8> = hex.chunks_exact(2).map(|b| strtoul_hex(b).to_le_bytes()[0]).collect();
        params.set_cut(name, &bytes);
        zeroize::Zeroize::zeroize(bytes.as_mut_slice());
    }
    params
}

/// `openbcm-ext-macsec-rs`: `args` is the whole command line, program name first. Returns the
/// exit code.
pub fn ext_macsec(args: &[OsString], out: &mut dyn Write, err: &mut dyn Write) -> u8 {
    let args: Vec<&[u8]> = args.iter().map(|a| a.as_bytes()).collect();
    let (file, render) = match args.as_slice() {
        [_, b"--check", file] => (*file, None),
        [_, b"--render", file, rest @ ..] if (2..=4).contains(&rest.len()) => (*file, Some(rest)),
        _ => {
            let program = args.first().copied().unwrap_or(b"openbcm-ext-macsec-rs");
            say(
                err,
                &[
                    b"usage: ",
                    program,
                    b" --check FILE | --render FILE SEQ PHY [PARAMS|- [READS]]",
                ],
            );
            return EXIT_USAGE;
        }
    };
    let Some(text) = slurp(file) else {
        say(err, &[file, b": cannot open"]);
        return EXIT_FAILED;
    };
    let table = match MacsecTable::parse(&text) {
        Ok(table) => table,
        Err(e) => {
            say(err, &[file, b": ", c_str(&e.to_bytes())]);
            return EXIT_FAILED;
        }
    };
    let Some(rest) = render else {
        let ops: usize = table.sequences().map(|(_, s)| s.ops().len()).sum();
        let counters = match table.counters() {
            crate::CounterMode::ClearOnRead => "clear-on-read",
            crate::CounterMode::Absolute => "absolute",
        };
        let summary = format!(
            ": {} ports, {} sequences, {ops} operations, counters {counters}, ifg-bits {}",
            table.ports().len(),
            table.sequences().len(),
            table.ifg_bits()
        );
        say(out, &[file, summary.as_bytes()]);
        return EXIT_OK;
    };
    let (Some(&seq_name), Some(&phy_word)) = (rest.first(), rest.get(1)) else {
        return EXIT_USAGE;
    };
    let Some(seq) = std::str::from_utf8(seq_name).ok().and_then(|name| table.sequence(name)) else {
        say(err, &[file, b": no sequence ", seq_name]);
        return EXIT_FAILED;
    };
    let Some(phy) = num16(phy_word, Radix::Hex, 0xffff) else {
        say(err, &[file, b": PHY ", phy_word, b": not a hex number up to ffff"]);
        return EXIT_USAGE;
    };
    let reads = match rest.get(3) {
        Some(path) => {
            let Some(text) = slurp(path) else {
                say(err, &[path, b": cannot open"]);
                return EXIT_FAILED;
            };
            // cut to 16 bits: what a bus can return
            Some(
                words(&text)
                    .into_iter()
                    .map(|w| u16::from_le_bytes(first_two(strtoul_hex(w))))
                    .collect::<Vec<_>>(),
            )
        }
        None => None,
    };
    let params = match rest.get(2) {
        Some(&path) if path != b"-" => {
            let Some(mut text) = slurp(path) else {
                say(err, &[path, b": cannot open"]);
                return EXIT_FAILED;
            };
            let params = read_params(&text);
            zeroize::Zeroize::zeroize(text.as_mut_slice());
            params
        }
        _ => Params::new(),
    };
    let mut bus = Render {
        out,
        reads: reads.map(Vec::into_iter),
        compare: None,
    };
    match run(&mut bus, seq.ops(), phy, &params) {
        Ok(_) => EXIT_OK,
        Err(e) => {
            say(err, &[file, b": ", seq_name, b": ", e.to_string().as_bytes()]);
            EXIT_FAILED
        }
    }
}

/// The low 16 bits of a 32-bit value, as little-endian bytes.
fn first_two(value: u32) -> [u8; 2] {
    let [a, b, _, _] = value.to_le_bytes();
    [a, b]
}

/// `openbcm-ext-phy-rs`: `args` is the whole command line, program name first. Returns the exit
/// code.
pub fn ext_phy(args: &[OsString], out: &mut dyn Write, err: &mut dyn Write) -> u8 {
    let args: Vec<&[u8]> = args.iter().map(|a| a.as_bytes()).collect();
    let [_, b"--check", file] = args.as_slice() else {
        let program = args.first().copied().unwrap_or(b"openbcm-ext-phy-rs");
        say(err, &[b"usage: ", program, b" --check FILE"]);
        return EXIT_USAGE;
    };
    let Some(text) = slurp(file) else {
        say(err, &[file, b": cannot open"]);
        return EXIT_FAILED;
    };
    let table = match ExtPhyTable::parse(&text) {
        Ok(table) => table,
        Err(e) => {
            say(err, &[file, b": ", c_str(&e.to_bytes())]);
            return EXIT_FAILED;
        }
    };
    let count = |section| table.ports().iter().map(|p| p.ops(section).len()).sum::<usize>();
    let polled = table
        .ports()
        .iter()
        .filter(|p| !p.ops(Section::Link).is_empty())
        .count();
    let summary = format!(
        ": {} ports ({polled} with a link poll), poll {} ms, up after {} good polls, hold-off {} ms; operations: link {}, admin-down {}, admin-up {}",
        table.ports().len(),
        table.poll_ms(),
        table.up_polls(),
        table.holdoff_ms(),
        count(Section::Link),
        count(Section::AdminDown),
        count(Section::AdminUp)
    );
    say(out, &[file, summary.as_bytes()]);
    EXIT_OK
}
