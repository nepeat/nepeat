/* SPDX-License-Identifier: Apache-2.0
 * C interface between the openbcm backend (C++) and the Broadcom SDK. Only
 * sdk_glue.c includes SDK headers; it is compiled with the SDK's own
 * defines (share/openbcm/cflags of the libsdk package). All functions
 * return BCM_E_* codes (0 = success, negative = error). */
#ifndef SAI_OPENBCM_SDK_GLUE_H
#define SAI_OPENBCM_SDK_GLUE_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

#define OB_E_NONE 0
#define OB_E_INTERNAL (-1)
#define OB_E_MEMORY (-2)
#define OB_E_UNIT (-3)
#define OB_E_PARAM (-4)
#define OB_E_EMPTY (-5)
#define OB_E_FULL (-6)
#define OB_E_NOT_FOUND (-7)
#define OB_E_EXISTS (-8)
#define OB_E_TIMEOUT (-9)
#define OB_E_BUSY (-10)
#define OB_E_FAIL (-11)
#define OB_E_DISABLED (-12)
#define OB_E_BADID (-13)
#define OB_E_RESOURCE (-14)
#define OB_E_CONFIG (-15)
#define OB_E_UNAVAIL (-16)
#define OB_E_INIT (-17)
#define OB_E_PORT (-18)

typedef struct {
    int port;          /* logical */
    int phys;          /* first physical port (= SONiC lane number) */
    int lanes;
    int speed;         /* Mb/s */
    char name[16];
} ob_port_info_t;

/* Destinations ("port" arguments of the L2 / L3 egress calls and of the L2
 * callback): an SDK logical port, or OB_LAG_DEST_BIT | trunk id. */
#define OB_LAG_DEST_BIT 0x40000000

typedef void (*ob_link_cb_t)(void* ctx, int port, int up);
typedef void (*ob_l2_cb_t)(void* ctx, const uint8_t mac[6], int vid, int port, int learned);

/* Bring-up, see sdk_glue.c for the exact sequence. err receives a message.
 * Both take the per-unit SDK lock (/run/openbcm-unit<N>.lock) for the life of
 * the process and fail with OB_E_BUSY while another SDK process holds it:
 * two processes on one unit would share the DMA pool and the S-channel. */
int ob_sdk_init(int unit, const char* config_file, const char* rc_file, char* err, int errlen);
/* Attach only: probe and attach the unit, no "init all", no chip reset,
 * ports and forwarding state untouched. Enough for register reads. On a
 * Tomahawk nothing has initialised since power-on it also programs the S-bus
 * ring map and the PVTMON trim (see sdk_glue.c th_sbus_wake). */
int ob_sdk_attach(int unit, char* err, int errlen);
/* Die temperature monitors in tenths of a degree C: current, and the highest
 * since the previous call (reading resets the hardware peak). Works after
 * ob_sdk_attach (Tomahawk family, through the S-channel) or ob_sdk_init
 * (BCM API, any chip with monitors). Returns BCM_E_*; *count monitors. */
int ob_sdk_temperatures(int unit, int* cur, int* peak, int max, int* count);
/* Over-temperature protection for the attach-only reader: power down the
 * core PLL exactly as the SDK's Tomahawk handler (soc_tomahawk_temperature_intr)
 * does; every S-channel access fails afterwards, the chip needs a power
 * cycle. Tomahawk only (not TH2). Refused (BCM_E_BUSY) after ob_sdk_init:
 * "init all" armed the SDK's own interrupt handler. */
int ob_sdk_th_overtemp_shutdown(int unit);
void ob_sdk_shutdown(int unit);
int ob_sdk_shell(int unit, const char* cmd);
/* The same with the shell's stdout output sent to fd (diag socket). */
int ob_sdk_shell_to(int unit, const char* cmd, int fd);
const char* ob_sdk_errmsg(int rv);
int ob_sdk_register_callbacks(int unit, ob_link_cb_t link, ob_l2_cb_t l2, void* ctx);
void ob_sdk_unregister_callbacks(int unit);
int ob_sdk_hw_info(int unit, char* buf, int len);

/* Free entries in the forwarding tables (bcm_switch_object_count_get,
 * bcm_l3_info; hosts in IPv4 units, an IPv6 host takes 2). */
typedef struct {
    uint32_t v4_routes_free, v6_routes_free, hosts_free, egress_free, fdb_free;
    uint32_t ecmp_groups_free, ecmp_members_free;   /* multipath groups, L3_ECMP entries */
} ob_table_usage_t;
int ob_sdk_table_usage(int unit, ob_table_usage_t* u);
/* Chip limits: packet buffer (Tomahawk: MMU cells x cell size, else
 * BCM_E_UNAVAIL), field entry priority range, VRF ids, ECMP groups / paths
 * per group / member entries, and for the log the table sizes. */
typedef struct {
    uint64_t buffer_bytes;
    uint32_t acl_prio_min, acl_prio_max;
    int max_host, max_route, max_nexthop, max_l2, ecmp_groups, ecmp_paths;
    int ecmp_members, vrf_max;
} ob_chip_limits_t;
int ob_sdk_chip_limits(int unit, ob_chip_limits_t* l);

int ob_sdk_cpu_port(int unit);
int ob_sdk_ports(int unit, ob_port_info_t* out, int max);   /* >= 0: count */

int ob_sdk_port_enable_set(int unit, int port, int enable);
int ob_sdk_port_link_get(int unit, int port, int* up);
int ob_sdk_port_speed_set(int unit, int port, int mbps);
int ob_sdk_port_speed_get(int unit, int port, int* mbps);
int ob_sdk_port_speeds(int unit, int port, uint32_t* out, int max);   /* >= 0: count */
int ob_sdk_port_frame_max_set(int unit, int port, int bytes);
int ob_sdk_port_fec_set(int unit, int port, int mode);           /* 0 none, 1 RS, 2 FC */
int ob_sdk_port_autoneg_set(int unit, int port, int on);
int ob_sdk_port_autoneg_get(int unit, int port, int* on);
int ob_sdk_port_loopback_set(int unit, int port, int mode);      /* 0 none, 1 MAC, 2 PHY */
int ob_sdk_port_pvid_set(int unit, int port, int vid);
int ob_sdk_port_learn_set(int unit, int port, int mode);         /* 0 hw, 1 off, 2 drop, 3 cpu */
int ob_sdk_port_stat_get(int unit, int port, int which, uint64_t* v);   /* which: ob::PortStat */
int ob_sdk_port_stat_clear(int unit, int port);

int ob_sdk_vlan_create(int unit, int vid);
int ob_sdk_vlan_destroy(int unit, int vid);
int ob_sdk_vlan_port_add(int unit, int vid, int port, int untagged);
int ob_sdk_vlan_port_remove(int unit, int vid, int port);
int ob_sdk_vlan_members(int unit, int vid, int* ports, int* untagged, int max);   /* >= 0: count */

int ob_sdk_l2_add(int unit, const uint8_t mac[6], int vid, int port, int is_static, int discard);
int ob_sdk_l2_delete(int unit, const uint8_t mac[6], int vid);
int ob_sdk_l2_flush(int unit, int port, int vid, int statics, int dynamics);
int ob_sdk_l2_age_set(int unit, int seconds);

int ob_sdk_knet_cleanup(int unit, int* netifs, int* filters);
int ob_sdk_netif_create(int unit, const char* name, int port, int vid, const uint8_t mac[6], int keep_tag,
                        int* netif_id, int* filter_id);
int ob_sdk_netif_destroy(int unit, int netif_id, int filter_id);
int ob_sdk_netif_filter_set(int unit, int netif_id, int port, int keep_tag, int* filter_id);

int ob_sdk_trap_set(int unit, int kind, int to_cpu);              /* kind: ob::TrapKind */

/* Trunks (SAI LAG). Members: ports[i] with flags[i] (1 egress disable,
 * 2 ingress disable); n == 0 empties the trunk. */
int ob_sdk_trunk_create(int unit, int* tid);
int ob_sdk_trunk_destroy(int unit, int tid);
int ob_sdk_trunk_set(int unit, int tid, const int* ports, const int* flags, int n);

/* vrf: the VRF the interface's VLAN routes in (0 = default). */
int ob_sdk_l3_intf_create(int unit, const uint8_t mac[6], int vid, int mtu, int vrf, int* intf_id);
int ob_sdk_l3_intf_update(int unit, int intf_id, const uint8_t mac[6], int vid, int mtu, int vrf);
int ob_sdk_l3_intf_destroy(int unit, int intf_id);
int ob_sdk_l3_station_add(int unit, const uint8_t mac[6], int* station_id);
int ob_sdk_l3_station_delete(int unit, int station_id);
int ob_sdk_l3_egress_create(int unit, int intf, const uint8_t mac[6], int port, int vid, int* egress_id);
int ob_sdk_l3_egress_update(int unit, int egress_id, int intf, const uint8_t mac[6], int port, int vid);
int ob_sdk_l3_egress_special(int unit, int drop, int* egress_id);   /* to CPU, or drop */
int ob_sdk_l3_egress_destroy(int unit, int egress_id);
int ob_sdk_l3_host_add(int unit, int vrf, int v6, const uint8_t* addr, int egress_id);
int ob_sdk_l3_host_delete(int unit, int vrf, int v6, const uint8_t* addr);
/* egress_id: an egress object, or with ecmp an ECMP group (BCM_L3_MULTIPATH). */
int ob_sdk_l3_route_add(int unit, int vrf, int v6, const uint8_t* addr, const uint8_t* mask, int egress_id,
                        int ecmp, int drop, int replace);
int ob_sdk_l3_route_delete(int unit, int vrf, int v6, const uint8_t* addr, const uint8_t* mask);
/* ECMP group over n egress objects: replace == 0 creates (*ecmp_id out),
 * else replaces the members of *ecmp_id. */
int ob_sdk_l3_ecmp_set(int unit, int* ecmp_id, int replace, const int* egress, int n);
int ob_sdk_l3_ecmp_destroy(int unit, int ecmp_id);

/* Ingress field processor (ACL tables, CoPP) and meters. Qualifier bits in
 * ob::FpQual order; addresses in network order (IPv4: first 4 bytes). */
enum {
    OB_FPQ_INPORTS, OB_FPQ_ETHERTYPE, OB_FPQ_IPTYPE, OB_FPQ_IPPROTO, OB_FPQ_SRCIP, OB_FPQ_DSTIP, OB_FPQ_SRCIP6,
    OB_FPQ_DSTIP6, OB_FPQ_L4SRC, OB_FPQ_L4DST, OB_FPQ_TCP, OB_FPQ_ICMP, OB_FPQ_TOS, OB_FPQ_DSTMAC, OB_FPQ_SRCMAC,
    OB_FPQ_VLAN, OB_FPQ_TTL, OB_FPQ_DSTEGRESS, OB_FPQ_COUNT
};
enum { OB_FP_IP, OB_FP_NON_IP, OB_FP_IPV4, OB_FP_NON_IPV4, OB_FP_IPV6, OB_FP_NON_IPV6, OB_FP_ARP, OB_FP_ARP_REQUEST,
       OB_FP_ARP_REPLY };
typedef struct {
    uint32_t present;
    uint16_t ether, ether_mask;
    int ip_type;                        /* OB_FP_IP.. */
    uint8_t proto, proto_mask;
    uint8_t src[16], src_mask[16], dst[16], dst_mask[16];
    uint16_t l4_src, l4_src_mask, l4_dst, l4_dst_mask;
    uint8_t tcp, tcp_mask;
    uint16_t icmp, icmp_mask;
    uint8_t tos, tos_mask;
    uint8_t dmac[6], dmac_mask[6], smac[6], smac_mask[6];
    uint16_t vlan, vlan_mask;
    uint8_t ttl, ttl_mask;
    int dst_egress;                     /* OB_FPQ_DSTEGRESS: L3 egress object the lookup chose */
    const int* ports;                   /* OB_FPQ_INPORTS */
    int nports;
    int priority;
    int drop, drop_cancel, copy_cpu, counter;
    int policer;                        /* -1: none */
    int green_drop, yellow_drop, red_drop, cpu_copy_only;
} ob_fp_rule_t;
int ob_sdk_fp_group_create(int unit, uint32_t qset, int priority, int* gid);
int ob_sdk_fp_group_destroy(int unit, int gid);
/* Installed on success; *stat_id: the entry's packet/byte counter or -1. */
int ob_sdk_fp_entry_create(int unit, int gid, const ob_fp_rule_t* r, int* eid, int* stat_id);
int ob_sdk_fp_entry_destroy(int unit, int eid, int stat_id);
int ob_sdk_fp_stat_get(int unit, int stat_id, uint64_t* packets, uint64_t* bytes);
typedef struct {
    int tr_tcm, packets, color_blind;   /* sr_tcm with pbs 0: one bucket (committed) */
    uint64_t cir, cbs, pir, pbs;        /* packets/s + packets, or bytes/s + bytes */
} ob_policer_cfg_t;
int ob_sdk_policer_create(int unit, const ob_policer_cfg_t* c, int* pid);
int ob_sdk_policer_set(int unit, int pid, const ob_policer_cfg_t* c);
int ob_sdk_policer_destroy(int unit, int pid);

#ifdef __cplusplus
}
#endif
#endif
