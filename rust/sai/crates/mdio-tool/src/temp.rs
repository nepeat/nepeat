//! The ASIC temperature files that fan control and monitoring read (port of sai-openbcm's
//! `temp_files.c`, as far as the tool uses it). Temperatures are tenths of a degree C.

use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::DirBuilderExt;
use std::path::{Path, PathBuf};

/// At least one monitor, and every one within -40..150 C.
pub fn plausible(cur: &[i32]) -> bool {
    !cur.is_empty() && cur.iter().all(|t| (-400..=1500).contains(t))
}

/// The hottest current monitor (the first of equals).
pub fn hottest(cur: &[i32]) -> Option<i32> {
    cur.iter().copied().reduce(|hot, t| if t > hot { t } else { hot })
}

fn celsius(tenths: i32) -> f64 {
    f64::from(tenths) / 10.0
}

/// The text of the plain file: the hottest monitor in C.
pub fn out_text(cur: &[i32]) -> Option<String> {
    hottest(cur).map(|hot| format!("{:.1}\n", celsius(hot)))
}

/// The text of the Prometheus textfile (`switch_asic_*` gauges).
pub fn prom_text(unit: i32, cur: &[i32], peak: &[i32], now: i64) -> Option<String> {
    use std::fmt::Write as _;
    let hot = hottest(cur)?;
    let mut s = String::from(
        "# HELP switch_asic_temperature_celsius ASIC die temperature monitor, current\n\
         # TYPE switch_asic_temperature_celsius gauge\n",
    );
    for (i, t) in cur.iter().enumerate() {
        let _ = writeln!(
            s,
            "switch_asic_temperature_celsius{{unit=\"{unit}\",sensor=\"pvtmon{i}\"}} {:.1}",
            celsius(*t)
        );
    }
    s.push_str(
        "# HELP switch_asic_temperature_peak_celsius ASIC die temperature monitor, highest since the previous read\n\
         # TYPE switch_asic_temperature_peak_celsius gauge\n",
    );
    for (i, t) in peak.iter().enumerate().take(cur.len()) {
        let _ = writeln!(
            s,
            "switch_asic_temperature_peak_celsius{{unit=\"{unit}\",sensor=\"pvtmon{i}\"}} {:.1}",
            celsius(*t)
        );
    }
    let _ = write!(
        s,
        "# HELP switch_asic_temperature_max_celsius hottest current ASIC monitor (what fan control reads)\n\
         # TYPE switch_asic_temperature_max_celsius gauge\n\
         switch_asic_temperature_max_celsius{{unit=\"{unit}\"}} {:.1}\n\
         # HELP switch_asic_temperature_read_timestamp_seconds time of the last good read\n\
         # TYPE switch_asic_temperature_read_timestamp_seconds gauge\n\
         switch_asic_temperature_read_timestamp_seconds{{unit=\"{unit}\"}} {now}\n",
        celsius(hot)
    );
    Some(s)
}

/// `path` replaced atomically (`path.tmp` + rename); a missing parent directory is created
/// (one level, e.g. `/run/asic`).
fn write_atomic(path: &Path, text: &str) -> std::io::Result<()> {
    let mut tmp = path.as_os_str().as_bytes().to_vec();
    tmp.extend_from_slice(b".tmp");
    let tmp = PathBuf::from(OsStr::from_bytes(&tmp));
    let written = match std::fs::write(&tmp, text) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let made = path
                .parent()
                .filter(|d| !d.as_os_str().is_empty() && *d != Path::new("/"))
                .is_some_and(|dir| match std::fs::DirBuilder::new().mode(0o755).create(dir) {
                    Ok(()) => true,
                    Err(e) => e.kind() == std::io::ErrorKind::AlreadyExists,
                });
            if made { std::fs::write(&tmp, text) } else { Err(e) }
        }
        other => other,
    }
    .and_then(|()| std::fs::rename(&tmp, path));
    if written.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    written
}

/// Writes the plain file and, when asked, the Prometheus file. The error: the file that could
/// not be written and why (both are attempted; the plain file's failure is reported first).
pub fn write(
    out: &Path,
    prom: Option<&Path>,
    unit: i32,
    cur: &[i32],
    peak: &[i32],
    now: i64,
) -> Result<(), (PathBuf, std::io::Error)> {
    let invalid = || std::io::Error::from(std::io::ErrorKind::InvalidInput);
    let first = out_text(cur)
        .ok_or_else(invalid)
        .and_then(|text| write_atomic(out, &text))
        .map_err(|e| (out.to_owned(), e));
    let second = prom.map_or(Ok(()), |prom| {
        prom_text(unit, cur, peak, now)
            .ok_or_else(invalid)
            .and_then(|text| write_atomic(prom, &text))
            .map_err(|e| (prom.to_owned(), e))
    });
    first.and(second)
}

/// Removes both, so consumers see "no reading", not an old one.
pub fn remove(out: &Path, prom: Option<&Path>) {
    let _ = std::fs::remove_file(out);
    if let Some(prom) = prom {
        let _ = std::fs::remove_file(prom);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plausibility_and_hottest() {
        assert!(!plausible(&[]));
        assert!(plausible(&[-400, 1500]));
        assert!(!plausible(&[413, 1501]));
        assert!(!plausible(&[-401]));
        assert_eq!(hottest(&[413, 428, 428, 9]), Some(428));
        assert_eq!(hottest(&[]), None);
    }

    #[test]
    fn file_texts() {
        assert_eq!(out_text(&[413, 418]).unwrap(), "41.8\n");
        assert_eq!(out_text(&[-5]).unwrap(), "-0.5\n");
        assert_eq!(out_text(&[]), None);
        assert_eq!(
            prom_text(0, &[413, 418], &[428, 433], 1_795_000_000).unwrap(),
            "# HELP switch_asic_temperature_celsius ASIC die temperature monitor, current\n\
             # TYPE switch_asic_temperature_celsius gauge\n\
             switch_asic_temperature_celsius{unit=\"0\",sensor=\"pvtmon0\"} 41.3\n\
             switch_asic_temperature_celsius{unit=\"0\",sensor=\"pvtmon1\"} 41.8\n\
             # HELP switch_asic_temperature_peak_celsius ASIC die temperature monitor, highest since the previous read\n\
             # TYPE switch_asic_temperature_peak_celsius gauge\n\
             switch_asic_temperature_peak_celsius{unit=\"0\",sensor=\"pvtmon0\"} 42.8\n\
             switch_asic_temperature_peak_celsius{unit=\"0\",sensor=\"pvtmon1\"} 43.3\n\
             # HELP switch_asic_temperature_max_celsius hottest current ASIC monitor (what fan control reads)\n\
             # TYPE switch_asic_temperature_max_celsius gauge\n\
             switch_asic_temperature_max_celsius{unit=\"0\"} 41.8\n\
             # HELP switch_asic_temperature_read_timestamp_seconds time of the last good read\n\
             # TYPE switch_asic_temperature_read_timestamp_seconds gauge\n\
             switch_asic_temperature_read_timestamp_seconds{unit=\"0\"} 1795000000\n"
        );
    }
}
