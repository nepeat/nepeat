// SPDX-License-Identifier: Apache-2.0
// Internal state of the SAI layer: object model, OID encoding, helpers.
// One global context; every SAI entry point takes its recursive mutex.
#pragma once

extern "C" {
#include <sai.h>
}

#include "backend.h"

#include <atomic>
#include <condition_variable>
#include <cstdarg>
#include <cstring>
#include <deque>
#include <map>
#include <memory>
#include <mutex>
#include <set>
#include <string>
#include <thread>
#include <vector>

namespace ob {

// ------------------------------------------------------------------ logging
bool log_enabled(sai_api_t api, sai_log_level_t lvl);
void log_set_level(sai_api_t api, sai_log_level_t lvl);
void log_msg(sai_api_t api, sai_log_level_t lvl, const char* fmt, ...) __attribute__((format(printf, 3, 4)));
void log_configure(bool to_stderr);

#define OB_LOG(api, lvl, ...)                                                   \
    do { if (::ob::log_enabled((api), (lvl))) ::ob::log_msg((api), (lvl), __VA_ARGS__); } while (0)
#define OB_ERR(api, ...) OB_LOG(api, SAI_LOG_LEVEL_ERROR, __VA_ARGS__)
#define OB_WARN(api, ...) OB_LOG(api, SAI_LOG_LEVEL_WARN, __VA_ARGS__)
#define OB_NOTICE(api, ...) OB_LOG(api, SAI_LOG_LEVEL_NOTICE, __VA_ARGS__)
#define OB_INFO(api, ...) OB_LOG(api, SAI_LOG_LEVEL_INFO, __VA_ARGS__)
#define OB_DBG(api, ...) OB_LOG(api, SAI_LOG_LEVEL_DEBUG, __VA_ARGS__)

// --------------------------------------------------------------------- OIDs
// 63..56 object type, 55..48 switch index, 47..0 per-type index.
constexpr unsigned OID_TYPE_SHIFT = 56;
constexpr unsigned OID_SWITCH_SHIFT = 48;
constexpr uint64_t OID_INDEX_MASK = (1ull << 48) - 1;

inline sai_object_id_t make_oid(sai_object_type_t t, uint64_t index, uint32_t sw = 0)
{
    return ((uint64_t)(t & 0xff) << OID_TYPE_SHIFT) | ((uint64_t)(sw & 0xff) << OID_SWITCH_SHIFT) |
           (index & OID_INDEX_MASK);
}
inline sai_object_type_t oid_type(sai_object_id_t oid)
{
    if (oid == SAI_NULL_OBJECT_ID) return SAI_OBJECT_TYPE_NULL;
    uint32_t t = (uint32_t)(oid >> OID_TYPE_SHIFT);
    if (t == SAI_OBJECT_TYPE_NULL || t >= SAI_OBJECT_TYPE_MAX) return SAI_OBJECT_TYPE_NULL;
    return (sai_object_type_t)t;
}
inline uint64_t oid_index(sai_object_id_t oid) { return oid & OID_INDEX_MASK; }
inline uint32_t oid_switch(sai_object_id_t oid) { return (uint32_t)((oid >> OID_SWITCH_SHIFT) & 0xff); }

// ------------------------------------------------------- attribute storage
// Attributes we accept and keep but do not program into hardware ("stubbed"
// in the README). Only scalar SAI value types (no lists), so the union can
// be copied whole.
enum class Kind : uint8_t { Bool, U8, U16, U32, U64, S32, Oid, Mac, Ip };
struct StubDef {
    sai_attr_id_t id;
    Kind kind;
    uint64_t def;           // default (integers / bool / oid)
    bool create_only;
};
using StubMap = std::map<sai_attr_id_t, sai_attribute_value_t>;

const StubDef* find_stub(const StubDef* defs, size_t n, sai_attr_id_t id);
void stub_default(const StubDef& d, sai_attribute_value_t& v);

// Status for attribute index i (SAI encodes the index into the code).
inline sai_status_t attr_status(sai_status_t base0, uint32_t i) { return base0 - (sai_status_t)i; }
inline sai_status_t attr_not_supported(uint32_t i) { return attr_status(SAI_STATUS_ATTR_NOT_SUPPORTED_0, i); }
inline sai_status_t attr_unknown(uint32_t i) { return attr_status(SAI_STATUS_UNKNOWN_ATTRIBUTE_0, i); }
inline sai_status_t attr_invalid(uint32_t i) { return attr_status(SAI_STATUS_INVALID_ATTR_VALUE_0, i); }
inline sai_status_t attr_not_implemented(uint32_t i) { return attr_status(SAI_STATUS_ATTR_NOT_IMPLEMENTED_0, i); }
// A backend failure while handling attribute i: re-index the generic codes.
sai_status_t attr_backend_status(sai_status_t st, uint32_t i);

template <typename L, typename T>
sai_status_t fill_list(L& list, const std::vector<T>& v)
{
    if (list.count < v.size() || (!v.empty() && list.list == nullptr)) {
        list.count = (uint32_t)v.size();
        return SAI_STATUS_BUFFER_OVERFLOW;
    }
    for (size_t i = 0; i < v.size(); i++) list.list[i] = v[i];
    list.count = (uint32_t)v.size();
    return SAI_STATUS_SUCCESS;
}

const sai_attribute_t* find_attr(uint32_t n, const sai_attribute_t* a, sai_attr_id_t id, uint32_t* idx = nullptr);

std::string mac_str(const uint8_t m[6]);
IpAddr to_ip(const sai_ip_address_t& a);
IpPrefix to_prefix(const sai_ip_prefix_t& p);
sai_ip_address_t from_ip(const IpAddr& a);

// ------------------------------------------------------------------ objects
struct PortObj {
    sai_object_id_t oid = 0;
    int hw = -1;
    bool cpu = false;
    std::string sdk_name;
    std::vector<uint32_t> lanes;
    bool admin = false;
    uint32_t mtu = 1514;
    int32_t fec = SAI_PORT_FEC_MODE_NONE;
    bool autoneg = false;
    int32_t loopback = SAI_PORT_LOOPBACK_MODE_NONE;
    uint16_t pvid = 1;
    sai_object_id_t lag = 0;        // LAG this port is a member of
    sai_port_oper_status_t oper = SAI_PORT_OPER_STATUS_DOWN;
    std::vector<sai_object_id_t> queues, ipgs;
    StubMap stub;
};

// LAG = SDK trunk. Bridge ports, port RIFs and FDB entries may sit on a LAG;
// VLAN membership, PVID and learning are then applied to every member port.
struct LagObj {
    sai_object_id_t oid = 0;
    int tid = -1;
    std::vector<sai_object_id_t> members;   // LAG_MEMBER oids, creation order
    uint16_t pvid = 1;
    StubMap stub;
};
struct LagMemberObj {
    sai_object_id_t lag = 0, port = 0;
    bool egress_disable = false, ingress_disable = false;
    StubMap stub;
};

struct VlanObj {
    sai_object_id_t oid = 0;
    uint16_t vid = 0;
    std::set<sai_object_id_t> members;
    StubMap stub;
};

struct VlanMemberObj {
    sai_object_id_t vlan = 0, bport = 0;
    int32_t tagging = SAI_VLAN_TAGGING_MODE_UNTAGGED;
};

struct BridgeObj {
    int32_t type = SAI_BRIDGE_TYPE_1Q;
    std::set<sai_object_id_t> ports;
    StubMap stub;
};

struct BridgePortObj {
    int32_t type = SAI_BRIDGE_PORT_TYPE_PORT;
    sai_object_id_t port = 0, bridge = 0;
    bool admin = false;
    int32_t learn = SAI_BRIDGE_PORT_FDB_LEARNING_MODE_HW;
    std::set<sai_object_id_t> vlan_members;
    StubMap stub;
};

struct FdbKey {
    uint8_t mac[6];
    sai_object_id_t bv;
    bool operator<(const FdbKey& o) const
    {
        int c = std::memcmp(mac, o.mac, 6);
        return c ? c < 0 : bv < o.bv;
    }
};
struct FdbObj {
    int32_t type = SAI_FDB_ENTRY_TYPE_DYNAMIC;
    sai_object_id_t bport = 0;
    int32_t action = SAI_PACKET_ACTION_FORWARD;
    StubMap stub;
};

struct HostifObj {
    int32_t type = SAI_HOSTIF_TYPE_NETDEV;
    sai_object_id_t obj = 0;
    std::string name;
    bool oper = false;
    int32_t vlan_tag = SAI_HOSTIF_VLAN_TAG_STRIP;
    uint32_t queue = 0;
    int netif = -1, filter = -1;
};
struct TrapGroupObj { bool admin = true; uint32_t queue = 0; sai_object_id_t policer = 0; };
struct TrapObj {
    int32_t type = 0;
    int32_t action = SAI_PACKET_ACTION_TRAP;
    uint32_t priority = 0;
    sai_object_id_t group = 0;
    bool programmed = false;
    StubMap stub;
};
struct HostifTableEntryObj {
    int32_t type = SAI_HOSTIF_TABLE_ENTRY_TYPE_WILDCARD;
    sai_object_id_t obj = 0, trap = 0;
    int32_t channel = SAI_HOSTIF_TABLE_ENTRY_CHANNEL_TYPE_NETDEV_PHYSICAL_PORT;
    sai_object_id_t hostif = 0;
};
struct QueueObj { int32_t type = SAI_QUEUE_TYPE_UNICAST; uint8_t index = 0; sai_object_id_t port = 0; };
struct IpgObj { uint8_t index = 0; sai_object_id_t port = 0; StubMap stub; };

// L3
struct RifObj {
    int32_t type = SAI_ROUTER_INTERFACE_TYPE_PORT;
    sai_object_id_t vr = 0, port = 0, vlan = 0;
    uint8_t mac[6] = {0};
    uint32_t mtu = 1514;
    bool v4 = true, v6 = true;
    uint16_t vid = 0;             // SDK VLAN the L3 interface sits on
    bool internal_vlan = false;   // VLAN allocated for a port RIF
    int intf = -1;                // SDK L3 interface id
    int station = -1;             // my-station entry if mac != switch mac
    StubMap stub;
};
struct NeighborKey {
    sai_object_id_t rif;
    IpAddr ip;
    bool operator<(const NeighborKey& o) const { return rif != o.rif ? rif < o.rif : ip < o.ip; }
};
struct NeighborObj { uint8_t mac[6] = {0}; bool no_host = false; int egress = -1; int hw = -1; bool host = false; };
struct NextHopObj { int32_t type = SAI_NEXT_HOP_TYPE_IP; IpAddr ip; sai_object_id_t rif = 0; int egress = -1; };
struct RouteKey {
    sai_object_id_t vr;
    IpPrefix p;
    bool operator<(const RouteKey& o) const { return vr != o.vr ? vr < o.vr : p < o.p; }
};
struct RouteObj { int32_t action = SAI_PACKET_ACTION_FORWARD; sai_object_id_t nh = 0; int egress = -1; StubMap stub; };

// --------------------------------------------------------------- events
struct Event {
    enum { Link, Fdb, Stop } kind;
    int hw = -1;
    bool up = false;
    FdbEvent fdb;
};

struct Ctx {
    std::recursive_mutex mu;
    bool api_initialized = false;
    sai_service_method_table_t smt{};
    std::unique_ptr<Backend> be;
    bool switch_created = false;
    sai_object_id_t switch_oid = 0;
    sai_mac_t src_mac = {0x02, 0x00, 0x00, 0x00, 0x00, 0x01};
    StubMap switch_stub;
    uint32_t fdb_aging = 0;

    // notification pointers (attr id -> pointer); typed copies of the ones we raise
    std::map<sai_attr_id_t, sai_pointer_t> notify;
    sai_port_state_change_notification_fn port_state_cb = nullptr;
    sai_fdb_event_notification_fn fdb_cb = nullptr;

    // objects
    std::map<sai_object_id_t, PortObj> ports;
    std::map<int, sai_object_id_t> hw2port;
    std::vector<sai_object_id_t> port_list;            // front panel, SDK order
    sai_object_id_t cpu_port = 0;
    std::map<sai_object_id_t, LagObj> lags;
    std::map<sai_object_id_t, LagMemberObj> lag_members;
    std::map<sai_object_id_t, VlanObj> vlans;
    std::map<uint16_t, sai_object_id_t> vid2vlan;
    std::map<sai_object_id_t, VlanMemberObj> vlan_members;
    std::map<sai_object_id_t, BridgeObj> bridges;
    sai_object_id_t default_bridge = 0;
    std::map<sai_object_id_t, BridgePortObj> bridge_ports;
    std::map<FdbKey, FdbObj> fdbs;
    sai_object_id_t default_stp = 0, default_vr = 0, default_trap_group = 0;
    std::map<sai_object_id_t, HostifObj> hostifs;
    std::map<sai_object_id_t, TrapGroupObj> trap_groups;
    std::map<sai_object_id_t, TrapObj> traps;
    std::map<sai_object_id_t, HostifTableEntryObj> table_entries;
    std::map<sai_object_id_t, QueueObj> queues;
    std::map<sai_object_id_t, IpgObj> ipgs;
    std::map<sai_object_id_t, RifObj> rifs;
    std::map<NeighborKey, NeighborObj> neighbors;
    std::map<sai_object_id_t, NextHopObj> next_hops;
    std::map<RouteKey, RouteObj> routes;
    std::set<uint16_t> internal_vlans;                  // allocated for port RIFs
    int switch_station = -1;
    std::map<sai_object_type_t, uint64_t> next_index;

    // event delivery (backend threads -> notifier thread -> SAI callbacks)
    std::mutex ev_mu;
    std::condition_variable ev_cv;
    std::deque<Event> events;
    std::thread notifier;
    std::atomic<uint64_t> events_delivered{0};

    sai_object_id_t alloc(sai_object_type_t t) { return make_oid(t, ++next_index[t]); }
};

Ctx& ctx();
std::string profile_get(const char* key, const char* def = "");

// Run an API body under the global lock; exceptions never cross into C.
template <typename F>
sai_status_t guarded(sai_api_t api, const char* fn, F&& f, bool need_switch = true)
{
    try {
        std::lock_guard<std::recursive_mutex> g(ctx().mu);
        if (!ctx().api_initialized) {
            OB_ERR(api, "%s: sai_api_initialize() not called", fn);
            return SAI_STATUS_UNINITIALIZED;
        }
        if (need_switch && !ctx().switch_created) {
            OB_ERR(api, "%s: no switch created", fn);
            return SAI_STATUS_UNINITIALIZED;
        }
        return f();
    } catch (const std::exception& e) {
        OB_ERR(api, "%s: exception: %s", fn, e.what());
    } catch (...) {
        OB_ERR(api, "%s: unknown exception", fn);
    }
    return SAI_STATUS_FAILURE;
}

// Object lookups by OID (nullptr if absent or wrong type).
template <typename M>
typename M::mapped_type* lookup(M& m, sai_object_id_t oid)
{
    auto it = m.find(oid);
    return it == m.end() ? nullptr : &it->second;
}

// Attribute capability table entry (sai_query_attribute_capability).
struct AttrCap { sai_object_type_t ot; sai_attr_id_t id; bool create, set, get; };
const std::vector<AttrCap>& attr_caps();

// API tables (defined in the per-object files).
extern const sai_switch_api_t switch_api;
extern const sai_port_api_t port_api;
extern const sai_vlan_api_t vlan_api;
extern const sai_bridge_api_t bridge_api;
extern const sai_fdb_api_t fdb_api;
extern const sai_stp_api_t stp_api;
extern const sai_hostif_api_t hostif_api;
extern const sai_queue_api_t queue_api;
extern const sai_buffer_api_t buffer_api;
extern const sai_virtual_router_api_t vr_api;
extern const sai_router_interface_api_t rif_api;
extern const sai_neighbor_api_t neighbor_api;
extern const sai_next_hop_api_t next_hop_api;
extern const sai_route_api_t route_api;
extern const sai_lag_api_t lag_api;

// Cross-object helpers implemented in the object files.
void switch_build_objects();                         // after backend init
void switch_clear_objects();
void events_start();
void events_stop();
void push_event(const Event& e);
sai_status_t port_stats_get(int hw, uint32_t n, const sai_stat_id_t* ids, uint64_t* out);
bool port_stat_supported(sai_stat_id_t id);
// Attachments: a front-panel PORT that is not a LAG member, or a LAG.
bool attach_valid(sai_object_id_t obj);
int attach_dest(sai_object_id_t obj);                // port hw or lag_dest(tid); -1 if neither
std::vector<int> attach_phys(sai_object_id_t obj);   // the port, or the LAG's member ports
sai_status_t attach_pvid_set(sai_object_id_t obj, uint16_t vid);
int bridge_port_dest(sai_object_id_t bport);         // -1 if not a PORT/LAG bridge port
std::vector<int> bridge_port_phys(sai_object_id_t bport);
sai_object_id_t bridge_port_of(sai_object_id_t obj); // bridge port on a PORT or LAG, 0 if none
sai_object_id_t bridge_port_of_dest(int dest);       // 0 if none
// A port joins / leaves a LAG: apply / undo the LAG's bridge port (VLAN
// membership, learning) and port-RIF VLAN on that port.
sai_status_t bridge_port_member_join(sai_object_id_t lag, int hw);
void bridge_port_member_leave(sai_object_id_t lag, int hw);
sai_status_t rif_member_join(sai_object_id_t lag, int hw);
void rif_member_leave(sai_object_id_t lag, int hw);
// ASIC temperature: SWITCH TEMP attributes, and the file publisher thread.
bool is_temp_attr(sai_attr_id_t id);
sai_status_t switch_temp_get(uint32_t i, sai_attribute_t& a);
void temp_publisher_start(const BackendConfig& cfg, Backend* be);
void temp_publisher_stop();
sai_status_t vlan_member_create_internal(sai_object_id_t vlan, sai_object_id_t bport, int32_t tagging,
                                         bool program, sai_object_id_t* out);
sai_status_t l3_switch_mac_changed(const uint8_t old_mac[6]);
TrapKind trap_kind(int32_t sai_trap_type);

} // namespace ob
