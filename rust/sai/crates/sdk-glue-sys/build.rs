//! Generates the declarations of `sdk_glue.h` with bindgen, checks them against the target's C
//! compiler, applies the rule for `safe fn` declarations (src/lib.rs), and links the real or the
//! fake glue when a feature asks. See src/lib.rs.
// A build script reports a failure by panicking with a message.
#![allow(clippy::expect_used, clippy::panic, clippy::too_many_lines)]

use std::collections::BTreeMap;
use std::env;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The functions declared `safe fn` (src/lib.rs, "Safe declarations"): every argument and the
/// result are scalars (checked below against bindgen's signature), and `sdk_glue.c` refuses every
/// out-of-range value itself before any SDK call: its body starts with an `OB_CHECK_*` macro
/// (checked below when `sdk_glue.c` is found). The text says what the C checks.
const SAFE: &[(&str, &str)] = &[
    ("ob_sdk_th_overtemp_shutdown", "unit attached; refused after init all"),
    (
        "ob_sdk_mdio_c45_write",
        "a bring-up with misc init on this unit; phy <= 0x7ff; dev 1..31; reg, val <= 0xffff",
    ),
    ("ob_sdk_cpu_port", "unit attached"),
    ("ob_sdk_port_enable_set", "init all on this unit; port valid"),
    ("ob_sdk_port_speed_set", "init all on this unit; port valid; mbps >= 0"),
    (
        "ob_sdk_port_frame_max_set",
        "init all on this unit; port valid; bytes > 0",
    ),
    ("ob_sdk_port_fec_set", "init all on this unit; port valid; mode 0..2"),
    ("ob_sdk_port_autoneg_set", "init all on this unit; port valid"),
    ("ob_sdk_port_ifg_set", "init all on this unit; port valid; bits >= 0"),
    (
        "ob_sdk_port_loopback_set",
        "init all on this unit; port valid; mode 0..2",
    ),
    ("ob_sdk_port_pvid_set", "init all on this unit; port valid; vid 1..4094"),
    ("ob_sdk_port_learn_set", "init all on this unit; port valid; mode 0..3"),
    ("ob_sdk_port_stat_clear", "init all on this unit; port valid"),
    ("ob_sdk_vlan_create", "init all on this unit; vid 1..4094"),
    ("ob_sdk_vlan_destroy", "init all on this unit; vid 1..4094"),
    ("ob_sdk_vlan_port_add", "init all on this unit; port valid; vid 1..4094"),
    (
        "ob_sdk_vlan_port_remove",
        "init all on this unit; port valid; vid 1..4094",
    ),
    ("ob_sdk_l2_age_set", "init all on this unit; seconds >= 0"),
    ("ob_sdk_netif_destroy", "init all on this unit; ids >= -1 (-1: none)"),
    ("ob_sdk_l3_intf_destroy", "init all on this unit; id >= 0"),
    ("ob_sdk_l3_station_delete", "init all on this unit; id >= 0"),
    ("ob_sdk_l3_egress_destroy", "init all on this unit; id >= 0"),
    ("ob_sdk_l3_ecmp_destroy", "init all on this unit; id >= 0"),
    ("ob_sdk_fp_group_destroy", "init all on this unit; id >= 0"),
    (
        "ob_sdk_fp_entry_destroy",
        "init all on this unit; entry id >= 0, stat id >= -1",
    ),
    ("ob_sdk_policer_destroy", "init all on this unit; id >= 0"),
];

/// Functions with scalar arguments only that stay `unsafe`, and why (the rule is necessary, not
/// sufficient: these share glue state with other calls without a lock).
const SCALAR_UNSAFE: &[(&str, &str)] = &[
    (
        "ob_sdk_shutdown",
        "writes the glue's callback pointers (g_link_cb, g_l2_cb) without a lock, which ob_sdk_register_callbacks writes and SDK threads read",
    ),
    (
        "ob_sdk_unregister_callbacks",
        "as ob_sdk_shutdown: writes the callback pointers without a lock",
    ),
    (
        "ob_sdk_trap_set",
        "does a read-modify-write of the glue's cached L2 user entry indices (g_l2c_index) without a lock",
    ),
    (
        "ob_sdk_trunk_destroy",
        "writes the glue's trunk bitmap (g_trunk_used) without a lock",
    ),
    (
        "ob_sdk_l2_flush",
        "reads the trunk bitmap trunk create / destroy write without a lock; port and vid not checked by the glue",
    ),
];

/// Order contracts the glue hard-codes for its callers (int arguments `sdk_glue.h` documents as
/// `ob::PortStat` etc.): `enum class` declarations of backend.h, read from the header's text.
const ORDER_ENUMS: &[(&str, &str)] = &[
    (
        "PortStat",
        "the `which` of `ob_sdk_port_stat_get` (sdk_glue.c: its counter table)",
    ),
    (
        "TrapKind",
        "the `kind` of `ob_sdk_trap_set` (sdk_glue.c: LLDP / LACP by index, the switch-control table)",
    ),
    ("Fec", "the `mode` of `ob_sdk_port_fec_set`"),
    ("Loopback", "the `mode` of `ob_sdk_port_loopback_set`"),
    ("Learn", "the `mode` of `ob_sdk_port_learn_set`"),
];

/// Test hooks of `fake/fake_glue.c` (not in `sdk_glue.h)`: declared only when the fake is linked.
const FAKE_HOOKS_RS: &str = r#"unsafe extern "C" {
    pub fn ob_fake_link_event(port: ::core::ffi::c_int, up: ::core::ffi::c_int) -> ::core::ffi::c_int;
    pub fn ob_fake_query(what: ::core::ffi::c_int, arg: ::core::ffi::c_int) -> ::core::ffi::c_int;
}
"#;

fn env_any(names: &[String]) -> Option<String> {
    names.iter().find_map(|v| {
        println!("cargo:rerun-if-env-changed={v}");
        env::var(v).ok().filter(|s| !s.is_empty())
    })
}

/// The target's C compiler (as sai-sys finds it), or None when cross-compiling without one.
fn c_compiler(target: &str, host: &str) -> Option<String> {
    let under = target.replace('-', "_");
    env_any(&[
        "SDK_GLUE_SYS_CC".into(),
        "SAI_SYS_ABI_CC".into(),
        format!("CC_{under}"),
        format!("CC_{}", under.to_uppercase()),
        "TARGET_CC".into(),
    ])
    .or_else(|| (host == target).then(|| env_any(&["CC".into()]).unwrap_or_else(|| "cc".into())))
}

fn archiver(target: &str, host: &str, cc: &str) -> String {
    let under = target.replace('-', "_");
    env_any(&[
        "SDK_GLUE_SYS_AR".into(),
        format!("AR_{under}"),
        format!("AR_{}", under.to_uppercase()),
        "TARGET_AR".into(),
    ])
    .or_else(|| (host == target).then(|| env_any(&["AR".into()])).flatten())
    .unwrap_or_else(|| {
        // <prefix>gcc / <prefix>cc -> <prefix>ar
        let program = cc.split_whitespace().next().unwrap_or("cc");
        match program.strip_suffix("gcc").or_else(|| program.strip_suffix("cc")) {
            Some(prefix) if host != target || !prefix.is_empty() => format!("{prefix}ar"),
            _ => "ar".into(),
        }
    })
}

fn command(words: &str) -> Command {
    let mut words = words.split_whitespace();
    let mut c = Command::new(words.next().expect("a program name"));
    c.args(words);
    c
}

/// Where sai-openbcm's sources may be, in order: `SDK_GLUE_INC` (must hold `sdk_glue.h` when set),
/// `MDIO_SCRIPT_C_DIR`, `SAI_OPENBCM_SRC`, then the monorepo's copy next to this workspace
/// (`ai_re/nix/packages/sai-openbcm/src`).
fn candidate_dirs() -> Vec<(String, PathBuf)> {
    let mut dirs = Vec::new();
    for v in ["SDK_GLUE_INC", "MDIO_SCRIPT_C_DIR", "SAI_OPENBCM_SRC"] {
        println!("cargo:rerun-if-env-changed={v}");
        if let Some(dir) = env::var_os(v).filter(|d| !d.is_empty()).map(PathBuf::from) {
            if v == "SDK_GLUE_INC" {
                assert!(
                    dir.join("sdk_glue.h").is_file(),
                    "sdk-glue-sys: no sdk_glue.h in SDK_GLUE_INC={}",
                    dir.display()
                );
            }
            dirs.push((v.to_owned(), dir));
        }
    }
    let manifest = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    dirs.push((
        "the monorepo".to_owned(),
        manifest.join("../../../../ai_re/nix/packages/sai-openbcm/src"),
    ));
    dirs
}

/// The first directory holding `file`: `first` (the header's own directory), then the candidates.
fn find(file: &str, first: Option<&Path>, dirs: &[(String, PathBuf)]) -> Option<PathBuf> {
    first
        .into_iter()
        .chain(dirs.iter().map(|(_, d)| d.as_path()))
        .map(|d| d.join(file))
        .find(|p| p.is_file())
        .inspect(|p| println!("cargo:rerun-if-changed={}", p.display()))
}

/// clang's spelling of the target (see sai-sys/build.rs: ppc64 `ELFv2`).
fn clang_target(target: &str) -> Vec<String> {
    match target
        .strip_suffix("gnuelfv2")
        .or_else(|| target.strip_suffix("gnuabielfv2"))
    {
        Some(stem) => vec![format!("--target={stem}gnu"), "-mabi=elfv2".into()],
        None => Vec::new(),
    }
}

/// One function of the generated declarations.
struct Func {
    name: String,
    /// (name, Rust type) per argument.
    args: Vec<(String, String)>,
    /// Rust return type, "" for none.
    ret: String,
}

/// Splits at commas outside parentheses and angle brackets.
fn split_top(s: &str) -> Vec<String> {
    let (mut out, mut cur, mut depth) = (Vec::new(), String::new(), 0i32);
    for ch in s.chars() {
        match ch {
            '(' | '<' => depth += 1,
            ')' | '>' => depth -= 1,
            ',' if depth == 0 => {
                out.push(std::mem::take(&mut cur));
                continue;
            }
            _ => {}
        }
        cur.push(ch);
    }
    out.push(cur);
    out
}

/// The functions of bindgen's output (`pub fn name(a: T, ..) -> R;`, whitespace-insensitive).
fn functions(bindings: &str) -> Vec<Func> {
    let flat: String = bindings.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut out = Vec::new();
    for part in flat.split("pub fn ").skip(1) {
        let Some((name, rest)) = part.split_once('(') else {
            continue;
        };
        if !name.starts_with("ob_") {
            continue;
        }
        let mut depth = 1usize;
        let mut end = None;
        for (i, ch) in rest.char_indices() {
            match ch {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        end = Some(i);
                        break;
                    }
                }
                _ => {}
            }
        }
        let (list, tail) = rest.split_at(end.expect("a closed argument list"));
        let tail = tail.trim_start_matches(')').trim();
        let result_ty = tail
            .split_once(';')
            .map_or("", |(r, _)| r)
            .trim()
            .trim_start_matches("->")
            .trim()
            .to_owned();
        let args = split_top(list)
            .into_iter()
            .filter(|a| !a.trim().is_empty())
            .map(|a| {
                let (n, t) = a.split_once(':').expect("name: type");
                (n.trim().to_owned(), t.trim().to_owned())
            })
            .collect();
        out.push(Func {
            name: name.trim().to_owned(),
            args,
            ret: result_ty,
        });
    }
    out
}

/// A scalar of the C interface (rule (a)): an integer passed by value.
fn is_scalar(rust: &str) -> bool {
    matches!(
        rust.trim_start_matches("::core::ffi::"),
        "c_int" | "c_uint" | "i32" | "u32" | "i64" | "u64"
    )
}

/// The C spelling of a Rust type of the declarations (for the redeclaration check and the
/// fake's stubs). An unknown spelling stops the build: extend the table.
fn c_type(rust: &str) -> String {
    let t = rust.replace("::core::ffi::", "").replace(' ', "");
    if let Some(inner) = t.strip_prefix("*const") {
        return format!("const {}*", c_type(inner));
    }
    if let Some(inner) = t.strip_prefix("*mut") {
        return format!("{}*", c_type(inner));
    }
    match t.as_str() {
        "c_int" | "i32" => "int".into(),
        "c_uint" => "unsigned".into(),
        "u32" => "uint32_t".into(),
        "c_char" => "char".into(),
        "c_void" => "void".into(),
        "u8" => "uint8_t".into(),
        "u16" => "uint16_t".into(),
        "u64" => "uint64_t".into(),
        other if other.starts_with("ob_") => other.into(),
        other => panic!("sdk-glue-sys: build.rs has no C spelling for the Rust type '{other}' (from '{rust}')"),
    }
}

/// The C declaration of a generated function.
fn c_decl(f: &Func, with_names: bool) -> String {
    let args: Vec<String> = f
        .args
        .iter()
        .map(|(n, t)| {
            if with_names {
                format!("{} {n}", c_type(t))
            } else {
                c_type(t)
            }
        })
        .collect();
    let ret = if f.ret.is_empty() {
        "void".to_owned()
    } else {
        c_type(&f.ret)
    };
    format!("{ret} {}({})", f.name, args.join(", "))
}

fn strip_c_comments(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(i) = rest.find("/*") {
        out.push_str(&rest[..i]);
        rest = rest[i + 2..].split_once("*/").map_or("", |(_, r)| r);
    }
    out.push_str(rest);
    out.lines()
        .map(|l| l.split_once("//").map_or(l, |(a, _)| a))
        .collect::<Vec<_>>()
        .join("\n")
}

/// The definition of the C function `name` in `src`: the 1-based line of each of its lines, and
/// its text, from the line with the name to the closing brace.
fn c_body(src: &str, name: &str) -> Option<Vec<(usize, String)>> {
    let lines: Vec<&str> = src.lines().collect();
    let start = lines.iter().position(|l| {
        (l.starts_with("int ") || l.starts_with("void ") || l.starts_with("const char"))
            && l.contains(&format!(" {name}("))
            && !l.trim_end().ends_with(';')
    })?;
    let mut body = Vec::new();
    let (mut depth, mut seen) = (0i32, false);
    for (i, l) in lines.iter().enumerate().skip(start) {
        for ch in l.chars() {
            if ch == '{' {
                depth += 1;
                seen = true;
            } else if ch == '}' {
                depth -= 1;
            }
        }
        body.push((i + 1, (*l).to_owned()));
        if seen && depth == 0 {
            return Some(body);
        }
    }
    None
}

/// Rule (b) in `sdk_glue.c`: in the body of `name` nothing that looks like a call comes before the
/// first `OB_CHECK_` macro. The line numbers of its `OB_CHECK_` lines, or why not.
fn checked_first(src: &str, name: &str) -> Result<Vec<usize>, String> {
    let body = c_body(src, name).ok_or_else(|| format!("{name}: no definition in sdk_glue.c"))?;
    let text: String = body
        .iter()
        .map(|(_, l)| l.as_str())
        .filter(|l| !l.trim_start().starts_with('#'))
        .collect::<Vec<_>>()
        .join("\n");
    let text = strip_c_comments(&text);
    let after_brace = text.split_once('{').map_or("", |(_, b)| b);
    let Some(pos) = after_brace.find("OB_CHECK_") else {
        return Err(format!("{name}: no OB_CHECK_ macro in its body"));
    };
    let before = &after_brace[..pos];
    for (i, b) in before.char_indices() {
        if b == '(' {
            let ident: String = before[..i]
                .chars()
                .rev()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                .collect();
            if !ident.is_empty() {
                let ident: String = ident.chars().rev().collect();
                return Err(format!("{name}: '{ident}(' comes before its first OB_CHECK_ macro"));
            }
        }
    }
    Ok(body
        .iter()
        .filter(|(_, l)| l.contains("OB_CHECK_"))
        .map(|(n, _)| *n)
        .collect())
}

/// The enumerators of `enum class NAME` in a C++ header, in order.
fn cxx_enum(text: &str, name: &str) -> Option<Vec<String>> {
    let clean = strip_c_comments(text);
    let start = clean.find(&format!("enum class {name} "))?;
    let (_, body) = clean[start..].split_once('{')?;
    let (body, _) = body.split_once('}')?;
    let names: Vec<String> = body
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| {
            assert!(
                !s.contains('='),
                "sdk-glue-sys: backend.h's enum class {name} sets a value ({s}): the generator expects the implicit order"
            );
            s.to_owned()
        })
        .collect();
    (!names.is_empty()).then_some(names)
}

/// Entries of the C array initialiser `array` in the body of `func`.
fn c_table_len(glue_c: &str, func: &str, array: &str) -> Option<usize> {
    let body: String = c_body(glue_c, func)?.into_iter().map(|(_, l)| l + "\n").collect();
    let (_, rest) = body.split_once(&format!("{array}[] = {{"))?;
    let (table, _) = rest.split_once("};")?;
    Some(table.split(',').filter(|s| !s.trim().is_empty()).count())
}

/// Rust code of the order enums, and notes on what was compared with `sdk_glue.c`.
fn order_enums(backend_h: &str, glue_c: Option<&str>) -> (String, Vec<String>) {
    let mut code = String::from(
        "/// The order contracts of the glue's int arguments, generated from backend.h (see the crate docs).\npub mod order {\n",
    );
    let mut parsed = BTreeMap::new();
    for (name, what) in ORDER_ENUMS {
        let mut names =
            cxx_enum(backend_h, name).unwrap_or_else(|| panic!("sdk-glue-sys: backend.h has no 'enum class {name}'"));
        if names.last().is_some_and(|n| n == "Count") {
            names.pop();
        }
        writeln!(code, "    /// `ob::{name}` (backend.h): {what}.").expect("write");
        code.push_str("    #[repr(i32)]\n    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]\n");
        writeln!(code, "    pub enum {name} {{").expect("write");
        for (i, n) in names.iter().enumerate() {
            writeln!(code, "        /// `ob::{name}::{n}`: {i}.\n        {n} = {i},").expect("write");
        }
        code.push_str("    }\n");
        writeln!(code, "    impl {name} {{").expect("write");
        writeln!(
            code,
            "        /// Every value, in order.\n        pub const ALL: [Self; {}] = [{}];",
            names.len(),
            names
                .iter()
                .map(|n| format!("Self::{n}"))
                .collect::<Vec<_>>()
                .join(", ")
        )
        .expect("write");
        code.push_str(
            "        /// The int the glue takes.\n        pub const fn raw(self) -> ::core::ffi::c_int {\n            self as ::core::ffi::c_int\n        }\n",
        );
        code.push_str("        /// The value of an int of the glue; `None` out of range.\n        pub const fn from_raw(raw: ::core::ffi::c_int) -> ::core::option::Option<Self> {\n            match raw {\n");
        for (i, n) in names.iter().enumerate() {
            writeln!(code, "                {i} => ::core::option::Option::Some(Self::{n}),").expect("write");
        }
        code.push_str("                _ => ::core::option::Option::None,\n            }\n        }\n    }\n");
        parsed.insert(*name, names);
    }
    code.push_str("}\n");
    let mut notes = Vec::new();
    let Some(c) = glue_c else {
        notes.push("sdk_glue.c not found: its tables were not compared".to_owned());
        return (code, notes);
    };
    // The tables of sdk_glue.c these orders index, as far as its text shows them.
    let stats = parsed.get("PortStat").map(Vec::len);
    assert_eq!(
        c_table_len(c, "ob_sdk_port_stat_get", "map"),
        stats,
        "sdk-glue-sys: sdk_glue.c's port counter table and backend.h's PortStat differ in length"
    );
    let kinds = parsed.get("TrapKind").cloned().unwrap_or_default();
    assert_eq!(
        c_table_len(c, "ob_sdk_trap_set", "ctl"),
        Some(kinds.len()),
        "sdk-glue-sys: sdk_glue.c's trap control table and backend.h's TrapKind differ in length"
    );
    notes.push(format!(
        "PortStat ({}) and TrapKind ({}) as long as sdk_glue.c's tables",
        stats.unwrap_or(0),
        kinds.len()
    ));
    for (_, line) in c_body(c, "ob_sdk_trap_set").unwrap_or_default() {
        let index = line
            .split_once("kind == ")
            .and_then(|(_, r)| r.split_once(')'))
            .map(|(k, _)| k.trim().to_owned());
        let named = line
            .split_once("ob::TrapKind::")
            .map(|(_, r)| r.trim().trim_end_matches("*/").trim().to_owned());
        if let (Some(index), Some(named)) = (index, named) {
            let k: usize = index.parse().expect("a trap kind index");
            assert_eq!(
                kinds.get(k),
                Some(&named),
                "sdk-glue-sys: sdk_glue.c takes trap kind {k} as ob::TrapKind::{named}; backend.h disagrees"
            );
            notes.push(format!("TrapKind::{named} = {k} as in sdk_glue.c"));
        }
    }
    (code, notes)
}

/// bindgen's layout assertions and the header's constants as C static assertions (the sai-sys
/// recipe), and every function declared again (C refuses an incompatible redeclaration).
fn c_check(bindings: &str, funcs: &[Func]) -> (String, usize) {
    let flat: String = bindings.chars().filter(|c| !c.is_whitespace()).collect();
    let is_struct = |t: &str| flat.contains(&format!("pubstruct{t}{{"));
    let mut c = String::from("#include <stddef.h>\n#include <stdint.h>\n#include \"sdk_glue.h\"\n");
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
            if is_struct(ty) {
                check(&mut c, format!("{cfn}({ty})"), value);
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
        if is_struct(ty) {
            check(&mut c, format!("offsetof({ty}, {field})"), value);
        }
    }
    for part in flat.split("pubconstOB_").skip(1) {
        let Some((name, rest)) = part.split_once(':') else {
            continue;
        };
        let Some((_, rest)) = rest.split_once('=') else {
            continue;
        };
        let Some((value, _)) = rest.split_once(';') else {
            continue;
        };
        if value.trim_start_matches('-').chars().all(|ch| ch.is_ascii_digit()) {
            check(&mut c, format!("(long long)(OB_{name})"), &format!("{value}LL"));
        }
    }
    for f in funcs {
        writeln!(c, "{};", c_decl(f, false)).expect("write");
    }
    (c, n + funcs.len())
}

fn run_c_check(out: &Path, cc: Option<&str>, target: &str, inc: &Path, c: &str, n: usize, required: bool) -> String {
    let Some(cc) = cc else {
        assert!(
            !required,
            "sdk-glue-sys: SDK_GLUE_SYS_CHECK=require, but no C compiler for {target} (set SDK_GLUE_SYS_CC)"
        );
        return format!("skipped: no C compiler for {target} (set SDK_GLUE_SYS_CC)");
    };
    let path = out.join("check.c");
    fs::write(&path, c).expect("write check.c");
    let run = command(cc)
        // -Wno-array-parameter: `const uint8_t mac[6]` redeclared as a pointer is compatible
        .args([
            "-std=gnu11",
            "-fsyntax-only",
            "-Wall",
            "-Werror",
            "-Wno-array-parameter",
            "-I",
        ])
        .arg(inc)
        .arg(&path)
        .output();
    match run {
        Ok(o) if o.status.success() => format!(
            "checked {n} declarations and layout assertions with {cc} for {target} against {}",
            inc.join("sdk_glue.h").display()
        ),
        Ok(o) => panic!(
            "sdk-glue-sys: bindgen's declarations disagree with {cc} about {} for {target}:\n{}",
            inc.join("sdk_glue.h").display(),
            String::from_utf8_lossy(&o.stderr)
        ),
        Err(e) => {
            assert!(
                !required,
                "sdk-glue-sys: SDK_GLUE_SYS_CHECK=require, but {cc} did not run: {e}"
            );
            format!("skipped: {cc} did not run ({e})")
        }
    }
}

/// A definition for every declared function `fake_glue.c` does not define: it returns
/// `OB_E_UNAVAIL` (a void one does nothing), so a program on the C fake links whatever it names.
fn fake_stubs(funcs: &[Func], fake_c: &str) -> String {
    let mut s = String::from(
        "/* Generated by sdk-glue-sys/build.rs: the glue functions fake_glue.c does not define. */\n#include \"sdk_glue.h\"\n",
    );
    for f in funcs {
        if c_body(fake_c, &f.name).is_some() {
            continue;
        }
        writeln!(s, "{}\n{{", c_decl(f, true)).expect("write");
        for (n, _) in &f.args {
            writeln!(s, "    (void){n};").expect("write");
        }
        if f.ret.contains('*') {
            s.push_str("    return 0;\n");
        } else if !f.ret.is_empty() {
            s.push_str("    return OB_E_UNAVAIL;\n");
        }
        s.push_str("}\n");
    }
    s
}

fn link_fake(out: &Path, cc: Option<&str>, target: &str, host: &str, inc: &Path, funcs: &[Func]) {
    let cc = cc.unwrap_or_else(|| {
        panic!("sdk-glue-sys: feature 'fake' needs a C compiler for {target} (set SDK_GLUE_SYS_CC)")
    });
    let src = Path::new(&env::var_os("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR")).join("fake/fake_glue.c");
    println!("cargo:rerun-if-changed={}", src.display());
    let fake_c = fs::read_to_string(&src).expect("read fake_glue.c");
    let stubs = out.join("fake_stubs.c");
    fs::write(&stubs, fake_stubs(funcs, &fake_c)).expect("write fake_stubs.c");
    let run = |mut c: Command, what: &str| {
        let o = c
            .output()
            .unwrap_or_else(|e| panic!("sdk-glue-sys: cannot run {what}: {e}"));
        assert!(
            o.status.success(),
            "sdk-glue-sys: {what} failed:\n{}",
            String::from_utf8_lossy(&o.stderr)
        );
    };
    let mut objs = Vec::new();
    for (file, obj) in [(src.as_path(), "fake_glue.o"), (stubs.as_path(), "fake_stubs.o")] {
        let obj = out.join(obj);
        let mut compile = command(cc);
        compile
            .args([
                "-std=gnu11",
                "-O1",
                "-g",
                "-fPIC",
                "-Wall",
                "-Wextra",
                "-Wno-array-parameter",
                "-I",
            ])
            .arg(out.join("include"))
            .arg("-I")
            .arg(inc)
            .arg("-c")
            .arg(file)
            .arg("-o")
            .arg(&obj);
        run(compile, cc);
        objs.push(obj);
    }
    let ar = archiver(target, host, cc);
    let lib = out.join("libob_fake_glue.a");
    let _ = fs::remove_file(&lib);
    let mut archive = command(&ar);
    archive.arg("rcs").arg(&lib).args(&objs);
    run(archive, &ar);
    println!("cargo:rustc-link-search=native={}", out.display());
    println!("cargo:rustc-link-lib=static=ob_fake_glue");
}

/// `SDK_GLUE_LINK`: `<dir>/lib<name>.a` (static), `-L<dir>`, `-l<name>` (dynamic), in link order.
fn link_real() {
    println!("cargo:rerun-if-env-changed=SDK_GLUE_LINK");
    let spec = env::var("SDK_GLUE_LINK").unwrap_or_else(|_| {
        panic!(
            "sdk-glue-sys: feature 'real' needs SDK_GLUE_LINK, e.g. \
             \"/build/libob_sdk_glue.a -L<libsdk>/lib -lsdk -lpthread\" (sdk_glue.o in an archive, then the SDK)"
        )
    });
    let mut items = 0;
    for item in spec.split_whitespace() {
        if let Some(dir) = item.strip_prefix("-L") {
            println!("cargo:rustc-link-search=native={dir}");
        } else if let Some(name) = item.strip_prefix("-l") {
            println!("cargo:rustc-link-lib=dylib={name}");
        } else {
            let path = Path::new(item);
            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .and_then(|n| n.strip_prefix("lib"))
                .and_then(|n| n.strip_suffix(".a"))
                .unwrap_or_else(|| {
                    panic!("sdk-glue-sys: SDK_GLUE_LINK item '{item}' is not -L<dir>, -l<name> or <dir>/lib<name>.a")
                });
            assert!(path.is_file(), "sdk-glue-sys: SDK_GLUE_LINK: no file {item}");
            println!("cargo:rerun-if-changed={item}");
            if let Some(dir) = path.parent() {
                println!("cargo:rustc-link-search=native={}", dir.display());
            }
            println!("cargo:rustc-link-lib=static={name}");
        }
        items += 1;
    }
    assert!(items > 0, "sdk-glue-sys: SDK_GLUE_LINK is empty");
}

/// What makes a non-scalar function unsafe to call, from its signature.
fn unsafe_kinds(f: &Func) -> String {
    let mut kinds = Vec::new();
    for (n, t) in &f.args {
        if t.contains("_cb_t") {
            kinds.push(format!("callback `{n}`"));
        } else if t.contains("ob_") && t.contains('*') {
            kinds.push(format!("struct pointer `{n}`"));
        } else if t.contains("c_char") {
            kinds.push(format!("string / buffer `{n}`"));
        } else if t.contains('*') {
            kinds.push(format!("pointer `{n}`"));
        }
    }
    if f.ret.contains('*') {
        kinds.push("returns a pointer".into());
    }
    kinds.join(", ")
}

/// Marks the [`SAFE`] functions `safe fn` after checking rule (a) on their signatures and, with
/// `sdk_glue.c`, rule (b) on their bodies. The table of every function, and a summary line.
fn apply_rule(bindings: &mut String, funcs: &[Func], glue_c: Option<&str>) -> (String, String) {
    let names: Vec<&str> = funcs.iter().map(|f| f.name.as_str()).collect();
    for (name, _) in SAFE.iter().chain(SCALAR_UNSAFE) {
        assert!(
            names.contains(name),
            "sdk-glue-sys: build.rs names {name}, which sdk_glue.h does not declare"
        );
    }
    let mut table = String::from("| function | declared | why |\n|---|---|---|\n");
    let (mut safe_n, mut unsafe_n) = (0, 0);
    for f in funcs {
        let scalar = f.args.iter().all(|(_, t)| is_scalar(t)) && (f.ret.is_empty() || is_scalar(&f.ret));
        if let Some((_, what)) = SAFE.iter().find(|(n, _)| *n == f.name) {
            assert!(
                scalar,
                "sdk-glue-sys: {} is listed as safe, but its signature is not all scalars (rule (a), src/lib.rs)",
                f.name
            );
            let lines = match glue_c.map(|c| checked_first(c, &f.name)) {
                Some(Ok(lines)) => format!(
                    " (sdk_glue.c line{} {})",
                    if lines.len() == 1 { "" } else { "s" },
                    lines.iter().map(ToString::to_string).collect::<Vec<_>>().join(", ")
                ),
                Some(Err(why)) => panic!("sdk-glue-sys: rule (b) (src/lib.rs) does not hold: {why}"),
                None => String::new(),
            };
            let needle = format!("pub fn {}(", f.name);
            assert_eq!(
                bindings.matches(&needle).count(),
                1,
                "sdk-glue-sys: {} is declared twice",
                f.name
            );
            *bindings = bindings.replace(&needle, &format!("pub safe fn {}(", f.name));
            writeln!(
                table,
                "| `{}` | safe | scalars; the glue checks: {what}{lines} |",
                f.name
            )
            .expect("write");
            safe_n += 1;
        } else {
            let why = if let Some((_, why)) = SCALAR_UNSAFE.iter().find(|(n, _)| *n == f.name) {
                format!("scalars, but it {why}")
            } else {
                assert!(
                    !scalar,
                    "sdk-glue-sys: {} has an all-scalar signature but is in neither SAFE nor SCALAR_UNSAFE (build.rs)",
                    f.name
                );
                unsafe_kinds(f)
            };
            writeln!(table, "| `{}` | unsafe | {why} |", f.name).expect("write");
            unsafe_n += 1;
        }
    }
    let rule_b = if glue_c.is_some() {
        format!("rule (b) checked in sdk_glue.c for all {safe_n}")
    } else {
        "rule (b) not checked (no sdk_glue.c)".to_owned()
    };
    let summary = format!(
        "{} functions: {safe_n} declared safe, {unsafe_n} unsafe; {rule_b}",
        funcs.len()
    );
    (table, summary)
}

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=SDK_GLUE_SYS_CHECK");
    let required = env::var("SDK_GLUE_SYS_CHECK").is_ok_and(|v| v == "require");
    let out = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR"));
    let target = env::var("TARGET").expect("TARGET");
    let host = env::var("HOST").expect("HOST");
    let dirs = candidate_dirs();
    let header = find("sdk_glue.h", None, &dirs).unwrap_or_else(|| {
        panic!(
            "sdk-glue-sys: no sdk_glue.h: set SDK_GLUE_INC to sai-openbcm's src directory (looked in {})",
            dirs.iter()
                .map(|(v, d)| format!("{v}: {}", d.display()))
                .collect::<Vec<_>>()
                .join(", ")
        )
    });
    let inc = header.parent().expect("a directory").to_path_buf();
    let backend_h = find("backend.h", Some(&inc), &dirs).unwrap_or_else(|| {
        panic!(
            "sdk-glue-sys: no backend.h (the glue's order contracts) next to {} or in SAI_OPENBCM_SRC",
            header.display()
        )
    });
    let glue_c = find("sdk_glue.c", Some(&inc), &dirs).map(|p| fs::read_to_string(p).expect("read sdk_glue.c"));
    assert!(
        glue_c.is_some() || !required,
        "sdk-glue-sys: SDK_GLUE_SYS_CHECK=require, but no sdk_glue.c next to {} (rule (b) is not checked)",
        header.display()
    );

    let generated = bindgen::Builder::default()
        .header(header.to_str().expect("a UTF-8 path"))
        .clang_args(clang_target(&target))
        .allowlist_function("ob_sdk_.*")
        .allowlist_type("ob_.*")
        .allowlist_var("OB_.*")
        .default_enum_style(bindgen::EnumVariation::Consts)
        .prepend_enum_name(false)
        .translate_enum_integer_types(true)
        .layout_tests(true)
        .derive_default(false) // with pointer fields bindgen writes unsafe zeroing impls
        .derive_debug(true)
        .generate_comments(false)
        .use_core()
        .formatter(bindgen::Formatter::Prettyplease)
        .generate()
        .expect("bindgen over sdk_glue.h");
    let mut bindings = generated.to_string();
    let funcs = functions(&bindings);
    assert!(
        funcs.len() >= 26,
        "sdk-glue-sys: only {} functions parsed from bindgen's output: the parser in build.rs is stale",
        funcs.len()
    );
    let (table, summary) = apply_rule(&mut bindings, &funcs, glue_c.as_deref());
    let backend_text = fs::read_to_string(&backend_h).expect("read backend.h");
    let (orders, notes) = order_enums(&backend_text, glue_c.as_deref());

    let (c, n) = c_check(&bindings, &funcs);
    let cc = c_compiler(&target, &host);
    let result = run_c_check(&out, cc.as_deref(), &target, &inc, &c, n, required);

    let mut rs =
        String::from("// Generated by build.rs: bindgen over sdk_glue.h, then the safe-fn rule (src/lib.rs).\n");
    rs.push_str(&bindings);
    rs.push_str(&orders);
    if env::var_os("CARGO_FEATURE_FAKE").is_some() {
        rs.push_str(FAKE_HOOKS_RS);
    }
    fs::write(out.join("bindings.rs"), rs).expect("write bindings.rs");
    fs::write(
        out.join("declarations.md"),
        format!(
            "\n\n# Declarations\n\n{summary}. Generated by `build.rs` from the `sdk_glue.h` of this build.\n\n{table}"
        ),
    )
    .expect("write declarations.md");
    fs::create_dir_all(out.join("include")).expect("mkdir include");
    // What fake_glue.c includes: the header itself (mdio-tool's build writes the same shim).
    fs::write(out.join("include/ob_sdk_glue_decls.h"), "#include \"sdk_glue.h\"\n").expect("write the C declarations");

    println!("cargo:rustc-env=SDK_GLUE_SYS_CHECK_RESULT={result}");
    println!("cargo:rustc-env=SDK_GLUE_SYS_RULE={summary}");
    println!("cargo:rustc-env=SDK_GLUE_SYS_ORDERS={}", notes.join("; "));
    // In the build log on purpose, like sai-sys: was the header consulted, by which compiler.
    println!("cargo:warning=sdk_glue.h check: {result}; {summary}");

    let real = env::var_os("CARGO_FEATURE_REAL").is_some();
    let fake = env::var_os("CARGO_FEATURE_FAKE").is_some();
    assert!(
        !(real && fake),
        "sdk-glue-sys: features 'real' and 'fake' both define the glue's functions: enable one \
         (the fake comes in through dev-dependencies: build the real tool without tests, e.g. cargo build -p mdio-tool --features sdk-real)"
    );
    if real {
        link_real();
    } else if fake {
        link_fake(&out, cc.as_deref(), &target, &host, &inc, &funcs);
    }
    let linked = if real {
        "real"
    } else if fake {
        "fake"
    } else {
        "none"
    };
    println!("cargo:rustc-env=SDK_GLUE_SYS_LINKED={linked}");
    // For the build scripts of direct dependents (DEP_OB_SDK_GLUE_LINKED): sdk-glue compiles
    // its test-hook module only when the fake is what is linked.
    println!("cargo:linked={linked}");
}
