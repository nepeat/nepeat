//! The ctypes file as data: `ifcs_ctypes_model.py`'s `Model` line for line. Nothing in the file
//! is executed: the lines ctypesgen writes (assignments, `class X(Structure|Union)` stubs,
//! `_fields_` lists, `_pack_`, `argtypes` / `restype`) are matched as the Python's regular
//! expressions match them, their right-hand sides parsed by [`crate::expr`] and evaluated by the
//! small interpreter below (integers, names, unary and binary integer operators, `int(x)`,
//! `POINTER` / `CFUNCTYPE` / `UNCHECKED`, arrays). Layouts: [`crate::layout`].

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};

use crate::Fatal;
use crate::expr::{self, BinOp, Expr, IntLit, UnOp};
use crate::py;

/// One `_fields_` entry: name, type expression, bit width (`None`: not written).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Field {
    pub(crate) name: String,
    pub(crate) ty: Expr,
    pub(crate) bits: Option<u64>,
}

/// A type expression (`T` of the Python).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum T {
    Void,
    /// A ctypes primitive by its ctypes name.
    Prim(&'static str),
    Name(String),
    Ptr(Box<T>),
    Arr(Box<T>, i128),
    /// Return type, argument types.
    Fn(Box<T>, Vec<T>),
}

/// ctypes name, C spelling, size (LP64: `x86_64` and `aarch64` Linux), may hold a bitfield.
pub(crate) const PRIM: &[(&str, &str, u64, bool)] = &[
    ("c_int8", "int8_t", 1, true),
    ("c_byte", "int8_t", 1, true),
    ("c_char", "char", 1, false),
    ("c_uint8", "uint8_t", 1, true),
    ("c_ubyte", "uint8_t", 1, true),
    ("c_int16", "int16_t", 2, true),
    ("c_short", "short", 2, true),
    ("c_uint16", "uint16_t", 2, true),
    ("c_ushort", "unsigned short", 2, true),
    ("c_int32", "int32_t", 4, true),
    ("c_int", "int", 4, true),
    ("c_uint32", "uint32_t", 4, true),
    ("c_uint", "unsigned int", 4, true),
    ("c_int64", "int64_t", 8, true),
    ("c_long", "long", 8, true),
    ("c_longlong", "long long", 8, true),
    ("c_uint64", "uint64_t", 8, true),
    ("c_ulong", "unsigned long", 8, true),
    ("c_ulonglong", "unsigned long long", 8, true),
    ("c_float", "float", 4, false),
    ("c_double", "double", 8, false),
    ("c_bool", "_Bool", 1, true),
    ("c_size_t", "size_t", 8, true),
    ("c_void_p", "void*", 8, false),
    ("c_char_p", "const char*", 8, false),
    ("String", "const char*", 8, false),
    ("ReturnString", "const char*", 8, false),
];

/// The entry of a ctypes primitive.
pub(crate) fn prim(name: &str) -> Option<&'static (&'static str, &'static str, u64, bool)> {
    PRIM.iter().find(|p| p.0 == name)
}

/// The kind of a `class X(...)` stub.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AggKind {
    Structure,
    Union,
}

impl AggKind {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            AggKind::Structure => "Structure",
            AggKind::Union => "Union",
        }
    }
}

/// The model of one ctypes file.
pub struct Model {
    /// The path as given (it is part of error messages).
    pub(crate) path: String,
    /// Last assignment of each name (as at import).
    pub(crate) assign: BTreeMap<String, Expr>,
    pub(crate) aggs: BTreeMap<String, AggKind>,
    pub(crate) fields: BTreeMap<String, Vec<Field>>,
    pub(crate) pack: BTreeMap<String, u64>,
    pub(crate) args: BTreeMap<String, Vec<Expr>>,
    pub(crate) ret: BTreeMap<String, Expr>,
    /// Computed aggregate layouts (and failures), as the Python's built ctypes classes.
    pub(crate) layouts: RefCell<BTreeMap<String, Result<crate::layout::Layout, Fatal>>>,
    pub(crate) building: RefCell<BTreeSet<String>>,
}

fn syntax(what: &str, src: &str) -> Fatal {
    Fatal::Syntax(format!("{what}: {src}"))
}

/// `_expr`: drops the `L` suffix of integer literals (`\b(0[xX][0-9a-fA-F]+|\d+)[lL]\b`), parses.
pub(crate) fn parse_expr(s: &str) -> Option<Expr> {
    expr::parse(&strip_long_suffix(py::strip(s)))
}

fn strip_long_suffix(s: &str) -> String {
    let cs: Vec<char> = s.chars().collect();
    let word = |k: usize| cs.get(k).copied().is_some_and(py::is_word);
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while let Some(&c) = cs.get(i) {
        let boundary = i == 0 || !word(i - 1);
        if boundary && c.is_ascii_digit() {
            let hex = c == '0'
                && matches!(cs.get(i + 1), Some('x' | 'X'))
                && cs.get(i + 2).is_some_and(char::is_ascii_hexdigit);
            let mut j = if hex { i + 2 } else { i };
            let digit = |c: &char| if hex { c.is_ascii_hexdigit() } else { c.is_ascii_digit() };
            while cs.get(j).is_some_and(digit) {
                j += 1;
            }
            let mut matched = matches!(cs.get(j), Some('l' | 'L')) && !word(j + 1);
            if !matched && hex {
                // the decimal alternative: "0" then [lL] (never: x follows), kept for exactness
                j = i;
                while cs.get(j).is_some_and(char::is_ascii_digit) {
                    j += 1;
                }
                matched = matches!(cs.get(j), Some('l' | 'L')) && !word(j + 1);
            }
            if matched {
                out.extend(cs.get(i..j).unwrap_or_default());
                i = j + 1;
                continue;
            }
        }
        out.push(c);
        i += 1;
    }
    out
}

/// `^\s+\('(\w+)', (.+?)(?:, (\d+))?\),\s*$`: a field line's name, type text, bits text.
fn field_line(line: &str) -> Option<(&str, &str, Option<&str>)> {
    let rest = line.trim_start_matches(py::is_space);
    if rest.len() == line.len() {
        return None;
    }
    let rest = rest.strip_prefix("('")?;
    let name_end = rest.find(|c: char| !py::is_word(c)).unwrap_or(rest.len());
    let name = rest.get(..name_end).filter(|n| !n.is_empty())?;
    let body = rest.get(name_end..)?.strip_prefix("', ")?;
    let tail_ok = |t: &str| t.strip_prefix("),").is_some_and(|r| r.chars().all(py::is_space));
    for (k, _) in body.char_indices().skip(1).chain(std::iter::once((body.len(), ' '))) {
        let (ty, tail) = (body.get(..k)?, body.get(k..)?);
        if let Some(d) = tail.strip_prefix(", ") {
            let n = d.find(|c: char| !c.is_ascii_digit()).unwrap_or(d.len());
            if n > 0 && tail_ok(d.get(n..)?) {
                return Some((name, ty, d.get(..n)));
            }
        }
        if tail_ok(tail) {
            return Some((name, ty, None));
        }
    }
    None
}

/// A leading `\w+` and the rest.
fn word_prefix(s: &str) -> Option<(&str, &str)> {
    let n = s.find(|c: char| !py::is_word(c)).unwrap_or(s.len());
    (n > 0).then(|| s.split_at(n))
}

/// `split_top`: commas at bracket depth 0 separate; a trailing empty part is dropped.
fn split_top(s: &str) -> Vec<String> {
    let (mut out, mut depth, mut cur) = (Vec::new(), 0i64, String::new());
    for ch in s.chars() {
        if matches!(ch, '(' | '[') {
            depth += 1;
        }
        if matches!(ch, ')' | ']') {
            depth -= 1;
        }
        if ch == ',' && depth == 0 {
            out.push(std::mem::take(&mut cur));
        } else {
            cur.push(ch);
        }
    }
    if !py::strip(&cur).is_empty() {
        out.push(cur);
    }
    out
}

impl Model {
    /// Reads a ctypes file's text (as `open(path, encoding="utf-8", errors="replace")`).
    pub fn parse(path: &str, bytes: &[u8]) -> Result<Model, Fatal> {
        let mut m = Model {
            path: path.to_owned(),
            assign: BTreeMap::new(),
            aggs: BTreeMap::new(),
            fields: BTreeMap::new(),
            pack: BTreeMap::new(),
            args: BTreeMap::new(),
            ret: BTreeMap::new(),
            layouts: RefCell::new(BTreeMap::new()),
            building: RefCell::new(BTreeSet::new()),
        };
        let src = String::from_utf8_lossy(bytes);
        let lines = py::splitlines(&src);
        // the preamble (ctypesgen's own helpers) ends at the library load
        let start = lines
            .iter()
            .position(|l| l.strip_prefix("_libs[").is_some_and(|r| r.contains("] = load_library")))
            .map_or(lines.len(), |i| i + 1);
        let mut cur: Option<String> = None;
        for line in lines.iter().skip(start) {
            m.line(line, &mut cur)?;
        }
        Ok(m)
    }

    #[allow(
        clippy::too_many_lines,
        reason = "the model's line matcher, one regular expression after the other"
    )]
    fn line(&mut self, line: &str, cur: &mut Option<String>) -> Result<(), Fatal> {
        if let Some(agg) = cur.as_ref() {
            if line.starts_with(']') {
                *cur = None;
            } else if let Some((name, ty, bits)) = field_line(line) {
                let ty = parse_expr(ty).ok_or_else(|| syntax("field type", ty))?;
                let bits = bits
                    .map(|b| b.parse::<u64>().map_err(|_| Fatal::Limit(format!("bit width {b}"))))
                    .transpose()?;
                self.fields.entry(agg.clone()).or_default().push(Field {
                    name: name.to_owned(),
                    ty,
                    bits,
                });
            }
            return Ok(());
        }
        if let Some(rest) = line.strip_prefix("class ") {
            if let Some((name, r)) = word_prefix(rest) {
                let kind = if r.starts_with("(Structure):") {
                    Some(AggKind::Structure)
                } else if r.starts_with("(Union):") {
                    Some(AggKind::Union)
                } else {
                    None
                };
                if let Some(k) = kind {
                    self.aggs.insert(name.to_owned(), k);
                    return Ok(());
                }
            }
        }
        if let Some((name, r)) = word_prefix(line) {
            if let Some(after) = r.strip_prefix("._fields_ = [") {
                if after.chars().all(py::is_space) {
                    self.fields.insert(name.to_owned(), Vec::new());
                    *cur = Some(name.to_owned());
                    return Ok(());
                }
            }
            if let Some(after) = r.strip_prefix("._pack_ = ") {
                let n = after.find(|c: char| !c.is_ascii_digit()).unwrap_or(after.len());
                if n > 0 {
                    let v = after.get(..n).unwrap_or_default();
                    let v = v.parse::<u64>().map_err(|_| Fatal::Limit(format!("_pack_ {v}")))?;
                    self.pack.insert(name.to_owned(), v);
                    return Ok(());
                }
            }
        }
        let indented = line.trim_start_matches(py::is_space);
        if indented.len() < line.len() {
            if let Some((name, r)) = word_prefix(indented) {
                if let Some(after) = r.strip_prefix(".argtypes = [") {
                    if let Some(inner) = py::rstrip(after).strip_suffix(']') {
                        let parts = if py::strip(inner).is_empty() {
                            Vec::new()
                        } else {
                            split_top(inner)
                        };
                        let args = parts
                            .iter()
                            .map(|a| parse_expr(a).ok_or_else(|| syntax("argtype", a)))
                            .collect::<Result<Vec<_>, _>>()?;
                        self.args.insert(name.to_owned(), args);
                        return Ok(());
                    }
                }
                if let Some(after) = r.strip_prefix(".restype = ") {
                    let v = py::rstrip(after);
                    let v = if v.is_empty() {
                        after.get(..1).unwrap_or_default()
                    } else {
                        v
                    };
                    if !v.is_empty() {
                        let e = parse_expr(v).ok_or_else(|| syntax("restype", v))?;
                        self.ret.entry(name.to_owned()).or_insert(e);
                        return Ok(());
                    }
                }
            }
        }
        // ^( {4})?(\w+) = (.+?)\s*$
        let body = line
            .strip_prefix("    ")
            .filter(|b| word_prefix(b).is_some())
            .unwrap_or(line);
        if let Some((name, r)) = word_prefix(body) {
            if let Some(after) = r.strip_prefix(" = ") {
                let v = py::rstrip(after);
                let v = if v.is_empty() {
                    after.get(..1).unwrap_or_default()
                } else {
                    v
                };
                if !v.is_empty() && !v.starts_with("_lib.") && !name.starts_with('_') {
                    if let Some(e) = parse_expr(v) {
                        self.assign.insert(name.to_owned(), e);
                    }
                }
            }
        }
        Ok(())
    }

    // ------------------------------------------------------------ values

    /// Integer value of a constant (enum member or `#define`), or `None`.
    pub(crate) fn value(&self, name: &str, depth: u32) -> Result<Option<i128>, Fatal> {
        match self.assign.get(name) {
            None => Ok(None),
            Some(e) => self.eval(e, depth),
        }
    }

    #[allow(clippy::many_single_char_names, reason = "the Python's names (e, v, a, b)")]
    pub(crate) fn eval(&self, e: &Expr, depth: u32) -> Result<Option<i128>, Fatal> {
        if depth > 50 {
            return Ok(None);
        }
        Ok(match e {
            Expr::Int(IntLit::Small(v)) => Some(*v),
            Expr::Int(IntLit::Big(d)) => {
                return Err(Fatal::Limit(format!(
                    "integer {d} is outside the range ifcs-gen computes in"
                )));
            }
            Expr::Name(n) => self.value(n, depth + 1)?,
            Expr::Unary(op, x) => match self.eval(x, depth + 1)? {
                None => None,
                Some(v) => match op {
                    UnOp::Neg => Some(v.checked_neg().ok_or_else(|| overflow("-", v, 0))?),
                    UnOp::Pos => Some(v),
                    UnOp::Invert => Some(!v),
                    UnOp::Not => None,
                },
            },
            Expr::Call(f, args, _) if matches!(&**f, Expr::Name(n) if n == "int") && args.len() == 1 => {
                match args.first() {
                    Some(a) => self.eval(a, depth + 1)?,
                    None => None,
                }
            }
            Expr::Bin(op, l, r) => {
                let (a, b) = (self.eval(l, depth + 1)?, self.eval(r, depth + 1)?);
                match (a, b) {
                    (Some(a), Some(b)) => binop(*op, a, b)?,
                    _ => None,
                }
            }
            _ => None,
        })
    }

    // ------------------------------------------------------------ types

    /// `typ`: an expression as a type (names kept: [`Model::resolve`] follows them).
    pub(crate) fn typ(&self, e: &Expr) -> Result<T, Fatal> {
        match e {
            Expr::NoneConst => return Ok(T::Void),
            Expr::Name(n) => {
                return Ok(match prim(n) {
                    Some(p) => T::Prim(p.0),
                    None => T::Name(n.clone()),
                });
            }
            Expr::Call(f, args, _) => {
                if let Expr::Name(f) = &**f {
                    let first = || {
                        args.first()
                            .ok_or_else(|| Fatal::Index(format!("{f}() without arguments: list index out of range")))
                    };
                    match f.as_str() {
                        "POINTER" => return Ok(T::Ptr(Box::new(self.typ(first()?)?))),
                        "UNCHECKED" => return self.typ(first()?),
                        "CFUNCTYPE" => {
                            let restype = self.typ(first()?)?;
                            let rest = args
                                .iter()
                                .skip(1)
                                .map(|a| self.typ(a))
                                .collect::<Result<Vec<_>, _>>()?;
                            return Ok(T::Fn(Box::new(restype), rest));
                        }
                        _ => {}
                    }
                }
            }
            Expr::Bin(BinOp::Mult, l, r) => {
                if let Some(n) = self.eval(r, 0)? {
                    return Ok(T::Arr(Box::new(self.typ(l)?), n));
                }
            }
            _ => {}
        }
        Err(Fatal::Value(format!("unsupported type expression {e:?}")))
    }

    /// `resolve`: follows name aliases to a primitive, an aggregate, a pointer, array, function or void.
    pub(crate) fn resolve(&self, t: &T) -> Result<T, Fatal> {
        let mut t = t.clone();
        let mut seen = 0;
        while let T::Name(n) = &t {
            if self.aggs.contains_key(n) {
                break;
            }
            let Some(e) = self.assign.get(n) else {
                return Err(Fatal::Key(format!("{}: type {n} not defined", self.path)));
            };
            t = self.typ(e)?;
            seen += 1;
            if seen > 50 {
                return Err(Fatal::Value(format!("alias loop at {t:?}")));
            }
        }
        Ok(t)
    }

    /// `agg_of`: the `struct_x` / `union_x` a typedef name names.
    pub(crate) fn agg_of(&self, typedef: &str) -> Option<String> {
        let t = if self.aggs.contains_key(typedef) {
            T::Name(typedef.to_owned())
        } else {
            match prim(typedef) {
                Some(p) => T::Prim(p.0),
                None => T::Name(typedef.to_owned()),
            }
        };
        match self.resolve(&t) {
            Ok(T::Name(a)) if self.aggs.contains_key(&a) => Some(a),
            _ => None,
        }
    }
}

fn overflow(op: &str, a: i128, b: i128) -> Fatal {
    Fatal::Limit(format!("{a} {op} {b} is outside the range ifcs-gen computes in"))
}

/// The binary integer operators the model evaluates (others: `None`), with Python's semantics.
fn binop(op: BinOp, a: i128, b: i128) -> Result<Option<i128>, Fatal> {
    let neg_shift = || Fatal::Value("negative shift count".into());
    Ok(Some(match op {
        BinOp::Add => a.checked_add(b).ok_or_else(|| overflow("+", a, b))?,
        BinOp::Sub => a.checked_sub(b).ok_or_else(|| overflow("-", a, b))?,
        BinOp::Mult => a.checked_mul(b).ok_or_else(|| overflow("*", a, b))?,
        BinOp::LShift => {
            if b < 0 {
                return Err(neg_shift());
            }
            if a == 0 {
                0
            } else {
                let s = u32::try_from(b)
                    .ok()
                    .filter(|s| *s < 127)
                    .ok_or_else(|| overflow("<<", a, b))?;
                let v = a.checked_shl(s).ok_or_else(|| overflow("<<", a, b))?;
                if v >> s != a {
                    return Err(overflow("<<", a, b));
                }
                v
            }
        }
        BinOp::RShift => {
            if b < 0 {
                return Err(neg_shift());
            }
            match u32::try_from(b).ok().filter(|s| *s < 127) {
                Some(s) => a >> s,
                None => {
                    if a < 0 {
                        -1
                    } else {
                        0
                    }
                }
            }
        }
        BinOp::BitOr => a | b,
        BinOp::BitAnd => a & b,
        BinOp::BitXor => a ^ b,
        BinOp::FloorDiv => {
            if b == 0 {
                return Err(Fatal::ZeroDivision("integer division or modulo by zero".into()));
            }
            let q = a.checked_div(b).ok_or_else(|| overflow("//", a, b))?;
            if a % b != 0 && ((a < 0) != (b < 0)) { q - 1 } else { q }
        }
        _ => return Ok(None),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_suffix() {
        assert_eq!(strip_long_suffix("10L + 0x1FL + 0L"), "10 + 0x1F + 0");
        assert_eq!(strip_long_suffix("AB1L x1L 12UL 1Lx"), "AB1L x1L 12UL 1Lx");
    }

    #[test]
    fn field_lines() {
        assert_eq!(field_line("    ('a', c_int),"), Some(("a", "c_int", None)));
        assert_eq!(field_line("    ('d', c_uint8, 1),"), Some(("d", "c_uint8", Some("1"))));
        assert_eq!(
            field_line("    ('f', CFUNCTYPE(c_int, c_int, 3)),  "),
            Some(("f", "CFUNCTYPE(c_int, c_int, 3)", None))
        );
        assert_eq!(field_line("    ('a', c_uint8 * 6),"), Some(("a", "c_uint8 * 6", None)));
        assert_eq!(field_line("('a', c_int),"), None);
        assert_eq!(field_line("    ('a', c_int)"), None);
    }

    #[test]
    fn python_integer_semantics() {
        assert_eq!(binop(BinOp::FloorDiv, -7, 2), Ok(Some(-4)));
        assert_eq!(binop(BinOp::FloorDiv, 7, -2), Ok(Some(-4)));
        assert_eq!(binop(BinOp::FloorDiv, 7, 2), Ok(Some(3)));
        assert_eq!(binop(BinOp::RShift, -1, 200), Ok(Some(-1)));
        assert_eq!(binop(BinOp::LShift, 1, 32), Ok(Some(1 << 32)));
        assert_eq!(binop(BinOp::Div, 1, 1), Ok(None));
        assert!(matches!(binop(BinOp::LShift, 1, -1), Err(Fatal::Value(_))));
        assert!(matches!(binop(BinOp::FloorDiv, 1, 0), Err(Fatal::ZeroDivision(_))));
        assert!(matches!(binop(BinOp::LShift, 1, 200), Err(Fatal::Limit(_))));
    }
}
