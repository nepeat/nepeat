/* SPDX-License-Identifier: Apache-2.0
 * A stand-in libifcs for tests/abi.rs (built by build.rs for the target, like libifcs_stub.c): every
 * IFCS_SYMBOLS entry defined, and the functions the L2 / route / queue keys and the attribute lists
 * go through defined with their REAL C signatures (checked against the header's <name>_fn typedefs
 * below), so the test calls them through the ifcs crate's own call sites. They behave as libifcs is
 * seen to on the CS8210 (evidence rust-h2/06-ifcs-key-padding.txt): a by-value member is copied into
 * the key byte for byte, the key setters write their field and a dirty bit, and every struct is
 * checked byte for byte against the image C makes of the same fields (memset 0, then the fields):
 * a byte that differs (padding that is not zero, a union byte past the member) answers IFCS_PARAM.
 *
 * What a function received is recorded and read back through ifcs_node_attr_get (attribute id =
 * slot, the value = the recorded bytes, at most 120), so the test needs no unsafe code:
 *   slot 1  the last ifcs_l2_entry_key_mac_l2vni_t passed by value (16 B)
 *   slot 2  the last ifcs_route_entry_key_ip_dest_l3vni_t passed by value (48 B)
 *   slot 3  the last L2 key at create / delete (100 B)
 *   slot 4  the last route key at create / delete (60 B)
 *   slot 5  the last IP prefix of ip_dest_set (40 B)
 *   slot 6  the last queue key of ifcs_queue_attr_set (16 B)
 *   slot 7  the first 8 bytes (id, padding) of each attribute of the last attribute list, up to 15
 * ifcs_node_attr_set with attribute id 0xabc0 fills 64 KiB of this function's stack with 0xa5 (a
 * stack that held other data, as on the switch), so bytes a caller leaves undefined are not zero
 * by luck. */
#include <stddef.h>
#include <string.h>
#include "ifcs_api.h"

#define OVERRIDDEN(X) \
    X(ifcs_node_attr_get) X(ifcs_node_attr_set) X(ifcs_l2_entry_key_t_init) X(ifcs_l2_entry_key_t_key_type_set) \
    X(ifcs_l2_entry_key_t_mac_l2vni_set) X(ifcs_l2_entry_key_mac_l2vni_t_mac_addr_set) \
    X(ifcs_l2_entry_key_mac_l2vni_t_l2vni_set) X(ifcs_l2_entry_create) X(ifcs_l2_entry_delete) \
    X(ifcs_ip_prefix_t_init) X(ifcs_ip_prefix_t_addr_family_set) X(ifcs_ip_prefix_t_addr_set) \
    X(ifcs_ip_prefix_t_mask_set) X(ifcs_route_entry_key_t_init) X(ifcs_route_entry_key_t_key_type_set) \
    X(ifcs_route_entry_key_t_ip_dest_l3vni_set) X(ifcs_route_entry_key_ip_dest_l3vni_t_l3vni_set) \
    X(ifcs_route_entry_key_ip_dest_l3vni_t_ip_dest_set) X(ifcs_route_entry_create) X(ifcs_route_entry_delete) \
    X(ifcs_queue_t_init) X(ifcs_queue_t_devport_set) X(ifcs_queue_t_queue_id_set) X(ifcs_queue_attr_set)

/* every other entry: a stub nobody calls (only dlsym sees it); the overridden ones renamed away */
#define ifcs_node_attr_get ifcs_node_attr_get__stub
#define ifcs_node_attr_set ifcs_node_attr_set__stub
#define ifcs_l2_entry_key_t_init ifcs_l2_entry_key_t_init__stub
#define ifcs_l2_entry_key_t_key_type_set ifcs_l2_entry_key_t_key_type_set__stub
#define ifcs_l2_entry_key_t_mac_l2vni_set ifcs_l2_entry_key_t_mac_l2vni_set__stub
#define ifcs_l2_entry_key_mac_l2vni_t_mac_addr_set ifcs_l2_entry_key_mac_l2vni_t_mac_addr_set__stub
#define ifcs_l2_entry_key_mac_l2vni_t_l2vni_set ifcs_l2_entry_key_mac_l2vni_t_l2vni_set__stub
#define ifcs_l2_entry_create ifcs_l2_entry_create__stub
#define ifcs_l2_entry_delete ifcs_l2_entry_delete__stub
#define ifcs_ip_prefix_t_init ifcs_ip_prefix_t_init__stub
#define ifcs_ip_prefix_t_addr_family_set ifcs_ip_prefix_t_addr_family_set__stub
#define ifcs_ip_prefix_t_addr_set ifcs_ip_prefix_t_addr_set__stub
#define ifcs_ip_prefix_t_mask_set ifcs_ip_prefix_t_mask_set__stub
#define ifcs_route_entry_key_t_init ifcs_route_entry_key_t_init__stub
#define ifcs_route_entry_key_t_key_type_set ifcs_route_entry_key_t_key_type_set__stub
#define ifcs_route_entry_key_t_ip_dest_l3vni_set ifcs_route_entry_key_t_ip_dest_l3vni_set__stub
#define ifcs_route_entry_key_ip_dest_l3vni_t_l3vni_set ifcs_route_entry_key_ip_dest_l3vni_t_l3vni_set__stub
#define ifcs_route_entry_key_ip_dest_l3vni_t_ip_dest_set ifcs_route_entry_key_ip_dest_l3vni_t_ip_dest_set__stub
#define ifcs_route_entry_create ifcs_route_entry_create__stub
#define ifcs_route_entry_delete ifcs_route_entry_delete__stub
#define ifcs_queue_t_init ifcs_queue_t_init__stub
#define ifcs_queue_t_devport_set ifcs_queue_t_devport_set__stub
#define ifcs_queue_t_queue_id_set ifcs_queue_t_queue_id_set__stub
#define ifcs_queue_attr_set ifcs_queue_attr_set__stub
#define STUB(n) int n(void); int n(void) { return 0; }
IFCS_SYMBOLS(STUB)
#undef ifcs_node_attr_get
#undef ifcs_node_attr_set
#undef ifcs_l2_entry_key_t_init
#undef ifcs_l2_entry_key_t_key_type_set
#undef ifcs_l2_entry_key_t_mac_l2vni_set
#undef ifcs_l2_entry_key_mac_l2vni_t_mac_addr_set
#undef ifcs_l2_entry_key_mac_l2vni_t_l2vni_set
#undef ifcs_l2_entry_create
#undef ifcs_l2_entry_delete
#undef ifcs_ip_prefix_t_init
#undef ifcs_ip_prefix_t_addr_family_set
#undef ifcs_ip_prefix_t_addr_set
#undef ifcs_ip_prefix_t_mask_set
#undef ifcs_route_entry_key_t_init
#undef ifcs_route_entry_key_t_key_type_set
#undef ifcs_route_entry_key_t_ip_dest_l3vni_set
#undef ifcs_route_entry_key_ip_dest_l3vni_t_l3vni_set
#undef ifcs_route_entry_key_ip_dest_l3vni_t_ip_dest_set
#undef ifcs_route_entry_create
#undef ifcs_route_entry_delete
#undef ifcs_queue_t_init
#undef ifcs_queue_t_devport_set
#undef ifcs_queue_t_queue_id_set
#undef ifcs_queue_attr_set

enum { SLOTS = 8, SLOT_BYTES = 120, DIRTY_STACK = 0xabc0 };
static unsigned char rec[SLOTS][SLOT_BYTES];
static size_t rec_len[SLOTS];

static void record(int slot, const void* p, size_t n)
{
    if (n > SLOT_BYTES) n = SLOT_BYTES;
    memcpy(rec[slot], p, n);
    rec_len[slot] = n;
}

/* the images C makes of the same fields: memset 0, then each field */
static ifcs_ip_prefix_t prefix_image(const ifcs_ip_prefix_t* p)
{
    ifcs_ip_prefix_t e;
    memset(&e, 0, sizeof e);
    e.addr_family = p->addr_family;
    memcpy(&e.addr, &p->addr, sizeof e.addr);
    memcpy(&e.mask, &p->mask, sizeof e.mask);
    e.dirty = p->dirty;
    return e;
}
static ifcs_l2_entry_key_mac_l2vni_t l2_member_image(const ifcs_l2_entry_key_mac_l2vni_t* m)
{
    ifcs_l2_entry_key_mac_l2vni_t e;
    memset(&e, 0, sizeof e);
    memcpy(e.mac_addr, m->mac_addr, sizeof e.mac_addr);
    e.l2vni = m->l2vni;
    e.dirty = m->dirty;
    return e;
}
static ifcs_route_entry_key_ip_dest_l3vni_t route_member_image(const ifcs_route_entry_key_ip_dest_l3vni_t* m)
{
    ifcs_route_entry_key_ip_dest_l3vni_t e;
    memset(&e, 0, sizeof e);
    ifcs_ip_prefix_t p = prefix_image(&m->ip_dest);
    memcpy(&e.ip_dest, &p, sizeof p);
    e.l3vni = m->l3vni;
    e.dirty = m->dirty;
    return e;
}
static int l2_key_ok(const ifcs_l2_entry_key_t* k)
{
    ifcs_l2_entry_key_t e;
    memset(&e, 0, sizeof e);
    e._base_key = k->_base_key;
    e.key_type = k->key_type;
    ifcs_l2_entry_key_mac_l2vni_t m = l2_member_image(&k->key.mac_l2vni);
    memcpy(&e.key.mac_l2vni, &m, sizeof m);
    e.dirty = k->dirty;
    return memcmp(&e, k, sizeof e) == 0;
}
static int route_key_ok(const ifcs_route_entry_key_t* k)
{
    ifcs_route_entry_key_t e;
    memset(&e, 0, sizeof e);
    e._base_key = k->_base_key;
    e.key_type = k->key_type;
    ifcs_route_entry_key_ip_dest_l3vni_t m = route_member_image(&k->key.ip_dest_l3vni);
    memcpy(&e.key.ip_dest_l3vni, &m, sizeof m);
    e.dirty = k->dirty;
    return memcmp(&e, k, sizeof e) == 0;
}
/* an attribute list: the 4 bytes after id are zero; slot 7 gets the first 8 bytes of each */
static int attrs_ok(uint32_t n, const ifcs_attr_t* a)
{
    static const unsigned char zero[8];
    unsigned char b[SLOT_BYTES];
    size_t len = 0;
    int ok = 1;
    for (uint32_t i = 0; a && i < n; i++) {
        const unsigned char* p = (const unsigned char*)&a[i];
        if (memcmp(p + sizeof a[i].id, zero, offsetof(ifcs_attr_t, value) - sizeof a[i].id) != 0) ok = 0;
        if (len + 8 <= sizeof b) {
            memcpy(b + len, p, 8);
            len += 8;
        }
    }
    record(7, b, len);
    return ok;
}

ifcs_status_t ifcs_node_attr_get(ifcs_node_id_t node, uint32_t n, ifcs_attr_t* a, uint32_t* got)
{
    (void)node;
    for (uint32_t i = 0; i < n; i++) {
        uint32_t s = a[i].id;
        memset(&a[i].value, 0, sizeof a[i].value);
        if (s < SLOTS) memcpy(&a[i].value, rec[s], rec_len[s]);
    }
    if (got) *got = n;
    return IFCS_SUCCESS;
}

/* fills 64 KiB of stack with 0xa5 (not optimised away: the buffer escapes through the barrier) */
static void __attribute__((noinline)) dirty_stack(void)
{
    volatile unsigned char buf[65536];
    memset((void*)buf, 0xa5, sizeof buf);
    __asm__ __volatile__("" : : "r"(buf) : "memory");
}

ifcs_status_t ifcs_node_attr_set(ifcs_node_id_t node, uint32_t n, ifcs_attr_t* a)
{
    (void)node;
    if (n == 1 && a && a[0].id == DIRTY_STACK) {
        dirty_stack();
        return IFCS_SUCCESS;
    }
    return attrs_ok(n, a) ? IFCS_SUCCESS : IFCS_PARAM;
}

ifcs_status_t ifcs_l2_entry_key_t_init(ifcs_l2_entry_key_t* k) { (void)k; return IFCS_SUCCESS; }
ifcs_status_t ifcs_l2_entry_key_t_key_type_set(ifcs_l2_entry_key_t* k, int t)
{
    k->key_type = t;
    k->dirty |= 1;
    return IFCS_SUCCESS;
}
ifcs_status_t ifcs_l2_entry_key_t_mac_l2vni_set(ifcs_l2_entry_key_t* k, ifcs_l2_entry_key_mac_l2vni_t v)
{
    record(1, &v, sizeof v);
    ifcs_l2_entry_key_mac_l2vni_t e = l2_member_image(&v);
    memcpy(&k->key.mac_l2vni, &v, sizeof v);   /* every byte, as libifcs */
    k->dirty |= 4;
    return memcmp(&e, &v, sizeof v) == 0 ? IFCS_SUCCESS : IFCS_PARAM;
}
ifcs_status_t ifcs_l2_entry_key_mac_l2vni_t_mac_addr_set(ifcs_l2_entry_key_mac_l2vni_t* m, ifcs_mac_t* mac)
{
    memcpy(m->mac_addr, *mac, sizeof m->mac_addr);
    m->dirty |= 1;
    return IFCS_SUCCESS;
}
ifcs_status_t ifcs_l2_entry_key_mac_l2vni_t_l2vni_set(ifcs_l2_entry_key_mac_l2vni_t* m, ifcs_handle_t h)
{
    m->l2vni = h;
    m->dirty |= 2;
    return IFCS_SUCCESS;
}
ifcs_status_t ifcs_l2_entry_create(ifcs_node_id_t node, ifcs_l2_entry_key_t* k, uint32_t n, ifcs_attr_t* a)
{
    (void)node;
    record(3, k, sizeof *k);
    int ok = attrs_ok(n, a);
    return l2_key_ok(k) && ok ? IFCS_SUCCESS : IFCS_PARAM;
}
ifcs_status_t ifcs_l2_entry_delete(ifcs_node_id_t node, ifcs_l2_entry_key_t* k)
{
    (void)node;
    record(3, k, sizeof *k);
    return l2_key_ok(k) ? IFCS_SUCCESS : IFCS_PARAM;
}

ifcs_status_t ifcs_ip_prefix_t_init(ifcs_ip_prefix_t* p) { (void)p; return IFCS_SUCCESS; }
ifcs_status_t ifcs_ip_prefix_t_addr_family_set(ifcs_ip_prefix_t* p, int f)
{
    p->addr_family = f;
    p->dirty |= 1;
    return IFCS_SUCCESS;
}
ifcs_status_t ifcs_ip_prefix_t_addr_set(ifcs_ip_prefix_t* p, ifcs_ip_addr_t* a)
{
    memcpy(&p->addr, a, sizeof p->addr);
    p->dirty |= 2;
    return IFCS_SUCCESS;
}
ifcs_status_t ifcs_ip_prefix_t_mask_set(ifcs_ip_prefix_t* p, ifcs_ip_addr_t* m)
{
    memcpy(&p->mask, m, sizeof p->mask);
    p->dirty |= 4;
    return IFCS_SUCCESS;
}
ifcs_status_t ifcs_route_entry_key_t_init(ifcs_route_entry_key_t* k) { (void)k; return IFCS_SUCCESS; }
ifcs_status_t ifcs_route_entry_key_t_key_type_set(ifcs_route_entry_key_t* k, int t)
{
    k->key_type = t;
    k->dirty |= 1;
    return IFCS_SUCCESS;
}
ifcs_status_t ifcs_route_entry_key_t_ip_dest_l3vni_set(ifcs_route_entry_key_t* k, ifcs_route_entry_key_ip_dest_l3vni_t v)
{
    record(2, &v, sizeof v);
    ifcs_route_entry_key_ip_dest_l3vni_t e = route_member_image(&v);
    memcpy(&k->key.ip_dest_l3vni, &v, sizeof v);
    k->dirty |= 4;
    return memcmp(&e, &v, sizeof v) == 0 ? IFCS_SUCCESS : IFCS_PARAM;
}
ifcs_status_t ifcs_route_entry_key_ip_dest_l3vni_t_l3vni_set(ifcs_route_entry_key_ip_dest_l3vni_t* m, ifcs_handle_t h)
{
    m->l3vni = h;
    m->dirty |= 2;
    return IFCS_SUCCESS;
}
ifcs_status_t ifcs_route_entry_key_ip_dest_l3vni_t_ip_dest_set(ifcs_route_entry_key_ip_dest_l3vni_t* m, ifcs_ip_prefix_t* p)
{
    record(5, p, sizeof *p);
    ifcs_ip_prefix_t e = prefix_image(p);
    memcpy(&m->ip_dest, p, sizeof *p);   /* every byte, as libifcs */
    m->dirty |= 1;
    return memcmp(&e, p, sizeof *p) == 0 ? IFCS_SUCCESS : IFCS_PARAM;
}
ifcs_status_t ifcs_route_entry_create(ifcs_node_id_t node, ifcs_route_entry_key_t* k, uint32_t n, ifcs_attr_t* a)
{
    (void)node;
    record(4, k, sizeof *k);
    int ok = attrs_ok(n, a);
    return route_key_ok(k) && ok ? IFCS_SUCCESS : IFCS_PARAM;
}
ifcs_status_t ifcs_route_entry_delete(ifcs_node_id_t node, ifcs_route_entry_key_t* k)
{
    (void)node;
    record(4, k, sizeof *k);
    return route_key_ok(k) ? IFCS_SUCCESS : IFCS_PARAM;
}

ifcs_status_t ifcs_queue_t_init(ifcs_queue_t* q) { (void)q; return IFCS_SUCCESS; }
ifcs_status_t ifcs_queue_t_devport_set(ifcs_queue_t* q, ifcs_devport_t d)
{
    q->devport = d;
    q->dirty |= 1;
    return IFCS_SUCCESS;
}
ifcs_status_t ifcs_queue_t_queue_id_set(ifcs_queue_t* q, uint32_t id)
{
    q->queue_id = id;
    q->dirty |= 2;
    return IFCS_SUCCESS;
}
ifcs_status_t ifcs_queue_attr_set(ifcs_node_id_t node, ifcs_queue_t* q, uint32_t n, ifcs_attr_t* a)
{
    (void)node;
    record(6, q, sizeof *q);
    ifcs_queue_t e;
    memset(&e, 0, sizeof e);
    e._base_key = q->_base_key;
    e.devport = q->devport;
    e.queue_id = q->queue_id;
    e.dirty = q->dirty;
    int ok = attrs_ok(n, a);
    return memcmp(&e, q, sizeof e) == 0 && ok ? IFCS_SUCCESS : IFCS_PARAM;
}

/* each definition above has the header's type (a mismatch does not compile) */
#define CHECK(n) static n##_fn const check_##n __attribute__((unused)) = n;
OVERRIDDEN(CHECK)
