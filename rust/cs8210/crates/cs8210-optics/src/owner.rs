//! Ownership of the I2C FPGA across processes: the flock every user takes (the same
//! file the Python `cs8210-qsfp` and the old `sonic_platform` used, so none of them can
//! touch the FPGA while the daemon holds it) and the dead marker that stops every
//! process until a reboot (`/run` is a tmpfs; the root port bits are sticky).

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::Path;
use std::time::{Duration, Instant};

/// Default lock file.
pub const LOCK_FILE: &str = "/run/lock/cs8210-qsfp.lock";
/// Default dead marker.
pub const DEAD_FILE: &str = "/run/cs8210-qsfp.dead";

/// The lock, held until dropped.
#[derive(Debug)]
pub struct Ownership {
    _file: File,
}

/// Take the lock exclusively, waiting up to `wait`.
pub fn acquire(path: &Path, wait: Duration) -> Result<Ownership, String> {
    if let Some(d) = path.parent() {
        fs::create_dir_all(d).map_err(|e| format!("{}: {e}", d.display()))?;
    }
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    let until = Instant::now() + wait;
    loop {
        match file.try_lock() {
            Ok(()) => return Ok(Ownership { _file: file }),
            Err(fs::TryLockError::WouldBlock) if Instant::now() < until => {
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(fs::TryLockError::WouldBlock) => {
                return Err(format!(
                    "{} is held by another process (cs8210-qsfp, an old sonic_platform?)",
                    path.display()
                ));
            }
            Err(fs::TryLockError::Error(e)) => return Err(format!("{}: {e}", path.display())),
        }
    }
}

/// The dead marker's text, if the FPGA was marked dead.
#[must_use]
pub fn dead_reason(path: &Path) -> Option<String> {
    match fs::read_to_string(path) {
        Ok(t) => Some(if t.trim().is_empty() {
            "marked dead".into()
        } else {
            t.trim().to_string()
        }),
        Err(e) if e.kind() == io::ErrorKind::NotFound => None,
        Err(e) => Some(format!("marked dead ({e})")),
    }
}

/// Write the dead marker (atomically).
pub fn mark_dead(path: &Path, bdf: &str, why: &str, now: &str) -> io::Result<()> {
    if let Some(d) = path.parent() {
        fs::create_dir_all(d)?;
    }
    let tmp = path.with_extension(format!("{}.tmp", std::process::id()));
    let mut f = File::create(&tmp)?;
    writeln!(f, "{now} {bdf}: {why}")?;
    fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lock_is_exclusive_and_dead_marker_roundtrips() {
        let d = std::env::temp_dir().join(format!("cs8210-owner-{}", std::process::id()));
        let lock = d.join("lock");
        let a = acquire(&lock, Duration::ZERO).unwrap();
        assert!(acquire(&lock, Duration::from_millis(100)).is_err());
        drop(a);
        assert!(acquire(&lock, Duration::ZERO).is_ok());
        let dead = d.join("dead");
        assert_eq!(dead_reason(&dead), None);
        mark_dead(&dead, "0002:01:00.0", "status reads 0xffffffff", "2026-11-25T00:00:00").unwrap();
        assert!(dead_reason(&dead).unwrap().contains("0xffffffff"));
    }
}
