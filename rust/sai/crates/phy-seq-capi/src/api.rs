//! The exported functions. Each checks its pointers through `ptr`, does its work in safe code
//! and writes its outputs last, all inside [`guard`].
#![allow(unsafe_code, reason = "the C-ABI rim: see the crate documentation")]

use std::ffi::{c_char, c_void};
use std::fmt;
use std::panic::{AssertUnwindSafe, catch_unwind};

use phy_seq::{Bus, Dev, ExecError, ExecErrorKind, Format, Op, Params, run};

use crate::flat::{from_flat, param_of};
use crate::ptr::{in_ref, in_slice, out, out_text};
use crate::sys;
use crate::table::Table;

/// A `PHY_SEQ_*` return code (bindgen types the macros `u32`) as the `int32_t` the functions return.
const fn code(constant: u32) -> i32 {
    constant.cast_signed()
}

const OK: i32 = code(sys::PHY_SEQ_OK);
const E_PARSE: i32 = code(sys::PHY_SEQ_E_PARSE);
const E_ARG: i32 = code(sys::PHY_SEQ_E_ARG);
const E_PARAM: i32 = code(sys::PHY_SEQ_E_PARAM);
const E_BUS: i32 = code(sys::PHY_SEQ_E_BUS);
const E_FAULT: i32 = code(sys::PHY_SEQ_E_FAULT);
const E_PANIC: i32 = code(sys::PHY_SEQ_E_PANIC);

/// Runs `body`; a panic inside it becomes `PHY_SEQ_E_PANIC` instead of unwinding into C.
/// There is no state to poison: every call works on its arguments only.
fn guard(body: impl FnOnce() -> i32) -> i32 {
    catch_unwind(AssertUnwindSafe(body)).unwrap_or(E_PANIC)
}

fn format_of(format: u32) -> Option<Format> {
    match format {
        sys::PHY_SEQ_FORMAT_EXT_MACSEC => Some(Format::ExtMacsec),
        sys::PHY_SEQ_FORMAT_EXT_PHY => Some(Format::ExtPhy),
        _ => None,
    }
}

/// `PHY_SEQ_ABI` of this library.
#[unsafe(no_mangle)]
pub extern "C" fn phy_seq_abi() -> u32 {
    sys::PHY_SEQ_ABI
}

/// Parses a table (see `phy_seq.h`).
///
/// # Safety
/// `text` points to `len` readable bytes (or `len` is 0); `table` is null or valid for one
/// write; `err` is null or points to `err_cap` writable bytes. Nothing else accesses them
/// during the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn phy_seq_parse(
    format: u32,
    text: *const u8,
    len: usize,
    table: *mut *mut sys::phy_seq_table,
    err: *mut c_char,
    err_cap: usize,
) -> i32 {
    guard(|| {
        // SAFETY: table is null (refused by out) or valid for one write (the caller's contract).
        if !unsafe { out(table, std::ptr::null_mut()) } {
            return E_ARG;
        }
        // SAFETY: err is null (ignored by out_text) or err_cap writable bytes (the caller's contract).
        unsafe { out_text(err, err_cap, b"") };
        // SAFETY: len readable bytes at text, unwritten during the call (the caller's contract).
        let (Some(format), Some(text)) = (format_of(format), unsafe { in_slice(text, len) }) else {
            return E_ARG;
        };
        match Table::parse(format, text) {
            Ok(parsed) => {
                let handle = Box::into_raw(Box::new(parsed)).cast::<sys::phy_seq_table>();
                // SAFETY: table was written above, so it is non-null and valid for a write.
                let _ = unsafe { out(table, handle) };
                OK
            }
            Err(e) => {
                // SAFETY: as for the first out_text.
                unsafe { out_text(err, err_cap, &e.to_bytes()) };
                E_PARSE
            }
        }
    })
}

/// Frees a table; null is allowed.
///
/// # Safety
/// `table` is null or a handle `phy_seq_parse` returned that was not freed before; no other
/// thread uses it, and nothing `phy_seq_table_seq` returned for it is used afterwards.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn phy_seq_free(table: *mut sys::phy_seq_table) {
    let _ = guard(|| {
        if !table.is_null() {
            // SAFETY: a non-null handle is the Box<Table> phy_seq_parse leaked, and this is the
            // one call that takes it back (the caller's contract).
            drop(unsafe { Box::from_raw(table.cast::<Table>()) });
        }
        OK
    });
}

/// The table behind a handle.
///
/// # Safety
/// `table` is null or a live handle of `phy_seq_parse` that is not freed for `'a`.
unsafe fn table_of<'a>(table: *const sys::phy_seq_table) -> Option<&'a Table> {
    // SAFETY: a non-null handle points to the Table phy_seq_parse boxed (so it is aligned and
    // initialised), alive for 'a (the caller's contract); a Table is never mutated.
    unsafe { in_ref(table.cast::<Table>()) }
}

/// What the table holds (see `phy_seq.h`).
///
/// # Safety
/// `table` is null or a live handle; `info` is null or valid for one write.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn phy_seq_table_info(table: *const sys::phy_seq_table, info: *mut sys::phy_seq_info) -> i32 {
    guard(|| {
        // SAFETY: a live handle or null (the caller's contract).
        let Some(table) = (unsafe { table_of(table) }) else {
            return E_ARG;
        };
        // SAFETY: info is null (refused) or valid for one write (the caller's contract).
        if unsafe { out(info, table.info) } { OK } else { E_ARG }
    })
}

/// Port `index` of the table (see `phy_seq.h`).
///
/// # Safety
/// `table` is null or a live handle; `port` is null or valid for one write.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn phy_seq_table_port(
    table: *const sys::phy_seq_table,
    index: u32,
    port: *mut sys::phy_seq_port,
) -> i32 {
    guard(|| {
        // SAFETY: a live handle or null (the caller's contract).
        let Some(table) = (unsafe { table_of(table) }) else {
            return E_ARG;
        };
        let Some(found) = usize::try_from(index).ok().and_then(|i| table.ports.get(i)) else {
            return E_ARG;
        };
        // SAFETY: port is null (refused) or valid for one write (the caller's contract).
        if unsafe { out(port, *found) } { OK } else { E_ARG }
    })
}

/// Sequence `index` of the table (see `phy_seq.h`). The pointers written to `seq` point into the
/// table and are valid until `phy_seq_free`.
///
/// # Safety
/// `table` is null or a live handle; `seq` is null or valid for one write.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn phy_seq_table_seq(
    table: *const sys::phy_seq_table,
    index: u32,
    seq: *mut sys::phy_seq_seq,
) -> i32 {
    guard(|| {
        // SAFETY: a live handle or null (the caller's contract).
        let Some(table) = (unsafe { table_of(table) }) else {
            return E_ARG;
        };
        let Some(found) = usize::try_from(index).ok().and_then(|i| table.seqs.get(i)) else {
            return E_ARG;
        };
        let view = sys::phy_seq_seq {
            name: found.name.as_ptr().cast::<c_char>(),
            name_len: found.name.len(),
            ops: found.ops.as_ptr(),
            n_ops: found.ops.len(),
            names: found.names.as_ptr().cast::<c_char>(),
            names_len: found.names.len(),
        };
        // SAFETY: seq is null (refused) or valid for one write (the caller's contract).
        if unsafe { out(seq, view) } { OK } else { E_ARG }
    })
}

type ReadFn = unsafe extern "C" fn(*mut c_void, u32, u16, u8, u16, *mut u16) -> i32;
type WriteFn = unsafe extern "C" fn(*mut c_void, u32, u16, u8, u16, u16) -> i32;
type DelayFn = unsafe extern "C" fn(*mut c_void, u32, u32);

/// The caller's status of a failed bus access; displays as the number (the C++ messages say
/// "MDIO error %d").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Status(i32);

impl fmt::Display for Status {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

/// The C callbacks as a [`Bus`].
struct CBus {
    ctx: *mut c_void,
    read: ReadFn,
    write: WriteFn,
    delay: DelayFn,
    /// Index of the operation being run.
    op: u32,
}

impl Bus for CBus {
    type Error = Status;

    fn begin(&mut self, index: usize, _op: &Op) {
        self.op = u32::try_from(index).unwrap_or(u32::MAX);
    }

    fn read(&mut self, phy: u16, dev: Dev, reg: u16) -> Result<u16, Status> {
        let mut value = 0u16;
        // SAFETY: the callback and ctx are the ones the caller of phy_seq_exec passed for this
        // call (its contract: callable with that ctx on this thread); value is a live u16.
        let status = unsafe { (self.read)(self.ctx, self.op, phy, dev.get(), reg, &raw mut value) };
        if status == 0 { Ok(value) } else { Err(Status(status)) }
    }

    fn write(&mut self, phy: u16, dev: Dev, reg: u16, value: u16) -> Result<(), Status> {
        // SAFETY: as for read.
        let status = unsafe { (self.write)(self.ctx, self.op, phy, dev.get(), reg, value) };
        if status == 0 { Ok(()) } else { Err(Status(status)) }
    }

    fn delay_us(&mut self, us: u32) {
        // SAFETY: as for read.
        unsafe { (self.delay)(self.ctx, self.op, us) };
    }
}

/// The return code and the message of a run that stopped.
fn failure(format: Format, e: &ExecError<Status>) -> (i32, i32, String) {
    let message = match format {
        Format::ExtMacsec => e.to_string(),
        Format::ExtPhy => e.ext_phy_message(),
    };
    match e.kind {
        ExecErrorKind::MissingParam(_) => (E_PARAM, 0, message),
        ExecErrorKind::Bus { error, .. } => (E_BUS, error.0, message),
        ExecErrorKind::Mismatch { .. } => (E_FAULT, 0, message),
    }
}

/// Runs operations (see `phy_seq.h`).
///
/// # Safety
/// `ops` points to `n_ops` operations, `names` to `names_len` bytes, `params` to `n_params`
/// parameters whose `bytes` point to `len` bytes each (a count of 0 needs no pointer); `io`
/// points to the callbacks, which may be called with `io->ctx` on this thread and return
/// normally; `outcome` is null or valid for one write; `why` is null or points to `why_cap`
/// writable bytes. Nothing else writes any of them during the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn phy_seq_exec(
    format: u32,
    ops: *const sys::phy_seq_op,
    n_ops: usize,
    names: *const c_char,
    names_len: usize,
    phy: u16,
    params: *const sys::phy_seq_param,
    n_params: usize,
    io: *const sys::phy_seq_io,
    outcome: *mut sys::phy_seq_outcome,
    why: *mut c_char,
    why_cap: usize,
) -> i32 {
    guard(|| {
        let mut result = sys::phy_seq_outcome {
            bus_status: 0,
            cond: 0,
            last: 0,
        };
        let done = |rc: i32, result: sys::phy_seq_outcome, message: &str| {
            // SAFETY: outcome is null (ignored) or valid for one write (the caller's contract).
            let _ = unsafe { out(outcome, result) };
            // SAFETY: why is null (ignored) or why_cap writable bytes (the caller's contract).
            unsafe { out_text(why, why_cap, message.as_bytes()) };
            rc
        };
        let Some(format) = format_of(format) else {
            return done(E_ARG, result, "phy-seq: unknown table format");
        };
        // SAFETY: n_ops operations at ops, unwritten during the call (the caller's contract).
        let Some(flat) = (unsafe { in_slice(ops, n_ops) }) else {
            return done(E_ARG, result, "phy-seq: no operations");
        };
        // SAFETY: names_len bytes at names, unwritten during the call (the caller's contract).
        let Some(names) = (unsafe { in_slice(names.cast::<u8>(), names_len) }) else {
            return done(E_ARG, result, "phy-seq: no result names");
        };
        // SAFETY: n_params parameters at params, unwritten during the call (the caller's contract).
        let Some(given) = (unsafe { in_slice(params, n_params) }) else {
            return done(E_ARG, result, "phy-seq: no parameters");
        };
        // SAFETY: io is null (refused) or points to the callback table (the caller's contract).
        let Some(io) = (unsafe { in_ref(io) }) else {
            return done(E_ARG, result, "phy-seq: no callbacks");
        };
        let (Some(read), Some(write), Some(delay)) = (io.read, io.write, io.delay) else {
            return done(E_ARG, result, "phy-seq: a read, write or delay callback is missing");
        };

        let mut typed = Vec::with_capacity(flat.len());
        for (index, f) in flat.iter().enumerate() {
            match from_flat(f, names, format) {
                Ok(op) => typed.push(op),
                Err(what) => {
                    return done(
                        E_ARG,
                        result,
                        &format!("phy-seq: operation {index} (line {}): {what}", f.line),
                    );
                }
            }
        }
        let mut par = Params::new();
        for p in given {
            let Some(name) = param_of(p.param) else {
                return done(E_ARG, result, "phy-seq: unknown parameter");
            };
            // SAFETY: len bytes at bytes, unwritten during the call (the caller's contract).
            let Some(bytes) = (unsafe { in_slice(p.bytes, p.len) }) else {
                return done(E_ARG, result, "phy-seq: a parameter without bytes");
            };
            par.set_cut(name, bytes);
        }

        let mut bus = CBus {
            ctx: io.ctx,
            read,
            write,
            delay,
            op: 0,
        };
        match run(&mut bus, &typed, phy, &par) {
            Ok(outcome) => {
                if let Some(deliver) = io.result {
                    for (name, words) in outcome.results.iter() {
                        for (word, &value) in (0u32..).zip(words) {
                            // SAFETY: the caller's result callback with its ctx (its contract);
                            // name points to name.len() bytes that live until the call returns.
                            unsafe {
                                deliver(
                                    io.ctx,
                                    name.as_str().as_ptr().cast::<c_char>(),
                                    name.as_str().len(),
                                    word,
                                    value,
                                );
                            };
                        }
                    }
                }
                result.cond = u32::from(outcome.cond);
                result.last = u32::from(outcome.last);
                done(OK, result, "")
            }
            Err(e) => {
                let (rc, bus_status, message) = failure(format, &e);
                result.bus_status = bus_status;
                done(rc, result, &message)
            }
        }
    })
}

/// Never called: the functions above against the prototypes of `phy_seq.h` (`sys` holds
/// bindgen's declarations of them). Each pair must coerce to one function pointer type, so a
/// header and an implementation that disagree do not compile.
#[allow(dead_code)]
fn prototypes_match_the_header() {
    fn same<T>(_: T, _: T) {}
    same::<unsafe extern "C" fn() -> _>(sys::phy_seq_abi, phy_seq_abi);
    same::<unsafe extern "C" fn(_, _, _, _, _, _) -> _>(sys::phy_seq_parse, phy_seq_parse);
    same::<unsafe extern "C" fn(_)>(sys::phy_seq_free, phy_seq_free);
    same::<unsafe extern "C" fn(_, _) -> _>(sys::phy_seq_table_info, phy_seq_table_info);
    same::<unsafe extern "C" fn(_, _, _) -> _>(sys::phy_seq_table_port, phy_seq_table_port);
    same::<unsafe extern "C" fn(_, _, _) -> _>(sys::phy_seq_table_seq, phy_seq_table_seq);
    same::<unsafe extern "C" fn(_, _, _, _, _, _, _, _, _, _, _, _) -> _>(sys::phy_seq_exec, phy_seq_exec);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A panic below an exported function stays inside the library.
    #[test]
    fn a_panic_becomes_a_return_code() {
        assert_eq!(guard(|| OK), OK);
        assert_eq!(guard(|| E_FAULT), E_FAULT);
        let quiet = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        let code = guard(|| panic!("test"));
        std::panic::set_hook(quiet);
        assert_eq!(code, E_PANIC);
    }

    #[test]
    fn the_codes_are_the_headers() {
        assert_eq!(
            [OK, E_PARSE, E_ARG, E_PARAM, E_BUS, E_FAULT, E_PANIC],
            [0, 1, 2, 3, 4, 5, 6]
        );
        assert_eq!(format_of(0), Some(Format::ExtMacsec));
        assert_eq!(format_of(1), Some(Format::ExtPhy));
        assert_eq!(format_of(2), None);
    }
}
