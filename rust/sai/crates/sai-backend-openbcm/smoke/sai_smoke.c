/* SPDX-License-Identifier: Apache-2.0
 * sai-openbcm-smoke: the G2 hardware gate of the Rust libsai.so with the openbcm backend
 * (rust/sai/README.md "Plan", gate G2). A C program against the SAI headers that loads the
 * library with dlopen and drives it through the SAI C API only, as syncd would: the switch with
 * the board's own sai.profile, its ports, link through the external PHYs, L2, a LAG, a host
 * interface netdev, L3, ACL, a policer, counters, temperatures, the diag socket and one
 * read-only MDIO read through it, then an orderly teardown.
 *
 *   sai-openbcm-smoke --dry
 *   sai-openbcm-smoke --sonic-is-stopped --libsai <libsai.so> --profile <sai.profile>
 *                     --cabled L,L,... [options]          (--help: every option)
 *
 * CREATING THE SWITCH RESETS THE SWITCH CHIP ("init all"), and with the profile's external PHY
 * table the library runs that table's MDIO sequences on the PHYs. The program refuses to run
 * without --sonic-is-stopped, and refuses when the unit's SDK lock (/run/openbcm-unit<N>.lock)
 * is held by another process (syncd, openbcm-asic-temp, bcm.user, the PHY bring-up).
 *
 * What the program itself does on the bus: nothing but SAI calls, and (step mdio-read) one
 * clause-45 READ of device 1 through the library's diag socket, exactly as mdio_report.c sends
 * it ("phy raw c45 0x<phy> 0x1 0x<reg>"). It never writes MDIO and never uses clause 22. Every
 * PHY write comes from the profile's own SAI_OPENBCM_EXT_PHY_FILE / _EXT_MACSEC_FILE, run by the
 * library; --no-ext-phy serves the profile without those two keys (the library then does not
 * touch the PHYs, and link is the ASIC side's).
 *
 * Ports are named by their first hardware lane (HW_LANE_LIST[0], the SONiC lane, as the ext-PHY
 * table names them). Every step prints one line: "PASS <step> <values>", "FAIL <step> <why> |
 * <values>" or "SKIP <step> <why>"; the last line is the summary ("SMOKE_OK ..." or
 * "SMOKE_FAILED ..."). Exit 0: no step failed; 1: a step failed (after the same teardown);
 * 2: usage or refusal (nothing was loaded or touched). --dry lists the steps and exits 0 without
 * loading anything. Without arguments it prints the usage and exits 2 (the test bundle's probe). */
#define _GNU_SOURCE
#include <dlfcn.h>
#include <errno.h>
#include <fcntl.h>
#include <net/if.h>
#include <poll.h>
#include <pthread.h>
#include <stdarg.h>
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/file.h>
#include <sys/ioctl.h>
#include <sys/socket.h>
#include <sys/stat.h>
#include <sys/un.h>
#include <time.h>
#include <unistd.h>

#include <sai.h>

#define MAX_PORTS 512
#define MAX_KEYS 256
#define MAX_CABLED 64
#define MAX_LANES 16

/* ---------------------------------------------------------------- the steps */

/* In the order they run; --dry prints this list. */
static const struct {
    const char* name;
    const char* touches;
} STEPS[] = {
    {"load", "dlopen the library, sai_api_initialize, sai_api_query: nothing on the chip"},
    {"profile", "the sai.profile served through profile_get_value / profile_get_next_value: nothing on the chip"},
    {"create-switch", "RESETS THE SWITCH CHIP (\"init all\"), the rc file (LED programs), KNET cleanup; with the "
                      "profile's tables: the ext-PHY attach (MDIO reads), the MACsec table's probe and reset to "
                      "bypass (that table's MDIO writes); every port disabled with the ext-PHY table's admin-down "
                      "sequence (MDIO writes), the board quirks; the temperature publisher (the profile's files); "
                      "the diag socket"},
    {"ports", "PORT_NUMBER, PORT_LIST, per port HW_LANE_LIST / SPEED / ADMIN_STATE / OPER_STATUS: reads only"},
    {"admin-up", "the cabled ports admin up: port enable, the ext-PHY table's admin-up sequence (MDIO writes)"},
    {"oper-up", "waits for OPER_STATUS up and a PORT_STATE_CHANGE up per cabled port: the library's link scan with "
                "the ext-PHY table's link polls (MDIO reads)"},
    {"vlan", "VLAN <vid> with the first cabled port's bridge port as untagged member: VLAN table"},
    {"fdb", "a static FDB entry 00:00:5e:00:53:01 on that VLAN to that bridge port: L2 table"},
    {"lag", "an uncabled admin-down port leaves VLAN 1 and the .1Q bridge, LAG + member, read back, member and LAG "
            "removed, the bridge port and VLAN 1 member re-created: trunk table, VLAN 1"},
    {"hostif", "a NETDEV host interface on the first cabled port: KNET netif + RX filter (a kernel netdev)"},
    {"hostif-netdev", "/sys/class/net/<name> exists; IPv6 off on it, IFF_UP; its carrier follows the host interface's "
                      "OPER_STATUS, set from the port's oper status as orchagent does (/proc/bcm/knet/link)"},
    {"l3", "a port RIF on the last cabled port (internal VLAN, my-station entry 02:00:00:00:53:01), neighbour "
           "192.0.2.2, next hop, route 198.51.100.0/24: L3 interface, egress, host and route tables"},
    {"acl", "ACL table (ingress, DST_IP), counter, entry (dst 192.0.2.1, FORWARD, counted), table group bound to the "
            "first cabled port (INGRESS_ACL), member: a field group + entry; the counter read"},
    {"policer", "a packet-mode sr_TCM policer, read back, CIR changed: a meter"},
    {"port-stats", "get_port_stats on the cabled ports: counter reads"},
    {"temperature", "SWITCH TEMP_LIST / MAX_TEMP / AVERAGE_TEMP: the ASIC's temperature monitors"},
    {"diag", "one command (--diag-cmd, default \"ps\") through the library's diag socket (SAI_OPENBCM_DIAG_SOCKET): "
             "the SDK diag shell inside the library"},
    {"mdio-read", "through the diag socket \"phy raw c45 0x<phy> 0x1 0x<reg>\": ONE clause-45 READ, device 1, of "
                  "--mdio-phy / --mdio-reg (the ext-PHY polls pause around it)"},
    {"teardown", "everything above removed in reverse order (policer, ACL, L3, host interface + netdev, FDB, VLAN)"},
    {"admin-down", "the cabled ports admin down (the ext-PHY table's admin-down sequence: MDIO writes), oper down "
                   "awaited, every port checked admin down"},
    {"remove-switch", "remove_switch: callbacks unregistered, BCM layer shut down; the SDK stays attached (no "
                      "detach), the process keeps the SDK lock until it ends"},
    {"recreate", "only with --recreate: create_switch again in this process (RESETS THE CHIP AGAIN, \"init all\" "
                 "re-run on the attached SDK), ports counted, remove_switch"},
    {"uninitialize", "sai_api_uninitialize"},
};
#define NSTEPS ((int)(sizeof(STEPS) / sizeof(STEPS[0])))

static int n_pass, n_fail, n_skip;
static const char* cur;
static char vals[4096], why[4096];
static bool step_bad;

static void appendv(char* buf, size_t cap, const char* fmt, va_list ap)
{
    size_t at = strlen(buf);
    if (at + 1 >= cap) return;
    vsnprintf(buf + at, cap - at, fmt, ap);
}

/* A value the step's line shows. */
__attribute__((format(printf, 1, 2))) static void val(const char* fmt, ...)
{
    va_list ap;
    if (vals[0]) strncat(vals, " ", sizeof(vals) - strlen(vals) - 1);
    va_start(ap, fmt);
    appendv(vals, sizeof(vals), fmt, ap);
    va_end(ap);
}

/* The step fails; the reason joins its line. */
__attribute__((format(printf, 1, 2))) static void bad(const char* fmt, ...)
{
    va_list ap;
    step_bad = true;
    if (why[0]) strncat(why, "; ", sizeof(why) - strlen(why) - 1);
    va_start(ap, fmt);
    appendv(why, sizeof(why), fmt, ap);
    va_end(ap);
}

static void begin(const char* name)
{
    cur = name;
    vals[0] = why[0] = 0;
    step_bad = false;
}

/* Ends the step with its line; true when it passed. */
static bool end(void)
{
    if (step_bad) {
        n_fail++;
        printf("FAIL %s %s%s%s\n", cur, why, vals[0] ? " | " : "", vals);
    } else {
        n_pass++;
        printf("PASS %s %s\n", cur, vals);
    }
    fflush(stdout);
    return !step_bad;
}

__attribute__((format(printf, 2, 3))) static void skip(const char* name, const char* fmt, ...)
{
    va_list ap;
    n_skip++;
    printf("SKIP %s ", name);
    va_start(ap, fmt);
    vprintf(fmt, ap);
    va_end(ap);
    printf("\n");
    fflush(stdout);
}

static const char* status_name(sai_status_t st)
{
    static char other[32];
    if (st == SAI_STATUS_SUCCESS) return "SUCCESS";
    if (st == SAI_STATUS_FAILURE) return "FAILURE";
    if (st == SAI_STATUS_NOT_SUPPORTED) return "NOT_SUPPORTED";
    if (st == SAI_STATUS_NO_MEMORY) return "NO_MEMORY";
    if (st == SAI_STATUS_INSUFFICIENT_RESOURCES) return "INSUFFICIENT_RESOURCES";
    if (st == SAI_STATUS_INVALID_PARAMETER) return "INVALID_PARAMETER";
    if (st == SAI_STATUS_ITEM_ALREADY_EXISTS) return "ITEM_ALREADY_EXISTS";
    if (st == SAI_STATUS_ITEM_NOT_FOUND) return "ITEM_NOT_FOUND";
    if (st == SAI_STATUS_BUFFER_OVERFLOW) return "BUFFER_OVERFLOW";
    if (st == SAI_STATUS_UNINITIALIZED) return "UNINITIALIZED";
    if (st == SAI_STATUS_TABLE_FULL) return "TABLE_FULL";
    if (st == SAI_STATUS_MANDATORY_ATTRIBUTE_MISSING) return "MANDATORY_ATTRIBUTE_MISSING";
    if (st == SAI_STATUS_NOT_IMPLEMENTED) return "NOT_IMPLEMENTED";
    if (st == SAI_STATUS_OBJECT_IN_USE) return "OBJECT_IN_USE";
    if (st == SAI_STATUS_INVALID_OBJECT_TYPE) return "INVALID_OBJECT_TYPE";
    if (st == SAI_STATUS_INVALID_OBJECT_ID) return "INVALID_OBJECT_ID";
    snprintf(other, sizeof(other), "status %d", (int)st);
    return other;
}

/* A SAI call's status: true on success, else the step fails with "<what>: <status>". */
static bool ok(const char* what, sai_status_t st)
{
    if (st == SAI_STATUS_SUCCESS) return true;
    bad("%s: %s", what, status_name(st));
    return false;
}

static long long now_ms(void)
{
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return (long long)ts.tv_sec * 1000 + ts.tv_nsec / 1000000;
}

static void nap(long ms)
{
    struct timespec ts = {ms / 1000, (ms % 1000) * 1000000L};
    nanosleep(&ts, NULL);
}

/* ---------------------------------------------------------------- options */

static struct {
    const char* libsai;
    const char* profile;
    const char* backend;
    long expect_ports, expect_lanes, expect_speed;
    uint32_t cabled[MAX_CABLED];
    int ncabled;
    long lag_lane; /* -1: the first uncabled port */
    long vlan;
    long link_timeout, netdev_timeout, diag_timeout;
    bool no_ext_phy, no_kernel, recreate, log_stderr, dry, confirmed;
    long mdio_phy, mdio_reg;
    long mdio_expect; /* -1: no expectation */
    const char* diag_cmd;
    const char* hostif_name;
    const char* poke; /* fake glue only: a diag command sent while waiting for a link change */
} opt = {
    .backend = "openbcm",
    .expect_ports = 32,
    .expect_lanes = 4,
    .expect_speed = 100000,
    .lag_lane = -1,
    .vlan = 100,
    .link_timeout = 30,
    .netdev_timeout = 5,
    .diag_timeout = 10,
    .mdio_phy = 0x0,
    .mdio_reg = 0x8b00,
    .mdio_expect = -1,
    .diag_cmd = "ps",
    .hostif_name = "sai-smoke0",
};

static int usage(void)
{
    fprintf(stderr,
            "usage: sai-openbcm-smoke --dry\n"
            "       sai-openbcm-smoke --sonic-is-stopped --libsai <libsai.so> --profile <sai.profile> --cabled L,L,...\n"
            "         [--backend NAME] [--expect-ports N] [--expect-lanes N] [--expect-speed MBPS] [--lag-port L]\n"
            "         [--vlan VID] [--link-timeout S] [--netdev-timeout S] [--diag-timeout S] [--no-ext-phy]\n"
            "         [--no-kernel] [--mdio-phy 0xP] [--mdio-reg 0xR] [--mdio-expect 0xV] [--diag-cmd CMD]\n"
            "         [--hostif-name NAME] [--log-stderr] [--recreate] [--poke-diag CMD]\n"
            "Creating the switch RESETS THE SWITCH CHIP: stop SONiC and every SDK process first\n"
            "(systemctl stop sonic.target openbcm-asic-temp), then pass --sonic-is-stopped.\n"
            "Ports are named by their first hardware lane (L). --dry lists the steps and what each touches.\n"
            "  --profile       KEY=VALUE lines (the board's sai.profile); the program adds SAI_RS_BACKEND=NAME\n"
            "                  (--backend, default openbcm)\n"
            "  --expect-ports/-lanes/-speed  default 32 ports, 4 lanes each, 100000 Mb/s\n"
            "  --cabled        first lanes of the cabled ports: only these go admin up, and they must reach oper up\n"
            "  --lag-port      first lane of an uncabled port for the LAG step (default: the first uncabled port)\n"
            "  --vlan          the VLAN step's VLAN (default 100)\n"
            "  --link-timeout  seconds for oper up / down (default 30); --netdev-timeout for the kernel netdev\n"
            "                  (default 5); --diag-timeout for a diag socket answer (default 10)\n"
            "  --no-ext-phy    serve the profile without SAI_OPENBCM_EXT_PHY_FILE and SAI_OPENBCM_EXT_MACSEC_FILE\n"
            "  --no-kernel     SKIP the kernel netdev checks (the fake glue has no KNET); on hardware a missing\n"
            "                  netdev is a FAIL\n"
            "  --mdio-phy/-reg the PHY address word and the device-1 register of the read-only MDIO read (default\n"
            "                  0x0 0x8b00); --mdio-expect: the value it must read\n"
            "  --diag-cmd      the diag step's command (default ps); --hostif-name (default sai-smoke0)\n"
            "  --log-stderr    also serve SAI_OPENBCM_LOG_STDERR=1 (the library's log on stderr)\n"
            "  --recreate      a second create_switch / remove_switch in this process: resets the chip again;\n"
            "                  untested on hardware\n"
            "  --poke-diag     fake glue only: a diag command sent every 100 ms while waiting for a link change\n"
            "                  (the fake has no link scan thread: 'fake linkscan' runs one pass)\n");
    return 2;
}

static bool parse_num(const char* s, long lo, long hi, long* out)
{
    char* e;
    long v;
    errno = 0;
    v = strtol(s, &e, 0);
    if (errno || e == s || *e || v < lo || v > hi) return false;
    *out = v;
    return true;
}

static bool parse_lanes(const char* s)
{
    char buf[512];
    char* save = NULL;
    if (strlen(s) >= sizeof(buf)) return false;
    strcpy(buf, s);
    opt.ncabled = 0;
    for (char* t = strtok_r(buf, ",", &save); t; t = strtok_r(NULL, ",", &save)) {
        long v;
        if (opt.ncabled == MAX_CABLED || !parse_num(t, 0, 0xffff, &v)) return false;
        for (int i = 0; i < opt.ncabled; i++)
            if (opt.cabled[i] == (uint32_t)v) return false;
        opt.cabled[opt.ncabled++] = (uint32_t)v;
    }
    return opt.ncabled > 0;
}

/* 0 parsed, 2 usage error (message printed). */
static int parse_args(int argc, char** argv)
{
    for (int i = 1; i < argc; i++) {
        const char* a = argv[i];
        const char* v = i + 1 < argc ? argv[i + 1] : NULL;
#define NUM(flag, lo, hi, dst)                                           \
    if (!strcmp(a, flag)) {                                              \
        if (!v || !parse_num(v, lo, hi, &dst)) {                         \
            fprintf(stderr, "%s: a number %ld..%ld\n", flag, (long)(lo), (long)(hi)); \
            return 2;                                                    \
        }                                                                \
        i++;                                                             \
        continue;                                                        \
    }
#define STR(flag, dst)          \
    if (!strcmp(a, flag)) {     \
        if (!v) return usage(); \
        dst = v;                \
        i++;                    \
        continue;               \
    }
#define FLAG(flag, dst)     \
    if (!strcmp(a, flag)) { \
        dst = true;         \
        continue;           \
    }
        FLAG("--sonic-is-stopped", opt.confirmed)
        FLAG("--dry", opt.dry)
        FLAG("--no-ext-phy", opt.no_ext_phy)
        FLAG("--no-kernel", opt.no_kernel)
        FLAG("--recreate", opt.recreate)
        FLAG("--log-stderr", opt.log_stderr)
        STR("--libsai", opt.libsai)
        STR("--profile", opt.profile)
        STR("--backend", opt.backend)
        STR("--diag-cmd", opt.diag_cmd)
        STR("--hostif-name", opt.hostif_name)
        STR("--poke-diag", opt.poke)
        NUM("--expect-ports", 1, MAX_PORTS, opt.expect_ports)
        NUM("--expect-lanes", 1, MAX_LANES, opt.expect_lanes)
        NUM("--expect-speed", 1, 1600000, opt.expect_speed)
        NUM("--lag-port", 0, 0xffff, opt.lag_lane)
        NUM("--vlan", 2, 4094, opt.vlan)
        NUM("--link-timeout", 1, 600, opt.link_timeout)
        NUM("--netdev-timeout", 1, 120, opt.netdev_timeout)
        NUM("--diag-timeout", 1, 120, opt.diag_timeout)
        NUM("--mdio-phy", 0, 0x7ff, opt.mdio_phy)
        NUM("--mdio-reg", 0, 0xffff, opt.mdio_reg)
        NUM("--mdio-expect", 0, 0xffff, opt.mdio_expect)
#undef NUM
#undef STR
#undef FLAG
        if (!strcmp(a, "--cabled")) {
            if (!v || !parse_lanes(v)) {
                fprintf(stderr, "--cabled: first lanes, comma separated, distinct (at most %d)\n", MAX_CABLED);
                return 2;
            }
            i++;
            continue;
        }
        return usage();
    }
    if (opt.dry) return 0;
    if (!opt.libsai || !opt.profile || !opt.ncabled) return usage();
    size_t n = strlen(opt.hostif_name);
    if (n == 0 || n >= SAI_HOSTIF_NAME_SIZE || n >= IFNAMSIZ || strpbrk(opt.hostif_name, "/ \t:")) {
        fprintf(stderr, "--hostif-name: 1..%d characters, no '/', ':' or blanks\n", SAI_HOSTIF_NAME_SIZE - 1);
        return 2;
    }
    if (strchr(opt.diag_cmd, '\n') || (opt.poke && strchr(opt.poke, '\n'))) {
        fprintf(stderr, "--diag-cmd / --poke-diag: one line\n");
        return 2;
    }
    return 0;
}

static int dry(void)
{
    printf("sai-openbcm-smoke: the steps, in order (--dry: nothing is loaded or touched)\n");
    for (int i = 0; i < NSTEPS; i++) printf("%2d. %-14s %s\n", i + 1, STEPS[i].name, STEPS[i].touches);
    printf("Every step prints PASS / FAIL / SKIP <step> ...; the last line is SMOKE_OK or SMOKE_FAILED;\n"
           "exit 0 = no FAIL, 1 = a FAIL (after the teardown), 2 = usage or refusal.\n"
           "The program itself never writes MDIO and never uses clause 22; its only bus access is step mdio-read.\n");
    return 0;
}

/* ---------------------------------------------------------------- the profile */

static struct {
    char* key;
    char* value;
} profile[MAX_KEYS];
static int nprofile;
static int next_index;

/* The keys --no-ext-phy leaves out. */
static const char* const EXT_KEYS[] = {"SAI_OPENBCM_EXT_PHY_FILE", "SAI_OPENBCM_EXT_MACSEC_FILE"};
#define NEXT_KEYS (sizeof(EXT_KEYS) / sizeof(EXT_KEYS[0]))
/* The keys the program answers itself, whatever the file says. */
static const char* const OWN_KEYS[] = {"SAI_RS_BACKEND", "SAI_OPENBCM_LOG_STDERR"};
#define NOWN_KEYS (sizeof(OWN_KEYS) / sizeof(OWN_KEYS[0]))

static const char* own_value(const char* key)
{
    if (!strcmp(key, "SAI_RS_BACKEND")) return opt.backend;
    if (opt.log_stderr && !strcmp(key, "SAI_OPENBCM_LOG_STDERR")) return "1";
    return NULL;
}

static bool dropped(const char* key)
{
    if (!opt.no_ext_phy) return false;
    for (size_t i = 0; i < NEXT_KEYS; i++)
        if (!strcmp(key, EXT_KEYS[i])) return true;
    return false;
}

static const char* file_value(const char* key)
{
    for (int i = 0; i < nprofile; i++)
        if (!strcmp(profile[i].key, key)) return profile[i].value;
    return NULL;
}

static const char* profile_get_value(sai_switch_profile_id_t profile_id, const char* variable)
{
    const char* v;
    (void)profile_id;
    if (!variable) return NULL;
    if ((v = own_value(variable))) return v;
    if (dropped(variable)) return NULL;
    return file_value(variable);
}

/* The file's pairs as served, then the program's own keys; null pointers restart. */
static int profile_get_next_value(sai_switch_profile_id_t profile_id, const char** variable, const char** value)
{
    (void)profile_id;
    if (!variable || !value) {
        next_index = 0;
        return 0;
    }
    while (next_index < nprofile) {
        int i = next_index++;
        if (dropped(profile[i].key) || own_value(profile[i].key)) continue;
        *variable = profile[i].key;
        *value = profile[i].value;
        return 0;
    }
    while ((size_t)(next_index - nprofile) < NOWN_KEYS) {
        const char* k = OWN_KEYS[next_index++ - nprofile];
        if (own_value(k)) {
            *variable = k;
            *value = own_value(k);
            return 0;
        }
    }
    return -1;
}

static int load_profile(const char* path)
{
    char line[4096];
    FILE* f = fopen(path, "r");
    if (!f) {
        fprintf(stderr, "cannot read the profile %s: %s\n", path, strerror(errno));
        return -1;
    }
    while (fgets(line, sizeof(line), f)) {
        char* eq;
        line[strcspn(line, "\r\n")] = 0;
        if (!line[0] || line[0] == '#') continue;
        eq = strchr(line, '=');
        if (!eq || eq == line || nprofile == MAX_KEYS) {
            fprintf(stderr, "%s: not a KEY=VALUE line (or too many): %s\n", path, line);
            fclose(f);
            return -1;
        }
        *eq = 0;
        profile[nprofile].key = strdup(line);
        profile[nprofile].value = strdup(eq + 1);
        if (!profile[nprofile].key || !profile[nprofile].value) {
            fclose(f);
            return -1;
        }
        nprofile++;
    }
    fclose(f);
    return 0;
}

/* 0: nobody holds the unit's SDK lock (or there is no lock file: no SDK process ran since
 * boot). The lock is released again at once; the library takes it itself. */
static int sdk_lock_free(void)
{
    char path[64];
    const char* unit = profile_get_value(0, "SAI_OPENBCM_UNIT");
    int fd, busy;
    snprintf(path, sizeof(path), "/run/openbcm-unit%d.lock", unit ? atoi(unit) : 0);
    fd = open(path, O_RDWR | O_CLOEXEC);
    if (fd < 0) {
        if (errno == ENOENT) return 0;
        fprintf(stderr, "cannot open %s: %s\n", path, strerror(errno));
        return -1;
    }
    busy = flock(fd, LOCK_EX | LOCK_NB) != 0;
    close(fd); /* releases it */
    if (busy) {
        fprintf(stderr,
                "refusing: another SDK process holds %s (syncd? openbcm-asic-temp? bcm.user?).\n"
                "Stop it first: systemctl stop sonic.target openbcm-asic-temp\n",
                path);
        return -1;
    }
    return 0;
}

/* ---------------------------------------------------------------- state */

static struct {
    sai_switch_api_t* sw;
    sai_port_api_t* port;
    sai_vlan_api_t* vlan;
    sai_bridge_api_t* bridge;
    sai_fdb_api_t* fdb;
    sai_lag_api_t* lag;
    sai_hostif_api_t* hostif;
    sai_router_interface_api_t* rif;
    sai_neighbor_api_t* nbr;
    sai_next_hop_api_t* nh;
    sai_route_api_t* route;
    sai_acl_api_t* acl;
    sai_policer_api_t* policer;
} api;

static sai_object_id_t sw;
static sai_object_id_t ports[MAX_PORTS];
static uint32_t first_lane[MAX_PORTS];
static uint32_t nports;
static int cabled_idx[MAX_CABLED]; /* port indices of --cabled, in its order */
static int ncabled_found;

/* Link notifications, per port index. */
static pthread_mutex_t ev_lock = PTHREAD_MUTEX_INITIALIZER;
static int ev_up[MAX_PORTS], ev_down[MAX_PORTS], ev_total, ev_unknown;

static void on_port_state(uint32_t count, const sai_port_oper_status_notification_t* data)
{
    pthread_mutex_lock(&ev_lock);
    for (uint32_t i = 0; i < count; i++) {
        uint32_t p = 0;
        ev_total++;
        while (p < nports && ports[p] != data[i].port_id) p++;
        if (p == nports) {
            ev_unknown++;
            continue;
        }
        if (data[i].port_state == SAI_PORT_OPER_STATUS_UP) ev_up[p]++;
        else ev_down[p]++;
    }
    pthread_mutex_unlock(&ev_lock);
}

static int events_of(int idx, bool up)
{
    int n;
    pthread_mutex_lock(&ev_lock);
    n = up ? ev_up[idx] : ev_down[idx];
    pthread_mutex_unlock(&ev_lock);
    return n;
}

/* What teardown removes (null / false: nothing). */
static sai_object_id_t vlan, vlan_member, hostif, rif, next_hop, policer;
static sai_object_id_t acl_table, acl_counter, acl_entry, acl_group, acl_member;
static bool fdb_made, nbr_made, route_made, acl_bound, netdev_up;
static sai_fdb_entry_t fdb_key;
static sai_neighbor_entry_t nbr_key;
static sai_route_entry_t route_key;

static int find_lane(uint32_t lane)
{
    for (uint32_t i = 0; i < nports; i++)
        if (first_lane[i] == lane) return (int)i;
    return -1;
}

static bool port_oper_up(int idx, bool* up)
{
    sai_attribute_t a = {.id = SAI_PORT_ATTR_OPER_STATUS};
    if (api.port->get_port_attribute(ports[idx], 1, &a) != SAI_STATUS_SUCCESS) return false;
    *up = a.value.s32 == SAI_PORT_OPER_STATUS_UP;
    return true;
}

static bool port_admin(int idx, bool up)
{
    sai_attribute_t a = {.id = SAI_PORT_ATTR_ADMIN_STATE, .value.booldata = up};
    char what[64];
    snprintf(what, sizeof(what), "port %u admin %s", first_lane[idx], up ? "up" : "down");
    return ok(what, api.port->set_port_attribute(ports[idx], &a));
}

/* ---------------------------------------------------------------- the diag socket */

static const char* diag_socket(void)
{
    return profile_get_value(0, "SAI_OPENBCM_DIAG_SOCKET");
}

/* One command through the socket, the protocol of openbcm_diag.c (one line, then the answer up
 * to EOF): 0 with the answer in out, or -1 with the reason in err. */
static int diag(const char* cmd, char* out, size_t cap, char* err, size_t errcap)
{
    const char* path = diag_socket();
    struct sockaddr_un a;
    char line[1100];
    size_t len = 0;
    long long deadline = now_ms() + opt.diag_timeout * 1000;
    int fd, n;
    out[0] = 0;
    if (!path || !*path) {
        snprintf(err, errcap, "the profile has no SAI_OPENBCM_DIAG_SOCKET");
        return -1;
    }
    memset(&a, 0, sizeof(a));
    a.sun_family = AF_UNIX;
    if (strlen(path) >= sizeof(a.sun_path)) {
        snprintf(err, errcap, "%s: path too long", path);
        return -1;
    }
    strcpy(a.sun_path, path);
    fd = socket(AF_UNIX, SOCK_STREAM | SOCK_CLOEXEC, 0);
    if (fd < 0 || connect(fd, (struct sockaddr*)&a, sizeof(a)) != 0) {
        snprintf(err, errcap, "%s: %s", path, strerror(errno));
        if (fd >= 0) close(fd);
        return -1;
    }
    n = snprintf(line, sizeof(line), "%s\n", cmd);
    if (n < 0 || (size_t)n >= sizeof(line)) {
        snprintf(err, errcap, "command too long");
        close(fd);
        return -1;
    }
    if (write(fd, line, (size_t)n) != n) {
        snprintf(err, errcap, "%s: write: %s", path, strerror(errno));
        close(fd);
        return -1;
    }
    shutdown(fd, SHUT_WR);
    while (len + 1 < cap) {
        struct pollfd p = {.fd = fd, .events = POLLIN};
        long long left = deadline - now_ms();
        ssize_t got;
        if (left <= 0 || poll(&p, 1, (int)left) <= 0) {
            snprintf(err, errcap, "no complete answer within %ld s", opt.diag_timeout);
            close(fd);
            return -1;
        }
        got = read(fd, out + len, cap - 1 - len);
        if (got < 0 && errno == EINTR) continue;
        if (got <= 0) break;
        len += (size_t)got;
        out[len] = 0;
    }
    close(fd);
    return 0;
}

/* The first non-empty line of an answer, tabs as blanks, for a step's line. */
static const char* first_line(const char* s, char* buf, size_t cap)
{
    size_t n;
    while (*s == '\n' || *s == '\r') s++;
    n = strcspn(s, "\r\n");
    if (n >= cap) n = cap - 1;
    memcpy(buf, s, n);
    buf[n] = 0;
    for (char* p = buf; *p; p++)
        if (*p == '\t') *p = ' ';
    return buf;
}

/* ---------------------------------------------------------------- the kernel netdev */

static bool netdev_exists(const char* name)
{
    char path[128];
    struct stat st;
    snprintf(path, sizeof(path), "/sys/class/net/%s", name);
    return stat(path, &st) == 0;
}

/* The netdev's carrier: 1, 0, or -1 (unreadable: the netdev is down or gone). */
static int netdev_carrier(const char* name)
{
    char path[128], buf[8] = "";
    int fd, n;
    snprintf(path, sizeof(path), "/sys/class/net/%s/carrier", name);
    fd = open(path, O_RDONLY | O_CLOEXEC);
    if (fd < 0) return -1;
    n = (int)read(fd, buf, sizeof(buf) - 1);
    close(fd);
    if (n <= 0) return -1;
    return buf[0] == '1' ? 1 : buf[0] == '0' ? 0 : -1;
}

static bool netdev_set_up(const char* name, bool up)
{
    struct ifreq r;
    bool done = false;
    int fd = socket(AF_INET, SOCK_DGRAM | SOCK_CLOEXEC, 0);
    if (fd < 0) return false;
    memset(&r, 0, sizeof(r));
    memcpy(r.ifr_name, name, strlen(name) < IFNAMSIZ ? strlen(name) : IFNAMSIZ - 1);
    if (ioctl(fd, SIOCGIFFLAGS, &r) == 0) {
        if (up) r.ifr_flags |= IFF_UP;
        else r.ifr_flags &= ~IFF_UP;
        done = ioctl(fd, SIOCSIFFLAGS, &r) == 0;
    }
    close(fd);
    return done;
}

/* IPv6 off on the netdev before it goes up (no router solicitations out of the port). Best
 * effort: false when it could not be set. */
static bool netdev_no_ipv6(const char* name)
{
    char path[160];
    int fd;
    bool done;
    snprintf(path, sizeof(path), "/proc/sys/net/ipv6/conf/%s/disable_ipv6", name);
    fd = open(path, O_WRONLY | O_CLOEXEC);
    if (fd < 0) return false;
    done = write(fd, "1", 1) == 1;
    close(fd);
    return done;
}

static bool wait_for(bool (*cond)(const char*), const char* name, long seconds)
{
    long long deadline = now_ms() + seconds * 1000;
    for (;;) {
        if (cond(name)) return true;
        if (now_ms() >= deadline) return false;
        nap(50);
    }
}

static bool netdev_gone(const char* name) { return !netdev_exists(name); }
static bool carrier_is_1(const char* name) { return netdev_carrier(name) == 1; }
static bool carrier_is_0(const char* name) { return netdev_carrier(name) == 0; }

/* ---------------------------------------------------------------- steps */

static bool step_create_switch(void)
{
    sai_attribute_t a[2];
    long long t0;
    begin("create-switch");
    memset(a, 0, sizeof(a));
    a[0].id = SAI_SWITCH_ATTR_INIT_SWITCH;
    a[0].value.booldata = true;
    a[1].id = SAI_SWITCH_ATTR_PORT_STATE_CHANGE_NOTIFY;
    a[1].value.ptr = (void*)on_port_state;
    printf("creating the switch (backend %s): the switch chip is reset now\n", opt.backend);
    fflush(stdout);
    t0 = now_ms();
    sw = SAI_NULL_OBJECT_ID;
    if (ok("create_switch", api.sw->create_switch(&sw, 2, a))) {
        val("switch=0x%016llx ms=%lld", (unsigned long long)sw, now_ms() - t0);
    } else {
        sw = SAI_NULL_OBJECT_ID;
        bad("the library's own lines above say why");
    }
    return end();
}

static bool step_ports(void)
{
    static uint32_t lanes_seen[MAX_PORTS * MAX_LANES];
    uint32_t nseen = 0;
    sai_attribute_t g[3];
    begin("ports");
    memset(g, 0, sizeof(g));
    g[0].id = SAI_SWITCH_ATTR_PORT_NUMBER;
    g[1].id = SAI_SWITCH_ATTR_PORT_LIST;
    g[1].value.objlist.count = MAX_PORTS;
    g[1].value.objlist.list = ports;
    g[2].id = SAI_SWITCH_ATTR_CPU_PORT;
    if (!ok("get PORT_NUMBER/PORT_LIST/CPU_PORT", api.sw->get_switch_attribute(sw, 3, g))) return end();
    nports = g[1].value.objlist.count;
    val("ports=%u cpu=0x%016llx", nports, (unsigned long long)g[2].value.oid);
    if (g[0].value.u32 != nports) bad("PORT_NUMBER %u, PORT_LIST has %u", g[0].value.u32, nports);
    if ((long)nports != opt.expect_ports) bad("%u ports, expected %ld", nports, opt.expect_ports);
    printf("%5s %-18s %-20s %7s %5s %4s\n", "lane", "port", "lanes", "speed", "admin", "oper");
    for (uint32_t i = 0; i < nports; i++) {
        uint32_t lanes[MAX_LANES], n, l0;
        char text[96] = "";
        sai_attribute_t p[4];
        memset(p, 0, sizeof(p));
        p[0].id = SAI_PORT_ATTR_HW_LANE_LIST;
        p[0].value.u32list.count = MAX_LANES;
        p[0].value.u32list.list = lanes;
        p[1].id = SAI_PORT_ATTR_SPEED;
        p[2].id = SAI_PORT_ATTR_ADMIN_STATE;
        p[3].id = SAI_PORT_ATTR_OPER_STATUS;
        first_lane[i] = UINT32_MAX;
        if (!ok("get_port_attribute", api.port->get_port_attribute(ports[i], 4, p))) continue;
        n = p[0].value.u32list.count;
        l0 = n ? lanes[0] : 0;
        for (uint32_t l = 0, at = 0; l < n && l < MAX_LANES && at + 12 < sizeof(text); l++)
            at += (uint32_t)snprintf(text + at, sizeof(text) - at, "%s%u", l ? "," : "", lanes[l]);
        if (n > 0) first_lane[i] = l0;
        printf("%5u 0x%016llx %-20s %7u %5s %4s\n", l0, (unsigned long long)ports[i], text, p[1].value.u32,
               p[2].value.booldata ? "up" : "down", p[3].value.s32 == SAI_PORT_OPER_STATUS_UP ? "up" : "down");
        if ((long)n != opt.expect_lanes) bad("port %u: %u lanes, expected %ld", l0, n, opt.expect_lanes);
        if ((long)p[1].value.u32 != opt.expect_speed)
            bad("port %u: speed %u, expected %ld", l0, p[1].value.u32, opt.expect_speed);
        if (p[2].value.booldata) bad("port %u is admin up right after create_switch", l0);
        for (uint32_t l = 0; l < n && l < MAX_LANES; l++) {
            for (uint32_t k = 0; k < nseen; k++)
                if (lanes_seen[k] == lanes[l]) bad("lane %u is in two ports", lanes[l]);
            if (nseen < MAX_PORTS * MAX_LANES) lanes_seen[nseen++] = lanes[l];
        }
    }
    fflush(stdout);
    val("lanes=%u speed=%ld", nseen, opt.expect_speed);
    ncabled_found = 0;
    for (int c = 0; c < opt.ncabled; c++) {
        int idx = find_lane(opt.cabled[c]);
        if (idx < 0) bad("cabled port %u: no port has that first lane", opt.cabled[c]);
        else cabled_idx[ncabled_found++] = idx;
    }
    val("cabled=%d", ncabled_found);
    if (opt.lag_lane >= 0 && find_lane((uint32_t)opt.lag_lane) < 0) bad("--lag-port %ld: no such port", opt.lag_lane);
    return end();
}

/* PORT_STATE_CHANGE up notifications per cabled port before admin up (oper-up waits for one more) */
static int up_before[MAX_CABLED];

static bool step_admin_up(void)
{
    begin("admin-up");
    for (int c = 0; c < ncabled_found; c++) up_before[c] = events_of(cabled_idx[c], true);
    for (int c = 0; c < ncabled_found; c++) {
        int idx = cabled_idx[c];
        sai_attribute_t g = {.id = SAI_PORT_ATTR_ADMIN_STATE};
        if (!port_admin(idx, true)) continue;
        if (!ok("get ADMIN_STATE", api.port->get_port_attribute(ports[idx], 1, &g))) continue;
        if (!g.value.booldata) bad("port %u: admin state reads down after admin up", first_lane[idx]);
        else val("%u=admin-up", first_lane[idx]);
    }
    return end();
}

/* Waits until each of the n ports idx[] reads oper 'up' and a notification of that state
 * arrived beyond before[] of them; per port: ms to the oper state, the notifications. */
static void wait_link(bool up, const int* idx, int n, const int* before)
{
    long long t0 = now_ms(), deadline = t0 + opt.link_timeout * 1000;
    long long at[MAX_CABLED];
    bool done[MAX_CABLED], poke_failed = false;
    int left = n;
    for (int c = 0; c < n; c++) {
        at[c] = -1;
        done[c] = false;
    }
    for (;;) {
        for (int c = 0; c < n; c++) {
            bool o;
            if (done[c] || !port_oper_up(idx[c], &o) || o != up) continue;
            if (at[c] < 0) at[c] = now_ms() - t0;
            if (events_of(idx[c], up) > before[c]) {
                done[c] = true;
                left--;
            }
        }
        if (left == 0 || now_ms() >= deadline) break;
        if (opt.poke && !poke_failed) {
            char ans[256], err[256];
            if (diag(opt.poke, ans, sizeof(ans), err, sizeof(err)) != 0 || strstr(ans, "openbcm-diag: ")) {
                bad("--poke-diag '%s': %s", opt.poke, ans[0] ? ans : err);
                poke_failed = true;
            }
        }
        nap(100);
    }
    for (int c = 0; c < n; c++) {
        const char* want = up ? "up" : "down";
        int got = events_of(idx[c], up) - before[c];
        if (at[c] < 0) {
            bad("port %u: oper not %s within %ld s", first_lane[idx[c]], want, opt.link_timeout);
            val("%u=not-%s/notif%d", first_lane[idx[c]], want, got);
        } else {
            if (!done[c])
                bad("port %u: oper %s, but no PORT_STATE_CHANGE %s notification", first_lane[idx[c]], want, want);
            val("%u=%s@%lldms/notif%d", first_lane[idx[c]], want, at[c], got);
        }
    }
}

static bool step_oper_up(void)
{
    const char* table = profile_get_value(0, "SAI_OPENBCM_EXT_PHY_FILE");
    begin("oper-up");
    val("ext-phy=%s", opt.no_ext_phy ? "off(--no-ext-phy)" : table ? "the-profile's-table" : "none-in-profile");
    wait_link(true, cabled_idx, ncabled_found, up_before);
    return end();
}

static sai_object_id_t bridge_port_of(int idx)
{
    static sai_object_id_t bps[MAX_PORTS + 64];
    sai_attribute_t g = {.id = SAI_SWITCH_ATTR_DEFAULT_1Q_BRIDGE_ID};
    sai_attribute_t l = {.id = SAI_BRIDGE_ATTR_PORT_LIST};
    if (api.sw->get_switch_attribute(sw, 1, &g) != SAI_STATUS_SUCCESS) return SAI_NULL_OBJECT_ID;
    l.value.objlist.count = MAX_PORTS + 64;
    l.value.objlist.list = bps;
    if (api.bridge->get_bridge_attribute(g.value.oid, 1, &l) != SAI_STATUS_SUCCESS) return SAI_NULL_OBJECT_ID;
    for (uint32_t i = 0; i < l.value.objlist.count; i++) {
        sai_attribute_t bp = {.id = SAI_BRIDGE_PORT_ATTR_PORT_ID};
        if (api.bridge->get_bridge_port_attribute(bps[i], 1, &bp) == SAI_STATUS_SUCCESS && bp.value.oid == ports[idx])
            return bps[i];
    }
    return SAI_NULL_OBJECT_ID;
}

/* The member of VLAN v whose bridge port is bp; count: the VLAN's members. */
static sai_object_id_t vlan_member_of(sai_object_id_t v, sai_object_id_t bp, uint32_t* count)
{
    static sai_object_id_t members[MAX_PORTS + 64];
    sai_attribute_t l = {.id = SAI_VLAN_ATTR_MEMBER_LIST};
    *count = 0;
    l.value.objlist.count = MAX_PORTS + 64;
    l.value.objlist.list = members;
    if (api.vlan->get_vlan_attribute(v, 1, &l) != SAI_STATUS_SUCCESS) return SAI_NULL_OBJECT_ID;
    *count = l.value.objlist.count;
    for (uint32_t i = 0; i < l.value.objlist.count; i++) {
        sai_attribute_t m = {.id = SAI_VLAN_MEMBER_ATTR_BRIDGE_PORT_ID};
        if (api.vlan->get_vlan_member_attribute(members[i], 1, &m) == SAI_STATUS_SUCCESS && m.value.oid == bp)
            return members[i];
    }
    return SAI_NULL_OBJECT_ID;
}

static bool make_vlan_member(sai_object_id_t* out, sai_object_id_t v, sai_object_id_t bp, const char* what)
{
    sai_attribute_t m[3];
    memset(m, 0, sizeof(m));
    m[0].id = SAI_VLAN_MEMBER_ATTR_VLAN_ID;
    m[0].value.oid = v;
    m[1].id = SAI_VLAN_MEMBER_ATTR_BRIDGE_PORT_ID;
    m[1].value.oid = bp;
    m[2].id = SAI_VLAN_MEMBER_ATTR_VLAN_TAGGING_MODE;
    m[2].value.s32 = SAI_VLAN_TAGGING_MODE_UNTAGGED;
    if (ok(what, api.vlan->create_vlan_member(out, sw, 3, m))) return true;
    *out = SAI_NULL_OBJECT_ID;
    return false;
}

static bool step_vlan(void)
{
    int idx = cabled_idx[0];
    sai_object_id_t bp;
    sai_attribute_t v = {.id = SAI_VLAN_ATTR_VLAN_ID, .value.u16 = (sai_vlan_id_t)opt.vlan};
    uint32_t count;
    begin("vlan");
    bp = bridge_port_of(idx);
    if (!bp) {
        bad("port %u has no bridge port in the default .1Q bridge", first_lane[idx]);
        return end();
    }
    if (!ok("create_vlan", api.vlan->create_vlan(&vlan, sw, 1, &v))) {
        vlan = SAI_NULL_OBJECT_ID;
        return end();
    }
    if (!make_vlan_member(&vlan_member, vlan, bp, "create_vlan_member")) return end();
    if (vlan_member_of(vlan, bp, &count) != vlan_member || count != 1)
        bad("VLAN %ld: MEMBER_LIST has %u members, not the one created", opt.vlan, count);
    val("vlan=%ld member=%u untagged", opt.vlan, first_lane[idx]);
    return end();
}

static bool step_fdb(void)
{
    int idx = cabled_idx[0];
    sai_object_id_t bp = bridge_port_of(idx);
    sai_attribute_t a[3], g[2];
    begin("fdb");
    if (!vlan || !bp) {
        bad("needs the VLAN step's VLAN and the port's bridge port");
        return end();
    }
    memset(&fdb_key, 0, sizeof(fdb_key));
    fdb_key.switch_id = sw;
    memcpy(fdb_key.mac_address, "\x00\x00\x5e\x00\x53\x01", 6);
    fdb_key.bv_id = vlan;
    memset(a, 0, sizeof(a));
    a[0].id = SAI_FDB_ENTRY_ATTR_TYPE;
    a[0].value.s32 = SAI_FDB_ENTRY_TYPE_STATIC;
    a[1].id = SAI_FDB_ENTRY_ATTR_BRIDGE_PORT_ID;
    a[1].value.oid = bp;
    a[2].id = SAI_FDB_ENTRY_ATTR_PACKET_ACTION;
    a[2].value.s32 = SAI_PACKET_ACTION_FORWARD;
    if (!ok("create_fdb_entry", api.fdb->create_fdb_entry(&fdb_key, 3, a))) return end();
    fdb_made = true;
    memset(g, 0, sizeof(g));
    g[0].id = SAI_FDB_ENTRY_ATTR_TYPE;
    g[1].id = SAI_FDB_ENTRY_ATTR_BRIDGE_PORT_ID;
    if (ok("get_fdb_entry_attribute", api.fdb->get_fdb_entry_attribute(&fdb_key, 2, g)) &&
        (g[0].value.s32 != SAI_FDB_ENTRY_TYPE_STATIC || g[1].value.oid != bp))
        bad("the entry reads back type %d bridge port 0x%llx", g[0].value.s32, (unsigned long long)g[1].value.oid);
    val("mac=00:00:5e:00:53:01 vlan=%ld port=%u static", opt.vlan, first_lane[idx]);
    return end();
}

static bool is_cabled(int idx)
{
    for (int c = 0; c < ncabled_found; c++)
        if (cabled_idx[c] == idx) return true;
    return false;
}

static bool step_lag(void)
{
    int idx = -1;
    sai_object_id_t bp, v1member = SAI_NULL_OBJECT_ID, lag = SAI_NULL_OBJECT_ID, member = SAI_NULL_OBJECT_ID, v1;
    sai_attribute_t g = {.id = SAI_SWITCH_ATTR_DEFAULT_VLAN_ID};
    uint32_t count;
    begin("lag");
    if (opt.lag_lane >= 0) idx = find_lane((uint32_t)opt.lag_lane);
    for (uint32_t i = 0; i < nports && idx < 0; i++)
        if (!is_cabled((int)i)) idx = (int)i;
    if (idx < 0 || is_cabled(idx)) {
        bad("no uncabled port for the LAG (--lag-port)");
        return end();
    }
    if (!ok("get DEFAULT_VLAN_ID", api.sw->get_switch_attribute(sw, 1, &g))) return end();
    v1 = g.value.oid;
    bp = bridge_port_of(idx);
    if (bp) v1member = vlan_member_of(v1, bp, &count);
    /* SONiC's order: the port leaves VLAN 1 and the bridge before it joins a LAG */
    if (v1member && !ok("remove the VLAN 1 member", api.vlan->remove_vlan_member(v1member))) return end();
    if (bp && !ok("remove the bridge port", api.bridge->remove_bridge_port(bp))) {
        if (v1member) make_vlan_member(&v1member, v1, bp, "re-create the VLAN 1 member");
        return end();
    }
    if (ok("create_lag", api.lag->create_lag(&lag, sw, 0, NULL))) {
        sai_attribute_t m[2];
        memset(m, 0, sizeof(m));
        m[0].id = SAI_LAG_MEMBER_ATTR_LAG_ID;
        m[0].value.oid = lag;
        m[1].id = SAI_LAG_MEMBER_ATTR_PORT_ID;
        m[1].value.oid = ports[idx];
        if (ok("create_lag_member", api.lag->create_lag_member(&member, sw, 2, m))) {
            sai_object_id_t list[8];
            sai_attribute_t l = {.id = SAI_LAG_ATTR_PORT_LIST};
            l.value.objlist.count = 8;
            l.value.objlist.list = list;
            if (ok("get LAG PORT_LIST", api.lag->get_lag_attribute(lag, 1, &l)) &&
                (l.value.objlist.count != 1 || list[0] != member))
                bad("LAG PORT_LIST has %u entries, not the member", l.value.objlist.count);
            val("lag=0x%016llx member=%u", (unsigned long long)lag, first_lane[idx]);
            if (ok("remove_lag_member", api.lag->remove_lag_member(member))) val("member-removed");
        }
        if (ok("remove_lag", api.lag->remove_lag(lag))) val("lag-removed");
    }
    if (bp) {
        /* the port back as create_switch made it: a bridge port in the .1Q bridge, VLAN 1 untagged */
        sai_attribute_t b[3];
        sai_object_id_t nbp = SAI_NULL_OBJECT_ID, nm = SAI_NULL_OBJECT_ID;
        memset(b, 0, sizeof(b));
        b[0].id = SAI_BRIDGE_PORT_ATTR_TYPE;
        b[0].value.s32 = SAI_BRIDGE_PORT_TYPE_PORT;
        b[1].id = SAI_BRIDGE_PORT_ATTR_PORT_ID;
        b[1].value.oid = ports[idx];
        b[2].id = SAI_BRIDGE_PORT_ATTR_ADMIN_STATE;
        b[2].value.booldata = true;
        if (ok("re-create the bridge port", api.bridge->create_bridge_port(&nbp, sw, 3, b)) &&
            (!v1member || make_vlan_member(&nm, v1, nbp, "re-create the VLAN 1 member")))
            val("%u-back-in-vlan1", first_lane[idx]);
    }
    return end();
}

static bool step_hostif(void)
{
    int idx = cabled_idx[0];
    sai_attribute_t a[3], g[2];
    begin("hostif");
    if (!opt.no_kernel && netdev_exists(opt.hostif_name)) {
        bad("a netdev %s exists already (a left-over?): another --hostif-name", opt.hostif_name);
        return end();
    }
    memset(a, 0, sizeof(a));
    a[0].id = SAI_HOSTIF_ATTR_TYPE;
    a[0].value.s32 = SAI_HOSTIF_TYPE_NETDEV;
    a[1].id = SAI_HOSTIF_ATTR_OBJ_ID;
    a[1].value.oid = ports[idx];
    a[2].id = SAI_HOSTIF_ATTR_NAME;
    memcpy(a[2].value.chardata, opt.hostif_name, strlen(opt.hostif_name)); /* checked: < SAI_HOSTIF_NAME_SIZE */
    if (!ok("create_hostif", api.hostif->create_hostif(&hostif, sw, 3, a))) {
        hostif = SAI_NULL_OBJECT_ID;
        return end();
    }
    memset(g, 0, sizeof(g));
    g[0].id = SAI_HOSTIF_ATTR_NAME;
    g[1].id = SAI_HOSTIF_ATTR_OBJ_ID;
    if (ok("get_hostif_attribute", api.hostif->get_hostif_attribute(hostif, 2, g)) &&
        (strncmp(g[0].value.chardata, opt.hostif_name, SAI_HOSTIF_NAME_SIZE) || g[1].value.oid != ports[idx]))
        bad("the host interface reads back another name or port");
    val("name=%s port=%u", opt.hostif_name, first_lane[idx]);
    return end();
}

static bool set_hostif_oper(bool up)
{
    sai_attribute_t a = {.id = SAI_HOSTIF_ATTR_OPER_STATUS, .value.booldata = up};
    return ok(up ? "hostif OPER_STATUS=true" : "hostif OPER_STATUS=false", api.hostif->set_hostif_attribute(hostif, &a));
}

static bool step_hostif_netdev(void)
{
    const char* name = opt.hostif_name;
    int idx = cabled_idx[0];
    bool up = false;
    begin("hostif-netdev");
    if (!port_oper_up(idx, &up)) bad("port %u: OPER_STATUS unreadable", first_lane[idx]);
    if (opt.no_kernel) {
        /* No KNET here: the openbcm backend sets a netdev's carrier through /proc/bcm/knet/link,
         * so the set fails as it does in the C++ backend; what it answered is shown, not judged. */
        sai_attribute_t a = {.id = SAI_HOSTIF_ATTR_OPER_STATUS, .value.booldata = up};
        sai_status_t st = api.hostif->set_hostif_attribute(hostif, &a);
        if (step_bad) return end();
        n_skip++;
        printf("SKIP %s --no-kernel: no /sys/class/net/%s and carrier check (hostif OPER_STATUS=%s from the port "
               "answered %s; /proc/bcm/knet/link %s)\n",
               cur, name, up ? "true" : "false", status_name(st),
               access("/proc/bcm/knet/link", W_OK) == 0 ? "exists" : "absent");
        fflush(stdout);
        return true;
    }
    if (!wait_for(netdev_exists, name, opt.netdev_timeout)) {
        bad("/sys/class/net/%s does not exist %ld s after create_hostif (KNET: /proc/bcm/knet?)", name,
            opt.netdev_timeout);
        return end();
    }
    val("netdev=/sys/class/net/%s ipv6-off=%s", name, netdev_no_ipv6(name) ? "yes" : "no");
    if (!netdev_set_up(name, true)) {
        bad("cannot set %s up: %s", name, strerror(errno));
        return end();
    }
    netdev_up = true;
    /* as orchagent: the host interface's oper status follows the port's */
    if (!up) bad("port %u is not oper up: the carrier check needs it up", first_lane[idx]);
    if (set_hostif_oper(true)) {
        if (wait_for(carrier_is_1, name, opt.netdev_timeout)) val("oper-up->carrier=1");
        else bad("hostif OPER_STATUS true: carrier %d after %ld s", netdev_carrier(name), opt.netdev_timeout);
    }
    if (set_hostif_oper(false)) {
        if (wait_for(carrier_is_0, name, opt.netdev_timeout)) val("oper-down->carrier=0");
        else bad("hostif OPER_STATUS false: carrier %d after %ld s", netdev_carrier(name), opt.netdev_timeout);
    }
    if (set_hostif_oper(up)) val("back-to-port's=%s", up ? "up" : "down");
    return end();
}

static bool step_l3(void)
{
    int idx = cabled_idx[ncabled_found - 1];
    sai_attribute_t g = {.id = SAI_SWITCH_ATTR_DEFAULT_VIRTUAL_ROUTER_ID};
    sai_attribute_t a[4];
    sai_object_id_t vr;
    begin("l3");
    if (!ok("get DEFAULT_VIRTUAL_ROUTER_ID", api.sw->get_switch_attribute(sw, 1, &g))) return end();
    vr = g.value.oid;
    memset(a, 0, sizeof(a));
    a[0].id = SAI_ROUTER_INTERFACE_ATTR_VIRTUAL_ROUTER_ID;
    a[0].value.oid = vr;
    a[1].id = SAI_ROUTER_INTERFACE_ATTR_TYPE;
    a[1].value.s32 = SAI_ROUTER_INTERFACE_TYPE_PORT;
    a[2].id = SAI_ROUTER_INTERFACE_ATTR_PORT_ID;
    a[2].value.oid = ports[idx];
    a[3].id = SAI_ROUTER_INTERFACE_ATTR_SRC_MAC_ADDRESS;
    memcpy(a[3].value.mac, "\x02\x00\x00\x00\x53\x01", 6);
    if (!ok("create_router_interface", api.rif->create_router_interface(&rif, sw, 4, a))) {
        rif = SAI_NULL_OBJECT_ID;
        return end();
    }
    val("rif=port-%u", first_lane[idx]);

    memset(&nbr_key, 0, sizeof(nbr_key));
    nbr_key.switch_id = sw;
    nbr_key.rif_id = rif;
    nbr_key.ip_address.addr_family = SAI_IP_ADDR_FAMILY_IPV4;
    memcpy(&nbr_key.ip_address.addr.ip4, "\xc0\x00\x02\x02", 4); /* 192.0.2.2, network order in memory */
    memset(a, 0, sizeof(a));
    a[0].id = SAI_NEIGHBOR_ENTRY_ATTR_DST_MAC_ADDRESS;
    memcpy(a[0].value.mac, "\x02\x00\x00\x00\x53\x02", 6);
    if (!ok("create_neighbor_entry", api.nbr->create_neighbor_entry(&nbr_key, 1, a))) return end();
    nbr_made = true;
    val("neighbour=192.0.2.2");

    memset(a, 0, sizeof(a));
    a[0].id = SAI_NEXT_HOP_ATTR_TYPE;
    a[0].value.s32 = SAI_NEXT_HOP_TYPE_IP;
    a[1].id = SAI_NEXT_HOP_ATTR_ROUTER_INTERFACE_ID;
    a[1].value.oid = rif;
    a[2].id = SAI_NEXT_HOP_ATTR_IP;
    a[2].value.ipaddr = nbr_key.ip_address;
    if (!ok("create_next_hop", api.nh->create_next_hop(&next_hop, sw, 3, a))) {
        next_hop = SAI_NULL_OBJECT_ID;
        return end();
    }
    val("next-hop=0x%016llx", (unsigned long long)next_hop);

    memset(&route_key, 0, sizeof(route_key));
    route_key.switch_id = sw;
    route_key.vr_id = vr;
    route_key.destination.addr_family = SAI_IP_ADDR_FAMILY_IPV4;
    memcpy(&route_key.destination.addr.ip4, "\xc6\x33\x64\x00", 4); /* 198.51.100.0/24 */
    memcpy(&route_key.destination.mask.ip4, "\xff\xff\xff\x00", 4);
    memset(a, 0, sizeof(a));
    a[0].id = SAI_ROUTE_ENTRY_ATTR_NEXT_HOP_ID;
    a[0].value.oid = next_hop;
    if (!ok("create_route_entry", api.route->create_route_entry(&route_key, 1, a))) return end();
    route_made = true;
    memset(a, 0, sizeof(a));
    a[0].id = SAI_ROUTE_ENTRY_ATTR_NEXT_HOP_ID;
    if (ok("get_route_entry_attribute", api.route->get_route_entry_attribute(&route_key, 1, a)) &&
        a[0].value.oid != next_hop)
        bad("the route reads back next hop 0x%llx", (unsigned long long)a[0].value.oid);
    val("route=198.51.100.0/24");
    return end();
}

static bool step_acl(void)
{
    int idx = cabled_idx[0];
    int32_t bind_points[1] = {SAI_ACL_BIND_POINT_TYPE_PORT};
    sai_attribute_t a[6];
    begin("acl");
    memset(a, 0, sizeof(a));
    a[0].id = SAI_ACL_TABLE_ATTR_ACL_STAGE;
    a[0].value.s32 = SAI_ACL_STAGE_INGRESS;
    a[1].id = SAI_ACL_TABLE_ATTR_ACL_BIND_POINT_TYPE_LIST;
    a[1].value.s32list.count = 1;
    a[1].value.s32list.list = bind_points;
    a[2].id = SAI_ACL_TABLE_ATTR_FIELD_DST_IP;
    a[2].value.booldata = true;
    if (!ok("create_acl_table", api.acl->create_acl_table(&acl_table, sw, 3, a))) {
        acl_table = SAI_NULL_OBJECT_ID;
        return end();
    }
    memset(a, 0, sizeof(a));
    a[0].id = SAI_ACL_COUNTER_ATTR_TABLE_ID;
    a[0].value.oid = acl_table;
    a[1].id = SAI_ACL_COUNTER_ATTR_ENABLE_PACKET_COUNT;
    a[1].value.booldata = true;
    a[2].id = SAI_ACL_COUNTER_ATTR_ENABLE_BYTE_COUNT;
    a[2].value.booldata = true;
    if (!ok("create_acl_counter", api.acl->create_acl_counter(&acl_counter, sw, 3, a))) {
        acl_counter = SAI_NULL_OBJECT_ID;
        return end();
    }
    memset(a, 0, sizeof(a));
    a[0].id = SAI_ACL_ENTRY_ATTR_TABLE_ID;
    a[0].value.oid = acl_table;
    a[1].id = SAI_ACL_ENTRY_ATTR_PRIORITY;
    a[1].value.u32 = 100;
    a[2].id = SAI_ACL_ENTRY_ATTR_ADMIN_STATE;
    a[2].value.booldata = true;
    a[3].id = SAI_ACL_ENTRY_ATTR_FIELD_DST_IP;
    a[3].value.aclfield.enable = true;
    memcpy(&a[3].value.aclfield.data.ip4, "\xc0\x00\x02\x01", 4); /* 192.0.2.1 */
    memcpy(&a[3].value.aclfield.mask.ip4, "\xff\xff\xff\xff", 4);
    a[4].id = SAI_ACL_ENTRY_ATTR_ACTION_COUNTER;
    a[4].value.aclaction.enable = true;
    a[4].value.aclaction.parameter.oid = acl_counter;
    a[5].id = SAI_ACL_ENTRY_ATTR_ACTION_PACKET_ACTION;
    a[5].value.aclaction.enable = true;
    a[5].value.aclaction.parameter.s32 = SAI_PACKET_ACTION_FORWARD;
    if (!ok("create_acl_entry", api.acl->create_acl_entry(&acl_entry, sw, 6, a))) {
        acl_entry = SAI_NULL_OBJECT_ID;
        return end();
    }
    memset(a, 0, sizeof(a));
    a[0].id = SAI_ACL_TABLE_GROUP_ATTR_ACL_STAGE;
    a[0].value.s32 = SAI_ACL_STAGE_INGRESS;
    a[1].id = SAI_ACL_TABLE_GROUP_ATTR_ACL_BIND_POINT_TYPE_LIST;
    a[1].value.s32list.count = 1;
    a[1].value.s32list.list = bind_points;
    a[2].id = SAI_ACL_TABLE_GROUP_ATTR_TYPE;
    a[2].value.s32 = SAI_ACL_TABLE_GROUP_TYPE_PARALLEL;
    if (!ok("create_acl_table_group", api.acl->create_acl_table_group(&acl_group, sw, 3, a))) {
        acl_group = SAI_NULL_OBJECT_ID;
        return end();
    }
    {
        sai_attribute_t b = {.id = SAI_PORT_ATTR_INGRESS_ACL, .value.oid = acl_group};
        if (!ok("bind the group (port INGRESS_ACL)", api.port->set_port_attribute(ports[idx], &b))) return end();
        acl_bound = true;
    }
    memset(a, 0, sizeof(a));
    a[0].id = SAI_ACL_TABLE_GROUP_MEMBER_ATTR_ACL_TABLE_GROUP_ID;
    a[0].value.oid = acl_group;
    a[1].id = SAI_ACL_TABLE_GROUP_MEMBER_ATTR_ACL_TABLE_ID;
    a[1].value.oid = acl_table;
    a[2].id = SAI_ACL_TABLE_GROUP_MEMBER_ATTR_PRIORITY;
    a[2].value.u32 = 100;
    if (!ok("create_acl_table_group_member", api.acl->create_acl_table_group_member(&acl_member, sw, 3, a))) {
        acl_member = SAI_NULL_OBJECT_ID;
        return end();
    }
    val("table+counter+entry(dst=192.0.2.1,forward)+group-bound-to=%u", first_lane[idx]);
    memset(a, 0, sizeof(a));
    a[0].id = SAI_ACL_COUNTER_ATTR_PACKETS;
    a[1].id = SAI_ACL_COUNTER_ATTR_BYTES;
    if (ok("get_acl_counter_attribute", api.acl->get_acl_counter_attribute(acl_counter, 2, a)))
        val("counter-packets=%llu bytes=%llu", (unsigned long long)a[0].value.u64, (unsigned long long)a[1].value.u64);
    return end();
}

static bool step_policer(void)
{
    sai_attribute_t a[5];
    begin("policer");
    memset(a, 0, sizeof(a));
    a[0].id = SAI_POLICER_ATTR_METER_TYPE;
    a[0].value.s32 = SAI_METER_TYPE_PACKETS;
    a[1].id = SAI_POLICER_ATTR_MODE;
    a[1].value.s32 = SAI_POLICER_MODE_SR_TCM;
    a[2].id = SAI_POLICER_ATTR_CIR;
    a[2].value.u64 = 600;
    a[3].id = SAI_POLICER_ATTR_CBS;
    a[3].value.u64 = 600;
    a[4].id = SAI_POLICER_ATTR_RED_PACKET_ACTION;
    a[4].value.s32 = SAI_PACKET_ACTION_DROP;
    if (!ok("create_policer", api.policer->create_policer(&policer, sw, 5, a))) {
        policer = SAI_NULL_OBJECT_ID;
        return end();
    }
    memset(a, 0, sizeof(a));
    a[0].id = SAI_POLICER_ATTR_CIR;
    if (ok("get CIR", api.policer->get_policer_attribute(policer, 1, a)) && a[0].value.u64 != 600)
        bad("CIR reads %llu", (unsigned long long)a[0].value.u64);
    a[0].value.u64 = 1000;
    if (ok("set CIR", api.policer->set_policer_attribute(policer, a))) {
        a[0].value.u64 = 0;
        if (ok("get CIR", api.policer->get_policer_attribute(policer, 1, a)) && a[0].value.u64 != 1000)
            bad("CIR reads %llu after the set", (unsigned long long)a[0].value.u64);
    }
    val("packets sr_tcm cir=600->1000 cbs=600");
    return end();
}

static bool step_port_stats(void)
{
    static const sai_stat_id_t ids[] = {SAI_PORT_STAT_IF_IN_OCTETS, SAI_PORT_STAT_IF_IN_UCAST_PKTS,
                                        SAI_PORT_STAT_IF_OUT_OCTETS, SAI_PORT_STAT_IF_OUT_UCAST_PKTS,
                                        SAI_PORT_STAT_IF_IN_ERRORS};
    begin("port-stats");
    for (int c = 0; c < ncabled_found; c++) {
        uint64_t v[5] = {0};
        int idx = cabled_idx[c];
        if (ok("get_port_stats", api.port->get_port_stats(ports[idx], 5, ids, v)))
            val("%u=in:%llu/%llu,out:%llu/%llu,in-err:%llu", first_lane[idx], (unsigned long long)v[0],
                (unsigned long long)v[1], (unsigned long long)v[2], (unsigned long long)v[3], (unsigned long long)v[4]);
    }
    return end();
}

static bool step_temperature(void)
{
    int32_t list[64], max = INT32_MIN;
    char text[512] = "";
    sai_attribute_t a[3];
    uint32_t n;
    begin("temperature");
    memset(a, 0, sizeof(a));
    a[0].id = SAI_SWITCH_ATTR_TEMP_LIST;
    a[0].value.s32list.count = 64;
    a[0].value.s32list.list = list;
    a[1].id = SAI_SWITCH_ATTR_MAX_TEMP;
    a[2].id = SAI_SWITCH_ATTR_AVERAGE_TEMP;
    if (!ok("get TEMP_LIST/MAX_TEMP/AVERAGE_TEMP", api.sw->get_switch_attribute(sw, 3, a))) return end();
    n = a[0].value.s32list.count;
    for (uint32_t i = 0, at = 0; i < n && i < 64; i++) {
        if (at + 16 < sizeof(text)) at += (uint32_t)snprintf(text + at, sizeof(text) - at, "%s%d", i ? "," : "", list[i]);
        if (list[i] > max) max = list[i];
        if (list[i] < -40 || list[i] > 150) bad("monitor %u reads %d C (plausible: -40..150)", i, list[i]);
    }
    if (n == 0) bad("TEMP_LIST is empty");
    /* The library reads the monitors again for each attribute (the C++ one too), and a monitor moves by a
     * degree between two reads: MAX_TEMP may differ from this list's maximum by one. */
    else if (a[1].value.s32 < max - 1 || a[1].value.s32 > max + 1) bad("MAX_TEMP %d, the list's maximum %d", a[1].value.s32, max);
    val("list=[%s] max=%d average=%d", text, a[1].value.s32, a[2].value.s32);
    return end();
}

static bool step_diag(void)
{
    static char ans[65536];
    char err[512], line[160];
    const char* marker;
    begin("diag");
    if (diag(opt.diag_cmd, ans, sizeof(ans), err, sizeof(err)) != 0) bad("'%s': %s", opt.diag_cmd, err);
    else if ((marker = strstr(ans, "openbcm-diag: ")))
        bad("'%s': the server answered '%s'", opt.diag_cmd, first_line(marker, line, sizeof(line)));
    else if (!ans[0]) bad("'%s': empty answer", opt.diag_cmd);
    else val("cmd='%s' bytes=%zu first='%s'", opt.diag_cmd, strlen(ans), first_line(ans, line, sizeof(line)));
    return end();
}

static bool step_mdio_read(void)
{
    char cmd[80], ans[1024], err[512], line[160];
    char *p = NULL, *e;
    unsigned long v;
    begin("mdio-read");
    /* exactly as mdio_report.c:694 sends it: clause 45, device 1, one read */
    snprintf(cmd, sizeof(cmd), "phy raw c45 0x%lx 0x%x 0x%lx", opt.mdio_phy, 0x1u, opt.mdio_reg);
    val("cmd='%s'", cmd);
    if (diag(cmd, ans, sizeof(ans), err, sizeof(err)) != 0) {
        bad("%s", err);
        return end();
    }
    if ((e = strstr(ans, "openbcm-diag: "))) {
        bad("the server answered '%s'", first_line(e, line, sizeof(line)));
        return end();
    }
    /* the shell prints "\t0x<reg>: 0x<value>": the last ": 0x", as mdio_report.c */
    for (e = ans; (e = strstr(e, ": 0x")) != NULL; e += 4) p = e + 2;
    if (!p) {
        bad("no value in the answer '%s'", first_line(ans, line, sizeof(line)));
        return end();
    }
    errno = 0;
    v = strtoul(p, &e, 16);
    if (errno || e == p || v > 0xffff) {
        bad("no 16-bit value in '%s'", first_line(ans, line, sizeof(line)));
        return end();
    }
    val("phy=0x%lx dev=1 reg=0x%lx value=0x%04lx", opt.mdio_phy, opt.mdio_reg, v);
    if (opt.mdio_expect >= 0 && (long)v != opt.mdio_expect) bad("read 0x%04lx, expected 0x%04lx", v, opt.mdio_expect);
    return end();
}

static bool step_teardown(void)
{
    begin("teardown");
    if (policer && ok("remove_policer", api.policer->remove_policer(policer))) val("policer");
    if (acl_member && ok("remove_acl_table_group_member", api.acl->remove_acl_table_group_member(acl_member)))
        val("acl-member");
    if (acl_bound) {
        sai_attribute_t b = {.id = SAI_PORT_ATTR_INGRESS_ACL, .value.oid = SAI_NULL_OBJECT_ID};
        if (ok("unbind the ACL group", api.port->set_port_attribute(ports[cabled_idx[0]], &b))) val("acl-unbound");
    }
    if (acl_group && ok("remove_acl_table_group", api.acl->remove_acl_table_group(acl_group))) val("acl-group");
    if (acl_entry && ok("remove_acl_entry", api.acl->remove_acl_entry(acl_entry))) val("acl-entry");
    if (acl_counter && ok("remove_acl_counter", api.acl->remove_acl_counter(acl_counter))) val("acl-counter");
    if (acl_table && ok("remove_acl_table", api.acl->remove_acl_table(acl_table))) val("acl-table");
    if (route_made && ok("remove_route_entry", api.route->remove_route_entry(&route_key))) val("route");
    if (next_hop && ok("remove_next_hop", api.nh->remove_next_hop(next_hop))) val("next-hop");
    if (nbr_made && ok("remove_neighbor_entry", api.nbr->remove_neighbor_entry(&nbr_key))) val("neighbour");
    if (rif && ok("remove_router_interface", api.rif->remove_router_interface(rif))) val("rif");
    if (netdev_up && !netdev_set_up(opt.hostif_name, false))
        bad("cannot set %s down: %s", opt.hostif_name, strerror(errno));
    if (hostif && ok("remove_hostif", api.hostif->remove_hostif(hostif))) {
        val("hostif");
        if (!opt.no_kernel) {
            if (wait_for(netdev_gone, opt.hostif_name, opt.netdev_timeout)) val("netdev-gone");
            else bad("/sys/class/net/%s still exists %ld s after remove_hostif", opt.hostif_name, opt.netdev_timeout);
        }
    }
    if (fdb_made && ok("remove_fdb_entry", api.fdb->remove_fdb_entry(&fdb_key))) val("fdb");
    if (vlan_member && ok("remove_vlan_member", api.vlan->remove_vlan_member(vlan_member))) val("vlan-member");
    if (vlan && ok("remove_vlan", api.vlan->remove_vlan(vlan))) val("vlan");
    if (!vals[0] && !step_bad) val("nothing-to-remove");
    return end();
}

static bool step_admin_down(void)
{
    int idx[MAX_CABLED], before[MAX_CABLED], n = 0;
    begin("admin-down");
    for (int c = 0; c < ncabled_found; c++) {
        bool up = false;
        if (port_oper_up(cabled_idx[c], &up) && up) {
            idx[n] = cabled_idx[c];
            before[n] = events_of(idx[n], false);
            n++;
        }
    }
    for (int c = 0; c < ncabled_found; c++) port_admin(cabled_idx[c], false);
    /* only the ports that were up can go down (the others are oper-up's failure) */
    if (n) wait_link(false, idx, n, before);
    for (uint32_t i = 0; i < nports; i++) {
        sai_attribute_t g = {.id = SAI_PORT_ATTR_ADMIN_STATE};
        if (api.port->get_port_attribute(ports[i], 1, &g) != SAI_STATUS_SUCCESS) {
            bad("port %u: ADMIN_STATE unreadable", first_lane[i]);
        } else if (g.value.booldata) {
            bad("port %u is still admin up", first_lane[i]);
            port_admin((int)i, false);
        }
    }
    val("all-%u-ports-admin-down", nports);
    return end();
}

static bool step_recreate(void)
{
    sai_attribute_t a[2], g[1];
    sai_object_id_t again = SAI_NULL_OBJECT_ID;
    begin("recreate");
    printf("RISK recreate: a second create_switch in this process re-runs \"init all\" on the SDK that stayed "
           "attached (no detach): the chip is reset again; untested on hardware (C++ and Rust)\n");
    fflush(stdout);
    memset(a, 0, sizeof(a));
    a[0].id = SAI_SWITCH_ATTR_INIT_SWITCH;
    a[0].value.booldata = true;
    a[1].id = SAI_SWITCH_ATTR_PORT_STATE_CHANGE_NOTIFY;
    a[1].value.ptr = (void*)on_port_state;
    if (!ok("create_switch (second)", api.sw->create_switch(&again, 2, a))) return end();
    memset(g, 0, sizeof(g));
    g[0].id = SAI_SWITCH_ATTR_PORT_NUMBER;
    if (ok("get PORT_NUMBER", api.sw->get_switch_attribute(again, 1, g))) {
        val("ports=%u", g[0].value.u32);
        if ((long)g[0].value.u32 != opt.expect_ports) bad("%u ports, expected %ld", g[0].value.u32, opt.expect_ports);
    }
    if (ok("remove_switch (second)", api.sw->remove_switch(again))) val("removed");
    return end();
}

/* ---------------------------------------------------------------- main */

static int finish(void)
{
    if (n_fail)
        printf("SMOKE_FAILED %d FAIL, %d PASS, %d SKIP\n", n_fail, n_pass, n_skip);
    else
        printf("SMOKE_OK %d PASS, %d SKIP: %u ports, the cabled ones up and down again, everything removed\n", n_pass,
               n_skip, nports);
    fflush(stdout);
    return n_fail ? 1 : 0;
}

static int step_index(const char* name)
{
    for (int i = 0; i < NSTEPS; i++)
        if (!strcmp(STEPS[i].name, name)) return i;
    return NSTEPS;
}

/* SKIP lines for the steps from 'from' up to (not including) 'to'. */
static void skip_steps(const char* from, const char* to, const char* reason)
{
    for (int i = step_index(from); i < step_index(to) && i < NSTEPS; i++) skip(STEPS[i].name, "%s", reason);
}

static void query_apis(sai_status_t (*query)(sai_api_t, void**))
{
    static const struct {
        sai_api_t id;
        size_t offset;
        const char* name;
    } Q[] = {
        {SAI_API_SWITCH, offsetof(__typeof__(api), sw), "SWITCH"},
        {SAI_API_PORT, offsetof(__typeof__(api), port), "PORT"},
        {SAI_API_VLAN, offsetof(__typeof__(api), vlan), "VLAN"},
        {SAI_API_BRIDGE, offsetof(__typeof__(api), bridge), "BRIDGE"},
        {SAI_API_FDB, offsetof(__typeof__(api), fdb), "FDB"},
        {SAI_API_LAG, offsetof(__typeof__(api), lag), "LAG"},
        {SAI_API_HOSTIF, offsetof(__typeof__(api), hostif), "HOSTIF"},
        {SAI_API_ROUTER_INTERFACE, offsetof(__typeof__(api), rif), "ROUTER_INTERFACE"},
        {SAI_API_NEIGHBOR, offsetof(__typeof__(api), nbr), "NEIGHBOR"},
        {SAI_API_NEXT_HOP, offsetof(__typeof__(api), nh), "NEXT_HOP"},
        {SAI_API_ROUTE, offsetof(__typeof__(api), route), "ROUTE"},
        {SAI_API_ACL, offsetof(__typeof__(api), acl), "ACL"},
        {SAI_API_POLICER, offsetof(__typeof__(api), policer), "POLICER"},
    };
    for (size_t i = 0; i < sizeof(Q) / sizeof(Q[0]); i++) {
        void** slot = (void**)((char*)&api + Q[i].offset);
        char what[64];
        snprintf(what, sizeof(what), "sai_api_query(%s)", Q[i].name);
        if (ok(what, query(Q[i].id, slot)) && !*slot) bad("%s: null table", what);
    }
}

static void step_profile(void)
{
    begin("profile");
    val("file=%s keys=%d served:SAI_RS_BACKEND=%s", opt.profile, nprofile, opt.backend);
    if (opt.log_stderr) val("served:SAI_OPENBCM_LOG_STDERR=1");
    for (size_t i = 0; i < NEXT_KEYS; i++) {
        const char* v = file_value(EXT_KEYS[i]);
        if (opt.no_ext_phy) val("DROPPED(--no-ext-phy):%s%s", EXT_KEYS[i], v ? "" : "(not-in-the-file)");
        else val("%s=%s", EXT_KEYS[i], v ? v : "(none)");
    }
    if (opt.no_ext_phy)
        printf("--no-ext-phy: the profile is served WITHOUT its external PHY and MACsec tables: the library does not "
               "touch the PHYs; link is the ASIC side's (the PHY's system side)\n");
    if (!file_value("SAI_INIT_CONFIG_FILE")) bad("no SAI_INIT_CONFIG_FILE in the profile");
    val("diag-socket=%s", diag_socket() ? diag_socket() : "(none)");
    end();
}

int main(int argc, char** argv)
{
    int rc;
    void* lib;
    if (argc < 2) return usage();
    if ((rc = parse_args(argc, argv)) != 0) return rc;
    if (opt.dry) return dry();
    if (!opt.confirmed) {
        fprintf(stderr, "refusing: creating the switch resets the switch chip. Stop SONiC and every other SDK process\n"
                        "(systemctl stop sonic.target openbcm-asic-temp), then pass --sonic-is-stopped.\n");
        return 2;
    }
    if (load_profile(opt.profile) != 0) return 2;
    if (sdk_lock_free() != 0) return 2;

    begin("load");
    lib = dlopen(opt.libsai, RTLD_NOW | RTLD_LOCAL);
    if (!lib) {
        bad("dlopen: %s", dlerror());
        end();
        skip_steps("profile", "", "the library did not load");
        return finish();
    }
    sai_status_t (*init)(uint64_t, const sai_service_method_table_t*) = (__typeof__(init))dlsym(lib, "sai_api_initialize");
    sai_status_t (*uninit)(void) = (__typeof__(uninit))dlsym(lib, "sai_api_uninitialize");
    sai_status_t (*query)(sai_api_t, void**) = (__typeof__(query))dlsym(lib, "sai_api_query");
    if (!init || !uninit || !query) {
        bad("the library lacks sai_api_initialize / _uninitialize / sai_api_query");
        end();
        skip_steps("profile", "", "not a SAI library");
        return finish();
    }
    const sai_service_method_table_t services = {profile_get_value, profile_get_next_value};
    if (!ok("sai_api_initialize", init(0, &services))) {
        end();
        skip_steps("profile", "", "sai_api_initialize failed");
        return finish();
    }
    query_apis(query);
    val("library=%s backend=%s", opt.libsai, opt.backend);
    if (!end()) {
        skip_steps("profile", "uninitialize", "an API table is missing");
    } else {
        step_profile();
        if (!step_create_switch()) {
            skip_steps("ports", "uninitialize", "create_switch failed");
        } else {
            if (step_ports() && ncabled_found == opt.ncabled) {
                step_admin_up();
                step_oper_up();
                step_vlan();
                step_fdb();
                step_lag();
                if (step_hostif()) step_hostif_netdev();
                else skip("hostif-netdev", "no host interface");
                step_l3();
                step_acl();
                step_policer();
                step_port_stats();
                step_temperature();
                step_diag();
                step_mdio_read();
                step_teardown();
                step_admin_down();
            } else {
                skip_steps("admin-up", "remove-switch", "the ports are not as expected");
            }
            begin("remove-switch");
            if (ok("remove_switch", api.sw->remove_switch(sw)))
                val("link-notifications=%d (unknown port: %d)", ev_total, ev_unknown);
            bool removed = end();
            if (!opt.recreate) skip("recreate", "not asked (--recreate: a second \"init all\" in this process)");
            else if (!removed) skip("recreate", "remove_switch failed");
            else step_recreate();
        }
    }
    begin("uninitialize");
    ok("sai_api_uninitialize", uninit());
    end();
    return finish();
}
