//! The file side of [`crate::Io`] with `std` (what the C did with stdio).

use std::ffi::OsStr;
use std::fs::File;
use std::io::{Read, Write};
use std::os::unix::ffi::OsStrExt as _;
use std::path::Path;

use crate::{Scan, scan_inject};

fn path(p: &[u8]) -> &Path {
    Path::new(OsStr::from_bytes(p))
}

/// `fopen(tmp, "w")`, `fwrite`, `fclose`, then `rename(tmp, path)`, or `unlink(tmp)` when the write
/// failed. False when `tmp` could not be created. (The C also counted a failing `fclose`; `std`
/// closes without reporting, so a write-back error at close is not seen here: the file is renamed.)
pub fn write_stats(tmp: &[u8], target: &[u8], data: &[u8]) -> bool {
    let Ok(mut f) = File::create(path(tmp)) else {
        return false;
    };
    let ok = f.write_all(data).is_ok();
    drop(f);
    let _ = if ok {
        std::fs::rename(path(tmp), path(target))
    } else {
        std::fs::remove_file(path(tmp))
    };
    true
}

/// The bytes of a reader through a small fixed buffer (no allocation), ending at the first error.
struct Chunks<R> {
    r: R,
    buf: [u8; 128],
    pos: usize,
    len: usize,
}

impl<R: Read> Iterator for Chunks<R> {
    type Item = u8;

    fn next(&mut self) -> Option<u8> {
        if self.pos >= self.len {
            self.len = self.r.read(&mut self.buf).ok()?;
            self.pos = 0;
            if self.len == 0 {
                return None;
            }
        }
        let c = self.buf.get(self.pos).copied()?;
        self.pos += 1;
        Some(c)
    }
}

/// `fopen(path, "r")` (`None` when that fails), `fscanf(f, "%x %u", ...)`, `fclose`, `unlink`.
pub fn take_inject(file: &[u8]) -> Option<Scan> {
    let f = File::open(path(file)).ok()?;
    let scan = scan_inject(Chunks {
        r: f,
        buf: [0; 128],
        pos: 0,
        len: 0,
    });
    let _ = std::fs::remove_file(path(file));
    Some(scan)
}

#[cfg(test)]
mod tests {
    use super::{take_inject, write_stats};
    use std::os::unix::ffi::OsStrExt as _;

    #[test]
    fn stats_are_renamed_into_place_and_inject_files_removed() {
        let dir = std::env::temp_dir().join(format!("ifcs-guard-fs-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let target = dir.join("g.prom");
        let tmp = dir.join("g.prom.tmp");
        assert!(write_stats(
            tmp.as_os_str().as_bytes(),
            target.as_os_str().as_bytes(),
            b"x 1\n"
        ));
        assert_eq!(std::fs::read(&target).unwrap(), b"x 1\n");
        assert!(!tmp.exists());
        let missing = dir.join("no/such.prom.tmp");
        assert!(!write_stats(missing.as_os_str().as_bytes(), b"/nonexistent", b"x"));

        let inj = dir.join("inject");
        assert_eq!(take_inject(inj.as_os_str().as_bytes()), None);
        std::fs::write(&inj, format!("{}4860 7", " ".repeat(300))).unwrap();
        let s = take_inject(inj.as_os_str().as_bytes()).unwrap();
        assert_eq!((s.got, s.off, s.cnt), (2, 0x4860, 7));
        assert!(!inj.exists());
        // end of file right after a digit, the first byte of the file a digit too
        std::fs::write(&inj, "4864 3").unwrap();
        let s = take_inject(inj.as_os_str().as_bytes()).unwrap();
        assert_eq!((s.got, s.off, s.cnt), (2, 0x4864, 3));
        std::fs::write(&inj, "4").unwrap();
        assert_eq!(take_inject(inj.as_os_str().as_bytes()).unwrap().got, 1);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
