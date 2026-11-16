/* SPDX-License-Identifier: Apache-2.0
 * Mock libifcs: L3VNIs, interfaces, next hops, routes, ECMP (mock.h). */
#include "mock.h"

static struct { int used; char key[96]; ifcs_handle_t nh; int fwd, ctc; ifcs_handle_t trap; } rt[MAX_OBJ];
static int fail_next_route; /* test hook */

/* ---------------------------------------------------------- test hooks */
void mock_fail_next_route(void) { fail_next_route = 1; }
int mock_route_count(void) { int n = 0; for (int i = 0; i < MAX_OBJ; i++) n += rt[i].used; return n; }
/* Route by key text "<l3vni hex>/<family>/<addr hex>/<mask hex>": fwd, ctc, nexthop. */
int mock_route_find(const char* key, int* fwd, int* ctc, uint32_t* nh)
{
    for (int i = 0; i < MAX_OBJ; i++)
        if (rt[i].used && strcmp(rt[i].key, key) == 0) {
            *fwd = rt[i].fwd;
            *ctc = rt[i].ctc;
            *nh = rt[i].nh;
            return 1;
        }
    return 0;
}

static void route_key_text(const ifcs_route_entry_key_t* k, char* b, size_t n)
{
    const ifcs_route_entry_key_ip_dest_l3vni_t* r = &k->key.ip_dest_l3vni;
    if (r->ip_dest.addr_family == IFCS_IP_ADDR_FAMILY_IPV4)
        snprintf(b, n, "%x/4/%08x/%08x", r->l3vni, r->ip_dest.addr.ipv4, r->ip_dest.mask.ipv4);
    else {
        char a[33], m[33];
        for (int i = 0; i < 16; i++) { sprintf(a + 2 * i, "%02x", r->ip_dest.addr.ipv6[i]); sprintf(m + 2 * i, "%02x", r->ip_dest.mask.ipv6[i]); }
        snprintf(b, n, "%x/6/%s/%s", r->l3vni, a, m);
    }
}

/* ------------------------------------------------------------------- L3 */
ifcs_status_t ifcs_l3vni_create(ifcs_node_id_t n, ifcs_handle_t* h, uint32_t c, ifcs_attr_t* a)
{
    (void)n; (void)a;
    mock_logf("ifcs_l3vni_create h=0x%x count=%u", *h, c);
    return IFCS_SUCCESS;
}
ifcs_status_t ifcs_intf_create(ifcs_node_id_t n, ifcs_handle_t* h, uint32_t c, ifcs_attr_t* a)
{
    (void)n;
    *h = next_handle++;
    mock_logf("ifcs_intf_create h=0x%x count=%u type=%u fwd=0x%x transport=0x%x", *h, c, a[0].value.u32, a[1].value.handle,
          a[2].value.handle);
    return IFCS_SUCCESS;
}
ifcs_status_t ifcs_intf_delete(ifcs_node_id_t n, ifcs_handle_t h) { (void)n; mock_logf("ifcs_intf_delete h=0x%x", h); return IFCS_SUCCESS; }
ifcs_status_t ifcs_intf_attr_set(ifcs_node_id_t n, ifcs_handle_t h, uint32_t c, ifcs_attr_t* a)
{
    (void)n;
    mock_logf("ifcs_intf_attr_set h=0x%x count=%u id=%u", h, c, a[0].id);
    return IFCS_SUCCESS;
}
ifcs_status_t ifcs_nexthop_create(ifcs_node_id_t n, ifcs_handle_t* h, uint32_t c, ifcs_attr_t* a)
{
    (void)n;
    *h = next_handle++;
    mock_logf("ifcs_nexthop_create h=0x%x count=%u intf=0x%x dest=0x%x", *h, c, a[0].value.handle, a[2].value.handle);
    return IFCS_SUCCESS;
}
ifcs_status_t ifcs_nexthop_delete(ifcs_node_id_t n, ifcs_handle_t h) { (void)n; mock_logf("ifcs_nexthop_delete h=0x%x", h); return IFCS_SUCCESS; }
ifcs_status_t ifcs_nexthop_attr_set(ifcs_node_id_t n, ifcs_handle_t h, uint32_t c, ifcs_attr_t* a)
{
    (void)n;
    mock_logf("ifcs_nexthop_attr_set h=0x%x count=%u id=%u", h, c, a[0].id);
    return IFCS_SUCCESS;
}
ifcs_status_t ifcs_nexthop_usage_get(ifcs_node_id_t n, uint32_t c, ifcs_attr_t* a, ifcs_usage_t* u)
{
    (void)n; (void)c; (void)a;
    u->max = 32768;
    u->current = 3;
    return IFCS_SUCCESS;
}
ifcs_status_t ifcs_route_entry_key_t_init(ifcs_route_entry_key_t* k) { memset(k, 0, sizeof(*k)); k->_base_key = 0x5e7; return IFCS_SUCCESS; }
ifcs_status_t ifcs_route_entry_key_t_key_type_set(ifcs_route_entry_key_t* k, int t) { k->key_type = t; return IFCS_SUCCESS; }
ifcs_status_t ifcs_route_entry_key_t_ip_dest_l3vni_set(ifcs_route_entry_key_t* k, ifcs_route_entry_key_ip_dest_l3vni_t v) { k->key.ip_dest_l3vni = v; k->dirty |= 4; return IFCS_SUCCESS; }
ifcs_status_t ifcs_route_entry_key_ip_dest_l3vni_t_l3vni_set(ifcs_route_entry_key_ip_dest_l3vni_t* k, ifcs_handle_t h) { k->l3vni = h; return IFCS_SUCCESS; }
ifcs_status_t ifcs_route_entry_key_ip_dest_l3vni_t_ip_dest_set(ifcs_route_entry_key_ip_dest_l3vni_t* k, ifcs_ip_prefix_t* p) { k->ip_dest = *p; return IFCS_SUCCESS; }
ifcs_status_t ifcs_ip_prefix_t_init(ifcs_ip_prefix_t* p) { memset(p, 0, sizeof(*p)); return IFCS_SUCCESS; }
ifcs_status_t ifcs_ip_prefix_t_addr_family_set(ifcs_ip_prefix_t* p, int f) { p->addr_family = f; return IFCS_SUCCESS; }
ifcs_status_t ifcs_ip_prefix_t_addr_set(ifcs_ip_prefix_t* p, ifcs_ip_addr_t* a) { p->addr = *a; return IFCS_SUCCESS; }
ifcs_status_t ifcs_ip_prefix_t_mask_set(ifcs_ip_prefix_t* p, ifcs_ip_addr_t* m) { p->mask = *m; return IFCS_SUCCESS; }
static int rt_find(const char* key)
{
    for (int i = 0; i < MAX_OBJ; i++)
        if (rt[i].used && strcmp(rt[i].key, key) == 0) return i;
    return -1;
}
static void rt_apply(int i, uint32_t c, const ifcs_attr_t* a)
{
    for (uint32_t j = 0; j < c; j++) {
        if (a[j].id == IFCS_ROUTE_ENTRY_ATTR_NEXTHOP) rt[i].nh = a[j].value.handle;
        if (a[j].id == IFCS_ROUTE_ENTRY_ATTR_FWD_POLICY) rt[i].fwd = a[j].value.fwd_policy.fwd_action;
        if (a[j].id == IFCS_ROUTE_ENTRY_ATTR_CTC_POLICY) { rt[i].ctc = a[j].value.ctc_policy.ctc_action; rt[i].trap = a[j].value.ctc_policy.trap_handle; }
    }
}
ifcs_status_t ifcs_route_entry_create(ifcs_node_id_t n, ifcs_route_entry_key_t* k, uint32_t c, ifcs_attr_t* a)
{
    (void)n;
    char key[96];
    route_key_text(k, key, sizeof(key));
    mock_logf("ifcs_route_entry_create key=%s count=%u", key, c);
    if (fail_next_route) { fail_next_route = 0; return IFCS_RESOURCE_FULL; }
    if (k->_base_key != 0x5e7) return IFCS_PARAM;
    if (rt_find(key) >= 0) return IFCS_EXIST;
    for (int i = 0; i < MAX_OBJ; i++)
        if (!rt[i].used) {
            rt[i].used = 1;
            snprintf(rt[i].key, sizeof(rt[i].key), "%s", key);
            rt[i].nh = 0;
            rt[i].fwd = 0;
            rt[i].ctc = 0;
            rt_apply(i, c, a);
            return IFCS_SUCCESS;
        }
    return IFCS_RESOURCE_FULL;
}
ifcs_status_t ifcs_route_entry_delete(ifcs_node_id_t n, ifcs_route_entry_key_t* k)
{
    (void)n;
    char key[96];
    route_key_text(k, key, sizeof(key));
    int i = rt_find(key);
    mock_logf("ifcs_route_entry_delete key=%s found=%d", key, i >= 0);
    if (i < 0) return IFCS_NOTFOUND;
    rt[i].used = 0;
    return IFCS_SUCCESS;
}
ifcs_status_t ifcs_route_entry_attr_set(ifcs_node_id_t n, ifcs_route_entry_key_t* k, uint32_t c, ifcs_attr_t* a)
{
    (void)n;
    char key[96];
    route_key_text(k, key, sizeof(key));
    int i = rt_find(key);
    mock_logf("ifcs_route_entry_attr_set key=%s count=%u found=%d", key, c, i >= 0);
    if (i < 0) return IFCS_NOTFOUND;
    rt_apply(i, c, a);
    return IFCS_SUCCESS;
}
ifcs_status_t ifcs_ecmp_create(ifcs_node_id_t n, ifcs_handle_t* h, uint32_t c, ifcs_attr_t* a) { (void)n; (void)a; *h = IFCS_NULL_HANDLE; return mock_create_obj("ifcs_ecmp_create", h, c); }
ifcs_status_t ifcs_ecmp_delete(ifcs_node_id_t n, ifcs_handle_t h) { (void)n; mock_logf("ifcs_ecmp_delete h=0x%x", h); return IFCS_SUCCESS; }
ifcs_status_t ifcs_ecmp_member_add(ifcs_node_id_t n, ifcs_handle_t e, uint32_t m, ifcs_handle_t* l, uint32_t c, ifcs_attr_t* a)
{
    (void)n;
    mock_logf("ifcs_ecmp_member_add ecmp=0x%x n=%u first=0x%x weight=%u", e, m, m ? l[0] : 0, c ? a[0].value.u32 : 1);
    return IFCS_SUCCESS;
}
ifcs_status_t ifcs_ecmp_member_remove(ifcs_node_id_t n, ifcs_handle_t e, uint32_t m, ifcs_handle_t* l)
{
    (void)n;
    mock_logf("ifcs_ecmp_member_remove ecmp=0x%x n=%u first=0x%x", e, m, m ? l[0] : 0);
    return IFCS_SUCCESS;
}
ifcs_status_t ifcs_ecmp_usage_get(ifcs_node_id_t n, uint32_t c, ifcs_attr_t* a, ifcs_usage_t* u)
{
    (void)n; (void)c; (void)a;
    u->max = 512;
    u->current = 0;
    return IFCS_SUCCESS;
}

