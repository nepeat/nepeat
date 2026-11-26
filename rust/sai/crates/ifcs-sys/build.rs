//! Generates the bindings of the IFCS headers and checks them against the target's C compiler.
//! See `src/lib.rs`.
// A build script reports a failure by panicking with a message.
#![allow(clippy::expect_used, clippy::panic, clippy::too_many_lines)]

use std::collections::BTreeSet;
use std::env;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The unions of the header, made opaque (a blob of their size and alignment): reading a Rust
/// union is unsafe, a blob is not. A union the header grows stops the build until it is listed.
const UNIONS: &[&str] = &[
    "ifcs_attr_value_t",
    "ifcs_ip_addr_t",
    "ifcs_l2_entry_key_map_t",
    "ifcs_route_entry_key_map_t",
    "ifcs_qos_map_fields_t",
];

fn main() {
    for v in [
        "IFCS_API_INC",
        "IFCS_CTYPES",
        "IFCS_OVERRIDES",
        "IFCS_API_EXPECT",
        "IFCS_SYS_ABI_CC",
        "IFCS_SYS_ABI_CHECK",
        "TARGET_CC",
        "CC",
    ] {
        println!("cargo:rerun-if-env-changed={v}");
    }
    let out = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR"));
    let arch = env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
    let os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if os != "linux" || !matches!(arch.as_str(), "x86_64" | "aarch64") {
        // src/lib.rs is configured out on this target: nothing to bind, no header needed.
        fs::write(out.join("bindings.rs"), "").expect("write bindings");
        fs::write(out.join("symbols.rs"), "").expect("write symbols");
        println!("cargo:rustc-env=IFCS_SYS_ABI_CHECK_RESULT=skipped: no Teralynx SAI for {arch}-{os}");
        return;
    }
    let inc = match (env::var_os("IFCS_API_INC"), env::var_os("IFCS_CTYPES")) {
        (Some(dir), _) => PathBuf::from(dir),
        (None, Some(ctypes)) => generate_headers(&out, Path::new(&ctypes)),
        (None, None) => panic!(
            "ifcs-sys: set IFCS_CTYPES to the ctypes file the IFCS headers are generated from (the pinned \
             ishell 2.0.1 ifcs_ctypes.py: rust/sai/nix/ifcs-headers.nix), or IFCS_API_INC to a directory with \
             headers generated already; they are never committed"
        ),
    };
    for h in ["ifcs_api.h", "ifcs_api_qos.h"] {
        assert!(
            inc.join(h).is_file(),
            "ifcs-sys: no {h} in IFCS_API_INC={}",
            inc.display()
        );
        println!("cargo:rerun-if-changed={}", inc.join(h).display());
    }
    let header_text = format!(
        "{}\n{}",
        fs::read_to_string(inc.join("ifcs_api.h")).expect("read ifcs_api.h"),
        fs::read_to_string(inc.join("ifcs_api_qos.h")).expect("read ifcs_api_qos.h")
    );
    let wrapper_h = out.join("ifcs_sys_wrapper.h");
    fs::write(&wrapper_h, "#include \"ifcs_api.h\"\n").expect("write wrapper");

    let base = || {
        bindgen::Builder::default()
            .header(wrapper_h.to_str().expect("utf-8 OUT_DIR"))
            .clang_arg(format!("-I{}", inc.display()))
            .allowlist_item("[iI][fF][cC][sS]_.*")
            .default_enum_style(bindgen::EnumVariation::Consts)
            .prepend_enum_name(false)
            .layout_tests(true)
            .derive_default(false) // its impls for unions and big arrays are unsafe { zeroed() }: none here
            .derive_debug(false)
            .generate_comments(false)
            .use_core()
            .formatter(bindgen::Formatter::Prettyplease)
    };
    // The unions as clang lays them out, then each turned into a byte blob of the same size and
    // alignment (bindgen's opaque_type does not take the header's typedef'd anonymous unions):
    // the union's body and its field offset tests go, its size and alignment tests stay.
    let mut text = base().generate().expect("bindgen over ifcs_api.h").to_string();
    for u in UNIONS {
        text = blobify(&text, u);
    }
    let unions: Vec<&str> = text
        .split("pub union ")
        .skip(1)
        .filter_map(|p| p.split_whitespace().next())
        .collect();
    assert!(
        unions.is_empty(),
        "ifcs-sys: the header has unions that are not in build.rs UNIONS (make them opaque): {unions:?}"
    );
    for u in UNIONS {
        assert!(
            text.contains(&format!("pub struct {u} {{")),
            "ifcs-sys: union {u} of build.rs UNIONS is not in the header any more"
        );
    }
    let symbols = symbol_list(&header_text);
    // Every struct gets a byte twin, and the by-value struct parameters of the IFCS functions take
    // it (src/lib.rs, "By-value structs"): written after the union check, so the twins (structs)
    // are not mistaken for unions.
    let text = twins(&text, &symbols);
    fs::write(out.join("bindings.rs"), &text).expect("write bindings");

    let flat: String = text.chars().filter(|c| !c.is_whitespace()).collect();
    for s in &symbols {
        assert!(
            flat.contains(&format!("pubtype{s}_fn=")),
            "ifcs-sys: IFCS_SYMBOLS entry {s} has no {s}_fn typedef in the bindings"
        );
    }
    // Every <name>_fn typedef of the header is an IFCS_SYMBOLS entry (and the other way round).
    let typedefs: BTreeSet<String> = flat
        .split("pubtype")
        .skip(1)
        .filter_map(|p| p.split_once('=').map(|(n, _)| n.to_owned()))
        .filter_map(|n| n.strip_suffix("_fn").map(str::to_owned))
        .collect();
    let listed: BTreeSet<String> = symbols.iter().cloned().collect();
    assert!(
        typedefs == listed,
        "ifcs-sys: <name>_fn typedefs and IFCS_SYMBOLS differ: only typedef {:?}, only listed {:?}",
        typedefs.difference(&listed).collect::<Vec<_>>(),
        listed.difference(&typedefs).collect::<Vec<_>>()
    );
    let mut sym = String::new();
    writeln!(
        sym,
        "/// Entries of the header's `IFCS_SYMBOLS` X-macro list (every function libifcs is asked for)."
    )
    .expect("w");
    writeln!(sym, "pub const SYMBOL_COUNT: usize = {};", symbols.len()).expect("w");
    writeln!(sym, "/// Their names, in the list's order.").expect("w");
    writeln!(sym, "pub const SYMBOLS: [&str; {}] = [", symbols.len()).expect("w");
    for s in &symbols {
        writeln!(sym, "    \"{s}\",").expect("w");
    }
    writeln!(sym, "];").expect("w");
    writeln!(
        sym,
        "/// Calls `$m!` with every `IFCS_SYMBOLS` entry as `(name, type)`, the type being its `<name>_fn`."
    )
    .expect("w");
    writeln!(
        sym,
        "#[macro_export]\nmacro_rules! for_each_symbol {{\n    ($m:ident) => {{\n        $m! {{"
    )
    .expect("w");
    for s in &symbols {
        writeln!(sym, "            ({s}, $crate::{s}_fn),").expect("w");
    }
    writeln!(sym, "        }}\n    }};\n}}").expect("w");
    fs::write(out.join("symbols.rs"), sym).expect("write symbols");

    let (c, n) = abi_check_source(&text, &header_text, &symbols);
    let c_path = out.join("ifcs_abi_check.c");
    fs::write(&c_path, c).expect("write ifcs_abi_check.c");
    let result = run_abi_check(&c_path, &inc, n);
    println!("cargo:rustc-env=IFCS_SYS_ABI_CHECK_RESULT={result}");
    // In the build log on purpose: which compiler confirmed the bindings, for which target.
    println!("cargo:warning=IFCS ABI check: {result} ({} symbols)", symbols.len());
    println!("cargo:include={}", inc.display());
}

/// Generates `ifcs_api.h` / `ifcs_api_qos.h` into `$OUT_DIR/ifcs-api` with ifcs-gen (our templates,
/// `IFCS_OVERRIDES`: the overrides file, default ifcs-gen's, empty: none) from the ctypes file; with
/// `IFCS_API_EXPECT` (a directory of headers, Nix: `nix/ifcs-headers.nix`) each must be equal to its
/// copy there, byte for byte (the C fixtures compile against those).
fn generate_headers(out: &Path, ctypes: &Path) -> PathBuf {
    println!("cargo:rerun-if-changed={}", ctypes.display());
    let path = ctypes.to_str().expect("IFCS_CTYPES: a UTF-8 path");
    let bytes = fs::read(ctypes).unwrap_or_else(|e| panic!("ifcs-sys: IFCS_CTYPES={path}: {e}"));
    let own = env::var("IFCS_OVERRIDES").ok();
    let text = own.as_deref().filter(|p| !p.is_empty()).map(|p| {
        println!("cargo:rerun-if-changed={p}");
        fs::read_to_string(p).unwrap_or_else(|e| panic!("ifcs-sys: IFCS_OVERRIDES={p}: {e}"))
    });
    let overrides = match (own.as_deref(), text.as_deref()) {
        (None, _) => Some(ifcs_gen::templates::OVERRIDES),
        (Some(p), Some(t)) => Some((p, t)),
        (Some(_), None) => None,
    };
    let g = ifcs_gen::generate(&ifcs_gen::Input {
        ctypes_path: path,
        ctypes: &bytes,
        overrides,
        source: None,
        templates: &ifcs_gen::templates::TEMPLATES,
    })
    .unwrap_or_else(|e| panic!("ifcs-sys: generating the IFCS headers from {path}: {e}"));
    assert!(
        g.errors.is_empty(),
        "ifcs-sys: generating the IFCS headers from {path}:\n  {}",
        g.errors.join("\n  ")
    );
    let dir = out.join("ifcs-api");
    fs::create_dir_all(&dir).expect("create ifcs-api");
    let expect = env::var_os("IFCS_API_EXPECT").map(PathBuf::from);
    for (name, text) in &g.outputs {
        fs::write(dir.join(name), text).expect("write a generated header");
        if let Some(e) = &expect {
            let want = fs::read(e.join(name)).unwrap_or_else(|err| panic!("ifcs-sys: IFCS_API_EXPECT: {name}: {err}"));
            assert!(
                want == text.as_bytes(),
                "ifcs-sys: the generated {name} differs from IFCS_API_EXPECT={}",
                e.display()
            );
        }
    }
    dir
}

/// bindgen's layout test value `what` (`Size` / `Alignment`) of `ty` in the whitespace-free bindings.
fn layout_of(flat: &str, ty: &str, what: &str, f: &str) -> Option<usize> {
    let key = format!("\"{what}of{ty}\"][::core::mem::{f}::<{ty}>()-");
    flat.split_once(&key)
        .and_then(|(_, r)| r.split_once("usize"))
        .and_then(|(v, _)| v.parse().ok())
}

/// The field types of `pub struct <name> { ... }` in the bindings (prettyplease's form: one
/// `pub field: type,` per line).
fn field_types<'a>(text: &'a str, name: &str) -> Vec<&'a str> {
    let head = format!("pub struct {name} {{");
    let Some(start) = text.find(&head) else {
        return Vec::new();
    };
    let body = &text[start + head.len()..];
    let body = &body[..body.find("\n}").unwrap_or(body.len())];
    body.lines()
        .filter_map(|l| l.trim().strip_prefix("pub "))
        .filter_map(|l| l.split_once(": ").map(|(_, t)| t.trim_end_matches(',').trim()))
        .collect()
}

/// Whether a type of the bindings is or holds a floating-point value (directly, in an array, or
/// in a struct it holds): such a struct is classified otherwise than its bytes (`SysV` SSE class,
/// AAPCS64 HFA), so it cannot be passed as a byte twin.
fn has_float(text: &str, structs: &BTreeSet<String>, ty: &str, depth: usize) -> bool {
    let t = ty.trim();
    if ["f32", "f64", "c_float", "c_double"].iter().any(|f| t.ends_with(f)) {
        return true;
    }
    if let Some(inner) = t.strip_prefix('[').and_then(|r| r.rsplit_once(';')).map(|(e, _)| e) {
        return has_float(text, structs, inner, depth);
    }
    if depth < 8 && structs.contains(t) {
        return field_types(text, t)
            .iter()
            .any(|f| has_float(text, structs, f, depth + 1));
    }
    false
}

/// The byte twins (`<struct>_bytes`, src/lib.rs "By-value structs") of every struct of the
/// bindings (the union blobs and bindgen's anonymous types aside), and the by-value struct
/// parameters of the IFCS functions' `<name>_fn` typedefs rewritten to them, listed in
/// `BY_VALUE_ARGS`. Refused (build failure): a by-value struct parameter of a struct with a
/// floating-point member or an alignment above 8 (another argument class than its bytes'), a
/// by-value struct anywhere else (a callback's parameter, a return value).
fn twins(text: &str, symbols: &[String]) -> String {
    let flat: String = text
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect::<String>()
        .replace("\",]", "\"]");
    let structs: BTreeSet<String> = text
        .split("pub struct ")
        .skip(1)
        .filter_map(|p| p.split_whitespace().next())
        .filter(|n| !n.starts_with("_bindgen") && !n.contains("__bindgen") && !UNIONS.contains(n))
        .map(str::to_owned)
        .collect();
    let mut out = String::with_capacity(text.len() + structs.len() * 600);
    let mut rewritten: Vec<(String, usize, String)> = Vec::new();
    let mut rest = text;
    let fn_head = "unsafe extern \"C\" fn(";
    while let Some(at) = rest.find(fn_head) {
        let open = at + fn_head.len();
        out.push_str(&rest[..open]);
        // the typedef this function type is the whole of (`pub type <name> = Option<fn(..)>`), if any
        let owner = out
            .rfind("pub type ")
            .map(|p| &out[p + "pub type ".len()..])
            .and_then(|s| {
                let flat: String = s.chars().filter(|c| !c.is_whitespace()).collect();
                let (name, rhs) = flat.split_once('=')?;
                (rhs == "::core::option::Option<unsafeextern\"C\"fn(").then(|| name.to_owned())
            })
            .unwrap_or_default();
        let mut depth = 1usize;
        let mut close = open;
        let mut prev = ' ';
        for (i, ch) in rest[open..].char_indices() {
            match ch {
                '(' | '<' | '[' => depth += 1,
                '>' if prev == '-' => {}
                ')' | '>' | ']' => depth -= 1,
                _ => {}
            }
            prev = ch;
            if depth == 0 {
                close = open + i;
                break;
            }
        }
        let params = &rest[open..close];
        let sym = owner.strip_suffix("_fn").filter(|s| symbols.iter().any(|x| x == s));
        let mut new_params = String::new();
        for (i, p) in split_top(params).into_iter().enumerate() {
            let ty = p.split_once(':').map(|(_, t)| t.trim());
            match ty.filter(|t| structs.contains(*t)) {
                Some(t) => {
                    let s = sym.unwrap_or_else(|| {
                        panic!("ifcs-sys: {owner} takes struct {t} by value (only IFCS functions may: build.rs twins)")
                    });
                    let align = layout_of(&flat, t, "Alignment", "align_of").unwrap_or(usize::MAX);
                    assert!(
                        align <= 8 && !has_float(text, &structs, t, 0),
                        "ifcs-sys: {s} takes {t} by value, which has a floating-point member or alignment {align}: \
                         its byte twin would be passed otherwise (src/lib.rs, \"By-value structs\")"
                    );
                    new_params.push_str(&p.replacen(t, &format!("{t}_bytes"), 1));
                    rewritten.push((s.to_owned(), i, t.to_owned()));
                }
                None => new_params.push_str(p),
            }
            new_params.push(',');
        }
        new_params.pop();
        out.push_str(&new_params);
        rest = &rest[close..];
        // the return type, up to the end of the type: no struct by value
        let ret_end = rest.find(['\n', ';']).unwrap_or(rest.len());
        if let Some((_, r)) = rest[..ret_end].split_once("->") {
            let r = r.trim().trim_end_matches(',');
            assert!(!structs.contains(r), "ifcs-sys: {owner} returns struct {r} by value");
        }
    }
    out.push_str(rest);
    out.push_str("\n// ---- byte twins (build.rs twins) ----\n");
    for s in &structs {
        let size = layout_of(&flat, s, "Size", "size_of")
            .unwrap_or_else(|| panic!("ifcs-sys: no size of struct {s} in bindgen's layout tests"));
        let align = layout_of(&flat, s, "Alignment", "align_of")
            .unwrap_or_else(|| panic!("ifcs-sys: no alignment of struct {s} in bindgen's layout tests"));
        let _ = write!(
            out,
            "/// The bytes of a [`{s}`]: its size and alignment, every byte a `u8` (no padding).\n\
             #[repr(C, align({align}))]\n#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]\n\
             pub struct {s}_bytes(pub [u8; {size}]);\n\
             const _: () = assert!(::core::mem::size_of::<{s}_bytes>() == ::core::mem::size_of::<{s}>());\n\
             const _: () = assert!(::core::mem::align_of::<{s}_bytes>() == ::core::mem::align_of::<{s}>());\n\
             impl crate::CBytes for {s}_bytes {{\n    type C = {s};\n    const ZERO: Self = Self([0; {size}]);\n\
             fn as_bytes(&self) -> &[u8] {{\n        &self.0\n    }}\n\
             fn as_bytes_mut(&mut self) -> &mut [u8] {{\n        &mut self.0\n    }}\n}}\n"
        );
    }
    out.push_str(
        "/// The by-value struct parameters of the IFCS functions, each declared as the struct's byte twin\n\
         /// in its `<name>_fn` typedef: (function, parameter index from 0, struct).\n\
         pub const BY_VALUE_ARGS: &[(&str, usize, &str)] = &[\n",
    );
    for (f, i, t) in &rewritten {
        let _ = writeln!(out, "    (\"{f}\", {i}, \"{t}\"),");
    }
    out.push_str("];\n");
    out
}

/// `s` split at its commas outside brackets.
fn split_top(s: &str) -> Vec<&str> {
    let (mut out, mut depth, mut start, mut prev) = (Vec::new(), 0usize, 0usize, ' ');
    for (i, ch) in s.char_indices() {
        match ch {
            '(' | '<' | '[' => depth += 1,
            '>' if prev == '-' => {}
            ')' | '>' | ']' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                out.push(&s[start..i]);
                start = i + 1;
            }
            _ => {}
        }
        prev = ch;
    }
    out.push(&s[start..]);
    out
}

/// Replaces `pub union <u> { ... }` with `pub struct <u> { pub blob: [u8; size] }` of the same
/// size and alignment (from bindgen's own layout test of the union), and drops the union's field
/// offset tests (every member is at offset 0).
fn blobify(text: &str, u: &str) -> String {
    let flat: String = text
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect::<String>()
        .replace("\",]", "\"]");
    let layout = |what: &str, f: &str| -> usize {
        let key = format!("\"{what}of{u}\"][::core::mem::{f}::<{u}>()-");
        flat.split_once(&key)
            .and_then(|(_, r)| r.split_once("usize"))
            .and_then(|(v, _)| v.parse().ok())
            .unwrap_or_else(|| panic!("ifcs-sys: no {what} of union {u} in bindgen's layout tests"))
    };
    let (size, align) = (layout("Size", "size_of"), layout("Alignment", "align_of"));
    let head = format!("pub union {u} {{");
    let start = text
        .find(&head)
        .unwrap_or_else(|| panic!("ifcs-sys: no union {u} in the bindings"));
    let end = start + text[start..].find("\n}").expect("union end") + 2;
    let attrs = text[..start].rfind("#[repr(C)]").expect("union attributes");
    let mut out = format!(
        "{}#[repr(C, align({align}))]\n#[derive(Copy, Clone)]\npub struct {u} {{\n    pub blob: [u8; {size}],\n}}{}",
        &text[..attrs],
        &text[end..]
    );
    let field = format!("\"Offset of field: {u}::");
    while let Some(at) = out.find(&field) {
        let stmt = out[..at].rfind("\n    [").expect("layout test start");
        let stop = at + out[at..].find("];").expect("layout test end") + 2;
        out.replace_range(stmt..stop, "");
    }
    out
}

/// The `IFCS_SYMBOLS` list as the header writes it: the X(name) entries of the macro bodies of
/// `IFCS_SYMBOLS_QOS(X)` and `IFCS_SYMBOLS(X)` (which starts with the former), in order.
fn symbol_list(header: &str) -> Vec<String> {
    let body = |name: &str| -> String {
        let start = format!("#define {name}(X)");
        let lines = header.lines().skip_while(|l| !l.starts_with(&start));
        let mut body = String::new();
        for line in lines {
            let cont = line.trim_end().ends_with('\\');
            body.push_str(line.trim_end().trim_end_matches('\\'));
            body.push(' ');
            if !cont {
                break;
            }
        }
        assert!(!body.is_empty(), "ifcs-sys: no {start} in the header");
        body.trim_start_matches(&start).to_owned()
    };
    let entries = |text: &str| -> Vec<String> {
        text.split("X(")
            .skip(1)
            .filter_map(|p| p.split_once(')').map(|(n, _)| n.trim().to_owned()))
            .filter(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'))
            .collect()
    };
    let qos = entries(&body("IFCS_SYMBOLS_QOS"));
    let main = body("IFCS_SYMBOLS");
    assert!(
        main.contains("IFCS_SYMBOLS_QOS(X)"),
        "ifcs-sys: IFCS_SYMBOLS no longer starts with IFCS_SYMBOLS_QOS(X)"
    );
    let mut all = qos;
    all.extend(entries(&main));
    let unique: BTreeSet<&String> = all.iter().collect();
    assert!(unique.len() == all.len(), "ifcs-sys: IFCS_SYMBOLS lists a name twice");
    assert!(
        all.len() > 100,
        "ifcs-sys: only {} IFCS_SYMBOLS entries found: build.rs is stale",
        all.len()
    );
    all
}

/// The C spelling of a type of the bindings: the header's types are typedefs of anonymous
/// structs (`typedef struct { ... } name;`); a tagged one would be `struct name`.
fn c_type(header: &str, bindings_flat: &str, t: &str) -> Option<String> {
    if t.contains("__bindgen") || t.starts_with("_bindgen") {
        return None; // anonymous member type: no C spelling
    }
    let known = bindings_flat.contains(&format!("pubstruct{t}{{"));
    if !known {
        return None;
    }
    if header.contains(&format!("}} {t};")) || header.contains(&format!("}}{t};")) {
        Some(t.to_owned())
    } else if header.contains(&format!("struct {t} {{")) {
        Some(format!("struct {t}"))
    } else if header.contains(&format!(" {t};")) {
        Some(t.to_owned()) // typedef of another typedef (ifcs_bam_queue_t)
    } else {
        None
    }
}

/// bindgen's layout assertions, every enumerator / constant (clang's view of the target) and the
/// symbol list as C static assertions for the target's own C compiler. Returns the source and
/// the number of assertions.
fn abi_check_source(bindings: &str, header: &str, symbols: &[String]) -> (String, usize) {
    // Whitespace-free copy: prettyplease wraps long assertions over several lines.
    let flat: String = bindings.chars().filter(|c| !c.is_whitespace()).collect();
    let mut c = String::from("#include <stddef.h>\n#include \"ifcs_api.h\"\n");
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
            if let Some(ct) = c_type(header, &flat, ty) {
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
        if let Some(ct) = c_type(header, &flat, ty) {
            check(&mut c, format!("offsetof({ct}, {})", c_field(field)), value);
        }
    }
    // Enumerators and object-like macros: "pub const IFCS_X: <type> = N;".
    let mut constants = 0usize;
    for part in flat.split("pubconstIFCS_").skip(1) {
        let Some((name, rest)) = part.split_once(':') else {
            continue;
        };
        let Some((_ty, rest)) = rest.split_once('=') else {
            continue;
        };
        let Some((value, _)) = rest.split_once(';') else {
            continue;
        };
        if !value.trim_start_matches('-').chars().all(|ch| ch.is_ascii_digit()) || value.is_empty() {
            continue;
        }
        check(&mut c, format!("(long long)(IFCS_{name})"), &format!("{value}LL"));
        constants += 1;
    }
    assert!(
        constants > 300,
        "ifcs-sys: only {constants} IFCS_ constants checked: build.rs is stale"
    );
    // The symbol list: the preprocessor's count of IFCS_SYMBOLS, each entry one of the names read
    // here (an entry not read is an undeclared identifier), each with its typedef.
    for s in symbols {
        writeln!(c, "enum {{ ifcs_sys_seen_{s} = 1 }};").expect("write");
        writeln!(c, "typedef {s}_fn ifcs_sys_typedef_{s};").expect("write");
    }
    c.push_str("#define IFCS_SYS_ENTRY(name) + ifcs_sys_seen_##name\n");
    check(
        &mut c,
        "0 IFCS_SYMBOLS(IFCS_SYS_ENTRY)".to_owned(),
        &symbols.len().to_string(),
    );
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

fn run_abi_check(c_path: &Path, inc: &Path, n: usize) -> String {
    let required = env::var("IFCS_SYS_ABI_CHECK").is_ok_and(|v| v == "require");
    assert!(
        n > 400,
        "ifcs-sys: only {n} ABI assertions were derived from the bindings: the extraction in build.rs is stale"
    );
    let target = env::var("TARGET").expect("TARGET");
    let host = env::var("HOST").expect("HOST");
    let under = target.replace('-', "_");
    let cc = [
        "IFCS_SYS_ABI_CC".to_string(),
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
            "ifcs-sys: IFCS_SYS_ABI_CHECK=require, but no C compiler for {target} (set IFCS_SYS_ABI_CC)"
        );
        return format!("skipped: no C compiler for {target} (set IFCS_SYS_ABI_CC or CC_{under})");
    };
    let mut words = cc.split_whitespace();
    let run = Command::new(words.next().expect("compiler"))
        .args(words)
        .args(["-std=gnu11", "-fsyntax-only"])
        .arg(format!("-I{}", inc.display()))
        .arg(c_path)
        .output();
    match run {
        Ok(o) if o.status.success() => format!("checked {n} assertions with {cc} for {target}"),
        Ok(o) => panic!(
            "ifcs-sys: the Rust bindings disagree with {cc} about the IFCS ABI for {target}:\n{}",
            String::from_utf8_lossy(&o.stderr)
        ),
        Err(e) => {
            assert!(
                !required,
                "ifcs-sys: IFCS_SYS_ABI_CHECK=require, but {cc} did not run: {e}"
            );
            format!("skipped: {cc} did not run ({e})")
        }
    }
}
