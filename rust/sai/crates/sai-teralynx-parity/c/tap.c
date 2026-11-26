/* SPDX-License-Identifier: Apache-2.0
 * The parity harness's tap on the mock libifcs's call log (crate sai-teralynx-parity, README there).
 *
 * The mock (ai_re/nix/packages/sai-teralynx/tests/mock, unmodified) records every IFCS call with
 * mock_logf() into a buffer that the tests clear as they go (mock_ifcs_log_clear), so the buffer
 * never holds a whole run. This library, LD_PRELOADed (or linked into the program), interposes
 * mock_logf: the mock calls it through its PLT (a global default-visibility function of a -fPIC
 * object), so every line it logs passes here first. Each line is formatted exactly as mock_core.c
 * does (2048-byte buffer, vsnprintf with size - 1, at most 2046 characters, then '\n'), appended to
 * PARITY_TAP_FILE with one write(2) (O_APPEND: lines of several threads never interleave inside a
 * line) and then handed to the mock's own mock_logf as "%s", so its buffer and every test hook that
 * reads it see what they would see without the tap.
 *
 *   PARITY_TAP_FILE     the stream (created / appended); unset: nothing written, lines still forwarded
 *   PARITY_MOCK         the mock's absolute path: its own mock_logf is looked up on its handle
 *                       (RTLD_NOLOAD: the library under test loaded it); fallback RTLD_NEXT
 *   PARITY_TAP_MUTATE   negative control: a line containing this text is written to the STREAM with
 *                       " [mutated]" appended (the mock's buffer gets the original line), so a
 *                       comparison of streams must report a difference
 *
 * parity_tap_mark(text) writes "@@ <text>" into the stream (section markers of the test_ifcs build,
 * sections.cpp). Nothing here allocates: the learn thread logs through it too. */
#define _GNU_SOURCE
#include <dlfcn.h>
#include <fcntl.h>
#include <pthread.h>
#include <stdarg.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

typedef void (*logf_fn)(const char* fmt, ...);
void mock_logf(const char* fmt, ...) __attribute__((format(printf, 1, 2)));

static pthread_once_t once = PTHREAD_ONCE_INIT;
static int tap_fd = -1;
static const char* mutate;

static void init(void)
{
    const char* f = getenv("PARITY_TAP_FILE");
    if (f && *f) tap_fd = open(f, O_WRONLY | O_CREAT | O_APPEND | O_CLOEXEC, 0644);
    mutate = getenv("PARITY_TAP_MUTATE");
    if (mutate && !*mutate) mutate = NULL;
}

static logf_fn real_logf(void)
{
    static logf_fn real;
    if (real) return real;
    const char* mock = getenv("PARITY_MOCK");
    logf_fn fn = NULL;
    if (mock && *mock) {
        void* h = dlopen(mock, RTLD_NOW | RTLD_NOLOAD);
        if (h) {
            fn = (logf_fn)dlsym(h, "mock_logf");
            dlclose(h);
        }
    }
    if (!fn) fn = (logf_fn)dlsym(RTLD_NEXT, "mock_logf");
    if (fn == (logf_fn)mock_logf) fn = NULL;   /* never ourselves */
    real = fn;
    return fn;
}

static void put(const char* b, size_t n)
{
    if (tap_fd < 0) return;
    ssize_t w = write(tap_fd, b, n);
    (void)w;
}

/* mock_core.c's mock_logf, line for line, then the stream */
void mock_logf(const char* fmt, ...)
{
    pthread_once(&once, init);
    char b[2048 + 16];
    va_list ap;
    va_start(ap, fmt);
    int n = vsnprintf(b, 2048 - 1, fmt, ap);
    va_end(ap);
    if (n < 0) return;
    if ((size_t)n > 2048 - 2) n = (int)2048 - 2;
    b[n] = 0;
    logf_fn real = real_logf();
    if (real) real("%s", b);
    if (mutate && strstr(b, mutate)) {
        memcpy(b + n, " [mutated]", 10);
        n += 10;
    }
    b[n++] = '\n';
    put(b, (size_t)n);
}

void parity_tap_mark(const char* text)
{
    pthread_once(&once, init);
    char b[512];
    int n = snprintf(b, sizeof(b) - 1, "@@ %s", text);
    if (n < 0) return;
    if ((size_t)n > sizeof(b) - 2) n = (int)sizeof(b) - 2;
    b[n++] = '\n';
    put(b, (size_t)n);
}
