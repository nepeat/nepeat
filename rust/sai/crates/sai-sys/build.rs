//! Generates the bindings and checks them against the C compiler. See src/lib.rs.
// A build script reports a failure by panicking with a message.
#![allow(clippy::expect_used, clippy::panic, clippy::too_many_lines)]

use std::env;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    for v in ["SAI_INC", "SAI_SYS_ABI_CC", "SAI_SYS_ABI_CHECK", "TARGET_CC", "CC"] {
        println!("cargo:rerun-if-env-changed={v}");
    }
    let inc = PathBuf::from(env::var_os("SAI_INC").unwrap_or_else(|| {
        panic!("sai-sys: set SAI_INC to the directory with sai.h (SAI/inc of the sonic-sairedis revision in use)")
    }));
    assert!(
        inc.join("sai.h").is_file(),
        "sai-sys: no sai.h in SAI_INC={}",
        inc.display()
    );
    println!("cargo:rerun-if-changed={}", inc.display());
    // saiobject.h includes the extension headers, which upstream keeps next to inc/.
    let experimental = inc.parent().map(|p| p.join("experimental")).filter(|p| p.is_dir());
    let mut includes = vec![inc.clone()];
    includes.extend(experimental);
    let out = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR"));

    // Status codes are "#define SAI_STATUS_X SAI_STATUS_CODE(0x..L)": a function-like macro,
    // which bindgen does not evaluate. An enum initialised from them makes the C front end do it.
    let status_h = fs::read_to_string(inc.join("saistatus.h")).expect("saistatus.h");
    let statuses: Vec<&str> = status_h
        .lines()
        .filter_map(|l| l.strip_prefix("#define SAI_STATUS_"))
        .filter_map(|l| l.split_whitespace().next())
        .filter(|n| !n.contains('('))
        .collect();
    assert!(
        statuses.len() > 30,
        "sai-sys: saistatus.h has changed shape ({} status macros found)",
        statuses.len()
    );
    let mut wrapper = String::from("#include <sai.h>\nenum sai_rs_status_value {\n");
    for s in &statuses {
        writeln!(wrapper, "    SAI_RS_STATUS_{s} = SAI_STATUS_{s},").expect("write");
    }
    wrapper.push_str("};\n");
    let wrapper_h = out.join("sai_rs_wrapper.h");
    fs::write(&wrapper_h, &wrapper).expect("write wrapper");

    // Big-endian ppc64 with the ELFv2 ABI is "powerpc64-unknown-linux-gnuelfv2" to rustc and
    // "...-gnuabielfv2" to nixpkgs; clang wants the plain triple and the ABI as a flag (without
    // it, it assumes ELFv1, whose glibc headers an ELFv2 sysroot does not have). In a nix build
    // bindgenHook's BINDGEN_EXTRA_CLANG_ARGS comes last and decides (see nix/package.nix).
    let target = env::var("TARGET").expect("TARGET");
    let clang_target: Vec<String> = match target
        .strip_suffix("gnuelfv2")
        .or_else(|| target.strip_suffix("gnuabielfv2"))
    {
        Some(stem) => vec![format!("--target={stem}gnu"), "-mabi=elfv2".into()],
        None => Vec::new(),
    };

    let bindings = bindgen::Builder::default()
        .header(wrapper_h.to_str().expect("utf-8 OUT_DIR"))
        .clang_args(clang_target)
        .clang_args(includes.iter().map(|i| format!("-I{}", i.display())))
        .allowlist_item("_?[sS][aA][iI]_.*")
        .default_enum_style(bindgen::EnumVariation::Consts)
        .prepend_enum_name(false)
        .layout_tests(true)
        .blocklist_item("SAI_STATUS_.*") // the literal-valued status macros; the enum above has them all
        .derive_default(false) // its impls for unions are unsafe { zeroed() }: keep this crate free of unsafe blocks
        .derive_debug(false)
        .generate_comments(false)
        .use_core()
        .formatter(bindgen::Formatter::Prettyplease)
        .generate()
        .expect("bindgen over sai.h");
    // SAI_RS_STATUS_X -> SAI_STATUS_X, typed as sai_status_t.
    let text = bindings
        .to_string()
        .replace("SAI_RS_STATUS_", "SAI_STATUS_")
        .replace(": sai_rs_status_value =", ": sai_status_t =");
    fs::write(out.join("bindings.rs"), &text).expect("write bindings");

    let (c, n) = abi_check_source(&text, &statuses);
    let c_path = out.join("abi_check.c");
    fs::write(&c_path, c).expect("write abi_check.c");
    let result = run_abi_check(&c_path, &includes, n);
    println!("cargo:rustc-env=SAI_SYS_ABI_CHECK_RESULT={result}");
    // In the build log on purpose: which compiler confirmed the bindings, for which target.
    println!("cargo:warning=SAI ABI check: {result}");
    println!("cargo:include={}", inc.display());
}

/// bindgen's layout assertions and enumerator values (clang's view of the target) as C
/// static assertions, for the target's own C compiler. Returns the source and the number of
/// assertions.
fn abi_check_source(bindings: &str, statuses: &[&str]) -> (String, usize) {
    // Whitespace-free copy: prettyplease wraps long assertions over several lines.
    let flat: String = bindings.chars().filter(|c| !c.is_whitespace()).collect();
    let c_type = |t: &str| -> Option<String> {
        if t.contains("__bindgen") {
            return None; // anonymous member type: no C spelling
        }
        if flat.contains(&format!("pubstruct{t}{{")) {
            Some(format!("struct {t}"))
        } else if flat.contains(&format!("pubunion{t}{{")) {
            Some(format!("union {t}"))
        } else {
            None
        }
    };
    let mut c = String::from("#include <stddef.h>\n#include \"sai_rs_wrapper.h\"\n");
    let mut n = 0usize;
    let mut check = |c: &mut String, expr: String, value: &str| {
        writeln!(
            c,
            "_Static_assert(({expr}) == {value}, \"{}\");",
            expr.replace('"', "'")
        )
        .expect("write");
        n += 1;
    };
    for (marker, cfn) in [("[\"Sizeof", "sizeof"), ("[\"Alignmentof", "_Alignof")] {
        for part in flat.split(marker).skip(1) {
            let Some((ty, rest)) = part.split_once('"') else {
                continue;
            };
            let Some(value) = rest
                .split_once(">()-")
                .and_then(|(_, r)| r.split_once("usize"))
                .map(|(v, _)| v)
            else {
                continue;
            };
            if let Some(ct) = c_type(ty) {
                check(&mut c, format!("{cfn}({ct})"), value);
            }
        }
    }
    for part in flat.split("[\"Offsetoffield:").skip(1) {
        let Some((path, rest)) = part.split_once('"') else {
            continue;
        };
        let Some((ty, field)) = path.split_once("::") else {
            continue;
        };
        let Some(value) = rest
            .split_once(")-")
            .and_then(|(_, r)| r.split_once("usize"))
            .map(|(v, _)| v)
        else {
            continue;
        };
        if let Some(ct) = c_type(ty) {
            check(&mut c, format!("offsetof({ct}, {})", c_field(field)), value);
        }
    }
    // Enumerators: "pub const SAI_X: _sai_y_t = N;" (the type is an enum's).
    for part in flat.split("pubconstSAI_").skip(1) {
        let Some((name, rest)) = part.split_once(':') else {
            continue;
        };
        let Some((ty, rest)) = rest.split_once('=') else {
            continue;
        };
        let Some((value, _)) = rest.split_once(';') else {
            continue;
        };
        let is_status = ty == "sai_status_t" && name.strip_prefix("STATUS_").is_some_and(|s| statuses.contains(&s));
        if !(ty.starts_with("_sai_") || is_status)
            || !value.trim_start_matches('-').chars().all(|ch| ch.is_ascii_digit())
        {
            continue;
        }
        check(&mut c, format!("(long long)SAI_{name}"), &format!("{value}LL"));
    }
    (c, n)
}

/// The C name of a field: bindgen appends "_" to names that are Rust keywords or primitive types.
fn c_field(rust: &str) -> &str {
    const RENAMED: &[&str] = &[
        "abstract", "as", "async", "await", "become", "bool", "box", "break", "const", "continue", "crate", "do",
        "dyn", "else", "enum", "extern", "f32", "f64", "false", "final", "fn", "for", "gen", "i128", "i16", "i32",
        "i64", "i8", "if", "impl", "in", "isize", "let", "loop", "macro", "match", "mod", "move", "mut", "override",
        "priv", "pub", "ref", "return", "self", "static", "str", "struct", "super", "trait", "true", "try", "type",
        "typeof", "u128", "u16", "u32", "u64", "u8", "unsafe", "unsized", "use", "usize", "virtual", "where", "while",
        "yield",
    ];
    let rust = rust.trim_start_matches("r#");
    match rust.strip_suffix('_') {
        Some(stem) if RENAMED.contains(&stem) => stem,
        _ => rust,
    }
}

fn run_abi_check(c_path: &Path, includes: &[PathBuf], n: usize) -> String {
    let required = env::var("SAI_SYS_ABI_CHECK").is_ok_and(|v| v == "require");
    assert!(
        n > 1000,
        "sai-sys: only {n} ABI assertions were derived from the bindings: the extraction in build.rs is stale"
    );
    let target = env::var("TARGET").expect("TARGET");
    let host = env::var("HOST").expect("HOST");
    let under = target.replace('-', "_");
    let cc = [
        "SAI_SYS_ABI_CC".to_string(),
        format!("CC_{under}"),
        format!("CC_{}", under.to_uppercase()),
        "TARGET_CC".to_string(),
    ]
    .iter()
    .find_map(|v| {
        println!("cargo:rerun-if-env-changed={v}");
        env::var(v).ok().filter(|s| !s.is_empty())
    })
    .or_else(|| {
        (host == target).then(|| {
            env::var("CC")
                .ok()
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| "cc".into())
        })
    });
    let Some(cc) = cc else {
        assert!(
            !required,
            "sai-sys: SAI_SYS_ABI_CHECK=require, but no C compiler for {target} (set SAI_SYS_ABI_CC)"
        );
        return format!("skipped: no C compiler for {target} (set SAI_SYS_ABI_CC or CC_{under})");
    };
    let mut words = cc.split_whitespace();
    let run = Command::new(words.next().expect("compiler"))
        .args(words)
        .args(["-std=gnu11", "-fsyntax-only"])
        .args(includes.iter().map(|i| format!("-I{}", i.display())))
        .arg("-I")
        .arg(c_path.parent().expect("OUT_DIR"))
        .arg(c_path)
        .output();
    match run {
        Ok(o) if o.status.success() => format!("checked {n} assertions with {cc} for {target}"),
        Ok(o) => panic!(
            "sai-sys: the Rust bindings disagree with {cc} about the SAI ABI for {target}:\n{}",
            String::from_utf8_lossy(&o.stderr)
        ),
        Err(e) => {
            assert!(
                !required,
                "sai-sys: SAI_SYS_ABI_CHECK=require, but {cc} did not run: {e}"
            );
            format!("skipped: {cc} did not run ({e})")
        }
    }
}
