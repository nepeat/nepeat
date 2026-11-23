//! `openbcm-asic-temp` in Rust: read the switch ASIC's die temperature monitors through the
//! Broadcom SDK and publish them for fan control and monitoring. The port of sai-openbcm's
//! `asic_temp.c`: the same command line, messages, files and exit codes, so the binary is a
//! drop-in for `openbcm-asic-temp.service` (`Restart=always`, `RestartPreventExitStatus=5`,
//! `ExecStartPost` waiting for the file).
//!
//! Attaches only (`ob_sdk_attach`, [`sdk_glue::Attached`] behind feature `sdk`): no chip
//! reset, no "init all", ports and forwarding untouched. It holds the per-unit SDK lock while it
//! runs, so bcm.user / syncd cannot start next to it: stop it first.
//!
//! ```text
//! openbcm-asic-temp --once                 print the monitors, exit
//! openbcm-asic-temp -o FILE [-p PROM] [-i SECONDS] [-u UNIT] [-s C]
//! ```
//!
//! FILE / PROM are written by `sai_core::temp` (the port of `temp_files.c`, the writer libsai
//! uses under syncd too): FILE the hottest monitor (`"41.8\n"`), PROM Prometheus gauges, each
//! replaced atomically (`.tmp` + rename), a missing parent directory made (one level). Both are
//! removed on exit and after a failed read, so a consumer sees "no reading" rather than an old
//! one.
//!
//! Exit: 0 stopped by SIGTERM / SIGINT / SIGHUP (or `--once` done, or `--help`), 1 usage,
//! 2 attach failed, 3 SDK lock held by another process, 4 the read failed (`--once`) or failed 3
//! times in a row, 5 over-temperature: when a current monitor is at or above `--shutdown-at`
//! (default 125 C, 0 = off) on 2 good reads in a row, the core PLL is powered down
//! (`ob_sdk_th_overtemp_shutdown`, as the SDK's own handler after "init all", which this reader
//! never runs), both files removed; the chip then needs a power cycle. Not in `--once` mode.
//!
//! # Shape
//!
//! [`run`] is the whole program over an [`Env`]: output streams, the SDK ([`Attacher`] /
//! [`Sensor`], implemented over `sdk-glue` in [`glue`]) and time + signals ([`Clock`]), so
//! every path is tested in the process, on `sdk-glue`'s Rust fake chip or a scripted sensor,
//! without sleeping. `main.rs` puts the process's streams, signals and clock in.
//!
//! # Differences from the C tool
//!
//! * The average in `--once` and in the first status line is formatted from the same `double`
//!   (`sum / 10.0 / n`) by Rust's formatter, which rounds the exact value as glibc's `printf`
//!   does (ties to even); the other temperatures are formatted from the integers
//!   (`sai_core::temp::celsius_text`), the same text for every value.
//! * A signal that arrives during a read ends the loop before the next sleep, as in C; the sleep
//!   itself is woken through a self-pipe (signal-hook) instead of `nanosleep`'s `EINTR`.
//! * If a signal handler cannot be installed, a line says so (C ignores `sigaction`'s result).
//! * Like every Rust program, it ignores `SIGPIPE` (C: default action).
//! * `POSIXLY_CORRECT` is not looked at.
#![forbid(unsafe_code)]

pub mod cli;
#[cfg(feature = "sdk")]
pub mod glue;

use std::ffi::{OsStr, OsString};
use std::io::Write;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

use sai_core::temp;

/// The name in every message (the C tool's).
pub const ME: &str = "openbcm-asic-temp";
/// Consecutive failed reads after which the reader exits (4).
pub const MAX_FAILS: u32 = 3;
/// Consecutive good reads at or above the limit that power the core PLL down (exit 5).
pub const HOT_READS: u32 = 2;

/// One read of the monitors, tenths of a degree C.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Reading {
    /// Current value per monitor.
    pub current: Vec<i32>,
    /// Highest value since the previous read, per monitor.
    pub peak: Vec<i32>,
}

/// The attached SDK as the reader needs it.
pub trait Sensor {
    /// Reads the monitors (`ob_sdk_temperatures`); the error is the SDK's text for its code.
    fn read(&mut self) -> Result<Reading, String>;
    /// Powers the core PLL down (`ob_sdk_th_overtemp_shutdown`); the error is the SDK's text.
    fn shutdown(&mut self) -> Result<(), String>;
}

/// An attach that failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttachError {
    /// Another process holds the unit's SDK lock (`OB_E_BUSY`: exit 3, else 2).
    pub busy: bool,
    /// The glue's reason, one line.
    pub message: String,
    /// The SDK's text for its error code (`ob_sdk_errmsg`).
    pub code_text: String,
}

/// The way to a [`Sensor`].
pub trait Attacher {
    /// Attaches unit `unit` (`ob_sdk_attach`). On success: the sensor and the glue's line.
    fn attach(&mut self, unit: i32) -> Result<(Box<dyn Sensor>, String), AttachError>;
}

/// Time and the stop signals.
pub trait Clock {
    /// The second after the discarded first read (before the signal handlers exist).
    fn settle(&mut self);
    /// Installs the stop handlers (SIGTERM, SIGINT, SIGHUP); a line on `err` for one that fails.
    fn arm(&mut self, err: &mut dyn Write);
    /// Whether a stop signal arrived.
    fn stopped(&self) -> bool;
    /// Waits `seconds`, or less when a stop signal arrives.
    fn wait(&mut self, seconds: u32);
    /// Unix seconds now (the Prometheus read timestamp).
    fn now(&self) -> i64;
}

/// What a run uses besides its arguments.
pub struct Env<'a> {
    /// Standard output (`--once`).
    pub out: &'a mut dyn Write,
    /// Standard error (every message).
    pub err: &'a mut dyn Write,
    /// The SDK.
    pub attacher: &'a mut dyn Attacher,
    /// Time and signals.
    pub clock: &'a mut dyn Clock,
}

/// The index of the hottest monitor, the first of equals (`ob_temp_hottest`'s index; its value
/// is [`temp::hottest`]).
fn hottest_index(cur: &[i32]) -> Option<usize> {
    let hot = temp::hottest(cur)?;
    cur.iter().position(|t| *t == hot)
}

/// The mean of the monitors as C prints `sum / 10.0 / n` with `%.1f` (`"0.0"` for none).
pub fn average_text(cur: &[i32]) -> String {
    // exact: at most 16 values of 32 bits; from +0.0 as C's long 0 (f64's Sum starts at -0.0)
    let sum = cur.iter().fold(0.0, |sum, t| sum + f64::from(*t));
    let n = f64::from(u32::try_from(cur.len()).unwrap_or(u32::MAX).max(1));
    format!("{:.1}", sum / 10.0 / n)
}

fn line(err: &mut dyn Write, parts: &[&[u8]]) {
    let mut text = Vec::new();
    for p in parts {
        text.extend_from_slice(p);
    }
    text.push(b'\n');
    let _ = err.write_all(&text);
}

/// The C program's `main`: `program` is `argv[0]` (in the usage text), `args` the rest.
/// Returns the exit code.
pub fn run(program: &OsStr, args: &[OsString], env: &mut Env<'_>) -> u8 {
    let opts = match cli::parse(program, args, env.err) {
        Ok(o) => o,
        Err(cli::Exit(code)) => return code,
    };
    let mut sensor = match env.attacher.attach(opts.unit) {
        Ok((sensor, message)) => {
            let _ = writeln!(env.err, "{ME}: unit {} {message}", opts.unit);
            sensor
        }
        Err(e) => {
            let _ = writeln!(env.err, "{ME}: {} ({})", e.message, e.code_text);
            return if e.busy { 3 } else { 2 };
        }
    };
    // The first read is discarded: the peak latch is garbage until a read resets it (the SDK
    // does the same dummy read at init).
    let _ = sensor.read();
    env.clock.settle();
    env.clock.arm(env.err);
    Reader {
        opts: &opts,
        out: opts.out.as_deref().map(Path::new),
        prom: opts.prom.as_deref().map(Path::new),
        fails: 0,
        hot_reads: 0,
        first: true,
    }
    .run(sensor.as_mut(), env)
}

/// The loop's state (the C locals).
struct Reader<'a> {
    opts: &'a cli::Options,
    out: Option<&'a Path>,
    prom: Option<&'a Path>,
    fails: u32,
    hot_reads: u32,
    first: bool,
}

/// What one pass of the loop decided.
enum Step {
    Again,
    Exit(u8),
}

impl Reader<'_> {
    fn run(mut self, sensor: &mut dyn Sensor, env: &mut Env<'_>) -> u8 {
        while !env.clock.stopped() {
            if let Step::Exit(code) = self.pass(sensor, env) {
                return code;
            }
            if !env.clock.stopped() {
                env.clock.wait(u32::try_from(self.opts.interval).unwrap_or(1));
            }
        }
        temp::remove_files(self.out, self.prom);
        let _ = writeln!(env.err, "{ME}: stopped");
        0
    }

    fn pass(&mut self, sensor: &mut dyn Sensor, env: &mut Env<'_>) -> Step {
        let reading = match sensor.read() {
            Ok(r) if temp::plausible(&r.current) => r,
            failed => return self.failed(&failed, env),
        };
        let cur = &reading.current;
        let (Some(hot), Some(max)) = (hottest_index(cur), temp::hottest(cur)) else {
            return Step::Again; // plausible() holds at least one monitor
        };
        let (max_text, avg_text) = (temp::celsius_text(max), average_text(cur));
        if self.opts.once {
            for (i, (c, p)) in cur.iter().zip(&reading.peak).enumerate() {
                let _ = writeln!(
                    env.out,
                    "monitor {i}: current {} C, peak {} C",
                    temp::celsius_text(*c),
                    temp::celsius_text(*p)
                );
            }
            let _ = writeln!(env.out, "max {max_text} C, average {avg_text} C");
            let _ = env.out.flush();
            return Step::Exit(0);
        }
        self.fails = 0;
        let limit = self.opts.shutdown_at;
        if limit > 0 && max >= limit.saturating_mul(10) {
            self.hot_reads = self.hot_reads.saturating_add(1);
            if self.hot_reads >= HOT_READS {
                let _ = writeln!(
                    env.err,
                    "{ME}: CRITICAL: monitor {hot} at {max_text} C (limit {limit} C, {} reads in a row): \
                     powering down the switch core PLL as the SDK's over-temperature handler does; \
                     the switch ASIC needs a power cycle",
                    self.hot_reads
                );
                temp::remove_files(self.out, self.prom);
                if let Err(text) = sensor.shutdown() {
                    let _ = writeln!(env.err, "{ME}: CRITICAL: core PLL power-down failed: {text}");
                }
                return Step::Exit(5);
            }
            let _ = writeln!(
                env.err,
                "{ME}: monitor {hot} at {max_text} C, at or above the {limit} C limit \
                 (core PLL power-down on the next such read)"
            );
        } else {
            self.hot_reads = 0;
        }
        self.publish(&reading, env);
        if self.first {
            let out = self.out.map(|p| p.as_os_str().as_bytes()).unwrap_or_default();
            let n = cur.len();
            let (interval, off) = (self.opts.interval, if limit == 0 { " (off)" } else { "" });
            line(
                env.err,
                &[
                    format!("{ME}: {n} monitors, max {max_text} C, average {avg_text} C; every {interval} s to ")
                        .as_bytes(),
                    out,
                    format!("; core PLL power-down at {limit} C{off}").as_bytes(),
                ],
            );
            self.first = false;
        }
        Step::Again
    }

    /// A read that failed or is not plausible: said, both files removed; the third in a row (or
    /// any in `--once`) ends the run with 4. The hot count stays (C `hot_reads`).
    fn failed(&mut self, read: &Result<Reading, String>, env: &mut Env<'_>) -> Step {
        let (code, n) = match read {
            Ok(r) => ("ok", r.current.len()),
            Err(text) => (text.as_str(), 0),
        };
        let implausible = if read.is_ok() && n > 0 {
            ", implausible value"
        } else {
            ""
        };
        let _ = writeln!(env.err, "{ME}: read failed: {code} ({n} monitors{implausible})");
        temp::remove_files(self.out, self.prom);
        if self.opts.once {
            return Step::Exit(4);
        }
        self.fails = self.fails.saturating_add(1);
        if self.fails >= MAX_FAILS {
            Step::Exit(4)
        } else {
            Step::Again
        }
    }

    /// Writes both files (`ob_temp_write`); a line per file that could not be written, each with
    /// the error of the last failure (C `errno`).
    fn publish(&self, reading: &Reading, env: &mut Env<'_>) {
        let unit = i64::from(self.opts.unit);
        let now = env.clock.now();
        let Err(failure) = temp::write_files(self.out, self.prom, unit, &reading.current, &reading.peak, now) else {
            return;
        };
        let why = temp::strerror(&failure.error);
        let (out_failed, prom_failed) = match failure.unwritten {
            temp::Unwritten::Out => (true, false),
            temp::Unwritten::Prom => (false, true),
            temp::Unwritten::Both => (true, true),
        };
        for (failed, path) in [(out_failed, self.out), (prom_failed, self.prom)] {
            if let (true, Some(path)) = (failed, path) {
                line(
                    env.err,
                    &[
                        ME.as_bytes(),
                        b": cannot write ",
                        path.as_os_str().as_bytes(),
                        b": ",
                        why.as_bytes(),
                    ],
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_average_is_printf_of_the_double() {
        // values whose double sits next to a decimal tie; glibc's printf("%.1f") output
        assert_eq!(average_text(&[1, 2]), "0.1"); // 0.15 is 0.1499999...
        assert_eq!(average_text(&[5, 0]), "0.2"); // 0.25 exactly: ties to even
        assert_eq!(average_text(&[7, 8]), "0.8"); // 0.75 exactly: ties to even
        assert_eq!(average_text(&[-1]), "-0.1");
        assert_eq!(average_text(&[0, -1]), "-0.1"); // -0.05 is -0.05000000000000000277
        assert_eq!(average_text(&[413, 418]), "41.5");
        assert_eq!(average_text(&[]), "0.0");
        assert_eq!(hottest_index(&[1, 9, 9, 3]), Some(1));
        assert_eq!(hottest_index(&[]), None);
    }
}
