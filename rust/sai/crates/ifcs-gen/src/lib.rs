//! The IFCS header generator in Rust: fills our templates of the IFCS declarations
//! (`templates/ifcs_api.h.in`, `templates/ifcs_api_qos.h.in`: names only, ours) with the values,
//! field lists and signatures of a ctypesgen file (the public ishell 2.0.1 `ifcs_ctypes.py`), read
//! as **data**, never executed, plus `templates/ifcs_overrides.txt` (the values where the target's
//! libifcs 0.12.41 differs). A port of `ai_re/nix/packages/sai-teralynx/tools/gen_ifcs_api.py` and
//! `ifcs_ctypes_model.py`: on the same input it writes the same bytes (`nix/ifcs-headers.nix`
//! compares the two live while the Python exists, and keeps a sha256 golden of the output).
//!
//! The output is vendor-derived: it exists only in a build (`ifcs-sys`'s build script, or the
//! `ifcs-gen` program of feature `cli` for the C fixtures), never in git.
//!
//! Where it does not do what the Python does (inputs no ctypes file of ours has): integers outside
//! `i128` and the rarer `format` specs are refused ([`Fatal::Limit`]); an alias loop that
//! the Python follows forever (`peel`) is an error; a few Python expressions are read as
//! unparsable (module [`expr`]'s list).
#![forbid(unsafe_code)]

mod expr;
mod fill;
mod layout;
mod model;
mod py;
mod sha256;

use std::collections::BTreeMap;
use std::fmt;

pub use layout::Layout;
pub use model::Model;

/// Our templates and overrides (copies of sai-teralynx's `src/` until that package goes, H5:
/// `nix/ifcs-headers.nix` checks they are equal).
pub mod templates {
    /// `ifcs_api.h.in`.
    pub const IFCS_API_H_IN: &str = include_str!("../templates/ifcs_api.h.in");
    /// `ifcs_api_qos.h.in` (the `QoS` / mirror areas).
    pub const IFCS_API_QOS_H_IN: &str = include_str!("../templates/ifcs_api_qos.h.in");
    /// The overrides: `NAME VALUE  # where it was verified`.
    pub const IFCS_OVERRIDES_TXT: &str = include_str!("../templates/ifcs_overrides.txt");
    /// The templates by file name, in the order the headers are generated.
    pub const TEMPLATES: [(&str, &str); 2] = [
        ("ifcs_api.h.in", IFCS_API_H_IN),
        ("ifcs_api_qos.h.in", IFCS_API_QOS_H_IN),
    ];
    /// The overrides file by name.
    pub const OVERRIDES: (&str, &str) = ("ifcs_overrides.txt", IFCS_OVERRIDES_TXT);
}

/// What stops a generation: an exception (or `sys.exit`) of the reference generator.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Fatal {
    /// `KeyError`: an undefined type, an aggregate without a field list.
    Key(String),
    /// `ValueError`: an unsupported type expression, an alias loop, a bad bit width or spec.
    Value(String),
    /// `IndexError`: a directive or call without its first argument.
    Index(String),
    /// `TypeError`: a ctypes type that cannot be built.
    Type(String),
    /// `ZeroDivisionError`.
    ZeroDivision(String),
    /// `RecursionError` (or the Python's endless loop).
    Recursion(String),
    /// `SyntaxError` of a field / argument type the model does not skip.
    Syntax(String),
    /// `sys.exit(message)` of the reference (the overrides file).
    Exit(String),
    /// A value or form outside what this port computes (see the crate documentation).
    Limit(String),
}

impl fmt::Display for Fatal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (kind, msg) = match self {
            Fatal::Key(m) => ("KeyError", m),
            Fatal::Value(m) => ("ValueError", m),
            Fatal::Index(m) => ("IndexError", m),
            Fatal::Type(m) => ("TypeError", m),
            Fatal::ZeroDivision(m) => ("ZeroDivisionError", m),
            Fatal::Recursion(m) => ("RecursionError", m),
            Fatal::Syntax(m) => ("SyntaxError", m),
            Fatal::Exit(m) => return f.write_str(m),
            Fatal::Limit(m) => ("not supported", m),
        };
        write!(f, "{kind}: {msg}")
    }
}

impl std::error::Error for Fatal {}

/// One generation's inputs (file contents; paths as the Python is given them: they appear in
/// the provenance line, by base name, and in error messages).
pub struct Input<'a> {
    /// The ctypes file's path.
    pub ctypes_path: &'a str,
    /// Its bytes (read as UTF-8, invalid sequences replaced).
    pub ctypes: &'a [u8],
    /// The overrides file (path, text), if any.
    pub overrides: Option<(&'a str, &'a str)>,
    /// `--source`: what the ctypes file is (appended to the provenance line).
    pub source: Option<&'a str>,
    /// The templates (path, text), in order.
    pub templates: &'a [(&'a str, &'a str)],
}

/// The outputs and the collected errors of a generation.
#[derive(Debug)]
pub struct Generated {
    /// (file name: the template's base name without `.in`, text), one per template.
    pub outputs: Vec<(String, String)>,
    /// The errors, in the Python's words and order (empty: the outputs are good).
    pub errors: Vec<String>,
}

/// `read_overrides`: `NAME VALUE` lines, each with a comment saying where it was verified.
fn read_overrides(path: &str, text: &str) -> Result<BTreeMap<String, i128>, Fatal> {
    let mut ov = BTreeMap::new();
    let text = py::universal_newlines(text);
    for line in text.split_inclusive('\n') {
        let code = py::strip(line.split('#').next().unwrap_or_default());
        if code.is_empty() {
            continue;
        }
        let words = py::split(code);
        let [name, val] = words.as_slice() else {
            return Err(Fatal::Value(format!("{path}: not a NAME VALUE line: {code}")));
        };
        if !line.contains('#') {
            return Err(Fatal::Exit(format!(
                "{path}: override {name} has no comment saying where it was verified"
            )));
        }
        let v = py::int_base0(val)
            .ok_or_else(|| Fatal::Value(format!("invalid literal for int() with base 0: '{val}'")))?;
        ov.insert((*name).to_owned(), v);
    }
    Ok(ov)
}

/// Runs the generator (`gen_ifcs_api.py`'s `main` without the file writes).
pub fn generate(input: &Input<'_>) -> Result<Generated, Fatal> {
    let tmpls: Vec<(String, String)> = input
        .templates
        .iter()
        .map(|(p, t)| ((*p).to_owned(), py::universal_newlines(t)))
        .collect();
    let model = Model::parse(input.ctypes_path, input.ctypes)?;
    let ov = match input.overrides {
        Some((p, t)) => read_overrides(p, t)?,
        None => BTreeMap::new(),
    };
    let texts: Vec<String> = tmpls.iter().map(|(_, t)| t.clone()).collect();
    let mut g = fill::Gen::new(&model, ov, fill::selected_names(&texts));
    let sha = sha256::hex(input.ctypes);
    let source = input
        .source
        .filter(|s| !s.is_empty())
        .map(|s| format!(", {s}"))
        .unwrap_or_default();
    let mut outputs = Vec::new();
    for (p, t) in &tmpls {
        let base = py::basename(p);
        let name = base.strip_suffix(".in").unwrap_or(base);
        let mut text = format!(
            "/* GENERATED by tools/gen_ifcs_api.py from {base} + {} (sha256 {sha}){source}.\n \
             * Do not edit or commit: it holds IFCS facts from vendor material. */\n",
            py::basename(input.ctypes_path)
        );
        let mut lines = Vec::new();
        for l in t.split('\n') {
            lines.push(g.line(l)?);
        }
        text.push_str(&lines.join("\n"));
        outputs.push((name.to_owned(), text));
    }
    let unused: Vec<String> = g
        .overrides()
        .keys()
        .filter(|k| !g.used_ov.contains(*k))
        .cloned()
        .collect();
    if !unused.is_empty() {
        g.errors.push(format!("overrides not used: {}", unused.join(" ")));
    }
    Ok(Generated {
        outputs,
        errors: g.errors,
    })
}

/// The model's facts in a line format the Python model can print too (the live differential of
/// `nix/ifcs-headers.nix`): `v NAME VALUE|None|ERR` per assigned name, `a AGG KIND SIZE ALIGN|ERR`
/// per aggregate, `f FN NARGS True|False` (has a `restype`) per function, each group sorted.
pub fn dump_model(path: &str, bytes: &[u8]) -> Result<String, Fatal> {
    use std::fmt::Write as _;
    let m = Model::parse(path, bytes)?;
    let mut out = String::new();
    for n in m.assign.keys() {
        let v = match m.value(n, 0) {
            Ok(Some(v)) => v.to_string(),
            Ok(None) => "None".to_owned(),
            Err(_) => "ERR".to_owned(),
        };
        let _ = writeln!(out, "v {n} {v}");
    }
    for (a, k) in &m.aggs {
        let l = m
            .agg_layout(a)
            .map_or_else(|_| "ERR".to_owned(), |l| format!("{} {}", l.size, l.align));
        let _ = writeln!(out, "a {a} {} {l}", k.as_str());
    }
    for (f, args) in &m.args {
        let ret = if m.ret.contains_key(f) { "True" } else { "False" };
        let _ = writeln!(out, "f {f} {} {ret}", args.len());
    }
    Ok(out)
}
