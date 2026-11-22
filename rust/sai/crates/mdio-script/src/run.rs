//! Running a parsed script against a bus.

use std::fmt;
use std::ops::ControlFlow;

use crate::hooks::{Check, Hooks};
use crate::parse::Script;
use crate::types::{DelayUs, Label, MdioBus, Op, OpLetter, RegAddr, Stats, Tries, Until, bump};

const MAX_DIFFER_LOG: u64 = 20;
const TICK_CHUNK_US: u64 = 500_000;

/// What the assertions of the verification section do in a run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum VerifyMode {
    /// They stop the script like any other poll (C: `verify_line == 0`).
    #[default]
    Stop,
    /// Every one is read and handed to [`Hooks::verify`]; mismatches fail the run at its end
    /// with [`RunErrorKind::Verify`] (C: `verify_line` from the parse pass).
    Report,
    /// As [`VerifyMode::Report`], and nothing before the section runs (C: `verify_only`).
    Only,
}

/// Options of [`Script::run`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct RunOptions {
    /// How the verification section is treated.
    pub verify: VerifyMode,
}

/// Why a run stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RunErrorKind {
    /// A `P` / `N` never matched: the chip does not follow the script.
    Poll,
    /// A bus read or write failed.
    Io,
    /// The script ran to its end; verification assertions failed.
    Verify,
    /// [`Hooks::tick`] asked to stop.
    Abort,
    /// [`VerifyMode::Only`] on a script without a verification section; nothing was done.
    NoVerifySection,
}

impl RunErrorKind {
    /// The C return code (`MDIO_SCRIPT_E_*`).
    pub const fn code(self) -> i32 {
        match self {
            Self::Poll => crate::E_POLL,
            Self::Io => crate::E_IO,
            Self::Verify => crate::E_VERIFY,
            Self::Abort => crate::E_ABORT,
            Self::NoVerifySection => crate::E_SYNTAX,
        }
    }
}

/// Where and why a run stopped (the C `mdio_script_fail_t`; all zero when the stop is not about
/// an operation, as for a stop request during a delay).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Failure {
    /// Script line, 0 = not about a line.
    pub line: u64,
    /// The operation, `None` for a stop request before the line.
    pub op: Option<OpLetter>,
    /// PHY address word.
    pub phy: u16,
    /// Device, 0 when not about an operation.
    pub dev: u8,
    /// Register.
    pub reg: u16,
    /// The value the line wants or writes.
    pub want: u16,
    /// The bits compared (`ffff` for lines without a mask).
    pub mask: u16,
    /// The last value read, when `have_value`.
    pub got: u16,
    /// Reads made by a `P` / `N`.
    pub tries: u32,
    /// `got` was read from the bus.
    pub have_value: bool,
    /// The label in force.
    pub label: Label,
}

/// A run that did not succeed, with everything the C code reports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunError {
    /// The error class.
    pub kind: RunErrorKind,
    /// The counters when it stopped.
    pub stats: Stats,
    /// The failure record; for [`RunErrorKind::Verify`] the first failed assertion.
    pub failure: Failure,
    /// One line naming the script line, the label in force and the registers involved.
    pub message: String,
}

impl fmt::Display for RunError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for RunError {}

struct Stop {
    kind: RunErrorKind,
    message: String,
}

fn stop<T>(kind: RunErrorKind, message: String) -> Result<T, Stop> {
    Err(Stop { kind, message })
}

/// The bus operation of the current line, for messages and the failure record.
struct Site<'a> {
    line: u64,
    op: OpLetter,
    at: RegAddr,
    want: u16,
    mask: u16,
    label: &'a Label,
}

impl Site<'_> {
    fn io_failed(&self, what: &str) -> String {
        let Self { line, at, label, .. } = self;
        format!(
            "line {line}: {what} {:x} {:x}.{:04x} failed [{label}]",
            at.phy,
            at.dev.get(),
            at.reg
        )
    }
}

struct Run<'a, B: ?Sized, H: ?Sized> {
    bus: &'a mut B,
    hooks: &'a mut H,
    ticking: bool,
    stats: Stats,
    failure: Failure,
    first_bad: String,
}

impl<B: MdioBus + ?Sized, H: Hooks + ?Sized> Run<'_, B, H> {
    fn tick(&mut self) -> ControlFlow<()> {
        if self.ticking {
            self.hooks.tick(&self.stats)
        } else {
            ControlFlow::Continue(())
        }
    }

    /// A delay, in pieces of at most 0.5 s with a tick between them when there is one.
    fn sleep(&mut self, us: DelayUs) -> ControlFlow<()> {
        let mut us = u64::from(us.get());
        while us > TICK_CHUNK_US && self.ticking {
            self.hooks.sleep_us(TICK_CHUNK_US);
            us = us.saturating_sub(TICK_CHUNK_US);
            self.hooks.tick(&self.stats)?;
        }
        if us > 0 {
            self.hooks.sleep_us(us);
        }
        ControlFlow::Continue(())
    }

    fn record(&mut self, site: &Site<'_>, got: Option<u16>, tries: u32) {
        self.failure = Failure {
            line: site.line,
            op: Some(site.op),
            phy: site.at.phy,
            dev: site.at.dev.get(),
            reg: site.at.reg,
            want: site.want,
            mask: site.mask,
            got: got.unwrap_or(0),
            tries,
            have_value: got.is_some(),
            label: site.label.clone(),
        };
    }

    fn read(&mut self, site: &Site<'_>, tries: u32) -> Result<u16, Stop> {
        let read = self.bus.read(site.at.phy, site.at.dev, site.at.reg);
        if read.is_err() {
            self.record(site, None, tries);
        }
        read.or_else(|_| stop(RunErrorKind::Io, site.io_failed("read")))
    }

    fn poll(&mut self, site: &Site<'_>, until: Until, tries: Tries, interval: DelayUs) -> Result<(), Stop> {
        let Site {
            line,
            at,
            want,
            mask,
            label,
            ..
        } = *site;
        let (phy, dev, reg) = (at.phy, at.dev.get(), at.reg);
        let mut v = 0;
        for t in 0..tries.get() {
            if t > 0 {
                if self.sleep(interval).is_break() || self.tick().is_break() {
                    self.record(site, Some(v), t);
                    return stop(
                        RunErrorKind::Abort,
                        format!(
                            "line {line}: stopped on request while polling phy {phy:x} {dev:x}.{reg:04x} [{label}]"
                        ),
                    );
                }
                bump(&mut self.stats.poll_rereads);
                bump(&mut self.stats.reads);
            }
            v = self.read(site, t)?;
            if until.matches(v, mask, want) {
                return Ok(());
            }
        }
        self.record(site, Some(v), tries.get());
        let but = if until == Until::Equal { "" } else { "anything but " };
        stop(
            RunErrorKind::Poll,
            format!(
                "line {line}: phy {phy:x} {dev:x}.{reg:04x} reads {v:04x}, script wants {but}{want:04x} \
                 (mask {mask:04x}) after {} reads [{label}]",
                tries.get()
            ),
        )
    }

    /// An assertion of the verification section: read, report, go on.
    fn check(&mut self, site: &Site<'_>, until: Until) {
        let Site {
            line,
            at,
            want,
            mask,
            label,
            ..
        } = *site;
        let (phy, dev, reg) = (at.phy, at.dev.get(), at.reg);
        let read = self.bus.read(phy, at.dev, reg);
        let io_error = read.is_err();
        let got = read.unwrap_or(0);
        let ok = !io_error && until.matches(got, mask, want);
        bump(&mut self.stats.verify_checks);
        if !ok {
            if self.stats.verify_failed == 0 {
                self.record(site, read.ok(), 1);
                let label = label.prefix(60);
                self.first_bad = if io_error {
                    format!("line {line}: read of phy {phy:x} {dev:x}.{reg:04x} failed [{label}]")
                } else {
                    let but = if until == Until::Equal { "" } else { "anything but " };
                    format!(
                        "line {line}: phy {phy:x} {dev:x}.{reg:04x} reads {got:04x}, script wants {but}{want:04x} \
                         (mask {mask:04x}) [{label}]"
                    )
                };
            }
            bump(&mut self.stats.verify_failed);
        }
        self.hooks.verify(&Check {
            line,
            until,
            at,
            want,
            mask,
            got,
            ok,
            io_error,
            label,
        });
    }

    /// One operation that is not a label. `in_section`: assertions here are verification.
    fn step(&mut self, line: u64, op: &Op, label: &Label, in_section: bool) -> Result<(), Stop> {
        if self.tick().is_break() {
            self.failure = Failure {
                line,
                mask: u16::MAX,
                label: label.clone(),
                ..Failure::default()
            };
            return stop(
                RunErrorKind::Abort,
                format!("line {line}: stopped on request before this line [{label}]"),
            );
        }
        self.stats.count(op);
        let site = |op, at, want, mask| Site {
            line,
            op,
            at,
            want,
            mask,
            label,
        };
        match *op {
            Op::Label(_) => {}
            Op::Delay(us) => {
                if self.sleep(us).is_break() {
                    return stop(
                        RunErrorKind::Abort,
                        format!("line {line}: stopped on request during a delay [{label}]"),
                    );
                }
            }
            Op::Write { at, val } => {
                if self.bus.write(at.phy, at.dev, at.reg, val).is_err() {
                    let site = site(OpLetter::W, at, val, u16::MAX);
                    self.record(&site, None, 0);
                    return stop(RunErrorKind::Io, site.io_failed("write"));
                }
            }
            Op::Read { at } => {
                self.read(&site(OpLetter::R, at, 0, u16::MAX), 0)?;
            }
            Op::Expect { at, val, mask } => {
                let v = self.read(&site(OpLetter::E, at, val, mask), 0)?;
                if v & mask != val {
                    if self.stats.differs < MAX_DIFFER_LOG {
                        self.hooks.log(&format!(
                            "differs: line {line}: {:x} {:x}.{:04x} = {v:04x}, script {val:04x} mask {mask:04x} [{label}]",
                            at.phy,
                            at.dev.get(),
                            at.reg
                        ));
                    }
                    bump(&mut self.stats.differs);
                }
            }
            Op::Poll {
                at,
                until,
                val,
                mask,
                tries,
                interval,
            } => {
                let site = site(until.letter(), at, val, mask);
                if in_section && tries.get() == 1 {
                    self.check(&site, until);
                } else {
                    self.poll(&site, until, tries, interval)?;
                }
            }
        }
        bump(&mut self.stats.ops);
        Ok(())
    }
}

impl Script {
    /// Runs the script. The statistics are returned on success and are part of the error
    /// otherwise; [`Hooks::tick`] sees them while the script runs.
    #[allow(
        clippy::result_large_err,
        reason = "the error carries the statistics and the failure record like the C call; Ok is as large"
    )]
    pub fn run<B, H>(&self, bus: &mut B, hooks: &mut H, opts: RunOptions) -> Result<Stats, RunError>
    where
        B: MdioBus + ?Sized,
        H: Hooks + ?Sized,
    {
        let verify_line = match opts.verify {
            VerifyMode::Stop => 0,
            VerifyMode::Report | VerifyMode::Only => self.stats().verify_line,
        };
        let only = opts.verify == VerifyMode::Only;
        if only && verify_line == 0 {
            return Err(RunError {
                kind: RunErrorKind::NoVerifySection,
                stats: Stats::default(),
                failure: Failure::default(),
                message: "no verification section (no assertions at its end)".to_owned(),
            });
        }
        let ticking = hooks.wants_tick();
        let mut run = Run {
            bus,
            hooks,
            ticking,
            stats: Stats::default(),
            failure: Failure::default(),
            first_bad: String::new(),
        };
        let start = Label::start();
        let mut label = &start;
        for item in self.lines() {
            // verify_only: everything before the section is counted, not run.
            let live = !(only && item.line < verify_line);
            if let Op::Label(l) = &item.op {
                label = l;
                run.stats.count(&item.op);
                if live {
                    run.hooks.log(&l.to_string());
                }
            } else if live {
                let in_section = verify_line != 0 && item.line >= verify_line;
                if let Err(Stop { kind, message }) = run.step(item.line, &item.op, label, in_section) {
                    return Err(RunError {
                        kind,
                        stats: run.stats,
                        failure: run.failure,
                        message,
                    });
                }
            } else {
                run.stats.count(&item.op);
                bump(&mut run.stats.ops);
            }
        }
        run.stats.verify_line = verify_line;
        if run.stats.verify_failed > 0 {
            let message = format!(
                "{} of {} verification reads are not what the script expects; first: {}",
                run.stats.verify_failed, run.stats.verify_checks, run.first_bad
            );
            return Err(RunError {
                kind: RunErrorKind::Verify,
                stats: run.stats,
                failure: run.failure,
                message,
            });
        }
        Ok(run.stats)
    }
}
