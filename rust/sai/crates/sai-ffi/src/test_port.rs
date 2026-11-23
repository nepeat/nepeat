//! `sai_rs_test_query`: the one test-only export (cargo feature `test-port`, off by default),
//! declared in `sai-test-port/include/sai_rs_test.h`. The C++ suite's test port
//! (`tests/test_port_rust.cpp`) asks it what the library and its virtual backend hold; the
//! answers come from the safe crate `sai-test-port`. This file only checks the pointers,
//! validates the UTF-8, takes the API lock when the request needs the library, runs the diag
//! server's two requests itself (`crate::diag`), and writes the reply if it fits.
use crate::guard::guard_value;
use crate::{notify, ptr, state};
use core::ffi::c_char;
use sai_core::backend::Backend;
use sai_sys as sys;
use sai_test_port::request::Request;
use sai_test_port::{Code, Context, Query, Reply};
use std::sync::atomic::Ordering;

/// Answers one test-port request (protocol: crate `sai-test-port`). Returns a
/// `SAI_RS_TEST_*` code; `*reply_len` is the reply's length, written whenever `reply_len` is
/// not null; the reply is written only when it fits (`SAI_RS_TEST_TOO_SMALL` otherwise).
///
/// # Safety
/// `request` is null or points to `request_len` readable bytes that are not written to during
/// the call; `reply` is null or valid for writes of `reply_cap` bytes; `reply_len` is null or
/// valid for a write of a `size_t`; the three do not overlap.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sai_rs_test_query(
    request: *const c_char,
    request_len: usize,
    reply: *mut c_char,
    reply_cap: usize,
    reply_len: *mut usize,
) -> i32 {
    guard_value(Code::Failure.raw(), || {
        // SAFETY: this function's contract for `reply_len`.
        let Ok(len_slot) = (unsafe { ptr::out(reply_len) }) else {
            return Code::Failure.raw();
        };
        let (code, len) = query(request, request_len, reply, reply_cap);
        len_slot.write(len);
        code
    })
}

/// The code and the reply's length.
fn query(request: *const c_char, request_len: usize, reply: *mut c_char, reply_cap: usize) -> (i32, usize) {
    if request.is_null() {
        return (Code::Failure.raw(), 0);
    }
    let Ok(count) = u32::try_from(request_len) else {
        return (Code::Failure.raw(), 0);
    };
    // SAFETY: `request` is non-null (checked) and points to `request_len` readable bytes not
    // written to during the call (contract of `sai_rs_test_query`, the only caller); a pointer
    // cast does not change that.
    let Ok(bytes) = (unsafe { ptr::in_slice(request.cast::<u8>(), count) }) else {
        return (Code::Failure.raw(), 0);
    };
    let Ok(text) = core::str::from_utf8(bytes) else {
        return (Code::Failure.raw(), 0);
    };
    let answer = answer(text);
    let needed = answer.text.len();
    if answer.code == Code::Failure {
        return (Code::Failure.raw(), 0);
    }
    if needed > reply_cap {
        return (Code::TooSmall.raw(), needed);
    }
    let Ok(count) = u32::try_from(needed) else {
        return (Code::Failure.raw(), needed);
    };
    // SAFETY: `reply` is null or valid for `reply_cap` >= `needed` byte writes with nothing else
    // accessing them (contract of `sai_rs_test_query`, the only caller); `out_slice` refuses null
    // with a non-zero count and does not look at the pointer for zero.
    let written =
        unsafe { ptr::out_slice(reply.cast::<u8>(), count) }.and_then(|slot| slot.write(answer.text.as_bytes()));
    match written {
        Ok(()) => (answer.code.raw(), needed),
        Err(_) => (Code::Failure.raw(), needed),
    }
}

/// The reply to a valid UTF-8 request: under the API lock when the request needs the library.
fn answer(text: &str) -> Reply {
    let query = match Query::parse(text) {
        Ok(query) => query,
        Err(reply) => return reply,
    };
    let context = Context {
        has_hardware_backend: cfg!(feature = "openbcm"),
        events_delivered: notify::EVENTS_DELIVERED.load(Ordering::Acquire),
    };
    // The diag server is the rim's (it must be stopped outside the API lock): `crate::diag`.
    match query.request() {
        Request::DiagStart(path) => {
            crate::diag::test_start(path);
            return Reply {
                code: Code::Ok,
                text: String::new(),
            };
        }
        Request::DiagStop => {
            crate::diag::test_stop();
            return Reply {
                code: Code::Ok,
                text: String::new(),
            };
        }
        _ => {}
    }
    if !query.needs_library() {
        return query.answer::<Box<dyn Backend>>(None, &context);
    }
    match state::with_api(|api| Ok(query.answer(Some(&mut api.sai), &context))) {
        Ok(reply) => reply,
        Err(sys::SAI_STATUS_UNINITIALIZED) => query.answer::<Box<dyn Backend>>(None, &context),
        // poisoned: a panic left the state unusable
        Err(_) => Reply {
            code: Code::Failure,
            text: String::new(),
        },
    }
}

/// Compile-time check that the export has the prototype of `sai_rs_test.h` (written out in Rust;
/// the test below compares the header's text). Never called.
#[allow(dead_code)]
fn prototype_matches_the_header() {
    fn same<T>(_: T, _: T) {}
    same::<unsafe extern "C" fn(*const c_char, usize, *mut c_char, usize, *mut usize) -> i32>(
        sai_rs_test_query,
        sai_rs_test_query,
    );
}

#[cfg(test)]
// Test code: a failed unwrap or index is the test failing. The unsafe blocks pass pointers to
// live locals of the stated length, or null; one comment here instead of one per call.
#[allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::undocumented_unsafe_blocks,
    reason = "tests"
)]
mod tests {
    use super::*;
    use core::ptr::{null, null_mut};

    /// Calls the export with a request and a reply buffer of `cap` bytes.
    fn call(request: &[u8], cap: usize) -> (i32, usize, Vec<u8>) {
        let mut buf = vec![0xAAu8; cap];
        let mut len = usize::MAX;
        let code = unsafe {
            sai_rs_test_query(
                request.as_ptr().cast(),
                request.len(),
                buf.as_mut_ptr().cast(),
                cap,
                &raw mut len,
            )
        };
        (code, len, buf)
    }

    // Only requests answered without the API lock: the state's own unit test poisons it.
    const MASK: &[u8] = b"oid_index_mask";
    const MASK_REPLY: &[u8] = b"value=281474976710655\n";

    #[test]
    fn the_reply_is_written_when_it_fits() {
        let (code, len, buf) = call(MASK, 64);
        assert_eq!((code, len), (0, MASK_REPLY.len()));
        assert_eq!(&buf[..len], MASK_REPLY);
        assert!(buf[len..].iter().all(|b| *b == 0xAA), "nothing after the reply, no NUL");
        // exact fit
        let (code, len, buf) = call(MASK, MASK_REPLY.len());
        assert_eq!((code, len, &buf[..]), (0, MASK_REPLY.len(), MASK_REPLY));
    }

    #[test]
    fn too_small_says_how_much_and_writes_nothing() {
        let (code, len, buf) = call(MASK, MASK_REPLY.len() - 1);
        assert_eq!((code, len), (2, MASK_REPLY.len()));
        assert!(buf.iter().all(|b| *b == 0xAA));
        // the size query: a null buffer of capacity 0
        let mut len = 0usize;
        let code = unsafe { sai_rs_test_query(MASK.as_ptr().cast(), MASK.len(), null_mut(), 0, &raw mut len) };
        assert_eq!((code, len), (2, MASK_REPLY.len()));
    }

    #[test]
    fn an_empty_reply_needs_no_buffer() {
        let mut len = usize::MAX;
        let request = b"log_to_stderr 0";
        let code = unsafe { sai_rs_test_query(request.as_ptr().cast(), request.len(), null_mut(), 0, &raw mut len) };
        assert_eq!((code, len), (0, 0));
    }

    #[test]
    fn bad_input_is_failure_or_bad_request() {
        // null request, with and without a length
        let mut len = usize::MAX;
        let code = unsafe { sai_rs_test_query(null(), 0, null_mut(), 0, &raw mut len) };
        assert_eq!((code, len), (-1, 0));
        let code = unsafe { sai_rs_test_query(null(), 5, null_mut(), 0, &raw mut len) };
        assert_eq!((code, len), (-1, 0));
        // null reply_len: nothing to tell the length through
        let code = unsafe { sai_rs_test_query(MASK.as_ptr().cast(), MASK.len(), null_mut(), 0, null_mut()) };
        assert_eq!(code, -1);
        // invalid UTF-8
        let (code, len, _) = call(b"oid_index \xff", 64);
        assert_eq!((code, len), (-1, 0));
        // a null buffer that claims a capacity
        let mut len = 0usize;
        let code = unsafe { sai_rs_test_query(MASK.as_ptr().cast(), MASK.len(), null_mut(), 64, &raw mut len) };
        assert_eq!(code, -1);
        // a NUL inside, an unknown name: the protocol's BAD_REQUEST with a reason
        let (code, len, buf) = call(b"oid_index_mask\0", 128);
        assert_eq!(code, 4);
        assert!(String::from_utf8_lossy(&buf[..len]).starts_with("oid_index_mask"));
        let (code, len, buf) = call(b"no_such_request", 128);
        assert_eq!((code, &buf[..len]), (4, &b"no_such_request: unknown request"[..]));
    }

    #[test]
    fn the_header_declares_this_prototype() {
        let header = include_str!("../../sai-test-port/include/sai_rs_test.h");
        let flat: String = header
            .lines()
            .filter(|l| !l.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join(" ")
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        assert!(
            flat.contains(
                "int32_t sai_rs_test_query(const char* request, size_t request_len, char* reply, size_t reply_cap, size_t* reply_len);"
            ),
            "{flat}"
        );
        for (name, code) in [
            ("SAI_RS_TEST_OK 0", Code::Ok),
            ("SAI_RS_TEST_ABSENT 1", Code::Absent),
            ("SAI_RS_TEST_TOO_SMALL 2", Code::TooSmall),
            ("SAI_RS_TEST_UNSUPPORTED 3", Code::Unsupported),
            ("SAI_RS_TEST_BAD_REQUEST 4", Code::BadRequest),
            ("SAI_RS_TEST_FAILURE (-1)", Code::Failure),
        ] {
            assert!(flat.contains(&format!("#define {name}")), "{name}");
            let value: i32 = name
                .rsplit(' ')
                .next()
                .unwrap()
                .trim_matches(['(', ')'])
                .parse()
                .unwrap();
            assert_eq!(code.raw(), value);
        }
    }
}
