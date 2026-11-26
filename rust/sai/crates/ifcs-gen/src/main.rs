//! `ifcs-gen`: the generator as a program, with `gen_ifcs_api.py`'s arguments:
//!
//! ```text
//! ifcs-gen --ctypes <ifcs_ctypes.py> [--overrides ifcs_overrides.txt] [--source "<what>"]
//!          [--keep-going] -o OUTDIR TEMPLATE.h.in...
//! ifcs-gen --dump-model <ifcs_ctypes.py>     the model's facts (the live differential)
//! ```
//!
//! Errors: `ifcs-gen:` and one indented line per error on stderr, exit status 1 (with
//! `--keep-going` after writing the outputs).
#![forbid(unsafe_code)]

use std::fs;
use std::path::Path;
use std::process::ExitCode;

#[derive(Default)]
struct Args {
    ctypes: Option<String>,
    overrides: Option<String>,
    source: Option<String>,
    keep_going: bool,
    outdir: Option<String>,
    dump: Option<String>,
    templates: Vec<String>,
}

fn parse_args() -> Result<Args, String> {
    let mut a = Args::default();
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        let (flag, inline) = match arg.split_once('=') {
            Some((f, v)) if f.starts_with("--") => (f.to_owned(), Some(v.to_owned())),
            _ => (arg.clone(), None),
        };
        let mut value = |name: &str| {
            inline
                .clone()
                .or_else(|| it.next())
                .ok_or(format!("{name} needs a value"))
        };
        match flag.as_str() {
            "--ctypes" => a.ctypes = Some(value("--ctypes")?),
            "--overrides" => a.overrides = Some(value("--overrides")?),
            "--source" => a.source = Some(value("--source")?),
            "-o" | "--outdir" => a.outdir = Some(value("--outdir")?),
            "--dump-model" => a.dump = Some(value("--dump-model")?),
            "--keep-going" => a.keep_going = true,
            f if f.starts_with("-o") && f.len() > 2 => a.outdir = f.get(2..).map(str::to_owned),
            f if f.starts_with('-') && f.len() > 1 => return Err(format!("unknown option {f}")),
            _ => a.templates.push(arg),
        }
    }
    Ok(a)
}

fn read(path: &str) -> Result<Vec<u8>, String> {
    fs::read(path).map_err(|e| format!("{path}: {e}"))
}

fn read_text(path: &str) -> Result<String, String> {
    String::from_utf8(read(path)?).map_err(|e| format!("{path}: {e}"))
}

fn run() -> Result<bool, String> {
    let a = parse_args()?;
    if let Some(p) = &a.dump {
        let text = ifcs_gen::dump_model(p, &read(p)?).map_err(|e| e.to_string())?;
        print!("{text}");
        return Ok(true);
    }
    let ctypes_path = a.ctypes.ok_or("--ctypes is required")?;
    let outdir = a.outdir.ok_or("-o/--outdir is required")?;
    if a.templates.is_empty() {
        return Err("at least one template is required".into());
    }
    let ctypes = read(&ctypes_path)?;
    let mut tmpls = Vec::new();
    for p in &a.templates {
        tmpls.push((p.as_str(), read_text(p)?));
    }
    let ov_text = a.overrides.as_deref().map(read_text).transpose()?;
    let templates: Vec<(&str, &str)> = tmpls.iter().map(|(p, t)| (*p, t.as_str())).collect();
    let input = ifcs_gen::Input {
        ctypes_path: &ctypes_path,
        ctypes: &ctypes,
        overrides: a.overrides.as_deref().zip(ov_text.as_deref()),
        source: a.source.as_deref(),
        templates: &templates,
    };
    let g = ifcs_gen::generate(&input).map_err(|e| e.to_string())?;
    if !g.errors.is_empty() && !a.keep_going {
        eprintln!("ifcs-gen:\n  {}", g.errors.join("\n  "));
        return Ok(false);
    }
    fs::create_dir_all(&outdir).map_err(|e| format!("{outdir}: {e}"))?;
    for (name, text) in &g.outputs {
        let path = Path::new(&outdir).join(name);
        let tmp = Path::new(&outdir).join(format!("{name}.tmp"));
        fs::write(&tmp, text).map_err(|e| format!("{}: {e}", tmp.display()))?;
        fs::rename(&tmp, &path).map_err(|e| format!("{}: {e}", path.display()))?;
    }
    if !g.errors.is_empty() {
        eprintln!("ifcs-gen (outputs written, --keep-going):\n  {}", g.errors.join("\n  "));
        return Ok(false);
    }
    Ok(true)
}

fn main() -> ExitCode {
    match run() {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(e) => {
            eprintln!("ifcs-gen: {e}");
            ExitCode::FAILURE
        }
    }
}
