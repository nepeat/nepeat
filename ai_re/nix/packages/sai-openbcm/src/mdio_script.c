/* SPDX-License-Identifier: Apache-2.0
 * MDIO script interpreter, see mdio_script.h. No SDK code: the bus is two
 * callbacks, so this builds and is tested on any host. */
#include <ctype.h>
#include <errno.h>
#include <stdarg.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

#include "mdio_script.h"

#define MAX_DIFFER_LOG 20
#define SPIN_BELOW_US 20000UL

static void sleep_us(const mdio_script_io_t* io, unsigned long us)
{
    if (!us) return;
    if (io->sleep_us) {
        io->sleep_us(io->ctx, us);
        return;
    }
    if (us < SPIN_BELOW_US) {
        /* Short delays are spun: a kernel without high-resolution timers
         * rounds every sleep up to a tick (4 ms on the T2080 appliance
         * kernel, which made 90,000 delays of 30-200 us take 6 minutes). */
        struct timespec t0, t;
        clock_gettime(CLOCK_MONOTONIC, &t0);
        do {
            clock_gettime(CLOCK_MONOTONIC, &t);
        } while ((unsigned long long)(t.tv_sec - t0.tv_sec) * 1000000ULL + (unsigned long long)(t.tv_nsec / 1000) <
                 (unsigned long long)(t0.tv_nsec / 1000) + us);
        return;
    }
    struct timespec ts = {(time_t)(us / 1000000UL), (long)(us % 1000000UL) * 1000L};
    while (nanosleep(&ts, &ts) != 0 && errno == EINTR) {}
}

static void logf_(const mdio_script_io_t* io, const char* fmt, ...)
{
    char b[400];
    va_list ap;
    if (!io || !io->log) return;
    va_start(ap, fmt);
    vsnprintf(b, sizeof(b), fmt, ap);
    va_end(ap);
    io->log(io->ctx, b);
}

/* Split a line into at most max fields; returns the count (max + 1: too many). */
static int fields(char* s, char** f, int max)
{
    int n = 0;
    while (*s) {
        while (*s && isspace((unsigned char)*s)) s++;
        if (!*s) break;
        if (n == max) return max + 1;
        f[n++] = s;
        while (*s && !isspace((unsigned char)*s)) s++;
        if (*s) *s++ = 0;
    }
    return n;
}

static int num(const char* s, int base, unsigned long max, unsigned long* out)
{
    char* end;
    unsigned long v;
    if (!*s || *s == '-' || *s == '+') return 0;
    errno = 0;
    v = strtoul(s, &end, base);
    if (errno || *end || v > max) return 0;
    *out = v;
    return 1;
}

int mdio_script_run(const char* path, const mdio_script_io_t* io, mdio_script_stats_t* st, char* err, int errlen)
{
    mdio_script_stats_t s;
    char line[512], label[160] = "(start)";
    unsigned long ln = 0;
    int rv = MDIO_SCRIPT_OK;
    FILE* fp = fopen(path, "r");
    memset(&s, 0, sizeof(s));
    if (err && errlen > 0) err[0] = 0;
#define FAIL(code, ...)                                                    \
    do {                                                                   \
        if (err && errlen > 0) snprintf(err, (size_t)errlen, __VA_ARGS__); \
        rv = (code);                                                       \
        goto out;                                                          \
    } while (0)
    if (!fp) FAIL(MDIO_SCRIPT_E_FILE, "%s: %s", path, strerror(errno));
    while (fgets(line, sizeof(line), fp)) {
        char* f[9];
        unsigned long a[7] = {0, 0, 0, 0, 0xffff, 1, 0};
        size_t len = strlen(line);
        int n, want;
        char op;
        ln++;
        if (len && line[len - 1] != '\n' && !feof(fp)) FAIL(MDIO_SCRIPT_E_SYNTAX, "%s:%lu: line too long", path, ln);
        while (len && (line[len - 1] == '\n' || line[len - 1] == '\r')) line[--len] = 0;
        if (ln == 1) {
            if (strcmp(line, "mdio-script 1") != 0)
                FAIL(MDIO_SCRIPT_E_SYNTAX, "%s: not an MDIO script (first line must be 'mdio-script 1')", path);
            continue;
        }
        if (!len || line[0] == '#') continue;
        if (line[0] == 'L' && (line[1] == ' ' || !line[1])) {
            snprintf(label, sizeof(label), "%.150s", line[1] ? line + 2 : "");
            s.labels++;
            if (io) logf_(io, "%s", label);
            continue;
        }
        n = fields(line, f, 8);
        if (n < 1) continue;   /* blank */
        op = f[0][0];
        if (f[0][1]) FAIL(MDIO_SCRIPT_E_SYNTAX, "%s:%lu: unknown operation", path, ln);
        if (op == 'D') {
            if (n != 2 || !num(f[1], 10, 60000000UL, &a[0]))
                FAIL(MDIO_SCRIPT_E_SYNTAX, "%s:%lu: D <usec up to 60000000>", path, ln);
            s.delays++;
            s.delay_us += a[0];
            if (io) sleep_us(io, a[0]);
            continue;
        }
        want = op == 'W' ? 5 : op == 'R' ? 4 : op == 'E' ? (n == 5 ? 5 : 6) : (op == 'P' || op == 'N') ? 8 : 0;
        if (!want) FAIL(MDIO_SCRIPT_E_SYNTAX, "%s:%lu: unknown operation '%c'", path, ln, op);
        if (n != want || !num(f[1], 16, 0xffff, &a[0]) || !num(f[2], 16, 31, &a[1]) || a[1] == 0 ||
            !num(f[3], 16, 0xffff, &a[2]) || (n > 4 && !num(f[4], 16, 0xffff, &a[3])) ||
            (n > 5 && !num(f[5], 16, 0xffff, &a[4])) || (n > 6 && (!num(f[6], 10, 1000000UL, &a[5]) || a[5] == 0)) ||
            (n > 7 && !num(f[7], 10, 60000000UL, &a[6])))
            FAIL(MDIO_SCRIPT_E_SYNTAX, "%s:%lu: bad arguments for '%c' (clause 45 only: dev 1..1f)", path, ln, op);
        s.lines++;
        if (op == 'W') {
            s.writes++;
            if (io && io->write(io->ctx, (unsigned)a[0], (unsigned)a[1], (unsigned)a[2], (unsigned)a[3]) != 0)
                FAIL(MDIO_SCRIPT_E_IO, "%s:%lu: write %lx %lx.%04lx failed [%s]", path, ln, a[0], a[1], a[2], label);
            continue;
        }
        if (op == 'R' || op == 'E') {
            unsigned v = 0;
            s.reads++;
            if (!io) continue;
            if (io->read(io->ctx, (unsigned)a[0], (unsigned)a[1], (unsigned)a[2], &v) != 0)
                FAIL(MDIO_SCRIPT_E_IO, "%s:%lu: read %lx %lx.%04lx failed [%s]", path, ln, a[0], a[1], a[2], label);
            if (op == 'E' && (v & a[4]) != a[3]) {
                if (s.differs++ < MAX_DIFFER_LOG)
                    logf_(io, "differs: line %lu: %lx %lx.%04lx = %04x, script %04lx mask %04lx [%s]", ln, a[0], a[1],
                          a[2], v, a[3], a[4], label);
            }
            continue;
        }
        /* P / N */
        s.polls++;
        s.reads++;
        if (io) {
            unsigned long t;
            unsigned v = 0;
            int ok = 0;
            for (t = 0; t < a[5]; t++) {
                if (t) {
                    sleep_us(io, a[6]);
                    s.poll_rereads++;
                    s.reads++;
                }
                if (io->read(io->ctx, (unsigned)a[0], (unsigned)a[1], (unsigned)a[2], &v) != 0)
                    FAIL(MDIO_SCRIPT_E_IO, "%s:%lu: read %lx %lx.%04lx failed [%s]", path, ln, a[0], a[1], a[2], label);
                ok = op == 'P' ? (v & a[4]) == a[3] : (v & a[4]) != a[3];
                if (ok) break;
            }
            if (!ok)
                FAIL(MDIO_SCRIPT_E_POLL, "%s:%lu: phy %lx %lx.%04lx reads %04x, script wants %s%04lx (mask %04lx) "
                     "after %lu reads [%s]", path, ln, a[0], a[1], a[2], v, op == 'P' ? "" : "anything but ", a[3],
                     a[4], a[5], label);
        }
    }
    if (ferror(fp)) FAIL(MDIO_SCRIPT_E_FILE, "%s: read error", path);
    if (ln == 0) FAIL(MDIO_SCRIPT_E_SYNTAX, "%s: empty file", path);
out:
    if (fp) fclose(fp);
    if (st) *st = s;
    return rv;
#undef FAIL
}
