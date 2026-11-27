//! The template filler: the Python generator `gen_ifcs_api.py`'s `Gen` line for line (its regular expressions
//! matched by hand with the same leftmost, greedy / lazy choices). The directives:
//!
//! - `NAME = @`, `#define NAME @`: NAME's value; `@{NAME}` / `@{NAME:spec}`: formatted;
//! - `@typedef A B ...`, `@struct A ...`, `@union A m1 m2 ...` (padded to the union's size when
//!   members are left out), `@alias A = B`, `@fn f ...` (`IFCS_FN(ret, f, (args))`), `@prefix A f1
//!   f2 ...`, `@bits A ...`, `@as T A ...`;
//!
//! the rules were the Python generator's docstring (the generator was deleted in H5 on 2026-11-27, last in the
//! tree at commit 767d9b0).
//! Errors the Python collects are collected here in the same words and order; an exception of
//! the Python is a [`Fatal`].

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use crate::Fatal;
use crate::expr::Expr;
use crate::model::{AggKind, Model, T, prim};
use crate::py;

const KINDS: &[&str] = &["typedef", "struct", "union", "alias", "fn", "bits", "prefix", "as"];

/// `^@(typedef|...|as)\s+(.*?)\s*(/\*.*)?$`: (kind, arguments, comment).
pub(crate) fn directive(line: &str) -> Option<(&'static str, &str, Option<&str>)> {
    let rest = line.strip_prefix('@')?;
    let (kind, after) = KINDS.iter().find_map(|k| {
        let a = rest.strip_prefix(k)?;
        a.starts_with(py::is_space).then_some((*k, a))
    })?;
    let body = after.trim_start_matches(py::is_space);
    for (k, _) in body.char_indices().chain(std::iter::once((body.len(), ' '))) {
        let tail = body.get(k..)?;
        let t = tail.trim_start_matches(py::is_space);
        if t.is_empty() {
            return Some((kind, body.get(..k)?, None));
        }
        if t.starts_with("/*") {
            return Some((kind, body.get(..k)?, Some(t)));
        }
    }
    None
}

/// `split_comments`: (code?, text) parts of a line; the comment state carries over lines.
fn split_comments(line: &str, mut in_comment: bool) -> (Vec<(bool, &str)>, bool) {
    let mut out = Vec::new();
    let mut i = 0;
    while i < line.len() {
        let rest = line.get(i..).unwrap_or_default();
        if in_comment {
            let Some(e) = rest.find("*/") else {
                out.push((false, rest));
                break;
            };
            out.push((false, rest.get(..e + 2).unwrap_or_default()));
            i += e + 2;
            in_comment = false;
            continue;
        }
        let Some(s) = rest.find("/*") else {
            out.push((true, rest));
            break;
        };
        out.push((true, rest.get(..s).unwrap_or_default()));
        i += s;
        in_comment = true;
    }
    (out, in_comment)
}

/// `selected_names`: every name a directive declares, over all templates.
pub(crate) fn selected_names(templates: &[String]) -> BTreeSet<String> {
    let mut sel = BTreeSet::new();
    for txt in templates {
        for line in py::splitlines(txt) {
            let Some((kind, args, _)) = directive(line) else {
                continue;
            };
            let a = py::split(args);
            match kind {
                "typedef" | "struct" => sel.extend(a.iter().map(|s| (*s).to_owned())),
                "union" | "prefix" | "alias" => sel.extend(a.first().map(|s| (*s).to_owned())),
                _ => {}
            }
        }
    }
    sel
}

/// `re.sub(r" (\*+)(?=\w|$)", r"\1 ", s)`.
fn star_fix(s: &str) -> String {
    let cs: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while let Some(&c) = cs.get(i) {
        if c == ' ' && cs.get(i + 1) == Some(&'*') {
            let mut j = i + 1;
            while cs.get(j) == Some(&'*') {
                j += 1;
            }
            if cs.get(j).is_none_or(|&n| py::is_word(n)) {
                out.extend(cs.get(i + 1..j).unwrap_or_default());
                out.push(' ');
                i = j;
                continue;
            }
        }
        out.push(c);
        i += 1;
    }
    out
}

/// One aggregate's selection outcome: its `struct_x` name, or the error lines.
enum Agg {
    Ok(String),
    Err(Vec<String>),
}

/// The generator's state over all templates (shared, as the Python's one `Gen`).
pub(crate) struct Gen<'m> {
    m: &'m Model,
    ov: BTreeMap<String, i128>,
    pub(crate) used_ov: BTreeSet<String>,
    pub(crate) errors: Vec<String>,
    bits: BTreeSet<String>,
    as_types: BTreeMap<String, String>,
    declared: BTreeSet<String>,
    in_comment: bool,
    sel: BTreeSet<String>,
}

const ERROR: &str = "/* ERROR */";
/// Guards where the Python would recurse until its limit (or loop forever: `peel`): a type
/// spelled through more than this many aliases / declarators is refused (ours: under 10).
const DEPTH: u32 = 100;

fn deep() -> Fatal {
    Fatal::Recursion("maximum recursion depth exceeded (type alias loop)".into())
}

impl<'m> Gen<'m> {
    pub(crate) fn new(m: &'m Model, ov: BTreeMap<String, i128>, sel: BTreeSet<String>) -> Self {
        Gen {
            m,
            ov,
            used_ov: BTreeSet::new(),
            errors: Vec::new(),
            bits: BTreeSet::new(),
            as_types: BTreeMap::new(),
            declared: BTreeSet::new(),
            in_comment: false,
            sel,
        }
    }

    pub(crate) fn overrides(&self) -> &BTreeMap<String, i128> {
        &self.ov
    }

    fn err(&mut self, msg: String) -> String {
        self.errors.push(msg);
        ERROR.to_owned()
    }

    fn value(&mut self, name: &str) -> Result<i128, Fatal> {
        if let Some(v) = self.ov.get(name) {
            self.used_ov.insert(name.to_owned());
            return Ok(*v);
        }
        if let Some(v) = self.m.value(name, 0)? {
            Ok(v)
        } else {
            {
                let path = self.m.path.clone();
                self.err(format!("no integer value for {name} in {path}"));
                Ok(0)
            }
        }
    }

    // ------------------------------------------------------------ types

    /// C spelling of a type expression around the declarator `decl`.
    fn cname(&mut self, t: &T, decl: &str, depth: u32) -> Result<String, Fatal> {
        if depth > DEPTH {
            return Err(deep());
        }
        let sp = if decl.is_empty() {
            String::new()
        } else {
            format!(" {decl}")
        };
        Ok(match t {
            T::Void => format!("void{sp}"),
            T::Prim(p) => format!("{}{sp}", prim(p).map_or("?", |p| p.1)),
            T::Name(n) => return self.cname_name(n, decl, &sp, depth),
            T::Ptr(inner) => {
                let tgt = self.peel(inner)?;
                let is_fn = match &tgt {
                    T::Fn(..) => true,
                    T::Name(a) => self.is_fn(a)?,
                    _ => false,
                };
                if is_fn {
                    return self.cname(inner, decl, depth + 1);
                }
                let d = if decl.is_empty() {
                    "*".to_owned()
                } else {
                    format!("*{decl}")
                };
                let s = self.cname(inner, &d, depth + 1)?;
                py::rstrip(&star_fix(&s)).to_owned()
            }
            T::Arr(elem, n) => self.cname(elem, &format!("{decl}[{n}]"), depth + 1)?,
            T::Fn(ret, args) => {
                let mut a = Vec::with_capacity(args.len());
                for x in args {
                    a.push(self.cname(x, "", depth + 1)?);
                }
                let a = if a.is_empty() { "void".to_owned() } else { a.join(", ") };
                format!("{} (*{decl})({a})", self.cname(ret, "", depth + 1)?)
            }
        })
    }

    fn cname_name(&mut self, n: &str, decl: &str, sp: &str, depth: u32) -> Result<String, Fatal> {
        if self.bits.contains(n) {
            return Ok(format!("{}{sp}", self.bits_storage(n)?));
        }
        if let Some(t) = self.as_types.get(n) {
            return Ok(format!("{t}{sp}"));
        }
        if self.sel.contains(n) && self.declared.contains(n) {
            return Ok(format!("{n}{sp}"));
        }
        if self.m.aggs.contains_key(n) {
            let both: Vec<String> = self.sel.intersection(&self.declared).cloned().collect();
            for s in both {
                if self.m.agg_of(&s).as_deref() == Some(n) {
                    return Ok(format!("{s}{sp}"));
                }
            }
            let d = if decl.is_empty() { "?" } else { decl };
            return Ok(format!("{}{sp}", self.err(format!("{d}: no selected typedef for {n}"))));
        }
        let Some(e) = self.m.assign.get(n) else {
            let path = self.m.path.clone();
            return Ok(format!("{}{sp}", self.err(format!("type {n} not in {path}"))));
        };
        if matches!(e, Expr::Name(id) if id.starts_with("enum_")) {
            return Ok(format!("int{sp}"));
        }
        let t = self.m.typ(e)?;
        self.cname(&t, decl, depth + 1)
    }

    /// One alias step at a time until a selected name or a non-name.
    fn peel(&self, t: &T) -> Result<T, Fatal> {
        let mut t = t.clone();
        for _ in 0..=DEPTH {
            let T::Name(a) = &t else { return Ok(t) };
            if self.sel.contains(a) || self.m.aggs.contains_key(a) {
                return Ok(t);
            }
            match self.m.assign.get(a) {
                None => return Ok(t),
                Some(Expr::Name(id)) if id.starts_with("enum_") => return Ok(t),
                Some(e) => t = self.m.typ(e)?,
            }
        }
        Err(deep())
    }

    fn is_fn(&self, n: &str) -> Result<bool, Fatal> {
        match self.m.assign.get(n) {
            None => Ok(false),
            Some(e) => Ok(matches!(self.m.resolve(&self.m.typ(e)?)?, T::Fn(..))),
        }
    }

    // ------------------------------------------------------------ declarations

    fn fields(&mut self, agg: &str) -> Result<Vec<String>, Fatal> {
        let Some(fl) = self.m.fields.get(agg) else {
            return Ok(vec![self.err(format!("{agg} has no field list"))]);
        };
        let dirty: Vec<usize> = (0..fl.len())
            .filter(|&i| fl.get(i).is_some_and(|f| f.name.starts_with("_dirty_")))
            .collect();
        let mut keep = fl.len();
        if !dirty.is_empty() {
            let trailing = dirty.iter().enumerate().all(|(k, &i)| i == fl.len() - dirty.len() + k);
            let ones = dirty.iter().all(|&i| fl.get(i).is_some_and(|f| f.bits == Some(1)));
            if !trailing || !ones || dirty.len() > 8 {
                return Ok(vec![
                    self.err(format!("{agg}: dirty bits not a trailing run of <= 8 one-bit fields")),
                ]);
            }
            keep -= dirty.len();
        }
        let mut out = Vec::new();
        for f in fl.iter().take(keep) {
            if f.bits.is_some_and(|b| b != 0) {
                return Ok(vec![self.err(format!("{agg}.{}: bitfield", f.name))]);
            }
            let t = self.m.typ(&f.ty)?;
            out.push(format!("{};", self.cname(&t, &f.name, 0)?));
        }
        if !dirty.is_empty() {
            out.push("uint8_t dirty;".to_owned());
        }
        Ok(out)
    }

    fn agg(&mut self, name: &str, kind: AggKind) -> Agg {
        let Some(a) = self.m.agg_of(name) else {
            let path = self.m.path.clone();
            return Agg::Err(vec![self.err(format!("{name}: not a struct/union typedef in {path}"))]);
        };
        match self.m.aggs.get(&a) {
            Some(k) if *k == kind => Agg::Ok(a),
            k => {
                let is = k.map_or("?", |k| k.as_str());
                Agg::Err(vec![self.err(format!("{name} is a {is}, not a {}", kind.as_str()))])
            }
        }
    }

    fn d_struct(&mut self, names: &[&str]) -> Result<Vec<String>, Fatal> {
        let mut out = Vec::new();
        for n in names {
            match self.agg(n, AggKind::Structure) {
                Agg::Err(e) => out.extend(e),
                Agg::Ok(a) => {
                    let body = self.fields(&a)?;
                    out.push(wrap("typedef struct {", &body, &format!("}} {n};")));
                }
            }
        }
        Ok(out)
    }

    fn d_prefix(&mut self, args: &[&str]) -> Result<Vec<String>, Fatal> {
        let (n, keep) = args
            .split_first()
            .ok_or_else(|| Fatal::Index("@prefix without a name: list index out of range".into()))?;
        let a = match self.agg(n, AggKind::Structure) {
            Agg::Err(e) => return Ok(e),
            Agg::Ok(a) => a,
        };
        let fl = self.m.fields.get(&a).cloned().unwrap_or_default();
        let names: Vec<&str> = fl
            .iter()
            .map(|f| f.name.as_str())
            .filter(|f| !f.starts_with("_dirty_"))
            .collect();
        let leading = names.get(..keep.len()).is_some_and(|l| l == keep);
        if !leading
            || fl
                .iter()
                .any(|f| keep.contains(&f.name.as_str()) && f.bits.is_some_and(|b| b != 0))
        {
            let (k, all) = (keep.join(" "), names.join(" "));
            return Ok(vec![
                self.err(format!("@prefix {n}: {k} are not its leading fields ({all})")),
            ]);
        }
        let mut body = Vec::new();
        for f in fl.iter().filter(|f| keep.contains(&f.name.as_str())) {
            let t = self.m.typ(&f.ty)?;
            body.push(format!("{};", self.cname(&t, &f.name, 0)?));
        }
        if names.len() < fl.len() {
            body.push("uint8_t dirty;".to_owned());
        }
        Ok(vec![wrap("typedef struct {", &body, &format!("}} {n};"))])
    }

    fn d_union(&mut self, args: &[&str]) -> Result<Vec<String>, Fatal> {
        let (n, members) = args
            .split_first()
            .ok_or_else(|| Fatal::Index("@union without a name: list index out of range".into()))?;
        let a = match self.agg(n, AggKind::Union) {
            Agg::Err(e) => return Ok(e),
            Agg::Ok(a) => a,
        };
        // {f.name: f}: first position, last value
        let mut order: Vec<String> = Vec::new();
        let mut by_name: BTreeMap<String, Expr> = BTreeMap::new();
        for f in self.m.fields.get(&a).cloned().unwrap_or_default() {
            if by_name.insert(f.name.clone(), f.ty).is_none() {
                order.push(f.name);
            }
        }
        let members: Vec<String> = if members.is_empty() {
            order.clone()
        } else {
            members.iter().map(|s| (*s).to_owned()).collect()
        };
        let mut body = Vec::new();
        for m in &members {
            let Some(ty) = by_name.get(m).cloned() else {
                body.push(self.err(format!("{n} has no member {m}")));
                continue;
            };
            let t = self.m.typ(&ty)?;
            body.push(format!("{};", self.cname(&t, m, 0)?));
        }
        let size = self.m.agg_layout(&a)?.size;
        if members.len() < order.len() {
            body.push(format!("uint8_t _size[{size}];"));
        }
        Ok(vec![wrap("typedef union {", &body, &format!("}} {n};"))])
    }

    fn d_alias(&mut self, args: &[&str]) -> Vec<String> {
        let [a, "=", b] = args else {
            return vec![self.err("@alias A = B".to_owned())];
        };
        let same = match (self.m.agg_of(a), self.m.agg_of(b)) {
            (Some(x), Some(y)) => {
                self.m.fields.get(&x).cloned().unwrap_or_default() == self.m.fields.get(&y).cloned().unwrap_or_default()
            }
            _ => false,
        };
        if !same {
            return vec![self.err(format!("@alias {a} = {b}: field lists differ"))];
        }
        vec![format!("typedef {b} {a};")]
    }

    fn d_typedef(&mut self, names: &[&str]) -> Result<Vec<String>, Fatal> {
        let mut out = Vec::new();
        for n in names {
            let Some(e) = self.m.assign.get(*n) else {
                let path = self.m.path.clone();
                out.push(self.err(format!("typedef {n} not in {path}")));
                continue;
            };
            let t = self.m.typ(e)?;
            if matches!(&t, T::Name(a) if a.starts_with("enum_")) {
                out.push(format!("typedef int {n};"));
                continue;
            }
            let removed = self.sel.remove(*n);
            let c = self.cname(&t, n, 0);
            if removed {
                self.sel.insert((*n).to_owned());
            }
            out.push(format!("typedef {};", c?));
        }
        Ok(out)
    }

    fn bits_storage(&mut self, n: &str) -> Result<String, Fatal> {
        let a = self.m.agg_of(n);
        let fl = a
            .as_ref()
            .and_then(|a| self.m.fields.get(a))
            .cloned()
            .unwrap_or_default();
        let mut kinds = BTreeSet::new();
        for f in fl.iter().filter(|f| f.bits.is_some_and(|b| b != 0)) {
            let t = self.m.typ(&f.ty)?;
            kinds.insert(self.cname(&t, "", 0)?);
        }
        if fl.is_empty() || fl.iter().any(|f| f.bits.is_none()) || kinds.len() != 1 {
            return Ok(self.err(format!("@bits {n}: not a struct of bitfields of one type")));
        }
        let st = kinds.pop_first().unwrap_or_default();
        let (Some(a), Some(first)) = (a, fl.first()) else {
            return Ok(st);
        };
        let whole = self.m.agg_layout(&a)?.size;
        let storage = self
            .m
            .ctype(&self.m.typ(&first.ty)?)?
            .ok_or_else(|| Fatal::Type("sizeof(None)".into()))?;
        if whole != storage.layout.size {
            return Ok(self.err(format!("@bits {n}: wider than its storage type {st}")));
        }
        Ok(st)
    }

    fn d_as(&mut self, args: &[&str]) -> Result<Vec<String>, Fatal> {
        let Some((t, names)) = args.split_first() else {
            return Err(Fatal::Index("@as without a type: list index out of range".into()));
        };
        if !matches!(*t, "uint32_t" | "int32_t" | "int" | "unsigned int") {
            return Ok(vec![self.err(format!("@as {t}: not a 4-byte integer type"))]);
        }
        for n in names {
            let e = self.m.assign.get(*n);
            let four = match e {
                Some(e @ Expr::Name(id)) if id.starts_with("enum_") => {
                    self.m
                        .ctype(&self.m.typ(e)?)?
                        .ok_or_else(|| Fatal::Type("sizeof(None)".into()))?
                        .layout
                        .size
                        == 4
                }
                _ => false,
            };
            if !four {
                self.err(format!("@as {t}: {n} is not a 4-byte enum type"));
                continue;
            }
            self.as_types.insert((*n).to_owned(), (*t).to_owned());
        }
        Ok(Vec::new())
    }

    fn d_fn(&mut self, names: &[&str]) -> Result<Vec<String>, Fatal> {
        let mut out = Vec::new();
        for n in names {
            let Some(args) = self.m.args.get(*n) else {
                let path = self.m.path.clone();
                out.push(self.err(format!("function {n} not in {path}")));
                continue;
            };
            let ret = match self.m.ret.get(*n) {
                Some(r) => {
                    let t = self.m.typ(r)?;
                    self.cname(&t, "", 0)?
                }
                None => "void".to_owned(),
            };
            let mut a = Vec::with_capacity(args.len());
            for x in args {
                let t = self.m.typ(x)?;
                a.push(self.cname(&t, "", 0)?);
            }
            out.push(format!("IFCS_FN({ret}, {n}, ({}))", a.join(", ")));
        }
        Ok(out)
    }

    // ------------------------------------------------------------ template

    /// One template line, filled in.
    pub(crate) fn line(&mut self, line: &str) -> Result<String, Fatal> {
        if let Some((kind, args, comment)) = directive(line) {
            let args = py::split(args);
            let mut out = match kind {
                "typedef" => self.d_typedef(&args)?,
                "struct" => self.d_struct(&args)?,
                "union" => self.d_union(&args)?,
                "alias" => self.d_alias(&args),
                "fn" => self.d_fn(&args)?,
                "bits" => {
                    self.bits.extend(args.iter().map(|s| (*s).to_owned()));
                    Vec::new()
                }
                "prefix" => self.d_prefix(&args)?,
                _ => self.d_as(&args)?,
            };
            match kind {
                "union" | "prefix" | "alias" => self.declared.extend(args.first().map(|s| (*s).to_owned())),
                "typedef" | "struct" => self.declared.extend(args.iter().map(|s| (*s).to_owned())),
                _ => {}
            }
            if let Some(c) = comment {
                self.in_comment = !c.contains("*/");
                match out.last_mut() {
                    Some(last) => {
                        last.push_str("   ");
                        last.push_str(c);
                    }
                    None => out.push(c.to_owned()),
                }
            }
            return Ok(out.join("\n"));
        }
        let (parts, still) = split_comments(line, self.in_comment);
        self.in_comment = still;
        let mut s = String::new();
        for (code, seg) in parts {
            if code {
                s.push_str(&self.fill(seg)?);
            } else {
                s.push_str(seg);
            }
        }
        Ok(s)
    }

    fn fill(&mut self, code: &str) -> Result<String, Fatal> {
        let code = self.fill_assign(code)?;
        let code = self.fill_define(&code)?;
        let code = self.fill_braced(&code)?;
        if code.contains('@') {
            self.err(format!("stray @ in: {}", py::strip(&code)));
        }
        Ok(code)
    }

    /// `\b([A-Za-z_]\w*)(\s*=\s*)@(?![{\w])` -> NAME = value.
    fn fill_assign(&mut self, code: &str) -> Result<String, Fatal> {
        let cs: Vec<char> = code.chars().collect();
        let word = |k: usize| cs.get(k).copied().is_some_and(py::is_word);
        let (mut out, mut i) = (String::new(), 0);
        while let Some(&c) = cs.get(i) {
            if (c.is_ascii_alphabetic() || c == '_') && (i == 0 || !word(i - 1)) {
                let mut j = i + 1;
                while word(j) {
                    j += 1;
                }
                let mut k = j;
                while cs.get(k).copied().is_some_and(py::is_space) {
                    k += 1;
                }
                if cs.get(k) == Some(&'=') {
                    k += 1;
                    while cs.get(k).copied().is_some_and(py::is_space) {
                        k += 1;
                    }
                    if cs.get(k) == Some(&'@') && cs.get(k + 1) != Some(&'{') && !word(k + 1) {
                        let name: String = cs.get(i..j).unwrap_or_default().iter().collect();
                        let eq: String = cs.get(j..k).unwrap_or_default().iter().collect();
                        let v = self.value(&name)?;
                        let _ = write!(out, "{name}{eq}{v}");
                        i = k + 1;
                        continue;
                    }
                }
            }
            out.push(c);
            i += 1;
        }
        Ok(out)
    }

    /// `(#define\s+(\w+)\s+)@(?![{\w])` -> #define NAME value.
    fn fill_define(&mut self, code: &str) -> Result<String, Fatal> {
        let cs: Vec<char> = code.chars().collect();
        let word = |k: usize| cs.get(k).copied().is_some_and(py::is_word);
        let space = |k: usize| cs.get(k).copied().is_some_and(py::is_space);
        let (mut out, mut i) = (String::new(), 0);
        while let Some(&c) = cs.get(i) {
            if "#define".chars().enumerate().all(|(k, d)| cs.get(i + k) == Some(&d)) {
                let mut j = i + 7;
                let s1 = j;
                while space(j) {
                    j += 1;
                }
                let n0 = j;
                while word(j) {
                    j += 1;
                }
                let n1 = j;
                while space(j) {
                    j += 1;
                }
                if n0 > s1
                    && n1 > n0
                    && j > n1
                    && cs.get(j) == Some(&'@')
                    && cs.get(j + 1) != Some(&'{')
                    && !word(j + 1)
                {
                    let head: String = cs.get(i..j).unwrap_or_default().iter().collect();
                    let name: String = cs.get(n0..n1).unwrap_or_default().iter().collect();
                    let v = self.value(&name)?;
                    let _ = write!(out, "{head}{v}");
                    i = j + 1;
                    continue;
                }
            }
            out.push(c);
            i += 1;
        }
        Ok(out)
    }

    /// `@\{(\w+)(?::([^}]*))?\}` -> format(value, spec or "d").
    fn fill_braced(&mut self, code: &str) -> Result<String, Fatal> {
        let cs: Vec<char> = code.chars().collect();
        let (mut out, mut i) = (String::new(), 0);
        while let Some(&c) = cs.get(i) {
            if c == '@' && cs.get(i + 1) == Some(&'{') {
                let mut j = i + 2;
                while cs.get(j).copied().is_some_and(py::is_word) {
                    j += 1;
                }
                if j > i + 2 {
                    let name: String = cs.get(i + 2..j).unwrap_or_default().iter().collect();
                    let found = match cs.get(j) {
                        Some('}') => Some((String::new(), j)),
                        Some(':') => cs
                            .iter()
                            .skip(j + 1)
                            .position(|&x| x == '}')
                            .map(|p| (cs.get(j + 1..j + 1 + p).unwrap_or_default().iter().collect(), j + 1 + p)),
                        _ => None,
                    };
                    if let Some((spec, end)) = found {
                        let v = self.value(&name)?;
                        let spec = if spec.is_empty() { "d" } else { spec.as_str() };
                        out.push_str(&py::format_int(v, spec)?);
                        i = end + 1;
                        continue;
                    }
                }
            }
            out.push(c);
            i += 1;
        }
        Ok(out)
    }
}

/// One line if it fits in 120 characters, else the body indented on lines of its own.
fn wrap(head: &str, body: &[String], tail: &str) -> String {
    let mut parts = vec![head.to_owned()];
    parts.extend(body.iter().cloned());
    parts.push(tail.to_owned());
    let one = parts.join(" ");
    if one.chars().count() <= 120 {
        return one;
    }
    let mut lines = vec![head.to_owned()];
    lines.extend(body.iter().map(|b| format!("    {b}")));
    lines.push(tail.to_owned());
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn directive_regex() {
        assert_eq!(directive("@fn a b"), Some(("fn", "a b", None)));
        assert_eq!(
            directive("@struct x   /* c */ y /* d */"),
            Some(("struct", "x", Some("/* c */ y /* d */")))
        );
        assert_eq!(directive("@fn /* c */"), Some(("fn", "", Some("/* c */"))));
        assert_eq!(directive("@as uint32_t a  "), Some(("as", "uint32_t a", None)));
        assert_eq!(directive("@asx y"), None);
        assert_eq!(directive("@fn"), None);
        assert_eq!(directive(" @fn a"), None);
    }

    #[test]
    fn stars() {
        assert_eq!(star_fix("int *x"), "int* x");
        assert_eq!(star_fix("int **x"), "int** x");
        assert_eq!(star_fix("int *"), "int* ");
        assert_eq!(star_fix("int *f[3]"), "int* f[3]");
        assert_eq!(star_fix("void (**)(int)"), "void (**)(int)");
    }

    #[test]
    fn comments_carry_over() {
        assert_eq!(
            split_comments("a /* b */ c", false),
            (vec![(true, "a "), (false, "/* b */"), (true, " c")], false)
        );
        assert_eq!(
            split_comments("a /* b", false),
            (vec![(true, "a "), (false, "/* b")], true)
        );
        assert_eq!(
            split_comments("b */ c", true),
            (vec![(false, "b */"), (true, " c")], false)
        );
    }

    #[test]
    fn wrapping() {
        assert_eq!(wrap("{", &["a;".into()], "}"), "{ a; }");
        let long = vec!["x".repeat(130)];
        assert_eq!(wrap("{", &long, "}"), format!("{{\n    {}\n}}", "x".repeat(130)));
    }
}
