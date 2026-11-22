/* C harness for libsai.so of the Rust SAI: loads the built library with dlopen and drives
 * the slice through the real C ABI with the real SAI headers (structs, unions and enums as
 * the C compiler lays them out). Built and run by tests/c_abi.rs.
 *   harness <path to libsai.so>
 * Prints "HARNESS_OK <checks>" and exits 0 when every check passed. */
#define _GNU_SOURCE
#include <dlfcn.h>
#include <pthread.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

#include <sai.h>

static int checks, failures;
#define CHECK(c)                                                         \
    do {                                                                 \
        checks++;                                                        \
        if (!(c)) {                                                      \
            fprintf(stderr, "FAIL %s:%d: %s\n", __FILE__, __LINE__, #c); \
            failures++;                                                  \
        }                                                                \
    } while (0)
#define CHECK_STATUS(expr, want)                                                                        \
    do {                                                                                                \
        sai_status_t s_ = (expr);                                                                       \
        checks++;                                                                                       \
        if (s_ != (want)) {                                                                             \
            fprintf(stderr, "FAIL %s:%d: %s = %d, expected %s (%d)\n", __FILE__, __LINE__, #expr, (int)s_, #want, (int)(want)); \
            failures++;                                                                                 \
        }                                                                                               \
    } while (0)

/* ---- what a SAI user provides: profile values and a notification callback ---- */
static int offer_backend = 1;
static const char* profile_get_value(sai_switch_profile_id_t profile_id, const char* variable)
{
    (void)profile_id;
    if (!strcmp(variable, "SAI_RS_BACKEND")) return offer_backend ? "virtual" : NULL;
    if (!strcmp(variable, "SAI_RS_VIRTUAL_PORTS")) return "8";
    if (!strcmp(variable, "SAI_RS_VIRTUAL_LANES")) return "2";
    if (!strcmp(variable, "SAI_RS_VIRTUAL_SPEED")) return "50000";
    return NULL;
}
static int profile_get_next_value(sai_switch_profile_id_t profile_id, const char** variable, const char** value)
{
    (void)profile_id; (void)variable; (void)value;
    return -1;
}

static pthread_mutex_t ev_lock = PTHREAD_MUTEX_INITIALIZER;
static struct { sai_object_id_t port; sai_port_oper_status_t state; } events[64];
static int nevents;
static sai_switch_api_t* switch_api;
static sai_object_id_t the_switch;
static int reentrant_ok;

static void on_port_state(uint32_t count, const sai_port_oper_status_notification_t* data)
{
    /* The library holds no lock while it calls us: a call back into it must work. */
    sai_attribute_t a = {.id = SAI_SWITCH_ATTR_PORT_NUMBER};
    int ok = switch_api && switch_api->get_switch_attribute(the_switch, 1, &a) == SAI_STATUS_SUCCESS && a.value.u32 == 8;
    pthread_mutex_lock(&ev_lock);
    reentrant_ok += ok;
    for (uint32_t i = 0; i < count && nevents < 64; i++) {
        events[nevents].port = data[i].port_id;
        events[nevents].state = data[i].port_state;
        nevents++;
    }
    pthread_mutex_unlock(&ev_lock);
}

static int wait_event(sai_object_id_t port, sai_port_oper_status_t state)
{
    for (int tries = 0; tries < 2000; tries++) {
        int found = 0;
        pthread_mutex_lock(&ev_lock);
        for (int i = 0; i < nevents; i++)
            if (events[i].port == port && events[i].state == state) found = 1;
        pthread_mutex_unlock(&ev_lock);
        if (found) return 1;
        struct timespec ts = {0, 2000000};
        nanosleep(&ts, NULL);
    }
    return 0;
}

#define SYM(name)                                             \
    __typeof__(name)* p_##name = (__typeof__(name)*)dlsym(lib, #name); \
    if (!p_##name) {                                          \
        fprintf(stderr, "missing symbol %s\n", #name);        \
        return 2;                                             \
    }

int main(int argc, char** argv)
{
    if (argc != 2) {
        fprintf(stderr, "usage: harness <libsai.so>\n");
        return 2;
    }
    void* lib = dlopen(argv[1], RTLD_NOW | RTLD_LOCAL);
    if (!lib) {
        fprintf(stderr, "dlopen: %s\n", dlerror());
        return 2;
    }
    SYM(sai_api_initialize)
    SYM(sai_api_uninitialize)
    SYM(sai_api_query)
    SYM(sai_log_set)
    SYM(sai_object_type_query)
    SYM(sai_switch_id_query)
    SYM(sai_query_api_version)
    SYM(sai_query_attribute_capability)
    SYM(sai_object_type_get_availability)
    SYM(sai_get_object_count)

    const sai_service_method_table_t services = {profile_get_value, profile_get_next_value};
    void* table = NULL;
    sai_api_version_t version = 0;

    /* ---- before initialisation ---- */
    CHECK_STATUS(p_sai_api_query(SAI_API_SWITCH, &table), SAI_STATUS_UNINITIALIZED);
    CHECK_STATUS(p_sai_api_uninitialize(), SAI_STATUS_UNINITIALIZED);
    CHECK_STATUS(p_sai_query_api_version(&version), SAI_STATUS_SUCCESS);
    CHECK(version == SAI_API_VERSION);
    CHECK_STATUS(p_sai_query_api_version(NULL), SAI_STATUS_INVALID_PARAMETER);
    CHECK_STATUS(p_sai_log_set(SAI_API_PORT, SAI_LOG_LEVEL_DEBUG), SAI_STATUS_SUCCESS);
    CHECK_STATUS(p_sai_log_set(SAI_API_PORT, SAI_LOG_LEVEL_WARN), SAI_STATUS_SUCCESS);
    CHECK_STATUS(p_sai_log_set((sai_api_t)1000, SAI_LOG_LEVEL_WARN), SAI_STATUS_INVALID_PARAMETER);

    /* ---- stage 1 ---- */
    CHECK_STATUS(p_sai_api_initialize(0, NULL), SAI_STATUS_INVALID_PARAMETER);
    CHECK_STATUS(p_sai_api_initialize(0, &services), SAI_STATUS_SUCCESS);
    CHECK_STATUS(p_sai_api_initialize(0, &services), SAI_STATUS_FAILURE);
    CHECK_STATUS(p_sai_api_query(SAI_API_SWITCH, NULL), SAI_STATUS_INVALID_PARAMETER);
    CHECK_STATUS(p_sai_api_query(SAI_API_NEXT_HOP, &table), SAI_STATUS_NOT_IMPLEMENTED);
    CHECK_STATUS(p_sai_api_query((sai_api_t)1000, &table), SAI_STATUS_INVALID_PARAMETER);
    CHECK(table == NULL);
    sai_port_api_t* port_api = NULL;
    sai_vlan_api_t* vlan_api = NULL;
    sai_bridge_api_t* bridge_api = NULL;
    CHECK_STATUS(p_sai_api_query(SAI_API_SWITCH, (void**)&switch_api), SAI_STATUS_SUCCESS);
    CHECK_STATUS(p_sai_api_query(SAI_API_PORT, (void**)&port_api), SAI_STATUS_SUCCESS);
    CHECK_STATUS(p_sai_api_query(SAI_API_VLAN, (void**)&vlan_api), SAI_STATUS_SUCCESS);
    CHECK_STATUS(p_sai_api_query(SAI_API_BRIDGE, (void**)&bridge_api), SAI_STATUS_SUCCESS);
    if (!switch_api || !port_api || !vlan_api || !bridge_api) return 1;
    CHECK(switch_api->create_switch && switch_api->remove_switch && switch_api->set_switch_attribute && switch_api->get_switch_attribute);
    CHECK(port_api->get_port_attribute && port_api->set_port_attribute && vlan_api->create_vlan && vlan_api->create_vlan_member);
    CHECK(bridge_api->create_bridge_port && bridge_api->get_bridge_port_attribute);
    /* not in this slice: null entries */
    CHECK(!port_api->get_port_stats && !vlan_api->create_vlan_members && !switch_api->switch_mdio_read);

    /* ---- stage 2: the switch ---- */
    sai_attribute_t sw_attrs[3];
    memset(sw_attrs, 0xa5, sizeof(sw_attrs)); /* unset union bytes are garbage, as on a C stack */
    sw_attrs[0].id = SAI_SWITCH_ATTR_INIT_SWITCH;
    sw_attrs[0].value.booldata = true;
    sw_attrs[1].id = SAI_SWITCH_ATTR_PORT_STATE_CHANGE_NOTIFY;
    sw_attrs[1].value.ptr = (sai_pointer_t)on_port_state;
    sw_attrs[2].id = SAI_SWITCH_ATTR_SRC_MAC_ADDRESS;
    memcpy(sw_attrs[2].value.mac, "\x02\x00\x00\xaa\xbb\xcc", 6);

    sai_object_id_t tmp = 0x1234;
    CHECK_STATUS(switch_api->create_switch(NULL, 3, sw_attrs), SAI_STATUS_INVALID_PARAMETER);
    CHECK_STATUS(switch_api->create_switch(&tmp, 3, NULL), SAI_STATUS_INVALID_PARAMETER);
    CHECK_STATUS(switch_api->create_switch(&tmp, 0, NULL), SAI_STATUS_MANDATORY_ATTRIBUTE_MISSING);
    {
        sai_attribute_t bad[2] = {sw_attrs[0], sw_attrs[0]};
        CHECK_STATUS(switch_api->create_switch(&tmp, 2, bad), SAI_STATUS_INVALID_PARAMETER); /* twice */
        bad[1].id = SAI_SWITCH_ATTR_PORT_NUMBER; /* read-only */
        CHECK_STATUS(switch_api->create_switch(&tmp, 2, bad), SAI_STATUS_INVALID_ATTRIBUTE_0 + SAI_STATUS_CODE(1));
        bad[1].id = SAI_SWITCH_ATTR_FDB_AGING_TIME; /* valid SAI, not in the slice */
        bad[1].value.u32 = 600;
        CHECK_STATUS(switch_api->create_switch(&tmp, 2, bad), SAI_STATUS_ATTR_NOT_SUPPORTED_0 + SAI_STATUS_CODE(1));
        bad[1].id = SAI_SWITCH_ATTR_ACL_ENTRY_MINIMUM_PRIORITY + 100000; /* nothing */
        CHECK_STATUS(switch_api->create_switch(&tmp, 2, bad), SAI_STATUS_UNKNOWN_ATTRIBUTE_0 + SAI_STATUS_CODE(1));
    }
    offer_backend = 0; /* the virtual backend must be asked for in the profile */
    CHECK_STATUS(switch_api->create_switch(&tmp, 3, sw_attrs), SAI_STATUS_FAILURE);
    offer_backend = 1;
    CHECK(tmp == 0x1234); /* no failed call wrote its output */
    CHECK_STATUS(switch_api->create_switch(&the_switch, 3, sw_attrs), SAI_STATUS_SUCCESS);
    CHECK_STATUS(switch_api->create_switch(&tmp, 3, sw_attrs), SAI_STATUS_ITEM_ALREADY_EXISTS);
    CHECK(p_sai_object_type_query(the_switch) == SAI_OBJECT_TYPE_SWITCH);
    CHECK(p_sai_switch_id_query(the_switch) == the_switch);
    CHECK(p_sai_object_type_query(SAI_NULL_OBJECT_ID) == SAI_OBJECT_TYPE_NULL);
    CHECK(p_sai_object_type_query(0xffffffffffffffffULL) == SAI_OBJECT_TYPE_NULL);
    CHECK(p_sai_switch_id_query(0xffffffffffffffffULL) == SAI_NULL_OBJECT_ID);

    /* scalars, several at once, in the order asked */
    {
        sai_attribute_t g[4] = {{.id = SAI_SWITCH_ATTR_PORT_NUMBER}, {.id = SAI_SWITCH_ATTR_SRC_MAC_ADDRESS},
                                {.id = SAI_SWITCH_ATTR_CPU_PORT}, {.id = SAI_SWITCH_ATTR_PORT_STATE_CHANGE_NOTIFY}};
        CHECK_STATUS(switch_api->get_switch_attribute(the_switch, 4, g), SAI_STATUS_SUCCESS);
        CHECK(g[0].value.u32 == 8 && !memcmp(g[1].value.mac, "\x02\x00\x00\xaa\xbb\xcc", 6));
        CHECK(p_sai_object_type_query(g[2].value.oid) == SAI_OBJECT_TYPE_PORT);
        CHECK(g[3].value.ptr == (sai_pointer_t)on_port_state);
        /* position of the failing attribute in the status */
        g[2].id = SAI_SWITCH_ATTR_DEFAULT_VIRTUAL_ROUTER_ID;
        CHECK_STATUS(switch_api->get_switch_attribute(the_switch, 4, g), SAI_STATUS_ATTR_NOT_SUPPORTED_0 + SAI_STATUS_CODE(2));
        CHECK_STATUS(switch_api->get_switch_attribute(the_switch, 0, NULL), SAI_STATUS_SUCCESS);
        CHECK_STATUS(switch_api->get_switch_attribute(the_switch, 4, NULL), SAI_STATUS_INVALID_PARAMETER);
        CHECK_STATUS(switch_api->get_switch_attribute(SAI_NULL_OBJECT_ID, 1, g), SAI_STATUS_INVALID_OBJECT_ID);
    }

    /* lists: size query, short buffer, exact buffer, roomy buffer */
    sai_object_id_t ports[16];
    {
        sai_attribute_t g = {.id = SAI_SWITCH_ATTR_PORT_LIST};
        g.value.objlist.count = 0;
        g.value.objlist.list = NULL;
        CHECK_STATUS(switch_api->get_switch_attribute(the_switch, 1, &g), SAI_STATUS_BUFFER_OVERFLOW);
        CHECK(g.value.objlist.count == 8);
        memset(ports, 0xee, sizeof(ports));
        g.value.objlist.count = 7;
        g.value.objlist.list = ports;
        CHECK_STATUS(switch_api->get_switch_attribute(the_switch, 1, &g), SAI_STATUS_BUFFER_OVERFLOW);
        CHECK(g.value.objlist.count == 8 && ports[0] == 0xeeeeeeeeeeeeeeeeULL); /* nothing written */
        g.value.objlist.count = 8;
        CHECK_STATUS(switch_api->get_switch_attribute(the_switch, 1, &g), SAI_STATUS_SUCCESS);
        CHECK(g.value.objlist.count == 8 && ports[7] != 0xeeeeeeeeeeeeeeeeULL && ports[8] == 0xeeeeeeeeeeeeeeeeULL);
        g.value.objlist.count = 16;
        CHECK_STATUS(switch_api->get_switch_attribute(the_switch, 1, &g), SAI_STATUS_SUCCESS);
        CHECK(g.value.objlist.count == 8);
        for (int i = 0; i < 8; i++) {
            CHECK(p_sai_object_type_query(ports[i]) == SAI_OBJECT_TYPE_PORT && p_sai_switch_id_query(ports[i]) == the_switch);
        }
    }

    /* ports */
    {
        uint32_t lanes[8];
        sai_attribute_t g[3] = {{.id = SAI_PORT_ATTR_HW_LANE_LIST}, {.id = SAI_PORT_ATTR_SPEED}, {.id = SAI_PORT_ATTR_ADMIN_STATE}};
        g[0].value.u32list.count = 8;
        g[0].value.u32list.list = lanes;
        g[2].value.u64 = ~0ULL;
        CHECK_STATUS(port_api->get_port_attribute(ports[3], 3, g), SAI_STATUS_SUCCESS);
        CHECK(g[0].value.u32list.count == 2 && lanes[0] == 7 && lanes[1] == 8 && g[1].value.u32 == 50000);
        CHECK(g[2].value.booldata == false); /* ports come up disabled */
        /* wrong object types */
        CHECK_STATUS(port_api->get_port_attribute(the_switch, 1, &g[1]), SAI_STATUS_INVALID_OBJECT_TYPE);
        CHECK_STATUS(switch_api->get_switch_attribute(ports[0], 1, &g[1]), SAI_STATUS_INVALID_OBJECT_TYPE);
        CHECK_STATUS(port_api->remove_port(ports[0]), SAI_STATUS_NOT_SUPPORTED);

        sai_attribute_t s = {.id = SAI_PORT_ATTR_SPEED, .value.u32 = 12345};
        CHECK_STATUS(port_api->set_port_attribute(ports[3], NULL), SAI_STATUS_INVALID_PARAMETER);
        CHECK_STATUS(port_api->set_port_attribute(ports[3], &s), SAI_STATUS_INVALID_ATTR_VALUE_0);
        s.value.u32 = 25000;
        CHECK_STATUS(port_api->set_port_attribute(ports[3], &s), SAI_STATUS_SUCCESS);
        s.id = SAI_PORT_ATTR_OPER_STATUS; /* read-only */
        CHECK_STATUS(port_api->set_port_attribute(ports[3], &s), SAI_STATUS_INVALID_ATTRIBUTE_0);
        /* a create-only list in a set, its value garbage: refused without following the pointer */
        memset(&s, 0xa5, sizeof(s));
        s.id = SAI_PORT_ATTR_HW_LANE_LIST;
        CHECK_STATUS(port_api->set_port_attribute(ports[3], &s), SAI_STATUS_INVALID_ATTRIBUTE_0);

        /* admin up: link up, told through the callback (from the library's thread) */
        sai_attribute_t admin = {.id = SAI_PORT_ATTR_ADMIN_STATE, .value.booldata = true};
        CHECK_STATUS(port_api->set_port_attribute(ports[3], &admin), SAI_STATUS_SUCCESS);
        CHECK(wait_event(ports[3], SAI_PORT_OPER_STATUS_UP));
        sai_attribute_t oper = {.id = SAI_PORT_ATTR_OPER_STATUS};
        CHECK_STATUS(port_api->get_port_attribute(ports[3], 1, &oper), SAI_STATUS_SUCCESS);
        CHECK(oper.value.s32 == SAI_PORT_OPER_STATUS_UP);
        admin.value.booldata = false;
        CHECK_STATUS(port_api->set_port_attribute(ports[3], &admin), SAI_STATUS_SUCCESS);
        CHECK(wait_event(ports[3], SAI_PORT_OPER_STATUS_DOWN));
        pthread_mutex_lock(&ev_lock);
        CHECK(nevents == 2 && reentrant_ok == 2);
        pthread_mutex_unlock(&ev_lock);
    }

    /* VLANs and members */
    {
        sai_object_id_t vlan = 0, vlan2 = 0, member = 0, bridge, bps[8], default_vlan;
        sai_attribute_t g[2] = {{.id = SAI_SWITCH_ATTR_DEFAULT_1Q_BRIDGE_ID}, {.id = SAI_SWITCH_ATTR_DEFAULT_VLAN_ID}};
        CHECK_STATUS(switch_api->get_switch_attribute(the_switch, 2, g), SAI_STATUS_SUCCESS);
        bridge = g[0].value.oid;
        default_vlan = g[1].value.oid;
        sai_attribute_t l = {.id = SAI_BRIDGE_ATTR_PORT_LIST};
        l.value.objlist.count = 8;
        l.value.objlist.list = bps;
        CHECK_STATUS(bridge_api->get_bridge_attribute(bridge, 1, &l), SAI_STATUS_SUCCESS);
        CHECK(l.value.objlist.count == 8);
        sai_attribute_t bp = {.id = SAI_BRIDGE_PORT_ATTR_PORT_ID};
        CHECK_STATUS(bridge_api->get_bridge_port_attribute(bps[2], 1, &bp), SAI_STATUS_SUCCESS);
        CHECK(bp.value.oid == ports[2]);
        /* VLAN 1 has every port */
        sai_object_id_t defaults[8];
        l.id = SAI_VLAN_ATTR_MEMBER_LIST;
        l.value.objlist.count = 8;
        l.value.objlist.list = defaults;
        CHECK_STATUS(vlan_api->get_vlan_attribute(default_vlan, 1, &l), SAI_STATUS_SUCCESS);
        CHECK(l.value.objlist.count == 8);

        sai_attribute_t v = {.id = SAI_VLAN_ATTR_VLAN_ID, .value.u16 = 100};
        CHECK_STATUS(vlan_api->create_vlan(NULL, the_switch, 1, &v), SAI_STATUS_INVALID_PARAMETER);
        CHECK_STATUS(vlan_api->create_vlan(&vlan, the_switch, 1, NULL), SAI_STATUS_INVALID_PARAMETER);
        CHECK_STATUS(vlan_api->create_vlan(&vlan, the_switch, 0, NULL), SAI_STATUS_MANDATORY_ATTRIBUTE_MISSING);
        CHECK_STATUS(vlan_api->create_vlan(&vlan, ports[0], 1, &v), SAI_STATUS_INVALID_OBJECT_TYPE); /* not a switch id */
        v.value.u16 = 4095;
        CHECK_STATUS(vlan_api->create_vlan(&vlan, the_switch, 1, &v), SAI_STATUS_INVALID_ATTR_VALUE_0);
        v.value.u16 = 100;
        CHECK(vlan == 0);
        CHECK_STATUS(vlan_api->create_vlan(&vlan, the_switch, 1, &v), SAI_STATUS_SUCCESS);
        CHECK(p_sai_object_type_query(vlan) == SAI_OBJECT_TYPE_VLAN);
        CHECK_STATUS(vlan_api->create_vlan(&vlan2, the_switch, 1, &v), SAI_STATUS_ITEM_ALREADY_EXISTS);

        sai_attribute_t m[3] = {{.id = SAI_VLAN_MEMBER_ATTR_VLAN_ID, .value.oid = vlan},
                                {.id = SAI_VLAN_MEMBER_ATTR_BRIDGE_PORT_ID, .value.oid = ports[2]},
                                {.id = SAI_VLAN_MEMBER_ATTR_VLAN_TAGGING_MODE, .value.s32 = SAI_VLAN_TAGGING_MODE_TAGGED}};
        /* a port where a bridge port belongs; a null id; a missing mandatory attribute */
        CHECK_STATUS(vlan_api->create_vlan_member(&member, the_switch, 3, m), SAI_STATUS_INVALID_ATTR_VALUE_0 + SAI_STATUS_CODE(1));
        m[1].value.oid = SAI_NULL_OBJECT_ID;
        CHECK_STATUS(vlan_api->create_vlan_member(&member, the_switch, 3, m), SAI_STATUS_INVALID_ATTR_VALUE_0 + SAI_STATUS_CODE(1));
        CHECK_STATUS(vlan_api->create_vlan_member(&member, the_switch, 1, m), SAI_STATUS_MANDATORY_ATTRIBUTE_MISSING);
        m[1].value.oid = bps[2];
        CHECK_STATUS(vlan_api->create_vlan_member(&member, the_switch, 3, m), SAI_STATUS_SUCCESS);
        CHECK_STATUS(vlan_api->create_vlan_member(&tmp, the_switch, 3, m), SAI_STATUS_ITEM_ALREADY_EXISTS);
        sai_attribute_t mg[3] = {{.id = SAI_VLAN_MEMBER_ATTR_VLAN_TAGGING_MODE}, {.id = SAI_VLAN_MEMBER_ATTR_VLAN_ID}, {.id = SAI_VLAN_MEMBER_ATTR_BRIDGE_PORT_ID}};
        CHECK_STATUS(vlan_api->get_vlan_member_attribute(member, 3, mg), SAI_STATUS_SUCCESS);
        CHECK(mg[0].value.s32 == SAI_VLAN_TAGGING_MODE_TAGGED && mg[1].value.oid == vlan && mg[2].value.oid == bps[2]);
        sai_attribute_t tag = {.id = SAI_VLAN_MEMBER_ATTR_VLAN_TAGGING_MODE, .value.s32 = SAI_VLAN_TAGGING_MODE_UNTAGGED};
        CHECK_STATUS(vlan_api->set_vlan_member_attribute(member, &tag), SAI_STATUS_SUCCESS);
        tag.value.s32 = SAI_VLAN_TAGGING_MODE_PRIORITY_TAGGED;
        CHECK_STATUS(vlan_api->set_vlan_member_attribute(member, &tag), SAI_STATUS_ATTR_NOT_SUPPORTED_0);

        /* lifecycle: in use, wrong type, stale */
        CHECK_STATUS(vlan_api->remove_vlan(vlan), SAI_STATUS_OBJECT_IN_USE);
        CHECK_STATUS(bridge_api->remove_bridge_port(bps[2]), SAI_STATUS_OBJECT_IN_USE);
        CHECK_STATUS(vlan_api->remove_vlan(member), SAI_STATUS_INVALID_OBJECT_TYPE);
        CHECK_STATUS(vlan_api->remove_vlan_member(vlan), SAI_STATUS_INVALID_OBJECT_TYPE);
        CHECK_STATUS(vlan_api->remove_vlan_member(member), SAI_STATUS_SUCCESS);
        CHECK_STATUS(vlan_api->remove_vlan_member(member), SAI_STATUS_INVALID_OBJECT_ID);
        CHECK_STATUS(vlan_api->get_vlan_member_attribute(member, 3, mg), SAI_STATUS_INVALID_OBJECT_ID);
        CHECK_STATUS(vlan_api->remove_vlan(vlan), SAI_STATUS_SUCCESS);
        CHECK_STATUS(vlan_api->remove_vlan(vlan), SAI_STATUS_INVALID_OBJECT_ID);
        CHECK_STATUS(vlan_api->create_vlan(&vlan2, the_switch, 1, &v), SAI_STATUS_SUCCESS);
        CHECK(vlan2 != vlan); /* a removed object's id is not given out again */

        /* the first thing orchagent does: default members and bridge ports go, bridge ports come back */
        for (int i = 0; i < 8; i++) CHECK_STATUS(vlan_api->remove_vlan_member(defaults[i]), SAI_STATUS_SUCCESS);
        for (int i = 0; i < 8; i++) CHECK_STATUS(bridge_api->remove_bridge_port(bps[i]), SAI_STATUS_SUCCESS);
        sai_attribute_t nb[3] = {{.id = SAI_BRIDGE_PORT_ATTR_TYPE, .value.s32 = SAI_BRIDGE_PORT_TYPE_PORT},
                                 {.id = SAI_BRIDGE_PORT_ATTR_PORT_ID, .value.oid = ports[5]},
                                 {.id = SAI_BRIDGE_PORT_ATTR_ADMIN_STATE, .value.booldata = true}};
        sai_object_id_t new_bp = 0;
        CHECK_STATUS(bridge_api->create_bridge_port(&new_bp, the_switch, 3, nb), SAI_STATUS_SUCCESS);
        CHECK(p_sai_object_type_query(new_bp) == SAI_OBJECT_TYPE_BRIDGE_PORT);
        m[0].value.oid = vlan2;
        m[1].value.oid = new_bp;
        CHECK_STATUS(vlan_api->create_vlan_member(&member, the_switch, 3, m), SAI_STATUS_SUCCESS);
    }

    /* global queries this slice answers "not implemented" without touching their pointers */
    CHECK_STATUS(p_sai_query_attribute_capability(the_switch, SAI_OBJECT_TYPE_PORT, SAI_PORT_ATTR_SPEED, NULL), SAI_STATUS_NOT_IMPLEMENTED);
    CHECK_STATUS(p_sai_object_type_get_availability(the_switch, SAI_OBJECT_TYPE_VLAN, 0, NULL, NULL), SAI_STATUS_NOT_IMPLEMENTED);
    CHECK_STATUS(p_sai_get_object_count(the_switch, SAI_OBJECT_TYPE_VLAN, NULL), SAI_STATUS_NOT_IMPLEMENTED);

    /* ---- teardown ---- */
    CHECK_STATUS(switch_api->remove_switch(ports[0]), SAI_STATUS_INVALID_OBJECT_TYPE);
    CHECK_STATUS(switch_api->remove_switch(the_switch), SAI_STATUS_SUCCESS);
    CHECK_STATUS(switch_api->remove_switch(the_switch), SAI_STATUS_UNINITIALIZED);
    {
        sai_attribute_t g = {.id = SAI_SWITCH_ATTR_PORT_NUMBER};
        CHECK_STATUS(switch_api->get_switch_attribute(the_switch, 1, &g), SAI_STATUS_UNINITIALIZED);
    }
    /* a second switch after the first, then uninitialize with it still up */
    CHECK_STATUS(switch_api->create_switch(&the_switch, 3, sw_attrs), SAI_STATUS_SUCCESS);
    CHECK_STATUS(p_sai_api_uninitialize(), SAI_STATUS_SUCCESS);
    CHECK_STATUS(p_sai_api_uninitialize(), SAI_STATUS_UNINITIALIZED);
    CHECK_STATUS(p_sai_api_query(SAI_API_SWITCH, &table), SAI_STATUS_UNINITIALIZED);

    dlclose(lib);
    if (failures) {
        fprintf(stderr, "%d of %d checks FAILED\n", failures, checks);
        return 1;
    }
    printf("HARNESS_OK %d\n", checks);
    return 0;
}
