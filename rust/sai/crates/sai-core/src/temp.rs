//! ASIC temperatures: the switch's temperature attributes and the publisher thread that writes
//! fan control's files. Ports `sai_temp.cpp` and `temp_files.c`.
//!
//! - [`switch_temp_get`]: `SAI_SWITCH_ATTR_MAX_NUMBER_OF_TEMP_SENSORS` / `TEMP_LIST` /
//!   `MAX_TEMP` / `AVERAGE_TEMP` ([`is_temp_attr`]), whole degrees C rounded half away from
//!   zero (C++ `std::lround(t / 10.0)`), each get a fresh read of the backend.
//! - [`Publisher`]: while this process owns the chip, a thread reads the monitors every
//!   `SAI_OPENBCM_TEMP_INTERVAL` seconds through a [`TempReader`] (without the SAI lock, as the
//!   C++ thread calls `asic_temperatures` unlocked) and keeps `SAI_OPENBCM_TEMP_FILE` (the hottest
//!   monitor, `"41.8\n"`, what fand reads) and `SAI_OPENBCM_TEMP_PROM` (Prometheus text) up to
//!   date; [`write_files`] is `ob_temp_write` byte for byte. A failed or implausible read removes
//!   both files: consumers see "no reading", never an old one. No over-temperature action here
//!   (after "init all" the SDK's own interrupt handler is armed).
//!
//! Differences from the C++: the C++ publisher is a process-wide singleton (a second start while
//! it runs is ignored); here each [`Publisher::start`] makes its own thread and the caller keeps
//! one. The C++ `atexit` handler (`temp_atexit`: `exit()` without `remove_switch` removes the
//! files) has no safe-Rust counterpart; dropping the [`Publisher`] (switch removal,
//! `sai_api_uninitialize`) stops it and removes the files. A thread that cannot be started is
//! logged and no publisher runs (C++: `std::thread` throws). Temperatures are formatted from
//! the integers, not through a `double` (the same text for every `i32`).
use crate::backend::types::AsicTemps;
use crate::backend::{Backend, Profile};
use crate::error::{AttrError, Error, Result};
use crate::value::Value;
use crate::{keys, log};
use sai_sys as sys;
use std::ffi::OsStr;
use std::io::Write as _;
use std::os::unix::ffi::OsStrExt as _;
use std::os::unix::fs::DirBuilderExt as _;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// A temperature read the publisher thread may call at any time, without the SAI lock (what
/// [`Backend::temperature_reader`] hands out): the backend's
/// [`asic_temperatures`](crate::backend::Temp::asic_temperatures) on state it shares with
/// the backend.
pub type TempReader = Arc<dyn Fn() -> Result<AsicTemps> + Send + Sync>;

/// Device nodes of the Broadcom kernel modules (C++ `sai_switch.cpp` `hardware_present`).
const HARDWARE_MARKERS: [&str; 2] = ["/dev/linux-user-bde", "/dev/linux-kernel-bde"];

/// The coldest plausible monitor value, tenths of a degree C (-40 C).
const PLAUSIBLE_MIN: i32 = -400;
/// The hottest plausible monitor value, tenths of a degree C (150 C).
const PLAUSIBLE_MAX: i32 = 1500;
/// The C++ Prometheus text buffer (`char text[8192]`, one byte for the terminating NUL).
const PROM_TEXT_MAX: usize = 8191;
/// Linux `ENOSPC`: what `ob_temp_write` reports for a Prometheus text that does not fit.
const ENOSPC: i32 = 28;
/// Linux `EINVAL`: `ob_temp_write` without monitors.
const EINVAL: i32 = 22;
/// `SAI_OPENBCM_TEMP_INTERVAL` when absent or out of range, seconds.
const DEFAULT_INTERVAL: u64 = 5;

/// Whether `attr` is one of the switch's temperature attributes (C++ `is_temp_attr`).
pub fn is_temp_attr(attr: sys::sai_attr_id_t) -> bool {
    matches!(
        attr,
        sys::SAI_SWITCH_ATTR_MAX_NUMBER_OF_TEMP_SENSORS
            | sys::SAI_SWITCH_ATTR_TEMP_LIST
            | sys::SAI_SWITCH_ATTR_MAX_TEMP
            | sys::SAI_SWITCH_ATTR_AVERAGE_TEMP
    )
}

/// One switch temperature attribute from a fresh read (C++ `switch_temp_get`). The backend is
/// read first, whatever `attr` is. Errors carry attribute index 0 (the caller re-indexes with
/// [`Error::at_index`]): a backend `InvalidParameter` / `NotSupported` / `NotImplemented` as
/// `INVALID_ATTR_VALUE` / `ATTR_NOT_SUPPORTED` / `ATTR_NOT_IMPLEMENTED` (C++
/// `attr_backend_status`), any other backend error as it is; no monitor is `Failure`; an
/// attribute that is not a temperature attribute `ATTR_NOT_SUPPORTED`.
pub fn switch_temp_get<B: Backend + ?Sized>(backend: &B, attr: sys::sai_attr_id_t) -> Result<Value> {
    let temps = backend.asic_temperatures().map_err(attr_backend_status)?;
    let cur = &temps.cur;
    let Some(hot) = hottest(cur) else {
        return Err(Error::Failure);
    };
    match attr {
        // C++ (uint8_t)cur.size(): wraps above 255 monitors.
        sys::SAI_SWITCH_ATTR_MAX_NUMBER_OF_TEMP_SENSORS => {
            Ok(Value::U8(u8::try_from(cur.len() % 256).unwrap_or(u8::MAX)))
        }
        sys::SAI_SWITCH_ATTR_TEMP_LIST => Ok(Value::I32List(cur.iter().map(|t| whole_degrees(*t)).collect())),
        sys::SAI_SWITCH_ATTR_MAX_TEMP => Ok(Value::I32(whole_degrees(hot))),
        sys::SAI_SWITCH_ATTR_AVERAGE_TEMP => Ok(Value::I32(average_degrees(cur))),
        _ => Err(Error::attr(AttrError::NotSupported, 0)),
    }
}

/// C++ `attr_backend_status(st, 0)`.
fn attr_backend_status(error: Error) -> Error {
    match error {
        Error::InvalidParameter => Error::attr(AttrError::InvalidValue, 0),
        Error::NotSupported => Error::attr(AttrError::NotSupported, 0),
        Error::NotImplemented => Error::attr(AttrError::NotImplemented, 0),
        other => other,
    }
}

/// `numerator / (10 * denominator)` rounded half away from zero (C++ `lround(x / 10.0)`; exact
/// where the C++ `double`s are, for every `i32` monitor and any average of fewer than 10^9).
fn round_tenths(numerator: i64, denominator: i64) -> i32 {
    let den = i128::from(denominator.max(1)) * 10;
    let num = i128::from(numerator);
    let magnitude = (2 * num.abs() + den) / (2 * den);
    let rounded = if num < 0 { -magnitude } else { magnitude };
    i32::try_from(rounded).unwrap_or(if rounded < 0 { i32::MIN } else { i32::MAX })
}

/// Tenths of a degree C as whole degrees (C++ `tenths_to_c`).
pub fn whole_degrees(tenths: i32) -> i32 {
    round_tenths(i64::from(tenths), 1)
}

/// The monitors' mean in whole degrees (C++ `tenths_to_c((double)sum / cur.size())`); 0 for none.
pub fn average_degrees(cur: &[i32]) -> i32 {
    let sum: i64 = cur.iter().map(|t| i64::from(*t)).sum();
    round_tenths(sum, i64::try_from(cur.len()).unwrap_or(i64::MAX))
}

/// Whether there is at least one monitor and every one is within -40..150 C
/// (`ob_temp_plausible`).
pub fn plausible(cur: &[i32]) -> bool {
    !cur.is_empty() && cur.iter().all(|t| (PLAUSIBLE_MIN..=PLAUSIBLE_MAX).contains(t))
}

/// The hottest current monitor, the first of equals (`ob_temp_hottest`); `None` for none.
pub fn hottest(cur: &[i32]) -> Option<i32> {
    cur.iter().copied().reduce(|hot, t| if t > hot { t } else { hot })
}

/// Tenths of a degree as C's `%.1f` prints `tenths / 10.0` (`-55`: `"-5.5"`, `-5`: `"-0.5"`).
pub fn celsius_text(tenths: i32) -> String {
    let magnitude = i64::from(tenths).abs();
    let sign = if tenths < 0 { "-" } else { "" };
    format!("{sign}{}.{}", magnitude / 10, magnitude % 10)
}

/// The text of `SAI_OPENBCM_TEMP_FILE`: the hottest monitor and a newline; `None` for none.
pub fn out_text(cur: &[i32]) -> Option<String> {
    hottest(cur).map(|hot| format!("{}\n", celsius_text(hot)))
}

/// The Prometheus text of `SAI_OPENBCM_TEMP_PROM` (`ob_temp_write`), `now` the read time in
/// Unix seconds; `None` for no monitor. Peaks beyond the current monitors are left out.
pub fn prom_text(unit: i64, cur: &[i32], peak: &[i32], now: i64) -> Option<String> {
    use core::fmt::Write as _;
    let hot = hottest(cur)?;
    let mut s = String::from(
        "# HELP switch_asic_temperature_celsius ASIC die temperature monitor, current\n\
         # TYPE switch_asic_temperature_celsius gauge\n",
    );
    for (i, t) in cur.iter().enumerate() {
        let _ = writeln!(
            s,
            "switch_asic_temperature_celsius{{unit=\"{unit}\",sensor=\"pvtmon{i}\"}} {}",
            celsius_text(*t)
        );
    }
    s.push_str(
        "# HELP switch_asic_temperature_peak_celsius ASIC die temperature monitor, highest since the previous read\n\
         # TYPE switch_asic_temperature_peak_celsius gauge\n",
    );
    for (i, t) in peak.iter().take(cur.len()).enumerate() {
        let _ = writeln!(
            s,
            "switch_asic_temperature_peak_celsius{{unit=\"{unit}\",sensor=\"pvtmon{i}\"}} {}",
            celsius_text(*t)
        );
    }
    let _ = write!(
        s,
        "# HELP switch_asic_temperature_max_celsius hottest current ASIC monitor (what fan control reads)\n\
         # TYPE switch_asic_temperature_max_celsius gauge\n\
         switch_asic_temperature_max_celsius{{unit=\"{unit}\"}} {}\n\
         # HELP switch_asic_temperature_read_timestamp_seconds time of the last good read\n\
         # TYPE switch_asic_temperature_read_timestamp_seconds gauge\n\
         switch_asic_temperature_read_timestamp_seconds{{unit=\"{unit}\"}} {now}\n",
        celsius_text(hot)
    );
    Some(s)
}

/// Which of the two files [`write_files`] could not write (C++ `OB_TEMP_W_*` bits).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unwritten {
    /// `OB_TEMP_W_OUT`: the plain file.
    Out,
    /// `OB_TEMP_W_PROM`: the Prometheus file.
    Prom,
    /// Both.
    Both,
}

/// Why [`write_files`] failed: which files, and the error of the last failure (C++ `errno`).
#[derive(Debug)]
pub struct WriteFailure {
    /// The files not written.
    pub unwritten: Unwritten,
    /// The last failure's error.
    pub error: std::io::Error,
}

/// The error's text as C's `strerror` gives it (without Rust's ` (os error N)`).
pub fn strerror(error: &std::io::Error) -> String {
    let text = error.to_string();
    match error.raw_os_error() {
        Some(code) => text
            .strip_suffix(&format!(" (os error {code})"))
            .map_or_else(|| text.clone(), str::to_owned),
        None => text,
    }
}

/// `path` with `.tmp` appended.
fn tmp_path(path: &Path) -> PathBuf {
    let mut tmp = path.as_os_str().as_bytes().to_vec();
    tmp.extend_from_slice(b".tmp");
    PathBuf::from(OsStr::from_bytes(&tmp))
}

/// C `mkparent`: creates the directory part of `path` (one level, mode 0755); an existing one is
/// fine. `None`: `path` has no directory part but the root (the C -1 without `mkdir`).
fn make_parent(path: &Path) -> Option<std::io::Result<()>> {
    let bytes = path.as_os_str().as_bytes();
    let slash = bytes.iter().rposition(|b| *b == b'/').filter(|i| *i > 0)?;
    let dir = Path::new(OsStr::from_bytes(bytes.get(..slash)?));
    Some(match std::fs::DirBuilder::new().mode(0o755).create(dir) {
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Ok(()),
        other => other,
    })
}

/// C `write_atomic`: `path.tmp` written, then renamed over `path`; a missing parent
/// directory is made once. On failure the temporary file is removed.
fn write_atomic(path: &Path, text: &str) -> std::io::Result<()> {
    let tmp = tmp_path(path);
    let file = match std::fs::File::create(&tmp) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => match make_parent(path) {
            Some(Ok(())) => std::fs::File::create(&tmp),
            Some(Err(mkdir)) => Err(mkdir),
            None => Err(e),
        },
        other => other,
    };
    let mut file = file?;
    let written = file.write_all(text.as_bytes()).and_then(|()| {
        drop(file);
        std::fs::rename(&tmp, path)
    });
    if written.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    written
}

/// Writes the plain file and the Prometheus file (each optional), each atomically, the plain
/// one first (`ob_temp_write`). A Prometheus text longer than the C++ buffer is not written
/// (`ENOSPC`); no monitor writes nothing (`EINVAL`).
pub fn write_files(
    out: Option<&Path>,
    prom: Option<&Path>,
    unit: i64,
    cur: &[i32],
    peak: &[i32],
    now: i64,
) -> core::result::Result<(), WriteFailure> {
    let unwritten = |out_failed: bool, prom_failed: bool| match (out_failed, prom_failed) {
        (true, true) => Some(Unwritten::Both),
        (true, false) => Some(Unwritten::Out),
        (false, true) => Some(Unwritten::Prom),
        (false, false) => None,
    };
    let Some(out_line) = out_text(cur) else {
        return match unwritten(out.is_some(), prom.is_some()) {
            Some(unwritten) => Err(WriteFailure {
                unwritten,
                error: std::io::Error::from_raw_os_error(EINVAL),
            }),
            None => Ok(()),
        };
    };
    let mut error = None;
    let out_failed = out.is_some_and(|out| write_atomic(out, &out_line).map_err(|e| error = Some(e)).is_err());
    let prom_failed = prom.is_some_and(|prom| {
        let text = prom_text(unit, cur, peak, now).unwrap_or_default();
        let result = if text.len() > PROM_TEXT_MAX {
            Err(std::io::Error::from_raw_os_error(ENOSPC))
        } else {
            write_atomic(prom, &text)
        };
        result.map_err(|e| error = Some(e)).is_err()
    });
    match (unwritten(out_failed, prom_failed), error) {
        (Some(unwritten), Some(error)) => Err(WriteFailure { unwritten, error }),
        _ => Ok(()),
    }
}

/// Removes both files (absent ones skipped), so consumers see "no reading" (`ob_temp_remove`).
pub fn remove_files(out: Option<&Path>, prom: Option<&Path>) {
    for path in [out, prom].into_iter().flatten() {
        let _ = std::fs::remove_file(path);
    }
}

/// Whether the host has a switch chip's kernel modules: `/dev/linux-user-bde` or
/// `/dev/linux-kernel-bde` exists (C++ `hardware_present`; the C++ starts no publisher for the
/// fake backend next to a chip).
pub fn hardware_present() -> bool {
    HARDWARE_MARKERS.iter().any(|m| Path::new(m).exists())
}

/// Unix seconds now (C `time(NULL)`).
fn unix_now() -> i64 {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(after) => i64::try_from(after.as_secs()).unwrap_or(i64::MAX),
        Err(before) => i64::try_from(before.duration().as_secs()).map_or(i64::MIN, |s| -s),
    }
}

/// What the publisher thread was started with.
#[derive(Debug)]
struct Settings {
    out: Option<PathBuf>,
    prom: Option<PathBuf>,
    unit: i64,
    interval: u64,
}

impl Settings {
    fn remove(&self) {
        remove_files(self.out.as_deref(), self.prom.as_deref());
    }
}

/// Shared by the thread and its owner: the stop flag (its lock also guards the files) and the
/// wake-up.
#[derive(Debug, Default)]
struct Control {
    stop: Mutex<bool>,
    wake: Condvar,
}

impl Control {
    fn lock(&self) -> MutexGuard<'_, bool> {
        // A panic in the thread while it held the lock leaves only the flag behind: keep it.
        self.stop.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// The thread that keeps fan control's temperature files up to date (C++
/// `temp_publisher_start` / `temp_publisher_stop`). Dropping it stops it as [`Publisher::stop`]
/// does.
pub struct Publisher {
    control: Arc<Control>,
    settings: Arc<Settings>,
    thread: Option<JoinHandle<()>>,
}

impl core::fmt::Debug for Publisher {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Publisher")
            .field("settings", &self.settings)
            .field("running", &self.thread.is_some())
            .finish_non_exhaustive()
    }
}

impl Publisher {
    /// Starts the thread when the profile names a file: `SAI_OPENBCM_TEMP_FILE` and / or
    /// `SAI_OPENBCM_TEMP_PROM` (both empty or absent: `None`), every
    /// `SAI_OPENBCM_TEMP_INTERVAL` seconds (`atoi`; outside 1..=3600 a warning and 5), unit
    /// `SAI_OPENBCM_UNIT` (`atoi`, 0) in the Prometheus labels. `reader` is called from the
    /// thread, never under the SAI lock.
    pub fn start(profile: &Profile, reader: TempReader) -> Option<Self> {
        let out = keys::get(profile, keys::TEMP_FILE);
        let prom = keys::get(profile, keys::TEMP_PROM);
        if out.is_empty() && prom.is_empty() {
            return None;
        }
        let interval = u64::try_from(keys::atoi(profile, keys::TEMP_INTERVAL, 5))
            .ok()
            .filter(|seconds| (1..=3600).contains(seconds))
            .unwrap_or_else(|| {
                log::warn(
                    sys::SAI_API_SWITCH,
                    format_args!(
                        "SAI_OPENBCM_TEMP_INTERVAL '{}' out of 1..3600; using 5",
                        keys::get(profile, keys::TEMP_INTERVAL)
                    ),
                );
                DEFAULT_INTERVAL
            });
        let path = |p: &str| (!p.is_empty()).then(|| PathBuf::from(p));
        let settings = Arc::new(Settings {
            out: path(out),
            prom: path(prom),
            unit: keys::atoi(profile, keys::UNIT, 0),
            interval,
        });
        let control = Arc::new(Control::default());
        let thread = {
            let (control, settings) = (Arc::clone(&control), Arc::clone(&settings));
            std::thread::Builder::new().spawn(move || run(&control, &settings, &*reader))
        };
        match thread {
            Ok(thread) => Some(Self {
                control,
                settings,
                thread: Some(thread),
            }),
            Err(e) => {
                log::error(
                    sys::SAI_API_SWITCH,
                    format_args!("ASIC temperature: cannot start the publisher thread: {}", strerror(&e)),
                );
                None
            }
        }
    }

    /// Stops the thread (waits for a read in progress), then removes both files.
    pub fn stop(self) {
        drop(self);
    }
}

impl Drop for Publisher {
    fn drop(&mut self) {
        let Some(thread) = self.thread.take() else { return };
        *self.control.lock() = true;
        self.control.wake.notify_all();
        // A panic in the thread (a reader's) has stopped it already; the files go either way.
        let _ = thread.join();
        let _files = self.control.lock();
        self.settings.remove();
    }
}

/// The thread (C++ `temp_loop`): read without the lock, then under it check for a stop, write
/// or remove the files, log transitions, and wait for the interval or a stop.
fn run(control: &Control, settings: &Settings, reader: &(dyn Fn() -> Result<AsicTemps> + Send + Sync)) {
    let (mut good_before, mut first, mut write_warned) = (false, true, false);
    let mut stop = control.lock();
    while !*stop {
        drop(stop);
        let read = reader();
        stop = control.lock();
        if *stop {
            break;
        }
        match &read {
            Ok(t) if t.peak.len() == t.cur.len() && plausible(&t.cur) => {
                let written = write_files(
                    settings.out.as_deref(),
                    settings.prom.as_deref(),
                    settings.unit,
                    &t.cur,
                    &t.peak,
                    unix_now(),
                );
                if let Err(failure) = &written {
                    if !write_warned {
                        // The C++ names the plain file whenever it failed, with the last error.
                        let path = match failure.unwritten {
                            Unwritten::Out | Unwritten::Both => settings.out.as_deref(),
                            Unwritten::Prom => settings.prom.as_deref(),
                        };
                        log::warn(
                            sys::SAI_API_SWITCH,
                            format_args!(
                                "ASIC temperature: cannot write {}: {}",
                                path.map(Path::display).map(|d| d.to_string()).unwrap_or_default(),
                                strerror(&failure.error)
                            ),
                        );
                    }
                }
                write_warned = written.is_err();
                if !good_before {
                    log::notice(
                        sys::SAI_API_SWITCH,
                        format_args!(
                            "ASIC temperature: {} monitors, max {} C; every {} s to {}{}{}",
                            t.cur.len(),
                            celsius_text(hottest(&t.cur).unwrap_or_default()),
                            settings.interval,
                            settings
                                .out
                                .as_deref()
                                .map_or_else(|| "-".to_owned(), |p| p.display().to_string()),
                            if settings.prom.is_some() { " and " } else { "" },
                            settings
                                .prom
                                .as_deref()
                                .map(|p| p.display().to_string())
                                .unwrap_or_default(),
                        ),
                    );
                }
                good_before = true;
            }
            other => {
                settings.remove();
                if good_before || first {
                    let (status, monitors) = match other {
                        Ok(t) => (sys::SAI_STATUS_SUCCESS, t.cur.len()),
                        Err(e) => (e.status(), 0),
                    };
                    log::warn(
                        sys::SAI_API_SWITCH,
                        format_args!(
                            "ASIC temperature read failed (status {status}, {monitors} monitors{}); files removed",
                            if other.is_ok() { ", implausible value" } else { "" }
                        ),
                    );
                }
                good_before = false;
            }
        }
        first = false;
        stop = control
            .wake
            .wait_timeout_while(stop, Duration::from_secs(settings.interval), |stop| !*stop)
            .unwrap_or_else(PoisonError::into_inner)
            .0;
    }
    settings.remove();
    drop(stop);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[allow(clippy::cast_possible_truncation, reason = "the C++ formula, values in range")]
    fn rounding_is_the_cpp_lround() {
        for t in -20_000..=20_000 {
            let cpp = (f64::from(t) / 10.0).round() as i32;
            assert_eq!(whole_degrees(t), cpp, "{t}");
        }
        assert_eq!(whole_degrees(415), 42);
        assert_eq!(whole_degrees(-55), -6);
        assert_eq!(whole_degrees(-54), -5);
        assert_eq!(average_degrees(&[-55, 1205]), 58);
        assert_eq!(average_degrees(&(0..8).map(|i| 400 + 15 * i).collect::<Vec<_>>()), 45);
        assert_eq!(whole_degrees(i32::MIN), -214_748_365);
        assert_eq!(average_degrees(&[]), 0);
    }

    #[test]
    #[allow(clippy::cast_possible_truncation, reason = "the C++ formula, values in range")]
    fn averages_match_the_double_formula() {
        let samples: [&[i32]; 6] = [
            &[1, 2],
            &[5, 10, 0],
            &[-5, -10, 1],
            &[412, 413, 415, 999],
            &[-1],
            &[15, 14],
        ];
        for cur in samples {
            let sum: i64 = cur.iter().map(|t| i64::from(*t)).sum();
            #[allow(clippy::cast_precision_loss, reason = "small test values")]
            let cpp = ((sum as f64 / cur.len() as f64) / 10.0).round() as i32;
            assert_eq!(average_degrees(cur), cpp, "{cur:?}");
        }
    }

    #[test]
    fn texts_are_the_c_printf_ones() {
        assert_eq!(celsius_text(-55), "-5.5");
        assert_eq!(celsius_text(-5), "-0.5");
        assert_eq!(celsius_text(0), "0.0");
        assert_eq!(celsius_text(505), "50.5");
        assert_eq!(celsius_text(i32::MIN), "-214748364.8");
        assert_eq!(out_text(&[413, 418]).as_deref(), Some("41.8\n"));
        assert_eq!(out_text(&[]), None);
        assert_eq!(prom_text(0, &[], &[], 0), None);
        assert!(plausible(&[-400, 1500]));
        assert!(!plausible(&[-401]));
        assert!(!plausible(&[1501]));
        assert!(!plausible(&[]));
        assert_eq!(hottest(&[1, 9, 9, 3]), Some(9));
        assert!(is_temp_attr(sys::SAI_SWITCH_ATTR_TEMP_LIST));
        assert!(!is_temp_attr(sys::SAI_SWITCH_ATTR_PORT_NUMBER));
    }

    #[test]
    fn errors_read_as_strerror() {
        assert_eq!(
            strerror(&std::io::Error::from_raw_os_error(2)),
            "No such file or directory"
        );
        assert_eq!(
            strerror(&std::io::Error::from_raw_os_error(ENOSPC)),
            "No space left on device"
        );
    }

    #[test]
    fn the_value_types_are_the_metadata_ones() {
        for (attr, value) in [
            (sys::SAI_SWITCH_ATTR_MAX_NUMBER_OF_TEMP_SENSORS, Value::U8(0)),
            (sys::SAI_SWITCH_ATTR_TEMP_LIST, Value::I32List(Vec::new())),
            (sys::SAI_SWITCH_ATTR_MAX_TEMP, Value::I32(0)),
            (sys::SAI_SWITCH_ATTR_AVERAGE_TEMP, Value::I32(0)),
        ] {
            let meta = sai_meta::attribute(sys::SAI_OBJECT_TYPE_SWITCH, attr).unwrap();
            assert_eq!(meta.value_type, value.value_type(), "{}", meta.name);
        }
    }
}
