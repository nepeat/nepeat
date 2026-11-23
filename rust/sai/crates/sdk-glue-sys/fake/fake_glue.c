/* SPDX-License-Identifier: Apache-2.0
 * A fake of sai-openbcm's sdk_glue.c for tests: the functions sdk-glue-sys declares, over an
 * in-memory register map, no SDK and no hardware. Linked by the feature "fake" of
 * sdk-glue-sys; the C openbcm-mdio-script can be linked with it too (-DOB_HAVE_SDK), which
 * is how the Rust tool is compared with it on the path that needs the SDK.
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
 * (dev 1..31, OB_E_PARAM otherwise), state for the life of the process. */
#include <errno.h>
#include <signal.h>
#include <stdarg.h>
#include <stdio.h>
#include <string.h>
#include <unistd.h>

#include "ob_sdk_glue_decls.h"

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

static void sw_reset(void);                  /* the switch part, at the end of this file */
static int sw_directive(const char* p);
static void sw_shutdown(int unit);

static void reset(void)
{
    sw_reset();
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
        } else if (sw_directive(p) != 1) {
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
    (void)unit;
    if (!ready) return OB_E_INIT;
    if (dev < 1 || dev > 31 || reg > 0xffff) return OB_E_PARAM;
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
    (void)unit;
    if (!ready) return OB_E_INIT;
    if (dev < 1 || dev > 31 || reg > 0xffff || val > 0xffff) return OB_E_PARAM;
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
    (void)unit;
    *count = 0;
    if (max > MAX_MON) max = MAX_MON;
    if (max <= 0) return OB_E_PARAM;
    if (!ready) return OB_E_UNIT;
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

void ob_sdk_shutdown(int unit)
{
    sw_shutdown(unit);   /* after ob_sdk_init: unregisters the callbacks, as the real glue */
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

/* ---- The full switch bring-up (ob_sdk_init) and what a SAI backend needs: ports, VLANs, the
 * link callback. Additional scenario directives (decimal numbers):
 *   ports <n> <lanes> <speed>          n front-panel ports: logical 1..n, first lane 1 + i * lanes, names ce0..
 *   port <logical> <phys> <lanes> <speed> <name>    one front-panel port
 *   cpu <logical>                      the CPU port (default 0)
 *   link <admin|none>                  admin: enabling a port brings its link up (callback from the
 *                                      enabling thread); none (default): only ob_fake_link_event does
 *   init <busy|unit|all|rc>            ob_sdk_init fails that way, with the real glue's text
 *   fail <op> <n> <code>               the n-th call of <op> returns <code> (an OB_E_* number, e.g. -11).
 *                                      ops: register hw-info ports enable link-get speed-set speed-get speeds
 *                                      fec autoneg interface ifg vlan-create vlan-destroy vlan-add
 *                                      vlan-remove vlan-members
 * After ob_sdk_init, as after "init all": every port enabled, link down, VLAN 1 with every
 * front-panel port untagged; MDIO access is allowed too. Not in sdk_glue.h, for tests:
 *   ob_fake_link_event(port, up)       a cable event: sets the port's link and calls the registered
 *                                      link callback on the calling thread (any thread); 1 if called
 *   ob_fake_query(what, arg)           0 link callback registered, 1 BCM layer up (init done, not shut
 *                                      down), 2 shutdown calls, 3 unregister calls (shutdown included),
 *                                      4 port <arg> enabled (-1: no such port), 5 register calls,
 *                                      6 port <arg> autoneg (-1 never set), 7 fec mode, 8 ifg bits,
 *                                      9 interface set (1) or not (0)
 * The callback is called with a lock held that register / unregister take, as the SDK's link
 * scan does: after ob_sdk_unregister_callbacks returns, no call is in progress. */
#include <pthread.h>

#define SW_MAX_PORTS 256
#define SW_MAX_FAILS 32
int ob_fake_link_event(int port, int up);
int ob_fake_query(int what, int arg);

static struct {
    int port, phys, lanes, speed, enabled, link, autoneg, fec, ifg, intf;
    char name[16];
} sw_ports[SW_MAX_PORTS];
static int sw_nports, sw_cpu, sw_link_admin, sw_inited, sw_shutdowns, sw_unregisters, sw_registers;
static unsigned char sw_vlan[4096];                 /* exists */
static unsigned char sw_member[4096][SW_MAX_PORTS]; /* by index into sw_ports: 0 no, 1 tagged, 2 untagged */
static struct { char op[20]; unsigned long n, seen; int code; } sw_fails[SW_MAX_FAILS];
static int sw_nfails;
static pthread_mutex_t sw_cb_mu = PTHREAD_MUTEX_INITIALIZER;
static ob_link_cb_t sw_link_cb;
static void* sw_cb_ctx;

static void sw_reset(void)
{
    pthread_mutex_lock(&sw_cb_mu);
    sw_link_cb = NULL;
    sw_cb_ctx = NULL;
    pthread_mutex_unlock(&sw_cb_mu);
    sw_nports = sw_cpu = sw_link_admin = sw_inited = sw_shutdowns = sw_unregisters = sw_registers = sw_nfails = 0;
    memset(sw_vlan, 0, sizeof(sw_vlan));
    memset(sw_member, 0, sizeof(sw_member));
}

static int sw_add_port(int port, int phys, int lanes, int speed, const char* name)
{
    int i;
    if (sw_nports >= SW_MAX_PORTS || port < 0 || lanes < 1) return -1;
    for (i = 0; i < sw_nports; i++)
        if (sw_ports[i].port == port) return -1;
    memset(&sw_ports[sw_nports], 0, sizeof(sw_ports[0]));
    sw_ports[sw_nports].port = port;
    sw_ports[sw_nports].phys = phys;
    sw_ports[sw_nports].lanes = lanes;
    sw_ports[sw_nports].speed = speed;
    sw_ports[sw_nports].autoneg = -1;
    sw_ports[sw_nports].fec = -1;
    sw_ports[sw_nports].ifg = -1;
    snprintf(sw_ports[sw_nports].name, sizeof(sw_ports[0].name), "%s", name);
    sw_nports++;
    return 0;
}

/* 1: the line was one of the switch directives; 0: not one (or a malformed one). */
static int sw_directive(const char* p)
{
    char word[32], name[16], opname[20];
    int a, b, c, d;
    unsigned long n;
    if (sscanf(p, "ports %d %d %d", &a, &b, &c) == 3) {
        int i;
        for (i = 0; i < a; i++) {
            snprintf(name, sizeof(name), "ce%d", i);
            if (sw_add_port(i + 1, 1 + i * b, b, c, name) != 0) return 0;
        }
        return a > 0;
    }
    if (sscanf(p, "port %d %d %d %d %15s", &a, &b, &c, &d, name) == 5) return sw_add_port(a, b, c, d, name) == 0;
    if (sscanf(p, "cpu %d", &a) == 1) {
        sw_cpu = a;
        return 1;
    }
    if (sscanf(p, "link %31s", word) == 1) {
        if (strcmp(word, "admin") != 0 && strcmp(word, "none") != 0) return 0;
        sw_link_admin = word[0] == 'a';
        return 1;
    }
    if (sscanf(p, "fail %19s %lu %d", opname, &n, &a) == 3 && sw_nfails < SW_MAX_FAILS && n > 0 && a < 0) {
        snprintf(sw_fails[sw_nfails].op, sizeof(sw_fails[0].op), "%s", opname);
        sw_fails[sw_nfails].n = n;
        sw_fails[sw_nfails].seen = 0;
        sw_fails[sw_nfails++].code = a;
        return 1;
    }
    return 0;
}

/* The scripted failure of this call of op, or 0. */
static int sw_fail(const char* op)
{
    int i, rv = 0;
    for (i = 0; i < sw_nfails; i++)
        if (strcmp(sw_fails[i].op, op) == 0 && ++sw_fails[i].seen == sw_fails[i].n) rv = sw_fails[i].code;
    if (rv) tr("%s ERR %d", op, rv);
    return rv;
}

static int sw_index(int port)
{
    int i;
    for (i = 0; i < sw_nports; i++)
        if (sw_ports[i].port == port) return i;
    return -1;
}

/* Sets a port's link; on a change the registered callback runs, on this thread. 1 if it ran. */
static int sw_set_link(int i, int up)
{
    int called = 0;
    pthread_mutex_lock(&sw_cb_mu);
    if (sw_ports[i].link != up) {
        sw_ports[i].link = up;
        if (sw_link_cb) {
            sw_link_cb(sw_cb_ctx, sw_ports[i].port, up);
            called = 1;
        }
    }
    pthread_mutex_unlock(&sw_cb_mu);
    return called;
}

int ob_fake_link_event(int port, int up)
{
    int i = sw_index(port);
    if (!sw_inited || i < 0) return 0;
    return sw_set_link(i, up != 0);
}

int ob_fake_query(int what, int arg)
{
    int i = sw_index(arg), registered;
    switch (what) {
    case 0:
        pthread_mutex_lock(&sw_cb_mu);
        registered = sw_link_cb != NULL;
        pthread_mutex_unlock(&sw_cb_mu);
        return registered;
    case 1: return sw_inited;
    case 2: return sw_shutdowns;
    case 3: return sw_unregisters;
    case 4: return i < 0 ? -1 : sw_ports[i].enabled;
    case 5: return sw_registers;
    case 6: return i < 0 ? -1 : sw_ports[i].autoneg;
    case 7: return i < 0 ? -1 : sw_ports[i].fec;
    case 8: return i < 0 ? -1 : sw_ports[i].ifg;
    case 9: return i < 0 ? -1 : sw_ports[i].intf;
    default: return -1;
    }
}

int ob_sdk_init(int unit, const char* config_file, const char* rc_file, char* err, int errlen)
{
    int ln, i;
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
    if (rc_file && *rc_file && access(rc_file, R_OK) != 0) {
        snprintf(err, (size_t)errlen, "rc file '%s' missing or unreadable (%d)", rc_file, errno);
        return OB_E_CONFIG;
    }
    ln = load(config_file);
    if (ln != 0) {
        snprintf(err, (size_t)errlen, "fake glue: %s:%d: not a scenario line", config_file, ln);
        return OB_E_CONFIG;
    }
    tr("init-all unit %d rc %s", unit, rc_file && *rc_file ? rc_file : "-");
    if (!strcmp(init_fail, "busy")) {
        snprintf(err, (size_t)errlen, "another SDK process owns the switch (lock /run/openbcm-unit%d.lock held; "
                 "e.g. systemctl stop openbcm-asic-temp)", unit);
        return OB_E_BUSY;
    }
    if (unit != 0 || !strcmp(init_fail, "unit")) {
        snprintf(err, (size_t)errlen, "no attached switch unit (soc_ndev %d)", 1);
        return OB_E_UNIT;
    }
    if (!strcmp(init_fail, "all")) {
        snprintf(err, (size_t)errlen, "init all failed: %d", OB_E_FAIL);
        return OB_E_FAIL;
    }
    if (!strcmp(init_fail, "rc")) {
        snprintf(err, (size_t)errlen, "rc file '%s' failed (%d)", rc_file ? rc_file : "", -1);
        return OB_E_FAIL;
    }
    if (init_fail[0]) {
        snprintf(err, (size_t)errlen, "fake glue: unknown init failure '%s'", init_fail);
        return OB_E_CONFIG;
    }
    sw_vlan[1] = 1;
    for (i = 0; i < sw_nports; i++) {
        sw_ports[i].enabled = 1;
        sw_member[1][i] = 2;
    }
    ready = 1;
    sw_inited = 1;
    snprintf(err, (size_t)errlen, "ok: stale KNET netifs %d filters %d destroyed; IPv4/IPv6 L3 on %d ports", 0, 0, sw_nports);
    return OB_E_NONE;
}

int ob_sdk_register_callbacks(int unit, ob_link_cb_t link, ob_l2_cb_t l2, void* ctx)
{
    int rv;
    (void)l2;
    if (!sw_inited) return OB_E_INIT;
    sw_registers++;
    tr("register unit %d", unit);
    /* As the real glue: the pointers are stored first, the SDK is asked second, and a refusal
     * leaves them stored. */
    pthread_mutex_lock(&sw_cb_mu);
    sw_link_cb = link;
    sw_cb_ctx = ctx;
    pthread_mutex_unlock(&sw_cb_mu);
    rv = sw_fail("register");
    return rv ? rv : OB_E_NONE;
}

void ob_sdk_unregister_callbacks(int unit)
{
    sw_unregisters++;
    tr("unregister unit %d", unit);
    pthread_mutex_lock(&sw_cb_mu);
    sw_link_cb = NULL;
    pthread_mutex_unlock(&sw_cb_mu);
}

/* From ob_sdk_shutdown. */
static void sw_shutdown(int unit)
{
    if (!sw_inited) return;
    ob_sdk_unregister_callbacks(unit);
    sw_shutdowns++;
    sw_inited = 0;
}

int ob_sdk_hw_info(int unit, char* buf, int len)
{
    int rv = sw_fail("hw-info");
    (void)unit;
    if (len > 0) snprintf(buf, (size_t)len, "%s", rv ? "" : chip);
    return rv ? rv : OB_E_NONE;
}

int ob_sdk_cpu_port(int unit)
{
    (void)unit;
    return sw_cpu;
}

int ob_sdk_ports(int unit, ob_port_info_t* out, int max)
{
    int i, rv = sw_fail("ports");
    (void)unit;
    if (!sw_inited) return OB_E_INIT;
    if (rv) return rv;
    for (i = 0; i < sw_nports && i < max; i++) {
        memset(&out[i], 0, sizeof(out[i]));
        out[i].port = sw_ports[i].port;
        out[i].phys = sw_ports[i].phys;
        out[i].lanes = sw_ports[i].lanes;
        out[i].speed = sw_ports[i].speed;
        snprintf(out[i].name, sizeof(out[i].name), "%s", sw_ports[i].name);
    }
    return i;
}

/* The checks every port call starts with: the port's index, or the error in *rv. */
static int sw_port_call(const char* op, int port, int* rv)
{
    int i = sw_index(port);
    *rv = OB_E_NONE;
    if (!sw_inited) *rv = OB_E_INIT;
    else if ((*rv = sw_fail(op)) != 0) return -1;
    else if (i < 0) *rv = OB_E_PORT;
    return *rv ? -1 : i;
}

int ob_sdk_port_enable_set(int unit, int port, int enable)
{
    int rv, i = sw_port_call("enable", port, &rv);
    (void)unit;
    if (i < 0) return rv;
    tr("port %d enable %d", port, enable != 0);
    sw_ports[i].enabled = enable != 0;
    if (!enable || sw_link_admin) sw_set_link(i, enable != 0);
    return OB_E_NONE;
}

int ob_sdk_port_link_get(int unit, int port, int* up)
{
    int rv, i = sw_port_call("link-get", port, &rv);
    (void)unit;
    *up = 0;
    if (i < 0) return rv;
    pthread_mutex_lock(&sw_cb_mu);
    *up = sw_ports[i].link;
    pthread_mutex_unlock(&sw_cb_mu);
    return OB_E_NONE;
}

static int sw_speeds(int lanes, uint32_t* out, int max)
{
    static const uint32_t four[] = {40000, 100000}, two[] = {20000, 50000}, one[] = {1000, 10000, 25000};
    const uint32_t* from = lanes >= 4 ? four : lanes >= 2 ? two : one;
    int count = lanes >= 4 ? 2 : lanes >= 2 ? 2 : 3, n;
    for (n = 0; n < count && n < max; n++) out[n] = from[n];
    return n;
}

int ob_sdk_port_speed_set(int unit, int port, int mbps)
{
    uint32_t can[4];
    int rv, n, k, i = sw_port_call("speed-set", port, &rv);
    (void)unit;
    if (i < 0) return rv;
    n = sw_speeds(sw_ports[i].lanes, can, 4);
    for (k = 0; k < n; k++)
        if ((int)can[k] == mbps) break;
    if (k == n) return OB_E_PARAM;
    tr("port %d speed %d", port, mbps);
    sw_ports[i].speed = mbps;
    return OB_E_NONE;
}

int ob_sdk_port_speed_get(int unit, int port, int* mbps)
{
    int rv, i = sw_port_call("speed-get", port, &rv);
    (void)unit;
    if (i < 0) return rv;
    *mbps = sw_ports[i].speed;
    return OB_E_NONE;
}

int ob_sdk_port_speeds(int unit, int port, uint32_t* out, int max)
{
    int rv, i = sw_port_call("speeds", port, &rv);
    (void)unit;
    if (i < 0) return rv;
    return sw_speeds(sw_ports[i].lanes, out, max);
}

int ob_sdk_port_fec_set(int unit, int port, int mode)
{
    int rv, i = sw_port_call("fec", port, &rv);
    (void)unit;
    if (i < 0) return rv;
    if (mode < 0 || mode > 2) return OB_E_PARAM;
    tr("port %d fec %d", port, mode);
    sw_ports[i].fec = mode;
    return OB_E_NONE;
}

int ob_sdk_port_autoneg_set(int unit, int port, int on)
{
    int rv, i = sw_port_call("autoneg", port, &rv);
    (void)unit;
    if (i < 0) return rv;
    tr("port %d autoneg %d", port, on != 0);
    sw_ports[i].autoneg = on != 0;
    return OB_E_NONE;
}

int ob_sdk_port_interface_set(int unit, int port, const char* name)
{
    /* The names of sdk_glue.c. */
    static const char* const names[] = {"caui", "caui4", "caui_c2c", "caui_c2m", "xlaui", "cr4", "sr4",
                                        "lr4", "kr4", "sfi", "xfi", "cr", "sr", "kr"};
    unsigned k;
    int rv, i = sw_port_call("interface", port, &rv);
    (void)unit;
    if (i < 0) return rv;
    for (k = 0; k < sizeof(names) / sizeof(names[0]); k++)
        if (strcmp(names[k], name) == 0) break;
    if (k == sizeof(names) / sizeof(names[0])) return OB_E_PARAM;
    tr("port %d interface %s", port, name);
    sw_ports[i].intf = 1;
    return OB_E_NONE;
}

int ob_sdk_port_ifg_set(int unit, int port, int bits)
{
    int rv, i = sw_port_call("ifg", port, &rv);
    (void)unit;
    if (i < 0) return rv;
    tr("port %d ifg %d", port, bits);
    sw_ports[i].ifg = bits;
    return OB_E_NONE;
}

static int sw_vlan_call(const char* op, int vid, int* rv)
{
    *rv = OB_E_NONE;
    if (!sw_inited) *rv = OB_E_INIT;
    else if ((*rv = sw_fail(op)) != 0) return 0;
    else if (vid < 1 || vid > 4094) *rv = OB_E_PARAM;
    return *rv == OB_E_NONE;
}

int ob_sdk_vlan_create(int unit, int vid)
{
    int rv;
    (void)unit;
    if (!sw_vlan_call("vlan-create", vid, &rv)) return rv;
    if (sw_vlan[vid]) return OB_E_EXISTS;
    tr("vlan %d create", vid);
    sw_vlan[vid] = 1;
    memset(sw_member[vid], 0, sizeof(sw_member[vid]));
    return OB_E_NONE;
}

int ob_sdk_vlan_destroy(int unit, int vid)
{
    int rv;
    (void)unit;
    if (!sw_vlan_call("vlan-destroy", vid, &rv)) return rv;
    if (!sw_vlan[vid]) return OB_E_NOT_FOUND;
    if (vid == 1) return OB_E_BADID;   /* the SDK refuses to destroy its default VLAN */
    tr("vlan %d destroy", vid);
    sw_vlan[vid] = 0;
    return OB_E_NONE;
}

int ob_sdk_vlan_port_add(int unit, int vid, int port, int untagged)
{
    int rv, i = sw_index(port);
    (void)unit;
    if (!sw_vlan_call("vlan-add", vid, &rv)) return rv;
    if (!sw_vlan[vid]) return OB_E_NOT_FOUND;
    if (i < 0) return OB_E_PORT;
    tr("vlan %d add %d %s", vid, port, untagged ? "untagged" : "tagged");
    sw_member[vid][i] = untagged ? 2 : 1;
    return OB_E_NONE;
}

int ob_sdk_vlan_port_remove(int unit, int vid, int port)
{
    int rv, i = sw_index(port);
    (void)unit;
    if (!sw_vlan_call("vlan-remove", vid, &rv)) return rv;
    if (!sw_vlan[vid]) return OB_E_NOT_FOUND;
    if (i < 0) return OB_E_PORT;
    tr("vlan %d remove %d", vid, port);
    sw_member[vid][i] = 0;   /* removing a non-member is no error (a port bitmap operation) */
    return OB_E_NONE;
}

int ob_sdk_vlan_members(int unit, int vid, int* ports, int* untagged, int max)
{
    int rv, i, n = 0;
    (void)unit;
    if (!sw_vlan_call("vlan-members", vid, &rv)) return rv;
    if (!sw_vlan[vid]) return OB_E_NOT_FOUND;
    for (i = 0; i < sw_nports && n < max; i++) {
        if (!sw_member[vid][i]) continue;
        ports[n] = sw_ports[i].port;
        untagged[n] = sw_member[vid][i] == 2;
        n++;
    }
    return n;
}
