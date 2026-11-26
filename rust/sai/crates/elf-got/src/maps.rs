//! The mappings of this process (`/proc/self/maps`): where a GOT slot's page is mapped and with
//! which protection, so the rim reads only mapped memory and puts the protection back as it was.
#![forbid(unsafe_code)]

/// One mapping: `[start, end)` and its protection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mapping {
    /// First byte.
    pub start: usize,
    /// One past the last byte.
    pub end: usize,
    /// Readable.
    pub read: bool,
    /// Writable.
    pub write: bool,
    /// Executable.
    pub exec: bool,
}

/// The mappings of a `/proc/<pid>/maps` text, in order; lines it cannot read are skipped.
pub fn parse(text: &str) -> Vec<Mapping> {
    text.lines()
        .filter_map(|line| {
            let mut words = line.split_whitespace();
            let (range, perms) = (words.next()?, words.next()?);
            let (start, end) = range.split_once('-')?;
            let start = usize::from_str_radix(start, 16).ok()?;
            let end = usize::from_str_radix(end, 16).ok()?;
            let p = perms.as_bytes();
            (start < end).then_some(Mapping {
                start,
                end,
                read: p.first() == Some(&b'r'),
                write: p.get(1) == Some(&b'w'),
                exec: p.get(2) == Some(&b'x'),
            })
        })
        .collect()
}

/// The mapping that holds all of `[addr, addr + len)`.
pub fn containing(maps: &[Mapping], addr: usize, len: usize) -> Option<Mapping> {
    let last = addr.checked_add(len)?;
    maps.iter().copied().find(|m| m.start <= addr && last <= m.end)
}

/// This process's mappings now.
pub fn current() -> std::io::Result<Vec<Mapping>> {
    std::fs::read_to_string("/proc/self/maps").map(|t| parse(&t))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_kernels_format() {
        let text = "\
55d0c0a00000-55d0c0a21000 r--p 00000000 fd:01 1234 /usr/bin/x
7f1e2c000000-7f1e2c021000 rw-p 00000000 00:00 0
7f1e2d800000-7f1e2d9a5000 r-xp 00028000 fd:01 99 /lib/libc.so.6
garbage
ffffffffff600000-ffffffffff601000 --xp 00000000 00:00 0 [vsyscall]
";
        let m = parse(text);
        assert_eq!(m.len(), 4);
        assert_eq!(
            m[1],
            Mapping {
                start: 0x7f1e_2c00_0000,
                end: 0x7f1e_2c02_1000,
                read: true,
                write: true,
                exec: false
            }
        );
        assert!(m[2].exec && !m[2].write);
        assert!(!m[3].read);
        assert_eq!(containing(&m, 0x7f1e_2c02_0ff8, 8), Some(m[1]));
        assert_eq!(containing(&m, 0x7f1e_2c02_0ffc, 8), None);
        assert_eq!(containing(&m, usize::MAX, 8), None);
    }

    #[test]
    fn this_process_has_its_own_code_mapped() {
        let maps = current().unwrap();
        let here = reads_the_kernels_format as fn() as usize;
        assert!(containing(&maps, here, 1).is_some_and(|m| m.exec));
    }
}
