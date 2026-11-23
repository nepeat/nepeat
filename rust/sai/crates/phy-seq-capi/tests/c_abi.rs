//! The exported functions, called the way C calls them: through the header's types (`sys`),
//! raw pointers and callbacks. Null and nonsense arguments, the table accessors against the
//! safe crate, runs with every outcome, and that no parameter byte
//! shows in a message.
//!
//! This file is test code of the C-ABI rim: it has unsafe blocks (calling `extern "C"` functions
//! with pointers is unsafe by definition), each with its reason.

#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    reason = "test code: a failed unwrap is a failed test (clippy.toml exempts only #[test] functions, not their helpers)"
)]

// the generators of table texts of the phy-seq tests
#[path = "../../phy-seq/tests/common/mod.rs"]
mod common;

use std::ffi::{CStr, c_char, c_void};

use phy_seq::{ExtPhyTable, MacsecTable, Section};
use phy_seq_capi::sys;
use phy_seq_capi::{
    phy_seq_abi, phy_seq_exec, phy_seq_free, phy_seq_parse, phy_seq_table_info, phy_seq_table_port, phy_seq_table_seq,
};

const OK: i32 = 0;
const MACSEC: u32 = sys::PHY_SEQ_FORMAT_EXT_MACSEC;
const PHY: u32 = sys::PHY_SEQ_FORMAT_EXT_PHY;

fn rc(constant: u32) -> i32 {
    i32::try_from(constant).unwrap()
}

/// A parsed table that frees itself.
struct Handle(*mut sys::phy_seq_table);

impl Drop for Handle {
    fn drop(&mut self) {
        // SAFETY: the handle came from phy_seq_parse and is freed once, here.
        unsafe { phy_seq_free(self.0) };
    }
}

fn parse(format: u32, text: &[u8]) -> Result<Handle, (i32, String)> {
    let mut table = std::ptr::null_mut();
    let mut err = [0x55 as c_char; 200];
    // SAFETY: text is len bytes, table one pointer, err 200 bytes; all live for the call.
    let code = unsafe {
        phy_seq_parse(
            format,
            text.as_ptr(),
            text.len(),
            &raw mut table,
            err.as_mut_ptr(),
            err.len(),
        )
    };
    // SAFETY: phy_seq_parse always leaves a NUL-terminated string in err.
    let message = unsafe { CStr::from_ptr(err.as_ptr()) }.to_string_lossy().into_owned();
    if code == OK {
        assert!(!table.is_null() && message.is_empty());
        Ok(Handle(table))
    } else {
        assert!(table.is_null());
        Err((code, message))
    }
}

fn info(t: &Handle) -> sys::phy_seq_info {
    let mut info = sys::phy_seq_info {
        format: 9,
        ports: 9,
        sequences: 9,
        clear_on_read: 9,
        ifg_bits: 9,
        poll_ms: 9,
        up_polls: 9,
        holdoff_ms: 9,
    };
    // SAFETY: a live handle and a live struct.
    assert_eq!(unsafe { phy_seq_table_info(t.0, &raw mut info) }, OK);
    info
}

/// Sequence `index`: its name, operations and result names, copied out.
fn seq(t: &Handle, index: u32) -> Option<(String, Vec<sys::phy_seq_op>, Vec<u8>)> {
    let mut view = sys::phy_seq_seq {
        name: std::ptr::null(),
        name_len: 0,
        ops: std::ptr::null(),
        n_ops: 0,
        names: std::ptr::null(),
        names_len: 0,
    };
    // SAFETY: a live handle and a live struct.
    if unsafe { phy_seq_table_seq(t.0, index, &raw mut view) } != OK {
        return None;
    }
    // SAFETY: the view points into the table, which lives as long as t.
    let name = unsafe { std::slice::from_raw_parts(view.name.cast::<u8>(), view.name_len) };
    // SAFETY: as above.
    let ops = unsafe { std::slice::from_raw_parts(view.ops, view.n_ops) };
    // SAFETY: as above.
    let names = unsafe { std::slice::from_raw_parts(view.names.cast::<u8>(), view.names_len) };
    Some((String::from_utf8(name.to_vec()).unwrap(), ops.to_vec(), names.to_vec()))
}

/// What the callbacks see and answer.
#[derive(Default)]
struct Ctx {
    log: Vec<String>,
    reads: Vec<u16>,
    fail_at: Option<usize>,
    accesses: usize,
    results: Vec<(String, u32, u16)>,
}

impl Ctx {
    fn access(&mut self) -> i32 {
        self.accesses += 1;
        if self.fail_at == Some(self.accesses - 1) { -7 } else { 0 }
    }
}

/// The `Ctx` behind a callback's `ctx` pointer.
fn ctx_of<'a>(ctx: *mut c_void) -> &'a mut Ctx {
    // SAFETY: every run below passes a pointer to a live, exclusively borrowed Ctx as ctx.
    unsafe { &mut *ctx.cast::<Ctx>() }
}

extern "C" fn cb_read(ctx: *mut c_void, op: u32, phy: u16, dev: u8, reg: u16, value: *mut u16) -> i32 {
    let ctx = ctx_of(ctx);
    let status = ctx.access();
    if status != 0 {
        return status;
    }
    ctx.log.push(format!("{op}:R {phy:x} {dev:x}.{reg:04x}"));
    let v = if ctx.reads.is_empty() { 0 } else { ctx.reads.remove(0) };
    // SAFETY: the library passes a pointer to a live u16.
    unsafe { value.write(v) };
    0
}

extern "C" fn cb_write(ctx: *mut c_void, op: u32, phy: u16, dev: u8, reg: u16, value: u16) -> i32 {
    let ctx = ctx_of(ctx);
    let status = ctx.access();
    if status == 0 {
        ctx.log.push(format!("{op}:W {phy:x} {dev:x}.{reg:04x} {value:04x}"));
    }
    status
}

extern "C" fn cb_delay(ctx: *mut c_void, op: u32, usec: u32) {
    ctx_of(ctx).log.push(format!("{op}:D {usec}"));
}

extern "C" fn cb_result(ctx: *mut c_void, name: *const c_char, name_len: usize, word: u32, value: u16) {
    // SAFETY: the library passes name_len bytes that live for the call.
    let name = unsafe { std::slice::from_raw_parts(name.cast::<u8>(), name_len) };
    ctx_of(ctx)
        .results
        .push((String::from_utf8(name.to_vec()).unwrap(), word, value));
}

struct Ran {
    code: i32,
    outcome: sys::phy_seq_outcome,
    why: String,
}

fn exec(format: u32, ops: &[sys::phy_seq_op], names: &[u8], phy: u16, params: &[(u32, &[u8])], ctx: &mut Ctx) -> Ran {
    let params: Vec<sys::phy_seq_param> = params
        .iter()
        .map(|&(id, bytes)| sys::phy_seq_param {
            bytes: bytes.as_ptr(),
            len: bytes.len(),
            param: u8::try_from(id).unwrap(),
        })
        .collect();
    let io = sys::phy_seq_io {
        ctx: std::ptr::from_mut(ctx).cast::<c_void>(),
        read: Some(cb_read),
        write: Some(cb_write),
        delay: Some(cb_delay),
        result: Some(cb_result),
    };
    let mut outcome = sys::phy_seq_outcome {
        bus_status: 99,
        cond: 99,
        last: 99,
    };
    let mut why = [0x55 as c_char; 200];
    // SAFETY: every pointer is to a live slice or struct of this function with the length passed;
    // the callbacks are the ones above and ctx is the exclusive borrow they expect.
    let code = unsafe {
        phy_seq_exec(
            format,
            ops.as_ptr(),
            ops.len(),
            names.as_ptr().cast::<c_char>(),
            names.len(),
            phy,
            params.as_ptr(),
            params.len(),
            &raw const io,
            &raw mut outcome,
            why.as_mut_ptr(),
            why.len(),
        )
    };
    // SAFETY: phy_seq_exec always leaves a NUL-terminated string in why.
    let why = unsafe { CStr::from_ptr(why.as_ptr()) }.to_string_lossy().into_owned();
    Ran { code, outcome, why }
}

const TABLE: &[u8] = b"ext-macsec 1\ncounters absolute\nifg-bits 384\nport 5 11\nport 1 1e0\n\
    seq s\nR p 1 900a\nW p 1 a000 key[1].key[0]\nW 22 1 a001 00.sci[7]\nX p 1 9005 $3\nX p 1 9005 $4\n\
    W p 1 9005 $3&7fff\nW p 1 9005 $4|8002\nD 7\nE p 1 a100 2 f\nP p 1 8221 12e3 ffff 3 5\nX p 1 a101 pn[0]\nX p 1 a102 pn[2]\n\
    seq empty\nseq a\nX p 1 1 bypass[0]\n";

#[test]
fn the_abi_number() {
    assert_eq!(phy_seq_abi(), sys::PHY_SEQ_ABI);
}

#[test]
fn a_table_through_the_accessors_is_the_table_of_the_safe_crate() {
    let t = parse(MACSEC, TABLE).unwrap();
    let i = info(&t);
    assert_eq!(
        (i.format, i.ports, i.sequences, i.clear_on_read, i.ifg_bits),
        (MACSEC, 2, 3, 0, 384)
    );
    let port = |index| {
        let mut port = sys::phy_seq_port {
            lane: 9,
            phy: 9,
            line: 9,
        };
        // SAFETY: a live handle and a live struct.
        (
            unsafe { phy_seq_table_port(t.0, index, &raw mut port) },
            port.lane,
            port.phy,
        )
    };
    assert_eq!([port(0), port(1)], [(OK, 1, 0x1e0), (OK, 5, 0x11)]);
    assert_eq!(port(2).0, rc(sys::PHY_SEQ_E_ARG));
    let safe = MacsecTable::parse(TABLE).unwrap();
    for (index, (name, sequence)) in (0u32..).zip(safe.sequences()) {
        let (got_name, ops, names) = seq(&t, index).unwrap();
        assert_eq!(got_name, name);
        assert_eq!(ops.len(), sequence.ops().len());
        for (flat, op) in ops.iter().zip(sequence.ops()) {
            assert_eq!((flat.line, char::from(flat.op)), (op.line, op.letter().as_char()));
        }
        assert_eq!(
            names,
            if name == "s" {
                &b"pnpn"[..]
            } else if name == "a" {
                &b"bypass"[..]
            } else {
                &b""[..]
            }
        );
    }
    assert!(seq(&t, 3).is_none());

    let text =
        b"ext-phy 1\npoll-ms 7\nup-polls 3\nholdoff-ms 9\nport 4\nadmin-up\nW 1 1 2 3\nport 2\nlink\nC 1 1f 2 3 3\n";
    let t = parse(PHY, text).unwrap();
    let i = info(&t);
    assert_eq!(
        (i.format, i.ports, i.sequences, i.poll_ms, i.up_polls, i.holdoff_ms),
        (PHY, 2, 6, 7, 3, 9)
    );
    let safe = ExtPhyTable::parse(text).unwrap();
    for (p, port) in (0u32..).zip(safe.ports()) {
        for (s, section) in (0u32..).zip(Section::ALL) {
            let (name, ops, _) = seq(&t, 3 * p + s).unwrap();
            assert_eq!((name.as_str(), ops.len()), (section.keyword(), port.ops(section).len()));
        }
    }
    let mut port = sys::phy_seq_port {
        lane: 9,
        phy: 9,
        line: 9,
    };
    // SAFETY: a live handle and a live struct.
    assert_eq!(unsafe { phy_seq_table_port(t.0, 1, &raw mut port) }, OK);
    assert_eq!((port.lane, port.line), (2, 8));
}

#[test]
fn parse_errors_carry_the_parsers_message() {
    let (code, message) = parse(MACSEC, b"ext-macsec 1\nseq a\nW p 0 a000 6\n").err().unwrap();
    assert_eq!(
        (code, message.as_str()),
        (rc(sys::PHY_SEQ_E_PARSE), "line 3: dev 0 (clause 22) is not allowed")
    );
    let (code, message) = parse(PHY, b"ext-macsec 1\n").err().unwrap();
    assert_eq!(
        (code, message.as_str()),
        (
            rc(sys::PHY_SEQ_E_PARSE),
            "line 1: not an ext-phy table (first line must be 'ext-phy 1')"
        )
    );
    let (code, message) = parse(MACSEC, b"").err().unwrap();
    assert_eq!(
        (code, message.as_str()),
        (
            rc(sys::PHY_SEQ_E_PARSE),
            "line 1: empty table (first line must be 'ext-macsec 1')"
        )
    );
    // a message is cut at a NUL byte it repeats from the table, and to the buffer
    let (_, message) = parse(MACSEC, b"ext-macsec 1\nab\0cd\n").err().unwrap();
    assert_eq!(message, "line 2: unknown keyword 'ab");
    let long = format!("ext-macsec 1\n{}\n", "k".repeat(500));
    let (_, message) = parse(MACSEC, long.as_bytes()).err().unwrap();
    assert_eq!(message.len(), 199);
    assert_eq!(parse(7, b"ext-macsec 1\n").err().unwrap().0, rc(sys::PHY_SEQ_E_ARG));
}

#[test]
fn null_pointers_are_refused_not_followed() {
    let arg = rc(sys::PHY_SEQ_E_ARG);
    let mut table = std::ptr::null_mut();
    // SAFETY: null pointers are what this test is about; the library must refuse them unread.
    unsafe {
        assert_eq!(
            phy_seq_parse(MACSEC, std::ptr::null(), 5, &raw mut table, std::ptr::null_mut(), 0),
            arg
        );
    }
    assert!(table.is_null());
    // SAFETY: as above: a null output pointer.
    let code = unsafe { phy_seq_parse(MACSEC, b"x".as_ptr(), 1, std::ptr::null_mut(), std::ptr::null_mut(), 0) };
    assert_eq!(code, arg);
    // an empty text needs no pointer, and no error buffer is needed either
    // SAFETY: len 0 and cap 0: neither pointer is used.
    let code = unsafe { phy_seq_parse(MACSEC, std::ptr::null(), 0, &raw mut table, std::ptr::null_mut(), 0) };
    assert_eq!(code, rc(sys::PHY_SEQ_E_PARSE));
    // SAFETY: freeing null is allowed.
    unsafe { phy_seq_free(std::ptr::null_mut()) };
    let mut i = sys::phy_seq_info {
        format: 0,
        ports: 0,
        sequences: 0,
        clear_on_read: 0,
        ifg_bits: 0,
        poll_ms: 0,
        up_polls: 0,
        holdoff_ms: 0,
    };
    // SAFETY: a null handle must be refused.
    assert_eq!(unsafe { phy_seq_table_info(std::ptr::null(), &raw mut i) }, arg);
    let t = parse(MACSEC, TABLE).unwrap();
    // SAFETY: null output pointers must be refused.
    unsafe {
        assert_eq!(phy_seq_table_info(t.0, std::ptr::null_mut()), arg);
    }
    // SAFETY: as above.
    assert_eq!(unsafe { phy_seq_table_port(t.0, 0, std::ptr::null_mut()) }, arg);
    // SAFETY: as above.
    assert_eq!(unsafe { phy_seq_table_seq(t.0, 0, std::ptr::null_mut()) }, arg);
    let (_, ops, names) = seq(&t, 2).unwrap();
    // SAFETY: no callbacks at all, then callbacks with a missing function: refused before any use.
    let code = unsafe {
        phy_seq_exec(
            MACSEC,
            ops.as_ptr(),
            ops.len(),
            names.as_ptr().cast(),
            names.len(),
            1,
            std::ptr::null(),
            0,
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            0,
        )
    };
    assert_eq!(code, arg);
    let io = sys::phy_seq_io {
        ctx: std::ptr::null_mut(),
        read: Some(cb_read),
        write: None,
        delay: Some(cb_delay),
        result: None,
    };
    // SAFETY: as above.
    let code = unsafe {
        phy_seq_exec(
            MACSEC,
            ops.as_ptr(),
            ops.len(),
            names.as_ptr().cast(),
            names.len(),
            1,
            std::ptr::null(),
            0,
            &raw const io,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            0,
        )
    };
    assert_eq!(code, arg);
    // SAFETY: operations announced but not given.
    let code = unsafe {
        phy_seq_exec(
            MACSEC,
            std::ptr::null(),
            3,
            std::ptr::null(),
            0,
            1,
            std::ptr::null(),
            0,
            &raw const io,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            0,
        )
    };
    assert_eq!(code, arg);
}

#[test]
fn the_golden_run_through_the_callbacks() {
    let t = parse(MACSEC, TABLE).unwrap();
    let (_, ops, names) = seq(&t, 2).unwrap();
    let key = [0x11u8, 0x22];
    let sci = [1u8, 2, 3, 4, 5, 6, 7, 8];
    let params: [(u32, &[u8]); 2] = [(sys::PHY_SEQ_PARAM_KEY, &key), (sys::PHY_SEQ_PARAM_SCI, &sci)];
    // reads in order: 900a, 9005, 9005, a100, 8221, 8221, a101, a102
    let mut ctx = Ctx {
        reads: vec![0, 0x8001, 0x0010, 0x12, 0, 0x12e3, 0xbeef, 1],
        ..Ctx::default()
    };
    let ran = exec(MACSEC, &ops, &names, 0x30, &params, &mut ctx);
    assert_eq!((ran.code, ran.why.as_str()), (OK, ""));
    assert_eq!((ran.outcome.bus_status, ran.outcome.cond, ran.outcome.last), (0, 1, 0));
    assert_eq!(
        ctx.log.join("|"),
        "0:R 30 1.900a|1:W 30 1.a000 2211|2:W 22 1.a001 0008|3:R 30 1.9005|4:R 30 1.9005|5:W 30 1.9005 0001|6:W 30 1.9005 8012|7:D 7|\
         8:R 30 1.a100|9:R 30 1.8221|9:D 5|9:R 30 1.8221|10:R 30 1.a101|11:R 30 1.a102"
    );
    assert_eq!(
        ctx.results,
        [
            ("pn".to_owned(), 0, 0xbeef),
            ("pn".to_owned(), 1, 0),
            ("pn".to_owned(), 2, 1)
        ]
    );

    // a missing parameter byte: nothing is accessed, and no result is delivered
    let short: [(u32, &[u8]); 2] = [(sys::PHY_SEQ_PARAM_KEY, &key[..1]), (sys::PHY_SEQ_PARAM_SCI, &sci)];
    let mut ctx = Ctx::default();
    let ran = exec(MACSEC, &ops, &names, 0x30, &short, &mut ctx);
    assert_eq!(
        (ran.code, ran.why.as_str()),
        (rc(sys::PHY_SEQ_E_PARAM), "line 8: parameter key[1] is not available")
    );
    assert_eq!((ctx.log.len(), ctx.results.len()), (0, 0));

    // a compare that fails: PHY_SEQ_E_FAULT, the C++ message, nothing after it
    let mut ctx = Ctx {
        reads: vec![0, 0, 0, 0x13],
        ..Ctx::default()
    };
    let ran = exec(MACSEC, &ops, &names, 0x30, &params, &mut ctx);
    assert_eq!(ran.code, rc(sys::PHY_SEQ_E_FAULT));
    assert_eq!(
        ran.why,
        "line 15: E phy 0x30 1.a100 read 0x0013, expected 0x0002 (mask 0x000f, 1 reads)"
    );
    assert_eq!(ctx.log.last().unwrap(), "8:R 30 1.a100");
    assert_eq!(ctx.results, Vec::new());

    // the bus master fails: its status comes back
    let mut ctx = Ctx {
        fail_at: Some(1),
        ..Ctx::default()
    };
    let ran = exec(MACSEC, &ops, &names, 0x30, &params, &mut ctx);
    assert_eq!((ran.code, ran.outcome.bus_status), (rc(sys::PHY_SEQ_E_BUS), -7));
    assert_eq!(ran.why, "line 8: W phy 0x30 1.a000: MDIO error -7");
    assert_eq!(ctx.log, ["0:R 30 1.900a"]);
}

#[test]
fn no_parameter_byte_in_any_message() {
    let t = parse(
        MACSEC,
        b"ext-macsec 1\nseq s\nW p 1 1 key[0].key[1]\nE p 1 2 1 1\nW p 1 3 hkey[15].salt[11]\n",
    )
    .unwrap();
    let (_, ops, names) = seq(&t, 0).unwrap();
    let key = [0xc7u8; 32];
    let all: Vec<(u32, &[u8])> = (sys::PHY_SEQ_PARAM_KEY..=sys::PHY_SEQ_PARAM_WINDOW)
        .map(|id| (id, &key[..4]))
        .collect();
    for fail_at in [None, Some(0), Some(1)] {
        let mut ctx = Ctx {
            fail_at,
            ..Ctx::default()
        };
        let ran = exec(MACSEC, &ops, &names, 0xc7c7, &all, &mut ctx);
        assert_ne!(ran.code, OK);
        // the PHY address is c7c7 on purpose: it is the only place the pattern may show
        let why = ran.why.replace("phy 0xc7c7", "");
        assert!(!why.contains("c7") && !why.contains("199"), "{}", ran.why);
    }
}

#[test]
fn operations_from_c_are_checked_before_anything_runs() {
    let t = parse(MACSEC, TABLE).unwrap();
    let (_, mut ops, names) = seq(&t, 2).unwrap();
    ops[10].dev = 0;
    let mut ctx = Ctx::default();
    let ran = exec(MACSEC, &ops, &names, 1, &[], &mut ctx);
    assert_eq!(
        (ran.code, ran.why.as_str()),
        (
            rc(sys::PHY_SEQ_E_ARG),
            "phy-seq: operation 10 (line 17): device is not 1..31"
        )
    );
    assert_eq!(ctx.log, Vec::<String>::new());
    // an ext-macsec sequence is not an ext-phy one
    ops[10].dev = 1;
    let ran = exec(PHY, &ops, &names, 1, &[], &mut ctx);
    assert_eq!(ran.code, rc(sys::PHY_SEQ_E_ARG));
    assert!(
        ran.why.starts_with("phy-seq: operation 0 (line 7): port_phy"),
        "{}",
        ran.why
    );
    let ran = exec(MACSEC, &ops, &names, 1, &[(9, &[1])], &mut ctx);
    assert_eq!(
        (ran.code, ran.why.as_str()),
        (rc(sys::PHY_SEQ_E_ARG), "phy-seq: unknown parameter")
    );
    assert_eq!(ctx.log, Vec::<String>::new());
}

#[test]
fn an_ext_phy_link_sequence_reports_condition_and_value() {
    let t = parse(
        PHY,
        b"ext-phy 1\nport 1\nlink\nP 10 1 8221 11e3 ffff 3 0\nE 10 1 a100 2\nC 10 1 a101 3 3\n",
    )
    .unwrap();
    let (name, ops, names) = seq(&t, 0).unwrap();
    assert_eq!(name, "link");
    let mut ctx = Ctx {
        reads: vec![0, 0x11e3, 2, 0xfff3],
        ..Ctx::default()
    };
    let ran = exec(PHY, &ops, &names, 0, &[], &mut ctx);
    assert_eq!((ran.code, ran.outcome.cond, ran.outcome.last), (OK, 1, 0xfff3));
    assert_eq!(
        ctx.log.join("|"),
        "0:R 10 1.8221|0:D 0|0:R 10 1.8221|1:R 10 1.a100|2:R 10 1.a101"
    );
    let mut ctx = Ctx {
        reads: vec![0x11e3, 2, 1],
        ..Ctx::default()
    };
    let ran = exec(PHY, &ops, &names, 0, &[], &mut ctx);
    assert_eq!((ran.code, ran.outcome.cond, ran.outcome.last), (OK, 0, 1));
    // the messages of sai_extphy.cpp
    let mut ctx = Ctx::default();
    let ran = exec(PHY, &ops, &names, 0, &[], &mut ctx);
    assert_eq!(ran.code, rc(sys::PHY_SEQ_E_FAULT));
    assert_eq!(
        ran.why,
        "line 4: P phy 0x10 1.8221 never read 0x11e3 (mask 0xffff) in 3 reads, last 0x0000"
    );
    let mut ctx = Ctx {
        reads: vec![0x11e3, 7],
        ..Ctx::default()
    };
    let ran = exec(PHY, &ops, &names, 0, &[], &mut ctx);
    assert_eq!(
        ran.why,
        "line 5: E phy 0x10 1.a100 read 0x0007, expected 0x0002 (mask 0xffff)"
    );
    let mut ctx = Ctx {
        fail_at: Some(0),
        ..Ctx::default()
    };
    let ran = exec(PHY, &ops, &names, 0, &[], &mut ctx);
    assert_eq!(
        (ran.code, ran.outcome.bus_status, ran.why.as_str()),
        (rc(sys::PHY_SEQ_E_BUS), -7, "line 4: P phy 0x10 1.8221: MDIO error -7")
    );
}

// ---- the C interface against the safe crate, on generated tables ----

/// The safe crate's side of the comparison: a [`phy_seq::Bus`] that behaves and logs exactly
/// as the callbacks above do.
#[derive(Default)]
struct SafeBus {
    ctx: Ctx,
    op: usize,
}

impl phy_seq::Bus for SafeBus {
    type Error = i32;
    fn begin(&mut self, index: usize, _op: &phy_seq::Op) {
        self.op = index;
    }
    fn read(&mut self, phy: u16, dev: phy_seq::Dev, reg: u16) -> Result<u16, i32> {
        let status = self.ctx.access();
        if status != 0 {
            return Err(status);
        }
        self.ctx
            .log
            .push(format!("{}:R {phy:x} {:x}.{reg:04x}", self.op, dev.get()));
        Ok(if self.ctx.reads.is_empty() {
            0
        } else {
            self.ctx.reads.remove(0)
        })
    }
    fn write(&mut self, phy: u16, dev: phy_seq::Dev, reg: u16, value: u16) -> Result<(), i32> {
        let status = self.ctx.access();
        if status != 0 {
            return Err(status);
        }
        self.ctx
            .log
            .push(format!("{}:W {phy:x} {:x}.{reg:04x} {value:04x}", self.op, dev.get()));
        Ok(())
    }
    fn delay_us(&mut self, us: u32) {
        self.ctx.log.push(format!("{}:D {us}", self.op));
    }
}

/// Every parameter with all its bytes, for both sides.
fn full_params() -> (phy_seq::Params, Vec<(u32, Vec<u8>)>) {
    let mut safe = phy_seq::Params::new();
    let mut c = Vec::new();
    for (id, name) in (sys::PHY_SEQ_PARAM_KEY..).zip(phy_seq::ParamName::ALL) {
        let bytes: Vec<u8> = (0..name.max_len())
            .map(|i| 0x10u8.wrapping_mul(u8::try_from(id).unwrap()).wrapping_add(i))
            .collect();
        safe.set(name, &bytes).unwrap();
        c.push((id, bytes));
    }
    (safe, c)
}

/// One sequence run through the C interface and through the crate: the same accesses, the
/// same results, the same return and message.
fn same_run(
    format: u32,
    flat: &[sys::phy_seq_op],
    names: &[u8],
    ops: &[phy_seq::Op],
    phy: u16,
    reads: &[u16],
    fail_at: Option<usize>,
) -> Result<(), proptest::test_runner::TestCaseError> {
    use proptest::prelude::*;
    let (safe_params, c_params) = full_params();
    let c_params: Vec<(u32, &[u8])> = c_params.iter().map(|(id, b)| (*id, b.as_slice())).collect();
    let mut ctx = Ctx {
        reads: reads.to_vec(),
        fail_at,
        ..Ctx::default()
    };
    let ran = exec(format, flat, names, phy, &c_params, &mut ctx);
    let mut bus = SafeBus {
        ctx: Ctx {
            reads: reads.to_vec(),
            fail_at,
            ..Ctx::default()
        },
        op: 0,
    };
    let safe = phy_seq::run(&mut bus, ops, phy, &safe_params);
    prop_assert_eq!(&ctx.log, &bus.ctx.log);
    match safe {
        Ok(outcome) => {
            prop_assert_eq!((ran.code, ran.why.as_str()), (OK, ""));
            prop_assert_eq!(
                (ran.outcome.cond, ran.outcome.last),
                (u32::from(outcome.cond), u32::from(outcome.last))
            );
            let mut want = Vec::new();
            for (name, words) in outcome.results.iter() {
                for (word, &value) in (0u32..).zip(words) {
                    want.push((name.as_str().to_owned(), word, value));
                }
            }
            prop_assert_eq!(&ctx.results, &want);
        }
        Err(e) => {
            let message = if format == PHY {
                e.ext_phy_message()
            } else {
                e.to_string()
            };
            let code = match e.kind {
                phy_seq::ExecErrorKind::MissingParam(_) => sys::PHY_SEQ_E_PARAM,
                phy_seq::ExecErrorKind::Bus { .. } => sys::PHY_SEQ_E_BUS,
                phy_seq::ExecErrorKind::Mismatch { .. } => sys::PHY_SEQ_E_FAULT,
            };
            prop_assert_eq!((ran.code, ran.why), (rc(code), message));
            prop_assert_eq!(ran.outcome.bus_status, if code == sys::PHY_SEQ_E_BUS { -7 } else { 0 });
            prop_assert_eq!(ctx.results.len(), 0);
        }
    }
    Ok(())
}

/// What `phy_seq_parse` leaves of a message in a buffer of 200 bytes.
fn c_message(e: &phy_seq::ParseError) -> String {
    let bytes = e.to_bytes();
    let cut = bytes.split(|&b| b == 0).next().unwrap();
    String::from_utf8_lossy(&cut[..cut.len().min(199)]).into_owned()
}

mod generated {
    use proptest::prelude::*;

    use super::*;

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(600))]

        #[test]
        fn ext_macsec_through_c_is_the_crate(
            text in common::macsec_text(),
            phy in any::<u16>(),
            reads in proptest::collection::vec(any::<u16>(), 0..12),
            fail_at in proptest::option::of(0usize..40),
        ) {
            match (MacsecTable::parse(&text), parse(MACSEC, &text)) {
                (Err(e), Err((code, message))) => prop_assert_eq!((code, message), (rc(sys::PHY_SEQ_E_PARSE), c_message(&e))),
                (Ok(table), Ok(handle)) => {
                    let i = info(&handle);
                    prop_assert_eq!(usize::try_from(i.sequences).unwrap(), table.sequences().len());
                    prop_assert_eq!(usize::try_from(i.ports).unwrap(), table.ports().len());
                    prop_assert_eq!((i.ifg_bits, i.clear_on_read), (u32::from(table.ifg_bits()), u32::from(table.counters() == phy_seq::CounterMode::ClearOnRead)));
                    for (index, (name, sequence)) in (0u32..).zip(table.sequences()) {
                        let (c_name, flat, names) = seq(&handle, index).unwrap();
                        prop_assert_eq!(c_name.as_str(), name);
                        same_run(MACSEC, &flat, &names, sequence.ops(), phy, &reads, fail_at)?;
                    }
                }
                (safe, c) => prop_assert!(false, "the verdicts differ: crate {:?}, C interface ok = {}", safe.map(|_| ()), c.is_ok()),
            }
        }

        #[test]
        fn ext_phy_through_c_is_the_crate(
            text in common::extphy_text(),
            reads in proptest::collection::vec(any::<u16>(), 0..12),
            fail_at in proptest::option::of(0usize..40),
        ) {
            match (ExtPhyTable::parse(&text), parse(PHY, &text)) {
                (Err(e), Err((code, message))) => prop_assert_eq!((code, message), (rc(sys::PHY_SEQ_E_PARSE), c_message(&e))),
                (Ok(table), Ok(handle)) => {
                    let i = info(&handle);
                    prop_assert_eq!((i.poll_ms, i.up_polls, i.holdoff_ms), (table.poll_ms(), table.up_polls(), table.holdoff_ms()));
                    prop_assert_eq!(usize::try_from(i.ports).unwrap(), table.ports().len());
                    for (p, port) in (0u32..).zip(table.ports()) {
                        for (s, section) in (0u32..).zip(Section::ALL) {
                            let (_, flat, names) = seq(&handle, 3 * p + s).unwrap();
                            same_run(PHY, &flat, &names, port.ops(section), 0, &reads, fail_at)?;
                        }
                    }
                }
                (safe, c) => prop_assert!(false, "the verdicts differ: crate {:?}, C interface ok = {}", safe.map(|_| ()), c.is_ok()),
            }
        }
    }
}
