//! A tiny pattern matcher for `line expect` and `line run --prompt` (no regex crate).
//!
//! Matched against one line of cleaned output at a time (escape sequences stripped,
//! `\r` and backspace applied), anywhere in the line unless anchored:
//!
//!   abc      the literal text (most characters stand for themselves)
//!   .        any one character
//!   [a-z_]   one of these (ranges allowed); [^...] none of these; `]` first is literal
//!   \d \s \w digit, whitespace, word character; \t a tab; \x any other x literally
//!   x* x+ x? zero or more, one or more, zero or one of the atom before (greedy)
//!   ^        (first) the start of the line
//!   $        (last) the end of the line, or the end of the output so far: a prompt
//!            has no newline after it, so `'# $'` matches `root@box:~# `
//!
//! `^` and `$` anywhere else are literal. No groups, no alternation.

#[derive(Debug, Clone, PartialEq)]
enum Atom {
    Char(char),
    Any,
    Class { neg: bool, ranges: Vec<(char, char)> },
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Rep {
    One,
    Star,
    Plus,
    Opt,
}

#[derive(Debug, Clone)]
pub struct Pattern {
    atoms: Vec<(Atom, Rep)>,
    start: bool,
    end: bool,
    src: String,
}

fn class_escape(c: char) -> Option<Vec<(char, char)>> {
    Some(match c {
        'd' => vec![('0', '9')],
        's' => vec![(' ', ' '), ('\t', '\t'), ('\n', '\n'), ('\r', '\r'), ('\x0b', '\x0c')],
        'w' => vec![('a', 'z'), ('A', 'Z'), ('0', '9'), ('_', '_')],
        _ => return None,
    })
}

fn plain_escape(c: char) -> char {
    match c {
        't' => '\t',
        c => c,
    }
}

impl Pattern {
    pub fn parse(src: &str) -> Result<Pattern, String> {
        let mut cs: Vec<char> = src.chars().collect();
        let start = cs.first() == Some(&'^');
        if start {
            cs.remove(0);
        }
        // a trailing $ anchors unless escaped (an odd number of backslashes before it)
        let end = cs.last() == Some(&'$') && cs.iter().rev().skip(1).take_while(|&&c| c == '\\').count() % 2 == 0;
        if end {
            cs.pop();
        }
        let mut atoms: Vec<(Atom, Rep)> = Vec::new();
        let mut i = 0;
        while i < cs.len() {
            let c = cs[i];
            i += 1;
            let atom = match c {
                '.' => Atom::Any,
                '*' | '+' | '?' => {
                    let rep = match c {
                        '*' => Rep::Star,
                        '+' => Rep::Plus,
                        _ => Rep::Opt,
                    };
                    match atoms.last_mut() {
                        Some((_, r @ Rep::One)) => *r = rep,
                        _ => return Err(format!("pattern {src:?}: {c} needs something before it")),
                    }
                    continue;
                }
                '\\' => {
                    let Some(&e) = cs.get(i) else {
                        return Err(format!("pattern {src:?}: ends with \\"));
                    };
                    i += 1;
                    match class_escape(e) {
                        Some(ranges) => Atom::Class { neg: false, ranges },
                        None => Atom::Char(plain_escape(e)),
                    }
                }
                '[' => {
                    let neg = cs.get(i) == Some(&'^');
                    if neg {
                        i += 1;
                    }
                    let mut ranges = Vec::new();
                    let mut first = true;
                    loop {
                        let Some(&c) = cs.get(i) else {
                            return Err(format!("pattern {src:?}: [ without ]"));
                        };
                        i += 1;
                        if c == ']' && !first {
                            break;
                        }
                        first = false;
                        let lo = if c == '\\' {
                            let Some(&e) = cs.get(i) else {
                                return Err(format!("pattern {src:?}: [ without ]"));
                            };
                            i += 1;
                            if let Some(r) = class_escape(e) {
                                ranges.extend(r);
                                continue;
                            }
                            plain_escape(e)
                        } else {
                            c
                        };
                        if cs.get(i) == Some(&'-') && cs.get(i + 1).is_some_and(|&c| c != ']') {
                            let hi = cs[i + 1];
                            i += 2;
                            if hi < lo {
                                return Err(format!("pattern {src:?}: range {lo}-{hi} is backwards"));
                            }
                            ranges.push((lo, hi));
                        } else {
                            ranges.push((lo, lo));
                        }
                    }
                    Atom::Class { neg, ranges }
                }
                c => Atom::Char(c),
            };
            atoms.push((atom, Rep::One));
        }
        Ok(Pattern {
            atoms,
            start,
            end,
            src: src.to_string(),
        })
    }

    pub fn as_str(&self) -> &str {
        &self.src
    }

    /// Anchored at the end: a match on a line still being written may stop matching when
    /// more arrives (callers wait for the output to settle).
    pub fn anchored_end(&self) -> bool {
        self.end
    }

    /// The first (leftmost) match in `line`: byte offsets `(start, end)`.
    pub fn find(&self, line: &str) -> Option<(usize, usize)> {
        let chars: Vec<(usize, char)> = line.char_indices().collect();
        let byte = |ci: usize| chars.get(ci).map_or(line.len(), |&(b, _)| b);
        let text: Vec<char> = chars.iter().map(|&(_, c)| c).collect();
        let last_start = if self.start { 0 } else { text.len() };
        for s in 0..=last_start {
            if let Some(e) = self.here(0, &text, s) {
                return Some((byte(s), byte(e)));
            }
        }
        None
    }

    pub fn is_match(&self, line: &str) -> bool {
        self.find(line).is_some()
    }

    /// Match atoms[ai..] at text[ti..]; the end index on success.
    fn here(&self, ai: usize, text: &[char], ti: usize) -> Option<usize> {
        let Some((atom, rep)) = self.atoms.get(ai) else {
            return if !self.end || ti == text.len() { Some(ti) } else { None };
        };
        let one = |t: usize| text.get(t).is_some_and(|&c| matches(atom, c));
        match rep {
            Rep::One => {
                if one(ti) {
                    self.here(ai + 1, text, ti + 1)
                } else {
                    None
                }
            }
            Rep::Opt => {
                if one(ti) {
                    if let Some(e) = self.here(ai + 1, text, ti + 1) {
                        return Some(e);
                    }
                }
                self.here(ai + 1, text, ti)
            }
            Rep::Star | Rep::Plus => {
                let mut n = 0;
                while one(ti + n) {
                    n += 1;
                }
                let min = if *rep == Rep::Plus { 1 } else { 0 };
                (min..=n).rev().find_map(|k| self.here(ai + 1, text, ti + k))
            }
        }
    }
}

fn matches(atom: &Atom, c: char) -> bool {
    match atom {
        Atom::Char(x) => *x == c,
        Atom::Any => true,
        Atom::Class { neg, ranges } => ranges.iter().any(|&(lo, hi)| lo <= c && c <= hi) != *neg,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(p: &str, s: &str) -> Option<(usize, usize)> {
        Pattern::parse(p).unwrap().find(s)
    }

    #[test]
    fn literals_and_anchors() {
        assert_eq!(m("login:", "cs8210 login: "), Some((7, 13)));
        assert_eq!(m("^login", "cs8210 login:"), None);
        assert_eq!(m("^cs", "cs8210 login:"), Some((0, 2)));
        assert_eq!(m("# $", "root@cs8210:~# "), Some((13, 15)));
        assert_eq!(m("# $", "root@cs8210:~# ls"), None);
        assert_eq!(m("a$b", "xa$by"), Some((1, 4)), "$ inside is literal");
        assert_eq!(m("\\$", "costs 5$"), Some((7, 8)), "escaped trailing $ is literal");
        assert_eq!(m("", "anything"), Some((0, 0)));
        assert_eq!(m("^$", ""), Some((0, 0)));
    }

    #[test]
    fn wildcards_classes_repeats() {
        assert_eq!(m("U-Boot 20..\\.", "U-Boot 2023.04"), Some((0, 12)));
        assert_eq!(m("[#$>] ?$", "=> "), Some((1, 3)));
        assert_eq!(m("[#$>] ?$", "Switch#"), Some((6, 7)));
        assert_eq!(m("[#$>] ?$", "a > b"), None);
        assert_eq!(m("^[^#]*# $", "root@box:~# "), Some((0, 12)));
        assert_eq!(m("\\d+ packets", "rx 1234 packets"), Some((3, 15)));
        assert_eq!(m("ab*c", "ac abbbc"), Some((0, 2)));
        assert_eq!(m("ab+c", "ac abbbc"), Some((3, 8)));
        assert_eq!(m("colou?r", "color colour"), Some((0, 5)));
        assert_eq!(m("[]x]", "a]"), Some((1, 2)));
        assert_eq!(m("[a-]", "-"), Some((0, 1)));
        assert_eq!(m("x.*y$", "x1y2y"), Some((0, 5)));
        assert_eq!(m("é.", "café!"), Some((3, 6)), "byte offsets of UTF-8 text");
    }

    #[test]
    fn bad_patterns() {
        for p in ["*a", "a\\", "[abc", "[z-a]", "^*"] {
            assert!(Pattern::parse(p).is_err(), "{p}");
        }
    }
}
