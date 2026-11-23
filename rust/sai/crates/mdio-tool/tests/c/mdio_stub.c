/* SPDX-License-Identifier: Apache-2.0
 * Test fixture of mdio-tool's differential (tests/differential.rs): the MDIO side of
 * sai-openbcm's sdk_glue.h (ob_sdk_init_mdio, ob_sdk_mdio_c45_read / _write, ob_sdk_temperatures,
 * ob_sdk_errmsg, and an ob_sdk_shutdown neither tool calls) over an in-memory register map, so the C openbcm-mdio-script can be built with
 * -DOB_HAVE_SDK without the SDK and compared with the Rust tool on sdk-glue's Rust fake. It is the
 * MDIO part of the former C fake of the glue (sdk-glue-sys/fake/fake_glue.c, deleted at G2) and
 * must answer as the Rust fake (sdk-glue/src/rust_fake.rs) does: same scenario directives, codes,
 * messages and trace lines; the differential is what holds them together. Built by
 * mdio-tool/build.rs, never part of a product.
 *
 * The scenario is the "config file" given to ob_sdk_init_mdio. One directive per line, '#'
 * comments, numbers hex unless marked decimal; an unknown line fails the init (OB_E_CONFIG):
 *   name <text>                        chip name in the init line (default FAKE56960_A0)
 *   init <busy|unit|soc|misc|delay|divisor>
 *                                      ob_sdk_init_mdio fails that way, with the real glue's text
 *   reg <phy> <dev> <reg> <val>        a register's value (default 0; a write is read back)
 *   const <phy> <val>                  the PHY reads <val> on every register (hung)
 *   dead <phy>                         every access to the PHY fails (OB_E_TIMEOUT)
 *   after <n, decimal> <phy> <dev> <reg> <val>
 *                                      the register reads <val> from its (n+1)th read on
 *   hang <n, decimal> <phy> <val>      once n writes were made (to any PHY), the PHY reads <val> everywhere
 *   fail-read <n, decimal>             the n-th read fails (OB_E_FAIL)
 *   fail-write <n, decimal>            the n-th write fails (OB_E_FAIL)
 *   temp <tenths of a degree, decimal>...   the monitors (none: OB_E_UNAVAIL); peak = current + 15
 *   temp-fail <n, decimal>             temperature reads from the n-th on fail (OB_E_TIMEOUT)
 *   sigterm <n, decimal>               SIGTERM to this process right after the n-th write
 *   trace <path>                       every call is appended to that file as one line
 * As in the real glue: no MDIO access before a successful init (OB_E_INIT), clause 45 only
 * (dev 1..31, OB_E_PARAM otherwise), the real glue's range checks (unit 0, phy <= 0x7ff, reg and
 * val <= 0xffff), state for the life of the process. The Rust fake's switch directives (ports,
 * port, cpu, link, fail, knet, linkscan) are not lines of this stub. */
#include <errno.h>
#include <signal.h>
#include <stdarg.h>
#include <stdio.h>
#include <string.h>
#include <unistd.h>

#include "sdk_glue.h"

#define MAX_REGS 8192
#define MAX_RULES 64
#define MAX_MON 16

static struct { unsigned phy, dev, reg, val; } regs[MAX_REGS];
static int nregs;
static struct { unsigned phy, val; } consts[MAX_RULES];
static int nconsts;
static unsigned dead[MAX_RULES];
static int ndead;
static struct { unsigned long n, seen; unsigned phy, dev, reg, val; } afters[MAX_RULES];
static int nafters;
static struct { unsigned long n; unsigned phy, val; } hangs[MAX_RULES];
static int nhangs;
static unsigned long fail_read, fail_write, temp_fail, sigterm_at;
static unsigned long nreads, nwrites, ntempcalls;
static int temps[MAX_MON], ntemps;
static char chip[32], init_fail[16];
static FILE* trace;
static int ready;

static void tr(const char* fmt, ...) __attribute__((format(printf, 1, 2)));
static void tr(const char* fmt, ...)
{
    va_list ap;
    if (!trace) return;
    va_start(ap, fmt);
    vfprintf(trace, fmt, ap);
    va_end(ap);
    fputc('\n', trace);
    fflush(trace);
}

static void reset(void)
{
    nregs = nconsts = ndead = nafters = nhangs = ntemps = 0;
    fail_read = fail_write = temp_fail = sigterm_at = 0;
    nreads = nwrites = ntempcalls = 0;
    snprintf(chip, sizeof(chip), "FAKE56960_A0");
    init_fail[0] = 0;
    if (trace) fclose(trace);
    trace = NULL;
    ready = 0;
}

static int set_reg(unsigned phy, unsigned dev, unsigned reg, unsigned val)
{
    int i;
    for (i = 0; i < nregs; i++)
        if (regs[i].phy == phy && regs[i].dev == dev && regs[i].reg == reg) break;
    if (i == MAX_REGS) return -1;
    if (i == nregs) nregs++;
    regs[i].phy = phy;
    regs[i].dev = dev;
    regs[i].reg = reg;
    regs[i].val = val;
    return 0;
}

static int is_dead(unsigned phy)
{
    int i;
    for (i = 0; i < ndead; i++)
        if (dead[i] == phy) return 1;
    return 0;
}

/* 0, or the number of the bad line. */
static int load(const char* path)
{
    char line[512], word[256];
    unsigned a, b, c, d;
    unsigned long n;
    int ln = 0;
    FILE* f = fopen(path, "r");
    if (!f) return -1;
    while (fgets(line, sizeof(line), f)) {
        char* p = line;
        int used = 0;
        ln++;
        line[strcspn(line, "\r\n")] = 0;
        while (*p == ' ' || *p == '\t') p++;
        if (!*p || *p == '#') continue;
        if (sscanf(p, "name %31s", word) == 1) snprintf(chip, sizeof(chip), "%.31s", word);
        else if (sscanf(p, "init %15s", word) == 1) snprintf(init_fail, sizeof(init_fail), "%.15s", word);
        else if (sscanf(p, "reg %x %x %x %x", &a, &b, &c, &d) == 4) {
            if (set_reg(a, b, c, d) != 0) goto bad;
        } else if (sscanf(p, "const %x %x", &a, &b) == 2 && nconsts < MAX_RULES) {
            consts[nconsts].phy = a;
            consts[nconsts++].val = b;
        } else if (sscanf(p, "dead %x", &a) == 1 && ndead < MAX_RULES) {
            dead[ndead++] = a;
        } else if (sscanf(p, "after %lu %x %x %x %x", &n, &a, &b, &c, &d) == 5 && nafters < MAX_RULES) {
            afters[nafters].n = n;
            afters[nafters].seen = 0;
            afters[nafters].phy = a;
            afters[nafters].dev = b;
            afters[nafters].reg = c;
            afters[nafters++].val = d;
        } else if (sscanf(p, "hang %lu %x %x", &n, &a, &b) == 3 && nhangs < MAX_RULES) {
            hangs[nhangs].n = n;
            hangs[nhangs].phy = a;
            hangs[nhangs++].val = b;
        } else if (sscanf(p, "fail-read %lu", &n) == 1) fail_read = n;
        else if (sscanf(p, "fail-write %lu", &n) == 1) fail_write = n;
        else if (sscanf(p, "temp-fail %lu", &n) == 1) temp_fail = n;
        else if (sscanf(p, "sigterm %lu", &n) == 1) sigterm_at = n;
        else if (sscanf(p, "trace %255s", word) == 1) {
            if (trace) fclose(trace);
            trace = fopen(word, "a");
            if (!trace) goto bad;
        } else if (strncmp(p, "temp ", 5) == 0) {
            int t;
            p += 5;
            ntemps = 0;
            while (ntemps < MAX_MON && sscanf(p, "%d%n", &t, &used) == 1) {
                temps[ntemps++] = t;
                p += used;
            }
            while (*p == ' ' || *p == '\t') p++;
            if (*p || !ntemps) goto bad;
        } else {
            goto bad;
        }
    }
    fclose(f);
    return 0;
bad:
    fclose(f);
    return ln;
}

int ob_sdk_init_mdio(int unit, const char* config_file, char* err, int errlen)
{
    int ln;
    char none[1];
    if (!err || errlen <= 0) {
        err = none;
        errlen = 1;
    }
    reset();
    if (!config_file || !*config_file || access(config_file, R_OK) != 0) {
        snprintf(err, (size_t)errlen, "SDK config file '%s' missing or unreadable (%d)", config_file ? config_file : "", errno);
        return OB_E_CONFIG;
    }
    ln = load(config_file);
    if (ln != 0) {
        snprintf(err, (size_t)errlen, "fake glue: %s:%d: not a scenario line", config_file, ln);
        return OB_E_CONFIG;
    }
    tr("init unit %d", unit);
    if (!strcmp(init_fail, "busy")) {
        snprintf(err, (size_t)errlen, "another SDK process owns the switch (lock /run/openbcm-unit%d.lock held; "
                 "e.g. systemctl stop openbcm-asic-temp)", unit);
        return OB_E_BUSY;
    }
    if (unit != 0 || !strcmp(init_fail, "unit")) {
        snprintf(err, (size_t)errlen, "no attached switch unit (soc_ndev %d)", 1);
        return OB_E_UNIT;
    }
    if (!strcmp(init_fail, "soc") || !strcmp(init_fail, "misc")) {
        snprintf(err, (size_t)errlen, "init %s failed (%d)", init_fail, -1);
        return OB_E_FAIL;
    }
    if (!strcmp(init_fail, "delay")) {
        snprintf(err, (size_t)errlen, "MDIO hold delay on the chip is %d, the config's mdio_output_delay is %d: "
                 "refusing MDIO access", 2, 13);
        return OB_E_MDIO_CLOCK;
    }
    if (!strcmp(init_fail, "divisor")) {
        snprintf(err, (size_t)errlen, "external MDIO clock divisor on the chip is %d, the config's "
                 "rate_ext_mdio_divisor is %d: refusing MDIO access", 1, 2);
        return OB_E_MDIO_CLOCK;
    }
    if (init_fail[0]) {
        snprintf(err, (size_t)errlen, "fake glue: unknown init failure '%s'", init_fail);
        return OB_E_CONFIG;
    }
    ready = 1;
    snprintf(err, (size_t)errlen, "%s: init soc + init misc done; external MDIO rate register 0x%08x, hold delay %d%s",
             chip, 0x000d0001u, 13, " (as configured)");
    return OB_E_NONE;
}

int ob_sdk_mdio_c45_read(int unit, unsigned phy, unsigned dev, unsigned reg, unsigned* val)
{
    unsigned v = 0;
    int i;
    if (!ready) return OB_E_INIT;
    if (unit != 0) return OB_E_UNIT;
    if (phy > 0x7ff || dev < 1 || dev > 31 || reg > 0xffff) return OB_E_PARAM;
    nreads++;
    if ((fail_read && nreads == fail_read) || is_dead(phy)) {
        tr("R %x %x %x ERR", phy, dev, reg);
        *val = 0;
        return is_dead(phy) ? OB_E_TIMEOUT : OB_E_FAIL;
    }
    for (i = 0; i < nregs; i++)
        if (regs[i].phy == phy && regs[i].dev == dev && regs[i].reg == reg) v = regs[i].val;
    for (i = 0; i < nafters; i++)
        if (afters[i].phy == phy && afters[i].dev == dev && afters[i].reg == reg && afters[i].seen++ >= afters[i].n)
            v = afters[i].val;
    for (i = 0; i < nconsts; i++)
        if (consts[i].phy == phy) v = consts[i].val;
    for (i = 0; i < nhangs; i++)
        if (hangs[i].phy == phy && nwrites >= hangs[i].n) v = hangs[i].val;
    *val = v & 0xffff;
    tr("R %x %x %x = %04x", phy, dev, reg, *val);
    return OB_E_NONE;
}

int ob_sdk_mdio_c45_write(int unit, unsigned phy, unsigned dev, unsigned reg, unsigned val)
{
    if (!ready) return OB_E_INIT;
    if (unit != 0) return OB_E_UNIT;
    if (phy > 0x7ff || dev < 1 || dev > 31 || reg > 0xffff || val > 0xffff) return OB_E_PARAM;
    nwrites++;
    if ((fail_write && nwrites == fail_write) || is_dead(phy) || set_reg(phy, dev, reg, val) != 0) {
        tr("W %x %x %x %x ERR", phy, dev, reg, val);
        return is_dead(phy) ? OB_E_TIMEOUT : OB_E_FAIL;
    }
    tr("W %x %x %x %x", phy, dev, reg, val);
    if (sigterm_at && nwrites == sigterm_at) raise(SIGTERM);
    return OB_E_NONE;
}

int ob_sdk_temperatures(int unit, int* cur, int* peak, int max, int* count)
{
    int i;
    *count = 0;
    if (max > MAX_MON) max = MAX_MON;
    if (max <= 0) return OB_E_PARAM;
    if (!ready || unit != 0) return OB_E_UNIT;
    ntempcalls++;
    if (temp_fail && ntempcalls >= temp_fail) {
        tr("T ERR");
        return OB_E_TIMEOUT;
    }
    if (!ntemps) return OB_E_UNAVAIL;
    for (i = 0; i < ntemps && i < max; i++) {
        cur[i] = temps[i];
        peak[i] = temps[i] + 15;
    }
    *count = i;
    tr("T %d", i);
    return OB_E_NONE;
}

/* Not called by either tool after ob_sdk_init_mdio (no BCM layer to shut down); here so a Rust
 * tool built with the real-glue module links against this stub (its Switch drop references it).
 * A call would show in the trace, where the differential would see it. */
void ob_sdk_shutdown(int unit)
{
    tr("shutdown unit %d", unit);
}

const char* ob_sdk_errmsg(int rv)
{
    /* The SDK's _shr_errmsg. */
    static const char* const msg[] = {
        "Ok", "Internal error", "Out of memory", "Invalid unit", "Invalid parameter", "Table empty", "Table full",
        "Entry not found", "Entry exists", "Operation timed out", "Operation still running", "Operation failed",
        "Operation disabled", "Invalid identifier", "No resources for operation", "Invalid configuration",
        "Feature unavailable", "Feature not initialized", "Invalid port", "Unknown error"};
    if (rv == OB_E_MDIO_CLOCK) return "MDIO clock setup not as configured";
    return msg[rv <= 0 && rv > -19 ? -rv : 19];
}
