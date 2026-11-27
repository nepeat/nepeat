//! ifcs-gen against goldens the Python generator (`gen_ifcs_api.py`, deleted in H5 on 2026-11-27, last in the
//! tree at commit 767d9b0) wrote from the SYNTHETIC
//! fixtures (`tests/fixtures`: our own ctypesgen-shaped file and templates, no vendor content),
//! negative inputs, and properties. The comparison on the real input (ishell 2.0.1) is `ishell_golden.rs`
//! (the input is not in the repository). The templates in `templates/` are the only copy now.

use ifcs_gen::{Fatal, Generated, Input, generate};
use proptest::prelude::*;

const MINI: &str = include_str!("fixtures/mini_ctypes.py");
const MINI_T: &str = include_str!("fixtures/mini_api.h.in");
const MINI_OV: &str = include_str!("fixtures/mini_overrides.txt");
const BAD_T: &str = include_str!("fixtures/bad_api.h.in");
const BAD_OV: &str = include_str!("fixtures/bad_overrides.txt");

fn run(ctypes: &str, ov: Option<&str>, source: Option<&str>, tmpl: &[(&str, &str)]) -> Result<Generated, Fatal> {
    generate(&Input {
        ctypes_path: "mini_ctypes.py",
        ctypes: ctypes.as_bytes(),
        overrides: ov.map(|t| ("overrides.txt", t)),
        source,
        templates: tmpl,
    })
}

#[allow(clippy::expect_used, reason = "a test helper")]
fn errors(g: &Result<Generated, Fatal>) -> Vec<String> {
    g.as_ref().expect("no exception").errors.clone()
}

#[test]
fn mini_equals_the_python_golden() {
    let g = run(
        MINI,
        Some(MINI_OV),
        Some("the test"),
        &[("fixtures/mini_api.h.in", MINI_T)],
    )
    .expect("generate");
    assert_eq!(g.errors, Vec::<String>::new());
    assert_eq!(g.outputs.len(), 1);
    assert_eq!(g.outputs[0].0, "mini_api.h");
    assert_eq!(g.outputs[0].1, include_str!("golden/mini_api.h"));
}

#[test]
fn collected_errors_equal_the_python_ones() {
    let g = run(MINI, Some(BAD_OV), None, &[("bad_api.h.in", BAD_T)]).expect("generate");
    let want: Vec<&str> = include_str!("golden/bad_api.errors.txt").lines().collect();
    assert_eq!(g.errors, want);
    // --keep-going writes this text
    assert_eq!(g.outputs[0].1, include_str!("golden/bad_api.h"));
}

#[test]
fn missing_name() {
    let g = run(
        MINI,
        None,
        None,
        &[("t.h.in", "enum { MINI_NOT_THERE = @ };\n@fn mini_absent\n")],
    );
    assert_eq!(
        errors(&g),
        [
            "no integer value for MINI_NOT_THERE in mini_ctypes.py",
            "function mini_absent not in mini_ctypes.py"
        ]
    );
    assert_eq!(
        g.expect("ok").outputs[0].1.lines().nth(2),
        Some("enum { MINI_NOT_THERE = 0 };")
    );
}

#[test]
fn override_of_an_unknown_symbol() {
    let ov = "MINI_NO_SUCH 3   # verified nowhere\n";
    assert_eq!(
        errors(&run(MINI, Some(ov), None, &[("t.h.in", "x\n")])),
        ["overrides not used: MINI_NO_SUCH"]
    );
}

#[test]
fn malformed_overrides() {
    let t = [("t.h.in", "enum { MINI_OK = @ };\n")];
    assert!(matches!(run(MINI, Some("MINI_OK 1\n"), None, &t), Err(Fatal::Exit(m)) if m.contains("has no comment")));
    assert!(matches!(
        run(MINI, Some("MINI_OK 1 2  # x\n"), None, &t),
        Err(Fatal::Value(_))
    ));
    assert!(matches!(
        run(MINI, Some("MINI_OK 012  # x\n"), None, &t),
        Err(Fatal::Value(_))
    ));
    let g = run(
        MINI,
        Some("# only comments\n\n   # x\nMINI_OK 0b11 # binary\r\n"),
        None,
        &t,
    )
    .expect("ok");
    assert_eq!(g.errors, Vec::<String>::new());
    assert!(g.outputs[0].1.contains("MINI_OK = 3"));
}

#[test]
fn malformed_ctypes() {
    let t = [("t.h.in", "x\n")];
    // a field type that is not Python: the Python model raises SyntaxError
    let bad_field =
        format!("{MINI}\nclass struct_q(Structure):\n    pass\nstruct_q._fields_ = [\n    ('a', c_int +),\n]\n");
    assert!(matches!(run(&bad_field, None, None, &t), Err(Fatal::Syntax(_))));
    let bad_args = format!("{MINI}\n    f.argtypes = [c_int, , c_int]\n");
    assert!(matches!(run(&bad_args, None, None, &t), Err(Fatal::Syntax(_))));
    // an assignment that is not Python is skipped (mini_unparsable in the fixture), as by the Python
    let g = run(MINI, None, None, &[("t.h.in", "enum { mini_unparsable = @ };\n")]);
    assert_eq!(errors(&g), ["no integer value for mini_unparsable in mini_ctypes.py"]);
    // no library load line: the whole file is preamble, nothing is known
    let g = run("MINI_OK = 0\n", None, None, &[("t.h.in", "enum { MINI_OK = @ };\n")]);
    assert_eq!(errors(&g), ["no integer value for MINI_OK in mini_ctypes.py"]);
    // not UTF-8: replaced, not refused
    let mut bytes = MINI.as_bytes().to_vec();
    bytes.extend_from_slice(b"\nMINI_X = 1# \xff\xfe\n");
    let g = generate(&Input {
        ctypes_path: "m.py",
        ctypes: &bytes,
        overrides: None,
        source: None,
        templates: &[("t.h.in", "enum { MINI_X = @ };\n")],
    })
    .expect("generate");
    assert_eq!(g.errors, Vec::<String>::new());
}

#[test]
fn exceptions_of_the_reference() {
    let one = |t: &str| run(MINI, None, None, &[("t.h.in", t)]);
    assert!(matches!(one("@union  /* no name */\n"), Err(Fatal::Index(_))));
    assert!(matches!(one("@prefix\t\n"), Err(Fatal::Index(_))));
    assert!(matches!(
        one("#define MINI_MASK_X @{MINI_MASK:.3}\n"),
        Err(Fatal::Value(_))
    ));
    let loop_ = format!("{MINI}\nmini_loop_a = mini_loop_b\nmini_loop_b = mini_loop_a\n");
    let g = run(&loop_, None, None, &[("t.h.in", "@typedef mini_loop_a\n")]);
    assert!(matches!(g, Err(Fatal::Value(_) | Fatal::Recursion(_))), "{g:?}");
    let unsupported = format!("{MINI}\nmini_weird_t = c_int + 1\n");
    assert!(matches!(
        run(&unsupported, None, None, &[("t.h.in", "@typedef mini_weird_t\n")]),
        Err(Fatal::Value(_))
    ));
    let shift = format!("{MINI}\nMINI_SHIFT = (1 << (-1))\n");
    assert!(matches!(
        run(&shift, None, None, &[("t.h.in", "enum { MINI_SHIFT = @ };\n")]),
        Err(Fatal::Value(_))
    ));
}

#[test]
fn provenance_line() {
    let g = run(MINI, None, Some(""), &[("dir/x.h", "y")]).expect("ok");
    assert_eq!(g.outputs[0].0, "x.h");
    let head = g.outputs[0].1.lines().next().expect("line");
    assert!(
        head.starts_with("/* GENERATED by tools/gen_ifcs_api.py from x.h + mini_ctypes.py (sha256 "),
        "{head}"
    );
    assert!(head.ends_with(").") && !head.contains(", "), "{head}");
}

proptest! {
    /// Any line through any template position: an error, an exception or text, never a panic.
    #[test]
    fn never_panics(line in "[ -~@{}#=*/\t]{0,60}") {
        let _ = run(MINI, None, None, &[("t.h.in", &line)]);
    }

    /// A constant's value lands where the template asks for it, in every form.
    #[test]
    fn values_substitute(v in -(1i128 << 62)..(1i128 << 62), name in "MINI_P[A-Z]{1,8}") {
        let ctypes = format!("{MINI}\n{name} = ({v})\n");
        let t = format!("enum {{ {name} = @ }};\n#define {name} @\nx @{{{name}:#x}} @{{{name}}}\n");
        let g = run(&ctypes, None, None, &[("t.h.in", &t)]).expect("generate");
        prop_assert_eq!(g.errors.clone(), Vec::<String>::new());
        let hex = if v < 0 { format!("-0x{:x}", v.unsigned_abs()) } else { format!("0x{v:x}") };
        let want = format!("enum {{ {name} = {v} }};\n#define {name} {v}\nx {hex} {v}\n");
        prop_assert!(g.outputs[0].1.ends_with(&want), "{}", g.outputs[0].1);
    }
}

/// A template read with CRLF line ends gives the same text (universal newlines, as Python's read).
#[test]
fn crlf_is_lf() {
    let crlf = MINI_T.replace('\n', "\r\n");
    let a = run(MINI, Some(MINI_OV), None, &[("a.h.in", MINI_T)]).expect("lf");
    let b = run(MINI, Some(MINI_OV), None, &[("a.h.in", &crlf)]).expect("crlf");
    assert_eq!(a.outputs[0].1, b.outputs[0].1);
}
