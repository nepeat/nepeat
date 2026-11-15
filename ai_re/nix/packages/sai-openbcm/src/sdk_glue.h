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

typedef void (*ob_link_cb_t)(void* ctx, int port, int up);
typedef void (*ob_l2_cb_t)(void* ctx, const uint8_t mac[6], int vid, int port, int learned);

/* Bring-up, see sdk_glue.c for the exact sequence. err receives a message. */
int ob_sdk_init(int unit, const char* config_file, const char* rc_file, char* err, int errlen);
void ob_sdk_shutdown(int unit);
int ob_sdk_shell(int unit, const char* cmd);
const char* ob_sdk_errmsg(int rv);
int ob_sdk_register_callbacks(int unit, ob_link_cb_t link, ob_l2_cb_t l2, void* ctx);
void ob_sdk_unregister_callbacks(int unit);
int ob_sdk_hw_info(int unit, char* buf, int len);

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

int ob_sdk_l3_intf_create(int unit, const uint8_t mac[6], int vid, int mtu, int* intf_id);
int ob_sdk_l3_intf_update(int unit, int intf_id, const uint8_t mac[6], int vid, int mtu);
int ob_sdk_l3_intf_destroy(int unit, int intf_id);
int ob_sdk_l3_station_add(int unit, const uint8_t mac[6], int* station_id);
int ob_sdk_l3_station_delete(int unit, int station_id);
int ob_sdk_l3_egress_create(int unit, int intf, const uint8_t mac[6], int port, int vid, int* egress_id);
int ob_sdk_l3_egress_update(int unit, int egress_id, int intf, const uint8_t mac[6], int port, int vid);
int ob_sdk_l3_egress_special(int unit, int drop, int* egress_id);   /* to CPU, or drop */
int ob_sdk_l3_egress_destroy(int unit, int egress_id);
int ob_sdk_l3_host_add(int unit, int v6, const uint8_t* addr, int egress_id);
int ob_sdk_l3_host_delete(int unit, int v6, const uint8_t* addr);
int ob_sdk_l3_route_add(int unit, int v6, const uint8_t* addr, const uint8_t* mask, int egress_id,
                        int drop, int replace);
int ob_sdk_l3_route_delete(int unit, int v6, const uint8_t* addr, const uint8_t* mask);

#ifdef __cplusplus
}
#endif
#endif
