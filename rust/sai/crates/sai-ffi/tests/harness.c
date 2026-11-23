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

/* A value as a C caller leaves it on its stack: garbage in every byte the member does not
 * cover. After a get, only the member's bytes may have changed: a library that writes
 * through the wrong union member (a u8 as a u32, say) fails here on a big-endian host. */
#define GARBAGE 0xa5
static void fill(sai_attribute_t* a, sai_attr_id_t id)
{
    memset(a, GARBAGE, sizeof(*a));
    a->id = id;
}
static int rest_untouched(const sai_attribute_t* a, size_t member_size)
{
    const unsigned char* bytes = (const unsigned char*)&a->value;
    for (size_t i = member_size; i < sizeof(a->value); i++)
        if (bytes[i] != GARBAGE) return 0;
    return 1;
}
#define MEMBER(a, m) rest_untouched(&(a), sizeof((a).value.m))

#define SYM(name)                                             \
    __typeof__(name)* p_##name = (__typeof__(name)*)dlsym(lib, #name); \
    if (!p_##name) {                                          \
        fprintf(stderr, "missing symbol %s\n", #name);        \
        return 2;                                             \
    }

/* Default objects, attribute-bag objects, entries, counters: the second slice. Every get
 * starts from a value full of garbage and checks the member the header names. */
static void second_slice(void* lib, const sai_object_id_t* ports)
{
    __typeof__(sai_api_query)* query = (__typeof__(sai_api_query)*)dlsym(lib, "sai_api_query");
    __typeof__(sai_object_type_query)* type_of = (__typeof__(sai_object_type_query)*)dlsym(lib, "sai_object_type_query");
    __typeof__(sai_query_attribute_enum_values_capability)* enum_caps =
        (__typeof__(sai_query_attribute_enum_values_capability)*)dlsym(lib, "sai_query_attribute_enum_values_capability");
    __typeof__(sai_query_stats_capability)* stats_caps = (__typeof__(sai_query_stats_capability)*)dlsym(lib, "sai_query_stats_capability");
    sai_port_api_t* port_api = NULL;
    sai_virtual_router_api_t* vr_api = NULL;
    sai_router_interface_api_t* rif_api = NULL;
    sai_next_hop_api_t* nh_api = NULL;
    sai_neighbor_api_t* neighbor_api = NULL;
    sai_route_api_t* route_api = NULL;
    sai_fdb_api_t* fdb_api = NULL;
    sai_hostif_api_t* hostif_api = NULL;
    sai_queue_api_t* queue_api = NULL;
    sai_buffer_api_t* buffer_api = NULL;
    sai_scheduler_api_t* scheduler_api = NULL;
    sai_scheduler_group_api_t* sg_api = NULL;
    sai_qos_map_api_t* qos_map_api = NULL;
    sai_policer_api_t* policer_api = NULL;
    sai_hash_api_t* hash_api = NULL;
    sai_stp_api_t* stp_api = NULL;
    sai_bridge_api_t* bridge_api = NULL;
    sai_lag_api_t* lag_api = NULL;
    if (!query || !type_of || !enum_caps || !stats_caps) { failures++; return; }
    CHECK_STATUS(query(SAI_API_PORT, (void**)&port_api), SAI_STATUS_SUCCESS);
    CHECK_STATUS(query(SAI_API_VIRTUAL_ROUTER, (void**)&vr_api), SAI_STATUS_SUCCESS);
    CHECK_STATUS(query(SAI_API_ROUTER_INTERFACE, (void**)&rif_api), SAI_STATUS_SUCCESS);
    CHECK_STATUS(query(SAI_API_NEXT_HOP, (void**)&nh_api), SAI_STATUS_SUCCESS);
    CHECK_STATUS(query(SAI_API_NEIGHBOR, (void**)&neighbor_api), SAI_STATUS_SUCCESS);
    CHECK_STATUS(query(SAI_API_ROUTE, (void**)&route_api), SAI_STATUS_SUCCESS);
    CHECK_STATUS(query(SAI_API_FDB, (void**)&fdb_api), SAI_STATUS_SUCCESS);
    CHECK_STATUS(query(SAI_API_HOSTIF, (void**)&hostif_api), SAI_STATUS_SUCCESS);
    CHECK_STATUS(query(SAI_API_QUEUE, (void**)&queue_api), SAI_STATUS_SUCCESS);
    CHECK_STATUS(query(SAI_API_BUFFER, (void**)&buffer_api), SAI_STATUS_SUCCESS);
    CHECK_STATUS(query(SAI_API_SCHEDULER, (void**)&scheduler_api), SAI_STATUS_SUCCESS);
    CHECK_STATUS(query(SAI_API_SCHEDULER_GROUP, (void**)&sg_api), SAI_STATUS_SUCCESS);
    CHECK_STATUS(query(SAI_API_QOS_MAP, (void**)&qos_map_api), SAI_STATUS_SUCCESS);
    CHECK_STATUS(query(SAI_API_POLICER, (void**)&policer_api), SAI_STATUS_SUCCESS);
    CHECK_STATUS(query(SAI_API_HASH, (void**)&hash_api), SAI_STATUS_SUCCESS);
    CHECK_STATUS(query(SAI_API_STP, (void**)&stp_api), SAI_STATUS_SUCCESS);
    CHECK_STATUS(query(SAI_API_BRIDGE, (void**)&bridge_api), SAI_STATUS_SUCCESS);
    CHECK_STATUS(query(SAI_API_LAG, (void**)&lag_api), SAI_STATUS_SUCCESS);
    if (!port_api || !vr_api || !rif_api || !nh_api || !neighbor_api || !route_api || !fdb_api || !hostif_api || !queue_api || !buffer_api ||
        !scheduler_api || !sg_api || !qos_map_api || !policer_api || !hash_api || !stp_api || !bridge_api || !lag_api) {
        failures++;
        return;
    }
    sai_attribute_t a;

    /* ---- switch: default objects (oid), u8, u32, u64, s8 list, the ACL capability struct ---- */
    sai_object_id_t vr, stp, trap_group, lag_hash, default_vlan, bridge;
    fill(&a, SAI_SWITCH_ATTR_DEFAULT_VIRTUAL_ROUTER_ID);
    CHECK_STATUS(switch_api->get_switch_attribute(the_switch, 1, &a), SAI_STATUS_SUCCESS);
    vr = a.value.oid;
    CHECK(type_of(vr) == SAI_OBJECT_TYPE_VIRTUAL_ROUTER && MEMBER(a, oid));
    fill(&a, SAI_SWITCH_ATTR_DEFAULT_STP_INST_ID);
    CHECK_STATUS(switch_api->get_switch_attribute(the_switch, 1, &a), SAI_STATUS_SUCCESS);
    stp = a.value.oid;
    CHECK(type_of(stp) == SAI_OBJECT_TYPE_STP);
    fill(&a, SAI_SWITCH_ATTR_DEFAULT_TRAP_GROUP);
    CHECK_STATUS(switch_api->get_switch_attribute(the_switch, 1, &a), SAI_STATUS_SUCCESS);
    trap_group = a.value.oid;
    CHECK(type_of(trap_group) == SAI_OBJECT_TYPE_HOSTIF_TRAP_GROUP);
    fill(&a, SAI_SWITCH_ATTR_LAG_HASH);
    CHECK_STATUS(switch_api->get_switch_attribute(the_switch, 1, &a), SAI_STATUS_SUCCESS);
    lag_hash = a.value.oid;
    CHECK(type_of(lag_hash) == SAI_OBJECT_TYPE_HASH);
    fill(&a, SAI_SWITCH_ATTR_DEFAULT_VLAN_ID);
    CHECK_STATUS(switch_api->get_switch_attribute(the_switch, 1, &a), SAI_STATUS_SUCCESS);
    default_vlan = a.value.oid;
    fill(&a, SAI_SWITCH_ATTR_DEFAULT_1Q_BRIDGE_ID);
    CHECK_STATUS(switch_api->get_switch_attribute(the_switch, 1, &a), SAI_STATUS_SUCCESS);
    bridge = a.value.oid;
    fill(&a, SAI_SWITCH_ATTR_QOS_MAX_NUMBER_OF_TRAFFIC_CLASSES);
    CHECK_STATUS(switch_api->get_switch_attribute(the_switch, 1, &a), SAI_STATUS_SUCCESS);
    CHECK(a.value.u8 == 8 && MEMBER(a, u8));
    fill(&a, SAI_SWITCH_ATTR_VXLAN_DEFAULT_PORT);
    CHECK_STATUS(switch_api->get_switch_attribute(the_switch, 1, &a), SAI_STATUS_SUCCESS);
    CHECK(a.value.u16 == 4789 && MEMBER(a, u16));
    fill(&a, SAI_SWITCH_ATTR_ACL_ENTRY_MAXIMUM_PRIORITY);
    CHECK_STATUS(switch_api->get_switch_attribute(the_switch, 1, &a), SAI_STATUS_SUCCESS);
    CHECK(a.value.u32 == 0x7fffffff && MEMBER(a, u32));
    fill(&a, SAI_SWITCH_ATTR_TOTAL_BUFFER_SIZE);
    CHECK_STATUS(switch_api->get_switch_attribute(the_switch, 1, &a), SAI_STATUS_SUCCESS);
    CHECK(a.value.u64 == 16384 && MEMBER(a, u64));
    fill(&a, SAI_SWITCH_ATTR_SWITCHING_MODE);
    CHECK_STATUS(switch_api->get_switch_attribute(the_switch, 1, &a), SAI_STATUS_SUCCESS);
    CHECK(a.value.s32 == SAI_SWITCH_SWITCHING_MODE_STORE_AND_FORWARD && MEMBER(a, s32));
    fill(&a, SAI_SWITCH_ATTR_UNINIT_DATA_PLANE_ON_REMOVAL);
    CHECK_STATUS(switch_api->get_switch_attribute(the_switch, 1, &a), SAI_STATUS_SUCCESS);
    CHECK(a.value.booldata == true && MEMBER(a, booldata));
    {
        int8_t info[4];
        fill(&a, SAI_SWITCH_ATTR_SWITCH_HARDWARE_INFO);
        a.value.s8list.count = 4;
        a.value.s8list.list = info;
        CHECK_STATUS(switch_api->get_switch_attribute(the_switch, 1, &a), SAI_STATUS_SUCCESS);
        CHECK(a.value.s8list.count == 0 && a.value.s8list.list == info && MEMBER(a, s8list));
        int32_t actions[4];
        fill(&a, SAI_SWITCH_ATTR_ACL_STAGE_INGRESS);
        a.value.aclcapability.action_list.count = 4;
        a.value.aclcapability.action_list.list = actions;
        CHECK_STATUS(switch_api->get_switch_attribute(the_switch, 1, &a), SAI_STATUS_SUCCESS);
        CHECK(a.value.aclcapability.is_action_list_mandatory == false && a.value.aclcapability.action_list.count == 0);
        CHECK(a.value.aclcapability.action_list.list == actions && MEMBER(a, aclcapability));
    }
    /* kept switch attributes: set and get of u32, u8, s32 */
    {
        sai_attribute_t s;
        fill(&s, SAI_SWITCH_ATTR_FDB_AGING_TIME);
        s.value.u32 = 0x01020304;
        CHECK_STATUS(switch_api->set_switch_attribute(the_switch, &s), SAI_STATUS_SUCCESS);
        fill(&a, SAI_SWITCH_ATTR_FDB_AGING_TIME);
        CHECK_STATUS(switch_api->get_switch_attribute(the_switch, 1, &a), SAI_STATUS_SUCCESS);
        CHECK(a.value.u32 == 0x01020304 && MEMBER(a, u32));
        fill(&s, SAI_SWITCH_ATTR_ECMP_DEFAULT_HASH_OFFSET);
        s.value.u8 = 0x5c;
        CHECK_STATUS(switch_api->set_switch_attribute(the_switch, &s), SAI_STATUS_SUCCESS);
        fill(&a, SAI_SWITCH_ATTR_ECMP_DEFAULT_HASH_OFFSET);
        CHECK_STATUS(switch_api->get_switch_attribute(the_switch, 1, &a), SAI_STATUS_SUCCESS);
        CHECK(a.value.u8 == 0x5c && MEMBER(a, u8));
        fill(&s, SAI_SWITCH_ATTR_ECMP_DEFAULT_HASH_ALGORITHM);
        s.value.s32 = SAI_HASH_ALGORITHM_XOR;
        CHECK_STATUS(switch_api->set_switch_attribute(the_switch, &s), SAI_STATUS_SUCCESS);
        s.value.s32 = 0x01000000; /* an enumerator only when read through the wrong bytes */
        CHECK_STATUS(switch_api->set_switch_attribute(the_switch, &s), SAI_STATUS_INVALID_ATTR_VALUE_0);
        fill(&a, SAI_SWITCH_ATTR_ECMP_DEFAULT_HASH_ALGORITHM);
        CHECK_STATUS(switch_api->get_switch_attribute(the_switch, 1, &a), SAI_STATUS_SUCCESS);
        CHECK(a.value.s32 == SAI_HASH_ALGORITHM_XOR && MEMBER(a, s32));
    }

    /* ---- ports: u16, u8, enum, bool set and get; queues, groups ---- */
    sai_object_id_t queues[8], groups[8], sgs[8];
    {
        sai_attribute_t s;
        fill(&s, SAI_PORT_ATTR_PORT_VLAN_ID);
        s.value.u16 = 0x0123;
        CHECK_STATUS(port_api->set_port_attribute(ports[1], &s), SAI_STATUS_SUCCESS);
        fill(&a, SAI_PORT_ATTR_PORT_VLAN_ID);
        CHECK_STATUS(port_api->get_port_attribute(ports[1], 1, &a), SAI_STATUS_SUCCESS);
        CHECK(a.value.u16 == 0x0123 && MEMBER(a, u16));
        fill(&s, SAI_PORT_ATTR_QOS_DEFAULT_TC);
        s.value.u8 = 5;
        CHECK_STATUS(port_api->set_port_attribute(ports[1], &s), SAI_STATUS_SUCCESS);
        fill(&a, SAI_PORT_ATTR_QOS_DEFAULT_TC);
        CHECK_STATUS(port_api->get_port_attribute(ports[1], 1, &a), SAI_STATUS_SUCCESS);
        CHECK(a.value.u8 == 5 && MEMBER(a, u8));
        fill(&s, SAI_PORT_ATTR_FEC_MODE);
        s.value.s32 = SAI_PORT_FEC_MODE_RS;
        CHECK_STATUS(port_api->set_port_attribute(ports[1], &s), SAI_STATUS_SUCCESS);
        fill(&a, SAI_PORT_ATTR_FEC_MODE);
        CHECK_STATUS(port_api->get_port_attribute(ports[1], 1, &a), SAI_STATUS_SUCCESS);
        CHECK(a.value.s32 == SAI_PORT_FEC_MODE_RS && MEMBER(a, s32));
        fill(&s, SAI_PORT_ATTR_AUTO_NEG_MODE);
        s.value.booldata = true;
        CHECK_STATUS(port_api->set_port_attribute(ports[1], &s), SAI_STATUS_SUCCESS);
        fill(&a, SAI_PORT_ATTR_AUTO_NEG_MODE);
        CHECK_STATUS(port_api->get_port_attribute(ports[1], 1, &a), SAI_STATUS_SUCCESS);
        CHECK(a.value.booldata == true && MEMBER(a, booldata));
        int32_t fec[4];
        fill(&a, SAI_PORT_ATTR_SUPPORTED_FEC_MODE);
        a.value.s32list.count = 4;
        a.value.s32list.list = fec;
        CHECK_STATUS(port_api->get_port_attribute(ports[1], 1, &a), SAI_STATUS_SUCCESS);
        CHECK(a.value.s32list.count == 3 && fec[1] == SAI_PORT_FEC_MODE_RS && MEMBER(a, s32list));

        fill(&a, SAI_PORT_ATTR_QOS_NUMBER_OF_QUEUES);
        CHECK_STATUS(port_api->get_port_attribute(ports[1], 1, &a), SAI_STATUS_SUCCESS);
        CHECK(a.value.u32 == 8 && MEMBER(a, u32));
        fill(&a, SAI_PORT_ATTR_QOS_QUEUE_LIST);
        a.value.objlist.count = 8;
        a.value.objlist.list = queues;
        CHECK_STATUS(port_api->get_port_attribute(ports[1], 1, &a), SAI_STATUS_SUCCESS);
        CHECK(a.value.objlist.count == 8 && MEMBER(a, objlist));
        fill(&a, SAI_PORT_ATTR_INGRESS_PRIORITY_GROUP_LIST);
        a.value.objlist.count = 8;
        a.value.objlist.list = groups;
        CHECK_STATUS(port_api->get_port_attribute(ports[1], 1, &a), SAI_STATUS_SUCCESS);
        CHECK(a.value.objlist.count == 8 && type_of(groups[7]) == SAI_OBJECT_TYPE_INGRESS_PRIORITY_GROUP);
        fill(&a, SAI_PORT_ATTR_QOS_SCHEDULER_GROUP_LIST);
        a.value.objlist.count = 8;
        a.value.objlist.list = sgs;
        CHECK_STATUS(port_api->get_port_attribute(ports[1], 1, &a), SAI_STATUS_SUCCESS);
        CHECK(a.value.objlist.count == 8 && type_of(sgs[0]) == SAI_OBJECT_TYPE_SCHEDULER_GROUP);

        sai_attribute_t q[3];
        fill(&q[0], SAI_QUEUE_ATTR_INDEX);
        fill(&q[1], SAI_QUEUE_ATTR_PORT);
        fill(&q[2], SAI_QUEUE_ATTR_TYPE);
        CHECK_STATUS(queue_api->get_queue_attribute(queues[6], 3, q), SAI_STATUS_SUCCESS);
        CHECK(q[0].value.u8 == 6 && MEMBER(q[0], u8) && q[1].value.oid == ports[1] && q[2].value.s32 == SAI_QUEUE_TYPE_UNICAST);
        CHECK_STATUS(queue_api->remove_queue(queues[6]), SAI_STATUS_NOT_SUPPORTED);
        sai_object_id_t children[2];
        fill(&a, SAI_SCHEDULER_GROUP_ATTR_CHILD_LIST);
        a.value.objlist.count = 2;
        a.value.objlist.list = children;
        CHECK_STATUS(sg_api->get_scheduler_group_attribute(sgs[6], 1, &a), SAI_STATUS_SUCCESS);
        CHECK(a.value.objlist.count == 1 && children[0] == queues[6]);
        uint16_t vlans[4];
        fill(&a, SAI_STP_ATTR_VLAN_LIST);
        a.value.vlanlist.count = 4;
        a.value.vlanlist.list = vlans;
        CHECK_STATUS(stp_api->get_stp_attribute(stp, 1, &a), SAI_STATUS_SUCCESS);
        CHECK(a.value.vlanlist.count >= 1 && vlans[0] == 1 && MEMBER(a, vlanlist));
    }

    /* ---- attribute-bag objects ---- */
    sai_object_id_t scheduler = 0, policer = 0, pool = 0, profile = 0, qos_map = 0, rif = 0, next_hop = 0, hostif = 0, vr2 = 0, trap = 0;
    {
        /* scheduler: u8, u64 */
        sai_attribute_t c[2];
        fill(&c[0], SAI_SCHEDULER_ATTR_SCHEDULING_WEIGHT);
        c[0].value.u8 = 0x11;
        fill(&c[1], SAI_SCHEDULER_ATTR_MAX_BANDWIDTH_RATE);
        c[1].value.u64 = 0x0102030405060708ULL;
        CHECK_STATUS(scheduler_api->create_scheduler(&scheduler, the_switch, 2, c), SAI_STATUS_SUCCESS);
        sai_attribute_t g[2];
        fill(&g[0], SAI_SCHEDULER_ATTR_SCHEDULING_WEIGHT);
        fill(&g[1], SAI_SCHEDULER_ATTR_MAX_BANDWIDTH_RATE);
        CHECK_STATUS(scheduler_api->get_scheduler_attribute(scheduler, 2, g), SAI_STATUS_SUCCESS);
        CHECK(g[0].value.u8 == 0x11 && MEMBER(g[0], u8) && g[1].value.u64 == 0x0102030405060708ULL && MEMBER(g[1], u64));
        /* a queue names it: a reference made by set */
        sai_attribute_t s;
        fill(&s, SAI_QUEUE_ATTR_SCHEDULER_PROFILE_ID);
        s.value.oid = scheduler;
        CHECK_STATUS(queue_api->set_queue_attribute(queues[0], &s), SAI_STATUS_SUCCESS);
        CHECK_STATUS(scheduler_api->remove_scheduler(scheduler), SAI_STATUS_OBJECT_IN_USE);
        s.value.oid = SAI_NULL_OBJECT_ID;
        CHECK_STATUS(queue_api->set_queue_attribute(queues[0], &s), SAI_STATUS_SUCCESS);
        CHECK_STATUS(scheduler_api->remove_scheduler(scheduler), SAI_STATUS_SUCCESS);
    }
    {
        /* buffer pool and profile: enum, u64, s8 */
        sai_attribute_t c[4];
        fill(&c[0], SAI_BUFFER_POOL_ATTR_TYPE);
        c[0].value.s32 = SAI_BUFFER_POOL_TYPE_INGRESS;
        fill(&c[1], SAI_BUFFER_POOL_ATTR_SIZE);
        c[1].value.u64 = 1ULL << 33;
        CHECK_STATUS(buffer_api->create_buffer_pool(&pool, the_switch, 2, c), SAI_STATUS_SUCCESS);
        fill(&c[0], SAI_BUFFER_PROFILE_ATTR_POOL_ID);
        c[0].value.oid = pool;
        fill(&c[1], SAI_BUFFER_PROFILE_ATTR_RESERVED_BUFFER_SIZE);
        c[1].value.u64 = 4096;
        fill(&c[2], SAI_BUFFER_PROFILE_ATTR_THRESHOLD_MODE);
        c[2].value.s32 = SAI_BUFFER_PROFILE_THRESHOLD_MODE_DYNAMIC;
        fill(&c[3], SAI_BUFFER_PROFILE_ATTR_SHARED_DYNAMIC_TH);
        c[3].value.s8 = -3;
        CHECK_STATUS(buffer_api->create_buffer_profile(&profile, the_switch, 4, c), SAI_STATUS_SUCCESS);
        fill(&a, SAI_BUFFER_PROFILE_ATTR_SHARED_DYNAMIC_TH);
        CHECK_STATUS(buffer_api->get_buffer_profile_attribute(profile, 1, &a), SAI_STATUS_SUCCESS);
        CHECK(a.value.s8 == -3 && MEMBER(a, s8));
        fill(&a, SAI_BUFFER_POOL_ATTR_SIZE);
        CHECK_STATUS(buffer_api->get_buffer_pool_attribute(pool, 1, &a), SAI_STATUS_SUCCESS);
        CHECK(a.value.u64 == 1ULL << 33 && MEMBER(a, u64));
        CHECK_STATUS(buffer_api->remove_buffer_pool(pool), SAI_STATUS_OBJECT_IN_USE);
        /* an object list set on a port and read back */
        sai_attribute_t s;
        fill(&s, SAI_PORT_ATTR_QOS_INGRESS_BUFFER_PROFILE_LIST);
        s.value.objlist.count = 1;
        s.value.objlist.list = &profile;
        CHECK_STATUS(port_api->set_port_attribute(ports[1], &s), SAI_STATUS_SUCCESS);
        sai_object_id_t back[2] = {0, 0};
        fill(&a, SAI_PORT_ATTR_QOS_INGRESS_BUFFER_PROFILE_LIST);
        a.value.objlist.count = 2;
        a.value.objlist.list = back;
        CHECK_STATUS(port_api->get_port_attribute(ports[1], 1, &a), SAI_STATUS_SUCCESS);
        CHECK(a.value.objlist.count == 1 && back[0] == profile);
        CHECK_STATUS(buffer_api->remove_buffer_profile(profile), SAI_STATUS_OBJECT_IN_USE);
        s.value.objlist.count = 0;
        s.value.objlist.list = NULL;
        CHECK_STATUS(port_api->set_port_attribute(ports[1], &s), SAI_STATUS_SUCCESS);
        CHECK_STATUS(buffer_api->remove_buffer_profile(profile), SAI_STATUS_SUCCESS);
        CHECK_STATUS(buffer_api->remove_buffer_pool(pool), SAI_STATUS_SUCCESS);
    }
    {
        /* QoS map: a list of structs of bytes and an enum */
        sai_qos_map_t map[3];
        memset(map, GARBAGE, sizeof(map));
        for (int i = 0; i < 3; i++) {
            map[i].key.dscp = (uint8_t)(10 + i);
            map[i].key.color = SAI_PACKET_COLOR_YELLOW;
            map[i].value.tc = (uint8_t)i;
            map[i].value.queue_index = (uint8_t)(7 - i);
        }
        sai_attribute_t c[2];
        fill(&c[0], SAI_QOS_MAP_ATTR_TYPE);
        c[0].value.s32 = SAI_QOS_MAP_TYPE_DSCP_TO_TC;
        fill(&c[1], SAI_QOS_MAP_ATTR_MAP_TO_VALUE_LIST);
        c[1].value.qosmap.count = 3;
        c[1].value.qosmap.list = map;
        CHECK_STATUS(qos_map_api->create_qos_map(&qos_map, the_switch, 2, c), SAI_STATUS_SUCCESS);
        sai_qos_map_t back[4];
        memset(back, 0, sizeof(back));
        fill(&a, SAI_QOS_MAP_ATTR_MAP_TO_VALUE_LIST);
        a.value.qosmap.count = 4;
        a.value.qosmap.list = back;
        CHECK_STATUS(qos_map_api->get_qos_map_attribute(qos_map, 1, &a), SAI_STATUS_SUCCESS);
        CHECK(a.value.qosmap.count == 3 && MEMBER(a, qosmap));
        CHECK(back[2].key.dscp == 12 && back[2].key.color == SAI_PACKET_COLOR_YELLOW && back[2].value.tc == 2 && back[2].value.queue_index == 5);
        CHECK(back[0].key.dscp == 10 && back[0].value.queue_index == 7);
        /* bound to a port */
        sai_attribute_t s;
        fill(&s, SAI_PORT_ATTR_QOS_DSCP_TO_TC_MAP);
        s.value.oid = qos_map;
        CHECK_STATUS(port_api->set_port_attribute(ports[1], &s), SAI_STATUS_SUCCESS);
        CHECK_STATUS(qos_map_api->remove_qos_map(qos_map), SAI_STATUS_OBJECT_IN_USE);
        s.value.oid = SAI_NULL_OBJECT_ID;
        CHECK_STATUS(port_api->set_port_attribute(ports[1], &s), SAI_STATUS_SUCCESS);
        CHECK_STATUS(qos_map_api->remove_qos_map(qos_map), SAI_STATUS_SUCCESS);
    }
    {
        /* policer: enums and u64 */
        sai_attribute_t c[3];
        fill(&c[0], SAI_POLICER_ATTR_METER_TYPE);
        c[0].value.s32 = SAI_METER_TYPE_PACKETS;
        fill(&c[1], SAI_POLICER_ATTR_MODE);
        c[1].value.s32 = SAI_POLICER_MODE_SR_TCM;
        fill(&c[2], SAI_POLICER_ATTR_CIR);
        c[2].value.u64 = 600;
        CHECK_STATUS(policer_api->create_policer(&policer, the_switch, 3, c), SAI_STATUS_SUCCESS);
        fill(&a, SAI_POLICER_ATTR_CIR);
        CHECK_STATUS(policer_api->get_policer_attribute(policer, 1, &a), SAI_STATUS_SUCCESS);
        CHECK(a.value.u64 == 600 && MEMBER(a, u64));
        /* the default trap group takes it */
        sai_attribute_t s;
        fill(&s, SAI_HOSTIF_TRAP_GROUP_ATTR_POLICER);
        s.value.oid = policer;
        CHECK_STATUS(hostif_api->set_hostif_trap_group_attribute(trap_group, &s), SAI_STATUS_SUCCESS);
        CHECK_STATUS(policer_api->remove_policer(policer), SAI_STATUS_OBJECT_IN_USE);
    }
    {
        /* hash: a list of enumerators, set and get */
        int32_t fields[3] = {SAI_NATIVE_HASH_FIELD_SRC_IP, SAI_NATIVE_HASH_FIELD_DST_IP, SAI_NATIVE_HASH_FIELD_L4_SRC_PORT};
        sai_attribute_t s;
        fill(&s, SAI_HASH_ATTR_NATIVE_HASH_FIELD_LIST);
        s.value.s32list.count = 3;
        s.value.s32list.list = fields;
        CHECK_STATUS(hash_api->set_hash_attribute(lag_hash, &s), SAI_STATUS_SUCCESS);
        int32_t back[4] = {0};
        fill(&a, SAI_HASH_ATTR_NATIVE_HASH_FIELD_LIST);
        a.value.s32list.count = 4;
        a.value.s32list.list = back;
        CHECK_STATUS(hash_api->get_hash_attribute(lag_hash, 1, &a), SAI_STATUS_SUCCESS);
        CHECK(a.value.s32list.count == 3 && back[2] == SAI_NATIVE_HASH_FIELD_L4_SRC_PORT);
        fields[1] = 0x01000000;
        CHECK_STATUS(hash_api->set_hash_attribute(lag_hash, &s), SAI_STATUS_INVALID_ATTR_VALUE_0);
        CHECK_STATUS(hash_api->remove_hash(lag_hash), SAI_STATUS_NOT_SUPPORTED);
    }
    {
        /* virtual router, router interface (mac), next hop (IP address), host interface (text) */
        CHECK_STATUS(vr_api->create_virtual_router(&vr2, the_switch, 0, NULL), SAI_STATUS_SUCCESS);
        CHECK(vr2 != vr);
        CHECK_STATUS(vr_api->remove_virtual_router(vr), SAI_STATUS_OBJECT_IN_USE); /* the default one stays */
        sai_attribute_t c[4];
        fill(&c[0], SAI_ROUTER_INTERFACE_ATTR_VIRTUAL_ROUTER_ID);
        c[0].value.oid = vr2;
        fill(&c[1], SAI_ROUTER_INTERFACE_ATTR_TYPE);
        c[1].value.s32 = SAI_ROUTER_INTERFACE_TYPE_PORT;
        fill(&c[2], SAI_ROUTER_INTERFACE_ATTR_PORT_ID);
        c[2].value.oid = ports[2];
        fill(&c[3], SAI_ROUTER_INTERFACE_ATTR_SRC_MAC_ADDRESS);
        memcpy(c[3].value.mac, "\x02\x11\x22\x33\x44\x55", 6);
        CHECK_STATUS(rif_api->create_router_interface(&rif, the_switch, 2, c), SAI_STATUS_MANDATORY_ATTRIBUTE_MISSING);
        c[2].value.oid = queues[0]; /* not a port */
        CHECK_STATUS(rif_api->create_router_interface(&rif, the_switch, 4, c), SAI_STATUS_INVALID_ATTR_VALUE_0 + SAI_STATUS_CODE(2));
        c[2].value.oid = ports[2];
        CHECK_STATUS(rif_api->create_router_interface(&rif, the_switch, 4, c), SAI_STATUS_SUCCESS);
        sai_attribute_t g[2];
        fill(&g[0], SAI_ROUTER_INTERFACE_ATTR_SRC_MAC_ADDRESS);
        fill(&g[1], SAI_ROUTER_INTERFACE_ATTR_MTU);
        CHECK_STATUS(rif_api->get_router_interface_attribute(rif, 2, g), SAI_STATUS_SUCCESS);
        CHECK(!memcmp(g[0].value.mac, "\x02\x11\x22\x33\x44\x55", 6) && MEMBER(g[0], mac) && g[1].value.u32 == 1514);
        CHECK_STATUS(vr_api->remove_virtual_router(vr2), SAI_STATUS_OBJECT_IN_USE);

        fill(&c[0], SAI_NEXT_HOP_ATTR_TYPE);
        c[0].value.s32 = SAI_NEXT_HOP_TYPE_IP;
        fill(&c[1], SAI_NEXT_HOP_ATTR_ROUTER_INTERFACE_ID);
        c[1].value.oid = rif;
        fill(&c[2], SAI_NEXT_HOP_ATTR_IP);
        c[2].value.ipaddr.addr_family = SAI_IP_ADDR_FAMILY_IPV4;
        memcpy(&c[2].value.ipaddr.addr.ip4, "\x0a\x01\x02\x03", 4); /* 10.1.2.3: network order in memory */
        CHECK_STATUS(nh_api->create_next_hop(&next_hop, the_switch, 3, c), SAI_STATUS_SUCCESS);
        fill(&a, SAI_NEXT_HOP_ATTR_IP);
        CHECK_STATUS(nh_api->get_next_hop_attribute(next_hop, 1, &a), SAI_STATUS_SUCCESS);
        CHECK(a.value.ipaddr.addr_family == SAI_IP_ADDR_FAMILY_IPV4 && !memcmp(&a.value.ipaddr.addr.ip4, "\x0a\x01\x02\x03", 4) && MEMBER(a, ipaddr));
        c[2].value.ipaddr.addr_family = (sai_ip_addr_family_t)9;
        sai_object_id_t none = 0;
        CHECK_STATUS(nh_api->create_next_hop(&none, the_switch, 3, c), SAI_STATUS_INVALID_ATTR_VALUE_0 + SAI_STATUS_CODE(2));
        /* IPv6 */
        sai_object_id_t next_hop6 = 0;
        c[2].value.ipaddr.addr_family = SAI_IP_ADDR_FAMILY_IPV6;
        memcpy(c[2].value.ipaddr.addr.ip6, "\xfc\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x02", 16);
        CHECK_STATUS(nh_api->create_next_hop(&next_hop6, the_switch, 3, c), SAI_STATUS_SUCCESS);
        fill(&a, SAI_NEXT_HOP_ATTR_IP);
        CHECK_STATUS(nh_api->get_next_hop_attribute(next_hop6, 1, &a), SAI_STATUS_SUCCESS);
        CHECK(a.value.ipaddr.addr_family == SAI_IP_ADDR_FAMILY_IPV6 && a.value.ipaddr.addr.ip6[0] == 0xfc && a.value.ipaddr.addr.ip6[15] == 2);
        CHECK_STATUS(nh_api->remove_next_hop(next_hop6), SAI_STATUS_SUCCESS);

        fill(&c[0], SAI_HOSTIF_ATTR_TYPE);
        c[0].value.s32 = SAI_HOSTIF_TYPE_NETDEV;
        fill(&c[1], SAI_HOSTIF_ATTR_OBJ_ID);
        c[1].value.oid = ports[2];
        fill(&c[2], SAI_HOSTIF_ATTR_NAME);
        memset(c[2].value.chardata, 0, sizeof(c[2].value.chardata));
        strcpy(c[2].value.chardata, "Ethernet8");
        CHECK_STATUS(hostif_api->create_hostif(&hostif, the_switch, 3, c), SAI_STATUS_SUCCESS);
        CHECK_STATUS(hostif_api->create_hostif(&none, the_switch, 3, c), SAI_STATUS_ITEM_ALREADY_EXISTS);
        fill(&a, SAI_HOSTIF_ATTR_NAME);
        CHECK_STATUS(hostif_api->get_hostif_attribute(hostif, 1, &a), SAI_STATUS_SUCCESS);
        CHECK(!strcmp(a.value.chardata, "Ethernet8") && MEMBER(a, chardata));
        sai_attribute_t s;
        fill(&s, SAI_HOSTIF_ATTR_OPER_STATUS);
        s.value.booldata = true;
        CHECK_STATUS(hostif_api->set_hostif_attribute(hostif, &s), SAI_STATUS_SUCCESS);
        fill(&a, SAI_HOSTIF_ATTR_OPER_STATUS);
        CHECK_STATUS(hostif_api->get_hostif_attribute(hostif, 1, &a), SAI_STATUS_SUCCESS);
        CHECK(a.value.booldata == true && MEMBER(a, booldata));

        fill(&c[0], SAI_HOSTIF_TRAP_ATTR_TRAP_TYPE);
        c[0].value.s32 = SAI_HOSTIF_TRAP_TYPE_LACP;
        fill(&c[1], SAI_HOSTIF_TRAP_ATTR_PACKET_ACTION);
        c[1].value.s32 = SAI_PACKET_ACTION_TRAP;
        fill(&c[2], SAI_HOSTIF_TRAP_ATTR_TRAP_GROUP);
        c[2].value.oid = trap_group;
        CHECK_STATUS(hostif_api->create_hostif_trap(&trap, the_switch, 3, c), SAI_STATUS_SUCCESS);
        CHECK_STATUS(hostif_api->create_hostif_trap(&none, the_switch, 3, c), SAI_STATUS_ITEM_ALREADY_EXISTS);
        CHECK_STATUS(hostif_api->recv_hostif_packet(hostif, NULL, NULL, NULL, NULL), SAI_STATUS_NOT_IMPLEMENTED);
    }

    /* ---- entries: the key is a struct ---- */
    {
        sai_neighbor_entry_t n;
        memset(&n, GARBAGE, sizeof(n));
        n.switch_id = the_switch;
        n.rif_id = rif;
        n.ip_address.addr_family = SAI_IP_ADDR_FAMILY_IPV4;
        memcpy(&n.ip_address.addr.ip4, "\x0a\x01\x02\x03", 4);
        sai_attribute_t c[2];
        fill(&c[0], SAI_NEIGHBOR_ENTRY_ATTR_DST_MAC_ADDRESS);
        memcpy(c[0].value.mac, "\x02\x00\x00\x00\x00\x42", 6);
        CHECK_STATUS(neighbor_api->create_neighbor_entry(NULL, 1, c), SAI_STATUS_INVALID_PARAMETER);
        CHECK_STATUS(neighbor_api->create_neighbor_entry(&n, 0, NULL), SAI_STATUS_MANDATORY_ATTRIBUTE_MISSING);
        CHECK_STATUS(neighbor_api->create_neighbor_entry(&n, 1, c), SAI_STATUS_SUCCESS);
        CHECK_STATUS(neighbor_api->create_neighbor_entry(&n, 1, c), SAI_STATUS_ITEM_ALREADY_EXISTS);
        /* the same address spelled as another neighbor: one byte differs */
        sai_neighbor_entry_t other = n;
        memcpy(&other.ip_address.addr.ip4, "\x0a\x01\x02\x04", 4);
        fill(&a, SAI_NEIGHBOR_ENTRY_ATTR_DST_MAC_ADDRESS);
        CHECK_STATUS(neighbor_api->get_neighbor_entry_attribute(&other, 1, &a), SAI_STATUS_ITEM_NOT_FOUND);
        CHECK_STATUS(neighbor_api->get_neighbor_entry_attribute(&n, 1, &a), SAI_STATUS_SUCCESS);
        CHECK(!memcmp(a.value.mac, "\x02\x00\x00\x00\x00\x42", 6) && MEMBER(a, mac));
        CHECK_STATUS(rif_api->remove_router_interface(rif), SAI_STATUS_OBJECT_IN_USE);

        sai_route_entry_t r;
        memset(&r, GARBAGE, sizeof(r));
        r.switch_id = the_switch;
        r.vr_id = vr2;
        r.destination.addr_family = SAI_IP_ADDR_FAMILY_IPV4;
        memcpy(&r.destination.addr.ip4, "\x0a\x14\x00\x00", 4);
        memcpy(&r.destination.mask.ip4, "\xff\xff\x00\x00", 4);
        fill(&c[0], SAI_ROUTE_ENTRY_ATTR_NEXT_HOP_ID);
        c[0].value.oid = next_hop;
        CHECK_STATUS(route_api->create_route_entry(&r, 1, c), SAI_STATUS_SUCCESS);
        /* host bits under the mask do not make another route (10.20.3.4/16 is 10.20.0.0/16) */
        sai_route_entry_t same = r;
        memcpy(&same.destination.addr.ip4, "\x0a\x14\x03\x04", 4);
        CHECK_STATUS(route_api->create_route_entry(&same, 1, c), SAI_STATUS_ITEM_ALREADY_EXISTS);
        /* 10.0.20.0/16 would be the same route if the address were read in the wrong byte order */
        sai_route_entry_t swapped = r;
        memcpy(&swapped.destination.addr.ip4, "\x00\x00\x14\x0a", 4);
        memcpy(&swapped.destination.mask.ip4, "\x00\x00\xff\xff", 4);
        fill(&a, SAI_ROUTE_ENTRY_ATTR_NEXT_HOP_ID);
        CHECK_STATUS(route_api->get_route_entry_attribute(&swapped, 1, &a), SAI_STATUS_ITEM_NOT_FOUND);
        sai_attribute_t g[2];
        fill(&g[0], SAI_ROUTE_ENTRY_ATTR_NEXT_HOP_ID);
        fill(&g[1], SAI_ROUTE_ENTRY_ATTR_PACKET_ACTION);
        CHECK_STATUS(route_api->get_route_entry_attribute(&r, 2, g), SAI_STATUS_SUCCESS);
        CHECK(g[0].value.oid == next_hop && MEMBER(g[0], oid) && g[1].value.s32 == SAI_PACKET_ACTION_FORWARD && MEMBER(g[1], s32));
        CHECK_STATUS(nh_api->remove_next_hop(next_hop), SAI_STATUS_OBJECT_IN_USE);
        sai_attribute_t s;
        fill(&s, SAI_ROUTE_ENTRY_ATTR_PACKET_ACTION);
        s.value.s32 = SAI_PACKET_ACTION_DROP;
        CHECK_STATUS(route_api->set_route_entry_attribute(&r, &s), SAI_STATUS_SUCCESS);
        CHECK_STATUS(route_api->get_route_entry_attribute(&r, 2, g), SAI_STATUS_SUCCESS);
        CHECK(g[1].value.s32 == SAI_PACKET_ACTION_DROP);
        /* bulk: a clean "not implemented" (syncd then does them one by one) */
        CHECK_STATUS(route_api->create_route_entries(1, &r, NULL, NULL, SAI_BULK_OP_ERROR_MODE_STOP_ON_ERROR, NULL), SAI_STATUS_NOT_IMPLEMENTED);
        CHECK_STATUS(route_api->remove_route_entry(&r), SAI_STATUS_SUCCESS);
        CHECK_STATUS(route_api->remove_route_entry(&r), SAI_STATUS_ITEM_NOT_FOUND);
        r.destination.addr_family = (sai_ip_addr_family_t)7;
        CHECK_STATUS(route_api->remove_route_entry(&r), SAI_STATUS_INVALID_PARAMETER);
        CHECK_STATUS(neighbor_api->remove_neighbor_entry(&n), SAI_STATUS_SUCCESS);
        CHECK_STATUS(nh_api->remove_next_hop(next_hop), SAI_STATUS_SUCCESS);
        CHECK_STATUS(rif_api->remove_router_interface(rif), SAI_STATUS_SUCCESS);
        CHECK_STATUS(vr_api->remove_virtual_router(vr2), SAI_STATUS_SUCCESS);

        /* FDB */
        sai_object_id_t bps[8];
        fill(&a, SAI_BRIDGE_ATTR_PORT_LIST);
        a.value.objlist.count = 8;
        a.value.objlist.list = bps;
        CHECK_STATUS(bridge_api->get_bridge_attribute(bridge, 1, &a), SAI_STATUS_SUCCESS);
        sai_fdb_entry_t f;
        memset(&f, GARBAGE, sizeof(f));
        f.switch_id = the_switch;
        f.bv_id = default_vlan;
        memcpy(f.mac_address, "\x02\x00\x00\x00\x00\x01", 6);
        fill(&c[0], SAI_FDB_ENTRY_ATTR_TYPE);
        c[0].value.s32 = SAI_FDB_ENTRY_TYPE_STATIC;
        fill(&c[1], SAI_FDB_ENTRY_ATTR_BRIDGE_PORT_ID);
        c[1].value.oid = bps[0]; /* the one bridge port main() left */
        CHECK_STATUS(fdb_api->create_fdb_entry(&f, 2, c), SAI_STATUS_SUCCESS);
        sai_fdb_entry_t f2 = f;
        f2.mac_address[5] = 2;
        c[0].value.s32 = SAI_FDB_ENTRY_TYPE_DYNAMIC;
        CHECK_STATUS(fdb_api->create_fdb_entry(&f2, 2, c), SAI_STATUS_SUCCESS);
        f2.bv_id = bridge; /* .1D: not implemented */
        CHECK_STATUS(fdb_api->create_fdb_entry(&f2, 2, c), SAI_STATUS_INVALID_PARAMETER);
        f2.bv_id = default_vlan;
        fill(&a, SAI_FDB_ENTRY_ATTR_BRIDGE_PORT_ID);
        CHECK_STATUS(fdb_api->get_fdb_entry_attribute(&f, 1, &a), SAI_STATUS_SUCCESS);
        CHECK(a.value.oid == bps[0] && MEMBER(a, oid));
        /* flush: dynamic only by default */
        CHECK_STATUS(fdb_api->flush_fdb_entries(the_switch, 0, NULL), SAI_STATUS_SUCCESS);
        CHECK_STATUS(fdb_api->get_fdb_entry_attribute(&f2, 1, &a), SAI_STATUS_ITEM_NOT_FOUND);
        CHECK_STATUS(fdb_api->get_fdb_entry_attribute(&f, 1, &a), SAI_STATUS_SUCCESS);
        sai_attribute_t all;
        fill(&all, SAI_FDB_FLUSH_ATTR_ENTRY_TYPE);
        all.value.s32 = SAI_FDB_FLUSH_ENTRY_TYPE_ALL;
        CHECK_STATUS(fdb_api->flush_fdb_entries(the_switch, 1, &all), SAI_STATUS_SUCCESS);
        CHECK_STATUS(fdb_api->remove_fdb_entry(&f), SAI_STATUS_ITEM_NOT_FOUND);
    }

    /* ---- counters ---- */
    {
        sai_stat_id_t ids[3] = {SAI_PORT_STAT_IF_IN_OCTETS, SAI_PORT_STAT_IF_OUT_UCAST_PKTS, SAI_PORT_STAT_IF_IN_ERRORS};
        uint64_t counters[4] = {7, 7, 7, 7};
        CHECK_STATUS(port_api->get_port_stats(ports[1], 3, ids, counters), SAI_STATUS_SUCCESS);
        CHECK(counters[0] == 0 && counters[2] == 0 && counters[3] == 7);
        CHECK_STATUS(port_api->get_port_stats(ports[1], 3, ids, NULL), SAI_STATUS_INVALID_PARAMETER);
        CHECK_STATUS(port_api->get_port_stats(ports[1], 3, NULL, counters), SAI_STATUS_INVALID_PARAMETER);
        CHECK_STATUS(port_api->get_port_stats_ext(ports[1], 3, ids, SAI_STATS_MODE_READ_AND_CLEAR, counters), SAI_STATUS_SUCCESS);
        CHECK_STATUS(port_api->clear_port_stats(ports[1], 3, ids), SAI_STATUS_SUCCESS);
        CHECK_STATUS(port_api->clear_port_all_stats(ports[1]), SAI_STATUS_NOT_IMPLEMENTED);
        ids[1] = SAI_PORT_STAT_ETHER_STATS_JABBERS;
        counters[0] = 7;
        CHECK_STATUS(port_api->get_port_stats(ports[1], 3, ids, counters), SAI_STATUS_NOT_SUPPORTED);
        CHECK(counters[0] == 7);
        sai_stat_id_t qids[2] = {SAI_QUEUE_STAT_PACKETS, SAI_QUEUE_STAT_BYTES};
        CHECK_STATUS(queue_api->get_queue_stats(queues[1], 2, qids, counters), SAI_STATUS_SUCCESS);
        CHECK(counters[0] == 0 && counters[1] == 0);
        CHECK_STATUS(queue_api->get_queue_stats(ports[1], 2, qids, counters), SAI_STATUS_INVALID_OBJECT_TYPE);
        sai_stat_id_t gids[1] = {SAI_INGRESS_PRIORITY_GROUP_STAT_PACKETS};
        CHECK_STATUS(buffer_api->get_ingress_priority_group_stats(groups[1], 1, gids, counters), SAI_STATUS_SUCCESS);
        CHECK_STATUS(switch_api->get_switch_stats(the_switch, 0, NULL, NULL), SAI_STATUS_NOT_SUPPORTED);

        sai_stat_capability_t caps[32];
        sai_stat_capability_list_t list = {.count = 0, .list = NULL};
        CHECK_STATUS(stats_caps(the_switch, SAI_OBJECT_TYPE_PORT, &list), SAI_STATUS_BUFFER_OVERFLOW);
        CHECK(list.count == 15);
        list.count = 32;
        list.list = caps;
        CHECK_STATUS(stats_caps(the_switch, SAI_OBJECT_TYPE_QUEUE, &list), SAI_STATUS_SUCCESS);
        CHECK(list.count == 3 && caps[0].stat_enum == SAI_QUEUE_STAT_PACKETS && (caps[0].stat_modes & SAI_STATS_MODE_READ));
        CHECK_STATUS(stats_caps(the_switch, SAI_OBJECT_TYPE_ROUTER_INTERFACE, &list), SAI_STATUS_NOT_SUPPORTED);
        CHECK_STATUS(stats_caps(the_switch, SAI_OBJECT_TYPE_PORT, NULL), SAI_STATUS_INVALID_PARAMETER);
        int32_t values[4];
        sai_s32_list_t enums = {.count = 4, .list = values};
        CHECK_STATUS(enum_caps(the_switch, SAI_OBJECT_TYPE_HOSTIF, SAI_HOSTIF_ATTR_TYPE, &enums), SAI_STATUS_SUCCESS);
        CHECK(enums.count == 1 && values[0] == SAI_HOSTIF_TYPE_NETDEV);
        CHECK_STATUS(enum_caps(the_switch, SAI_OBJECT_TYPE_PORT, SAI_PORT_ATTR_SPEED, &enums), SAI_STATUS_NOT_SUPPORTED);
    }
    /* LAG: create, an empty PORT_LIST, remove; its bulk entries say "not implemented" and are a
       table entry, not a null pointer (syncd calls bulk entries unchecked) */
    {
        sai_object_id_t lag = 0x77;
        CHECK_STATUS(lag_api->create_lag(&lag, the_switch, 0, NULL), SAI_STATUS_SUCCESS);
        CHECK(type_of(lag) == SAI_OBJECT_TYPE_LAG);
        sai_object_id_t lag_ports[4];
        sai_attribute_t la = {.id = SAI_LAG_ATTR_PORT_LIST, .value.objlist = {.count = 4, .list = lag_ports}};
        CHECK_STATUS(lag_api->get_lag_attribute(lag, 1, &la), SAI_STATUS_SUCCESS);
        CHECK(la.value.objlist.count == 0);
        CHECK_STATUS(lag_api->remove_lag(lag), SAI_STATUS_SUCCESS);
        CHECK(lag_api->create_lag_members && lag_api->remove_lag_members);
        sai_object_id_t member = 0x77;
        sai_status_t member_status = SAI_STATUS_SUCCESS;
        CHECK_STATUS(lag_api->create_lag_members(the_switch, 0, NULL, NULL, SAI_BULK_OP_ERROR_MODE_STOP_ON_ERROR,
                                                 &member, &member_status),
                     SAI_STATUS_NOT_IMPLEMENTED);
        CHECK(member == 0x77);
    }
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
    CHECK_STATUS(p_sai_api_query(SAI_API_TUNNEL, &table), SAI_STATUS_NOT_IMPLEMENTED);
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
    /* no entry is null: what is not implemented is a function that says so */
    CHECK(port_api->get_port_stats && vlan_api->create_vlan_members && switch_api->switch_mdio_read && port_api->create_ports);
    CHECK_STATUS(vlan_api->create_vlan_members(0, 1, NULL, NULL, SAI_BULK_OP_ERROR_MODE_STOP_ON_ERROR, NULL, NULL), SAI_STATUS_NOT_IMPLEMENTED);
    CHECK_STATUS(switch_api->switch_mdio_read(0, 1, 2, 1, NULL), SAI_STATUS_NOT_IMPLEMENTED);

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
        bad[1].id = SAI_SWITCH_ATTR_CRC_CHECK_ENABLE; /* valid SAI, not implemented */
        bad[1].value.booldata = false;
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
        g[2].id = SAI_SWITCH_ATTR_MAX_STP_INSTANCE;
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

    second_slice(lib, ports);

    /* global queries: capabilities as the C++ sai-openbcm's table; the rest "not supported" without touching their pointers */
    {
        sai_attr_capability_t cap;
        memset(&cap, 0xa5, sizeof(cap));
        CHECK_STATUS(p_sai_query_attribute_capability(the_switch, SAI_OBJECT_TYPE_PORT, SAI_PORT_ATTR_SPEED, &cap), SAI_STATUS_SUCCESS);
        CHECK(!cap.create_implemented && cap.set_implemented && cap.get_implemented);
        CHECK_STATUS(p_sai_query_attribute_capability(the_switch, SAI_OBJECT_TYPE_ACL_TABLE, SAI_ACL_TABLE_ATTR_ACL_STAGE, &cap), SAI_STATUS_SUCCESS);
        CHECK(!cap.create_implemented && !cap.set_implemented && !cap.get_implemented);
        CHECK_STATUS(p_sai_query_attribute_capability(the_switch, SAI_OBJECT_TYPE_PORT, SAI_PORT_ATTR_SPEED, NULL), SAI_STATUS_INVALID_PARAMETER);
    }
    CHECK_STATUS(p_sai_object_type_get_availability(the_switch, SAI_OBJECT_TYPE_VLAN, 0, NULL, NULL), SAI_STATUS_NOT_SUPPORTED);
    CHECK_STATUS(p_sai_get_object_count(the_switch, SAI_OBJECT_TYPE_VLAN, NULL), SAI_STATUS_NOT_SUPPORTED);

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
