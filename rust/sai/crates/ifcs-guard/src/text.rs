//! Bounded text the way `ifcs_guard.c` builds it: `snprintf` into fixed buffers (no allocation),
//! the journal's pending buffer with its exact bookkeeping, and `%p`.

use std::fmt;

/// `snprintf(buf, buf.len(), ...)`: stores at most `buf.len() - 1` bytes and a terminating NUL
/// (nothing when `buf` is empty), counts every byte the format produced (the return value).
pub(crate) struct Snprintf<'a> {
    buf: &'a mut [u8],
    total: usize,
}

impl<'a> Snprintf<'a> {
    pub(crate) fn new(buf: &'a mut [u8]) -> Self {
        Snprintf { buf, total: 0 }
    }

    /// Formats `args` like `snprintf` and returns its return value (the untruncated length).
    pub(crate) fn print(buf: &'a mut [u8], args: fmt::Arguments<'_>) -> usize {
        let mut w = Snprintf::new(buf);
        let _ = fmt::Write::write_fmt(&mut w, args);
        w.finish()
    }

    /// Writes the terminating NUL; returns the untruncated length.
    pub(crate) fn finish(self) -> usize {
        let room = self.buf.len().saturating_sub(1);
        if let Some(nul) = self.buf.get_mut(self.total.min(room)) {
            *nul = 0;
        }
        self.total
    }
}

impl fmt::Write for Snprintf<'_> {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        let room = self.buf.len().saturating_sub(1);
        let start = self.total.min(room);
        let n = s.len().min(room - start);
        if let (Some(dst), Some(src)) = (self.buf.get_mut(start..start + n), s.as_bytes().get(..n)) {
            dst.copy_from_slice(src);
        }
        self.total = self.total.saturating_add(s.len());
        Ok(())
    }
}

/// glibc's `%p`: `0x` and lower-case hex, `(nil)` for a null pointer.
pub(crate) struct Ptr(pub(crate) usize);

impl fmt::Display for Ptr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.0 == 0 {
            f.write_str("(nil)")
        } else {
            write!(f, "{:#x}", self.0)
        }
    }
}

/// Size of the journal's pending buffer (`char pending[2048]`).
pub(crate) const PENDING: usize = 2048;

/// `pending` / `pending_len` of `ifcs_guard.c`: lines formatted under the guard's lock, written to
/// the journal after it is released. `len` is the C's `pending_len` with its arithmetic (it adds
/// what `snprintf` returned, so after a truncation it can exceed what the buffer holds; the C's
/// later checks and `take` handle that, and so do these).
pub(crate) struct Pending {
    buf: [u8; PENDING],
    pub(crate) len: usize,
}

impl Pending {
    pub(crate) const fn new() -> Self {
        Pending {
            buf: [0; PENDING],
            len: 0,
        }
    }

    /// `snprintf(pending + pending_len, sizeof(pending) - pending_len, ...)`; the C calls it only
    /// with `pending_len < sizeof(pending)` (here: a no-op returning 0 otherwise).
    pub(crate) fn put(&mut self, args: fmt::Arguments<'_>) -> usize {
        match self.buf.get_mut(self.len..) {
            Some(rest) if !rest.is_empty() => Snprintf::print(rest, args),
            _ => 0,
        }
    }

    /// The end of `say_locked`: clamp, newline, NUL.
    pub(crate) fn end_line(&mut self) {
        if self.len > PENDING - 2 {
            self.len = PENDING - 2;
        }
        if let Some(c) = self.buf.get_mut(self.len) {
            *c = b'\n';
        }
        self.len += 1;
        if let Some(c) = self.buf.get_mut(self.len) {
            *c = 0;
        }
    }

    /// `take_pending_locked(out, cap)` then what `print_out` writes: the first
    /// `min(pending_len, cap - 1)` bytes up to the first NUL (`fputs`). Empties the buffer.
    pub(crate) fn take<'o>(&mut self, cap: usize, out: &'o mut [u8; PENDING]) -> &'o [u8] {
        let n = self.len.min(cap.saturating_sub(1)).min(PENDING);
        self.len = 0;
        let (Some(dst), Some(src)) = (out.get_mut(..n), self.buf.get(..n)) else {
            return &[];
        };
        dst.copy_from_slice(src);
        let text = out.get(..n).unwrap_or(&[]);
        let end = text.iter().position(|&c| c == 0).unwrap_or(n);
        text.get(..end).unwrap_or(&[])
    }
}

/// A C string in a fixed buffer, `snprintf(buf, sizeof(buf), "%s", s)`: at most `N - 1` bytes.
pub(crate) struct CPath<const N: usize> {
    buf: [u8; N],
    len: usize,
}

impl<const N: usize> CPath<N> {
    pub(crate) const fn new() -> Self {
        CPath { buf: [0; N], len: 0 }
    }

    /// Stores `s` up to its first NUL, truncated to `N - 1` bytes.
    pub(crate) fn set(&mut self, s: &[u8]) {
        let s = s.split(|&c| c == 0).next().unwrap_or(&[]);
        let n = s.len().min(N.saturating_sub(1));
        if let (Some(dst), Some(src)) = (self.buf.get_mut(..n), s.get(..n)) {
            dst.copy_from_slice(src);
        }
        self.len = n;
    }

    /// `snprintf(buf, N, "%s%s", a, b)`: `a` then `b`, truncated to `N - 1` bytes.
    pub(crate) fn set_joined(&mut self, a: &[u8], b: &[u8]) {
        self.set(a);
        let n = b.len().min(N.saturating_sub(1).saturating_sub(self.len));
        if let (Some(dst), Some(src)) = (self.buf.get_mut(self.len..self.len + n), b.get(..n)) {
            dst.copy_from_slice(src);
            self.len += n;
        }
    }

    pub(crate) fn bytes(&self) -> &[u8] {
        self.buf.get(..self.len).unwrap_or(&[])
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.len == 0
    }
}

#[cfg(test)]
mod tests {
    use super::{CPath, PENDING, Pending, Ptr, Snprintf};

    #[test]
    fn snprintf_truncates_and_counts() {
        let mut b = [0xaa_u8; 6];
        assert_eq!(Snprintf::print(&mut b, format_args!("abc{}", 12345)), 8);
        assert_eq!(&b, b"abc12\0");
        let mut e = [0_u8; 0];
        assert_eq!(Snprintf::print(&mut e, format_args!("xyz")), 3);
        let mut one = [7_u8; 1];
        assert_eq!(Snprintf::print(&mut one, format_args!("xyz")), 3);
        assert_eq!(one, [0]);
    }

    #[test]
    fn pointer_like_glibc() {
        assert_eq!(Ptr(0).to_string(), "(nil)");
        assert_eq!(Ptr(0xdead_beef).to_string(), "0xdeadbeef");
    }

    #[test]
    fn pending_take_respects_the_callers_buffer() {
        let mut p = Pending::new();
        let k = p.put(format_args!("{}", "x".repeat(700)));
        p.len += k;
        p.end_line();
        let mut out = [0_u8; PENDING];
        assert_eq!(p.take(512, &mut out).len(), 511);
        assert_eq!(p.len, 0);
        assert_eq!(p.take(2048, &mut out), b"");
    }

    #[test]
    fn paths_truncate_like_snprintf() {
        let mut c = CPath::<4>::new();
        c.set(b"abcdef");
        assert_eq!(c.bytes(), b"abc");
        c.set(b"a\0b");
        assert_eq!(c.bytes(), b"a");
        c.set(b"");
        assert!(c.is_empty());
    }
}
