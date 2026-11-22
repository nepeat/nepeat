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

#define TICK_CHUNK_US 500000UL

static void sleep_once(const mdio_script_io_t* io, unsigned long us)
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

/* A delay, in pieces of at most TICK_CHUNK_US with io->tick between them.
 * Non-zero: tick asked to stop. */
static int sleep_us(const mdio_script_io_t* io, unsigned long us)
{
    while (us > TICK_CHUNK_US && io->tick) {
        sleep_once(io, TICK_CHUNK_US);
        us -= TICK_CHUNK_US;
        if (io->tick(io->ctx)) return 1;
    }
    sleep_once(io, us);
    return 0;
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
    return mdio_script_run_opts(path, io, NULL, st, err, errlen);
}

int mdio_script_run_opts(const char* path, const mdio_script_io_t* io_all, const mdio_script_opts_t* opts,
                         mdio_script_stats_t* st, char* err, int errlen)
{
    mdio_script_stats_t local;
    mdio_script_stats_t* s = st ? st : &local;
    mdio_script_fail_t* fail = opts ? opts->fail : NULL;
    const unsigned long vline = opts ? opts->verify_line : 0;
    const int vonly = opts && opts->verify_only;
    char line[512], label[160] = "(start)", first_bad[200] = "";
    unsigned long ln = 0, vcand = 0, vchecks = 0;
    int rv = MDIO_SCRIPT_OK;
    FILE* fp = fopen(path, "r");
    memset(s, 0, sizeof(*s));
    if (fail) memset(fail, 0, sizeof(*fail));
    if (err && errlen > 0) err[0] = 0;
#define FAIL(code, ...)                                                    \
    do {                                                                   \
        if (err && errlen > 0) snprintf(err, (size_t)errlen, __VA_ARGS__); \
        rv = (code);                                                       \
        goto out;                                                          \
    } while (0)
/* The operation of this line, for the caller's failure record. */
#define AT(v, have, n)                                      \
    do {                                                    \
        if (fail) {                                         \
            fail->line = ln;                                \
            fail->op = op;                                  \
            fail->phy = (unsigned)a[0];                     \
            fail->dev = (unsigned)a[1];                     \
            fail->reg = (unsigned)a[2];                     \
            fail->want = (unsigned)a[3];                    \
            fail->mask = (unsigned)a[4];                    \
            fail->got = (v);                                \
            fail->have_value = (have);                      \
            fail->tries = (n);                              \
            snprintf(fail->label, sizeof(fail->label), "%s", label); \
        }                                                   \
    } while (0)
    if (!fp) FAIL(MDIO_SCRIPT_E_FILE, "%s: %s", path, strerror(errno));
    if (vonly && !vline) FAIL(MDIO_SCRIPT_E_SYNTAX, "%s: no verification section (no assertions at its end)", path);
    while (fgets(line, sizeof(line), fp)) {
        /* verify_only: everything before the section is parsed, not run. */
        const mdio_script_io_t* io = (vonly && ln + 1 < vline) ? NULL : io_all;
        char* f[9];
        unsigned long a[7] = {0, 0, 0, 0, 0xffff, 1, 0};
        size_t len = strlen(line);
        int n, want, check;
        char op = 0;
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
            s->labels++;
            if (!vcand) vcand = ln;   /* a label may open the verification section */
            if (io) logf_(io, "%s", label);
            continue;
        }
        n = fields(line, f, 8);
        if (n < 1) continue;   /* blank */
        op = f[0][0];
        if (f[0][1]) FAIL(MDIO_SCRIPT_E_SYNTAX, "%s:%lu: unknown operation", path, ln);
        if (io && io->tick && io->tick(io->ctx)) {
            AT(0, 0, 0);
            if (fail) fail->op = 0;
            FAIL(MDIO_SCRIPT_E_ABORT, "%s:%lu: stopped on request before this line [%s]", path, ln, label);
        }
        if (op == 'D') {
            if (n != 2 || !num(f[1], 10, 60000000UL, &a[0]))
                FAIL(MDIO_SCRIPT_E_SYNTAX, "%s:%lu: D <usec up to 60000000>", path, ln);
            s->delays++;
            s->delay_us += a[0];
            vcand = vchecks = 0;
            if (io && sleep_us(io, a[0]))
                FAIL(MDIO_SCRIPT_E_ABORT, "%s:%lu: stopped on request during a delay [%s]", path, ln, label);
            s->ops++;
            continue;
        }
        want = op == 'W' ? 5 : op == 'R' ? 4 : op == 'E' ? (n == 5 ? 5 : 6) : (op == 'P' || op == 'N') ? 8 : 0;
        if (!want) FAIL(MDIO_SCRIPT_E_SYNTAX, "%s:%lu: unknown operation '%c'", path, ln, op);
        if (n != want || !num(f[1], 16, 0xffff, &a[0]) || !num(f[2], 16, 31, &a[1]) || a[1] == 0 ||
            !num(f[3], 16, 0xffff, &a[2]) || (n > 4 && !num(f[4], 16, 0xffff, &a[3])) ||
            (n > 5 && !num(f[5], 16, 0xffff, &a[4])) || (n > 6 && (!num(f[6], 10, 1000000UL, &a[5]) || a[5] == 0)) ||
            (n > 7 && !num(f[7], 10, 60000000UL, &a[6])))
            FAIL(MDIO_SCRIPT_E_SYNTAX, "%s:%lu: bad arguments for '%c' (clause 45 only: dev 1..1f)", path, ln, op);
        s->lines++;
        /* The verification section is the assertions after the last other operation. */
        check = (op == 'P' || op == 'N') && a[5] == 1;
        if (check) {
            if (!vcand) vcand = ln;
            vchecks++;
        } else {
            vcand = vchecks = 0;
        }
        if (op == 'W') {
            s->writes++;
            if (io && io->write(io->ctx, (unsigned)a[0], (unsigned)a[1], (unsigned)a[2], (unsigned)a[3]) != 0) {
                AT(0, 0, 0);
                FAIL(MDIO_SCRIPT_E_IO, "%s:%lu: write %lx %lx.%04lx failed [%s]", path, ln, a[0], a[1], a[2], label);
            }
            s->ops++;
            continue;
        }
        if (op == 'R' || op == 'E') {
            unsigned v = 0;
            s->reads++;
            if (!io) {
                s->ops++;
                continue;
            }
            if (io->read(io->ctx, (unsigned)a[0], (unsigned)a[1], (unsigned)a[2], &v) != 0) {
                AT(0, 0, 0);
                FAIL(MDIO_SCRIPT_E_IO, "%s:%lu: read %lx %lx.%04lx failed [%s]", path, ln, a[0], a[1], a[2], label);
            }
            if (op == 'E' && (v & a[4]) != a[3]) {
                if (s->differs++ < MAX_DIFFER_LOG)
                    logf_(io, "differs: line %lu: %lx %lx.%04lx = %04x, script %04lx mask %04lx [%s]", ln, a[0], a[1],
                          a[2], v, a[3], a[4], label);
            }
            s->ops++;
            continue;
        }
        /* P / N */
        s->polls++;
        s->reads++;
        if (io && check && vline && ln >= vline) {
            /* Verification: read, report, go on. */
            mdio_script_check_t ck = {ln, op, (unsigned)a[0], (unsigned)a[1], (unsigned)a[2], (unsigned)a[3],
                                      (unsigned)a[4], 0, 0, 0, label};
            ck.io_error = io->read(io->ctx, ck.phy, ck.dev, ck.reg, &ck.got) != 0;
            if (ck.io_error) ck.got = 0;
            ck.ok = !ck.io_error && (op == 'P' ? (ck.got & ck.mask) == ck.want : (ck.got & ck.mask) != ck.want);
            s->verify_checks++;
            if (!ck.ok) {
                if (!s->verify_failed++) {
                    AT(ck.got, !ck.io_error, 1);
                    if (!ck.io_error)
                        snprintf(first_bad, sizeof(first_bad),
                                 "line %lu: phy %x %x.%04x reads %04x, script wants %s%04x (mask %04x) [%.60s]", ln, ck.phy,
                                 ck.dev, ck.reg, ck.got, op == 'P' ? "" : "anything but ", ck.want, ck.mask, label);
                    else
                        snprintf(first_bad, sizeof(first_bad), "line %lu: read of phy %x %x.%04x failed [%.60s]", ln,
                                 ck.phy, ck.dev, ck.reg, label);
                }
            }
            if (io->verify) io->verify(io->ctx, &ck);
            s->ops++;
            continue;
        }
        if (io) {
            unsigned long t;
            unsigned v = 0;
            int ok = 0;
            for (t = 0; t < a[5]; t++) {
                if (t) {
                    if (sleep_us(io, a[6]) || (io->tick && io->tick(io->ctx))) {
                        AT(v, 1, t);
                        FAIL(MDIO_SCRIPT_E_ABORT, "%s:%lu: stopped on request while polling phy %lx %lx.%04lx [%s]", path,
                             ln, a[0], a[1], a[2], label);
                    }
                    s->poll_rereads++;
                    s->reads++;
                }
                if (io->read(io->ctx, (unsigned)a[0], (unsigned)a[1], (unsigned)a[2], &v) != 0) {
                    AT(0, 0, t);
                    FAIL(MDIO_SCRIPT_E_IO, "%s:%lu: read %lx %lx.%04lx failed [%s]", path, ln, a[0], a[1], a[2], label);
                }
                ok = op == 'P' ? (v & a[4]) == a[3] : (v & a[4]) != a[3];
                if (ok) break;
            }
            if (!ok) {
                AT(v, 1, a[5]);
                FAIL(MDIO_SCRIPT_E_POLL, "%s:%lu: phy %lx %lx.%04lx reads %04x, script wants %s%04lx (mask %04lx) "
                     "after %lu reads [%s]", path, ln, a[0], a[1], a[2], v, op == 'P' ? "" : "anything but ", a[3],
                     a[4], a[5], label);
            }
        }
        s->ops++;
    }
    if (ferror(fp)) FAIL(MDIO_SCRIPT_E_FILE, "%s: read error", path);
    if (ln == 0) FAIL(MDIO_SCRIPT_E_SYNTAX, "%s: empty file", path);
    if (!io_all) {
        /* Parse pass: where the verification section starts. */
        s->verify_line = vchecks ? vcand : 0;
        s->verify_checks = vchecks;
    } else {
        s->verify_line = vline;
        if (s->verify_failed)
            FAIL(MDIO_SCRIPT_E_VERIFY, "%s: %lu of %lu verification reads are not what the script expects; first: %s",
                 path, s->verify_failed, s->verify_checks, first_bad);
    }
out:
    if (fp) fclose(fp);
    return rv;
#undef FAIL
#undef AT
}
