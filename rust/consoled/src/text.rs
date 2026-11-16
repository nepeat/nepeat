//! Turning raw console output into text: escape sequences out (colours, cursor moves,
//! titles, bracketed paste), carriage returns and backspaces applied the way a terminal
//! would, the echo of a sent command dropped. For `line expect`, `line run` and the
//! scrollback view.

/// Erase-to-end-of-line (CSI K) survives cleaning as this character (VT, which is dropped
/// from the input itself) so `render` can apply it.
const ERASE: char = '\x0b';

#[derive(Default, Clone, Copy, PartialEq)]
enum St {
    #[default]
    Ground,
    Esc,
    EscInter,
    Csi,
    /// OSC, DCS, SOS, PM, APC: a string until BEL or ESC \
    Str,
    StrEsc,
}

/// Incremental: push bytes as they arrive (sequences and UTF-8 may be split anywhere),
/// read the text so far from `out`.
#[derive(Default)]
pub struct Cleaner {
    st: St,
    csi: Vec<u8>,
    utf8: Vec<u8>,
    pub out: String,
}

impl Cleaner {
    pub fn push(&mut self, bytes: &[u8]) {
        for &b in bytes {
            match self.st {
                St::Ground => match b {
                    0x1b => {
                        self.flush_utf8();
                        self.st = St::Esc;
                    }
                    b'\n' | b'\r' | b'\t' | 0x08 => {
                        self.flush_utf8();
                        self.out.push(b as char);
                    }
                    0..0x20 | 0x7f => self.flush_utf8(),
                    _ => self.utf8.push(b),
                },
                St::Esc => {
                    self.st = match b {
                        b'[' => {
                            self.csi.clear();
                            St::Csi
                        }
                        b']' | b'P' | b'X' | b'^' | b'_' => St::Str,
                        0x20..=0x2f => St::EscInter,
                        0x1b => St::Esc,
                        _ => St::Ground, // two-byte sequence (ESC 7, ESC =, ...)
                    }
                }
                St::EscInter => {
                    if !(0x20..=0x2f).contains(&b) {
                        self.st = St::Ground;
                    }
                }
                St::Csi => {
                    if (0x40..=0x7e).contains(&b) {
                        // erase in line, to the right (no parameter or 0)
                        if b == b'K' && (self.csi.is_empty() || self.csi == b"0") {
                            self.out.push(ERASE);
                        }
                        self.st = St::Ground;
                    } else if b == 0x1b {
                        self.st = St::Esc;
                    } else if self.csi.len() < 64 {
                        self.csi.push(b);
                    }
                }
                St::Str => match b {
                    0x07 => self.st = St::Ground,
                    0x1b => self.st = St::StrEsc,
                    _ => {}
                },
                St::StrEsc => self.st = if b == b'\\' { St::Ground } else { St::Str },
            }
        }
        self.flush_partial_utf8();
    }

    fn flush_utf8(&mut self) {
        if !self.utf8.is_empty() {
            self.out.push_str(&String::from_utf8_lossy(&self.utf8));
            self.utf8.clear();
        }
    }

    /// Decode what is complete; keep an unfinished UTF-8 sequence for the next push.
    fn flush_partial_utf8(&mut self) {
        let keep = match std::str::from_utf8(&self.utf8) {
            Ok(_) => 0,
            Err(e) if e.error_len().is_none() => self.utf8.len() - e.valid_up_to(),
            Err(_) => 0,
        };
        let done: Vec<u8> = self.utf8.drain(..self.utf8.len() - keep).collect();
        self.out.push_str(&String::from_utf8_lossy(&done));
    }
}

pub fn clean(bytes: &[u8]) -> String {
    let mut c = Cleaner::default();
    c.push(bytes);
    c.flush_utf8();
    c.out
}

/// One line as a terminal would show it: `\r` back to the start (later text overwrites),
/// backspace one left, erase-to-end applied.
pub fn render_line(s: &str) -> String {
    if !s.contains(['\r', '\x08', ERASE]) {
        return s.to_string();
    }
    let mut buf: Vec<char> = Vec::with_capacity(s.len());
    let mut col: usize = 0;
    for c in s.chars() {
        match c {
            '\r' => col = 0,
            '\x08' => col = col.saturating_sub(1),
            ERASE => buf.truncate(col),
            c => {
                if col < buf.len() {
                    buf[col] = c;
                } else {
                    buf.resize(col, ' ');
                    buf.push(c);
                }
                col += 1;
            }
        }
    }
    buf.into_iter().collect()
}

/// Cleaned text to rendered lines; the last one is whatever follows the final newline
/// (maybe empty: the line still being written).
pub fn lines(s: &str) -> Vec<String> {
    s.split('\n').map(render_line).collect()
}

fn squash(s: &str) -> String {
    s.chars().filter(|c| !c.is_whitespace() && *c != '\x08').collect()
}

/// Drop the echo of `cmd` from the start of cleaned output: everything up to the first
/// newline before which the text (whitespace aside, so a line wrapped by the remote shell
/// still counts, and a redrawn prompt in front is fine) ends with the command. Unchanged
/// when no echo is found near the start.
pub fn drop_echo<'a>(s: &'a str, cmd: &str) -> &'a str {
    let want = squash(cmd);
    if want.is_empty() {
        // an empty command echoes as a bare newline
        return s.find('\n').map_or(s, |p| &s[p + 1..]);
    }
    let mut from = 0;
    while let Some(p) = s[from..].find('\n') {
        let head = squash(&s[..from + p]);
        if head.ends_with(&want) {
            return &s[from + p + 1..];
        }
        if head.len() > want.len() + 512 {
            break;
        }
        from += p + 1;
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_are_stripped_even_when_split() {
        let raw =
            b"\x1b[?2004h\x1b]0;root@box: ~\x07root@box:~# \x1b[01;32mls\x1b[0m\r\n\x1b[?2004l\xc3\xa9t\xc3\xa9\r\n";
        assert_eq!(clean(raw), "root@box:~# ls\r\n\u{e9}t\u{e9}\r\n");
        let mut c = Cleaner::default();
        for b in raw {
            c.push(std::slice::from_ref(b)); // one byte at a time: same result
        }
        assert_eq!(c.out, "root@box:~# ls\r\n\u{e9}t\u{e9}\r\n");
        assert_eq!(
            clean(b"a\x1bPq#0;1\x1b\\b\x1b(Bc\x07\x00d"),
            "abcd",
            "DCS, charset, BEL, NUL"
        );
    }

    #[test]
    fn carriage_return_backspace_erase() {
        assert_eq!(render_line("50%\r100%\r"), "100%");
        assert_eq!(render_line("abc\rX"), "Xbc");
        assert_eq!(render_line("ab\x08\x08xy"), "xy");
        assert_eq!(render_line(&clean(b"long text\r\x1b[Kshort")), "short");
        assert_eq!(lines("a\r\nb\r\nprompt# "), vec!["a", "b", "prompt# "]);
    }

    #[test]
    fn echo_is_dropped() {
        assert_eq!(drop_echo("uname -a\r\nLinux\r\n# ", "uname -a"), "Linux\r\n# ");
        assert_eq!(drop_echo("root@box:~# uname -a\r\nLinux\r\n", "uname -a"), "Linux\r\n");
        // bash wrapping a long line on an 80-column serial console: "... \r" mid-command
        assert_eq!(
            drop_echo("echo aaaa bbbb cc \rcc dddd\r\nout\r\n", "echo aaaa bbbb cccc dddd"),
            "out\r\n"
        );
        assert_eq!(drop_echo("no echo here\r\n", "uname"), "no echo here\r\n");
        assert_eq!(drop_echo("\r\n# ", ""), "# ");
    }
}
