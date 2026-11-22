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
    // QoS (sai_qos.cpp): maps by QosMapKind, scheduler groups (one per queue),
    // PFC, port shaper, buffer profile lists; mirror / sampling (sai_mirror.cpp).
    sai_object_id_t qos_map[(int)QosMapKind::Count] = {};
    uint8_t default_tc = 0;
    int32_t pfc_mode = SAI_PORT_PRIORITY_FLOW_CONTROL_MODE_COMBINED;
    uint8_t pfc = 0, pfc_tx = 0, pfc_rx = 0;
    sai_object_id_t scheduler = 0;
    std::vector<sai_object_id_t> sched_groups, in_buffers, eg_buffers;
    std::vector<sai_object_id_t> mirror_in, mirror_eg;
    sai_object_id_t sample_in = 0, sample_eg = 0;
    sai_object_id_t storm[3] = {0, 0, 0};   // StormKind -> policer (sai_hash.cpp port_storm_attr)
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
    std::string mcgrp;            // GENETLINK: multicast group
    int genl = -1;                // GENETLINK: backend id (Backend::genetlink_create)
};
struct TrapGroupObj { bool admin = true; uint32_t queue = 0; sai_object_id_t policer = 0; };
struct TrapObj {
    int32_t type = 0;
    int32_t action = SAI_PACKET_ACTION_TRAP;
    uint32_t priority = 0;
    sai_object_id_t group = 0;
    bool programmed = false;
    bool full = false;            // the backend's trap_program owns it (queue, policer); no CoPP field entries
    StubMap stub;
};
struct HostifTableEntryObj {
    int32_t type = SAI_HOSTIF_TABLE_ENTRY_TYPE_WILDCARD;
    sai_object_id_t obj = 0, trap = 0;
    int32_t channel = SAI_HOSTIF_TABLE_ENTRY_CHANNEL_TYPE_NETDEV_PHYSICAL_PORT;
    sai_object_id_t hostif = 0;
};
struct QueueObj {
    int32_t type = SAI_QUEUE_TYPE_UNICAST;
    uint8_t index = 0;
    sai_object_id_t port = 0;
    sai_object_id_t group = 0;                  // its scheduler group (QoS backends)
    sai_object_id_t sched = 0, wred = 0, buffer = 0;   // SCHEDULER_PROFILE_ID (direct), WRED, BUFFER profile
    bool pfc_dlr = false;
};
struct IpgObj { uint8_t index = 0; sai_object_id_t port = 0; sai_object_id_t buffer = 0; StubMap stub; };

// QoS objects (sai_qos.cpp). id: the backend's, -1 = kept by the SAI layer only.
struct QosMapObj {
    int32_t type = SAI_QOS_MAP_TYPE_DOT1P_TO_TC;
    std::vector<sai_qos_map_t> list;
    int id = -1;
};
struct SchedulerObj {
    int32_t type = SAI_SCHEDULING_TYPE_DWRR;
    uint8_t weight = 1;
    int32_t meter = SAI_METER_TYPE_BYTES;
    uint64_t min_rate = 0, min_burst = 0, max_rate = 0, max_burst = 0;
};
struct SchedGroupObj {
    sai_object_id_t port = 0, parent = 0, sched = 0;
    uint8_t level = 0, max_childs = 1;
    std::vector<sai_object_id_t> children;      // queues
};
struct WredObj { WredCfg cfg; int id = -1; };
struct BufferPoolObj {
    int32_t type = SAI_BUFFER_POOL_TYPE_INGRESS;
    uint64_t size = 0, xoff = 0;
    int32_t mode = SAI_BUFFER_POOL_THRESHOLD_MODE_DYNAMIC;
};
struct BufferProfileObj {
    sai_object_id_t pool = 0;
    uint64_t reserved = 0, static_th = 0, xoff = 0, xon = 0, xon_offset = 0;
    int8_t dynamic_th = 0;
    int32_t mode = -1;                          // -1: the pool's
};
// Mirror sessions and SAMPLEPACKET (sai_mirror.cpp).
struct MirrorObj {
    int32_t type = SAI_MIRROR_SESSION_TYPE_LOCAL;
    sai_object_id_t monitor = 0, policer = 0;
    MirrorCfg cfg;
    uint8_t tc = 0;
    int32_t congestion = SAI_MIRROR_SESSION_CONGESTION_MODE_INDEPENDENT;
    int id = -1;
};
// SWITCH LAG_HASH / ECMP_HASH (sai_hash.cpp).
struct HashObj { bool lag = false; std::vector<int32_t> fields; };
struct SampleObj {
    uint32_t rate = 0;
    int32_t type = SAI_SAMPLEPACKET_TYPE_SLOW_PATH, mode = SAI_SAMPLEPACKET_MODE_EXCLUSIVE;
    int id = -1;
};

// L3
// Virtual router = SDK VRF (vrf 0: the default VR, created with the switch).
struct VrObj {
    int vrf = 0;
    bool v4 = true, v6 = true;
    uint8_t mac[6] = {0};         // kept: RIFs carry their own SRC_MAC (orchagent sets it)
    StubMap stub;
};
struct RifObj {
    int32_t type = SAI_ROUTER_INTERFACE_TYPE_PORT;
    int vrf = 0;                  // the VR's SDK VRF
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
struct NeighborObj { uint8_t mac[6] = {0}; bool no_host = false; int egress = -1; int hw = -1; bool host = false; int vrf = 0; };
struct NextHopObj { int32_t type = SAI_NEXT_HOP_TYPE_IP; IpAddr ip; sai_object_id_t rif = 0; int egress = -1; };
struct RouteKey {
    sai_object_id_t vr;
    IpPrefix p;
    bool operator<(const RouteKey& o) const { return vr != o.vr ? vr < o.vr : p < o.p; }
};
struct RouteObj {
    int32_t action = SAI_PACKET_ACTION_FORWARD;
    sai_object_id_t nh = 0;
    int egress = -1;              // programmed target: egress object, or ECMP group if ecmp
    bool ecmp = false;
    StubMap stub;
};
// Next-hop group = SDK ECMP group over the members' egress objects, only
// while at least one member's next hop is resolved (has its neighbor's
// egress object); otherwise no SDK group and routes on it drop.
struct NhgObj {
    int32_t type = SAI_NEXT_HOP_GROUP_TYPE_ECMP;
    std::vector<sai_object_id_t> members;   // NEXT_HOP_GROUP_MEMBER oids, creation order
    int ecmp = -1;                          // SDK ECMP id, -1 = none
    std::vector<int> programmed;            // egress ids in the SDK group
};
struct NhgMemberObj {
    sai_object_id_t group = 0, nh = 0;
    uint32_t weight = 1;                    // programmed as repetitions of the egress object
    StubMap stub;
};

// Policers (SDK meters) and the ingress field processor (sai_acl.cpp).
struct PolicerObj {
    int32_t meter = SAI_METER_TYPE_PACKETS, mode = SAI_POLICER_MODE_SR_TCM;
    int32_t color = SAI_POLICER_COLOR_SOURCE_AWARE;
    uint64_t cir = 0, cbs = 0, pir = 0, pbs = 0;
    int32_t green = SAI_PACKET_ACTION_FORWARD, yellow = SAI_PACKET_ACTION_FORWARD, red = SAI_PACKET_ACTION_FORWARD;
    int pid = -1;                 // backend policer
};
struct AclTableObj {
    int32_t stage = SAI_ACL_STAGE_INGRESS;
    std::vector<int32_t> bind_types, action_types;
    std::set<sai_attr_id_t> fields;     // SAI_ACL_TABLE_ATTR_FIELD_* set to true
    std::vector<int32_t> range_types;   // FIELD_ACL_RANGE_TYPE: kept (AclOrch always asks for L4 port ranges)
    uint32_t size = 0;
    int gid = -1;                       // SDK field group
    std::vector<sai_object_id_t> entries;
};
struct AclFieldVal {
    sai_acl_field_data_t d;
    std::vector<sai_object_id_t> objs;  // object lists (IN_PORTS) copied
};
struct AclEntryObj {
    sai_object_id_t table = 0;
    uint32_t priority = 0;
    bool admin = true;
    std::map<sai_attr_id_t, AclFieldVal> fields;            // enabled matches
    std::map<sai_attr_id_t, sai_acl_action_data_t> actions; // enabled actions
    std::map<sai_attr_id_t, std::vector<sai_object_id_t>> action_objs;   // object-list actions (MIRROR_*) copied
    int eid = -1;                       // installed SDK entry, -1 = none
    std::string key;                    // what eid was installed with
    bool hw_enabled = true;             // FpCap::EntryEnable: the installed entry's state
};
struct AclRangeObj { int32_t type = 0; uint32_t min = 0, max = 0; };   // ACL_RANGE (FpCap::Ranges)
struct AclCounterObj {
    sai_object_id_t table = 0, entry = 0;
    bool packets = false, bytes = false;
    uint64_t base_packets = 0, base_bytes = 0;  // carried over SDK entry re-creation
};
struct AclGroupObj {
    int32_t stage = SAI_ACL_STAGE_INGRESS, type = SAI_ACL_TABLE_GROUP_TYPE_PARALLEL;
    std::vector<int32_t> bind_types;
    std::vector<sai_object_id_t> members;
};
struct AclGroupMemberObj { sai_object_id_t group = 0, table = 0; uint32_t priority = 0; };
struct CoppEntry { int eid = -1; sai_object_id_t policer = 0; };

// --------------------------------------------------------------- events
struct Event {
    enum { Link, Fdb, Stop } kind;
    int hw = -1;
    bool up = false;
    FdbEvent fdb;
};

// Board data for front-panel ports whose line side is another chip (a
// retimer or MACsec PHY between the ASIC and the cage): profile keys
// SAI_OPENBCM_PORT_INTERFACE / _FEC / _AUTONEG / _IFG, applied to every
// front-panel port at switch creation and again after a speed change. With a
// fixed FEC or autoneg the SAI attribute is kept and returned, not programmed.
struct PortQuirks {
    std::string interface;          // "" = SDK default
    int fec = -1;                   // -1: SAI FEC_MODE programs the ASIC; else (int)Fec, fixed
    int autoneg = -1;               // -1: SAI AUTO_NEG_MODE programs the ASIC; 0/1 fixed
    uint32_t ifg = 0;               // average inter-frame gap in bit times; 0 = SDK default
    bool failed = false;            // a quirk could not be applied (create_switch fails)
};
sai_status_t port_quirks_parse(const BackendConfig& cfg, PortQuirks& q);   // sai_port.cpp
void port_quirks_apply(PortObj& p);

struct Ctx {
    std::recursive_mutex mu;
    PortQuirks port_quirks;
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
    std::map<sai_object_id_t, QosMapObj> qos_maps;
    std::map<sai_object_id_t, SchedulerObj> schedulers;
    std::map<sai_object_id_t, SchedGroupObj> sched_groups;
    std::map<sai_object_id_t, WredObj> wreds;
    std::map<sai_object_id_t, BufferPoolObj> buffer_pools;
    std::map<sai_object_id_t, BufferProfileObj> buffer_profiles;
    sai_object_id_t switch_qos_map[(int)QosMapKind::Count] = {};   // SWITCH QOS_*_MAP: ports without their own
    std::map<sai_object_id_t, MirrorObj> mirrors;
    std::map<sai_object_id_t, SampleObj> samplepackets;
    std::map<sai_object_id_t, HashObj> hashes;
    sai_object_id_t lag_hash = 0, ecmp_hash = 0;
    std::map<sai_object_id_t, RifObj> rifs;
    std::map<NeighborKey, NeighborObj> neighbors;
    std::map<sai_object_id_t, NextHopObj> next_hops;
    std::map<RouteKey, RouteObj> routes;
    std::map<sai_object_id_t, VrObj> vrs;
    std::map<sai_object_id_t, NhgObj> nhgs;
    std::map<sai_object_id_t, NhgMemberObj> nhg_members;
    std::set<uint16_t> internal_vlans;                  // allocated for port RIFs
    std::map<sai_object_id_t, PolicerObj> policers;
    std::map<sai_object_id_t, AclTableObj> acl_tables;
    std::map<sai_object_id_t, AclEntryObj> acl_entries;
    std::map<sai_object_id_t, AclCounterObj> acl_counters;
    std::map<sai_object_id_t, AclGroupObj> acl_groups;
    std::map<sai_object_id_t, AclGroupMemberObj> acl_group_members;
    std::map<sai_object_id_t, sai_object_id_t> acl_bind;  // PORT/LAG -> ingress ACL table group (or table)
    std::map<sai_object_id_t, sai_object_id_t> acl_bind_egress;  // PORT/LAG -> egress ACL (FpCap::Egress)
    std::map<sai_object_id_t, AclRangeObj> acl_ranges;
    int acl_next_prio = 1;                              // SDK group priority per table, creation order
    int copp_gid = -1;                                  // SDK field group of the CoPP policing rules
    bool copp_dst_cpu = false;                          // that group can match "routed to the CPU"
    std::map<std::string, CoppEntry> copp;              // installed CoPP rules by rule key
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
extern const sai_next_hop_group_api_t nhg_api;
extern const sai_lag_api_t lag_api;
extern const sai_policer_api_t policer_api;
extern const sai_acl_api_t acl_api;
extern const sai_qos_map_api_t qos_map_api;
extern const sai_scheduler_api_t scheduler_api;
extern const sai_scheduler_group_api_t scheduler_group_api;
extern const sai_wred_api_t wred_api;
extern const sai_mirror_api_t mirror_api;
extern const sai_samplepacket_api_t samplepacket_api;
extern const sai_hash_api_t hash_api;

// Cross-object helpers implemented in the object files.
void switch_build_objects();                         // after backend init
void switch_clear_objects();
void events_start();
void events_stop();
void push_event(const Event& e);
sai_status_t port_stats_get(int hw, uint32_t n, const sai_stat_id_t* ids, uint64_t* out);
bool port_stat_supported(sai_stat_id_t id);
bool rif_stat_supported(sai_stat_id_t id);           // sai_l3.cpp: SAI_ROUTER_INTERFACE_STAT_*
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
// SAI_OPENBCM_DIAG_SOCKET: diag-shell commands over a root-only unix socket.
void diag_start(const BackendConfig& cfg, Backend* be);
void diag_stop();                                    // without the SAI lock
sai_status_t vlan_member_create_internal(sai_object_id_t vlan, sai_object_id_t bport, int32_t tagging,
                                         bool program, sai_object_id_t* out);
sai_status_t l3_switch_mac_changed(const uint8_t old_mac[6]);
TrapKind trap_kind(int32_t sai_trap_type);
// ACL binding (PORT / LAG INGRESS_ACL) and CoPP policing, sai_acl.cpp.
sai_status_t acl_bind_set(sai_object_id_t obj, bool ingress, sai_object_id_t acl, uint32_t attr_index);
sai_object_id_t acl_bind_get(sai_object_id_t obj, bool ingress);
void acl_reprogram();                                // after binding / LAG membership changes
void copp_refresh();                                 // after trap / trap group / policer changes
TrapSpec trap_spec(const TrapObj& t);               // the trap with its group and policer (Backend::trap_program)
bool policer_valid(sai_object_id_t oid);
void acl_clear_objects();                            // switch removal
bool acl_table_field_supported(sai_attr_id_t table_field);
std::vector<int32_t> acl_ingress_actions();          // SAI_ACL_ACTION_TYPE_* implemented
std::vector<int32_t> acl_egress_actions();           // the same for egress tables (empty: no egress ACL)
// QoS (sai_qos.cpp): per-port attributes (handled = the attribute is a QoS
// one), the scheduler groups built with the switch, teardown.
sai_status_t port_qos_set(PortObj& p, const sai_attribute_t& a, uint32_t i, bool& handled);
sai_status_t port_qos_get(PortObj& p, sai_attribute_t& a, uint32_t i, bool& handled);
sai_status_t switch_qos_set(const sai_attribute_t& a, bool& handled);
sai_status_t switch_qos_get(sai_attribute_t& a, bool& handled);
void qos_build_port(PortObj& p);                     // switch_build_objects, after the queues
void qos_clear_objects();
// sai_query_stats_capability for QUEUE / INGRESS_PRIORITY_GROUP: what the backend answers.
sai_status_t qos_stats_capability(sai_object_type_t t, sai_stat_capability_list_t* out);
bool port_pfc_stat(sai_stat_id_t id, int& prio, bool& tx);   // SAI_PORT_STAT_PFC_<n>_{RX,TX}_PKTS
// Mirror / sampling (sai_mirror.cpp).
sai_status_t port_mirror_set(PortObj& p, const sai_attribute_t& a, uint32_t i, bool& handled);
sai_status_t port_mirror_get(PortObj& p, sai_attribute_t& a, uint32_t i, bool& handled);
int mirror_backend_id(sai_object_id_t session);      // -1: none / not programmed
bool mirror_valid(sai_object_id_t session);
void mirror_clear_objects();
// Hash objects, storm control, VLAN flood control (sai_hash.cpp).
void hash_build_objects();                           // switch_build_objects
sai_status_t switch_hash_set(const sai_attribute_t& a, bool& handled);   // *_DEFAULT_HASH_SEED
sai_status_t switch_hash_get(sai_attribute_t& a, bool& handled);         // LAG_HASH, ECMP_HASH
sai_status_t port_storm_attr_set(PortObj& p, const sai_attribute_t& a, uint32_t i, bool& handled);
sai_status_t port_storm_attr_get(PortObj& p, sai_attribute_t& a, bool& handled);
bool vlan_flood_attr(sai_attr_id_t id);              // the three *_FLOOD_CONTROL_TYPE ids
sai_status_t vlan_flood_apply(uint16_t vid, const sai_attribute_t& a, uint32_t i, bool& keep);   // keep: store as a stub

} // namespace ob
