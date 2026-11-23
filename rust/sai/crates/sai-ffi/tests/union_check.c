/* The C side of the union test (src/convert.rs, test "the_c_compiler_agrees_on_every_member"):
 * sai_attribute_value_t is a union, and which bytes a member occupies is the C compiler's
 * decision for the target (on big-endian ppc64 a u8, a u16 and a bool do not sit where the
 * low byte of a u32 does). This program builds and inspects values through the member the
 * SAI header names, with the real headers and the target's compiler; the Rust test sends
 * it the byte images its encoder produced and decodes the images made here.
 *
 * Built by build.rs into $OUT_DIR/target-tests/union-check (the target test bundle copies
 * it); no test compiles C at run time. Protocol, one request per line on stdin:
 *   S                      -> "<sizeof value> <sizeof pointer>"
 *   R <case> <addr> <cap>  -> a value as a caller prepares it for a get: every byte 0xa5,
 *                             and for a list case the list's count = cap, pointer = addr
 *   M <case> <addr>        -> "<value hex> <element hex or ->": the case's value made in C
 *                             (0xa5 everywhere else), a list pointing to addr
 *   E <case> <addr> <value hex> <element hex or ->  -> "ok" when the image holds the
 *                             case's value in the right member and no byte outside the
 *                             member changed from 0xa5, else "FAIL <what>"
 */
#include <stdint.h>
#include <stdio.h>
#include <string.h>

#include <sai.h>

#define GARBAGE 0xa5
#define MAX_ELEMS 512

typedef struct {
    sai_attribute_value_t v;
    unsigned char elems[MAX_ELEMS];
    size_t elem_bytes;   /* bytes of elems in use */
    size_t member_size;  /* bytes of the union the member covers */
} image_t;

static const unsigned char IP6[16] = {0xfc, 0x00, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 0x07};
static const unsigned char MASK6[16] = {0xff, 0xff, 0xff, 0xff, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0};

static void qos(sai_qos_map_params_t* p, int n)
{
    p->tc = (uint8_t)(1 + n);
    p->dscp = (uint8_t)(10 + n);
    p->dot1p = (uint8_t)(2 + n);
    p->prio = (uint8_t)(3 + n);
    p->pg = (uint8_t)(4 + n);
    p->queue_index = (uint8_t)(5 + n);
    p->color = n % 2 ? SAI_PACKET_COLOR_RED : SAI_PACKET_COLOR_YELLOW;
    p->mpls_exp = (uint8_t)(6 + n);
    p->fc = (uint8_t)(7 + n);
    p->dei = (uint8_t)(n % 2);
    p->vc = (uint8_t)(8 + n);
}

#define SCALAR(member, expr)            \
    do {                                \
        im->v.member = (expr);          \
        im->member_size = sizeof(im->v.member); \
    } while (0)
#define BYTES(member, src)                      \
    do {                                        \
        memcpy(&im->v.member, (src), sizeof(im->v.member)); \
        im->member_size = sizeof(im->v.member); \
    } while (0)
#define LIST(member, type, ...)                                 \
    do {                                                        \
        const type items[] = {__VA_ARGS__};                     \
        memcpy(im->elems, items, sizeof(items));                \
        im->elem_bytes = sizeof(items);                         \
        im->v.member.count = sizeof(items) / sizeof(items[0]);  \
        im->v.member.list = (type*)addr;                        \
        im->member_size = sizeof(im->v.member);                 \
    } while (0)

/* ACL fields (enable + mask + data) and actions (enable + parameter): the members the case
 * names, set through them. For u8list the data list is at addr and the mask list at addr+16. */
#define ACLF(member, dval, mval)                       \
    do {                                               \
        im->v.aclfield.enable = true;                  \
        im->v.aclfield.data.member = (dval);           \
        im->v.aclfield.mask.member = (mval);           \
    } while (0)
#define ACLF_BYTES(member, dsrc, msrc)                                         \
    do {                                                                       \
        im->v.aclfield.enable = true;                                          \
        memcpy(&im->v.aclfield.data.member, (dsrc), sizeof(im->v.aclfield.data.member)); \
        memcpy(&im->v.aclfield.mask.member, (msrc), sizeof(im->v.aclfield.mask.member)); \
    } while (0)
#define ACLA(member, val)                              \
    do {                                               \
        im->v.aclaction.enable = true;                 \
        im->v.aclaction.parameter.member = (val);      \
    } while (0)
#define ACLA_BYTES(member, src)                                                        \
    do {                                                                               \
        im->v.aclaction.enable = true;                                                 \
        memcpy(&im->v.aclaction.parameter.member, (src), sizeof(im->v.aclaction.parameter.member)); \
    } while (0)
static const sai_object_id_t ACL_OIDS[3] = {0x2100000000000010ULL, 0x20, 0x0102030405060708ULL};

static int make_acl(const char* name, image_t* im, uintptr_t addr)
{
    int field = !strncmp(name, "aclfield_", 9);
    im->member_size = field ? sizeof(im->v.aclfield) : sizeof(im->v.aclaction);
    if (!strcmp(name, "aclfield_off")) im->v.aclfield.enable = false;
    else if (!strcmp(name, "aclfield_bool")) { im->v.aclfield.enable = true; im->v.aclfield.data.booldata = true; }
    else if (!strcmp(name, "aclfield_u8")) ACLF(u8, 0x5c, 0xf0);
    else if (!strcmp(name, "aclfield_u16")) ACLF(u16, 0x0806, 0xffff);
    else if (!strcmp(name, "aclfield_u32")) ACLF(u32, 0x01020304, 0xffff0000);
    else if (!strcmp(name, "aclfield_u64")) ACLF(u64, 0x0102030405060708ULL, 0xffffffff00000000ULL);
    else if (!strcmp(name, "aclfield_enum")) ACLF(s32, 3, -1);
    else if (!strcmp(name, "aclfield_mac")) ACLF_BYTES(mac, "\x02\x11\x22\x33\x44\x55", "\xff\xff\xff\x00\x00\x00");
    else if (!strcmp(name, "aclfield_ip4")) ACLF_BYTES(ip4, "\x0a\x01\x02\x03", "\xff\xff\x00\x00");
    else if (!strcmp(name, "aclfield_ip6")) ACLF_BYTES(ip6, IP6, MASK6);
    else if (!strcmp(name, "aclfield_oid")) { im->v.aclfield.enable = true; im->v.aclfield.data.oid = 0x2100000000000042ULL; }
    else if (!strcmp(name, "aclfield_objlist")) {
        im->v.aclfield.enable = true;
        memcpy(im->elems, ACL_OIDS, sizeof(ACL_OIDS));
        im->elem_bytes = sizeof(ACL_OIDS);
        im->v.aclfield.data.objlist.count = 3;
        im->v.aclfield.data.objlist.list = (sai_object_id_t*)addr;
    } else if (!strcmp(name, "aclfield_u8list")) {
        im->v.aclfield.enable = true;
        memset(im->elems, 0, 19);
        memcpy(im->elems, "\x01\x02\x03", 3);
        memcpy(im->elems + 16, "\xff\xf0\x0f", 3);
        im->elem_bytes = 19;
        im->v.aclfield.data.u8list.count = 3;
        im->v.aclfield.data.u8list.list = (uint8_t*)addr;
        im->v.aclfield.mask.u8list.count = 3;
        im->v.aclfield.mask.u8list.list = (uint8_t*)(addr + 16);
    } else if (!strcmp(name, "aclaction_off")) im->v.aclaction.enable = false;
    else if (!strcmp(name, "aclaction_bool")) ACLA(booldata, true);
    else if (!strcmp(name, "aclaction_u8")) ACLA(u8, 0x5c);
    else if (!strcmp(name, "aclaction_u16")) ACLA(u16, 0x1234);
    else if (!strcmp(name, "aclaction_u32")) ACLA(u32, 0x01020304);
    else if (!strcmp(name, "aclaction_s32")) ACLA(s32, -7);
    else if (!strcmp(name, "aclaction_enum")) ACLA(s32, SAI_PACKET_ACTION_DROP);
    else if (!strcmp(name, "aclaction_mac")) ACLA_BYTES(mac, "\x02\x11\x22\x33\x44\x55");
    else if (!strcmp(name, "aclaction_ip4")) ACLA_BYTES(ip4, "\x0a\x01\x02\x03");
    else if (!strcmp(name, "aclaction_ip6")) ACLA_BYTES(ip6, IP6);
    else if (!strcmp(name, "aclaction_oid")) ACLA(oid, 0x2100000000000042ULL);
    else if (!strcmp(name, "aclaction_objlist")) {
        im->v.aclaction.enable = true;
        memcpy(im->elems, ACL_OIDS, sizeof(ACL_OIDS));
        im->elem_bytes = sizeof(ACL_OIDS);
        im->v.aclaction.parameter.objlist.count = 3;
        im->v.aclaction.parameter.objlist.list = (sai_object_id_t*)addr;
    } else if (!strcmp(name, "aclaction_ipaddr6")) {
        im->v.aclaction.enable = true;
        im->v.aclaction.parameter.ipaddr.addr_family = SAI_IP_ADDR_FAMILY_IPV6;
        memcpy(im->v.aclaction.parameter.ipaddr.addr.ip6, IP6, 16);
    } else
        return 0;
    return 1;
}

/* The value of a case, set through the member the header names. 0: no such case. */
static int make(const char* name, image_t* im, uintptr_t addr)
{
    memset(im, 0, sizeof(*im));
    memset(&im->v, GARBAGE, sizeof(im->v));
    memset(im->elems, GARBAGE, sizeof(im->elems));
    if (!strcmp(name, "bool")) SCALAR(booldata, true);
    else if (!strcmp(name, "u8")) SCALAR(u8, 0x5c);
    else if (!strcmp(name, "s8")) SCALAR(s8, -3);
    else if (!strcmp(name, "u16")) SCALAR(u16, 0x1234);
    else if (!strcmp(name, "s16")) SCALAR(s16, -2);
    else if (!strcmp(name, "u32")) SCALAR(u32, 0x01020304);
    else if (!strcmp(name, "s32")) SCALAR(s32, -7);
    else if (!strcmp(name, "enum")) SCALAR(s32, 0x00010203);
    else if (!strcmp(name, "u64")) SCALAR(u64, 0x0102030405060708ULL);
    else if (!strcmp(name, "s64")) SCALAR(s64, -(1LL << 40));
    else if (!strcmp(name, "oid")) SCALAR(oid, 0x2100000000000042ULL);
    else if (!strcmp(name, "ptr")) SCALAR(ptr, (sai_pointer_t)(uintptr_t)0x1000);
    else if (!strcmp(name, "mac")) BYTES(mac, "\x02\x11\x22\x33\x44\x55");
    else if (!strcmp(name, "ip4")) BYTES(ip4, "\x0a\x01\x02\x03");
    else if (!strcmp(name, "ip6")) BYTES(ip6, IP6);
    else if (!strcmp(name, "chardata")) {
        memset(im->v.chardata, 0, sizeof(im->v.chardata));
        strcpy(im->v.chardata, "Ethernet0");
        im->member_size = sizeof(im->v.chardata);
    } else if (!strcmp(name, "u32range")) {
        im->v.u32range.min = 5;
        im->v.u32range.max = 0x01020304;
        im->member_size = sizeof(im->v.u32range);
    } else if (!strcmp(name, "u16range")) {
        im->v.u16range.min = 1;
        im->v.u16range.max = 4094;
        im->member_size = sizeof(im->v.u16range);
    } else if (!strcmp(name, "s32range")) {
        im->v.s32range.min = -5;
        im->v.s32range.max = 9;
        im->member_size = sizeof(im->v.s32range);
    } else if (!strcmp(name, "ipaddr4")) {
        im->v.ipaddr.addr_family = SAI_IP_ADDR_FAMILY_IPV4;
        memset(&im->v.ipaddr.addr, 0, sizeof(im->v.ipaddr.addr));
        memcpy(&im->v.ipaddr.addr.ip4, "\xc0\xa8\x00\x01", 4);
        im->member_size = sizeof(im->v.ipaddr);
    } else if (!strcmp(name, "ipaddr6")) {
        im->v.ipaddr.addr_family = SAI_IP_ADDR_FAMILY_IPV6;
        memcpy(im->v.ipaddr.addr.ip6, IP6, 16);
        im->member_size = sizeof(im->v.ipaddr);
    } else if (!strcmp(name, "ipprefix4")) {
        im->v.ipprefix.addr_family = SAI_IP_ADDR_FAMILY_IPV4;
        memset(&im->v.ipprefix.addr, 0, sizeof(im->v.ipprefix.addr));
        memset(&im->v.ipprefix.mask, 0, sizeof(im->v.ipprefix.mask));
        memcpy(&im->v.ipprefix.addr.ip4, "\x0a\x01\x00\x00", 4);
        memcpy(&im->v.ipprefix.mask.ip4, "\xff\xff\x00\x00", 4);
        im->member_size = sizeof(im->v.ipprefix);
    } else if (!strcmp(name, "ipprefix6")) {
        im->v.ipprefix.addr_family = SAI_IP_ADDR_FAMILY_IPV6;
        memcpy(im->v.ipprefix.addr.ip6, IP6, 16);
        memset(im->v.ipprefix.addr.ip6 + 4, 0, 12); /* the address under the mask */
        memcpy(im->v.ipprefix.mask.ip6, MASK6, 16);
        im->member_size = sizeof(im->v.ipprefix);
    } else if (!strcmp(name, "timespec")) {
        im->v.timespec.tv_sec = 1ULL << 40;
        im->v.timespec.tv_nsec = 999999999;
        im->member_size = sizeof(im->v.timespec);
    } else if (!strcmp(name, "objlist")) LIST(objlist, sai_object_id_t, 0x2100000000000010ULL, 0x20, 0x0102030405060708ULL);
    else if (!strcmp(name, "u8list")) LIST(u8list, uint8_t, 1, 2, 0xfe);
    else if (!strcmp(name, "s8list")) LIST(s8list, int8_t, -1, 2, -3);
    else if (!strcmp(name, "u16list")) LIST(u16list, uint16_t, 0x0102, 0x0304, 0x0506);
    else if (!strcmp(name, "s16list")) LIST(s16list, int16_t, -1, 2, -3);
    else if (!strcmp(name, "u32list")) LIST(u32list, uint32_t, 0x01020304, 5, 6);
    else if (!strcmp(name, "s32list")) LIST(s32list, int32_t, -1, 0x00010203, -3);
    else if (!strcmp(name, "vlanlist")) LIST(vlanlist, sai_vlan_id_t, 1, 100, 4094);
    else if (!strcmp(name, "maplist")) {
        sai_map_t items[2] = {{.key = 1, .value = -1}, {.key = 0x01020304, .value = 20}};
        memcpy(im->elems, items, sizeof(items));
        im->elem_bytes = sizeof(items);
        im->v.maplist.count = 2;
        im->v.maplist.list = (sai_map_t*)addr;
        im->member_size = sizeof(im->v.maplist);
    } else if (!strcmp(name, "aclresource")) {
        sai_acl_resource_t items[1] = {{.stage = SAI_ACL_STAGE_EGRESS, .bind_point = SAI_ACL_BIND_POINT_TYPE_VLAN, .avail_num = 300}};
        memcpy(im->elems, items, sizeof(items));
        im->elem_bytes = sizeof(items);
        im->v.aclresource.count = 1;
        im->v.aclresource.list = (sai_acl_resource_t*)addr;
        im->member_size = sizeof(im->v.aclresource);
    } else if (!strcmp(name, "qosmap")) {
        sai_qos_map_t items[2];
        memset(items, GARBAGE, sizeof(items));
        for (int i = 0; i < 2; i++) {
            qos(&items[i].key, i);
            qos(&items[i].value, 20 + i);
        }
        memcpy(im->elems, items, sizeof(items));
        im->elem_bytes = sizeof(items);
        im->v.qosmap.count = 2;
        im->v.qosmap.list = (sai_qos_map_t*)addr;
        im->member_size = sizeof(im->v.qosmap);
    } else if (!strcmp(name, "aclcapability")) {
        const int32_t items[] = {3, 0x00010203};
        memcpy(im->elems, items, sizeof(items));
        im->elem_bytes = sizeof(items);
        im->v.aclcapability.is_action_list_mandatory = true;
        im->v.aclcapability.action_list.count = 2;
        im->v.aclcapability.action_list.list = (int32_t*)addr;
        im->v.aclcapability.supported_match_type = SAI_ACL_TABLE_SUPPORTED_MATCH_TYPE_EXACT;
        im->v.aclcapability.is_non_contiguous_bits_exact_match_supported = false;
        im->member_size = sizeof(im->v.aclcapability);
    } else if (!strncmp(name, "aclfield_", 9) || !strncmp(name, "aclaction_", 10)) {
        return make_acl(name, im, addr);
    } else
        return 0;
    return 1;
}

/* A value as a caller prepares it before a get. */
static int request(const char* name, image_t* im, uintptr_t addr, uint32_t capacity)
{
    if (!make(name, im, addr)) return 0;
    size_t member_size = im->member_size;
    int is_list = im->elem_bytes != 0;
    memset(&im->v, GARBAGE, sizeof(im->v));
    im->elem_bytes = 0;
    im->member_size = member_size;
    if (!is_list) return 1;
    /* every list member is { uint32_t count; T* list; }: set through the member itself */
#define PREPARE(member)                          \
    if (!strcmp(name, #member)) {                \
        im->v.member.count = capacity;           \
        im->v.member.list = (void*)addr;         \
    }
    PREPARE(objlist) PREPARE(u8list) PREPARE(s8list) PREPARE(u16list) PREPARE(s16list) PREPARE(u32list) PREPARE(s32list)
    PREPARE(vlanlist) PREPARE(maplist) PREPARE(aclresource) PREPARE(qosmap)
    if (!strcmp(name, "aclcapability")) {
        im->v.aclcapability.action_list.count = capacity;
        im->v.aclcapability.action_list.list = (int32_t*)addr;
    }
    if (!strcmp(name, "aclfield_objlist")) {
        im->v.aclfield.data.objlist.count = capacity;
        im->v.aclfield.data.objlist.list = (sai_object_id_t*)addr;
    }
    if (!strcmp(name, "aclfield_u8list")) {
        im->v.aclfield.data.u8list.count = capacity;
        im->v.aclfield.data.u8list.list = (uint8_t*)addr;
        im->v.aclfield.mask.u8list.count = capacity;
        im->v.aclfield.mask.u8list.list = (uint8_t*)(addr + 16);
    }
    if (!strcmp(name, "aclaction_objlist")) {
        im->v.aclaction.parameter.objlist.count = capacity;
        im->v.aclaction.parameter.objlist.list = (sai_object_id_t*)addr;
    }
    return 1;
}

#define SAME(field)                                                           \
    do {                                                                      \
        if (memcmp(&got->v.field, &want.v.field, sizeof(want.v.field))) return #field; \
    } while (0)

/* NULL when got holds the case's value; else what differs. Structs with padding are compared
 * field by field: padding bytes are nobody's. */
static const char* compare(const char* name, const image_t* got, uintptr_t addr)
{
    image_t want;
    if (!make(name, &want, addr)) return "no such case";
    const unsigned char* bytes = (const unsigned char*)&got->v;
    for (size_t i = want.member_size; i < sizeof(got->v); i++)
        if (bytes[i] != GARBAGE) return "a byte outside the member changed";
    if (!strcmp(name, "timespec")) {
        SAME(timespec.tv_sec);
        SAME(timespec.tv_nsec);
        return NULL;
    }
    if (!strncmp(name, "aclfield_", 9)) {
        /* enable and the members the case names; the rest of the struct is the caller's */
        SAME(aclfield.enable);
        const char* m = name + 9;
        if (!strcmp(m, "bool")) SAME(aclfield.data.booldata);
        else if (!strcmp(m, "u8")) { SAME(aclfield.data.u8); SAME(aclfield.mask.u8); }
        else if (!strcmp(m, "u16")) { SAME(aclfield.data.u16); SAME(aclfield.mask.u16); }
        else if (!strcmp(m, "u32")) { SAME(aclfield.data.u32); SAME(aclfield.mask.u32); }
        else if (!strcmp(m, "u64")) { SAME(aclfield.data.u64); SAME(aclfield.mask.u64); }
        else if (!strcmp(m, "enum")) { SAME(aclfield.data.s32); SAME(aclfield.mask.s32); }
        else if (!strcmp(m, "mac")) { SAME(aclfield.data.mac); SAME(aclfield.mask.mac); }
        else if (!strcmp(m, "ip4")) { SAME(aclfield.data.ip4); SAME(aclfield.mask.ip4); }
        else if (!strcmp(m, "ip6")) { SAME(aclfield.data.ip6); SAME(aclfield.mask.ip6); }
        else if (!strcmp(m, "oid")) SAME(aclfield.data.oid);
        else if (!strcmp(m, "objlist")) { SAME(aclfield.data.objlist.count); SAME(aclfield.data.objlist.list); }
        else if (!strcmp(m, "u8list")) {
            SAME(aclfield.data.u8list.count);
            SAME(aclfield.data.u8list.list);
            SAME(aclfield.mask.u8list.count);
            SAME(aclfield.mask.u8list.list);
        }
        if (got->elem_bytes != want.elem_bytes) return "element bytes";
        if (memcmp(got->elems, want.elems, want.elem_bytes)) return "elements";
        return NULL;
    }
    if (!strncmp(name, "aclaction_", 10)) {
        SAME(aclaction.enable);
        const char* m = name + 10;
        if (!strcmp(m, "bool")) SAME(aclaction.parameter.booldata);
        else if (!strcmp(m, "u8")) SAME(aclaction.parameter.u8);
        else if (!strcmp(m, "u16")) SAME(aclaction.parameter.u16);
        else if (!strcmp(m, "u32")) SAME(aclaction.parameter.u32);
        else if (!strcmp(m, "s32") || !strcmp(m, "enum")) SAME(aclaction.parameter.s32);
        else if (!strcmp(m, "mac")) SAME(aclaction.parameter.mac);
        else if (!strcmp(m, "ip4")) SAME(aclaction.parameter.ip4);
        else if (!strcmp(m, "ip6")) SAME(aclaction.parameter.ip6);
        else if (!strcmp(m, "oid")) SAME(aclaction.parameter.oid);
        else if (!strcmp(m, "objlist")) { SAME(aclaction.parameter.objlist.count); SAME(aclaction.parameter.objlist.list); }
        else if (!strcmp(m, "ipaddr6")) { SAME(aclaction.parameter.ipaddr.addr_family); SAME(aclaction.parameter.ipaddr.addr.ip6); }
        if (got->elem_bytes != want.elem_bytes) return "element bytes";
        if (memcmp(got->elems, want.elems, want.elem_bytes)) return "elements";
        return NULL;
    }
    if (!strcmp(name, "aclcapability")) {
        SAME(aclcapability.is_action_list_mandatory);
        SAME(aclcapability.action_list.count);
        SAME(aclcapability.action_list.list);
        SAME(aclcapability.supported_match_type);
        SAME(aclcapability.is_non_contiguous_bits_exact_match_supported);
    } else if (want.elem_bytes == 0) {
        /* scalars, byte arrays and structs without padding: the member's bytes */
        if (memcmp(&got->v, &want.v, want.member_size)) return "the member's bytes";
        return NULL;
    } else {
        /* a list header: count and pointer (padding lies between them) */
        if (memcmp(&got->v.u32list.count, &want.v.u32list.count, sizeof(uint32_t))) return "list count";
        if (got->v.u32list.list != want.v.u32list.list) return "list pointer";
    }
    if (got->elem_bytes != want.elem_bytes) return "element bytes";
    if (!strcmp(name, "qosmap")) {
        sai_qos_map_t g[2], w[2];
        memcpy(g, got->elems, sizeof(g));
        memcpy(w, want.elems, sizeof(w));
        for (int i = 0; i < 2; i++) {
            const sai_qos_map_params_t* gp[2] = {&g[i].key, &g[i].value};
            const sai_qos_map_params_t* wp[2] = {&w[i].key, &w[i].value};
            for (int k = 0; k < 2; k++) {
                if (gp[k]->tc != wp[k]->tc || gp[k]->dscp != wp[k]->dscp || gp[k]->dot1p != wp[k]->dot1p || gp[k]->prio != wp[k]->prio ||
                    gp[k]->pg != wp[k]->pg || gp[k]->queue_index != wp[k]->queue_index || gp[k]->color != wp[k]->color ||
                    gp[k]->mpls_exp != wp[k]->mpls_exp || gp[k]->fc != wp[k]->fc || gp[k]->dei != wp[k]->dei || gp[k]->vc != wp[k]->vc)
                    return "qos map element";
            }
        }
        return NULL;
    }
    if (memcmp(got->elems, want.elems, want.elem_bytes)) return "elements";
    return NULL;
}

static int hexval(int c)
{
    if (c >= '0' && c <= '9') return c - '0';
    if (c >= 'a' && c <= 'f') return c - 'a' + 10;
    return -1;
}
/* bytes written, or (size_t)-1 */
static size_t unhex(const char* s, unsigned char* out, size_t max)
{
    size_t n = 0;
    if (!strcmp(s, "-")) return 0;
    while (s[0] && s[1]) {
        int hi = hexval(s[0]), lo = hexval(s[1]);
        if (hi < 0 || lo < 0 || n >= max) return (size_t)-1;
        out[n++] = (unsigned char)(hi << 4 | lo);
        s += 2;
    }
    return s[0] ? (size_t)-1 : n;
}
static void puthex(const unsigned char* p, size_t n)
{
    if (n == 0) putchar('-');
    for (size_t i = 0; i < n; i++) printf("%02x", p[i]);
}

int main(void)
{
    static char line[8192], name[64], value_hex[4096], elem_hex[4096];
    while (fgets(line, sizeof(line), stdin)) {
        unsigned long long addr = 0;
        unsigned capacity = 0;
        image_t im;
        if (line[0] == 'S') {
            printf("%zu %zu\n", sizeof(sai_attribute_value_t), sizeof(void*));
        } else if (sscanf(line, "R %63s %llu %u", name, &addr, &capacity) == 3) {
            if (!request(name, &im, (uintptr_t)addr, capacity)) {
                printf("FAIL no such case\n");
            } else {
                puthex((const unsigned char*)&im.v, sizeof(im.v));
                putchar('\n');
            }
        } else if (sscanf(line, "M %63s %llu", name, &addr) == 2) {
            if (!make(name, &im, (uintptr_t)addr)) {
                printf("FAIL no such case\n");
            } else {
                puthex((const unsigned char*)&im.v, sizeof(im.v));
                putchar(' ');
                puthex(im.elems, im.elem_bytes);
                putchar('\n');
            }
        } else if (sscanf(line, "E %63s %llu %4095s %4095s", name, &addr, value_hex, elem_hex) == 4) {
            memset(&im, 0, sizeof(im));
            size_t n = unhex(value_hex, (unsigned char*)&im.v, sizeof(im.v));
            im.elem_bytes = unhex(elem_hex, im.elems, sizeof(im.elems));
            const char* why = n != sizeof(im.v) || im.elem_bytes == (size_t)-1 ? "bad request" : compare(name, &im, (uintptr_t)addr);
            if (why) printf("FAIL %s\n", why);
            else printf("ok\n");
        } else {
            printf("FAIL bad request\n");
        }
        fflush(stdout);
    }
    return 0;
}
