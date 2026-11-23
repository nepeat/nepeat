// SPDX-License-Identifier: Apache-2.0
// sai_rs_test.h: the test-only C ABI of the Rust libsai (rust/sai). libsai.so exports it only
// when built with the cargo feature "test-port" of sai-ffi (never in a production library).
//
// The C++ suite's tests/test_port.h is implemented over this function by
// tests/test_port_rust.cpp (sai-openbcm). The protocol (requests, reply records, encodings) is
// documented in rust/sai/crates/sai-test-port/src/lib.rs; in short: a request is
// "<name> <arg> ..." (single blanks, UTF-8), a reply is lines of blank-separated key=value
// fields, each line ending in "\n".
#pragma once

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

#define SAI_RS_TEST_OK 0           // reply holds the answer
#define SAI_RS_TEST_ABSENT 1       // the thing asked about does not exist; reply empty
#define SAI_RS_TEST_TOO_SMALL 2    // reply_cap < *reply_len (the length needed); nothing written
#define SAI_RS_TEST_UNSUPPORTED 3  // the library cannot answer this yet; reply = one-line reason
#define SAI_RS_TEST_BAD_REQUEST 4  // unknown request or malformed arguments; reply = one-line reason
#define SAI_RS_TEST_FAILURE (-1)   // null request / reply_len, invalid UTF-8, a panic, a poisoned lock

// request: request_len bytes of UTF-8 (not null; no NUL needed, none allowed inside). reply: a buffer of reply_cap bytes (may be null when reply_cap is 0).
// *reply_len is always written (when reply_len is not null): the reply's length in bytes. The
// reply is written only if it fits, and is not NUL-terminated.
// Runs under the library's API lock, like every SAI call: never call it from a SAI
// notification callback. Events a hook raises are delivered by the library's notifier thread
// after the call returns.
int32_t sai_rs_test_query(const char* request, size_t request_len, char* reply, size_t reply_cap,
                          size_t* reply_len);

#ifdef __cplusplus
}
#endif
