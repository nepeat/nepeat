/* SPDX-License-Identifier: Apache-2.0
 * Mock libifcs: STP, mc_dests, flood groups, L2VNIs, L2 entries, L2 events (mock.h). */
#include "mock.h"

static ifcs_l2_event_notify_fn_t l2_cb;
static void* l2_user;
static struct { int used; ifcs_l2_entry_key_t key; ifcs_handle_t dest; uint32_t type; } l2[MAX_OBJ];
static int l2vni_exists[4096];
/* Who is in an L2VNI or behind an mc_dest (libifcs refuses such a sysport as a
 * LAG member: mock_lag.c asks mock_l2_refs). */
static struct { int used; uint32_t vni; ifcs_handle_t h; } vmem[MAX_OBJ];
static struct { int used; ifcs_handle_t mcd, dest; } mcds[MAX_OBJ];
int mock_l2_refs(ifcs_handle_t h);
extern int g_mlr;

/* ---------------------------------------------------------- test hooks */
int mock_fire_l2(int type, const uint8_t mac[6], uint16_t vid, uint32_t d)
{
    if (!l2_cb) return -1;
    ifcs_l2_event_info_t e;
    ifcs_attr_t a;
    memset(&e, 0, sizeof(e));
    memset(&a, 0, sizeof(a));
    e.event_type = type;
    e.count = 1;
    e.l2_entry.key_type = IFCS_L2_ENTRY_KEY_TYPE_MAC_L2VNI;
    memcpy(e.l2_entry.key.mac_l2vni.mac_addr, mac, 6);
    e.l2_entry.key.mac_l2vni.l2vni = IFCS_HANDLE_L2VNI(vid);
    a.id = IFCS_L2_ENTRY_ATTR_ENTRY_DEST;
    a.value.handle = sp_of(d);
    e.attr_count = 1;
    e.attr_p = &a;
    l2_cb(0, l2_user, &e);
    return 0;
}
int mock_l2_count(void) { int n = 0; for (int i = 0; i < MAX_OBJ; i++) n += l2[i].used; return n; }
/* An L2 event whose ENTRY_DEST is any handle (a LAG). */
int mock_fire_l2_dest(int type, const uint8_t mac[6], uint16_t vid, uint32_t dest)
{
    if (!l2_cb) return -1;
    ifcs_l2_event_info_t e;
    ifcs_attr_t a;
    memset(&e, 0, sizeof(e));
    memset(&a, 0, sizeof(a));
    e.event_type = type;
    e.count = 1;
    e.l2_entry.key_type = IFCS_L2_ENTRY_KEY_TYPE_MAC_L2VNI;
    memcpy(e.l2_entry.key.mac_l2vni.mac_addr, mac, 6);
    e.l2_entry.key.mac_l2vni.l2vni = IFCS_HANDLE_L2VNI(vid);
    a.id = IFCS_L2_ENTRY_ATTR_ENTRY_DEST;
    a.value.handle = dest;
    e.attr_count = 1;
    e.attr_p = &a;
    l2_cb(0, l2_user, &e);
    return 0;
}
int mock_l2vni_has(uint16_t vid, uint32_t h)
{
    for (int i = 0; i < MAX_OBJ; i++)
        if (vmem[i].used && vmem[i].vni == vid && vmem[i].h == h) return 1;
    return 0;
}
/* ENTRY_DEST of the L2 entry mac/vid, 0 if none. */
uint32_t mock_l2_dest(const uint8_t mac[6], uint16_t vid)
{
    for (int i = 0; i < MAX_OBJ; i++)
        if (l2[i].used && memcmp(l2[i].key.key.mac_l2vni.mac_addr, mac, 6) == 0 &&
            IFCS_HANDLE_VALUE(l2[i].key.key.mac_l2vni.l2vni) == vid)
            return l2[i].dest;
    return 0;
}
int mock_l2_refs(ifcs_handle_t h)
{
    int n = 0;
    for (int i = 0; i < MAX_OBJ; i++) {
        n += vmem[i].used && vmem[i].h == h;
        n += mcds[i].used && mcds[i].dest == h;
        n += l2[i].used && l2[i].dest == h;
    }
    return n;
}

/* ------------------------------------------------------------------- L2 */
ifcs_status_t ifcs_stp_create(ifcs_node_id_t n, ifcs_handle_t* h, uint32_t c, ifcs_attr_t* a) { (void)n; (void)a; return mock_create_obj("ifcs_stp_create", h, c); }
ifcs_status_t ifcs_stp_port_add(ifcs_node_id_t n, ifcs_handle_t s, uint32_t m, ifcs_handle_t* l, uint32_t c, ifcs_attr_t* a)
{
    (void)n; (void)l;
    mock_logf("ifcs_stp_port_add stp=0x%x members=%u attrs=%u state=%u first=0x%x", s, m, c, c ? a[0].value.u32 : 99, m ? l[0] : 0);
    return IFCS_SUCCESS;
}
ifcs_status_t ifcs_mc_dest_create(ifcs_node_id_t n, ifcs_handle_t* h, uint32_t c, ifcs_attr_t* a)
{
    (void)n;
    if (*h == IFCS_NULL_HANDLE) *h = next_handle++;
    mock_logf("ifcs_mc_dest_create h=0x%x count=%u dest=0x%x", *h, c, c ? a[0].value.handle : 0);
    for (int i = 0; i < MAX_OBJ; i++)
        if (!mcds[i].used) { mcds[i].used = 1; mcds[i].mcd = *h; mcds[i].dest = c ? a[0].value.handle : 0; break; }
    return IFCS_SUCCESS;
}
ifcs_status_t ifcs_mc_dest_delete(ifcs_node_id_t n, ifcs_handle_t h)
{
    (void)n;
    mock_logf("ifcs_mc_dest_delete h=0x%x", h);
    for (int i = 0; i < MAX_OBJ; i++)
        if (mcds[i].used && mcds[i].mcd == h) mcds[i].used = 0;
    return IFCS_SUCCESS;
}
ifcs_status_t ifcs_mdg_create(ifcs_node_id_t n, ifcs_handle_t* h, uint32_t c, ifcs_attr_t* a) { (void)n; (void)a; return mock_create_obj("ifcs_mdg_create", h, c); }
ifcs_status_t ifcs_mdg_delete(ifcs_node_id_t n, ifcs_handle_t h) { (void)n; mock_logf("ifcs_mdg_delete h=0x%x", h); return IFCS_SUCCESS; }
ifcs_status_t ifcs_mdg_mc_dest_add(ifcs_node_id_t n, ifcs_handle_t g, uint32_t m, ifcs_handle_t* l, uint32_t c, ifcs_attr_t* a)
{
    (void)n; (void)c; (void)a;
    mock_logf("ifcs_mdg_mc_dest_add mdg=0x%x n=%u first=0x%x", g, m, m ? l[0] : 0);
    for (uint32_t j = 0; j < m; j++)
        for (int i = 0; i < MAX_OBJ; i++)   /* a LAG (mock handles 0x282xxxxx) needs multicast LAG resolution */
            if (mcds[i].used && mcds[i].mcd == l[j] && (mcds[i].dest >> 20) == 0x282 && !g_mlr) return IFCS_INVAL;
    return IFCS_SUCCESS;
}
ifcs_status_t ifcs_mdg_mc_dest_remove(ifcs_node_id_t n, ifcs_handle_t g, uint32_t m, ifcs_handle_t* l)
{
    (void)n;
    mock_logf("ifcs_mdg_mc_dest_remove mdg=0x%x n=%u first=0x%x", g, m, m ? l[0] : 0);
    return IFCS_SUCCESS;
}
ifcs_status_t ifcs_l2vni_create(ifcs_node_id_t n, ifcs_handle_t* h, uint32_t c, ifcs_attr_t* a)
{
    (void)n;
    uint32_t v = IFCS_HANDLE_VALUE(*h);
    mock_logf("ifcs_l2vni_create vni=%u count=%u stp=0x%x flood=0x%x", v, c, c > 0 ? a[0].value.handle : 0, c > 1 ? a[1].value.handle : 0);
    if (v >= 4096) return IFCS_PARAM;
    if (l2vni_exists[v]) return IFCS_EXIST;
    l2vni_exists[v] = 1;
    return IFCS_SUCCESS;
}
ifcs_status_t ifcs_l2vni_delete(ifcs_node_id_t n, ifcs_handle_t h)
{
    (void)n;
    uint32_t v = IFCS_HANDLE_VALUE(h);
    mock_logf("ifcs_l2vni_delete vni=%u", v);
    if (v >= 4096 || !l2vni_exists[v]) return IFCS_NOTFOUND;
    for (int i = 0; i < MAX_OBJ; i++)
        if (vmem[i].used && vmem[i].vni == v) return IFCS_BUSY;   /* members first */
    l2vni_exists[v] = 0;
    return IFCS_SUCCESS;
}
ifcs_status_t ifcs_l2vni_attr_set(ifcs_node_id_t n, ifcs_handle_t h, uint32_t c, ifcs_attr_t* a)
{
    (void)n; (void)a;
    mock_logf("ifcs_l2vni_attr_set vni=%u count=%u", IFCS_HANDLE_VALUE(h), c);
    return IFCS_SUCCESS;
}
ifcs_status_t ifcs_l2vni_member_add(ifcs_node_id_t n, ifcs_handle_t h, uint32_t m, ifcs_handle_t* l, uint32_t c, ifcs_attr_t* a)
{
    (void)n; (void)c; (void)a;
    mock_logf("ifcs_l2vni_member_add vni=%u n=%u first=0x%x", IFCS_HANDLE_VALUE(h), m, m ? l[0] : 0);
    if (!l2vni_exists[IFCS_HANDLE_VALUE(h) & 4095]) return IFCS_NOTFOUND;
    for (uint32_t j = 0; j < m; j++) {
        if (mock_l2vni_has((uint16_t)IFCS_HANDLE_VALUE(h), l[j])) return IFCS_EXIST;
        for (int i = 0; i < MAX_OBJ; i++)
            if (!vmem[i].used) { vmem[i].used = 1; vmem[i].vni = IFCS_HANDLE_VALUE(h); vmem[i].h = l[j]; break; }
    }
    return IFCS_SUCCESS;
}
ifcs_status_t ifcs_l2vni_member_remove(ifcs_node_id_t n, ifcs_handle_t h, uint32_t m, ifcs_handle_t* l)
{
    (void)n;
    mock_logf("ifcs_l2vni_member_remove vni=%u n=%u first=0x%x", IFCS_HANDLE_VALUE(h), m, m ? l[0] : 0);
    for (uint32_t j = 0; j < m; j++)
        for (int i = 0; i < MAX_OBJ; i++)
            if (vmem[i].used && vmem[i].vni == IFCS_HANDLE_VALUE(h) && vmem[i].h == l[j]) vmem[i].used = 0;
    return IFCS_SUCCESS;
}
ifcs_status_t ifcs_stp_port_remove(ifcs_node_id_t n, ifcs_handle_t s, uint32_t m, ifcs_handle_t* l)
{
    (void)n;
    mock_logf("ifcs_stp_port_remove stp=0x%x members=%u first=0x%x", s, m, m ? l[0] : 0);
    return IFCS_SUCCESS;
}
ifcs_status_t ifcs_l2_entry_key_t_init(ifcs_l2_entry_key_t* k) { memset(k, 0, sizeof(*k)); k->_base_key = 0xb45e; return IFCS_SUCCESS; }
ifcs_status_t ifcs_l2_entry_key_t_key_type_set(ifcs_l2_entry_key_t* k, int t) { k->key_type = t; k->dirty |= 1; return IFCS_SUCCESS; }
ifcs_status_t ifcs_l2_entry_key_t_mac_l2vni_set(ifcs_l2_entry_key_t* k, ifcs_l2_entry_key_mac_l2vni_t v) { k->key.mac_l2vni = v; k->dirty |= 4; return IFCS_SUCCESS; }
ifcs_status_t ifcs_l2_entry_key_mac_l2vni_t_mac_addr_set(ifcs_l2_entry_key_mac_l2vni_t* k, ifcs_mac_t* m) { memcpy(k->mac_addr, *m, 6); return IFCS_SUCCESS; }
ifcs_status_t ifcs_l2_entry_key_mac_l2vni_t_l2vni_set(ifcs_l2_entry_key_mac_l2vni_t* k, ifcs_handle_t h) { k->l2vni = h; return IFCS_SUCCESS; }
static int l2_find(const ifcs_l2_entry_key_t* k)
{
    for (int i = 0; i < MAX_OBJ; i++)
        if (l2[i].used && memcmp(l2[i].key.key.mac_l2vni.mac_addr, k->key.mac_l2vni.mac_addr, 6) == 0 &&
            l2[i].key.key.mac_l2vni.l2vni == k->key.mac_l2vni.l2vni)
            return i;
    return -1;
}
ifcs_status_t ifcs_l2_entry_create(ifcs_node_id_t n, ifcs_l2_entry_key_t* k, uint32_t c, ifcs_attr_t* a)
{
    (void)n;
    const uint8_t* m = k->key.mac_l2vni.mac_addr;
    mock_logf("ifcs_l2_entry_create base=%x type=%d mac=%02x:%02x:%02x:%02x:%02x:%02x vni=%u count=%u dest=0x%x etype=%u",
          k->_base_key, k->key_type, m[0], m[1], m[2], m[3], m[4], m[5], IFCS_HANDLE_VALUE(k->key.mac_l2vni.l2vni), c,
          c > 0 ? a[0].value.handle : 0, c > 1 ? a[1].value.u32 : 99);
    if (k->_base_key != 0xb45e) return IFCS_PARAM;   /* key not built through the accessors */
    if (l2_find(k) >= 0) return IFCS_EXIST;
    for (int i = 0; i < MAX_OBJ; i++)
        if (!l2[i].used) {
            l2[i].used = 1;
            l2[i].key = *k;
            l2[i].dest = c > 0 ? a[0].value.handle : 0;
            l2[i].type = c > 1 ? a[1].value.u32 : 0;
            return IFCS_SUCCESS;
        }
    return IFCS_RESOURCE_FULL;
}
ifcs_status_t ifcs_l2_entry_delete(ifcs_node_id_t n, ifcs_l2_entry_key_t* k)
{
    (void)n;
    int i = l2_find(k);
    mock_logf("ifcs_l2_entry_delete vni=%u found=%d", IFCS_HANDLE_VALUE(k->key.mac_l2vni.l2vni), i >= 0);
    if (i < 0) return IFCS_NOTFOUND;
    l2[i].used = 0;
    return IFCS_SUCCESS;
}
ifcs_status_t ifcs_l2_entry_get_all(ifcs_node_id_t n, ifcs_l2_entry_key_t* k, uint32_t c, ifcs_attr_t* a,
                                    ifcs_l2_entry_user_cb_t cb, void* u, uint32_t* cnt)
{
    (void)k; (void)c; (void)a;
    mock_logf("ifcs_l2_entry_get_all");
    uint32_t x = 0;
    for (int i = 0; i < MAX_OBJ; i++)
        if (l2[i].used) {
            ifcs_l2_entry_key_t key = l2[i].key;
            cb(n, &key, 0, NULL, u);
            x++;
        }
    if (cnt) *cnt = x;
    return IFCS_SUCCESS;
}
ifcs_status_t ifcs_l2_entry_attr_get(ifcs_node_id_t n, ifcs_l2_entry_key_t* k, uint32_t c, ifcs_attr_t* a, uint32_t* got)
{
    (void)n;
    int i = l2_find(k);
    if (i < 0) return IFCS_NOTFOUND;
    for (uint32_t j = 0; j < c; j++) {
        if (a[j].id == IFCS_L2_ENTRY_ATTR_ENTRY_TYPE) a[j].value.u32 = l2[i].type;
        else if (a[j].id == IFCS_L2_ENTRY_ATTR_ENTRY_DEST) a[j].value.handle = l2[i].dest;
    }
    if (got) *got = c;
    return IFCS_SUCCESS;
}
ifcs_status_t ifcs_l2_entry_usage_get(ifcs_node_id_t n, ifcs_l2_entry_key_t* k, uint32_t c, ifcs_attr_t* a, ifcs_usage_t* us)
{
    (void)n; (void)k; (void)c; (void)a;
    us->max = 45056;
    us->current = (uint32_t)mock_l2_count();
    return IFCS_SUCCESS;
}
ifcs_status_t ifcs_l2_event_register_notify(ifcs_node_id_t n, void* u, ifcs_l2_event_notify_fn_t f)
{
    mock_logf("ifcs_l2_event_register_notify node=%u", n);
    l2_cb = f;
    l2_user = u;
    return IFCS_SUCCESS;
}
ifcs_status_t ifcs_l2_event_unregister_notify(ifcs_node_id_t n) { mock_logf("ifcs_l2_event_unregister_notify node=%u", n); l2_cb = NULL; return IFCS_SUCCESS; }

