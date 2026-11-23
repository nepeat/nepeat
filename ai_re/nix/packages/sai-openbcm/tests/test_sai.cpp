// SPDX-License-Identifier: Apache-2.0
// Unit tests of the SAI layer against the fake backend. No hardware, no
// redis, no syncd: the test is the SAI adapter host (service method table).
//
// Only the SAI C ABI and tests/test_port.h (namespace tp): no library-internal header, no ob::
// symbol. The library is linked (or loaded) through the SAI ABI alone, so the same source runs
// on another libsai with its own test_port implementation. The pure C++ unit tests of
// library internals (OID status helpers, ext-PHY / ext-MACsec table parsers and executor) live
// in tests/test_units.cpp.
#include "test_port.h"
#include "test_tables.h"

#include <algorithm>
#include <chrono>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <fstream>
#include <functional>
#include <iterator>
#include <map>
#include <mutex>
#include <string>
#include <thread>
#include <tuple>
#include <vector>

#include <sys/socket.h>
#include <sys/stat.h>
#include <sys/un.h>
#include <unistd.h>

static int g_fail = 0, g_checks = 0;
#define CHECK(cond)                                                                    \
    do {                                                                               \
        g_checks++;                                                                    \
        if (!(cond)) {                                                                 \
            g_fail++;                                                                  \
            std::fprintf(stderr, "FAIL %s:%d: %s\n", __FILE__, __LINE__, #cond);       \
        }                                                                              \
    } while (0)
#define CHECK_EQ(a, b)                                                                 \
    do {                                                                               \
        g_checks++;                                                                    \
        auto _a = (a);                                                                 \
        auto _b = (b);                                                                 \
        if (!(_a == _b)) {                                                             \
            g_fail++;                                                                  \
            std::fprintf(stderr, "FAIL %s:%d: %s == %s (%lld vs %lld)\n", __FILE__,     \
                         __LINE__, #a, #b, (long long)_a, (long long)_b);              \
        }                                                                              \
    } while (0)
#define OK(x) CHECK_EQ((x), SAI_STATUS_SUCCESS)

// ------------------------------------------------------------- profile
static std::map<std::string, std::string> g_profile;
static std::map<std::string, std::string>::iterator g_iter;
static const char* prof_get(sai_switch_profile_id_t, const char* k)
{
    auto it = g_profile.find(k);
    return it == g_profile.end() ? nullptr : it->second.c_str();
}
static int prof_next(sai_switch_profile_id_t, const char** k, const char** v)
{
    if (!k || !v) { g_iter = g_profile.begin(); return 0; }
    if (g_iter == g_profile.end()) return -1;
    *k = g_iter->first.c_str();
    *v = g_iter->second.c_str();
    ++g_iter;
    return 0;
}
static sai_service_method_table_t g_smt = {prof_get, prof_next};

// ------------------------------------------------------- notifications
static std::mutex g_nmu;
static std::vector<sai_port_oper_status_notification_t> g_port_events;
static std::vector<std::pair<sai_fdb_event_t, sai_object_id_t>> g_fdb_events;   // event, bridge port
static void on_port(uint32_t n, const sai_port_oper_status_notification_t* d)
{
    std::lock_guard<std::mutex> g(g_nmu);
    for (uint32_t i = 0; i < n; i++) g_port_events.push_back(d[i]);
}
static void on_fdb(uint32_t n, const sai_fdb_event_notification_data_t* d)
{
    std::lock_guard<std::mutex> g(g_nmu);
    for (uint32_t i = 0; i < n; i++) {
        sai_object_id_t bp = 0;
        for (uint32_t a = 0; a < d[i].attr_count; a++)
            if (d[i].attr[a].id == SAI_FDB_ENTRY_ATTR_BRIDGE_PORT_ID) bp = d[i].attr[a].value.oid;
        g_fdb_events.push_back({d[i].event_type, bp});
    }
}
static bool wait_for(std::function<bool()> f)
{
    for (int i = 0; i < 200; i++) {
        { std::lock_guard<std::mutex> g(g_nmu); if (f()) return true; }
        std::this_thread::sleep_for(std::chrono::milliseconds(10));
    }
    return false;
}

// ---------------------------------------------------------------- APIs
static sai_switch_api_t* sw;
static sai_port_api_t* pa;
static sai_vlan_api_t* va;
static sai_bridge_api_t* ba;
static sai_fdb_api_t* fa;
static sai_hostif_api_t* ha;
static sai_router_interface_api_t* ra;
static sai_neighbor_api_t* na;
static sai_next_hop_api_t* nha;
static sai_route_api_t* rta;
static sai_queue_api_t* qa;
static sai_stp_api_t* sta;
static sai_lag_api_t* la;
static sai_virtual_router_api_t* vra;
static sai_next_hop_group_api_t* nga;

static void query_apis()
{
    OK(sai_api_query(SAI_API_SWITCH, (void**)&sw));
    OK(sai_api_query(SAI_API_PORT, (void**)&pa));
    OK(sai_api_query(SAI_API_VLAN, (void**)&va));
    OK(sai_api_query(SAI_API_BRIDGE, (void**)&ba));
    OK(sai_api_query(SAI_API_FDB, (void**)&fa));
    OK(sai_api_query(SAI_API_HOSTIF, (void**)&ha));
    OK(sai_api_query(SAI_API_ROUTER_INTERFACE, (void**)&ra));
    OK(sai_api_query(SAI_API_NEIGHBOR, (void**)&na));
    OK(sai_api_query(SAI_API_NEXT_HOP, (void**)&nha));
    OK(sai_api_query(SAI_API_ROUTE, (void**)&rta));
    OK(sai_api_query(SAI_API_QUEUE, (void**)&qa));
    OK(sai_api_query(SAI_API_STP, (void**)&sta));
    OK(sai_api_query(SAI_API_LAG, (void**)&la));
    OK(sai_api_query(SAI_API_VIRTUAL_ROUTER, (void**)&vra));
    OK(sai_api_query(SAI_API_NEXT_HOP_GROUP, (void**)&nga));
}

// ------------------------------------------------- test-port helpers (tp:: only)
// SAI's attribute-indexed status: the code of attribute i of a call.
static sai_status_t attr_status(sai_status_t code0, uint32_t i) { return code0 - (sai_status_t)i; }
template <typename T> static T first_of(const std::vector<T>& v) { return v.empty() ? T{} : v[0]; }
static bool vmember(uint16_t vid, int hw)
{
    for (auto& m : tp::vlan_members(vid))
        if (m.hw == hw) return true;
    return false;
}
static bool vuntagged(uint16_t vid, int hw)
{
    for (auto& m : tp::vlan_members(vid))
        if (m.hw == hw) return m.untagged;
    return false;
}
static bool l2_find(const uint8_t mac[6], uint16_t vid, tp::L2Entry& out)
{
    for (auto& e : tp::l2_entries())
        if (e.vid == vid && std::memcmp(e.mac, mac, 6) == 0) { out = e; return true; }
    return false;
}
static tp::Egress egr_of(int id)
{
    tp::Egress e;
    tp::egress(id, e);
    return e;
}
static tp::L3Intf intf_of(int id)
{
    for (auto& i : tp::l3_intfs())
        if (i.id == id) return i;
    return tp::L3Intf{};
}
static std::vector<int> ecmp_members(int id)
{
    for (auto& g : tp::ecmp_groups())
        if (g.id == id) return g.egress;
    return {};
}
static bool lag_exists(int tid)
{
    std::vector<tp::LagMember> v;
    return tp::lag_members(tid, v);
}
static std::vector<tp::LagMember> lagm(int tid)
{
    std::vector<tp::LagMember> v;
    tp::lag_members(tid, v);
    return v;
}
static tp::LagMember lag0(int tid)
{
    std::vector<tp::LagMember> v = lagm(tid);
    return v.empty() ? tp::LagMember{-1, false, false} : v[0];
}
// A hardware learn / age event as the backend raises it.
struct FdbEv {
    bool learned = true;
    uint8_t mac[6] = {0};
    uint16_t vid = 0;
    int hw_port = -1;   // dest
};
static void inject_fdb(const FdbEv& e) { tp::inject_fdb(e.learned, e.mac, e.vid, e.hw_port); }

static sai_object_id_t g_sw;
static std::vector<sai_object_id_t> g_ports;

static sai_status_t create_switch_default()
{
    sai_attribute_t at[3];
    at[0].id = SAI_SWITCH_ATTR_INIT_SWITCH;
    at[0].value.booldata = true;
    at[1].id = SAI_SWITCH_ATTR_PORT_STATE_CHANGE_NOTIFY;
    at[1].value.ptr = (void*)on_port;
    at[2].id = SAI_SWITCH_ATTR_FDB_EVENT_NOTIFY;
    at[2].value.ptr = (void*)on_fdb;
    return sw->create_switch(&g_sw, 3, at);
}

static sai_object_id_t get_oid(sai_object_id_t sw_id, sai_attr_id_t id)
{
    sai_attribute_t a;
    a.id = id;
    a.value.oid = 0;
    CHECK_EQ(sw->get_switch_attribute(sw_id, 1, &a), SAI_STATUS_SUCCESS);
    return a.value.oid;
}

// SWITCH attributes read without counting a check.
static sai_object_id_t switch_oid_attr(sai_attr_id_t id)
{
    sai_attribute_t a;
    a.id = id;
    a.value.oid = 0;
    sw->get_switch_attribute(g_sw, 1, &a);
    return a.value.oid;
}
static std::vector<uint8_t> switch_mac()
{
    sai_attribute_t a;
    std::memset(&a, 0, sizeof(a));
    a.id = SAI_SWITCH_ATTR_SRC_MAC_ADDRESS;
    sw->get_switch_attribute(g_sw, 1, &a);
    return std::vector<uint8_t>(a.value.mac, a.value.mac + 6);
}

static sai_object_id_t bport_of(sai_object_id_t port)
{
    sai_object_id_t br = get_oid(g_sw, SAI_SWITCH_ATTR_DEFAULT_1Q_BRIDGE_ID);
    sai_object_id_t list[64];
    sai_attribute_t a;
    a.id = SAI_BRIDGE_ATTR_PORT_LIST;
    a.value.objlist.count = 64;
    a.value.objlist.list = list;
    if (ba->get_bridge_attribute(br, 1, &a) != SAI_STATUS_SUCCESS) return 0;
    for (uint32_t i = 0; i < a.value.objlist.count; i++) {
        sai_attribute_t p;
        p.id = SAI_BRIDGE_PORT_ATTR_PORT_ID;
        if (ba->get_bridge_port_attribute(list[i], 1, &p) == SAI_STATUS_SUCCESS && p.value.oid == port) return list[i];
    }
    return 0;
}

// ================================================================ tests
static void test_oid_encoding()
{
    sai_object_id_t o = tp::make_oid(SAI_OBJECT_TYPE_PORT, 50, 3);
    CHECK_EQ(tp::oid_type(o), SAI_OBJECT_TYPE_PORT);
    CHECK_EQ(tp::oid_index(o), 50u);
    CHECK_EQ(tp::oid_switch(o), 3u);
    CHECK_EQ(sai_object_type_query(o), SAI_OBJECT_TYPE_PORT);
    CHECK_EQ(sai_switch_id_query(o), tp::make_oid(SAI_OBJECT_TYPE_SWITCH, 1, 3));
    CHECK_EQ(sai_object_type_query(SAI_NULL_OBJECT_ID), SAI_OBJECT_TYPE_NULL);
    CHECK_EQ(sai_switch_id_query(SAI_NULL_OBJECT_ID), (sai_object_id_t)SAI_NULL_OBJECT_ID);
    CHECK_EQ(sai_object_type_query(0xff00000000000001ull), SAI_OBJECT_TYPE_NULL);   // type out of range
    sai_object_id_t big = tp::make_oid(SAI_OBJECT_TYPE_FDB_FLUSH, tp::oid_index_mask(), 255);
    CHECK_EQ(tp::oid_type(big), SAI_OBJECT_TYPE_FDB_FLUSH);
    CHECK_EQ(tp::oid_index(big), tp::oid_index_mask());
    CHECK_EQ(tp::oid_switch(big), 255u);
    // (the library's attribute-indexed status helpers: tests/test_units.cpp)
}

static void test_init_and_backend_selection()
{
    void* t = nullptr;
    CHECK_EQ(sai_api_query(SAI_API_SWITCH, &t), SAI_STATUS_UNINITIALIZED);
    CHECK_EQ(sai_api_initialize(0, nullptr), SAI_STATUS_INVALID_PARAMETER);
    g_profile.clear();
    OK(sai_api_initialize(0, &g_smt));
    CHECK_EQ(sai_api_initialize(0, &g_smt), SAI_STATUS_FAILURE);
    CHECK_EQ(sai_api_query(SAI_API_SWITCH, nullptr), SAI_STATUS_INVALID_PARAMETER);
    CHECK_EQ(sai_api_query(SAI_API_TUNNEL, &t), SAI_STATUS_NOT_IMPLEMENTED);
    query_apis();
    sai_api_version_t v = 0;
    OK(sai_query_api_version(&v));
    CHECK(v >= SAI_VERSION(1, 9, 0));
    OK(sai_log_set(SAI_API_PORT, SAI_LOG_LEVEL_DEBUG));
    CHECK_EQ(sai_log_set((sai_api_t)9999, SAI_LOG_LEVEL_DEBUG), SAI_STATUS_INVALID_PARAMETER);
    OK(sai_log_set(SAI_API_PORT, SAI_LOG_LEVEL_WARN));

    // No backend key: a fake-only build must refuse instead of silently faking.
    sai_attribute_t a;
    a.id = SAI_SWITCH_ATTR_INIT_SWITCH;
    a.value.booldata = true;
    sai_object_id_t s = 0;
    if (!tp::has_hardware_backend()) {
        CHECK(sw->create_switch(&s, 1, &a) != SAI_STATUS_SUCCESS);
        g_profile["SAI_OPENBCM_BACKEND"] = "openbcm";
        CHECK(sw->create_switch(&s, 1, &a) != SAI_STATUS_SUCCESS);
    }
    g_profile["SAI_OPENBCM_BACKEND"] = "bogus";
    CHECK(sw->create_switch(&s, 1, &a) != SAI_STATUS_SUCCESS);
    g_profile["SAI_OPENBCM_BACKEND"] = "fake";
    // Fake netdev / wire keys: bad values refuse the switch (TAP mode itself
    // needs CAP_NET_ADMIN and is exercised by the NixOS VM test).
    for (auto kv : std::vector<std::pair<std::string, std::string>>{
             {"SAI_OPENBCM_FAKE_NETDEV", "bogus"},
             {"SAI_OPENBCM_FAKE_WIRE", "Ethernet0:eth1"},          // a wire needs NETDEV=tap
             {"SAI_OPENBCM_FAKE_WIRE", "Ethernet0"},
             {"SAI_OPENBCM_FAKE_WIRE", "Ethernet0:eth1,Ethernet4:eth1"}}) {
        g_profile[kv.first] = kv.second;
        CHECK_EQ(sw->create_switch(&s, 1, &a), SAI_STATUS_INVALID_PARAMETER);
        CHECK(!tp::switch_created());
        g_profile.erase(kv.first);
    }
    a.value.booldata = false;
    CHECK_EQ(sw->create_switch(&s, 1, &a), SAI_STATUS_NOT_SUPPORTED);
    CHECK_EQ(sw->create_switch(&s, 0, nullptr), SAI_STATUS_MANDATORY_ATTRIBUTE_MISSING);
    CHECK(!tp::switch_created());
    // Calls before a switch exists fail cleanly.
    sai_attribute_t q;
    q.id = SAI_PORT_ATTR_SPEED;
    CHECK_EQ(pa->get_port_attribute(tp::make_oid(SAI_OBJECT_TYPE_PORT, 1), 1, &q), SAI_STATUS_UNINITIALIZED);
}

static void test_switch_create()
{
    g_profile["SAI_OPENBCM_BACKEND"] = "fake";
    g_profile["SAI_OPENBCM_RC_FILE"] = "/nonexistent.soc";
    OK(create_switch_default());
    CHECK(tp::switch_created());
    CHECK_EQ(sai_object_type_query(g_sw), SAI_OBJECT_TYPE_SWITCH);
    sai_object_id_t again = 0;
    sai_attribute_t a;
    a.id = SAI_SWITCH_ATTR_INIT_SWITCH;
    a.value.booldata = true;
    CHECK(sw->create_switch(&again, 1, &a) != SAI_STATUS_SUCCESS);

    a.id = SAI_SWITCH_ATTR_PORT_NUMBER;
    OK(sw->get_switch_attribute(g_sw, 1, &a));
    CHECK_EQ(a.value.u32, 32u);

    // PORT_LIST: buffer overflow semantics, then the real list.
    sai_object_id_t small[4];
    a.id = SAI_SWITCH_ATTR_PORT_LIST;
    a.value.objlist.count = 4;
    a.value.objlist.list = small;
    CHECK_EQ(sw->get_switch_attribute(g_sw, 1, &a), SAI_STATUS_BUFFER_OVERFLOW);
    CHECK_EQ(a.value.objlist.count, 32u);
    g_ports.assign(32, 0);
    a.value.objlist.count = 32;
    a.value.objlist.list = g_ports.data();
    OK(sw->get_switch_attribute(g_sw, 1, &a));
    for (auto p : g_ports) CHECK_EQ(sai_object_type_query(p), SAI_OBJECT_TYPE_PORT);

    sai_attr_id_t defaults[] = {SAI_SWITCH_ATTR_CPU_PORT, SAI_SWITCH_ATTR_DEFAULT_VIRTUAL_ROUTER_ID,
                                SAI_SWITCH_ATTR_DEFAULT_VLAN_ID, SAI_SWITCH_ATTR_DEFAULT_1Q_BRIDGE_ID,
                                SAI_SWITCH_ATTR_DEFAULT_STP_INST_ID, SAI_SWITCH_ATTR_DEFAULT_TRAP_GROUP};
    sai_object_type_t types[] = {SAI_OBJECT_TYPE_PORT, SAI_OBJECT_TYPE_VIRTUAL_ROUTER, SAI_OBJECT_TYPE_VLAN,
                                 SAI_OBJECT_TYPE_BRIDGE, SAI_OBJECT_TYPE_STP, SAI_OBJECT_TYPE_HOSTIF_TRAP_GROUP};
    for (int i = 0; i < 6; i++) CHECK_EQ(sai_object_type_query(get_oid(g_sw, defaults[i])), types[i]);

    // multi-attribute get: failure index is encoded in the status
    sai_attribute_t two[2];
    two[0].id = SAI_SWITCH_ATTR_PORT_NUMBER;
    two[1].id = SAI_SWITCH_ATTR_ECMP_HASH;
    CHECK_EQ(sw->get_switch_attribute(g_sw, 2, two), attr_status(SAI_STATUS_ATTR_NOT_SUPPORTED_0, 1));

    a.id = SAI_SWITCH_ATTR_SRC_MAC_ADDRESS;
    sai_mac_t m = {0x02, 0x11, 0x22, 0x33, 0x44, 0x55};
    std::memcpy(a.value.mac, m, 6);
    OK(sw->set_switch_attribute(g_sw, &a));
    std::memset(a.value.mac, 0, 6);
    OK(sw->get_switch_attribute(g_sw, 1, &a));
    CHECK(std::memcmp(a.value.mac, m, 6) == 0);

    a.id = SAI_SWITCH_ATTR_FDB_AGING_TIME;
    a.value.u32 = 600;
    OK(sw->set_switch_attribute(g_sw, &a));
    CHECK_EQ(tp::l2_age_seconds(), 600u);
    a.id = SAI_SWITCH_ATTR_ECMP_DEFAULT_HASH_SEED;   // stubbed: stored
    a.value.u32 = 1234;
    OK(sw->set_switch_attribute(g_sw, &a));
    a.value.u32 = 0;
    OK(sw->get_switch_attribute(g_sw, 1, &a));
    CHECK_EQ(a.value.u32, 1234u);
    a.id = SAI_SWITCH_ATTR_ECMP_HASH;
    CHECK(SAI_STATUS_IS_ATTR_NOT_SUPPORTED(sw->set_switch_attribute(g_sw, &a)));
    CHECK_EQ(sw->get_switch_attribute(0x1234, 1, &a), SAI_STATUS_INVALID_OBJECT_ID);

    // rc script handed to the backend
    CHECK_EQ(tp::shell_log().size(), 0u);   // fake backend ignores SAI_OPENBCM_RC_FILE (openbcm runs it)
}

static void test_ports()
{
    sai_object_id_t p0 = g_ports[0];
    sai_attribute_t a;
    uint32_t lanes[8];
    a.id = SAI_PORT_ATTR_HW_LANE_LIST;
    a.value.u32list.count = 8;
    a.value.u32list.list = lanes;
    OK(pa->get_port_attribute(p0, 1, &a));
    CHECK_EQ(a.value.u32list.count, 4u);
    CHECK_EQ(lanes[0], 1u);
    CHECK_EQ(lanes[3], 4u);
    a.value.u32list.count = 2;
    CHECK_EQ(pa->get_port_attribute(p0, 1, &a), SAI_STATUS_BUFFER_OVERFLOW);
    CHECK_EQ(a.value.u32list.count, 4u);

    // Safety rule: ports start admin-down, both in SAI and in the backend.
    a.id = SAI_PORT_ATTR_ADMIN_STATE;
    OK(pa->get_port_attribute(p0, 1, &a));
    CHECK(!a.value.booldata);
    for (auto& p : tp::ports()) CHECK(!p.admin);

    a.id = SAI_PORT_ATTR_SPEED;
    OK(pa->get_port_attribute(p0, 1, &a));
    CHECK_EQ(a.value.u32, 100000u);
    a.value.u32 = 40000;
    OK(pa->set_port_attribute(p0, &a));
    OK(pa->get_port_attribute(p0, 1, &a));
    CHECK_EQ(a.value.u32, 40000u);
    a.value.u32 = 25000;
    CHECK(SAI_STATUS_IS_ATTR_NOT_SUPPORTED(pa->set_port_attribute(p0, &a)));
    a.value.u32 = 100000;
    OK(pa->set_port_attribute(p0, &a));

    uint32_t sp[8];
    a.id = SAI_PORT_ATTR_SUPPORTED_SPEED;
    a.value.u32list.count = 8;
    a.value.u32list.list = sp;
    OK(pa->get_port_attribute(p0, 1, &a));
    CHECK_EQ(a.value.u32list.count, 2u);

    a.id = SAI_PORT_ATTR_MTU;
    a.value.u32 = 9100;
    OK(pa->set_port_attribute(p0, &a));
    CHECK_EQ(tp::port(1).frame_max, 9108u);
    a.value.u32 = 20000;
    CHECK(SAI_STATUS_IS_INVALID_ATTR_VALUE(pa->set_port_attribute(p0, &a)));

    a.id = SAI_PORT_ATTR_FEC_MODE;
    a.value.s32 = SAI_PORT_FEC_MODE_RS;
    OK(pa->set_port_attribute(p0, &a));
    CHECK(tp::port(1).fec == SAI_PORT_FEC_MODE_RS);
    a.id = SAI_PORT_ATTR_AUTO_NEG_MODE;
    a.value.booldata = true;
    OK(pa->set_port_attribute(p0, &a));
    CHECK(tp::port(1).autoneg);

    // Admin up -> link up in the fake -> asynchronous port notification.
    a.id = SAI_PORT_ATTR_OPER_STATUS;
    OK(pa->get_port_attribute(p0, 1, &a));
    CHECK_EQ(a.value.s32, SAI_PORT_OPER_STATUS_DOWN);
    a.id = SAI_PORT_ATTR_ADMIN_STATE;
    a.value.booldata = true;
    OK(pa->set_port_attribute(p0, &a));
    CHECK(tp::port(1).admin);
    CHECK(wait_for([&] { return !g_port_events.empty(); }));
    {
        std::lock_guard<std::mutex> g(g_nmu);
        CHECK_EQ(g_port_events.size(), 1u);
        CHECK_EQ(g_port_events[0].port_id, p0);
        CHECK_EQ(g_port_events[0].port_state, SAI_PORT_OPER_STATUS_UP);
        g_port_events.clear();
    }
    a.id = SAI_PORT_ATTR_OPER_STATUS;
    OK(pa->get_port_attribute(p0, 1, &a));
    CHECK_EQ(a.value.s32, SAI_PORT_OPER_STATUS_UP);
    tp::inject_link(1, false);   // cable pulled
    CHECK(wait_for([&] { return !g_port_events.empty(); }));
    {
        std::lock_guard<std::mutex> g(g_nmu);
        CHECK_EQ(g_port_events[0].port_state, SAI_PORT_OPER_STATUS_DOWN);
        g_port_events.clear();
    }
    a.id = SAI_PORT_ATTR_ADMIN_STATE;
    a.value.booldata = false;
    OK(pa->set_port_attribute(p0, &a));

    // Loopback on an admin-up port brings the link up even with FAKE_LINK=none semantics.
    a.id = SAI_PORT_ATTR_LOOPBACK_MODE;
    a.value.s32 = SAI_PORT_LOOPBACK_MODE_MAC;
    OK(pa->set_port_attribute(g_ports[1], &a));
    CHECK(tp::port(5).loopback == SAI_PORT_LOOPBACK_MODE_MAC);

    // CPU port
    sai_object_id_t cpu = get_oid(g_sw, SAI_SWITCH_ATTR_CPU_PORT);
    a.id = SAI_PORT_ATTR_TYPE;
    OK(pa->get_port_attribute(cpu, 1, &a));
    CHECK_EQ(a.value.s32, SAI_PORT_TYPE_CPU);
    a.id = SAI_PORT_ATTR_ADMIN_STATE;
    a.value.booldata = false;
    CHECK(pa->set_port_attribute(cpu, &a) != SAI_STATUS_SUCCESS);

    // queues: 8 per port, discoverable
    sai_object_id_t qs[16];
    a.id = SAI_PORT_ATTR_QOS_QUEUE_LIST;
    a.value.objlist.count = 16;
    a.value.objlist.list = qs;
    OK(pa->get_port_attribute(p0, 1, &a));
    CHECK_EQ(a.value.objlist.count, 8u);
    sai_attribute_t qa1[2];
    qa1[0].id = SAI_QUEUE_ATTR_PORT;
    qa1[1].id = SAI_QUEUE_ATTR_INDEX;
    OK(qa->get_queue_attribute(qs[3], 2, qa1));
    CHECK_EQ(qa1[0].value.oid, p0);
    CHECK_EQ(qa1[1].value.u8, 3);

    // stats
    tp::set_port_stat(1, SAI_PORT_STAT_IF_IN_OCTETS, 1000);
    tp::set_port_stat(1, SAI_PORT_STAT_IF_OUT_UCAST_PKTS, 7);
    sai_stat_id_t ids[2] = {SAI_PORT_STAT_IF_IN_OCTETS, SAI_PORT_STAT_IF_OUT_UCAST_PKTS};
    uint64_t vals[2] = {0, 0};
    OK(pa->get_port_stats(p0, 2, ids, vals));
    CHECK_EQ(vals[0], 1000u);
    CHECK_EQ(vals[1], 7u);
    sai_stat_id_t bad = SAI_PORT_STAT_PFC_0_RX_PKTS;
    CHECK_EQ(pa->get_port_stats(p0, 1, &bad, vals), SAI_STATUS_NOT_SUPPORTED);
    OK(pa->clear_port_all_stats(p0));
    OK(pa->get_port_stats(p0, 2, ids, vals));
    CHECK_EQ(vals[0], 0u);
    sai_stat_capability_t caps[64];
    sai_stat_capability_list_t cl;
    cl.count = 64;
    cl.list = caps;
    OK(sai_query_stats_capability(g_sw, SAI_OBJECT_TYPE_PORT, &cl));
    CHECK_EQ(cl.count, 15u);

    // bad input never crashes
    CHECK_EQ(pa->get_port_attribute(tp::make_oid(SAI_OBJECT_TYPE_PORT, 999), 1, &a), SAI_STATUS_INVALID_OBJECT_ID);
    CHECK_EQ(pa->get_port_attribute(g_ports[0], 1, nullptr), SAI_STATUS_INVALID_PARAMETER);
    CHECK_EQ(pa->set_port_attribute(g_ports[0], nullptr), SAI_STATUS_INVALID_PARAMETER);
    CHECK_EQ(pa->get_port_attribute(g_sw, 1, &a), SAI_STATUS_INVALID_OBJECT_ID);
    a.id = 0x7fffffff;
    CHECK(SAI_STATUS_IS_ATTR_NOT_SUPPORTED(pa->get_port_attribute(g_ports[0], 1, &a)));
    sai_object_id_t dummy;
    CHECK_EQ(pa->create_port(&dummy, g_sw, 0, nullptr), SAI_STATUS_NOT_SUPPORTED);
}

static void test_vlan_bridge()
{
    sai_object_id_t vlan1 = get_oid(g_sw, SAI_SWITCH_ATTR_DEFAULT_VLAN_ID);
    sai_object_id_t members[64];
    sai_attribute_t a;
    a.id = SAI_VLAN_ATTR_MEMBER_LIST;
    a.value.objlist.count = 64;
    a.value.objlist.list = members;
    OK(va->get_vlan_attribute(vlan1, 1, &a));
    CHECK_EQ(a.value.objlist.count, 32u);   // SDK default: every front-panel port in VLAN 1

    // remove port0 from VLAN 1 (what orchagent does at init)
    sai_object_id_t bp0 = bport_of(g_ports[0]);
    CHECK(bp0 != 0);
    sai_object_id_t m0 = 0;
    for (uint32_t i = 0; i < a.value.objlist.count; i++) {
        sai_attribute_t b;
        b.id = SAI_VLAN_MEMBER_ATTR_BRIDGE_PORT_ID;
        OK(va->get_vlan_member_attribute(members[i], 1, &b));
        if (b.value.oid == bp0) m0 = members[i];
    }
    CHECK(m0 != 0);
    CHECK_EQ(ba->remove_bridge_port(bp0), SAI_STATUS_OBJECT_IN_USE);   // still a VLAN member
    OK(va->remove_vlan_member(m0));
    CHECK(!vmember(1, 1));
    CHECK(vmember(1, 0));   // CPU stays

    // bridge port re-created with learning disabled
    OK(ba->remove_bridge_port(bp0));
    sai_attribute_t bpa[4];
    bpa[0].id = SAI_BRIDGE_PORT_ATTR_TYPE;
    bpa[0].value.s32 = SAI_BRIDGE_PORT_TYPE_PORT;
    bpa[1].id = SAI_BRIDGE_PORT_ATTR_PORT_ID;
    bpa[1].value.oid = g_ports[0];
    bpa[2].id = SAI_BRIDGE_PORT_ATTR_ADMIN_STATE;
    bpa[2].value.booldata = true;
    bpa[3].id = SAI_BRIDGE_PORT_ATTR_FDB_LEARNING_MODE;
    bpa[3].value.s32 = SAI_BRIDGE_PORT_FDB_LEARNING_MODE_DISABLE;
    OK(ba->create_bridge_port(&bp0, g_sw, 4, bpa));
    CHECK(tp::port(1).learn == tp::LEARN_DISABLE);
    sai_object_id_t dup;
    CHECK_EQ(ba->create_bridge_port(&dup, g_sw, 4, bpa), SAI_STATUS_ITEM_ALREADY_EXISTS);
    bpa[0].value.s32 = SAI_BRIDGE_PORT_TYPE_SUB_PORT;
    CHECK(SAI_STATUS_IS_ATTR_NOT_SUPPORTED(ba->create_bridge_port(&dup, g_sw, 4, bpa)));

    // VLAN 100
    sai_object_id_t v100;
    a.id = SAI_VLAN_ATTR_VLAN_ID;
    a.value.u16 = 100;
    OK(va->create_vlan(&v100, g_sw, 1, &a));
    CHECK_EQ(sai_object_type_query(v100), SAI_OBJECT_TYPE_VLAN);
    CHECK(tp::vlan_exists(100));
    CHECK(vmember(100, 0));   // CPU member, so floods reach KNET
    CHECK_EQ(va->create_vlan(&dup, g_sw, 1, &a), SAI_STATUS_ITEM_ALREADY_EXISTS);
    a.value.u16 = 4095;
    CHECK(SAI_STATUS_IS_INVALID_ATTR_VALUE(va->create_vlan(&dup, g_sw, 1, &a)));
    CHECK_EQ(va->create_vlan(&dup, g_sw, 0, nullptr), SAI_STATUS_MANDATORY_ATTRIBUTE_MISSING);

    sai_attribute_t ma[3];
    ma[0].id = SAI_VLAN_MEMBER_ATTR_VLAN_ID;
    ma[0].value.oid = v100;
    ma[1].id = SAI_VLAN_MEMBER_ATTR_BRIDGE_PORT_ID;
    ma[1].value.oid = bp0;
    ma[2].id = SAI_VLAN_MEMBER_ATTR_VLAN_TAGGING_MODE;
    ma[2].value.s32 = SAI_VLAN_TAGGING_MODE_TAGGED;
    sai_object_id_t m100;
    OK(va->create_vlan_member(&m100, g_sw, 3, ma));
    CHECK(vmember(100, 1) && !vuntagged(100, 1));
    CHECK_EQ(va->create_vlan_member(&dup, g_sw, 3, ma), SAI_STATUS_ITEM_ALREADY_EXISTS);
    a.id = SAI_VLAN_MEMBER_ATTR_VLAN_TAGGING_MODE;
    a.value.s32 = SAI_VLAN_TAGGING_MODE_UNTAGGED;
    OK(va->set_vlan_member_attribute(m100, &a));
    CHECK(vuntagged(100, 1));
    a.id = SAI_PORT_ATTR_PORT_VLAN_ID;
    a.value.u16 = 100;
    OK(pa->set_port_attribute(g_ports[0], &a));
    CHECK_EQ(tp::port(1).pvid, 100);

    CHECK_EQ(va->remove_vlan(v100), SAI_STATUS_OBJECT_IN_USE);
    CHECK_EQ(va->remove_vlan(vlan1), SAI_STATUS_OBJECT_IN_USE);
    OK(va->remove_vlan_member(m100));
    OK(va->remove_vlan(v100));
    CHECK(!tp::vlan_exists(100));
    CHECK_EQ(va->remove_vlan(v100), SAI_STATUS_INVALID_OBJECT_ID);

    // STP default instance lists the VLANs
    sai_vlan_id_t vids[8];
    a.id = SAI_STP_ATTR_VLAN_LIST;
    a.value.vlanlist.count = 8;
    a.value.vlanlist.list = vids;
    OK(sta->get_stp_attribute(get_oid(g_sw, SAI_SWITCH_ATTR_DEFAULT_STP_INST_ID), 1, &a));
    CHECK_EQ(a.value.vlanlist.count, 1u);
}

static sai_fdb_entry_t fdb_key(sai_object_id_t vlan, uint8_t last)
{
    sai_fdb_entry_t e;
    std::memset(&e, 0, sizeof(e));
    e.switch_id = g_sw;
    e.bv_id = vlan;
    uint8_t m[6] = {0x00, 0x11, 0x22, 0x33, 0x44, last};
    std::memcpy(e.mac_address, m, 6);
    return e;
}

static void test_fdb()
{
    sai_object_id_t vlan1 = get_oid(g_sw, SAI_SWITCH_ATTR_DEFAULT_VLAN_ID);
    sai_object_id_t bp1 = bport_of(g_ports[1]);
    sai_fdb_entry_t e = fdb_key(vlan1, 0x55);
    sai_attribute_t at[3];
    at[0].id = SAI_FDB_ENTRY_ATTR_TYPE;
    at[0].value.s32 = SAI_FDB_ENTRY_TYPE_STATIC;
    at[1].id = SAI_FDB_ENTRY_ATTR_BRIDGE_PORT_ID;
    at[1].value.oid = bp1;
    at[2].id = SAI_FDB_ENTRY_ATTR_PACKET_ACTION;
    at[2].value.s32 = SAI_PACKET_ACTION_FORWARD;
    OK(fa->create_fdb_entry(&e, 3, at));
    CHECK_EQ(fa->create_fdb_entry(&e, 3, at), SAI_STATUS_ITEM_ALREADY_EXISTS);
    {
        tp::L2Entry le;
        bool found = l2_find(e.mac_address, 1, le);
        CHECK(found);
        if (found) { CHECK_EQ(le.dest, 5); CHECK(le.is_static); }
    }
    sai_attribute_t g;
    g.id = SAI_FDB_ENTRY_ATTR_BRIDGE_PORT_ID;
    OK(fa->get_fdb_entry_attribute(&e, 1, &g));
    CHECK_EQ(g.value.oid, bp1);

    sai_fdb_entry_t d = fdb_key(vlan1, 0x66);
    at[0].value.s32 = SAI_FDB_ENTRY_TYPE_DYNAMIC;
    OK(fa->create_fdb_entry(&d, 3, at));
    CHECK_EQ(fa->create_fdb_entry(&d, 0, nullptr), SAI_STATUS_MANDATORY_ATTRIBUTE_MISSING);

    // flush dynamic on the port: static stays
    sai_attribute_t fl[2];
    fl[0].id = SAI_FDB_FLUSH_ATTR_BRIDGE_PORT_ID;
    fl[0].value.oid = bp1;
    fl[1].id = SAI_FDB_FLUSH_ATTR_ENTRY_TYPE;
    fl[1].value.s32 = SAI_FDB_FLUSH_ENTRY_TYPE_DYNAMIC;
    OK(fa->flush_fdb_entries(g_sw, 2, fl));
    CHECK_EQ(fa->get_fdb_entry_attribute(&d, 1, &g), SAI_STATUS_ITEM_NOT_FOUND);
    OK(fa->get_fdb_entry_attribute(&e, 1, &g));
    CHECK_EQ(tp::l2_entries().size(), 1u);
    OK(fa->remove_fdb_entry(&e));
    CHECK_EQ(tp::l2_entries().size(), 0u);
    CHECK_EQ(fa->remove_fdb_entry(&e), SAI_STATUS_ITEM_NOT_FOUND);
    // .1D bridge keys are rejected, unknown VLANs too
    sai_fdb_entry_t bad = fdb_key(get_oid(g_sw, SAI_SWITCH_ATTR_DEFAULT_1Q_BRIDGE_ID), 1);
    CHECK_EQ(fa->create_fdb_entry(&bad, 3, at), SAI_STATUS_NOT_SUPPORTED);
    bad = fdb_key(tp::make_oid(SAI_OBJECT_TYPE_VLAN, 999), 1);
    CHECK_EQ(fa->create_fdb_entry(&bad, 3, at), SAI_STATUS_INVALID_PARAMETER);

    // hardware learning -> FDB_EVENT notification with the bridge port
    FdbEv ev;
    uint8_t mac[6] = {0x00, 0xaa, 0xbb, 0xcc, 0xdd, 0x01};
    std::memcpy(ev.mac, mac, 6);
    ev.vid = 1;
    ev.hw_port = 9;
    ev.learned = true;
    inject_fdb(ev);
    CHECK(wait_for([&] { return !g_fdb_events.empty(); }));
    {
        std::lock_guard<std::mutex> l(g_nmu);
        CHECK_EQ(g_fdb_events[0].first, SAI_FDB_EVENT_LEARNED);
        CHECK_EQ(g_fdb_events[0].second, bport_of(g_ports[2]));
        g_fdb_events.clear();
    }
    sai_fdb_entry_t le = fdb_key(vlan1, 0);
    std::memcpy(le.mac_address, mac, 6);
    g.id = SAI_FDB_ENTRY_ATTR_TYPE;
    OK(fa->get_fdb_entry_attribute(&le, 1, &g));
    CHECK_EQ(g.value.s32, SAI_FDB_ENTRY_TYPE_DYNAMIC);
    ev.learned = false;
    inject_fdb(ev);
    CHECK(wait_for([&] { return !g_fdb_events.empty(); }));
    {
        std::lock_guard<std::mutex> l(g_nmu);
        CHECK_EQ(g_fdb_events[0].first, SAI_FDB_EVENT_AGED);
        g_fdb_events.clear();
    }
    CHECK_EQ(fa->get_fdb_entry_attribute(&le, 1, &g), SAI_STATUS_ITEM_NOT_FOUND);

    // An entry created through SAI and then reported by the SDK's learn
    // callback is not announced again; a move to another port is.
    sai_fdb_entry_t own = fdb_key(vlan1, 0x77);
    at[0].value.s32 = SAI_FDB_ENTRY_TYPE_DYNAMIC;
    at[1].value.oid = bp1;
    OK(fa->create_fdb_entry(&own, 3, at));
    std::memcpy(ev.mac, own.mac_address, 6);
    ev.learned = true;
    ev.hw_port = 5;
    inject_fdb(ev);
    ev.hw_port = 9;
    inject_fdb(ev);
    CHECK(wait_for([&] { return !g_fdb_events.empty(); }));
    {
        std::lock_guard<std::mutex> l(g_nmu);
        CHECK_EQ(g_fdb_events.size(), 1u);
        CHECK_EQ(g_fdb_events[0].first, SAI_FDB_EVENT_MOVE);
        g_fdb_events.clear();
    }
    OK(fa->remove_fdb_entry(&own));
}

static void name_attr(sai_attribute_t& a, const char* n)
{
    a.id = SAI_HOSTIF_ATTR_NAME;
    std::memset(a.value.chardata, 0, sizeof(a.value.chardata));
    std::strncpy(a.value.chardata, n, SAI_HOSTIF_NAME_SIZE - 1);
}

static void test_hostif()
{
    sai_attribute_t at[3];
    at[0].id = SAI_HOSTIF_ATTR_TYPE;
    at[0].value.s32 = SAI_HOSTIF_TYPE_NETDEV;
    at[1].id = SAI_HOSTIF_ATTR_OBJ_ID;
    at[1].value.oid = g_ports[0];
    name_attr(at[2], "Ethernet0");
    sai_object_id_t h;
    OK(ha->create_hostif(&h, g_sw, 3, at));
    CHECK_EQ(tp::netifs().size(), 1u);
    tp::Netif n = first_of(tp::netifs());
    CHECK(n.name == "Ethernet0");
    CHECK_EQ(n.hw, 1);
    CHECK(!n.keep_tag);
    CHECK(std::memcmp(n.mac, switch_mac().data(), 6) == 0);
    sai_object_id_t dup;
    CHECK_EQ(ha->create_hostif(&dup, g_sw, 3, at), SAI_STATUS_ITEM_ALREADY_EXISTS);
    at[1].value.oid = get_oid(g_sw, SAI_SWITCH_ATTR_CPU_PORT);
    name_attr(at[2], "cpu");
    CHECK_EQ(ha->create_hostif(&dup, g_sw, 3, at), SAI_STATUS_NOT_SUPPORTED);
    at[0].value.s32 = SAI_HOSTIF_TYPE_GENETLINK;
    CHECK(SAI_STATUS_IS_ATTR_NOT_SUPPORTED(ha->create_hostif(&dup, g_sw, 3, at)));

    sai_attribute_t a;
    a.id = SAI_HOSTIF_ATTR_OPER_STATUS;
    a.value.booldata = true;
    OK(ha->set_hostif_attribute(h, &a));
    CHECK(first_of(tp::netifs()).carrier);
    int old_filter = first_of(tp::netifs()).filter;
    a.id = SAI_HOSTIF_ATTR_VLAN_TAG;
    a.value.s32 = SAI_HOSTIF_VLAN_TAG_KEEP;
    OK(ha->set_hostif_attribute(h, &a));
    CHECK(first_of(tp::netifs()).keep_tag);
    CHECK(first_of(tp::netifs()).filter != old_filter);
    a.id = SAI_HOSTIF_ATTR_NAME;
    OK(ha->get_hostif_attribute(h, 1, &a));
    CHECK(std::string(a.value.chardata) == "Ethernet0");

    // trap group + traps
    sai_object_id_t grp;
    a.id = SAI_HOSTIF_TRAP_GROUP_ATTR_QUEUE;
    a.value.u32 = 4;
    OK(ha->create_hostif_trap_group(&grp, g_sw, 1, &a));
    sai_attribute_t ta[3];
    ta[0].id = SAI_HOSTIF_TRAP_ATTR_TRAP_TYPE;
    ta[0].value.s32 = SAI_HOSTIF_TRAP_TYPE_ARP_REQUEST;
    ta[1].id = SAI_HOSTIF_TRAP_ATTR_PACKET_ACTION;
    ta[1].value.s32 = SAI_PACKET_ACTION_COPY;
    ta[2].id = SAI_HOSTIF_TRAP_ATTR_TRAP_GROUP;
    ta[2].value.oid = grp;
    sai_object_id_t t1, t2;
    OK(ha->create_hostif_trap(&t1, g_sw, 3, ta));
    CHECK(tp::trap_to_cpu(SAI_HOSTIF_TRAP_TYPE_ARP_REQUEST));
    CHECK_EQ(ha->create_hostif_trap(&dup, g_sw, 3, ta), SAI_STATUS_ITEM_ALREADY_EXISTS);
    ta[0].value.s32 = SAI_HOSTIF_TRAP_TYPE_BGP;   // no SDK control: kept, not programmed
    OK(ha->create_hostif_trap(&t2, g_sw, 3, ta));
    CHECK_EQ(ha->remove_hostif_trap_group(grp), SAI_STATUS_OBJECT_IN_USE);
    a.id = SAI_HOSTIF_TRAP_ATTR_PACKET_ACTION;
    a.value.s32 = SAI_PACKET_ACTION_DROP;
    OK(ha->set_hostif_trap_attribute(t1, &a));
    CHECK(!tp::trap_to_cpu(SAI_HOSTIF_TRAP_TYPE_ARP_REQUEST));
    OK(ha->remove_hostif_trap(t1));
    OK(ha->remove_hostif_trap(t2));
    OK(ha->remove_hostif_trap_group(grp));
    CHECK_EQ(ha->remove_hostif_trap_group(get_oid(g_sw, SAI_SWITCH_ATTR_DEFAULT_TRAP_GROUP)), SAI_STATUS_OBJECT_IN_USE);

    // the wildcard table entry orchagent creates at init
    sai_attribute_t te[2];
    te[0].id = SAI_HOSTIF_TABLE_ENTRY_ATTR_TYPE;
    te[0].value.s32 = SAI_HOSTIF_TABLE_ENTRY_TYPE_WILDCARD;
    te[1].id = SAI_HOSTIF_TABLE_ENTRY_ATTR_CHANNEL_TYPE;
    te[1].value.s32 = SAI_HOSTIF_TABLE_ENTRY_CHANNEL_TYPE_NETDEV_PHYSICAL_PORT;
    sai_object_id_t ent;
    OK(ha->create_hostif_table_entry(&ent, g_sw, 2, te));
    OK(ha->remove_hostif_table_entry(ent));

    OK(ha->remove_hostif(h));
    CHECK_EQ(tp::netifs().size(), 0u);
    CHECK_EQ(ha->remove_hostif(h), SAI_STATUS_INVALID_OBJECT_ID);
}

static sai_ip_address_t ip4(uint8_t a, uint8_t b, uint8_t c, uint8_t d)
{
    sai_ip_address_t r;
    std::memset(&r, 0, sizeof(r));
    r.addr_family = SAI_IP_ADDR_FAMILY_IPV4;
    uint8_t x[4] = {a, b, c, d};
    std::memcpy(&r.addr.ip4, x, 4);
    return r;
}

static sai_route_entry_t route(uint8_t a, uint8_t b, uint8_t c, uint8_t d, int len)
{
    sai_route_entry_t e;
    std::memset(&e, 0, sizeof(e));
    e.switch_id = g_sw;
    e.vr_id = get_oid(g_sw, SAI_SWITCH_ATTR_DEFAULT_VIRTUAL_ROUTER_ID);
    e.destination.addr_family = SAI_IP_ADDR_FAMILY_IPV4;
    uint8_t x[4] = {a, b, c, d};
    std::memcpy(&e.destination.addr.ip4, x, 4);
    uint32_t m = len ? 0xffffffffu << (32 - len) : 0;
    uint8_t mk[4] = {(uint8_t)(m >> 24), (uint8_t)(m >> 16), (uint8_t)(m >> 8), (uint8_t)m};
    std::memcpy(&e.destination.mask.ip4, mk, 4);
    return e;
}

static void test_l3()
{
    sai_object_id_t vr = get_oid(g_sw, SAI_SWITCH_ATTR_DEFAULT_VIRTUAL_ROUTER_ID);
    // port 3 (hw 13): remove it from VLAN 1 first, like orchagent
    sai_attribute_t at[3];
    at[0].id = SAI_ROUTER_INTERFACE_ATTR_VIRTUAL_ROUTER_ID;
    at[0].value.oid = vr;
    at[1].id = SAI_ROUTER_INTERFACE_ATTR_TYPE;
    at[1].value.s32 = SAI_ROUTER_INTERFACE_TYPE_PORT;
    at[2].id = SAI_ROUTER_INTERFACE_ATTR_PORT_ID;
    at[2].value.oid = g_ports[3];
    sai_object_id_t rif;
    OK(ra->create_router_interface(&rif, g_sw, 3, at));
    CHECK_EQ(sai_object_type_query(rif), SAI_OBJECT_TYPE_ROUTER_INTERFACE);
    CHECK(tp::vlan_exists(4094) && vmember(4094, 13) && vuntagged(4094, 13));
    CHECK_EQ(tp::port(13).pvid, 4094);
    CHECK_EQ(tp::l3_intfs().size(), 1u);
    CHECK_EQ(tp::count(tp::T_STATIONS), 1u);
    sai_object_id_t dup;
    CHECK_EQ(ra->create_router_interface(&dup, g_sw, 3, at), SAI_STATUS_ITEM_ALREADY_EXISTS);
    sai_attribute_t v;
    v.id = SAI_VLAN_ATTR_VLAN_ID;
    v.value.u16 = 4094;
    CHECK_EQ(va->create_vlan(&dup, g_sw, 1, &v), SAI_STATUS_ITEM_ALREADY_EXISTS);   // internal VLAN

    sai_neighbor_entry_t ne;
    std::memset(&ne, 0, sizeof(ne));
    ne.switch_id = g_sw;
    ne.rif_id = rif;
    ne.ip_address = ip4(10, 0, 0, 2);
    sai_attribute_t na1;
    na1.id = SAI_NEIGHBOR_ENTRY_ATTR_DST_MAC_ADDRESS;
    uint8_t nmac[6] = {0x00, 0x00, 0x5e, 0x00, 0x53, 0x01};
    std::memcpy(na1.value.mac, nmac, 6);

    // next hop first (unresolved -> CPU), then the neighbor resolves it
    sai_attribute_t nh[3];
    nh[0].id = SAI_NEXT_HOP_ATTR_TYPE;
    nh[0].value.s32 = SAI_NEXT_HOP_TYPE_IP;
    nh[1].id = SAI_NEXT_HOP_ATTR_IP;
    nh[1].value.ipaddr = ip4(10, 0, 0, 2);
    nh[2].id = SAI_NEXT_HOP_ATTR_ROUTER_INTERFACE_ID;
    nh[2].value.oid = rif;
    sai_object_id_t nhid;
    OK(nha->create_next_hop(&nhid, g_sw, 3, nh));
    sai_route_entry_t r1 = route(192, 168, 0, 0, 24);
    sai_attribute_t ra1;
    ra1.id = SAI_ROUTE_ENTRY_ATTR_NEXT_HOP_ID;
    ra1.value.oid = nhid;
    OK(rta->create_route_entry(&r1, 1, &ra1));
    CHECK_EQ(rta->create_route_entry(&r1, 1, &ra1), SAI_STATUS_ITEM_ALREADY_EXISTS);
    CHECK_EQ(tp::routes().size(), 1u);
    int egr = first_of(tp::routes()).target;
    CHECK(egr_of(egr).cpu);

    OK(na->create_neighbor_entry(&ne, 1, &na1));
    CHECK_EQ(tp::hosts().size(), 1u);
    int nbr_egr = first_of(tp::hosts()).egress;
    CHECK_EQ(egr_of(nbr_egr).dest, 13);
    CHECK(std::memcmp(egr_of(nbr_egr).mac, nmac, 6) == 0);
    CHECK_EQ(first_of(tp::routes()).target, nbr_egr);   // route re-pointed at the neighbor

    // IP2ME and default-drop routes, as orchagent installs them
    sai_route_entry_t me = route(10, 0, 0, 1, 32);
    sai_attribute_t ra2;
    ra2.id = SAI_ROUTE_ENTRY_ATTR_NEXT_HOP_ID;
    ra2.value.oid = get_oid(g_sw, SAI_SWITCH_ATTR_CPU_PORT);
    OK(rta->create_route_entry(&me, 1, &ra2));
    sai_route_entry_t def = route(0, 0, 0, 0, 0);
    sai_attribute_t ra3;
    ra3.id = SAI_ROUTE_ENTRY_ATTR_PACKET_ACTION;
    ra3.value.s32 = SAI_PACKET_ACTION_DROP;
    OK(rta->create_route_entry(&def, 1, &ra3));
    CHECK(egr_of(tp::route_target(0, def.destination)).drop);
    // prefix canonicalisation: host bits are ignored in the key
    sai_route_entry_t r1b = route(192, 168, 0, 77, 24);
    sai_attribute_t g;
    g.id = SAI_ROUTE_ENTRY_ATTR_NEXT_HOP_ID;
    OK(rta->get_route_entry_attribute(&r1b, 1, &g));
    CHECK_EQ(g.value.oid, nhid);

    // in-use checks, then teardown in order
    CHECK_EQ(na->remove_neighbor_entry(&ne), SAI_STATUS_OBJECT_IN_USE);
    CHECK_EQ(nha->remove_next_hop(nhid), SAI_STATUS_OBJECT_IN_USE);
    CHECK_EQ(ra->remove_router_interface(rif), SAI_STATUS_OBJECT_IN_USE);
    OK(rta->remove_route_entry(&r1));
    OK(nha->remove_next_hop(nhid));
    OK(na->remove_neighbor_entry(&ne));
    OK(rta->remove_route_entry(&me));
    OK(rta->remove_route_entry(&def));
    OK(ra->remove_router_interface(rif));
    CHECK(!tp::vlan_exists(4094));
    CHECK_EQ(tp::port(13).pvid, 1);
    CHECK_EQ(tp::l3_intfs().size(), 0u);
    CHECK_EQ(tp::hosts().size(), 0u);
    CHECK_EQ(tp::routes().size(), 0u);

    // VLAN RIF; a route on an unknown next-hop group
    sai_object_id_t v200, vrif;
    v.value.u16 = 200;
    OK(va->create_vlan(&v200, g_sw, 1, &v));
    at[1].value.s32 = SAI_ROUTER_INTERFACE_TYPE_VLAN;
    at[2].id = SAI_ROUTER_INTERFACE_ATTR_VLAN_ID;
    at[2].value.oid = v200;
    OK(ra->create_router_interface(&vrif, g_sw, 3, at));
    CHECK_EQ(first_of(tp::l3_intfs()).vid, 200);
    CHECK_EQ(va->remove_vlan(v200), SAI_STATUS_OBJECT_IN_USE);
    sai_route_entry_t r2 = route(172, 16, 0, 0, 12);
    ra1.value.oid = tp::make_oid(SAI_OBJECT_TYPE_NEXT_HOP_GROUP, 1);
    CHECK_EQ(rta->create_route_entry(&r2, 1, &ra1), SAI_STATUS_INVALID_PARAMETER);
    OK(ra->remove_router_interface(vrif));
    OK(va->remove_vlan(v200));
    at[1].value.s32 = SAI_ROUTER_INTERFACE_TYPE_SUB_PORT;
    CHECK_EQ(ra->create_router_interface(&dup, g_sw, 3, at), SAI_STATUS_NOT_SUPPORTED);
}

// ---------------------------------------------------------- VRF / ECMP helpers
static sai_object_id_t mk_port_rif(sai_object_id_t vr, sai_object_id_t port)
{
    sai_attribute_t at[3];
    at[0].id = SAI_ROUTER_INTERFACE_ATTR_VIRTUAL_ROUTER_ID;
    at[0].value.oid = vr;
    at[1].id = SAI_ROUTER_INTERFACE_ATTR_TYPE;
    at[1].value.s32 = SAI_ROUTER_INTERFACE_TYPE_PORT;
    at[2].id = SAI_ROUTER_INTERFACE_ATTR_PORT_ID;
    at[2].value.oid = port;
    sai_object_id_t rif = 0;
    OK(ra->create_router_interface(&rif, g_sw, 3, at));
    return rif;
}
static sai_neighbor_entry_t nbr(sai_object_id_t rif, sai_ip_address_t ip)
{
    sai_neighbor_entry_t ne;
    std::memset(&ne, 0, sizeof(ne));
    ne.switch_id = g_sw;
    ne.rif_id = rif;
    ne.ip_address = ip;
    return ne;
}
static void mk_nbr(sai_object_id_t rif, sai_ip_address_t ip, uint8_t last)
{
    sai_neighbor_entry_t ne = nbr(rif, ip);
    sai_attribute_t a;
    a.id = SAI_NEIGHBOR_ENTRY_ATTR_DST_MAC_ADDRESS;
    uint8_t m[6] = {0x00, 0x00, 0x5e, 0x00, 0x53, last};
    std::memcpy(a.value.mac, m, 6);
    OK(na->create_neighbor_entry(&ne, 1, &a));
}
static sai_object_id_t mk_nh(sai_object_id_t rif, sai_ip_address_t ip)
{
    sai_attribute_t nh[3];
    nh[0].id = SAI_NEXT_HOP_ATTR_TYPE;
    nh[0].value.s32 = SAI_NEXT_HOP_TYPE_IP;
    nh[1].id = SAI_NEXT_HOP_ATTR_IP;
    nh[1].value.ipaddr = ip;
    nh[2].id = SAI_NEXT_HOP_ATTR_ROUTER_INTERFACE_ID;
    nh[2].value.oid = rif;
    sai_object_id_t id = 0;
    OK(nha->create_next_hop(&id, g_sw, 3, nh));
    return id;
}
static sai_route_entry_t route_in(sai_object_id_t vr, uint8_t a, uint8_t b, uint8_t c, uint8_t d, int len)
{
    sai_route_entry_t e = route(a, b, c, d, len);
    e.vr_id = vr;
    return e;
}
static void mk_route(const sai_route_entry_t& e, sai_object_id_t nh)
{
    sai_attribute_t a;
    a.id = SAI_ROUTE_ENTRY_ATTR_NEXT_HOP_ID;
    a.value.oid = nh;
    OK(rta->create_route_entry(&e, 1, &a));
}
static int nbr_egress(sai_object_id_t rif, sai_ip_address_t ip)
{
    return tp::neighbor_egress(rif, ip);
}
static int drop_egress()
{
    return tp::drop_egress();
}

static void test_vrf()
{
    sai_object_id_t dvr = get_oid(g_sw, SAI_SWITCH_ATTR_DEFAULT_VIRTUAL_ROUTER_ID);
    CHECK_EQ(vra->remove_virtual_router(dvr), SAI_STATUS_OBJECT_IN_USE);
    uint8_t vmac[6] = {0x02, 0x00, 0x00, 0x00, 0x0a, 0x01};
    sai_attribute_t va2[2];
    va2[0].id = SAI_VIRTUAL_ROUTER_ATTR_SRC_MAC_ADDRESS;
    std::memcpy(va2[0].value.mac, vmac, 6);
    va2[1].id = SAI_VIRTUAL_ROUTER_ATTR_VIOLATION_TTL1_PACKET_ACTION;   // stub: kept
    va2[1].value.s32 = SAI_PACKET_ACTION_DROP;
    sai_object_id_t vr1 = 0, vr2 = 0, bad = 0;
    OK(vra->create_virtual_router(&vr1, g_sw, 2, va2));
    OK(vra->create_virtual_router(&vr2, g_sw, 0, nullptr));
    CHECK(vr1 != vr2 && vr1 != dvr && vr2 != dvr);
    CHECK_EQ(sai_object_type_query(vr1), SAI_OBJECT_TYPE_VIRTUAL_ROUTER);
    CHECK_EQ(tp::backend_id(vr1), 1);
    CHECK_EQ(tp::backend_id(vr2), 2);
    sai_attribute_t off;
    off.id = SAI_VIRTUAL_ROUTER_ATTR_ADMIN_V4_STATE;   // routing cannot be switched off per VRF
    off.value.booldata = false;
    CHECK_EQ(vra->create_virtual_router(&bad, g_sw, 1, &off), SAI_STATUS_ATTR_NOT_SUPPORTED_0);
    sai_attribute_t g[2];
    g[0].id = SAI_VIRTUAL_ROUTER_ATTR_SRC_MAC_ADDRESS;
    g[1].id = SAI_VIRTUAL_ROUTER_ATTR_VIOLATION_TTL1_PACKET_ACTION;
    OK(vra->get_virtual_router_attribute(vr1, 2, g));
    CHECK(std::memcmp(g[0].value.mac, vmac, 6) == 0);
    CHECK_EQ(g[1].value.s32, SAI_PACKET_ACTION_DROP);
    sai_attribute_t smac;
    smac.id = SAI_SWITCH_ATTR_SRC_MAC_ADDRESS;
    OK(sw->get_switch_attribute(g_sw, 1, &smac));
    OK(vra->get_virtual_router_attribute(vr2, 1, g));
    CHECK(std::memcmp(g[0].value.mac, smac.value.mac, 6) == 0);   // unset: the switch MAC

    // RIF on port 6 in vr1 (takes vr1's MAC), RIF on port 7 in the default VR
    sai_object_id_t rif1 = mk_port_rif(vr1, g_ports[6]);
    sai_object_id_t rif0 = mk_port_rif(dvr, g_ports[7]);
    tp::Rif r1 = tp::rif(rif1);
    CHECK_EQ(r1.vrf, 1);
    CHECK_EQ(intf_of(r1.intf).vrf, 1);
    CHECK(std::memcmp(intf_of(r1.intf).mac, vmac, 6) == 0);
    CHECK_EQ(intf_of(tp::rif(rif0).intf).vrf, 0);
    sai_attribute_t rv;
    rv.id = SAI_ROUTER_INTERFACE_ATTR_VIRTUAL_ROUTER_ID;
    OK(ra->get_router_interface_attribute(rif1, 1, &rv));
    CHECK_EQ(rv.value.oid, vr1);
    // the MTU update keeps the VRF binding
    sai_attribute_t mtu;
    mtu.id = SAI_ROUTER_INTERFACE_ATTR_MTU;
    mtu.value.u32 = 9100;
    OK(ra->set_router_interface_attribute(rif1, &mtu));
    CHECK_EQ(intf_of(tp::rif(rif1).intf).vrf, 1);
    // a RIF in an unknown VR
    sai_attribute_t at[3];
    at[0].id = SAI_ROUTER_INTERFACE_ATTR_VIRTUAL_ROUTER_ID;
    at[0].value.oid = tp::make_oid(SAI_OBJECT_TYPE_VIRTUAL_ROUTER, 99);
    at[1].id = SAI_ROUTER_INTERFACE_ATTR_TYPE;
    at[1].value.s32 = SAI_ROUTER_INTERFACE_TYPE_LOOPBACK;
    CHECK_EQ(ra->create_router_interface(&bad, g_sw, 2, at), SAI_STATUS_INVALID_ATTR_VALUE_0);
    at[0].value.oid = vr1;   // loopback RIF in a VRF: no SDK object
    sai_object_id_t lo1 = 0;
    OK(ra->create_router_interface(&lo1, g_sw, 2, at));

    // the same neighbor address in both VRFs: one host entry each
    sai_ip_address_t ip = ip4(10, 9, 0, 1);
    mk_nbr(rif1, ip, 0x11);
    mk_nbr(rif0, ip, 0x12);
    CHECK(tp::host_egress(1, ip) >= 0);
    CHECK(tp::host_egress(0, ip) >= 0);
    CHECK(tp::host_egress(1, ip) != tp::host_egress(0, ip));

    // the same prefix in two VRs; a route in vr1 leaking to a next hop in the default VR
    sai_object_id_t nh1 = mk_nh(rif1, ip), nh0 = mk_nh(rif0, ip);
    sai_route_entry_t p1 = route_in(vr1, 198, 51, 100, 0, 24), p0 = route_in(dvr, 198, 51, 100, 0, 24);
    sai_route_entry_t leak = route_in(vr1, 203, 0, 113, 0, 24);
    mk_route(p1, nh1);
    sai_attribute_t drop;
    drop.id = SAI_ROUTE_ENTRY_ATTR_PACKET_ACTION;
    drop.value.s32 = SAI_PACKET_ACTION_DROP;
    OK(rta->create_route_entry(&p0, 1, &drop));
    mk_route(leak, nh0);
    sai_route_entry_t me = route_in(vr1, 10, 9, 0, 0, 32);   // IP2ME in the VRF
    mk_route(me, get_oid(g_sw, SAI_SWITCH_ATTR_CPU_PORT));
    CHECK_EQ(tp::route_target(1, p1.destination), nbr_egress(rif1, ip));
    CHECK_EQ(tp::route_target(0, p0.destination), drop_egress());
    CHECK_EQ(tp::route_target(1, leak.destination), nbr_egress(rif0, ip));
    CHECK(egr_of(tp::route_target(1, me.destination)).cpu);
    sai_attribute_t gn;
    gn.id = SAI_ROUTE_ENTRY_ATTR_NEXT_HOP_ID;
    OK(rta->get_route_entry_attribute(&p1, 1, &gn));
    CHECK_EQ(gn.value.oid, nh1);
    CHECK_EQ(rta->create_route_entry(&p1, 1, &gn), SAI_STATUS_ITEM_ALREADY_EXISTS);
    sai_route_entry_t nowhere = route_in(tp::make_oid(SAI_OBJECT_TYPE_VIRTUAL_ROUTER, 99), 10, 0, 0, 0, 8);
    CHECK_EQ(rta->create_route_entry(&nowhere, 1, &drop), SAI_STATUS_INVALID_PARAMETER);
    CHECK_EQ(vra->remove_virtual_router(vr1), SAI_STATUS_OBJECT_IN_USE);

    // teardown
    OK(rta->remove_route_entry(&p1));
    OK(rta->remove_route_entry(&p0));
    OK(rta->remove_route_entry(&leak));
    OK(rta->remove_route_entry(&me));
    OK(nha->remove_next_hop(nh1));
    OK(nha->remove_next_hop(nh0));
    sai_neighbor_entry_t n1 = nbr(rif1, ip), n0 = nbr(rif0, ip);
    OK(na->remove_neighbor_entry(&n1));
    CHECK(tp::host_egress(1, ip) < 0);
    CHECK(tp::host_egress(0, ip) >= 0);
    OK(na->remove_neighbor_entry(&n0));
    CHECK_EQ(vra->remove_virtual_router(vr1), SAI_STATUS_OBJECT_IN_USE);   // RIFs still in it
    OK(ra->remove_router_interface(rif1));
    OK(ra->remove_router_interface(lo1));
    OK(ra->remove_router_interface(rif0));
    OK(vra->remove_virtual_router(vr1));
    OK(vra->remove_virtual_router(vr2));
    CHECK_EQ(vra->remove_virtual_router(vr2), SAI_STATUS_INVALID_OBJECT_ID);
    CHECK_EQ(tp::routes().size(), 0u);
    CHECK_EQ(tp::hosts().size(), 0u);
    CHECK_EQ(tp::object_count(SAI_OBJECT_TYPE_VIRTUAL_ROUTER), 1u);

    // VRF ids run out at the chip's maximum (fake: 1023)
    std::vector<sai_object_id_t> many;
    for (;;) {
        sai_object_id_t v = 0;
        sai_status_t st = vra->create_virtual_router(&v, g_sw, 0, nullptr);
        if (st != SAI_STATUS_SUCCESS) {
            CHECK_EQ(st, SAI_STATUS_INSUFFICIENT_RESOURCES);
            break;
        }
        many.push_back(v);
        if (many.size() > 5000) break;
    }
    CHECK_EQ(many.size(), 1023u);
    for (auto v : many) OK(vra->remove_virtual_router(v));
}

static void test_ecmp()
{
    auto sw_u32 = [](sai_attr_id_t id) {
        sai_attribute_t a;
        a.id = id;
        CHECK_EQ(sw->get_switch_attribute(g_sw, 1, &a), SAI_STATUS_SUCCESS);
        return a.value.u32;
    };
    sai_object_id_t dvr = get_oid(g_sw, SAI_SWITCH_ATTR_DEFAULT_VIRTUAL_ROUTER_ID);
    sai_object_id_t rif6 = mk_port_rif(dvr, g_ports[6]), rif7 = mk_port_rif(dvr, g_ports[7]);
    sai_ip_address_t ipa = ip4(10, 8, 0, 1), ipb = ip4(10, 8, 1, 1), ipc = ip4(10, 8, 2, 1);
    mk_nbr(rif6, ipa, 0x21);
    sai_object_id_t nha_ = mk_nh(rif6, ipa), nhb = mk_nh(rif7, ipb), nhc = mk_nh(rif7, ipc);   // b, c unresolved
    int ea = nbr_egress(rif6, ipa);

    // group create: TYPE is mandatory, only (unordered) ECMP
    sai_object_id_t grp = 0, bad = 0;
    CHECK_EQ(nga->create_next_hop_group(&bad, g_sw, 0, nullptr), SAI_STATUS_MANDATORY_ATTRIBUTE_MISSING);
    sai_attribute_t ty;
    ty.id = SAI_NEXT_HOP_GROUP_ATTR_TYPE;
    ty.value.s32 = SAI_NEXT_HOP_GROUP_TYPE_DYNAMIC_ORDERED_ECMP;
    CHECK_EQ(nga->create_next_hop_group(&bad, g_sw, 1, &ty), SAI_STATUS_ATTR_NOT_SUPPORTED_0);
    ty.value.s32 = SAI_NEXT_HOP_GROUP_TYPE_ECMP;
    OK(nga->create_next_hop_group(&grp, g_sw, 1, &ty));
    CHECK_EQ(sai_object_type_query(grp), SAI_OBJECT_TYPE_NEXT_HOP_GROUP);

    // a route on the empty group drops; no SDK group yet
    sai_route_entry_t rt = route(192, 0, 2, 0, 24);
    mk_route(rt, grp);
    auto target = [&]() { return tp::route_target(0, rt.destination); };
    CHECK_EQ(target(), drop_egress());
    CHECK_EQ(tp::ecmp_groups().size(), 0u);

    auto mk_member = [&](sai_object_id_t nh, uint32_t weight) {
        sai_attribute_t m[3];
        m[0].id = SAI_NEXT_HOP_GROUP_MEMBER_ATTR_NEXT_HOP_GROUP_ID;
        m[0].value.oid = grp;
        m[1].id = SAI_NEXT_HOP_GROUP_MEMBER_ATTR_NEXT_HOP_ID;
        m[1].value.oid = nh;
        m[2].id = SAI_NEXT_HOP_GROUP_MEMBER_ATTR_WEIGHT;
        m[2].value.u32 = weight;
        sai_object_id_t id = 0;
        OK(nga->create_next_hop_group_member(&id, g_sw, weight ? 3 : 2, m));
        return id;
    };
    // first resolved member: the SDK group appears and the route follows
    sai_object_id_t ma = mk_member(nha_, 0);
    CHECK_EQ(tp::ecmp_groups().size(), 1u);
    int ecmp = first_of(tp::ecmp_groups()).id;
    CHECK_EQ(target(), ecmp);
    CHECK(ecmp_members(ecmp) == std::vector<int>({ea}));
    CHECK_EQ(sw_u32(SAI_SWITCH_ATTR_AVAILABLE_NEXT_HOP_GROUP_ENTRY), 4095u);
    // unresolved members are kept, not programmed; the neighbor brings them in
    sai_object_id_t mb = mk_member(nhb, 0);
    sai_object_id_t mc = mk_member(nhc, 2);
    CHECK(ecmp_members(ecmp) == std::vector<int>({ea}));
    sai_attribute_t ga[2];
    sai_object_id_t list[8];
    ga[0].id = SAI_NEXT_HOP_GROUP_ATTR_NEXT_HOP_COUNT;
    ga[1].id = SAI_NEXT_HOP_GROUP_ATTR_NEXT_HOP_MEMBER_LIST;
    ga[1].value.objlist.count = 8;
    ga[1].value.objlist.list = list;
    OK(nga->get_next_hop_group_attribute(grp, 2, ga));
    CHECK_EQ(ga[0].value.u32, 3u);
    CHECK_EQ(ga[1].value.objlist.count, 3u);
    CHECK(list[0] == ma && list[1] == mb && list[2] == mc);
    mk_nbr(rif7, ipb, 0x22);
    int eb = nbr_egress(rif7, ipb);
    CHECK(ecmp_members(ecmp) == std::vector<int>({ea, eb}));
    CHECK_EQ(target(), ecmp);   // same SDK group, updated in place
    mk_nbr(rif7, ipc, 0x23);
    int ec = nbr_egress(rif7, ipc);
    CHECK(ecmp_members(ecmp) == std::vector<int>({ea, eb, ec, ec}));   // weight 2
    CHECK_EQ(sw_u32(SAI_SWITCH_ATTR_AVAILABLE_NEXT_HOP_GROUP_MEMBER_ENTRY), 16384u - 4);

    // member attributes
    sai_attribute_t w;
    w.id = SAI_NEXT_HOP_GROUP_MEMBER_ATTR_WEIGHT;
    w.value.u32 = 0;
    CHECK_EQ(nga->set_next_hop_group_member_attribute(ma, &w), SAI_STATUS_INVALID_ATTR_VALUE_0);
    w.value.u32 = 3;
    OK(nga->set_next_hop_group_member_attribute(ma, &w));
    CHECK(ecmp_members(ecmp) == std::vector<int>({ea, ea, ea, eb, ec, ec}));
    w.value.u32 = 100;   // more paths than the chip's 64: the rest is left out
    OK(nga->set_next_hop_group_member_attribute(ma, &w));
    CHECK_EQ(ecmp_members(ecmp).size(), 64u);
    w.value.u32 = 1;
    OK(nga->set_next_hop_group_member_attribute(ma, &w));
    sai_attribute_t mg[3];
    mg[0].id = SAI_NEXT_HOP_GROUP_MEMBER_ATTR_NEXT_HOP_GROUP_ID;
    mg[1].id = SAI_NEXT_HOP_GROUP_MEMBER_ATTR_NEXT_HOP_ID;
    mg[2].id = SAI_NEXT_HOP_GROUP_MEMBER_ATTR_WEIGHT;
    OK(nga->get_next_hop_group_member_attribute(mc, 3, mg));
    CHECK(mg[0].value.oid == grp && mg[1].value.oid == nhc && mg[2].value.u32 == 2u);
    sai_attribute_t nhset;   // a member moves to another next hop
    nhset.id = SAI_NEXT_HOP_GROUP_MEMBER_ATTR_NEXT_HOP_ID;
    nhset.value.oid = nha_;
    OK(nga->set_next_hop_group_member_attribute(mc, &nhset));
    CHECK(ecmp_members(ecmp) == std::vector<int>({ea, eb, ea, ea}));
    nhset.value.oid = nhc;
    OK(nga->set_next_hop_group_member_attribute(mc, &nhset));
    CHECK_EQ(nga->set_next_hop_group_attribute(grp, &ty), SAI_STATUS_INVALID_ATTR_VALUE_0);   // create only

    // in use
    CHECK_EQ(nha->remove_next_hop(nha_), SAI_STATUS_OBJECT_IN_USE);
    CHECK_EQ(nga->remove_next_hop_group(grp), SAI_STATUS_OBJECT_IN_USE);
    sai_neighbor_entry_t nb_a = nbr(rif6, ipa);
    CHECK_EQ(na->remove_neighbor_entry(&nb_a), SAI_STATUS_OBJECT_IN_USE);
    CHECK_EQ(tp::backend_egress_destroy(ea), SAI_STATUS_OBJECT_IN_USE);

    // ECMP in a VRF: a route in vr1 on the same group (egress objects have no VRF)
    sai_object_id_t vr1 = 0;
    OK(vra->create_virtual_router(&vr1, g_sw, 0, nullptr));
    sai_route_entry_t rv = route_in(vr1, 192, 0, 2, 0, 24);
    mk_route(rv, grp);
    CHECK_EQ(tp::route_target(tp::backend_id(vr1), rv.destination), ecmp);
    OK(rta->remove_route_entry(&rv));
    OK(vra->remove_virtual_router(vr1));

    // members leave; the last one takes the SDK group with it, the route drops
    OK(nga->remove_next_hop_group_member(ma));
    CHECK(ecmp_members(ecmp) == std::vector<int>({eb, ec, ec}));
    OK(nga->remove_next_hop_group_member(mc));
    OK(nga->remove_next_hop_group_member(mb));
    CHECK_EQ(tp::ecmp_groups().size(), 0u);
    CHECK_EQ(target(), drop_egress());
    CHECK_EQ(nga->remove_next_hop_group_member(mb), SAI_STATUS_INVALID_OBJECT_ID);
    // a route moved from a next hop to a group and back
    sai_attribute_t to;
    to.id = SAI_ROUTE_ENTRY_ATTR_NEXT_HOP_ID;
    to.value.oid = nhb;
    OK(rta->set_route_entry_attribute(&rt, &to));
    CHECK_EQ(target(), eb);
    sai_object_id_t m2 = mk_member(nhb, 0);
    to.value.oid = grp;
    OK(rta->set_route_entry_attribute(&rt, &to));
    CHECK_EQ(target(), first_of(tp::ecmp_groups()).id);
    OK(rta->remove_route_entry(&rt));
    OK(nga->remove_next_hop_group_member(m2));
    OK(nga->remove_next_hop_group(grp));
    CHECK_EQ(nga->remove_next_hop_group(grp), SAI_STATUS_INVALID_OBJECT_ID);
    CHECK_EQ(sw_u32(SAI_SWITCH_ATTR_AVAILABLE_NEXT_HOP_GROUP_ENTRY), 4096u);
    CHECK_EQ(sw_u32(SAI_SWITCH_ATTR_AVAILABLE_NEXT_HOP_GROUP_MEMBER_ENTRY), 16384u);
    // a member on an unknown group / next hop
    sai_attribute_t m[2];
    m[0].id = SAI_NEXT_HOP_GROUP_MEMBER_ATTR_NEXT_HOP_GROUP_ID;
    m[0].value.oid = grp;
    m[1].id = SAI_NEXT_HOP_GROUP_MEMBER_ATTR_NEXT_HOP_ID;
    m[1].value.oid = nhb;
    CHECK_EQ(nga->create_next_hop_group_member(&bad, g_sw, 2, m), SAI_STATUS_INVALID_ATTR_VALUE_0);

    // teardown
    OK(nha->remove_next_hop(nha_));
    OK(nha->remove_next_hop(nhb));
    OK(nha->remove_next_hop(nhc));
    for (auto x : {nbr(rif6, ipa), nbr(rif7, ipb), nbr(rif7, ipc)}) OK(na->remove_neighbor_entry(&x));
    OK(ra->remove_router_interface(rif6));
    OK(ra->remove_router_interface(rif7));
    CHECK_EQ(tp::routes().size(), 0u);
    CHECK_EQ(tp::hosts().size(), 0u);
}

static void test_capabilities()
{
    // Every advertised get on SWITCH / PORT / LAG / LAG_MEMBER / VIRTUAL_ROUTER /
    // NEXT_HOP_GROUP(_MEMBER) works on a live object.
    sai_object_id_t lag, lm;
    OK(la->create_lag(&lag, g_sw, 0, nullptr));
    sai_attribute_t ma[2];
    ma[0].id = SAI_LAG_MEMBER_ATTR_LAG_ID;
    ma[0].value.oid = lag;
    ma[1].id = SAI_LAG_MEMBER_ATTR_PORT_ID;
    ma[1].value.oid = g_ports[4];   // freed by test_lag
    OK(la->create_lag_member(&lm, g_sw, 2, ma));
    // a VR, a next-hop group with one (unresolved) member on a loopback RIF
    sai_object_id_t vr = 0, lo = 0, nh = 0, grp = 0, gm = 0;
    OK(vra->create_virtual_router(&vr, g_sw, 0, nullptr));
    sai_attribute_t la2[2];
    la2[0].id = SAI_ROUTER_INTERFACE_ATTR_VIRTUAL_ROUTER_ID;
    la2[0].value.oid = vr;
    la2[1].id = SAI_ROUTER_INTERFACE_ATTR_TYPE;
    la2[1].value.s32 = SAI_ROUTER_INTERFACE_TYPE_LOOPBACK;
    OK(ra->create_router_interface(&lo, g_sw, 2, la2));
    nh = mk_nh(lo, ip4(10, 7, 0, 1));
    sai_attribute_t ty;
    ty.id = SAI_NEXT_HOP_GROUP_ATTR_TYPE;
    ty.value.s32 = SAI_NEXT_HOP_GROUP_TYPE_ECMP;
    OK(nga->create_next_hop_group(&grp, g_sw, 1, &ty));
    sai_attribute_t gma[2];
    gma[0].id = SAI_NEXT_HOP_GROUP_MEMBER_ATTR_NEXT_HOP_GROUP_ID;
    gma[0].value.oid = grp;
    gma[1].id = SAI_NEXT_HOP_GROUP_MEMBER_ATTR_NEXT_HOP_ID;
    gma[1].value.oid = nh;
    OK(nga->create_next_hop_group_member(&gm, g_sw, 2, gma));
    for (auto& c : tp::attr_caps()) {
        if (!c.get) continue;
        sai_object_id_t oid;
        if (c.ot == SAI_OBJECT_TYPE_SWITCH) oid = g_sw;
        else if (c.ot == SAI_OBJECT_TYPE_PORT) oid = g_ports[0];
        else if (c.ot == SAI_OBJECT_TYPE_LAG) oid = lag;
        else if (c.ot == SAI_OBJECT_TYPE_LAG_MEMBER) oid = lm;
        else if (c.ot == SAI_OBJECT_TYPE_VIRTUAL_ROUTER) oid = vr;
        else if (c.ot == SAI_OBJECT_TYPE_NEXT_HOP_GROUP) oid = grp;
        else if (c.ot == SAI_OBJECT_TYPE_NEXT_HOP_GROUP_MEMBER) oid = gm;
        else continue;
        sai_attribute_t a;
        std::memset(&a, 0, sizeof(a));
        a.id = c.id;
        uint64_t buf[64];
        a.value.objlist.count = 64;   // works for every list type (same layout: count + pointer)
        a.value.objlist.list = (sai_object_id_t*)buf;
        if (c.id == SAI_SWITCH_ATTR_ACL_STAGE_INGRESS || c.id == SAI_SWITCH_ATTR_ACL_STAGE_EGRESS) {
            a.value.aclcapability.action_list.count = 64;   // a list inside a struct
            a.value.aclcapability.action_list.list = (int32_t*)buf;
        }
        sai_status_t st = c.ot == SAI_OBJECT_TYPE_SWITCH ? sw->get_switch_attribute(oid, 1, &a)
                          : c.ot == SAI_OBJECT_TYPE_PORT ? pa->get_port_attribute(oid, 1, &a)
                          : c.ot == SAI_OBJECT_TYPE_LAG  ? la->get_lag_attribute(oid, 1, &a)
                          : c.ot == SAI_OBJECT_TYPE_LAG_MEMBER ? la->get_lag_member_attribute(oid, 1, &a)
                          : c.ot == SAI_OBJECT_TYPE_VIRTUAL_ROUTER ? vra->get_virtual_router_attribute(oid, 1, &a)
                          : c.ot == SAI_OBJECT_TYPE_NEXT_HOP_GROUP ? nga->get_next_hop_group_attribute(oid, 1, &a)
                                                                   : nga->get_next_hop_group_member_attribute(oid, 1, &a);
        if (st != SAI_STATUS_SUCCESS) std::fprintf(stderr, "  capability get ot=%d attr=%d -> %d\n", c.ot, c.id, st);
        CHECK_EQ(st, SAI_STATUS_SUCCESS);
    }
    OK(la->remove_lag_member(lm));
    OK(la->remove_lag(lag));
    OK(nga->remove_next_hop_group_member(gm));
    OK(nga->remove_next_hop_group(grp));
    OK(nha->remove_next_hop(nh));
    OK(ra->remove_router_interface(lo));
    OK(vra->remove_virtual_router(vr));
    sai_attr_capability_t cap;
    OK(sai_query_attribute_capability(g_sw, SAI_OBJECT_TYPE_PORT, SAI_PORT_ATTR_ADMIN_STATE, &cap));
    CHECK(cap.set_implemented && cap.get_implemented && !cap.create_implemented);
    OK(sai_query_attribute_capability(g_sw, SAI_OBJECT_TYPE_PORT, SAI_PORT_ATTR_PRBS_CONFIG, &cap));
    CHECK(!cap.set_implemented && !cap.get_implemented);
    int32_t ev[8];
    sai_s32_list_t el;
    el.count = 8;
    el.list = ev;
    OK(sai_query_attribute_enum_values_capability(g_sw, SAI_OBJECT_TYPE_PORT, SAI_PORT_ATTR_FEC_MODE, &el));
    CHECK_EQ(el.count, 3u);
    uint64_t cnt;
    CHECK_EQ(sai_object_type_get_availability(g_sw, SAI_OBJECT_TYPE_ROUTE_ENTRY, 0, nullptr, &cnt), SAI_STATUS_NOT_SUPPORTED);
}

// orchagent's init for a port that joins a LAG: no VLAN 1 member, no bridge port.
static void free_port(sai_object_id_t port)
{
    sai_object_id_t bp = bport_of(port);
    if (!bp) return;
    sai_object_id_t vlan1 = get_oid(g_sw, SAI_SWITCH_ATTR_DEFAULT_VLAN_ID);
    sai_object_id_t members[64];
    sai_attribute_t a;
    a.id = SAI_VLAN_ATTR_MEMBER_LIST;
    a.value.objlist.count = 64;
    a.value.objlist.list = members;
    OK(va->get_vlan_attribute(vlan1, 1, &a));
    for (uint32_t i = 0; i < a.value.objlist.count; i++) {
        sai_attribute_t b;
        b.id = SAI_VLAN_MEMBER_ATTR_BRIDGE_PORT_ID;
        OK(va->get_vlan_member_attribute(members[i], 1, &b));
        if (b.value.oid == bp) OK(va->remove_vlan_member(members[i]));
    }
    OK(ba->remove_bridge_port(bp));
}

static void test_temperature()
{
    // fake: 8 monitors at 40.0, 41.5, ... 50.5 C
    sai_attribute_t a;
    a.id = SAI_SWITCH_ATTR_MAX_NUMBER_OF_TEMP_SENSORS;
    OK(sw->get_switch_attribute(g_sw, 1, &a));
    CHECK_EQ(a.value.u8, 8);
    int32_t t[16];
    a.id = SAI_SWITCH_ATTR_TEMP_LIST;
    a.value.s32list.count = 2;
    a.value.s32list.list = t;
    CHECK_EQ(sw->get_switch_attribute(g_sw, 1, &a), SAI_STATUS_BUFFER_OVERFLOW);
    CHECK_EQ(a.value.s32list.count, 8u);
    a.value.s32list.count = 16;
    OK(sw->get_switch_attribute(g_sw, 1, &a));
    CHECK_EQ(a.value.s32list.count, 8u);
    CHECK_EQ(t[0], 40);
    CHECK_EQ(t[1], 42);   // 41.5 rounds away from zero
    CHECK_EQ(t[7], 51);
    sai_attribute_t mx[2];
    mx[0].id = SAI_SWITCH_ATTR_MAX_TEMP;
    mx[1].id = SAI_SWITCH_ATTR_AVERAGE_TEMP;
    OK(sw->get_switch_attribute(g_sw, 2, mx));
    CHECK_EQ(mx[0].value.s32, 51);   // 50.5
    CHECK_EQ(mx[1].value.s32, 45);   // 45.25
    tp::set_temperatures({-55, 1205});   // below zero rounds away from zero too
    OK(sw->get_switch_attribute(g_sw, 2, mx));
    CHECK_EQ(mx[0].value.s32, 121);
    CHECK_EQ(mx[1].value.s32, 58);   // 57.5
    a.value.s32list.count = 16;
    OK(sw->get_switch_attribute(g_sw, 1, &a));
    CHECK_EQ(t[0], -6);
    // sensor read fails: the error, index-encoded, no stale values
    tp::set_temperature_fail(true);
    mx[0].id = SAI_SWITCH_ATTR_PORT_NUMBER;
    mx[1].id = SAI_SWITCH_ATTR_MAX_TEMP;
    CHECK_EQ(sw->get_switch_attribute(g_sw, 2, mx), SAI_STATUS_FAILURE);
    tp::set_temperature_fail(false);
    std::vector<int> t8;
    for (int i = 0; i < 8; i++) t8.push_back(400 + 15 * i);
    tp::set_temperatures(t8);
    a.id = SAI_SWITCH_ATTR_MAX_TEMP;
    CHECK(sw->set_switch_attribute(g_sw, &a) != SAI_STATUS_SUCCESS);   // read-only
}

static void test_lag()
{
    sai_object_id_t p4 = g_ports[4], p5 = g_ports[5], p6 = g_ports[6];   // hw 17, 21, 25
    free_port(p4);
    free_port(p5);
    free_port(p6);
    sai_object_id_t lag, dup;
    OK(la->create_lag(&lag, g_sw, 0, nullptr));
    CHECK_EQ(sai_object_type_query(lag), SAI_OBJECT_TYPE_LAG);
    int tid = tp::backend_id(lag);
    CHECK(lag_exists(tid));
    sai_attribute_t a;
    a.id = SAI_LAG_ATTR_PORT_VLAN_ID;
    OK(la->get_lag_attribute(lag, 1, &a));
    CHECK_EQ(a.value.u16, 1);
    a.id = SAI_LAG_ATTR_TPID;
    OK(la->get_lag_attribute(lag, 1, &a));
    CHECK_EQ(a.value.u16, 0x8100);   // stub
    a.id = SAI_LAG_ATTR_SYSTEM_PORT_AGGREGATE_ID;
    a.value.u32 = 1;
    CHECK(SAI_STATUS_IS_ATTR_NOT_SUPPORTED(la->create_lag(&dup, g_sw, 1, &a)));

    // member, created disabled as orchagent does before LACP converges
    sai_attribute_t ma[4];
    ma[0].id = SAI_LAG_MEMBER_ATTR_LAG_ID;
    ma[0].value.oid = lag;
    ma[1].id = SAI_LAG_MEMBER_ATTR_PORT_ID;
    ma[1].value.oid = p4;
    ma[2].id = SAI_LAG_MEMBER_ATTR_EGRESS_DISABLE;
    ma[2].value.booldata = true;
    ma[3].id = SAI_LAG_MEMBER_ATTR_INGRESS_DISABLE;
    ma[3].value.booldata = true;
    sai_object_id_t m4, m5, m6;
    OK(la->create_lag_member(&m4, g_sw, 4, ma));
    CHECK_EQ(sai_object_type_query(m4), SAI_OBJECT_TYPE_LAG_MEMBER);
    CHECK_EQ(lagm(tid).size(), 1u);
    CHECK(lag0(tid).hw == 17 && lag0(tid).egress_disable && lag0(tid).ingress_disable);
    CHECK_EQ(la->create_lag_member(&dup, g_sw, 4, ma), SAI_STATUS_ITEM_ALREADY_EXISTS);
    sai_attribute_t g4[4];
    g4[0].id = SAI_LAG_MEMBER_ATTR_LAG_ID;
    g4[1].id = SAI_LAG_MEMBER_ATTR_PORT_ID;
    g4[2].id = SAI_LAG_MEMBER_ATTR_EGRESS_DISABLE;
    g4[3].id = SAI_LAG_MEMBER_ATTR_INGRESS_DISABLE;
    OK(la->get_lag_member_attribute(m4, 4, g4));
    CHECK(g4[0].value.oid == lag && g4[1].value.oid == p4 && g4[2].value.booldata && g4[3].value.booldata);
    // collecting, then distributing (setCollectionOnLagMember / setDistributionOnLagMember)
    a.id = SAI_LAG_MEMBER_ATTR_INGRESS_DISABLE;
    a.value.booldata = false;
    OK(la->set_lag_member_attribute(m4, &a));
    CHECK(!lag0(tid).ingress_disable && lag0(tid).egress_disable);
    a.id = SAI_LAG_MEMBER_ATTR_EGRESS_DISABLE;
    OK(la->set_lag_member_attribute(m4, &a));
    CHECK(!lag0(tid).egress_disable);
    a.id = SAI_LAG_MEMBER_ATTR_PORT_ID;
    a.value.oid = p5;
    CHECK(SAI_STATUS_IS_INVALID_ATTR_VALUE(la->set_lag_member_attribute(m4, &a)));   // create-only

    // bad members
    ma[1].value.oid = get_oid(g_sw, SAI_SWITCH_ATTR_CPU_PORT);
    CHECK(SAI_STATUS_IS_INVALID_ATTR_VALUE(la->create_lag_member(&dup, g_sw, 2, ma)));
    ma[1].value.oid = g_ports[7];   // still has its default bridge port
    CHECK_EQ(la->create_lag_member(&dup, g_sw, 2, ma), SAI_STATUS_OBJECT_IN_USE);
    CHECK_EQ(la->create_lag_member(&dup, g_sw, 1, ma), SAI_STATUS_MANDATORY_ATTRIBUTE_MISSING);
    ma[1].value.oid = p5;
    OK(la->create_lag_member(&m5, g_sw, 2, ma));
    sai_object_id_t ml[8];
    a.id = SAI_LAG_ATTR_PORT_LIST;
    a.value.objlist.count = 8;
    a.value.objlist.list = ml;
    OK(la->get_lag_attribute(lag, 1, &a));
    CHECK_EQ(a.value.objlist.count, 2u);
    CHECK(ml[0] == m4 && ml[1] == m5);
    CHECK_EQ(la->remove_lag(lag), SAI_STATUS_OBJECT_IN_USE);

    // LAG PVID goes to every member
    a.id = SAI_LAG_ATTR_PORT_VLAN_ID;
    a.value.u16 = 300;
    OK(la->set_lag_attribute(lag, &a));
    CHECK(tp::port(17).pvid == 300 && tp::port(21).pvid == 300);
    a.value.u16 = 0;   // orchagent writes .u32 into this u16 attribute: on big-endian it reads as 0
    CHECK(SAI_STATUS_IS_INVALID_ATTR_VALUE(la->set_lag_attribute(lag, &a)));

    // bridge port on the LAG + VLAN member: applied to every member port
    sai_attribute_t bpa[3];
    bpa[0].id = SAI_BRIDGE_PORT_ATTR_TYPE;
    bpa[0].value.s32 = SAI_BRIDGE_PORT_TYPE_PORT;
    bpa[1].id = SAI_BRIDGE_PORT_ATTR_PORT_ID;
    bpa[1].value.oid = lag;
    bpa[2].id = SAI_BRIDGE_PORT_ATTR_FDB_LEARNING_MODE;
    bpa[2].value.s32 = SAI_BRIDGE_PORT_FDB_LEARNING_MODE_DISABLE;
    sai_object_id_t lbp;
    OK(ba->create_bridge_port(&lbp, g_sw, 3, bpa));
    CHECK(tp::port(17).learn == tp::LEARN_DISABLE && tp::port(21).learn == tp::LEARN_DISABLE);
    CHECK_EQ(ba->create_bridge_port(&dup, g_sw, 3, bpa), SAI_STATUS_ITEM_ALREADY_EXISTS);
    bpa[1].value.oid = p4;   // a member cannot have its own
    CHECK_EQ(ba->create_bridge_port(&dup, g_sw, 3, bpa), SAI_STATUS_INVALID_PARAMETER);
    a.id = SAI_BRIDGE_PORT_ATTR_FDB_LEARNING_MODE;
    a.value.s32 = SAI_BRIDGE_PORT_FDB_LEARNING_MODE_HW;
    OK(ba->set_bridge_port_attribute(lbp, &a));
    CHECK(tp::port(17).learn == tp::LEARN_HW && tp::port(21).learn == tp::LEARN_HW);
    sai_object_id_t v300, vm;
    a.id = SAI_VLAN_ATTR_VLAN_ID;
    a.value.u16 = 300;
    OK(va->create_vlan(&v300, g_sw, 1, &a));
    sai_attribute_t vma[3];
    vma[0].id = SAI_VLAN_MEMBER_ATTR_VLAN_ID;
    vma[0].value.oid = v300;
    vma[1].id = SAI_VLAN_MEMBER_ATTR_BRIDGE_PORT_ID;
    vma[1].value.oid = lbp;
    vma[2].id = SAI_VLAN_MEMBER_ATTR_VLAN_TAGGING_MODE;
    vma[2].value.s32 = SAI_VLAN_TAGGING_MODE_TAGGED;
    OK(va->create_vlan_member(&vm, g_sw, 3, vma));
    CHECK(vmember(300, 17) && !vuntagged(300, 17) && vmember(300, 21));
    a.id = SAI_VLAN_MEMBER_ATTR_VLAN_TAGGING_MODE;
    a.value.s32 = SAI_VLAN_TAGGING_MODE_UNTAGGED;
    OK(va->set_vlan_member_attribute(vm, &a));
    CHECK(vuntagged(300, 17) && vuntagged(300, 21));

    // a port joining later gets the LAG's VLANs, PVID and learning; leaving undoes them
    a.id = SAI_BRIDGE_PORT_ATTR_FDB_LEARNING_MODE;
    a.value.s32 = SAI_BRIDGE_PORT_FDB_LEARNING_MODE_DISABLE;
    OK(ba->set_bridge_port_attribute(lbp, &a));
    ma[1].value.oid = p6;
    OK(la->create_lag_member(&m6, g_sw, 2, ma));
    CHECK(vmember(300, 25) && vuntagged(300, 25));
    CHECK_EQ(tp::port(25).pvid, 300);
    CHECK(tp::port(25).learn == tp::LEARN_DISABLE);
    CHECK_EQ(lagm(tid).size(), 3u);
    OK(la->remove_lag_member(m6));
    CHECK(!vmember(300, 25));
    CHECK_EQ(tp::port(25).pvid, 1);
    CHECK(tp::port(25).learn == tp::LEARN_HW);
    CHECK_EQ(lagm(tid).size(), 2u);
    CHECK_EQ(la->remove_lag_member(m6), SAI_STATUS_INVALID_OBJECT_ID);

    // FDB on the LAG: the entry points at the trunk; hardware learning on
    // the trunk is reported with the LAG's bridge port
    sai_fdb_entry_t e = fdb_key(v300, 0x91);
    sai_attribute_t fat[2];
    fat[0].id = SAI_FDB_ENTRY_ATTR_TYPE;
    fat[0].value.s32 = SAI_FDB_ENTRY_TYPE_STATIC;
    fat[1].id = SAI_FDB_ENTRY_ATTR_BRIDGE_PORT_ID;
    fat[1].value.oid = lbp;
    OK(fa->create_fdb_entry(&e, 2, fat));
    {
        tp::L2Entry le;
        CHECK(l2_find(e.mac_address, 300, le) && le.dest == tp::lag_dest(tid));
    }
    FdbEv ev;
    uint8_t mac[6] = {0x00, 0xaa, 0xbb, 0xcc, 0xdd, 0x92};
    std::memcpy(ev.mac, mac, 6);
    ev.vid = 300;
    ev.hw_port = tp::lag_dest(tid);
    ev.learned = true;
    inject_fdb(ev);
    CHECK(wait_for([&] { return !g_fdb_events.empty(); }));
    {
        std::lock_guard<std::mutex> l(g_nmu);
        CHECK_EQ(g_fdb_events[0].first, SAI_FDB_EVENT_LEARNED);
        CHECK_EQ(g_fdb_events[0].second, lbp);
        g_fdb_events.clear();
    }
    sai_attribute_t fl[2];
    fl[0].id = SAI_FDB_FLUSH_ATTR_BRIDGE_PORT_ID;
    fl[0].value.oid = lbp;
    fl[1].id = SAI_FDB_FLUSH_ATTR_ENTRY_TYPE;
    fl[1].value.s32 = SAI_FDB_FLUSH_ENTRY_TYPE_DYNAMIC;
    OK(fa->flush_fdb_entries(g_sw, 2, fl));
    CHECK_EQ(tp::l2_entries().size(), 1u);   // the static one stays
    CHECK_EQ(ba->remove_bridge_port(lbp), SAI_STATUS_OBJECT_IN_USE);
    OK(fa->remove_fdb_entry(&e));
    OK(va->remove_vlan_member(vm));
    CHECK(!vmember(300, 17) && !vmember(300, 21));
    OK(va->remove_vlan(v300));
    OK(ba->remove_bridge_port(lbp));
    CHECK(tp::port(17).learn == tp::LEARN_DISABLE);   // removing a bridge port does not touch learning

    // router interface on the LAG: internal VLAN on every member, egress to the trunk
    sai_attribute_t rat[3];
    rat[0].id = SAI_ROUTER_INTERFACE_ATTR_VIRTUAL_ROUTER_ID;
    rat[0].value.oid = get_oid(g_sw, SAI_SWITCH_ATTR_DEFAULT_VIRTUAL_ROUTER_ID);
    rat[1].id = SAI_ROUTER_INTERFACE_ATTR_TYPE;
    rat[1].value.s32 = SAI_ROUTER_INTERFACE_TYPE_PORT;
    rat[2].id = SAI_ROUTER_INTERFACE_ATTR_PORT_ID;
    rat[2].value.oid = lag;
    sai_object_id_t rif;
    OK(ra->create_router_interface(&rif, g_sw, 3, rat));
    uint16_t ivid = tp::rif(rif).vid;
    CHECK(vmember(ivid, 17) && vuntagged(ivid, 17) && vmember(ivid, 21));
    CHECK(tp::port(17).pvid == ivid && tp::port(21).pvid == ivid);
    a.id = SAI_LAG_ATTR_PORT_VLAN_ID;
    OK(la->get_lag_attribute(lag, 1, &a));
    CHECK_EQ(a.value.u16, ivid);
    rat[2].value.oid = p4;   // not on a member
    CHECK_EQ(ra->create_router_interface(&dup, g_sw, 3, rat), SAI_STATUS_INVALID_PARAMETER);
    OK(la->create_lag_member(&m6, g_sw, 2, ma));   // joins the internal VLAN
    CHECK(vmember(ivid, 25) && tp::port(25).pvid == ivid);
    OK(la->remove_lag_member(m6));
    CHECK(!vmember(ivid, 25));
    sai_neighbor_entry_t ne;
    std::memset(&ne, 0, sizeof(ne));
    ne.switch_id = g_sw;
    ne.rif_id = rif;
    ne.ip_address = ip4(10, 1, 0, 2);
    sai_attribute_t na1;
    na1.id = SAI_NEIGHBOR_ENTRY_ATTR_DST_MAC_ADDRESS;
    uint8_t nmac[6] = {0x00, 0x00, 0x5e, 0x00, 0x53, 0x07};
    std::memcpy(na1.value.mac, nmac, 6);
    OK(na->create_neighbor_entry(&ne, 1, &na1));
    CHECK_EQ(egr_of(first_of(tp::hosts()).egress).dest, tp::lag_dest(tid));
    CHECK_EQ(la->remove_lag(lag), SAI_STATUS_OBJECT_IN_USE);
    OK(na->remove_neighbor_entry(&ne));
    OK(ra->remove_router_interface(rif));
    CHECK(!tp::vlan_exists(ivid));
    CHECK(tp::port(17).pvid == 1 && tp::port(21).pvid == 1);

    // LACP to the CPU; the members' own netdevs (KNET on the physical port) stay
    sai_attribute_t at[3];
    at[0].id = SAI_HOSTIF_ATTR_TYPE;
    at[0].value.s32 = SAI_HOSTIF_TYPE_NETDEV;
    at[1].id = SAI_HOSTIF_ATTR_OBJ_ID;
    at[1].value.oid = p4;
    name_attr(at[2], "Ethernet16");
    sai_object_id_t h;
    OK(ha->create_hostif(&h, g_sw, 3, at));
    at[1].value.oid = lag;
    name_attr(at[2], "PortChannel1");
    CHECK_EQ(ha->create_hostif(&dup, g_sw, 3, at), SAI_STATUS_NOT_SUPPORTED);   // teamd makes that netdev
    bool found = false;
    for (auto& n : tp::netifs()) found |= n.name == "Ethernet16" && n.hw == 17;
    CHECK(found);
    sai_attribute_t ta[2];
    ta[0].id = SAI_HOSTIF_TRAP_ATTR_TRAP_TYPE;
    ta[0].value.s32 = SAI_HOSTIF_TRAP_TYPE_LACP;
    ta[1].id = SAI_HOSTIF_TRAP_ATTR_PACKET_ACTION;
    ta[1].value.s32 = SAI_PACKET_ACTION_TRAP;
    sai_object_id_t lt;
    OK(ha->create_hostif_trap(&lt, g_sw, 2, ta));
    CHECK(tp::trap_to_cpu(SAI_HOSTIF_TRAP_TYPE_LACP));
    CHECK(tp::trap_programmed(lt));
    OK(ha->remove_hostif_trap(lt));
    CHECK(!tp::trap_to_cpu(SAI_HOSTIF_TRAP_TYPE_LACP));
    OK(ha->remove_hostif(h));

    OK(la->remove_lag_member(m4));
    OK(la->remove_lag_member(m5));
    CHECK_EQ(lagm(tid).size(), 0u);
    CHECK_EQ(tp::port_lag(p4), (sai_object_id_t)0);
    OK(la->remove_lag(lag));
    CHECK(!lag_exists(tid));
    CHECK_EQ(la->remove_lag(lag), SAI_STATUS_INVALID_OBJECT_ID);
}

static std::string slurp(const std::string& p)
{
    std::ifstream f(p);
    if (!f) return "";
    return std::string((std::istreambuf_iterator<char>(f)), std::istreambuf_iterator<char>());
}
static bool exists(const std::string& p) { return std::ifstream(p).good(); }

// Board port quirks (SAI_OPENBCM_PORT_*): applied to every port at creation
// and after a speed change; fixed FEC / autoneg keep the SAI attribute
// without programming it; bad values refuse the switch.
static void test_port_quirks()
{
    sai_attribute_t a;
    sai_object_id_t ports[64];
    OK(sw->remove_switch(g_sw));
    g_profile["SAI_OPENBCM_FAKE_PORTS"] = "4";
    g_profile["SAI_OPENBCM_FAKE_LANES"] = "4";
    g_profile["SAI_OPENBCM_FAKE_SPEED"] = "100000";
    for (auto kv : std::vector<std::pair<std::string, std::string>>{
             {"SAI_OPENBCM_PORT_INTERFACE", "bogus"}, {"SAI_OPENBCM_PORT_FEC", "on"},
             {"SAI_OPENBCM_PORT_AUTONEG", "0"}, {"SAI_OPENBCM_PORT_IFG", "100"}, {"SAI_OPENBCM_PORT_IFG", "16"},
             {"SAI_OPENBCM_PORT_IFG", "128x"}}) {
        g_profile[kv.first] = kv.second;
        CHECK_EQ(create_switch_default(), SAI_STATUS_INVALID_PARAMETER);
        CHECK(!tp::switch_created());
        g_profile.erase(kv.first);
    }
    g_profile["SAI_OPENBCM_PORT_INTERFACE"] = "caui";
    g_profile["SAI_OPENBCM_PORT_FEC"] = "none";
    g_profile["SAI_OPENBCM_PORT_AUTONEG"] = "off";
    g_profile["SAI_OPENBCM_PORT_IFG"] = "128";
    OK(create_switch_default());
    a.id = SAI_SWITCH_ATTR_PORT_LIST;
    a.value.objlist.count = 64;
    a.value.objlist.list = ports;
    OK(sw->get_switch_attribute(g_sw, 1, &a));
    CHECK_EQ(a.value.objlist.count, 4u);
    for (auto& p : tp::ports()) {
        CHECK(p.iface == "caui");
        CHECK_EQ(p.ifg, 128u);
        CHECK(p.fec == SAI_PORT_FEC_MODE_NONE && !p.autoneg && !p.admin);
    }
    // SONiC's FEC and autoneg: accepted, returned, not programmed.
    a.id = SAI_PORT_ATTR_FEC_MODE;
    a.value.s32 = SAI_PORT_FEC_MODE_RS;
    OK(pa->set_port_attribute(ports[0], &a));
    a.value.s32 = 0;
    OK(pa->get_port_attribute(ports[0], 1, &a));
    CHECK_EQ(a.value.s32, SAI_PORT_FEC_MODE_RS);
    CHECK(tp::port(1).fec == SAI_PORT_FEC_MODE_NONE);
    a.value.s32 = 99;
    CHECK(SAI_STATUS_IS_INVALID_ATTR_VALUE(pa->set_port_attribute(ports[0], &a)));
    a.id = SAI_PORT_ATTR_AUTO_NEG_MODE;
    a.value.booldata = true;
    OK(pa->set_port_attribute(ports[0], &a));
    OK(pa->get_port_attribute(ports[0], 1, &a));
    CHECK(a.value.booldata);
    CHECK(!tp::port(1).autoneg);
    // A speed change applies the quirks again (the SDK re-does the MAC setup).
    int before = tp::port(1).quirk_calls;
    tp::port_forget_quirks(1);
    a.id = SAI_PORT_ATTR_SPEED;
    a.value.u32 = 40000;
    OK(pa->set_port_attribute(ports[0], &a));
    CHECK_EQ(tp::port(1).quirk_calls, before + 2);
    CHECK(tp::port(1).iface == "caui");
    CHECK_EQ(tp::port(1).ifg, 128u);
    for (auto& p : tp::ports())                      // other ports untouched
        if (p.hw != 1) CHECK_EQ(p.quirk_calls, 2);
    OK(sw->remove_switch(g_sw));
    // A quirk the chip refuses (4-lane interface on 1-lane ports) refuses the switch.
    g_profile["SAI_OPENBCM_FAKE_LANES"] = "1";
    g_profile["SAI_OPENBCM_FAKE_SPEED"] = "25000";
    CHECK_EQ(create_switch_default(), SAI_STATUS_FAILURE);
    CHECK(!tp::switch_created());
    // No keys: nothing is touched.
    for (const char* k : {"SAI_OPENBCM_PORT_INTERFACE", "SAI_OPENBCM_PORT_FEC", "SAI_OPENBCM_PORT_AUTONEG",
                          "SAI_OPENBCM_PORT_IFG"})
        g_profile.erase(k);
    OK(create_switch_default());
    for (auto& p : tp::ports()) CHECK_EQ(p.quirk_calls, 0);
    a.id = SAI_SWITCH_ATTR_PORT_LIST;
    a.value.objlist.count = 64;
    a.value.objlist.list = ports;
    OK(sw->get_switch_attribute(g_sw, 1, &a));
    a.id = SAI_PORT_ATTR_FEC_MODE;
    a.value.s32 = SAI_PORT_FEC_MODE_RS;
    OK(pa->set_port_attribute(ports[0], &a));
    CHECK(tp::port(1).fec == SAI_PORT_FEC_MODE_RS);
}

static void test_ext_phy();   // tests/test_extphy.inc
static void test_macsec();    // tests/test_macsec.inc
static void test_dump_and_teardown()
{
    const char* path = "test-dump.txt";
    OK(sai_dbg_generate_dump(path));
    std::ifstream f(path);
    std::string first;
    std::getline(f, first);
    CHECK(first == "sai-openbcm dump");
    std::remove(path);

    OK(sw->remove_switch(g_sw));
    CHECK(!tp::switch_created());
    sai_attribute_t a;
    a.id = SAI_SWITCH_ATTR_PORT_NUMBER;
    CHECK_EQ(sw->get_switch_attribute(g_sw, 1, &a), SAI_STATUS_UNINITIALIZED);
    // re-create works (state fully reset), then uninitialize removes it.
    // This time with the temperature publisher (fan control's files).
    g_profile["SAI_OPENBCM_FAKE_PORTS"] = "4";
    g_profile["SAI_OPENBCM_FAKE_LANES"] = "1";
    g_profile["SAI_OPENBCM_FAKE_SPEED"] = "25000";
    const std::string tdir = "test-temp", out = tdir + "/temperature", prom = tdir + "/asic.prom";
    std::remove(out.c_str());
    std::remove(prom.c_str());
    std::remove(tdir.c_str());   // the publisher creates a missing parent directory
    g_profile["SAI_OPENBCM_TEMP_FILE"] = out;
    g_profile["SAI_OPENBCM_TEMP_PROM"] = prom;
    g_profile["SAI_OPENBCM_TEMP_INTERVAL"] = "1";
    OK(create_switch_default());
    OK(sw->get_switch_attribute(g_sw, 1, &a));
    CHECK_EQ(a.value.u32, 4u);
    auto poll = [](std::function<bool()> f) {
        for (int i = 0; i < 400; i++) {
            if (f()) return true;
            std::this_thread::sleep_for(std::chrono::milliseconds(10));
        }
        return false;
    };
    CHECK(poll([&] { return exists(out) && exists(prom); }));
    CHECK(slurp(out) == "50.5\n");
    std::string p = slurp(prom);
    CHECK(p.find("switch_asic_temperature_max_celsius{unit=\"0\"} 50.5\n") != std::string::npos);
    CHECK(p.find("switch_asic_temperature_celsius{unit=\"0\",sensor=\"pvtmon7\"} 50.5\n") != std::string::npos);
    CHECK(p.find("switch_asic_temperature_peak_celsius{unit=\"0\",sensor=\"pvtmon0\"} 40.5\n") != std::string::npos);
    CHECK(p.find("switch_asic_temperature_read_timestamp_seconds{unit=\"0\"} ") != std::string::npos);
    tp::set_temperature_fail(true);   // failed read: both files go
    CHECK(poll([&] { return !exists(out) && !exists(prom); }));
    tp::set_temperatures({612, 2000});   // 200 C: implausible, still no file (set before the
    tp::set_temperature_fail(false);     // failure ends: no read sees the old values)
    std::this_thread::sleep_for(std::chrono::milliseconds(1500));
    CHECK(!exists(out));
    tp::set_temperatures({612, 433});
    CHECK(poll([&] { return slurp(out) == "61.2\n"; }));
    OK(sw->remove_switch(g_sw));   // removes the files
    CHECK(!exists(out) && !exists(prom));
    g_profile.erase("SAI_OPENBCM_TEMP_FILE");
    g_profile.erase("SAI_OPENBCM_TEMP_PROM");
    g_profile["SAI_OPENBCM_TEMP_INTERVAL"] = "1";
    OK(create_switch_default());   // no file keys: no publisher
    std::this_thread::sleep_for(std::chrono::milliseconds(1200));
    CHECK(!exists(out));
    std::remove(tdir.c_str());
    test_port_quirks();
    test_ext_phy();
    test_macsec();
    OK(sai_api_uninitialize());
    CHECK(!tp::switch_created());
    CHECK_EQ(sai_api_uninitialize(), SAI_STATUS_UNINITIALIZED);
}

// Capacity attributes orchagent reads (AclOrch, RouteOrch, BufferOrch, CRM):
// fake constants, free counts follow the model.
static void test_capacity()
{
    auto get = [](sai_attr_id_t id) {
        sai_attribute_t a;
        std::memset(&a, 0, sizeof(a));
        a.id = id;
        CHECK_EQ(sw->get_switch_attribute(g_sw, 1, &a), SAI_STATUS_SUCCESS);
        return a;
    };
    CHECK_EQ(get(SAI_SWITCH_ATTR_ACL_ENTRY_MINIMUM_PRIORITY).value.u32, 0u);
    CHECK_EQ(get(SAI_SWITCH_ATTR_ACL_ENTRY_MAXIMUM_PRIORITY).value.u32, 0x7fffffffu);
    sai_attribute_t both[2];
    both[0].id = SAI_SWITCH_ATTR_ACL_ENTRY_MINIMUM_PRIORITY;   // AclOrch gets both in one call
    both[1].id = SAI_SWITCH_ATTR_ACL_ENTRY_MAXIMUM_PRIORITY;
    OK(sw->get_switch_attribute(g_sw, 2, both));
    CHECK(both[0].value.u32 < both[1].value.u32);
    CHECK_EQ(get(SAI_SWITCH_ATTR_MAX_ACL_ACTION_COUNT).value.u32, 3u);   // test_acl checks the lists
    int32_t acts[8];
    for (sai_attr_id_t st : {SAI_SWITCH_ATTR_ACL_STAGE_INGRESS, SAI_SWITCH_ATTR_ACL_STAGE_EGRESS}) {
        sai_attribute_t a;
        a.id = st;
        a.value.aclcapability.is_action_list_mandatory = true;
        a.value.aclcapability.action_list.count = 8;
        a.value.aclcapability.action_list.list = acts;
        OK(sw->get_switch_attribute(g_sw, 1, &a));
        CHECK_EQ(a.value.aclcapability.action_list.count, st == SAI_SWITCH_ATTR_ACL_STAGE_INGRESS ? 3u : 0u);
        CHECK(!a.value.aclcapability.is_action_list_mandatory);
    }
    // ECMP: the fake's limits (test_ecmp checks the free counts as groups come and go)
    CHECK_EQ(get(SAI_SWITCH_ATTR_NUMBER_OF_ECMP_GROUPS).value.u32, 4096u);
    CHECK_EQ(get(SAI_SWITCH_ATTR_ECMP_MEMBERS).value.u32, 64u);
    CHECK_EQ(get(SAI_SWITCH_ATTR_MAX_ECMP_MEMBER_COUNT).value.u32, 16384u);
    CHECK_EQ(get(SAI_SWITCH_ATTR_AVAILABLE_NEXT_HOP_GROUP_ENTRY).value.u32, 4096u);
    CHECK_EQ(get(SAI_SWITCH_ATTR_AVAILABLE_NEXT_HOP_GROUP_MEMBER_ENTRY).value.u32, 16384u);
    CHECK_EQ(get(SAI_SWITCH_ATTR_TOTAL_BUFFER_SIZE).value.u64, 20165ull * 4 * 208 / 1024);   // 16384 KB
    // Still unsupported (orchagent: "not supported" paths): STP instances, NAT, ACL resources, hashes.
    for (sai_attr_id_t id : {SAI_SWITCH_ATTR_MAX_STP_INSTANCE, SAI_SWITCH_ATTR_AVAILABLE_SNAT_ENTRY,
                             SAI_SWITCH_ATTR_AVAILABLE_DNAT_ENTRY, SAI_SWITCH_ATTR_ECMP_HASH,
                             SAI_SWITCH_ATTR_LAG_HASH}) {
        sai_attribute_t a;
        a.id = id;
        CHECK_EQ(sw->get_switch_attribute(g_sw, 1, &a), SAI_STATUS_ATTR_NOT_SUPPORTED_0);
    }

    // Free entries: capacity minus what the model holds.
    uint32_t v4 = get(SAI_SWITCH_ATTR_AVAILABLE_IPV4_ROUTE_ENTRY).value.u32;
    uint32_t v6 = get(SAI_SWITCH_ATTR_AVAILABLE_IPV6_ROUTE_ENTRY).value.u32;
    uint32_t n4 = get(SAI_SWITCH_ATTR_AVAILABLE_IPV4_NEIGHBOR_ENTRY).value.u32;
    uint32_t n6 = get(SAI_SWITCH_ATTR_AVAILABLE_IPV6_NEIGHBOR_ENTRY).value.u32;
    uint32_t nh = get(SAI_SWITCH_ATTR_AVAILABLE_IPV4_NEXTHOP_ENTRY).value.u32;
    uint32_t fdb = get(SAI_SWITCH_ATTR_AVAILABLE_FDB_ENTRY).value.u32;
    size_t r4 = 0, r6 = 0;
    for (auto& r : tp::routes()) (r.addr.v6 ? r6 : r4)++;
    CHECK_EQ(v4, 16384u - r4);
    CHECK_EQ(v6, 8192u - r6);
    CHECK_EQ(nh, 32768u - tp::egress_objects().size());
    CHECK_EQ(fdb, 32768u - tp::l2_entries().size());
    CHECK_EQ(get(SAI_SWITCH_ATTR_AVAILABLE_IPV6_NEXTHOP_ENTRY).value.u32, nh);
    sai_route_entry_t d4 = route(198, 51, 100, 0, 24);
    sai_attribute_t drop;
    drop.id = SAI_ROUTE_ENTRY_ATTR_PACKET_ACTION;
    drop.value.s32 = SAI_PACKET_ACTION_DROP;
    OK(rta->create_route_entry(&d4, 1, &drop));
    CHECK_EQ(get(SAI_SWITCH_ATTR_AVAILABLE_IPV4_ROUTE_ENTRY).value.u32, v4 - 1);
    CHECK_EQ(get(SAI_SWITCH_ATTR_AVAILABLE_IPV6_ROUTE_ENTRY).value.u32, v6);
    OK(rta->remove_route_entry(&d4));
    CHECK_EQ(get(SAI_SWITCH_ATTR_AVAILABLE_IPV4_ROUTE_ENTRY).value.u32, v4);
    sai_ip_address_t h6;
    std::memset(&h6, 0, sizeof(h6));
    h6.addr_family = SAI_IP_ADDR_FAMILY_IPV6;
    h6.addr.ip6[0] = 0x20;
    h6.addr.ip6[15] = 9;
    const uint8_t m9[6] = {0x02, 0, 0, 0, 0, 9};
    tp::backend_host_insert(0, h6, first_of(tp::egress_objects()).id);   // an IPv6 host takes 2 entries
    tp::backend_l2_insert(m9, 1, 0);
    CHECK_EQ(get(SAI_SWITCH_ATTR_AVAILABLE_FDB_ENTRY).value.u32, fdb - 1);
    // A neighbor needs a host entry and an egress object; here the host table is the bound.
    CHECK(n4 < nh);
    CHECK_EQ(get(SAI_SWITCH_ATTR_AVAILABLE_IPV4_NEIGHBOR_ENTRY).value.u32, n4 - 2);
    CHECK_EQ(n6, n4 / 2);
    CHECK_EQ(get(SAI_SWITCH_ATTR_AVAILABLE_IPV6_NEIGHBOR_ENTRY).value.u32, (n4 - 2) / 2);
    tp::backend_host_erase(0, h6);
    tp::backend_l2_erase(m9, 1);
    CHECK_EQ(get(SAI_SWITCH_ATTR_AVAILABLE_IPV4_NEIGHBOR_ENTRY).value.u32, n4);
}

#include "test_acl.inc"
#include "test_qos.inc"

// SAI_OPENBCM_DIAG_SOCKET: one command per connection, output back, a few refused.
static std::string diag_cmd(const std::string& path, const std::string& line)
{
    int fd = ::socket(AF_UNIX, SOCK_STREAM, 0);
    struct sockaddr_un a;
    std::memset(&a, 0, sizeof(a));
    a.sun_family = AF_UNIX;
    std::snprintf(a.sun_path, sizeof(a.sun_path), "%s", path.c_str());
    if (fd < 0 || ::connect(fd, (struct sockaddr*)&a, sizeof(a)) != 0) {
        if (fd >= 0) ::close(fd);
        return "(no connect)";
    }
    (void)!::write(fd, line.data(), line.size());
    ::shutdown(fd, SHUT_WR);
    std::string out;
    char buf[256];
    ssize_t n;
    while ((n = ::read(fd, buf, sizeof(buf))) > 0) out.append(buf, (size_t)n);
    ::close(fd);
    return out;
}

static void test_diag()
{
    const char* tmp = std::getenv("TMPDIR");
    std::string dir = std::string(tmp && *tmp ? tmp : "/tmp") + "/sai-openbcm-test-" + std::to_string(::getpid());
    std::string path = dir + "/diag.sock";
    tp::diag_start("");   // no key: nothing
    struct stat sb;
    CHECK(::stat(path.c_str(), &sb) != 0);
    tp::diag_start(path);
    CHECK(::stat(path.c_str(), &sb) == 0 && S_ISSOCK(sb.st_mode) && (sb.st_mode & 0777) == 0600);
    CHECK(::stat(dir.c_str(), &sb) == 0 && (sb.st_mode & 0777) == 0700);
    size_t before = tp::shell_log().size();
    CHECK(diag_cmd(path, "l2 station show\n") == "fake: l2 station show\n");
    CHECK(diag_cmd(path, "  l3 intf show  \r\n") == "fake: l3 intf show\n");
    CHECK_EQ(tp::shell_log().size(), before + 2);
    for (const char* bad : {"exit\n", "Quit\n", "init all\n", "rcload x\n", "!ls\n", "ps; exit\n", "\n"})
        CHECK(diag_cmd(path, bad).rfind("openbcm-diag: ", 0) == 0);
    CHECK_EQ(tp::shell_log().size(), before + 2);
    tp::diag_stop();
    CHECK(::stat(path.c_str(), &sb) != 0);
    CHECK(diag_cmd(path, "l2 show\n") == "(no connect)");
    ::rmdir(dir.c_str());
}

#include "test_extphy.inc"
#include "test_macsec.inc"

int main()
{
    test_oid_encoding();
    test_init_and_backend_selection();
    test_switch_create();
    test_ports();
    test_vlan_bridge();
    test_fdb();
    test_hostif();
    test_l3();
    test_vrf();
    test_ecmp();
    test_lag();
    test_temperature();
    test_capabilities();
    test_capacity();
    test_policer_copp();
    test_acl();
    test_qos();
    test_l2_extras();
    test_diag();
    test_dump_and_teardown();
    std::printf("%d checks, %d failures\n", g_checks, g_fail);
    return g_fail ? 1 : 0;
}
