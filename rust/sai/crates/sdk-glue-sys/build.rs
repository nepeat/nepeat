//! Generates the declarations (Rust and C) from one table, checks them against `sdk_glue.h` when
//! it is available, and links the real or the fake glue when a feature asks. See src/lib.rs.
// A build script reports a failure by panicking with a message.
#![allow(clippy::expect_used, clippy::panic)]

use std::env;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The `OB_E_*` codes of `sdk_glue.h`.
const CODES: &[(&str, i32)] = &[
    ("OB_E_NONE", 0),
    ("OB_E_INTERNAL", -1),
    ("OB_E_MEMORY", -2),
    ("OB_E_UNIT", -3),
    ("OB_E_PARAM", -4),
    ("OB_E_EMPTY", -5),
    ("OB_E_FULL", -6),
    ("OB_E_NOT_FOUND", -7),
    ("OB_E_EXISTS", -8),
    ("OB_E_TIMEOUT", -9),
    ("OB_E_BUSY", -10),
    ("OB_E_FAIL", -11),
    ("OB_E_DISABLED", -12),
    ("OB_E_BADID", -13),
    ("OB_E_RESOURCE", -14),
    ("OB_E_CONFIG", -15),
    ("OB_E_UNAVAIL", -16),
    ("OB_E_INIT", -17),
    ("OB_E_PORT", -18),
    ("OB_E_MDIO_CLOCK", -100),
];

struct Func {
    name: &'static str,
    ret: &'static str,
    args: &'static [(&'static str, &'static str)],
}

/// The functions bound so far, with the C types of the header. To bind another one, add a row;
/// to use a C type not in `rust_type()`, add it there.
const FUNCS: &[Func] = &[
    Func {
        name: "ob_sdk_init_mdio",
        ret: "int",
        args: &[
            ("unit", "int"),
            ("config_file", "const char*"),
            ("err", "char*"),
            ("errlen", "int"),
        ],
    },
    Func {
        name: "ob_sdk_mdio_c45_read",
        ret: "int",
        args: &[
            ("unit", "int"),
            ("phy", "unsigned"),
            ("dev", "unsigned"),
            ("reg", "unsigned"),
            ("val", "unsigned*"),
        ],
    },
    Func {
        name: "ob_sdk_mdio_c45_write",
        ret: "int",
        args: &[
            ("unit", "int"),
            ("phy", "unsigned"),
            ("dev", "unsigned"),
            ("reg", "unsigned"),
            ("val", "unsigned"),
        ],
    },
    Func {
        name: "ob_sdk_temperatures",
        ret: "int",
        args: &[
            ("unit", "int"),
            ("cur", "int*"),
            ("peak", "int*"),
            ("max", "int"),
            ("count", "int*"),
        ],
    },
    Func {
        name: "ob_sdk_shutdown",
        ret: "void",
        args: &[("unit", "int")],
    },
    Func {
        name: "ob_sdk_errmsg",
        ret: "const char*",
        args: &[("rv", "int")],
    },
    // The full switch bring-up and what a SAI backend needs (sdk-glue: switch, port, vlan, link).
    Func {
        name: "ob_sdk_init",
        ret: "int",
        args: &[
            ("unit", "int"),
            ("config_file", "const char*"),
            ("rc_file", "const char*"),
            ("err", "char*"),
            ("errlen", "int"),
        ],
    },
    Func {
        name: "ob_sdk_register_callbacks",
        ret: "int",
        args: &[
            ("unit", "int"),
            ("link", "ob_link_cb_t"),
            ("l2", "ob_l2_cb_t"),
            ("ctx", "void*"),
        ],
    },
    Func {
        name: "ob_sdk_unregister_callbacks",
        ret: "void",
        args: &[("unit", "int")],
    },
    Func {
        name: "ob_sdk_hw_info",
        ret: "int",
        args: &[("unit", "int"), ("buf", "char*"), ("len", "int")],
    },
    Func {
        name: "ob_sdk_cpu_port",
        ret: "int",
        args: &[("unit", "int")],
    },
    Func {
        name: "ob_sdk_ports",
        ret: "int",
        args: &[("unit", "int"), ("out", "ob_port_info_t*"), ("max", "int")],
    },
    Func {
        name: "ob_sdk_port_enable_set",
        ret: "int",
        args: &[("unit", "int"), ("port", "int"), ("enable", "int")],
    },
    Func {
        name: "ob_sdk_port_link_get",
        ret: "int",
        args: &[("unit", "int"), ("port", "int"), ("up", "int*")],
    },
    Func {
        name: "ob_sdk_port_speed_set",
        ret: "int",
        args: &[("unit", "int"), ("port", "int"), ("mbps", "int")],
    },
    Func {
        name: "ob_sdk_port_speed_get",
        ret: "int",
        args: &[("unit", "int"), ("port", "int"), ("mbps", "int*")],
    },
    Func {
        name: "ob_sdk_port_speeds",
        ret: "int",
        args: &[("unit", "int"), ("port", "int"), ("out", "uint32_t*"), ("max", "int")],
    },
    Func {
        name: "ob_sdk_port_fec_set",
        ret: "int",
        args: &[("unit", "int"), ("port", "int"), ("mode", "int")],
    },
    Func {
        name: "ob_sdk_port_autoneg_set",
        ret: "int",
        args: &[("unit", "int"), ("port", "int"), ("on", "int")],
    },
    Func {
        name: "ob_sdk_port_interface_set",
        ret: "int",
        args: &[("unit", "int"), ("port", "int"), ("name", "const char*")],
    },
    Func {
        name: "ob_sdk_port_ifg_set",
        ret: "int",
        args: &[("unit", "int"), ("port", "int"), ("bits", "int")],
    },
    Func {
        name: "ob_sdk_vlan_create",
        ret: "int",
        args: &[("unit", "int"), ("vid", "int")],
    },
    Func {
        name: "ob_sdk_vlan_destroy",
        ret: "int",
        args: &[("unit", "int"), ("vid", "int")],
    },
    Func {
        name: "ob_sdk_vlan_port_add",
        ret: "int",
        args: &[("unit", "int"), ("vid", "int"), ("port", "int"), ("untagged", "int")],
    },
    Func {
        name: "ob_sdk_vlan_port_remove",
        ret: "int",
        args: &[("unit", "int"), ("vid", "int"), ("port", "int")],
    },
    Func {
        name: "ob_sdk_vlan_members",
        ret: "int",
        args: &[
            ("unit", "int"),
            ("vid", "int"),
            ("ports", "int*"),
            ("untagged", "int*"),
            ("max", "int"),
        ],
    },
];

/// The types of `sdk_glue.h` that cross the interface besides integers and strings: one struct
/// of four ints and a name, and the two callback types. Rust side; the layout numbers are
/// asserted here and, with the same numbers, against the header by the C compiler
/// (`TYPES_CHECK`), so a header that changes the struct stops the build on both sides.
const TYPES_RS: &str = r#"#[repr(C)]
#[derive(Clone, Copy, Debug)]
#[allow(non_camel_case_types)]
pub struct ob_port_info_t {
    pub port: ::core::ffi::c_int,
    pub phys: ::core::ffi::c_int,
    pub lanes: ::core::ffi::c_int,
    pub speed: ::core::ffi::c_int,
    pub name: [::core::ffi::c_char; 16],
}
const _: () = assert!(::core::mem::size_of::<ob_port_info_t>() == 32);
const _: () = assert!(::core::mem::align_of::<ob_port_info_t>() == 4);
const _: () = assert!(::core::mem::offset_of!(ob_port_info_t, port) == 0);
const _: () = assert!(::core::mem::offset_of!(ob_port_info_t, phys) == 4);
const _: () = assert!(::core::mem::offset_of!(ob_port_info_t, lanes) == 8);
const _: () = assert!(::core::mem::offset_of!(ob_port_info_t, speed) == 12);
const _: () = assert!(::core::mem::offset_of!(ob_port_info_t, name) == 16);
#[allow(non_camel_case_types)]
pub type ob_link_cb_t =
    ::core::option::Option<unsafe extern "C" fn(ctx: *mut ::core::ffi::c_void, port: ::core::ffi::c_int, up: ::core::ffi::c_int)>;
#[allow(non_camel_case_types)]
pub type ob_l2_cb_t = ::core::option::Option<
    unsafe extern "C" fn(
        ctx: *mut ::core::ffi::c_void,
        mac: *const u8,
        vid: ::core::ffi::c_int,
        port: ::core::ffi::c_int,
        learned: ::core::ffi::c_int,
    ),
>;
"#;

/// The same types for the C declarations when `sdk_glue.h` is not included (the fake glue).
const TYPES_C: &str = r"#include <stdint.h>
#ifndef SAI_OPENBCM_SDK_GLUE_H
typedef struct {
    int port;
    int phys;
    int lanes;
    int speed;
    char name[16];
} ob_port_info_t;
typedef void (*ob_link_cb_t)(void* ctx, int port, int up);
typedef void (*ob_l2_cb_t)(void* ctx, const uint8_t mac[6], int vid, int port, int learned);
#endif
";

/// The header's struct against the numbers `TYPES_RS` asserts, and its field types.
const TYPES_CHECK: &str = r#"#include <stddef.h>
_Static_assert(sizeof(ob_port_info_t) == 32, "ob_port_info_t size");
_Static_assert(_Alignof(ob_port_info_t) == 4, "ob_port_info_t alignment");
_Static_assert(offsetof(ob_port_info_t, port) == 0, "ob_port_info_t.port");
_Static_assert(offsetof(ob_port_info_t, phys) == 4, "ob_port_info_t.phys");
_Static_assert(offsetof(ob_port_info_t, lanes) == 8, "ob_port_info_t.lanes");
_Static_assert(offsetof(ob_port_info_t, speed) == 12, "ob_port_info_t.speed");
_Static_assert(offsetof(ob_port_info_t, name) == 16, "ob_port_info_t.name");
_Static_assert(sizeof(int) == 4 && sizeof(uint32_t) == sizeof(unsigned), "int is 32 bits");
_Static_assert(__builtin_types_compatible_p(__typeof__(((ob_port_info_t*)0)->port), int), "port: int");
_Static_assert(__builtin_types_compatible_p(__typeof__(((ob_port_info_t*)0)->phys), int), "phys: int");
_Static_assert(__builtin_types_compatible_p(__typeof__(((ob_port_info_t*)0)->lanes), int), "lanes: int");
_Static_assert(__builtin_types_compatible_p(__typeof__(((ob_port_info_t*)0)->speed), int), "speed: int");
_Static_assert(__builtin_types_compatible_p(__typeof__(((ob_port_info_t*)0)->name), char[16]), "name: char[16]");
_Static_assert(__builtin_types_compatible_p(ob_link_cb_t, void (*)(void*, int, int)), "ob_link_cb_t");
_Static_assert(__builtin_types_compatible_p(ob_l2_cb_t, void (*)(void*, const uint8_t*, int, int, int)), "ob_l2_cb_t");
"#;

/// Test hooks of `fake/fake_glue.c` (not in `sdk_glue.h`): declared only when the fake is linked.
const FAKE_HOOKS_RS: &str = r#"unsafe extern "C" {
    pub fn ob_fake_link_event(port: ::core::ffi::c_int, up: ::core::ffi::c_int) -> ::core::ffi::c_int;
    pub fn ob_fake_query(what: ::core::ffi::c_int, arg: ::core::ffi::c_int) -> ::core::ffi::c_int;
}
"#;

/// The Rust spelling of a C type of the table ("" for a void return).
fn rust_type(c: &str) -> &'static str {
    match c {
        "void" => "",
        "int" => "::core::ffi::c_int",
        "unsigned" => "::core::ffi::c_uint",
        "int*" => "*mut ::core::ffi::c_int",
        "unsigned*" => "*mut ::core::ffi::c_uint",
        "char*" => "*mut ::core::ffi::c_char",
        "const char*" => "*const ::core::ffi::c_char",
        "uint32_t*" => "*mut u32",
        "void*" => "*mut ::core::ffi::c_void",
        "ob_port_info_t*" => "*mut ob_port_info_t",
        "ob_link_cb_t" => "ob_link_cb_t",
        "ob_l2_cb_t" => "ob_l2_cb_t",
        other => panic!("sdk-glue-sys: build.rs has no Rust type for the C type '{other}'"),
    }
}

fn rust_bindings() -> String {
    let mut s = String::from("// Generated by build.rs from its table; checked against sdk_glue.h.\n");
    s.push_str(TYPES_RS);
    if env::var_os("CARGO_FEATURE_FAKE").is_some() {
        s.push_str(FAKE_HOOKS_RS);
    }
    for (name, value) in CODES {
        writeln!(s, "pub const {name}: ::core::ffi::c_int = {value};").expect("write");
    }
    s.push_str("unsafe extern \"C\" {\n");
    for f in FUNCS {
        let args: Vec<String> = f.args.iter().map(|(n, t)| format!("{n}: {}", rust_type(t))).collect();
        let ret = match rust_type(f.ret) {
            "" => String::new(),
            t => format!(" -> {t}"),
        };
        writeln!(s, "    pub fn {}({}){ret};", f.name, args.join(", ")).expect("write");
    }
    s.push_str("}\n");
    s
}

/// The same declarations as a C header (what `fake_glue.c` is compiled against).
fn c_header() -> String {
    let mut s = String::from(
        "/* Generated by sdk-glue-sys/build.rs from its table. */\n#ifndef OB_SDK_GLUE_DECLS_H\n#define OB_SDK_GLUE_DECLS_H\n",
    );
    s.push_str(TYPES_C);
    for (name, value) in CODES {
        writeln!(s, "#ifndef {name}\n#define {name} ({value})\n#endif").expect("write");
    }
    for f in FUNCS {
        let args: Vec<String> = f.args.iter().map(|(n, t)| format!("{t} {n}")).collect();
        writeln!(s, "{} {}({});", f.ret, f.name, args.join(", ")).expect("write");
    }
    s.push_str("#endif\n");
    s
}

/// `sdk_glue.h` first, then our prototypes (an incompatible redeclaration is an error) and values.
fn c_check() -> String {
    let mut s = String::from("#include \"sdk_glue.h\"\n#include \"ob_sdk_glue_decls.h\"\n");
    s.push_str(TYPES_CHECK);
    for (name, value) in CODES {
        writeln!(s, "_Static_assert(({name}) == ({value}), \"{name}\");").expect("write");
    }
    s
}

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

fn header_dir() -> Option<PathBuf> {
    for v in ["SDK_GLUE_INC", "MDIO_SCRIPT_C_DIR"] {
        println!("cargo:rerun-if-env-changed={v}");
        if let Some(dir) = env::var_os(v).map(PathBuf::from) {
            let header = dir.join("sdk_glue.h");
            if header.is_file() {
                println!("cargo:rerun-if-changed={}", header.display());
                return Some(dir);
            }
            assert!(
                v != "SDK_GLUE_INC",
                "sdk-glue-sys: no sdk_glue.h in SDK_GLUE_INC={}",
                dir.display()
            );
        }
    }
    None
}

fn header_check(out: &Path, cc: Option<&str>, target: &str) -> String {
    println!("cargo:rerun-if-env-changed=SDK_GLUE_SYS_CHECK");
    let required = env::var("SDK_GLUE_SYS_CHECK").is_ok_and(|v| v == "require");
    let skipped = |why: String| {
        assert!(!required, "sdk-glue-sys: SDK_GLUE_SYS_CHECK=require, but {why}");
        format!("skipped: {why}")
    };
    let Some(inc) = header_dir() else {
        return skipped("no sdk_glue.h (set SDK_GLUE_INC to sai-openbcm's src directory)".into());
    };
    let Some(cc) = cc else {
        return skipped(format!("no C compiler for {target} (set SDK_GLUE_SYS_CC)"));
    };
    let check = out.join("check.c");
    fs::write(&check, c_check()).expect("write check.c");
    let run = command(cc)
        .args(["-std=gnu11", "-fsyntax-only", "-Wall", "-Werror", "-I"])
        .arg(&inc)
        .arg("-I")
        .arg(out.join("include"))
        .arg(&check)
        .output();
    match run {
        Ok(o) if o.status.success() => format!(
            "checked {} declarations with {cc} for {target} against {}",
            CODES.len() + FUNCS.len(),
            inc.join("sdk_glue.h").display()
        ),
        Ok(o) => panic!(
            "sdk-glue-sys: the declarations in build.rs disagree with {}:\n{}",
            inc.join("sdk_glue.h").display(),
            String::from_utf8_lossy(&o.stderr)
        ),
        Err(e) => skipped(format!("{cc} did not run ({e})")),
    }
}

fn link_fake(out: &Path, cc: Option<&str>, target: &str, host: &str) {
    let cc = cc.unwrap_or_else(|| {
        panic!("sdk-glue-sys: feature 'fake' needs a C compiler for {target} (set SDK_GLUE_SYS_CC)")
    });
    let src = Path::new(&env::var_os("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR")).join("fake/fake_glue.c");
    println!("cargo:rerun-if-changed={}", src.display());
    let obj = out.join("fake_glue.o");
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
    let mut compile = command(cc);
    compile
        .args(["-std=gnu11", "-O1", "-g", "-fPIC", "-Wall", "-Wextra", "-I"])
        .arg(out.join("include"))
        .arg("-c")
        .arg(&src)
        .arg("-o")
        .arg(&obj);
    run(compile, cc);
    let ar = archiver(target, host, cc);
    let lib = out.join("libob_fake_glue.a");
    let _ = fs::remove_file(&lib);
    let mut archive = command(&ar);
    archive.arg("rcs").arg(&lib).arg(&obj);
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

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    let out = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR"));
    let target = env::var("TARGET").expect("TARGET");
    let host = env::var("HOST").expect("HOST");
    fs::write(out.join("bindings.rs"), rust_bindings()).expect("write bindings.rs");
    fs::create_dir_all(out.join("include")).expect("mkdir include");
    fs::write(out.join("include/ob_sdk_glue_decls.h"), c_header()).expect("write the C declarations");

    let cc = c_compiler(&target, &host);
    let result = header_check(&out, cc.as_deref(), &target);
    println!("cargo:rustc-env=SDK_GLUE_SYS_CHECK_RESULT={result}");
    // In the build log on purpose, like sai-sys: was the header consulted, by which compiler.
    println!("cargo:warning=sdk_glue.h check: {result}");

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
        link_fake(&out, cc.as_deref(), &target, &host);
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
