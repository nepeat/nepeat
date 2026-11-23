//! The per-PHY result of a verification section.

use mdio_script::{Check, Until};

use crate::names::Names;
use crate::text::{pad, put};

/// PHYs a report holds; checks of further PHYs are counted as dropped.
pub const MAX_PHYS: usize = 256;
/// Checks a report holds per PHY; further ones are counted as dropped.
pub const MAX_CHECKS: usize = 12;

/// What the verification reads say about one PHY.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum PhyState {
    /// Every register is what the script expects.
    #[default]
    Ok,
    /// It answers; some register is not what the script expects.
    Mismatch,
    /// Every verified register reads the same value: no PHY answering, or a hung one.
    Constant,
    /// The bus master reported an error.
    Io,
}

impl PhyState {
    /// `ok`, `mismatch`, `constant`, `io-error` (the JSON's words).
    pub const fn name(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Mismatch => "mismatch",
            Self::Constant => "constant",
            Self::Io => "io-error",
        }
    }
}

/// One verification read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CheckRecord {
    /// Device (MMD).
    pub dev: u8,
    /// Register.
    pub reg: u16,
    /// The value compared with.
    pub want: u16,
    /// The bits compared.
    pub mask: u16,
    /// The value read (0 when the read failed).
    pub got: u16,
    /// `P` (equal) or `N` (not equal).
    pub until: Until,
    /// The assertion holds.
    pub ok: bool,
    /// The read itself failed.
    pub io_error: bool,
}

/// The verification reads of one PHY.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PhyReport {
    /// The bus master's address word.
    pub phy: u16,
    /// Set by [`Report::classify`].
    pub state: PhyState,
    /// For [`PhyState::Constant`]: the value.
    pub constant: u16,
    checks: Vec<CheckRecord>,
}

impl PhyReport {
    /// The reads, in script order (at most [`MAX_CHECKS`]).
    pub fn checks(&self) -> &[CheckRecord] {
        &self.checks
    }
}

/// Every verification read of a run, filed under its PHY (order of first appearance).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Report {
    phys: Vec<PhyReport>,
    dropped: u64,
}

impl Report {
    /// An empty report.
    pub const fn new() -> Self {
        Self {
            phys: Vec::new(),
            dropped: 0,
        }
    }

    /// Forgets everything.
    pub fn reset(&mut self) {
        *self = Self::new();
    }

    /// Files one check under its PHY: the body of [`mdio_script::Hooks::verify`].
    pub fn add(&mut self, c: &Check<'_>) {
        let at = if let Some(i) = self.phys.iter().position(|p| p.phy == c.at.phy) {
            i
        } else {
            if self.phys.len() == MAX_PHYS {
                self.dropped = self.dropped.saturating_add(1);
                return;
            }
            self.phys.push(PhyReport {
                phy: c.at.phy,
                state: PhyState::Ok,
                constant: 0,
                checks: Vec::new(),
            });
            self.phys.len().saturating_sub(1)
        };
        let Some(p) = self.phys.get_mut(at) else { return };
        if p.checks.len() == MAX_CHECKS {
            self.dropped = self.dropped.saturating_add(1);
            if !c.ok && p.state == PhyState::Ok {
                p.state = PhyState::Mismatch; // never lose a failure
            }
            return;
        }
        p.checks.push(CheckRecord {
            dev: c.at.dev.get(),
            reg: c.at.reg,
            want: c.want,
            mask: c.mask,
            got: c.got,
            until: c.until,
            ok: c.ok,
            io_error: c.io_error,
        });
    }

    /// Sets every PHY's state. A PHY is [`PhyState::Constant`] when at least 3 registers were
    /// read, all returned one value and that is not what the script expects.
    pub fn classify(&mut self) {
        for p in &mut self.phys {
            let first = p.checks.first().map_or(0, |c| c.got);
            let bad = p.state == PhyState::Mismatch || p.checks.iter().any(|c| !c.ok);
            let io = p.checks.iter().any(|c| c.io_error);
            let same = p.checks.iter().all(|c| c.got == first);
            p.state = if io {
                PhyState::Io
            } else if !bad {
                PhyState::Ok
            } else if p.checks.len() >= 3 && same {
                p.constant = first;
                PhyState::Constant
            } else {
                PhyState::Mismatch
            };
        }
    }

    /// The PHYs, in order of first appearance.
    pub fn phys(&self) -> &[PhyReport] {
        &self.phys
    }

    /// Checks beyond the table sizes.
    pub const fn dropped(&self) -> u64 {
        self.dropped
    }

    /// How many PHYs are in `state`.
    pub fn count(&self, state: PhyState) -> usize {
        self.phys.iter().filter(|p| p.state == state).count()
    }

    /// The PHY at address word `phy`.
    pub fn find(&self, phy: u16) -> Option<&PhyReport> {
        self.phys.iter().find(|p| p.phy == phy)
    }

    /// One row per PHY: name, address, the value of every verified register (`!` after one that
    /// differs), state.
    pub fn table(&self, out: &mut Vec<u8>, names: Option<&Names>) {
        let mut cols: Vec<(u8, u16)> = Vec::new();
        let mut width = 4;
        for p in &self.phys {
            if let Some(name) = names.and_then(|n| n.phy(u32::from(p.phy))) {
                width = width.max(name.len());
            }
            for c in &p.checks {
                if !cols.contains(&(c.dev, c.reg)) && cols.len() < MAX_CHECKS {
                    cols.push((c.dev, c.reg));
                }
            }
        }
        pad(out, b"name", width);
        out.extend_from_slice(b"  ");
        pad(out, b"phy", 6);
        for &(dev, reg) in &cols {
            let head = names
                .and_then(|n| n.reg(u32::from(dev), u32::from(reg)))
                .map_or_else(|| format!("{dev:x}.{reg:04x}").into_bytes(), <[u8]>::to_vec);
            out.push(b' ');
            pad(out, head.get(..7).unwrap_or(&head), 7);
        }
        out.extend_from_slice(b" state\n");
        for p in &self.phys {
            let name = names.and_then(|n| n.phy(u32::from(p.phy))).unwrap_or(b"-");
            pad(out, name, width);
            put!(out, "  0x{:03x} ", p.phy);
            for &(dev, reg) in &cols {
                out.push(b' ');
                match p.checks.iter().find(|c| c.dev == dev && c.reg == reg) {
                    None => pad(out, b"-", 7),
                    Some(c) if c.io_error => pad(out, b"error!", 7),
                    Some(c) => {
                        put!(out, "{:04x}", c.got);
                        pad(out, if c.ok { b"" } else { b"!" }, 3);
                    }
                }
            }
            out.push(b' ');
            out.extend_from_slice(state_text(p).as_bytes());
            out.push(b'\n');
        }
        if self.dropped > 0 {
            put!(
                out,
                "({} verification reads beyond the table size are not shown)\n",
                self.dropped
            );
        }
    }
}

fn state_text(p: &PhyReport) -> String {
    match p.state {
        PhyState::Ok => "ok".to_owned(),
        PhyState::Constant => format!("CONSTANT 0x{:04x}: not answering or hung", p.constant),
        PhyState::Mismatch => "MISMATCH".to_owned(),
        PhyState::Io => "MDIO ERROR".to_owned(),
    }
}
