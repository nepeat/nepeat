/* SPDX-License-Identifier: Apache-2.0
 * Thin C layer over the Broadcom SDK (OpenBCM 6.5.27) for sai-openbcm.
 *
 * Bring-up replicates systems/linux/user/common/socdiag.c (the diag shell's
 * main) minus the interactive loop:
 *   sal_config_file_set(config.bcm)   - before sal_appl_init, which reads it
 *   sal_core_init, sal_appl_init
 *   soc_knet_config (KNET kcom over the bcm-knet ioctl), chip-info vector
 *   diag_sdk_init with BOOT_F_NO_RC   - bde_create, probe, attach, no rc.soc
 *   system_shutdown + system_init     - exactly "init all"
 *   optional "rcload <rc file>"
 *   KNET: destroy stale netifs/filters (they survive SDK restarts)
 *   bcm_rx_start (KNET needs the SDK's RX DMA running), drop-all RX handler
 *   L3 egress mode on
 * Destinations with OB_LAG_DEST_BIT are trunks (SAI LAGs): L2 entries, L2
 * flushes, L3 egress objects and the L2 callback carry the trunk id.
 * Endianness: all conversions to SDK types happen here; bcm_ip_t is a host
 * integer built from network-order bytes, so this is byte-order agnostic.
 */
#include <errno.h>
#include <fcntl.h>
#include <sys/file.h>
#include <stdio.h>
#include <string.h>
#include <unistd.h>

#include <sal/core/boot.h>
#include <sal/appl/sal.h>
#include <sal/appl/config.h>
#include <soc/drv.h>
#include <soc/knet.h>
#include <bcm/error.h>
#include <bcm/init.h>
#include <bcm/port.h>
#include <bcm/vlan.h>
#include <bcm/l2.h>
#include <bcm/l3.h>
#include <bcm/stat.h>
#include <bcm/knet.h>
#include <bcm/rx.h>
#include <bcm/switch.h>
#include <bcm/link.h>
#include <bcm/stack.h>
#include <bcm/trunk.h>
#include <bcm/field.h>
#include <bcm/policer.h>
#include <soc/tomahawk.h>
#include <bcm_int/control.h>
#include <bcm_int/esw/firebolt.h>   /* L3 software tables: ECMP member accounting */
#include <appl/diag/shell.h>
#include <bcm-knet-kcom.h>

#include "sdk_glue.h"

/* From systems/linux/user/common (libsdk), see socdiag.c. */
extern void diag_sdk_init(void);
extern int system_init(int unit);
extern int system_shutdown(int unit, int cleanup);
extern int bde_icid_get(int d, uint8 *data, int len);
extern int bde_irq_mask_set(int unit, uint32 addr, uint32 mask);
extern int bde_hw_unit_get(int unit, int inverse);

/* sdk_glue.h mirrors the BCM_E_* values so C++ needs no SDK header. */
_Static_assert(BCM_E_NONE == OB_E_NONE && BCM_E_PARAM == OB_E_PARAM && BCM_E_NOT_FOUND == OB_E_NOT_FOUND &&
               BCM_E_EXISTS == OB_E_EXISTS && BCM_E_FULL == OB_E_FULL && BCM_E_UNAVAIL == OB_E_UNAVAIL &&
               BCM_E_PORT == OB_E_PORT && BCM_E_BUSY == OB_E_BUSY && BCM_E_INIT == OB_E_INIT &&
               BCM_E_CONFIG == OB_E_CONFIG && BCM_E_RESOURCE == OB_E_RESOURCE && BCM_E_BADID == OB_E_BADID &&
               BCM_E_MEMORY == OB_E_MEMORY && BCM_E_UNIT == OB_E_UNIT && BCM_E_EMPTY == OB_E_EMPTY,
               "BCM_E_* values differ from sdk_glue.h");

static int sdk_booted;
static int sdk_inited;      /* "init all" succeeded at least once */
static int sdk_lock_fd = -1;
static ob_link_cb_t g_link_cb;
static ob_l2_cb_t g_l2_cb;
static void* g_cb_ctx;
static volatile unsigned long g_rx_dropped;
static int g_cpu_egress = -1, g_drop_egress = -1;
static int g_l2c_index[2] = {-1, -1};   /* our L2 user entries: LLDP, LACP */
#define OB_MAX_TRUNKS 1024
static unsigned char g_trunk_used[OB_MAX_TRUNKS];   /* trunks we created (global L2 flush) */

static int is_lag(int dest) { return dest >= 0 && (dest & OB_LAG_DEST_BIT); }
static int lag_tid(int dest) { return dest & ~OB_LAG_DEST_BIT; }

static soc_knet_vectors_t knet_vect = {
    {bcm_knet_kcom_open, bcm_knet_kcom_close, bcm_knet_kcom_msg_send, bcm_knet_kcom_msg_recv},
    bde_irq_mask_set,
    bde_hw_unit_get};
static soc_chip_info_vectors_t chip_info_vect = {bde_icid_get};

const char* ob_sdk_errmsg(int rv) { return bcm_errmsg(rv); }

static void seterr(char* err, int errlen, const char* fmt, const char* a, int rv)
{
    if (err && errlen > 0) snprintf(err, errlen, fmt, a ? a : "", rv);
}

static bcm_rx_t rx_drop(int unit, bcm_pkt_t* pkt, void* cookie)
{
    (void)unit; (void)pkt; (void)cookie;
    g_rx_dropped++;   /* packets KNET did not take: no SAI packet API consumer */
    return BCM_RX_HANDLED;
}

static int knet_collect_netif(int unit, bcm_knet_netif_t* n, void* ud)
{
    int* ids = (int*)ud;
    (void)unit;
    if (ids[0] < 255) ids[++ids[0]] = n->id;
    return BCM_E_NONE;
}

static int knet_collect_filter(int unit, bcm_knet_filter_t* f, void* ud)
{
    int* ids = (int*)ud;
    (void)unit;
    /* Keep the SDK's own default filter (to the BCM RX API). */
    if (f->dest_type != BCM_KNET_DEST_T_BCM_RX_API && ids[0] < 255) ids[++ids[0]] = f->id;
    return BCM_E_NONE;
}

int ob_sdk_knet_cleanup(int unit, int* netifs, int* filters)
{
    int ids[256], i, rv;
    *netifs = *filters = 0;
    ids[0] = 0;
    rv = bcm_knet_filter_traverse(unit, knet_collect_filter, ids);
    if (rv < 0 && rv != BCM_E_UNAVAIL) return rv;
    for (i = 1; i <= ids[0]; i++)
        if (bcm_knet_filter_destroy(unit, ids[i]) >= 0) (*filters)++;
    ids[0] = 0;
    rv = bcm_knet_netif_traverse(unit, knet_collect_netif, ids);
    if (rv < 0 && rv != BCM_E_UNAVAIL) return rv;
    for (i = 1; i <= ids[0]; i++)
        if (bcm_knet_netif_destroy(unit, ids[i]) >= 0) (*netifs)++;
    return BCM_E_NONE;
}

/* One SDK process per unit: take /run/openbcm-unit<N>.lock for the life of
 * the process (bcm.user is wrapped with the same lock by the NixOS module). */
static int sdk_lock(int unit, char* err, int errlen)
{
    char path[64];
    if (sdk_lock_fd >= 0) return BCM_E_NONE;
    snprintf(path, sizeof(path), "/run/openbcm-unit%d.lock", unit);
    sdk_lock_fd = open(path, O_RDWR | O_CREAT | O_CLOEXEC, 0600);
    if (sdk_lock_fd < 0) {
        seterr(err, errlen, "cannot open SDK lock %s (%d)", path, errno);
        return BCM_E_INIT;
    }
    if (flock(sdk_lock_fd, LOCK_EX | LOCK_NB) != 0) {
        close(sdk_lock_fd);
        sdk_lock_fd = -1;
        seterr(err, errlen, "another SDK process owns the switch (lock %s held; "
               "e.g. systemctl stop openbcm-asic-temp)", path, 0);
        return BCM_E_BUSY;
    }
    return BCM_E_NONE;
}

/* SAL, BDE probe and attach, once per process. */
static int sdk_boot(int unit, const char* config_file, char* err, int errlen)
{
    static char tmpname[600];
    int rv;
    if (sdk_booted) return BCM_E_NONE;
    rv = sdk_lock(unit, err, errlen);
    if (rv < 0) return rv;
    snprintf(tmpname, sizeof(tmpname), "/tmp/sai-openbcm-config-%d.bcm.tmp", (int)getpid());
    if (sal_config_file_set(config_file, tmpname) < 0) {
        seterr(err, errlen, "sal_config_file_set(%s) failed (%d)", config_file, -1);
        return BCM_E_CONFIG;
    }
    if (sal_core_init() < 0 || sal_appl_init() < 0) {
        seterr(err, errlen, "SAL initialization failed%s (%d)", "", -1);
        return BCM_E_INIT;
    }
    soc_knet_config(&knet_vect);
    soc_chip_info_vect_config(&chip_info_vect);
    sal_boot_flags_set(sal_boot_flags_get() | BOOT_F_NO_RC);
    diag_sdk_init();   /* bde_create, probe, attach */
    sdk_booted = 1;
    return BCM_E_NONE;
}

#ifdef BCM_TOMAHAWK_SUPPORT
/* A chip nothing has initialised since power-on has an all-zero S-bus ring
 * map, so S-channel accesses to most blocks (TOP, where the PVTMONs are)
 * time out (seen on the AS7760 after a cold power cycle). Program the ring
 * map exactly as soc_tomahawk_chip_reset does, plus the PVTMON band-gap trim
 * it sets (BG_ADJ: 3 at power-on, SDK 0; reads ~3 C lower at 3). Routing and
 * sensor trim only: no PLL, port or forwarding change. Skipped when the ring
 * map is already set (the chip was initialised before). */
static int th_sbus_wake(int unit, int* woke)
{
    uint32 rval = 0;
    *woke = 0;
    if (!SOC_IS_TOMAHAWK(unit) || !soc_feature(unit, soc_feature_cmicm)) return BCM_E_NONE;
    SOC_IF_ERROR_RETURN(READ_CMIC_SBUS_RING_MAP_0_7r(unit, &rval));
    if (rval != 0) return BCM_E_NONE;
    WRITE_CMIC_SBUS_RING_MAP_0_7r(unit, 0x55222100);
    WRITE_CMIC_SBUS_RING_MAP_8_15r(unit, 0x30004005);
    WRITE_CMIC_SBUS_RING_MAP_16_23r(unit, 0x43333333);
    WRITE_CMIC_SBUS_RING_MAP_24_31r(unit, 0x44444444);
    WRITE_CMIC_SBUS_RING_MAP_32_39r(unit, 0x34444444);
    WRITE_CMIC_SBUS_RING_MAP_40_47r(unit, 0x03333333);
    WRITE_CMIC_SBUS_RING_MAP_48_55r(unit, 0x30000000);
    WRITE_CMIC_SBUS_RING_MAP_56_63r(unit, 0x00005000);
    WRITE_CMIC_SBUS_TIMEOUTr(unit, 0x7d0);
    sal_usleep(10000);
    SOC_IF_ERROR_RETURN(READ_TOP_PVTMON_CTRL_0r(unit, &rval));
    soc_reg_field_set(unit, TOP_PVTMON_CTRL_0r, &rval, BG_ADJf, 0);
    SOC_IF_ERROR_RETURN(WRITE_TOP_PVTMON_CTRL_0r(unit, rval));
    *woke = 1;
    return BCM_E_NONE;
}
#endif

int ob_sdk_attach(int unit, char* err, int errlen)
{
    /* No config: attach needs none, and nothing here may init the chip. */
    int rv = sdk_boot(unit, "/dev/null", err, errlen), woke = 0;
    if (rv < 0) return rv;
    if (unit < 0 || unit >= soc_ndev || !soc_attached(unit)) {
        seterr(err, errlen, "no attached switch unit%s (soc_ndev %d)", "", soc_ndev);
        return BCM_E_UNIT;
    }
#ifdef BCM_TOMAHAWK_SUPPORT
    rv = th_sbus_wake(unit, &woke);
    if (rv < 0) {
        seterr(err, errlen, "S-bus ring map setup failed%s: %d", "", rv);
        return rv;
    }
#endif
    if (err && errlen > 0)
        snprintf(err, errlen, "attached %s%s", soc_dev_name(unit),
                 woke ? " (uninitialised chip: S-bus ring map and PVTMON trim programmed)" : "");
    return BCM_E_NONE;
}

#ifdef BCM_TOMAHAWK_SUPPORT
extern int soc_tomahawk_temperature_monitor_get(int unit, int temperature_max,
    soc_switch_temperature_monitor_t* temperature_array, int* temperature_count);
#endif

int ob_sdk_temperatures(int unit, int* cur, int* peak, int max, int* count)
{
    bcm_switch_temperature_monitor_t t[16];
    int i, n = 0, rv;
    *count = 0;
    if (max > 16) max = 16;
    if (max <= 0) return BCM_E_PARAM;
    if (!sdk_booted || unit < 0 || unit >= soc_ndev || !soc_attached(unit)) return BCM_E_UNIT;
    if (sdk_inited) {
        rv = bcm_switch_temperature_monitor_get(unit, max, t, &n);
    } else {
#ifdef BCM_TOMAHAWK_SUPPORT
        /* Same struct (both are _shr_switch_temperature_monitor_t). */
        if (!SOC_IS_TOMAHAWK(unit)) return BCM_E_UNAVAIL;
        rv = soc_tomahawk_temperature_monitor_get(unit, max, (soc_switch_temperature_monitor_t*)t, &n);
#else
        return BCM_E_UNAVAIL;
#endif
    }
    if (rv < 0) return rv;
    for (i = 0; i < n && i < max; i++) {
        cur[i] = t[i].curr;
        peak[i] = t[i].peak;
    }
    *count = i;
    return BCM_E_NONE;
}

int ob_sdk_th_overtemp_shutdown(int unit)
{
#ifdef BCM_TOMAHAWK_SUPPORT
    uint32 rval = 0;
    int rv, rv2;
    if (!sdk_booted || unit < 0 || unit >= soc_ndev || !soc_attached(unit)) return BCM_E_UNIT;
    if (sdk_inited) return BCM_E_BUSY;
    if (!SOC_IS_TOMAHAWK(unit) || SOC_IS_TOMAHAWK2(unit)) return BCM_E_UNAVAIL;
    /* As soc_tomahawk_temperature_intr: the write is attempted even when
     * the read fails. */
    rv = READ_TOP_CORE_PLL0_CTRL_1r(unit, &rval);
    soc_reg_field_set(unit, TOP_CORE_PLL0_CTRL_1r, &rval, ISO_INf, 1);
    soc_reg_field_set(unit, TOP_CORE_PLL0_CTRL_1r, &rval, PWRONf, 0);
    soc_reg_field_set(unit, TOP_CORE_PLL0_CTRL_1r, &rval, PWRON_LDOf, 0);
    rv2 = WRITE_TOP_CORE_PLL0_CTRL_1r(unit, rval);
    return rv2 < 0 ? rv2 : rv;
#else
    (void)unit;
    return BCM_E_UNAVAIL;
#endif
}

int ob_sdk_init(int unit, const char* config_file, const char* rc_file, char* err, int errlen)
{
    int rv, nn = 0, nf = 0, l3_ports = 0;
    char cmd[512];

    if (!config_file || !*config_file || access(config_file, R_OK) != 0) {
        seterr(err, errlen, "SDK config file '%s' missing or unreadable (%d)", config_file, errno);
        return BCM_E_CONFIG;
    }
    if (rc_file && *rc_file && access(rc_file, R_OK) != 0) {
        seterr(err, errlen, "rc file '%s' missing or unreadable (%d)", rc_file, errno);
        return BCM_E_CONFIG;
    }
    rv = sdk_boot(unit, config_file, err, errlen);
    if (rv < 0) return rv;
    if (unit < 0 || unit >= soc_ndev || !soc_attached(unit)) {
        seterr(err, errlen, "no attached switch unit%s (soc_ndev %d)", "", soc_ndev);
        return BCM_E_UNIT;
    }
#ifndef ALPM_ENABLE
    /* The soc layer sizes the LPM tables for ALPM from l3_alpm_enable even
     * when the BCM layer is built without ALPM, which then programs routes
     * the chip never looks up. Refuse instead of routing nothing. */
    if (soc_property_get(unit, spn_L3_ALPM_ENABLE, 0)) {
        seterr(err, errlen, "config sets l3_alpm_enable but this SDK build has no ALPM%s (%d)", "", 0);
        return BCM_E_CONFIG;
    }
#endif
    system_shutdown(unit, 0);
    system_init(unit);   /* "init all"; result is kept in BCM_CONTROL */
    rv = BCM_CONTROL(unit) ? BCM_CONTROL(unit)->system_initialized_rv : BCM_E_INIT;
    if (rv < 0) {
        seterr(err, errlen, "init all failed%s: %d", "", rv);
        return rv;
    }
    if (rc_file && *rc_file) {
        snprintf(cmd, sizeof(cmd), "rcload %s", rc_file);
        if (sh_process_command(unit, cmd) != CMD_OK) {
            seterr(err, errlen, "rc file '%s' failed (%d)", rc_file, -1);
            return BCM_E_FAIL;
        }
    }
    rv = ob_sdk_knet_cleanup(unit, &nn, &nf);
    if (rv < 0) {
        seterr(err, errlen, "KNET cleanup failed%s: %d", "", rv);
        return rv;
    }
    if (!bcm_rx_active(unit)) {
        rv = bcm_rx_start(unit, NULL);
        if (rv < 0) {
            seterr(err, errlen, "bcm_rx_start failed%s: %d", "", rv);
            return rv;
        }
    }
    bcm_rx_unregister(unit, rx_drop, BCM_RX_PRIO_MIN);
    rv = bcm_rx_register(unit, "sai-openbcm-drop", rx_drop, BCM_RX_PRIO_MIN, NULL, BCM_RCO_F_ALL_COS);
    if (rv < 0) {
        seterr(err, errlen, "bcm_rx_register failed%s: %d", "", rv);
        return rv;
    }
    rv = bcm_switch_control_set(unit, bcmSwitchL3EgressMode, 1);
    if (rv < 0) {
        seterr(err, errlen, "L3 egress mode failed%s: %d", "", rv);
        return rv;
    }
    /* IPv4/IPv6 routing on every front-panel port (PORT_TAB V4L3_ENABLE /
     * V6L3_ENABLE; nothing in "init all" sets them). Whether a frame is
     * routed is then decided by the router MAC (my-station entry) and the
     * L3 interface of its VLAN. */
    {
        bcm_port_config_t pc;
        bcm_port_t p;
        l3_ports = 0;
        rv = bcm_port_config_get(unit, &pc);
        if (rv < 0) {
            seterr(err, errlen, "port config failed%s: %d", "", rv);
            return rv;
        }
        BCM_PBMP_ITER(pc.port, p) {
            rv = bcm_port_control_set(unit, p, bcmPortControlIP4, 1);
            if (rv >= 0) rv = bcm_port_control_set(unit, p, bcmPortControlIP6, 1);
            if (rv < 0 && rv != BCM_E_UNAVAIL) {
                seterr(err, errlen, "L3 enable on port %s failed: %d", SOC_PORT_NAME(unit, p), rv);
                return rv;
            }
            if (rv >= 0) l3_ports++;
        }
    }
    g_cpu_egress = g_drop_egress = -1;
    g_l2c_index[0] = g_l2c_index[1] = -1;   /* "init all" rebuilt the L2 user table */
    memset(g_trunk_used, 0, sizeof(g_trunk_used));
    sdk_inited = 1;
    if (err && errlen > 0)
        snprintf(err, errlen, "ok: stale KNET netifs %d filters %d destroyed; IPv4/IPv6 L3 on %d ports", nn, nf, l3_ports);
    return BCM_E_NONE;
}

static void link_handler(int unit, bcm_port_t port, bcm_port_info_t* info)
{
    (void)unit;
    if (g_link_cb && info) g_link_cb(g_cb_ctx, port, info->linkstatus == BCM_PORT_LINK_STATUS_UP);
}

static void l2_handler(int unit, bcm_l2_addr_t* l2, int op, void* ud)
{
    int learned;
    (void)unit; (void)ud;
    if (!g_l2_cb || !l2 || (l2->flags & BCM_L2_STATIC)) return;
    if (op == BCM_L2_CALLBACK_ADD || op == BCM_L2_CALLBACK_LEARN_EVENT || op == BCM_L2_CALLBACK_MOVE_EVENT)
        learned = 1;
    else if (op == BCM_L2_CALLBACK_DELETE || op == BCM_L2_CALLBACK_AGE_EVENT)
        learned = 0;
    else
        return;
    g_l2_cb(g_cb_ctx, l2->mac, l2->vid,
            (l2->flags & BCM_L2_TRUNK_MEMBER) ? (OB_LAG_DEST_BIT | l2->tgid) : l2->port, learned);
}

int ob_sdk_register_callbacks(int unit, ob_link_cb_t link, ob_l2_cb_t l2, void* ctx)
{
    int rv;
    g_link_cb = link;
    g_l2_cb = l2;
    g_cb_ctx = ctx;
    rv = bcm_linkscan_register(unit, link_handler);
    if (rv < 0) return rv;
    rv = bcm_l2_addr_register(unit, l2_handler, NULL);
    return rv;
}

void ob_sdk_unregister_callbacks(int unit)
{
    bcm_linkscan_unregister(unit, link_handler);
    bcm_l2_addr_unregister(unit, l2_handler, NULL);
    g_link_cb = NULL;
    g_l2_cb = NULL;
}

void ob_sdk_shutdown(int unit)
{
    ob_sdk_unregister_callbacks(unit);
    bcm_rx_unregister(unit, rx_drop, BCM_RX_PRIO_MIN);
    /* The SDK stays attached for the life of the process (no clean detach
     * path in socdiag either); a new create_switch re-runs "init all". */
}

int ob_sdk_shell(int unit, const char* cmd)
{
    char buf[1024];
    snprintf(buf, sizeof(buf), "%s", cmd);
    return sh_process_command(unit, buf) == CMD_OK ? BCM_E_NONE : BCM_E_FAIL;
}

/* One diag shell command with the shell's output (it prints to stdout)
 * going to fd. Callers serialize (one command at a time). Output other
 * threads print meanwhile lands in fd too. */
int ob_sdk_shell_to(int unit, const char* cmd, int fd)
{
    int saved, rv;
    fflush(stdout);
    saved = dup(1);
    if (saved < 0) return BCM_E_INTERNAL;
    if (dup2(fd, 1) < 0) {
        close(saved);
        return BCM_E_INTERNAL;
    }
    rv = ob_sdk_shell(unit, cmd);
    fflush(stdout);
    dup2(saved, 1);
    close(saved);
    return rv;
}

int ob_sdk_hw_info(int unit, char* buf, int len)
{
    snprintf(buf, len, "%s", soc_dev_name(unit));
    return BCM_E_NONE;
}

static uint32_t left(int max, int used) { return max > used ? (uint32_t)(max - used) : 0; }

int ob_sdk_table_usage(int unit, ob_table_usage_t* u)
{
    int v4 = 0, v6 = 0, l2max = 0, l2cur = 0, rv;
    bcm_l3_info_t li;
    memset(u, 0, sizeof(*u));
    if ((rv = bcm_switch_object_count_get(unit, bcmSwitchObjectL3RouteV4RoutesFree, &v4)) < 0) return rv;
    if ((rv = bcm_switch_object_count_get(unit, bcmSwitchObjectL3RouteV6Routes64bFree, &v6)) < 0) return rv;
    if ((rv = bcm_switch_object_count_get(unit, bcmSwitchObjectL2EntryMax, &l2max)) < 0) return rv;
    if ((rv = bcm_switch_object_count_get(unit, bcmSwitchObjectL2EntryCurrent, &l2cur)) < 0) return rv;
    bcm_l3_info_t_init(&li);
    if ((rv = bcm_l3_info(unit, &li)) < 0) return rv;
    u->v4_routes_free = v4 > 0 ? (uint32_t)v4 : 0;
    u->v6_routes_free = v6 > 0 ? (uint32_t)v6 : 0;
    u->hosts_free = left(li.l3info_max_host, li.l3info_occupied_host);
    u->egress_free = left(li.l3info_max_nexthop, li.l3info_used_nexthop);
    u->fdb_free = left(l2max, l2cur);
    {
        int gmax = 0, gcur = 0;
        if ((rv = bcm_switch_object_count_get(unit, bcmSwitchObjectEcmpMax, &gmax)) < 0) return rv;
        if ((rv = bcm_switch_object_count_get(unit, bcmSwitchObjectEcmpCurrent, &gcur)) < 0) return rv;
        u->ecmp_groups_free = left(gmax, gcur);
    }
    {
        /* L3_ECMP entries the SDK has handed out (a group reserves its
         * entries), as bcmSwitchObjectEcmpMembersUsed counts them on the
         * chips that support that object (not the BCM56960). */
        _bcm_l3_tbl_t* t = BCM_XGS3_L3_TBL_PTR(unit, ecmp);
        int i, used = 0;
        if (t == NULL || t->ext_arr == NULL) return BCM_E_INIT;
        for (i = 0; i <= t->idx_max; i++)
            if (BCM_XGS3_L3_ENT_REF_CNT(t, i)) used++;
        u->ecmp_members_free = left(t->idx_max + 1, used);
    }
    return BCM_E_NONE;
}

int ob_sdk_chip_limits(int unit, ob_chip_limits_t* l)
{
    bcm_l3_info_t li;
    int rv;
    memset(l, 0, sizeof(*l));
    if (!SOC_IS_TOMAHAWK(unit)) return BCM_E_UNAVAIL;   /* MMU geometry below is the BCM56960's */
    l->buffer_bytes = (uint64_t)_TH_MMU_TOTAL_CELLS * _TH_MMU_BYTES_PER_CELL;
    l->acl_prio_min = BCM_FIELD_ENTRY_PRIO_LOWEST;
    l->acl_prio_max = BCM_FIELD_ENTRY_PRIO_HIGHEST;
    bcm_l3_info_t_init(&li);
    if ((rv = bcm_l3_info(unit, &li)) < 0) return rv;
    l->max_host = li.l3info_max_host;
    l->max_route = li.l3info_max_route;
    l->max_nexthop = li.l3info_max_nexthop;
    l->ecmp_groups = li.l3info_max_ecmp_groups;
    l->ecmp_paths = li.l3info_max_ecmp;
    l->vrf_max = li.l3info_max_vrf;
    {
        _bcm_l3_tbl_t* t = BCM_XGS3_L3_TBL_PTR(unit, ecmp);
        l->ecmp_members = (t != NULL && t->ext_arr != NULL) ? t->idx_max + 1 : 0;
    }
    if ((rv = bcm_switch_object_count_get(unit, bcmSwitchObjectL2EntryMax, &l->max_l2)) < 0) return rv;
    return BCM_E_NONE;
}

int ob_sdk_cpu_port(int unit) { return CMIC_PORT(unit); }

int ob_sdk_ports(int unit, ob_port_info_t* out, int max)
{
    bcm_port_config_t cfg;
    bcm_pbmp_t pbm;
    bcm_port_t p;
    int n = 0, rv;
    rv = bcm_port_config_get(unit, &cfg);
    if (rv < 0) return rv;
    BCM_PBMP_ASSIGN(pbm, cfg.port);
    BCM_PBMP_REMOVE(pbm, PBMP_MANAGEMENT(unit));   /* Tomahawk xe management ports */
    BCM_PBMP_ITER(pbm, p) {
        int speed = 0;
        if (n >= max) break;
        memset(&out[n], 0, sizeof(out[n]));
        out[n].port = p;
        out[n].phys = SOC_INFO(unit).port_l2p_mapping[p];
        out[n].lanes = SOC_INFO(unit).port_num_lanes[p];
        if (out[n].lanes <= 0) out[n].lanes = 1;
        bcm_port_speed_get(unit, p, &speed);
        out[n].speed = speed;
        snprintf(out[n].name, sizeof(out[n].name), "%s", SOC_PORT_NAME(unit, p));
        n++;
    }
    return n;
}

int ob_sdk_port_enable_set(int unit, int port, int enable) { return bcm_port_enable_set(unit, port, enable); }

int ob_sdk_port_link_get(int unit, int port, int* up)
{
    int st = 0, rv = bcm_port_link_status_get(unit, port, &st);
    *up = st == BCM_PORT_LINK_STATUS_UP;
    return rv;
}

int ob_sdk_port_speed_set(int unit, int port, int mbps) { return bcm_port_speed_set(unit, port, mbps); }
int ob_sdk_port_speed_get(int unit, int port, int* mbps) { return bcm_port_speed_get(unit, port, mbps); }

int ob_sdk_port_speeds(int unit, int port, uint32_t* out, int max)
{
    static const struct { bcm_port_abil_t bit; uint32_t mbps; } map[] = {
        {BCM_PORT_ABILITY_1000MB, 1000}, {BCM_PORT_ABILITY_10GB, 10000}, {BCM_PORT_ABILITY_20GB, 20000},
        {BCM_PORT_ABILITY_25GB, 25000}, {BCM_PORT_ABILITY_40GB, 40000}, {BCM_PORT_ABILITY_50GB, 50000},
        {BCM_PORT_ABILITY_100GB, 100000}};
    bcm_port_ability_t ab;
    int i, n = 0, rv;
    memset(&ab, 0, sizeof(ab));
    rv = bcm_port_ability_local_get(unit, port, &ab);
    if (rv < 0) return rv;
    for (i = 0; i < (int)(sizeof(map) / sizeof(map[0])) && n < max; i++)
        if (ab.speed_full_duplex & map[i].bit) out[n++] = map[i].mbps;
    return n;
}

int ob_sdk_port_frame_max_set(int unit, int port, int bytes) { return bcm_port_frame_max_set(unit, port, bytes); }

int ob_sdk_port_fec_set(int unit, int port, int mode)
{
    int speed = 0, rv, rv2;
    bcm_port_speed_get(unit, port, &speed);
    if (mode == 0) {
        rv = bcm_port_phy_control_set(unit, port, BCM_PORT_PHY_CONTROL_FORWARD_ERROR_CORRECTION_CL91, 0);
        rv2 = bcm_port_phy_control_set(unit, port, BCM_PORT_PHY_CONTROL_FORWARD_ERROR_CORRECTION,
                                       BCM_PORT_PHY_CONTROL_FEC_OFF);
        if (rv < 0 && rv2 < 0) return rv;
        return BCM_E_NONE;
    }
    if (mode == 1)
        return bcm_port_phy_control_set(unit, port,
                                        speed >= 100000 ? BCM_PORT_PHY_CONTROL_FORWARD_ERROR_CORRECTION_CL91
                                                        : BCM_PORT_PHY_CONTROL_FORWARD_ERROR_CORRECTION_CL108,
                                        1);
    if (mode == 2)
        return bcm_port_phy_control_set(unit, port, BCM_PORT_PHY_CONTROL_FORWARD_ERROR_CORRECTION,
                                        BCM_PORT_PHY_CONTROL_FEC_ON);
    return BCM_E_PARAM;
}

int ob_sdk_port_autoneg_set(int unit, int port, int on) { return bcm_port_autoneg_set(unit, port, on); }
int ob_sdk_port_autoneg_get(int unit, int port, int* on) { return bcm_port_autoneg_get(unit, port, on); }

int ob_sdk_port_loopback_set(int unit, int port, int mode)
{
    int lb = mode == 1 ? BCM_PORT_LOOPBACK_MAC : mode == 2 ? BCM_PORT_LOOPBACK_PHY : BCM_PORT_LOOPBACK_NONE;
    return bcm_port_loopback_set(unit, port, lb);
}

int ob_sdk_port_pvid_set(int unit, int port, int vid) { return bcm_port_untagged_vlan_set(unit, port, (bcm_vlan_t)vid); }

int ob_sdk_port_learn_set(int unit, int port, int mode)
{
    uint32 f;
    switch (mode) {
    case 1: f = BCM_PORT_LEARN_FWD; break;
    case 2: f = 0; break;
    case 3: f = BCM_PORT_LEARN_CPU | BCM_PORT_LEARN_FWD; break;
    default: f = BCM_PORT_LEARN_ARL | BCM_PORT_LEARN_FWD; break;
    }
    return bcm_port_learn_set(unit, port, f);
}

int ob_sdk_port_stat_get(int unit, int port, int which, uint64_t* v)
{
    /* Order of ob::PortStat. */
    static const bcm_stat_val_t map[] = {
        snmpIfHCInOctets, snmpIfHCInUcastPkts, snmpIfInNUcastPkts, snmpIfInDiscards, snmpIfInErrors,
        snmpIfInUnknownProtos, snmpIfHCInBroadcastPkts, snmpIfHCInMulticastPkts, snmpIfHCOutOctets,
        snmpIfHCOutUcastPkts, snmpIfOutNUcastPkts, snmpIfOutDiscards, snmpIfOutErrors, snmpIfHCOutBroadcastPckts,
        snmpIfHCOutMulticastPkts};
    uint64 val = 0;
    int rv;
    if (which < 0 || which >= (int)(sizeof(map) / sizeof(map[0]))) return BCM_E_PARAM;
    rv = bcm_stat_get(unit, port, map[which], &val);
    *v = (uint64_t)val;
    return rv;
}

int ob_sdk_port_stat_clear(int unit, int port) { return bcm_stat_clear(unit, port); }

static void cpu_pbmp(int unit, bcm_pbmp_t* pbm)
{
    BCM_PBMP_CLEAR(*pbm);
    BCM_PBMP_PORT_ADD(*pbm, CMIC_PORT(unit));
}

int ob_sdk_vlan_create(int unit, int vid)
{
    bcm_pbmp_t pbm, ubm;
    int rv = bcm_vlan_create(unit, (bcm_vlan_t)vid);
    if (rv < 0) return rv;
    cpu_pbmp(unit, &pbm);
    BCM_PBMP_CLEAR(ubm);
    rv = bcm_vlan_port_add(unit, (bcm_vlan_t)vid, pbm, ubm);   /* CPU tagged: floods reach KNET */
    if (rv < 0) bcm_vlan_destroy(unit, (bcm_vlan_t)vid);
    return rv;
}

int ob_sdk_vlan_destroy(int unit, int vid) { return bcm_vlan_destroy(unit, (bcm_vlan_t)vid); }

int ob_sdk_vlan_port_add(int unit, int vid, int port, int untagged)
{
    bcm_pbmp_t pbm, ubm;
    BCM_PBMP_CLEAR(pbm);
    BCM_PBMP_CLEAR(ubm);
    BCM_PBMP_PORT_ADD(pbm, port);
    if (untagged) BCM_PBMP_PORT_ADD(ubm, port);
    bcm_vlan_port_remove(unit, (bcm_vlan_t)vid, pbm);   /* re-add: tagging may change */
    return bcm_vlan_port_add(unit, (bcm_vlan_t)vid, pbm, ubm);
}

int ob_sdk_vlan_port_remove(int unit, int vid, int port)
{
    bcm_pbmp_t pbm;
    BCM_PBMP_CLEAR(pbm);
    BCM_PBMP_PORT_ADD(pbm, port);
    return bcm_vlan_port_remove(unit, (bcm_vlan_t)vid, pbm);
}

int ob_sdk_vlan_members(int unit, int vid, int* ports, int* untagged, int max)
{
    bcm_pbmp_t pbm, ubm;
    bcm_port_t p;
    int n = 0, rv = bcm_vlan_port_get(unit, (bcm_vlan_t)vid, &pbm, &ubm);
    if (rv < 0) return rv;
    BCM_PBMP_ITER(pbm, p) {
        if (p == CMIC_PORT(unit) || n >= max) continue;
        ports[n] = p;
        untagged[n] = BCM_PBMP_MEMBER(ubm, p) ? 1 : 0;
        n++;
    }
    return n;
}

static int my_modid(int unit)
{
    int m = 0;
    if (bcm_stk_my_modid_get(unit, &m) < 0) m = 0;
    return m;
}

int ob_sdk_l2_add(int unit, const uint8_t mac[6], int vid, int port, int is_static, int discard)
{
    bcm_l2_addr_t l2;
    bcm_mac_t m;
    memcpy(m, mac, 6);
    bcm_l2_addr_t_init(&l2, m, (bcm_vlan_t)vid);
    if (is_lag(port)) {
        l2.flags |= BCM_L2_TRUNK_MEMBER;
        l2.tgid = lag_tid(port);
    } else {
        l2.port = port;
        l2.modid = my_modid(unit);
    }
    if (is_static) l2.flags |= BCM_L2_STATIC;
    if (discard) l2.flags |= BCM_L2_DISCARD_DST;
    return bcm_l2_addr_add(unit, &l2);
}

int ob_sdk_l2_delete(int unit, const uint8_t mac[6], int vid)
{
    bcm_mac_t m;
    memcpy(m, mac, 6);
    return bcm_l2_addr_delete(unit, m, (bcm_vlan_t)vid);
}

int ob_sdk_l2_flush(int unit, int port, int vid, int statics, int dynamics)
{
    uint32 flags;
    int modid = my_modid(unit);
    if (statics && !dynamics) return BCM_E_UNAVAIL;   /* SDK deletes dynamic with static, never static alone */
    if (!statics && !dynamics) return BCM_E_NONE;
    flags = statics ? BCM_L2_DELETE_STATIC : 0;
    if (is_lag(port)) {
        if (vid >= 0) return bcm_l2_addr_delete_by_vlan_trunk(unit, (bcm_vlan_t)vid, lag_tid(port), flags);
        return bcm_l2_addr_delete_by_trunk(unit, lag_tid(port), flags);
    }
    if (port >= 0 && vid >= 0) return bcm_l2_addr_delete_by_vlan_port(unit, (bcm_vlan_t)vid, modid, port, flags);
    if (port >= 0) return bcm_l2_addr_delete_by_port(unit, modid, port, flags);
    if (vid >= 0) return bcm_l2_addr_delete_by_vlan(unit, (bcm_vlan_t)vid, flags);
    {
        bcm_port_config_t cfg;
        bcm_port_t p;
        int rv = bcm_port_config_get(unit, &cfg);
        if (rv < 0) return rv;
        BCM_PBMP_ITER(cfg.port, p) {
            rv = bcm_l2_addr_delete_by_port(unit, modid, p, flags);
            if (rv < 0 && rv != BCM_E_NOT_FOUND) return rv;
        }
        for (p = 0; p < OB_MAX_TRUNKS; p++) {
            if (!g_trunk_used[p]) continue;
            rv = bcm_l2_addr_delete_by_trunk(unit, p, flags);
            if (rv < 0 && rv != BCM_E_NOT_FOUND) return rv;
        }
    }
    return BCM_E_NONE;
}

int ob_sdk_l2_age_set(int unit, int seconds) { return bcm_l2_age_timer_set(unit, seconds); }

static int filter_create(int unit, const char* name, int netif_id, int port, int keep_tag, int* filter_id)
{
    bcm_knet_filter_t f;
    int rv;
    bcm_knet_filter_t_init(&f);
    f.type = BCM_KNET_FILTER_T_RX_PKT;
    f.priority = 0;
    f.dest_type = BCM_KNET_DEST_T_NETIF;
    f.dest_id = netif_id;
    f.match_flags = BCM_KNET_FILTER_M_INGPORT;
    f.m_ingport = port;
    if (!keep_tag) f.flags |= BCM_KNET_FILTER_F_STRIP_TAG;
    snprintf(f.desc, sizeof(f.desc), "%s", name);
    rv = bcm_knet_filter_create(unit, &f);
    if (rv >= 0) *filter_id = f.id;
    return rv;
}

int ob_sdk_netif_create(int unit, const char* name, int port, int vid, const uint8_t mac[6], int keep_tag,
                        int* netif_id, int* filter_id)
{
    bcm_knet_netif_t n;
    int rv;
    bcm_knet_netif_t_init(&n);
    n.type = BCM_KNET_NETIF_T_TX_LOCAL_PORT;
    n.port = port;
    n.vlan = (bcm_vlan_t)vid;
    memcpy(n.mac_addr, mac, 6);
    snprintf(n.name, sizeof(n.name), "%s", name);
    if (keep_tag) n.flags |= BCM_KNET_NETIF_F_KEEP_RX_TAG;
    rv = bcm_knet_netif_create(unit, &n);
    if (rv < 0) return rv;
    rv = filter_create(unit, name, n.id, port, keep_tag, filter_id);
    if (rv < 0) {
        bcm_knet_netif_destroy(unit, n.id);
        return rv;
    }
    *netif_id = n.id;
    return BCM_E_NONE;
}

int ob_sdk_netif_destroy(int unit, int netif_id, int filter_id)
{
    int rv = BCM_E_NONE;
    if (filter_id >= 0) bcm_knet_filter_destroy(unit, filter_id);
    if (netif_id >= 0) rv = bcm_knet_netif_destroy(unit, netif_id);
    return rv;
}

int ob_sdk_netif_filter_set(int unit, int netif_id, int port, int keep_tag, int* filter_id)
{
    bcm_knet_netif_t n;
    int rv, nf = -1;
    rv = bcm_knet_netif_get(unit, netif_id, &n);
    if (rv < 0) return rv;
    rv = filter_create(unit, n.name, netif_id, port, keep_tag, &nf);
    if (rv < 0) return rv;
    if (*filter_id >= 0) bcm_knet_filter_destroy(unit, *filter_id);
    *filter_id = nf;
    return BCM_E_NONE;
}

/* An exact L2 user ("cache") entry for a link-local protocol MAC: BPDU (so
 * it also passes on blocked / not-collecting ports) and either to the CPU
 * only or discarded. The SDK's own entries from "init all" (01:80:c2:00:00:0x
 * masked, to the CPU) sit at the end of the table; exact ones match first. */
static int l2cache_trap(int unit, int which, const uint8_t mac[6], int to_cpu)
{
    bcm_l2_cache_addr_t a;
    int used = -1, rv;
    bcm_l2_cache_addr_t_init(&a);
    memcpy(a.mac, mac, 6);
    memset(a.mac_mask, 0xff, 6);
    a.flags = BCM_L2_CACHE_BPDU | (to_cpu ? BCM_L2_CACHE_CPU : BCM_L2_CACHE_DISCARD);
    a.dest_modid = my_modid(unit);
    a.dest_port = CMIC_PORT(unit);
    rv = bcm_l2_cache_set(unit, g_l2c_index[which], &a, &used);
    if (rv >= 0) g_l2c_index[which] = used;
    return rv;
}

int ob_sdk_trap_set(int unit, int kind, int to_cpu)
{
    static const uint8_t lldp[6] = {0x01, 0x80, 0xc2, 0x00, 0x00, 0x0e};
    static const uint8_t lacp[6] = {0x01, 0x80, 0xc2, 0x00, 0x00, 0x02};   /* slow protocols */
    if (kind == 7) return l2cache_trap(unit, 0, lldp, to_cpu);           /* ob::TrapKind::Lldp */
    if (kind == 8) return l2cache_trap(unit, 1, lacp, to_cpu);           /* ob::TrapKind::Lacp */
    /* Order of ob::TrapKind. -1: no switch control on this SDK path. */
    static const int ctl[] = {
        bcmSwitchArpRequestToCpu, bcmSwitchArpReplyToCpu, bcmSwitchDhcpPktToCpu, -1, bcmSwitchIgmpPktToCpu,
        bcmSwitchMldPktToCpu, bcmSwitchNdPktToCpu, -1, -1, -1, -1};
    if (kind < 0 || kind >= (int)(sizeof(ctl) / sizeof(ctl[0])) || ctl[kind] < 0) return BCM_E_UNAVAIL;
    return bcm_switch_control_set(unit, (bcm_switch_control_t)ctl[kind], to_cpu ? 1 : 0);
}

int ob_sdk_trunk_create(int unit, int* tid)
{
    bcm_trunk_t t = 0;
    int rv = bcm_trunk_create(unit, 0, &t);
    if (rv < 0) return rv;
    if (t < 0 || t >= OB_MAX_TRUNKS) {
        bcm_trunk_destroy(unit, t);
        return BCM_E_RESOURCE;
    }
    g_trunk_used[t] = 1;
    *tid = t;
    return BCM_E_NONE;
}

int ob_sdk_trunk_destroy(int unit, int tid)
{
    int rv = bcm_trunk_destroy(unit, (bcm_trunk_t)tid);
    if (rv >= 0 && tid >= 0 && tid < OB_MAX_TRUNKS) g_trunk_used[tid] = 0;
    return rv;
}

int ob_sdk_trunk_set(int unit, int tid, const int* ports, const int* flags, int n)
{
    bcm_trunk_info_t info;
    bcm_trunk_member_t m[64];
    int i, modid = my_modid(unit);
    if (n < 0 || n > 64) return BCM_E_PARAM;
    if (n == 0) return bcm_trunk_member_delete_all(unit, (bcm_trunk_t)tid);   /* trunk_set needs >= 1 */
    bcm_trunk_info_t_init(&info);
    info.psc = BCM_TRUNK_PSC_SRCDSTIP;   /* RTAG 6, source + destination IP (not tuned) */
    info.dlf_index = info.mc_index = info.ipmc_index = BCM_TRUNK_UNSPEC_INDEX;
    for (i = 0; i < n; i++) {
        bcm_trunk_member_t_init(&m[i]);
        BCM_GPORT_MODPORT_SET(m[i].gport, modid, ports[i]);
        if (flags[i] & 1) m[i].flags |= BCM_TRUNK_MEMBER_EGRESS_DISABLE;
        if (flags[i] & 2) m[i].flags |= BCM_TRUNK_MEMBER_INGRESS_DISABLE;
    }
    return bcm_trunk_set(unit, (bcm_trunk_t)tid, &info, n, m);
}

/* The VLAN of an L3 interface routes IPv4 and IPv6 (VLAN profile
 * IPV4L3_ENABLE / IPV6L3_ENABLE); changed only when the VLAN has it off. */
static int vlan_l3_enable(int unit, int vid)
{
    bcm_vlan_control_vlan_t vc;
    int rv;
    bcm_vlan_control_vlan_t_init(&vc);
    rv = bcm_vlan_control_vlan_get(unit, (bcm_vlan_t)vid, &vc);
    if (rv < 0) return rv;
    if (!(vc.flags & (BCM_VLAN_IP4_DISABLE | BCM_VLAN_IP6_DISABLE))) return BCM_E_NONE;
    vc.flags &= ~(BCM_VLAN_IP4_DISABLE | BCM_VLAN_IP6_DISABLE);
    return bcm_vlan_control_vlan_set(unit, (bcm_vlan_t)vid, vc);
}

/* The SDK binds the interface's VLAN to l3a_vrf (VLAN_TAB VRF_ID or the
 * L3_IIF entry of the VLAN): packets routed in on that VLAN look up that VRF.
 * The router MAC is a my-station entry (ob_sdk_l3_station_add), not an L2
 * entry with the L3 bit (BCM_L3_ADD_TO_ARL): such a static entry sits on the
 * CPU port, and PORT_TAB's static-move drop would then discard every frame
 * arriving with SA = router MAC (all of them on a cable looped between two
 * ports of this switch). */
int ob_sdk_l3_intf_create(int unit, const uint8_t mac[6], int vid, int mtu, int vrf, int* intf_id)
{
    bcm_l3_intf_t i;
    int rv;
    rv = vlan_l3_enable(unit, vid);
    if (rv < 0) return rv;
    bcm_l3_intf_t_init(&i);
    memcpy(i.l3a_mac_addr, mac, 6);
    i.l3a_vid = (bcm_vlan_t)vid;
    i.l3a_mtu = mtu;
    i.l3a_vrf = (bcm_vrf_t)vrf;
    rv = bcm_l3_intf_create(unit, &i);
    if (rv >= 0) *intf_id = i.l3a_intf_id;
    return rv;
}

int ob_sdk_l3_intf_update(int unit, int intf_id, const uint8_t mac[6], int vid, int mtu, int vrf)
{
    bcm_l3_intf_t i;
    bcm_l3_intf_t_init(&i);
    i.l3a_flags = BCM_L3_WITH_ID | BCM_L3_REPLACE;
    i.l3a_intf_id = intf_id;
    memcpy(i.l3a_mac_addr, mac, 6);
    i.l3a_vid = (bcm_vlan_t)vid;
    i.l3a_mtu = mtu;
    i.l3a_vrf = (bcm_vrf_t)vrf;
    return bcm_l3_intf_create(unit, &i);
}

int ob_sdk_l3_intf_destroy(int unit, int intf_id)
{
    bcm_l3_intf_t i;
    bcm_l3_intf_t_init(&i);
    i.l3a_intf_id = intf_id;
    return bcm_l3_intf_delete(unit, &i);
}

int ob_sdk_l3_station_add(int unit, const uint8_t mac[6], int* station_id)
{
    bcm_l2_station_t s;
    bcm_l2_station_t_init(&s);
    memcpy(s.dst_mac, mac, 6);
    memset(s.dst_mac_mask, 0xff, 6);
    /* ARP_RARP: ARP to the router MAC is terminated, which the ARP reply
     * trap (bcmSwitchArpReplyToCpu) needs; without it unicast ARP replies to
     * the router MAC were L2-switched (and dropped on the looped cable). */
    s.flags = BCM_L2_STATION_IPV4 | BCM_L2_STATION_IPV6 | BCM_L2_STATION_ARP_RARP;
    return bcm_l2_station_add(unit, station_id, &s);
}

int ob_sdk_l3_station_delete(int unit, int station_id) { return bcm_l2_station_delete(unit, station_id); }

static void egress_fill(int unit, bcm_l3_egress_t* e, int intf, const uint8_t mac[6], int port, int vid)
{
    bcm_l3_egress_t_init(e);
    e->intf = intf;
    memcpy(e->mac_addr, mac, 6);
    if (is_lag(port)) {
        e->flags |= BCM_L3_TGID;
        e->trunk = lag_tid(port);
    } else {
        e->port = port;
        e->module = my_modid(unit);
    }
    e->vlan = (bcm_vlan_t)vid;
}

int ob_sdk_l3_egress_create(int unit, int intf, const uint8_t mac[6], int port, int vid, int* egress_id)
{
    bcm_l3_egress_t e;
    bcm_if_t id = 0;
    int rv;
    egress_fill(unit, &e, intf, mac, port, vid);
    rv = bcm_l3_egress_create(unit, 0, &e, &id);
    if (rv >= 0) *egress_id = id;
    return rv;
}

int ob_sdk_l3_egress_update(int unit, int egress_id, int intf, const uint8_t mac[6], int port, int vid)
{
    bcm_l3_egress_t e;
    bcm_if_t id = egress_id;
    egress_fill(unit, &e, intf, mac, port, vid);
    return bcm_l3_egress_create(unit, BCM_L3_REPLACE | BCM_L3_WITH_ID, &e, &id);
}

int ob_sdk_l3_egress_special(int unit, int drop, int* egress_id)
{
    bcm_l3_egress_t e;
    bcm_if_t id = 0;
    int rv;
    int* cache = drop ? &g_drop_egress : &g_cpu_egress;
    if (*cache >= 0) { *egress_id = *cache; return BCM_E_NONE; }
    bcm_l3_egress_t_init(&e);
    e.flags = BCM_L3_L2TOCPU | (drop ? BCM_L3_DST_DISCARD : 0);   /* SDK fills CPU port + L2CPU intf */
    rv = bcm_l3_egress_create(unit, 0, &e, &id);
    if (rv < 0) return rv;
    *cache = *egress_id = id;
    return BCM_E_NONE;
}

int ob_sdk_l3_egress_destroy(int unit, int egress_id) { return bcm_l3_egress_destroy(unit, egress_id); }

static bcm_ip_t ip4_host(const uint8_t* a)
{
    return ((bcm_ip_t)a[0] << 24) | ((bcm_ip_t)a[1] << 16) | ((bcm_ip_t)a[2] << 8) | (bcm_ip_t)a[3];
}

int ob_sdk_l3_host_add(int unit, int vrf, int v6, const uint8_t* addr, int egress_id)
{
    bcm_l3_host_t h;
    bcm_l3_host_t_init(&h);
    h.l3a_vrf = (bcm_vrf_t)vrf;
    if (v6) { h.l3a_flags |= BCM_L3_IP6; memcpy(h.l3a_ip6_addr, addr, 16); }
    else h.l3a_ip_addr = ip4_host(addr);
    h.l3a_intf = egress_id;
    return bcm_l3_host_add(unit, &h);
}

int ob_sdk_l3_host_delete(int unit, int vrf, int v6, const uint8_t* addr)
{
    bcm_l3_host_t h;
    bcm_l3_host_t_init(&h);
    h.l3a_vrf = (bcm_vrf_t)vrf;
    if (v6) { h.l3a_flags |= BCM_L3_IP6; memcpy(h.l3a_ip6_addr, addr, 16); }
    else h.l3a_ip_addr = ip4_host(addr);
    return bcm_l3_host_delete(unit, &h);
}

static void route_fill(bcm_l3_route_t* r, int vrf, int v6, const uint8_t* addr, const uint8_t* mask)
{
    bcm_l3_route_t_init(r);
    r->l3a_vrf = (bcm_vrf_t)vrf;
    if (v6) {
        r->l3a_flags |= BCM_L3_IP6;
        memcpy(r->l3a_ip6_net, addr, 16);
        memcpy(r->l3a_ip6_mask, mask, 16);
    } else {
        r->l3a_subnet = ip4_host(addr) & ip4_host(mask);
        r->l3a_ip_mask = ip4_host(mask);
    }
}

int ob_sdk_l3_route_add(int unit, int vrf, int v6, const uint8_t* addr, const uint8_t* mask, int egress_id,
                        int ecmp, int drop, int replace)
{
    bcm_l3_route_t r;
    route_fill(&r, vrf, v6, addr, mask);
    r.l3a_intf = egress_id;
    if (ecmp) r.l3a_flags |= BCM_L3_MULTIPATH;
    if (drop) r.l3a_flags |= BCM_L3_DST_DISCARD;
    if (replace) r.l3a_flags |= BCM_L3_REPLACE;
    return bcm_l3_route_add(unit, &r);
}

int ob_sdk_l3_route_delete(int unit, int vrf, int v6, const uint8_t* addr, const uint8_t* mask)
{
    bcm_l3_route_t r;
    route_fill(&r, vrf, v6, addr, mask);
    return bcm_l3_route_delete(unit, &r);
}

/* ECMP groups: multipath egress objects (max_paths 0 = the SDK's default
 * group size, bcm_l3_route_max_ecmp_set). */
int ob_sdk_l3_ecmp_set(int unit, int* ecmp_id, int replace, const int* egress, int n)
{
    bcm_l3_egress_ecmp_t e;
    bcm_if_t members[256];
    int i, rv;
    if (n <= 0 || n > (int)(sizeof(members) / sizeof(members[0]))) return BCM_E_PARAM;
    for (i = 0; i < n; i++) members[i] = (bcm_if_t)egress[i];
    bcm_l3_egress_ecmp_t_init(&e);
    if (replace) {
        e.flags = BCM_L3_REPLACE | BCM_L3_WITH_ID;
        e.ecmp_intf = (bcm_if_t)*ecmp_id;
    }
    rv = bcm_l3_egress_ecmp_create(unit, &e, n, members);
    if (rv >= 0) *ecmp_id = e.ecmp_intf;
    return rv;
}

int ob_sdk_l3_ecmp_destroy(int unit, int ecmp_id)
{
    bcm_l3_egress_ecmp_t e;
    bcm_l3_egress_ecmp_t_init(&e);
    e.ecmp_intf = (bcm_if_t)ecmp_id;
    return bcm_l3_egress_ecmp_destroy(unit, &e);
}

/* ------------------------------------------------------------------ FP */
static const int fp_qual_map[OB_FPQ_COUNT] = {
    bcmFieldQualifyInPorts, bcmFieldQualifyEtherType, bcmFieldQualifyIpType, bcmFieldQualifyIpProtocol,
    bcmFieldQualifySrcIp, bcmFieldQualifyDstIp, bcmFieldQualifySrcIp6, bcmFieldQualifyDstIp6,
    bcmFieldQualifyL4SrcPort, bcmFieldQualifyL4DstPort, bcmFieldQualifyTcpControl, bcmFieldQualifyIcmpTypeCode,
    bcmFieldQualifyTos, bcmFieldQualifyDstMac, bcmFieldQualifySrcMac, bcmFieldQualifyOuterVlanId,
    bcmFieldQualifyTtl, bcmFieldQualifyDstL3Egress};

/* Auto mode (bcm_field_group_create): the SDK widens the group (double /
 * triple slices) as the qualifier set needs. */
int ob_sdk_fp_group_create(int unit, uint32_t q, int priority, int* gid)
{
    bcm_field_qset_t qset;
    bcm_field_group_t g = 0;
    int i, rv;
    BCM_FIELD_QSET_INIT(qset);
    BCM_FIELD_QSET_ADD(qset, bcmFieldQualifyStageIngress);
    for (i = 0; i < OB_FPQ_COUNT; i++)
        if (q & (1u << i)) BCM_FIELD_QSET_ADD(qset, fp_qual_map[i]);
    rv = bcm_field_group_create(unit, qset, priority, &g);
    if (rv < 0) return rv;
    *gid = (int)g;
    return BCM_E_NONE;
}

int ob_sdk_fp_group_destroy(int unit, int gid) { return bcm_field_group_destroy(unit, (bcm_field_group_t)gid); }

static bcm_ip_t ip4_be(const uint8_t* a)
{
    return ((bcm_ip_t)a[0] << 24) | ((bcm_ip_t)a[1] << 16) | ((bcm_ip_t)a[2] << 8) | (bcm_ip_t)a[3];
}

static int fp_qualify(int unit, bcm_field_entry_t e, const ob_fp_rule_t* r)
{
    int rv = BCM_E_NONE, i;
#define HAS(q) (r->present & (1u << (q)))
#define Q(call) do { if ((rv = (call)) < 0) return rv; } while (0)
    if (HAS(OB_FPQ_INPORTS)) {
        bcm_pbmp_t d, m;
        BCM_PBMP_CLEAR(d);
        for (i = 0; i < r->nports; i++) BCM_PBMP_PORT_ADD(d, r->ports[i]);
        BCM_PBMP_ASSIGN(m, PBMP_PORT_ALL(unit));   /* data = ports to match, mask = all (any of data) */
        Q(bcm_field_qualify_InPorts(unit, e, d, m));
    }
    if (HAS(OB_FPQ_ETHERTYPE)) Q(bcm_field_qualify_EtherType(unit, e, r->ether, r->ether_mask));
    if (HAS(OB_FPQ_IPTYPE)) {
        static const bcm_field_IpType_t t[] = {bcmFieldIpTypeIp, bcmFieldIpTypeNonIp, bcmFieldIpTypeIpv4Any,
                                               bcmFieldIpTypeIpv4Not, bcmFieldIpTypeIpv6, bcmFieldIpTypeIpv6Not,
                                               bcmFieldIpTypeArp, bcmFieldIpTypeArpRequest, bcmFieldIpTypeArpReply};
        if (r->ip_type < 0 || r->ip_type >= (int)(sizeof(t) / sizeof(t[0]))) return BCM_E_PARAM;
        Q(bcm_field_qualify_IpType(unit, e, t[r->ip_type]));
    }
    if (HAS(OB_FPQ_IPPROTO)) Q(bcm_field_qualify_IpProtocol(unit, e, r->proto, r->proto_mask));
    if (HAS(OB_FPQ_SRCIP)) Q(bcm_field_qualify_SrcIp(unit, e, ip4_be(r->src), ip4_be(r->src_mask)));
    if (HAS(OB_FPQ_DSTIP)) Q(bcm_field_qualify_DstIp(unit, e, ip4_be(r->dst), ip4_be(r->dst_mask)));
    if (HAS(OB_FPQ_SRCIP6)) {
        bcm_ip6_t a, m;
        memcpy(a, r->src, 16); memcpy(m, r->src_mask, 16);
        Q(bcm_field_qualify_SrcIp6(unit, e, a, m));
    }
    if (HAS(OB_FPQ_DSTIP6)) {
        bcm_ip6_t a, m;
        memcpy(a, r->dst, 16); memcpy(m, r->dst_mask, 16);
        Q(bcm_field_qualify_DstIp6(unit, e, a, m));
    }
    if (HAS(OB_FPQ_L4SRC)) Q(bcm_field_qualify_L4SrcPort(unit, e, r->l4_src, r->l4_src_mask));
    if (HAS(OB_FPQ_L4DST)) Q(bcm_field_qualify_L4DstPort(unit, e, r->l4_dst, r->l4_dst_mask));
    if (HAS(OB_FPQ_TCP)) Q(bcm_field_qualify_TcpControl(unit, e, r->tcp, r->tcp_mask));
    if (HAS(OB_FPQ_ICMP)) Q(bcm_field_qualify_IcmpTypeCode(unit, e, r->icmp, r->icmp_mask));
    if (HAS(OB_FPQ_TOS)) Q(bcm_field_qualify_Tos(unit, e, r->tos, r->tos_mask));
    if (HAS(OB_FPQ_DSTMAC)) {
        bcm_mac_t a, m;
        memcpy(a, r->dmac, 6); memcpy(m, r->dmac_mask, 6);
        Q(bcm_field_qualify_DstMac(unit, e, a, m));
    }
    if (HAS(OB_FPQ_SRCMAC)) {
        bcm_mac_t a, m;
        memcpy(a, r->smac, 6); memcpy(m, r->smac_mask, 6);
        Q(bcm_field_qualify_SrcMac(unit, e, a, m));
    }
    if (HAS(OB_FPQ_VLAN)) Q(bcm_field_qualify_OuterVlanId(unit, e, r->vlan, r->vlan_mask));
    if (HAS(OB_FPQ_TTL)) Q(bcm_field_qualify_Ttl(unit, e, r->ttl, r->ttl_mask));
    if (HAS(OB_FPQ_DSTEGRESS)) Q(bcm_field_qualify_DstL3Egress(unit, e, (bcm_if_t)r->dst_egress));
#undef HAS
    return rv;
}

static int fp_actions(int unit, bcm_field_entry_t e, const ob_fp_rule_t* r)
{
    int rv = BCM_E_NONE;
    if (r->drop) Q(bcm_field_action_add(unit, e, bcmFieldActionDrop, 0, 0));
    if (r->drop_cancel) Q(bcm_field_action_add(unit, e, bcmFieldActionDropCancel, 0, 0));
    if (r->copy_cpu) Q(bcm_field_action_add(unit, e, bcmFieldActionCopyToCpu, 0, 0));
    if (r->policer >= 0) {
        /* Colours: drop, or (CoPP) only cancel the copy to the CPU. */
        if (r->green_drop)
            Q(bcm_field_action_add(unit, e, r->cpu_copy_only ? bcmFieldActionGpCopyToCpuCancel : bcmFieldActionGpDrop, 0, 0));
        if (r->yellow_drop)
            Q(bcm_field_action_add(unit, e, r->cpu_copy_only ? bcmFieldActionYpCopyToCpuCancel : bcmFieldActionYpDrop, 0, 0));
        if (r->red_drop)
            Q(bcm_field_action_add(unit, e, r->cpu_copy_only ? bcmFieldActionRpCopyToCpuCancel : bcmFieldActionRpDrop, 0, 0));
    }
    return rv;
}

int ob_sdk_fp_entry_create(int unit, int gid, const ob_fp_rule_t* r, int* eid, int* stat_id)
{
    bcm_field_entry_t e = 0;
    int rv, stat = -1;
    *stat_id = -1;
    rv = bcm_field_entry_create(unit, (bcm_field_group_t)gid, &e);
    if (rv < 0) return rv;
    if ((rv = bcm_field_entry_prio_set(unit, e, r->priority)) < 0) goto fail;
    if ((rv = fp_qualify(unit, e, r)) < 0) goto fail;
    if ((rv = fp_actions(unit, e, r)) < 0) goto fail;
    if (r->counter) {
        bcm_field_stat_t s[2] = {bcmFieldStatPackets, bcmFieldStatBytes};
        if ((rv = bcm_field_stat_create(unit, (bcm_field_group_t)gid, 2, s, &stat)) < 0) goto fail;
        if ((rv = bcm_field_entry_stat_attach(unit, e, stat)) < 0) goto fail;
    }
    if (r->policer >= 0 && (rv = bcm_field_entry_policer_attach(unit, e, 0, (bcm_policer_t)r->policer)) < 0) goto fail;
    if ((rv = bcm_field_entry_install(unit, e)) < 0) goto fail;
    *eid = (int)e;
    *stat_id = stat;
    return BCM_E_NONE;
fail:
    if (r->policer >= 0) bcm_field_entry_policer_detach_all(unit, e);
    if (stat >= 0) {
        bcm_field_entry_stat_detach(unit, e, stat);
        bcm_field_stat_destroy(unit, stat);
    }
    bcm_field_entry_destroy(unit, e);
    return rv;
}

int ob_sdk_fp_entry_destroy(int unit, int eid, int stat_id)
{
    bcm_field_entry_t e = (bcm_field_entry_t)eid;
    int rv;
    bcm_field_entry_remove(unit, e);   /* out of the TCAM first */
    bcm_field_entry_policer_detach_all(unit, e);
    if (stat_id >= 0) bcm_field_entry_stat_detach(unit, e, stat_id);
    rv = bcm_field_entry_destroy(unit, e);
    if (stat_id >= 0) bcm_field_stat_destroy(unit, stat_id);
    return rv;
}

int ob_sdk_fp_stat_get(int unit, int stat_id, uint64_t* packets, uint64_t* bytes)
{
    uint64 p = 0, b = 0;
    int rv;
    if ((rv = bcm_field_stat_sync_get(unit, stat_id, bcmFieldStatPackets, &p)) < 0) return rv;
    if ((rv = bcm_field_stat_sync_get(unit, stat_id, bcmFieldStatBytes, &b)) < 0) return rv;
    *packets = p;
    *bytes = b;
    return BCM_E_NONE;
}
#undef Q

/* ------------------------------------------------------------ meters */
static uint32 kbits(uint64_t bytes)   /* bytes(/s) -> kbits(/s), rounded up */
{
    uint64_t k = (bytes * 8 + 999) / 1000;
    return k > 0xffffffffu ? 0xffffffffu : (uint32)k;
}
static uint32 cap32(uint64_t v) { return v > 0xffffffffu ? 0xffffffffu : (uint32)v; }

/* Packet mode (BCM_POLICER_MODE_PACKETS): the kbits fields carry packets/s
 * and packets. srTCM refills its excess bucket at the committed rate. */
static void policer_fill(bcm_policer_config_t* pc, const ob_policer_cfg_t* c)
{
    uint32 (*conv)(uint64_t) = c->packets ? cap32 : kbits;
    bcm_policer_config_t_init(pc);
    pc->flags = (c->packets ? BCM_POLICER_MODE_PACKETS : BCM_POLICER_MODE_BYTES) |
                (c->color_blind ? BCM_POLICER_COLOR_BLIND : 0);
    pc->ckbits_sec = conv(c->cir);
    pc->ckbits_burst = conv(c->cbs);
    if (c->tr_tcm) {
        pc->mode = bcmPolicerModeTrTcm;
        pc->pkbits_sec = conv(c->pir);
        pc->pkbits_burst = conv(c->pbs);
    } else if (c->pbs) {
        pc->mode = bcmPolicerModeSrTcm;
        pc->pkbits_sec = pc->ckbits_sec;
        pc->pkbits_burst = conv(c->pbs);
    } else {
        pc->mode = bcmPolicerModeCommitted;
    }
}

int ob_sdk_policer_create(int unit, const ob_policer_cfg_t* c, int* pid)
{
    bcm_policer_config_t pc;
    bcm_policer_t id = 0;
    int rv;
    policer_fill(&pc, c);
    rv = bcm_policer_create(unit, &pc, &id);
    if (rv < 0) return rv;
    *pid = (int)id;
    return BCM_E_NONE;
}

int ob_sdk_policer_set(int unit, int pid, const ob_policer_cfg_t* c)
{
    bcm_policer_config_t pc;
    policer_fill(&pc, c);
    return bcm_policer_set(unit, (bcm_policer_t)pid, &pc);
}

int ob_sdk_policer_destroy(int unit, int pid) { return bcm_policer_destroy(unit, (bcm_policer_t)pid); }
