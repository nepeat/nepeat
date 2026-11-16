/* SPDX-License-Identifier: Apache-2.0
 * Mock libifcs: netdevs, hostif traps (mock.h). */
#include "mock.h"

/* --------------------------------------------------------------- netdev */
ifcs_status_t ifcs_netdev_create(ifcs_node_id_t n, ifcs_handle_t* h, uint32_t c, ifcs_attr_t* a)
{
    (void)n;
    char name[32] = "";
    uint32_t devport = 0, tag = 99;
    const uint8_t* mac = NULL;
    for (uint32_t i = 0; i < c; i++) {
        if (a[i].id == IFCS_NETDEV_ATTR_INTF_NAME && a[i].value.string.count < sizeof(name)) {
            memcpy(name, a[i].value.string.arr, a[i].value.string.count);
            name[a[i].value.string.count] = 0;
        }
        if (a[i].id == IFCS_NETDEV_ATTR_DEVPORTNUM) devport = a[i].value.devport;
        if (a[i].id == IFCS_NETDEV_ATTR_MACADDR) mac = a[i].value.mac;
        if (a[i].id == IFCS_NETDEV_ATTR_VLAN_TAG) tag = a[i].value.u32;
    }
    *h = next_handle++;
    mock_logf("ifcs_netdev_create h=0x%x name=%s devport=%u mac=%02x:..:%02x tag=%u", *h, name, devport, mac ? mac[0] : 0,
          mac ? mac[5] : 0, tag);
    return IFCS_SUCCESS;
}
ifcs_status_t ifcs_netdev_delete(ifcs_node_id_t n, ifcs_handle_t h) { (void)n; mock_logf("ifcs_netdev_delete h=0x%x", h); return IFCS_SUCCESS; }
ifcs_status_t ifcs_netdev_attr_set(ifcs_node_id_t n, ifcs_handle_t h, uint32_t c, ifcs_attr_t* a)
{
    (void)n;
    mock_logf("ifcs_netdev_attr_set h=0x%x count=%u id=%u u32=%u", h, c, a[0].id, a[0].value.u32);
    return IFCS_SUCCESS;
}

/* ---------------------------------------------------------- hostif traps */
ifcs_status_t ifcs_hostif_trap_create(ifcs_node_id_t n, ifcs_handle_t* h, uint32_t c, ifcs_attr_t* a)
{
    (void)n;
    *h = next_handle++;
    mock_logf("ifcs_hostif_trap_create h=0x%x count=%u queue=%u", *h, c, c ? a[0].value.u32 : 99);
    return IFCS_SUCCESS;
}
ifcs_status_t ifcs_hostif_trap_delete(ifcs_node_id_t n, ifcs_handle_t h) { (void)n; mock_logf("ifcs_hostif_trap_delete h=0x%x", h); return IFCS_SUCCESS; }
ifcs_status_t ifcs_hostif_trap_attr_set(ifcs_node_id_t n, ifcs_handle_t h, uint32_t c, ifcs_attr_t* a)
{
    (void)n;
    for (uint32_t i = 0; i < c; i++) mock_logf("ifcs_hostif_trap_attr_set h=0x%x id=%u u32=%u", h, a[i].id, a[i].value.u32);
    return IFCS_SUCCESS;
}
ifcs_status_t ifcs_hostif_trap_stats_get(ifcs_node_id_t n, ifcs_handle_t h, uint32_t c, ifcs_hostif_trap_stats_id_t* ids,
                                         uint64_t* v, uint32_t* got)
{
    (void)n; (void)h; (void)ids;
    for (uint32_t i = 0; i < c; i++) v[i] = 7;
    if (got) *got = c;
    return IFCS_SUCCESS;
}
