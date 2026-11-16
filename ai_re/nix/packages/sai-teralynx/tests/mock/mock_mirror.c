/* SPDX-License-Identifier: Apache-2.0
 * Mock libifcs: mirror / sampling (tl_mirror.cpp): duplicate-copy
 * destination ports, collectors, collector sets and their members (mock.h). */
#include "mock.h"

static int dcdps, collectors, sets, members;

int mock_mirror_dcdps(void) { return dcdps; }
int mock_mirror_collectors(void) { return collectors; }
int mock_mirror_sets(void) { return sets; }
int mock_mirror_members(void) { return members; }

ifcs_status_t ifcs_dup_copy_dest_port_create(ifcs_node_id_t n, ifcs_handle_t* h, uint32_t c, ifcs_attr_t* a)
{
    (void)n;
    if (c != 1 || a[0].id != IFCS_DUP_COPY_DEST_PORT_ATTR_SYSPORT) return IFCS_PARAM;
    *h = next_handle++;
    mock_logf("ifcs_dup_copy_dest_port_create h=0x%x sysport=0x%x", *h, a[0].value.handle);
    dcdps++;
    return IFCS_SUCCESS;
}
ifcs_status_t ifcs_dup_copy_dest_port_delete(ifcs_node_id_t n, ifcs_handle_t h) { (void)n; mock_logf("ifcs_dup_copy_dest_port_delete h=0x%x", h); dcdps--; return IFCS_SUCCESS; }

ifcs_status_t ifcs_collector_create(ifcs_node_id_t n, ifcs_handle_t* h, uint32_t c, ifcs_attr_t* a)
{
    (void)n;
    *h = next_handle++;
    char b[1024];
    int m = snprintf(b, sizeof(b), "ifcs_collector_create h=0x%x n=%u", *h, c);
    for (uint32_t i = 0; i < c && m < (int)sizeof(b) - 64; i++) {
        switch (a[i].id) {
        case IFCS_COLLECTOR_ATTR_DEST_MAC: case IFCS_COLLECTOR_ATTR_SRC_MAC: {
            const uint8_t* x = a[i].value.mac;
            m += snprintf(b + m, sizeof(b) - (size_t)m, " %u=%02x%02x%02x%02x%02x%02x", a[i].id, x[0], x[1], x[2], x[3], x[4], x[5]);
            break;
        }
        case IFCS_COLLECTOR_ATTR_SRC_IP: case IFCS_COLLECTOR_ATTR_DEST_IP:
            m += snprintf(b + m, sizeof(b) - (size_t)m, " %u=%08x", a[i].id, a[i].value.ip_addr.addr.ipv4);
            break;
        case IFCS_COLLECTOR_ATTR_DSCP: case IFCS_COLLECTOR_ATTR_TTL:
            m += snprintf(b + m, sizeof(b) - (size_t)m, " %u=%u", a[i].id, a[i].value.u8);
            break;
        case IFCS_COLLECTOR_ATTR_SESSION_ID: case IFCS_COLLECTOR_ATTR_DOT1Q_VLAN:
            m += snprintf(b + m, sizeof(b) - (size_t)m, " %u=%u", a[i].id, a[i].value.u16);
            break;
        default:
            m += snprintf(b + m, sizeof(b) - (size_t)m, " %u=0x%x", a[i].id, a[i].value.u32);
        }
    }
    mock_logf("%s", b);
    collectors++;
    return IFCS_SUCCESS;
}
ifcs_status_t ifcs_collector_delete(ifcs_node_id_t n, ifcs_handle_t h) { (void)n; mock_logf("ifcs_collector_delete h=0x%x", h); collectors--; return IFCS_SUCCESS; }
ifcs_status_t ifcs_collector_set_create(ifcs_node_id_t n, ifcs_handle_t* h, uint32_t c, ifcs_attr_t* a)
{
    (void)n;
    if (c != 2) return IFCS_PARAM;
    *h = next_handle++;
    mock_logf("ifcs_collector_set_create h=0x%x type=%u dir=%u", *h, a[0].value.u32, a[1].value.u32);
    sets++;
    return IFCS_SUCCESS;
}
ifcs_status_t ifcs_collector_set_delete(ifcs_node_id_t n, ifcs_handle_t h) { (void)n; mock_logf("ifcs_collector_set_delete h=0x%x", h); sets--; return IFCS_SUCCESS; }
ifcs_status_t ifcs_collector_set_member_add(ifcs_node_id_t n, ifcs_handle_t s, uint32_t c, ifcs_handle_t* l, uint32_t ac, ifcs_attr_t* a)
{
    (void)n; (void)ac; (void)a;
    for (uint32_t i = 0; i < c; i++) mock_logf("ifcs_collector_set_member_add set=0x%x member=0x%x", s, l[i]);
    members += (int)c;
    return IFCS_SUCCESS;
}
ifcs_status_t ifcs_collector_set_member_remove(ifcs_node_id_t n, ifcs_handle_t s, uint32_t c, ifcs_handle_t* l)
{
    (void)n;
    for (uint32_t i = 0; i < c; i++) mock_logf("ifcs_collector_set_member_remove set=0x%x member=0x%x", s, l[i]);
    members -= (int)c;
    return IFCS_SUCCESS;
}
ifcs_status_t ifcs_traffic_monitor_policy_t_init(ifcs_traffic_monitor_policy_t* p) { memset(p, 0, sizeof(*p)); return IFCS_SUCCESS; }
