/* SPDX-License-Identifier: Apache-2.0
 * Mock libifcs: L3VNIs, interfaces, next hops, routes, ECMP (mock.h). */
#include "mock.h"

static struct { int used; char key[96]; ifcs_handle_t nh; int fwd, ctc, ttlp; ifcs_handle_t trap; } rt[MAX_OBJ];
static int fail_next_route; /* test hook */

/* ---------------------------------------------------------- test hooks */
void mock_fail_next_route(void) { fail_next_route = 1; }
/* TTL check profile of a route (key as mock_route_find), -1 if none. */
int mock_route_ttlp(const char* key)
{
    for (int i = 0; i < MAX_OBJ; i++)
        if (rt[i].used && strcmp(rt[i].key, key) == 0) return rt[i].ttlp;
    return -1;
}
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
static struct { int used; ifcs_handle_t h; uint32_t type, fwd, transport, mtu; uint8_t mac[6]; uint64_t stats[34]; } in[MAX_OBJ];
static struct { int used; ifcs_handle_t h, intf, dest; uint8_t mac[6]; } nhs[MAX_OBJ];
static struct { int used; ifcs_handle_t h; int n; ifcs_handle_t m[64]; uint32_t w[64]; } ec[256];

static int in_find(ifcs_handle_t h) { for (int i = 0; i < MAX_OBJ; i++) if (in[i].used && in[i].h == h) return i; return -1; }
static int nh_find(ifcs_handle_t h) { for (int i = 0; i < MAX_OBJ; i++) if (nhs[i].used && nhs[i].h == h) return i; return -1; }
static int ec_find(ifcs_handle_t h) { for (int i = 0; i < 256; i++) if (ec[i].used && ec[i].h == h) return i; return -1; }
static void in_apply(int i, uint32_t c, const ifcs_attr_t* a)
{
    for (uint32_t j = 0; j < c; j++) switch (a[j].id) {
        case IFCS_INTF_ATTR_TYPE: in[i].type = a[j].value.u32; break;
        case IFCS_INTF_ATTR_FORWARDING_INSTANCE: in[i].fwd = a[j].value.handle; break;
        case IFCS_INTF_ATTR_TRANSPORT_INSTANCE: in[i].transport = a[j].value.handle; break;
        case IFCS_INTF_ATTR_TRANSPORT_MAC: memcpy(in[i].mac, a[j].value.mac, 6); break;
        case IFCS_INTF_ATTR_MTU: in[i].mtu = a[j].value.u32; break;
        }
}
static void nh_apply(int i, uint32_t c, const ifcs_attr_t* a)
{
    for (uint32_t j = 0; j < c; j++) switch (a[j].id) {
        case IFCS_NEXTHOP_ATTR_INTF: nhs[i].intf = a[j].value.handle; break;
        case IFCS_NEXTHOP_ATTR_DEST_MAC: memcpy(nhs[i].mac, a[j].value.mac, 6); break;
        case IFCS_NEXTHOP_ATTR_LOCAL_DESTINATION: nhs[i].dest = a[j].value.handle; break;
        }
}

/* test hooks: interface / next hop / ECMP state, interface counters */
int mock_intf_count(void) { int n = 0; for (int i = 0; i < MAX_OBJ; i++) n += in[i].used; return n; }
int mock_intf_info(uint32_t h, uint32_t* type, uint32_t* fwd, uint32_t* transport, uint32_t* mtu, uint8_t* mac)
{
    int i = in_find(h);
    if (i < 0) return 0;
    *type = in[i].type; *fwd = in[i].fwd; *transport = in[i].transport; *mtu = in[i].mtu;
    memcpy(mac, in[i].mac, 6);
    return 1;
}
void mock_set_intf_stat(uint32_t h, int id, uint64_t v) { int i = in_find(h); if (i >= 0 && id >= 0 && id < 34) in[i].stats[id] = v; }
int mock_nh_count(void) { int n = 0; for (int i = 0; i < MAX_OBJ; i++) n += nhs[i].used; return n; }
int mock_nh_info(uint32_t h, uint32_t* intf, uint8_t* mac, uint32_t* dest)
{
    int i = nh_find(h);
    if (i < 0) return 0;
    *intf = nhs[i].intf; *dest = nhs[i].dest;
    memcpy(mac, nhs[i].mac, 6);
    return 1;
}
int mock_ecmp_count(void) { int n = 0; for (int i = 0; i < 256; i++) n += ec[i].used; return n; }
/* Members of ECMP group h (handles, weights); -1 if no such group. */
int mock_ecmp_members(uint32_t h, uint32_t* m, uint32_t* w, int max)
{
    int i = ec_find(h);
    if (i < 0) return -1;
    for (int k = 0; k < ec[i].n && k < max; k++) { m[k] = ec[i].m[k]; w[k] = ec[i].w[k]; }
    return ec[i].n;
}

ifcs_status_t ifcs_l3vni_create(ifcs_node_id_t n, ifcs_handle_t* h, uint32_t c, ifcs_attr_t* a)
{
    (void)n;
    char ids[64] = "";
    for (uint32_t i = 0; i < c && i < 8; i++)
        snprintf(ids + strlen(ids), sizeof(ids) - strlen(ids), "%s%u=%d", i ? "," : "", a[i].id, a[i].value.data);
    mock_logf("ifcs_l3vni_create h=0x%x count=%u attrs=%s", *h, c, ids);
    return IFCS_SUCCESS;
}
ifcs_status_t ifcs_intf_create(ifcs_node_id_t n, ifcs_handle_t* h, uint32_t c, ifcs_attr_t* a)
{
    (void)n;
    *h = next_handle++;
    mock_logf("ifcs_intf_create h=0x%x count=%u type=%u fwd=0x%x transport=0x%x", *h, c, a[0].value.u32, a[1].value.handle,
          a[2].value.handle);
    for (int i = 0; i < MAX_OBJ; i++)
        if (!in[i].used) {
            memset(&in[i], 0, sizeof(in[i]));
            in[i].used = 1;
            in[i].h = *h;
            in_apply(i, c, a);
            return IFCS_SUCCESS;
        }
    return IFCS_RESOURCE_FULL;
}
ifcs_status_t ifcs_intf_delete(ifcs_node_id_t n, ifcs_handle_t h)
{
    (void)n;
    int i = in_find(h);
    mock_logf("ifcs_intf_delete h=0x%x found=%d", h, i >= 0);
    if (i < 0) return IFCS_NOTFOUND;
    in[i].used = 0;
    return IFCS_SUCCESS;
}
ifcs_status_t ifcs_intf_attr_set(ifcs_node_id_t n, ifcs_handle_t h, uint32_t c, ifcs_attr_t* a)
{
    (void)n;
    mock_logf("ifcs_intf_attr_set h=0x%x count=%u id=%u", h, c, a[0].id);
    int i = in_find(h);
    if (i < 0) return IFCS_NOTFOUND;
    in_apply(i, c, a);
    return IFCS_SUCCESS;
}
ifcs_status_t ifcs_intf_stats_get(ifcs_node_id_t n, ifcs_handle_t h, uint32_t c, ifcs_intf_stats_id_t* ids, uint64_t* v, uint32_t* got)
{
    (void)n;
    mock_logf("ifcs_intf_stats_get h=0x%x count=%u", h, c);
    int i = in_find(h);
    if (i < 0) return IFCS_NOTFOUND;
    for (uint32_t k = 0; k < c; k++) {
        if (ids[k] < 0 || ids[k] >= 34) return IFCS_PARAM;
        v[k] = in[i].stats[ids[k]];
    }
    if (got) *got = c;
    return IFCS_SUCCESS;
}
ifcs_status_t ifcs_intf_stats_clear(ifcs_node_id_t n, ifcs_handle_t h, uint32_t c, ifcs_intf_stats_id_t* ids)
{
    (void)n;
    mock_logf("ifcs_intf_stats_clear h=0x%x count=%u", h, c);
    int i = in_find(h);
    if (i < 0) return IFCS_NOTFOUND;
    for (uint32_t k = 0; k < c; k++)
        if (ids[k] >= 0 && ids[k] < 34) in[i].stats[ids[k]] = 0;
    return IFCS_SUCCESS;
}
ifcs_status_t ifcs_nexthop_create(ifcs_node_id_t n, ifcs_handle_t* h, uint32_t c, ifcs_attr_t* a)
{
    (void)n;
    *h = next_handle++;
    mock_logf("ifcs_nexthop_create h=0x%x count=%u intf=0x%x dest=0x%x", *h, c, a[0].value.handle, a[2].value.handle);
    for (int i = 0; i < MAX_OBJ; i++)
        if (!nhs[i].used) {
            memset(&nhs[i], 0, sizeof(nhs[i]));
            nhs[i].used = 1;
            nhs[i].h = *h;
            nh_apply(i, c, a);
            return IFCS_SUCCESS;
        }
    return IFCS_RESOURCE_FULL;
}
ifcs_status_t ifcs_nexthop_delete(ifcs_node_id_t n, ifcs_handle_t h)
{
    (void)n;
    int i = nh_find(h);
    mock_logf("ifcs_nexthop_delete h=0x%x found=%d", h, i >= 0);
    if (i < 0) return IFCS_NOTFOUND;
    nhs[i].used = 0;
    return IFCS_SUCCESS;
}
ifcs_status_t ifcs_nexthop_attr_set(ifcs_node_id_t n, ifcs_handle_t h, uint32_t c, ifcs_attr_t* a)
{
    (void)n;
    mock_logf("ifcs_nexthop_attr_set h=0x%x count=%u id=%u", h, c, a[0].id);
    int i = nh_find(h);
    if (i < 0) return IFCS_NOTFOUND;
    nh_apply(i, c, a);
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
        if (a[j].id == IFCS_ROUTE_ENTRY_ATTR_TTL_CHECK_PROFILE) rt[i].ttlp = (int)a[j].value.u32;
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
            rt[i].ttlp = 0;
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
ifcs_status_t ifcs_ecmp_create(ifcs_node_id_t n, ifcs_handle_t* h, uint32_t c, ifcs_attr_t* a)
{
    (void)n; (void)a;
    *h = IFCS_NULL_HANDLE;
    ifcs_status_t r = mock_create_obj("ifcs_ecmp_create", h, c);
    for (int i = 0; i < 256; i++)
        if (!ec[i].used) {
            memset(&ec[i], 0, sizeof(ec[i]));
            ec[i].used = 1;
            ec[i].h = *h;
            return r;
        }
    return IFCS_RESOURCE_FULL;
}
ifcs_status_t ifcs_ecmp_delete(ifcs_node_id_t n, ifcs_handle_t h)
{
    (void)n;
    int i = ec_find(h);
    mock_logf("ifcs_ecmp_delete h=0x%x members=%d", h, i >= 0 ? ec[i].n : -1);
    if (i < 0) return IFCS_NOTFOUND;
    if (ec[i].n) return IFCS_BUSY;   /* members first (I) */
    ec[i].used = 0;
    return IFCS_SUCCESS;
}
ifcs_status_t ifcs_ecmp_member_add(ifcs_node_id_t n, ifcs_handle_t e, uint32_t m, ifcs_handle_t* l, uint32_t c, ifcs_attr_t* a)
{
    (void)n;
    mock_logf("ifcs_ecmp_member_add ecmp=0x%x n=%u first=0x%x weight=%u", e, m, m ? l[0] : 0, c ? a[0].value.u32 : 1);
    int i = ec_find(e);
    if (i < 0) return IFCS_NOTFOUND;
    for (uint32_t k = 0; k < m; k++) {   /* a repeated nexthop = more weight (isai's form) */
        if (nh_find(l[k]) < 0) return IFCS_PARAM;
        if (ec[i].n == 64) return IFCS_RESOURCE_FULL;
        ec[i].m[ec[i].n] = l[k];
        ec[i].w[ec[i].n] = c ? a[0].value.u32 : 1;
        ec[i].n++;
    }
    return IFCS_SUCCESS;
}
ifcs_status_t ifcs_ecmp_member_replace(ifcs_node_id_t n, ifcs_handle_t e, uint32_t m, ifcs_handle_t* l, uint32_t c, ifcs_attr_t* a)
{
    (void)n; (void)c; (void)a;
    mock_logf("ifcs_ecmp_member_replace ecmp=0x%x n=%u first=0x%x", e, m, m ? l[0] : 0);
    int i = ec_find(e);
    if (i < 0) return IFCS_NOTFOUND;
    if (m > 64) return IFCS_RESOURCE_FULL;
    for (uint32_t k = 0; k < m; k++)
        if (nh_find(l[k]) < 0) return IFCS_PARAM;
    for (uint32_t k = 0; k < m; k++) { ec[i].m[k] = l[k]; ec[i].w[k] = 1; }
    ec[i].n = (int)m;
    return IFCS_SUCCESS;
}
ifcs_status_t ifcs_ecmp_member_remove(ifcs_node_id_t n, ifcs_handle_t e, uint32_t m, ifcs_handle_t* l)
{
    (void)n;
    mock_logf("ifcs_ecmp_member_remove ecmp=0x%x n=%u first=0x%x", e, m, m ? l[0] : 0);
    int i = ec_find(e);
    if (i < 0) return IFCS_NOTFOUND;
    for (uint32_t k = 0; k < m; k++) {
        int j;
        for (j = 0; j < ec[i].n && ec[i].m[j] != l[k]; j++) {}
        if (j == ec[i].n) return IFCS_NOTFOUND;
        for (; j + 1 < ec[i].n; j++) { ec[i].m[j] = ec[i].m[j + 1]; ec[i].w[j] = ec[i].w[j + 1]; }
        ec[i].n--;
    }
    return IFCS_SUCCESS;
}
ifcs_status_t ifcs_ecmp_usage_get(ifcs_node_id_t n, uint32_t c, ifcs_attr_t* a, ifcs_usage_t* u)
{
    (void)n; (void)c; (void)a;
    u->max = 512;
    u->current = 0;
    return IFCS_SUCCESS;
}

