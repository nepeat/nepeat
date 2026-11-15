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
#include <bcm_int/control.h>
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

int ob_sdk_init(int unit, const char* config_file, const char* rc_file, char* err, int errlen)
{
    int rv, nn = 0, nf = 0;
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
    g_cpu_egress = g_drop_egress = -1;
    sdk_inited = 1;
    if (err && errlen > 0) snprintf(err, errlen, "ok: stale KNET netifs %d filters %d destroyed", nn, nf);
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
    if (!g_l2_cb || !l2 || (l2->flags & BCM_L2_STATIC) || (l2->flags & BCM_L2_TRUNK_MEMBER)) return;
    if (op == BCM_L2_CALLBACK_ADD || op == BCM_L2_CALLBACK_LEARN_EVENT || op == BCM_L2_CALLBACK_MOVE_EVENT)
        learned = 1;
    else if (op == BCM_L2_CALLBACK_DELETE || op == BCM_L2_CALLBACK_AGE_EVENT)
        learned = 0;
    else
        return;
    g_l2_cb(g_cb_ctx, l2->mac, l2->vid, l2->port, learned);
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

int ob_sdk_hw_info(int unit, char* buf, int len)
{
    snprintf(buf, len, "%s", soc_dev_name(unit));
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
    l2.port = port;
    l2.modid = my_modid(unit);
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

int ob_sdk_trap_set(int unit, int kind, int to_cpu)
{
    /* Order of ob::TrapKind. -1: no switch control on this SDK path. */
    static const int ctl[] = {
        bcmSwitchArpRequestToCpu, bcmSwitchArpReplyToCpu, bcmSwitchDhcpPktToCpu, -1, bcmSwitchIgmpPktToCpu,
        bcmSwitchMldPktToCpu, bcmSwitchNdPktToCpu, -1, -1, -1, -1};
    if (kind < 0 || kind >= (int)(sizeof(ctl) / sizeof(ctl[0])) || ctl[kind] < 0) return BCM_E_UNAVAIL;
    return bcm_switch_control_set(unit, (bcm_switch_control_t)ctl[kind], to_cpu ? 1 : 0);
}

int ob_sdk_l3_intf_create(int unit, const uint8_t mac[6], int vid, int mtu, int* intf_id)
{
    bcm_l3_intf_t i;
    int rv;
    bcm_l3_intf_t_init(&i);
    memcpy(i.l3a_mac_addr, mac, 6);
    i.l3a_vid = (bcm_vlan_t)vid;
    i.l3a_mtu = mtu;
    rv = bcm_l3_intf_create(unit, &i);
    if (rv >= 0) *intf_id = i.l3a_intf_id;
    return rv;
}

int ob_sdk_l3_intf_update(int unit, int intf_id, const uint8_t mac[6], int vid, int mtu)
{
    bcm_l3_intf_t i;
    bcm_l3_intf_t_init(&i);
    i.l3a_flags = BCM_L3_WITH_ID | BCM_L3_REPLACE;
    i.l3a_intf_id = intf_id;
    memcpy(i.l3a_mac_addr, mac, 6);
    i.l3a_vid = (bcm_vlan_t)vid;
    i.l3a_mtu = mtu;
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
    s.flags = BCM_L2_STATION_IPV4 | BCM_L2_STATION_IPV6;
    return bcm_l2_station_add(unit, station_id, &s);
}

int ob_sdk_l3_station_delete(int unit, int station_id) { return bcm_l2_station_delete(unit, station_id); }

static void egress_fill(int unit, bcm_l3_egress_t* e, int intf, const uint8_t mac[6], int port, int vid)
{
    bcm_l3_egress_t_init(e);
    e->intf = intf;
    memcpy(e->mac_addr, mac, 6);
    e->port = port;
    e->module = my_modid(unit);
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

int ob_sdk_l3_host_add(int unit, int v6, const uint8_t* addr, int egress_id)
{
    bcm_l3_host_t h;
    bcm_l3_host_t_init(&h);
    if (v6) { h.l3a_flags |= BCM_L3_IP6; memcpy(h.l3a_ip6_addr, addr, 16); }
    else h.l3a_ip_addr = ip4_host(addr);
    h.l3a_intf = egress_id;
    return bcm_l3_host_add(unit, &h);
}

int ob_sdk_l3_host_delete(int unit, int v6, const uint8_t* addr)
{
    bcm_l3_host_t h;
    bcm_l3_host_t_init(&h);
    if (v6) { h.l3a_flags |= BCM_L3_IP6; memcpy(h.l3a_ip6_addr, addr, 16); }
    else h.l3a_ip_addr = ip4_host(addr);
    return bcm_l3_host_delete(unit, &h);
}

static void route_fill(bcm_l3_route_t* r, int v6, const uint8_t* addr, const uint8_t* mask)
{
    bcm_l3_route_t_init(r);
    if (v6) {
        r->l3a_flags |= BCM_L3_IP6;
        memcpy(r->l3a_ip6_net, addr, 16);
        memcpy(r->l3a_ip6_mask, mask, 16);
    } else {
        r->l3a_subnet = ip4_host(addr) & ip4_host(mask);
        r->l3a_ip_mask = ip4_host(mask);
    }
}

int ob_sdk_l3_route_add(int unit, int v6, const uint8_t* addr, const uint8_t* mask, int egress_id, int drop,
                        int replace)
{
    bcm_l3_route_t r;
    route_fill(&r, v6, addr, mask);
    r.l3a_intf = egress_id;
    if (drop) r.l3a_flags |= BCM_L3_DST_DISCARD;
    if (replace) r.l3a_flags |= BCM_L3_REPLACE;
    return bcm_l3_route_add(unit, &r);
}

int ob_sdk_l3_route_delete(int unit, int v6, const uint8_t* addr, const uint8_t* mask)
{
    bcm_l3_route_t r;
    route_fill(&r, v6, addr, mask);
    return bcm_l3_route_delete(unit, &r);
}
