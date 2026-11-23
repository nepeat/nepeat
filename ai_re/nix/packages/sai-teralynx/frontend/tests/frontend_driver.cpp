// SPDX-License-Identifier: Apache-2.0
// Frontend driver: the C++ half of the differential oracle between the C++ sai-openbcm frontend
// and the Rust libsai. Reads an op script (one SAI operation per line), runs it against the C++
// libsai on its fake backend wrapped in a RecordingBackend (src/recording_backend.h), and prints
// per operation: the SAI status, the attributes / counters read, the backend calls the operation
// caused (call-log lines) and the notifications it raised. The Rust driver implements the same
// script grammar and output; the two outputs are compared as text.
//
//   frontend_driver [--queries] [script]        script: a file, or stdin when absent / "-"
//     --queries   the call log in full mode (also the constant-answer queries, see
//                 recording_backend.cpp)
//
// It reaches the library through the SAI C ABI, tests/test_port.h (injections, the event
// counter, backend ids for inject targets) and, as the one C++-only part, the factory below:
// SAI_OPENBCM_BACKEND=openbcm selects a RecordingBackend over the fake backend.
//
// ============================================================================ script grammar
// Lines are split into words at blanks; a word in double quotes may contain blanks (no
// escapes). Blank lines and lines starting with '#' are skipped (not echoed).
//
//   profile <KEY>=<VALUE>          set a SAI profile key; read at the next "switch". Defaults:
//                                  SAI_OPENBCM_BACKEND=openbcm (the recorder; the Rust driver:
//                                  its virtual backend), SAI_OPENBCM_FAKE_PORTS=8,
//                                  SAI_OPENBCM_FAKE_LANES=4, SAI_OPENBCM_FAKE_SPEED=100000
//   switch [<ATTR>=<value> ...]    sai_api_initialize (first time) and create_switch with
//                                  INIT_SWITCH=true, the port-state and FDB notification
//                                  callbacks and the given SWITCH attributes. On success binds the
//                                  names: switch, cpu, port0..portN-1 (PORT_LIST order), bport0..
//                                  (the default bridge port of portI), vlan1, vr0, bridge, stp0,
//                                  trapgroup0
//   remove_switch                  remove_switch; forgets every name
//   create <type> <target> [<ATTR>=<value> ...]
//   remove <type> <target>
//   set <type> <target> <ATTR>=<value>
//   get <type> <target> <ATTR> [<ATTR> ...]        one get call with all the attributes
//   stats <type> <target> <STAT> [<STAT> ...]      get_<type>_stats
//   clear <type> <target> <STAT> [<STAT> ...]      clear_<type>_stats
//   flush [<ATTR>=<value> ...]                     flush_fdb_entries (FDB_FLUSH attributes)
//   inject link <port> up|down                     the backend's link changes (tp::inject_link)
//   inject fdb learned|aged <mac> <vid> <port|lag> hardware learning / ageing (tp::inject_fdb)
//   inject stat <port> <PORT STAT> <value>         a port counter's value (tp::set_port_stat)
//   mdio <phy> <dev> <reg> <value>                 the backend's MDIO register file (hex numbers)
//   feature qos|mirror|l2 on|off                   the backend's optional groups (tp::set_feature)
//   linkscan                                       one link-scan pass (tp::linkscan)
//
//   <type>    an object type without SAI_OBJECT_TYPE_, lower case: port, vlan, vlan_member,
//             bridge, bridge_port, stp, lag, lag_member, hostif, hostif_trap_group, hostif_trap,
//             hostif_table_entry, virtual_router, router_interface, next_hop, next_hop_group,
//             next_hop_group_member, policer, acl_table, acl_entry, acl_counter, acl_range,
//             acl_table_group, acl_table_group_member, qos_map, scheduler, scheduler_group, wred,
//             buffer_pool, buffer_profile, ingress_priority_group, queue, mirror_session,
//             samplepacket, hash, macsec, macsec_port, macsec_flow, macsec_sc, macsec_sa, switch
//             (set/get only), and the entries fdb_entry, neighbor_entry, route_entry
//   <target>  create: a new name ([a-z][a-z0-9_.#]*) for the object; otherwise a bound name.
//             Entries are keys instead: fdb_entry <mac>@<bv>, neighbor_entry <ip>@<rif>,
//             route_entry <prefix>@<vr>
//   <ATTR>    the attribute name without SAI_<TYPE>_ATTR_ (VLAN_ID for SAI_VLAN_ATTR_VLAN_ID)
//   <STAT>    the counter name without SAI_<TYPE>_STAT_ (IF_IN_OCTETS for a port)
//   <value>   by the attribute's SAI @type:
//             bool: true | false;  integers: decimal or 0x hex;  enum: the enumerator without
//             its type's prefix (TAGGED for SAI_VLAN_TAGGING_MODE_TAGGED), the full name, or a
//             number;  sai_mac_t: aa:bb:cc:dd:ee:ff;  sai_ip_address_t / sai_ip4_t /
//             sai_ip6_t: an IPv4 or IPv6 address;  sai_object_id_t: a bound name or null;
//             lists: [v,v,...] ([] empty) of the element type (names / enumerators / numbers);
//             char: a word or a quoted string;  sai_u32_range_t: <min>-<max>;
//             sai_qos_map_list_t: [<key>=<value>,...] where key and value are field:number pairs
//             joined by '/' (fields tc dscp dot1p prio pg queue color mpls_exp fc; unset 0),
//             e.g. [dscp:46=tc:5,tc:2/color:1=dscp:12];
//             sai_acl_field_data_t: off | <data> | <data>/<mask> (data by the field's type; an
//             object list field takes a list, a bool field true / false);
//             sai_acl_action_data_t: off | on | <parameter> (by the action's type);
//             SAI_MACSEC_SA_ATTR_SAK / AUTH_KEY / SALT: hex bytes without separators
//   <port>, <lag>, <bv>, <rif>, <vr>: bound names; <mac>, <ip>, <prefix> (a.b.c.d/len, v6/len)
//
// ==================================================================================== output
// Per script line:
//   > <the line's words joined by one blank>
//   = <STATUS>                      SAI status (call-log names: SUCCESS, INVALID_ATTR_VALUE_0,
//                                   ...), or "= PARSE_ERROR <reason>" (nothing was called)
//     <ATTR>=<value>                get: each attribute on SUCCESS (values as above; object ids
//                                   by name, unknown ones get the name <type>#<n>, n counting
//                                   per type in order of first sight, which scripts may use too;
//                                   lists show at most 256 elements), on BUFFER_OVERFLOW
//                                   "<ATTR> count=<n>" for each list attribute
//     <STAT>=<value>                stats on SUCCESS
//     be <call-log line>            every backend call the operation made, in order
//     notify port <port> UP|DOWN    port-state notifications the operation caused, then
//     notify fdb <EVENT> <mac>@<bv> bridge_port=<name>   FDB notifications, in delivery order
// After each operation the driver waits (at most 2 s) until the library has processed every
// event the backend raised during it, so the notify lines are complete and deterministic.
#include "recording_backend.h"
#include "test_port.h"

#include <arpa/inet.h>

#include <cctype>
#include <cerrno>
#include <chrono>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <deque>
#include <fstream>
#include <iostream>
#include <map>
#include <memory>
#include <mutex>
#include <sstream>
#include <string>
#include <thread>
#include <vector>

// --------------------------------------------------------------------- the recording factory
static std::shared_ptr<ob::CallLog> g_log = std::make_shared<ob::CallLog>();
static bool g_full = false;
namespace ob {
Backend* make_openbcm_backend()
{
    return new RecordingBackend(make_fake_backend(), g_log, g_full ? RecordingBackend::Mode::Full : RecordingBackend::Mode::Changes);
}
} // namespace ob

namespace {

// ------------------------------------------------------------------ name tables (SAI headers)
struct EnumName { const char* type; const char* name; long long value; };
const EnumName kEnums[] = {
#define E(T, N) {#T, #N, (long long)N},
#define A(N, T)
#include "driver_names.inc"
#undef A
#undef E
};
struct AttrType { const char* name; long long id; const char* type; };
const AttrType kAttrs[] = {
#define E(T, N)
#define A(N, T) {#N, (long long)N, T},
#include "driver_names.inc"
#undef A
#undef E
};

std::string upper(std::string s)
{
    for (auto& c : s) c = (char)std::toupper((unsigned char)c);
    return s;
}
bool is_marker(const std::string& n)
{
    auto ends = [&](const char* s) { size_t l = std::strlen(s); return n.size() >= l && n.compare(n.size() - l, l, s) == 0; };
    return ends("_START") || ends("_END") || ends("_MAX") || n.find("CUSTOM_RANGE") != std::string::npos ||
           n.find("RANGE_BASE") != std::string::npos;
}
bool enum_value(const std::string& type, const std::string& name, long long& v)
{
    for (auto& e : kEnums)
        if (type == e.type && name == e.name) { v = e.value; return true; }
    return false;
}
bool enum_type_exists(const std::string& type)
{
    for (auto& e : kEnums)
        if (type == e.type) return true;
    return false;
}
// "sai_vlan_tagging_mode_t" -> "SAI_VLAN_TAGGING_MODE_"
std::string enum_prefix(const std::string& type) { return upper(type.substr(0, type.size() - 2)) + "_"; }
std::string enum_name(const std::string& type, long long v)
{
    std::string pre = enum_prefix(type), marker;
    for (auto& e : kEnums) {
        if (type != e.type || e.value != v) continue;
        std::string n = e.name;
        if (is_marker(n)) { if (marker.empty()) marker = n; continue; }
        return n.compare(0, pre.size(), pre) == 0 ? n.substr(pre.size()) : n;
    }
    if (!marker.empty()) return marker.compare(0, pre.size(), pre) == 0 ? marker.substr(pre.size()) : marker;
    return std::to_string(v);
}
const AttrType* attr_named(const std::string& full)
{
    for (auto& a : kAttrs)
        if (full == a.name) return &a;
    return nullptr;
}

// ------------------------------------------------------------------------------ value kinds
enum class VK { Bool, U8, I8, U16, I16, U32, I32, U64, I64, Enum, Mac, Ip, Ip4, Ip6, Oid, OidList, U8List, I8List, U16List,
                I16List, U32List, I32List, VlanList, U32Range, Chars, QosMapList, AclField, AclAction, MacsecSak,
                MacsecAuthKey, MacsecSalt, Unsupported };
struct VT {
    VK k = VK::Unsupported;
    std::string enum_type;          // Enum / I32List of enumerators
    VK sub = VK::Unsupported;       // AclField / AclAction parameter
    std::string sub_enum;
};
VT vt_simple(const std::string& t)
{
    VT v;
    static const std::map<std::string, VK> m = {
        {"bool", VK::Bool}, {"sai_uint8_t", VK::U8}, {"sai_int8_t", VK::I8}, {"sai_uint16_t", VK::U16},
        {"sai_int16_t", VK::I16}, {"sai_uint32_t", VK::U32}, {"sai_int32_t", VK::I32}, {"sai_uint64_t", VK::U64},
        {"sai_int64_t", VK::I64}, {"sai_mac_t", VK::Mac}, {"sai_ip_address_t", VK::Ip}, {"sai_ip4_t", VK::Ip4},
        {"sai_ip6_t", VK::Ip6}, {"sai_object_id_t", VK::Oid}, {"sai_object_list_t", VK::OidList},
        {"sai_u8_list_t", VK::U8List}, {"sai_s8_list_t", VK::I8List}, {"sai_u16_list_t", VK::U16List},
        {"sai_s16_list_t", VK::I16List}, {"sai_u32_list_t", VK::U32List}, {"sai_s32_list_t", VK::I32List},
        {"sai_vlan_list_t", VK::VlanList}, {"sai_u32_range_t", VK::U32Range}, {"char", VK::Chars},
        {"sai_qos_map_list_t", VK::QosMapList}, {"sai_macsec_sak_t", VK::MacsecSak},
        {"sai_macsec_auth_key_t", VK::MacsecAuthKey}, {"sai_macsec_salt_t", VK::MacsecSalt},
    };
    auto it = m.find(t);
    if (it != m.end()) v.k = it->second;
    else if (enum_type_exists(t)) { v.k = VK::Enum; v.enum_type = t; }
    return v;
}
VT vt_of(const std::string& text)
{
    std::istringstream is(text);
    std::string a, b;
    is >> a >> b;
    if (a == "sai_acl_field_data_t" || a == "sai_acl_action_data_t") {
        VT v;
        v.k = a == "sai_acl_field_data_t" ? VK::AclField : VK::AclAction;
        VT s = vt_simple(b);
        v.sub = s.k;
        v.sub_enum = s.enum_type;
        return v;
    }
    VT v = vt_simple(a);
    if (v.k == VK::I32List && !b.empty() && enum_type_exists(b)) v.enum_type = b;
    return v;
}

// ----------------------------------------------------------------------------- object types
using CreateFn = sai_status_t (*)(void*, sai_object_id_t*, sai_object_id_t, uint32_t, const sai_attribute_t*);
using RemoveFn = sai_status_t (*)(void*, sai_object_id_t);
using SetFn = sai_status_t (*)(void*, sai_object_id_t, const sai_attribute_t*);
using GetFn = sai_status_t (*)(void*, sai_object_id_t, uint32_t, sai_attribute_t*);
using StatsFn = sai_status_t (*)(void*, sai_object_id_t, uint32_t, const sai_stat_id_t*, uint64_t*);
using ClearFn = sai_status_t (*)(void*, sai_object_id_t, uint32_t, const sai_stat_id_t*);
struct ObjType {
    const char* name;               // lower case, as scripts write it
    sai_object_type_t ot;
    sai_api_t api;
    CreateFn create;
    RemoveFn remove;
    SetFn set;
    GetFn get;
    StatsFn stats;
    ClearFn clear;
};
#define OBJ(T, API, APIT, N)                                                                                        \
    {#N, SAI_OBJECT_TYPE_##T, API,                                                                                  \
     [](void* p, sai_object_id_t* o, sai_object_id_t s, uint32_t n, const sai_attribute_t* a) { return ((APIT*)p)->create_##N(o, s, n, a); }, \
     [](void* p, sai_object_id_t o) { return ((APIT*)p)->remove_##N(o); },                                        \
     [](void* p, sai_object_id_t o, const sai_attribute_t* a) { return ((APIT*)p)->set_##N##_attribute(o, a); },  \
     [](void* p, sai_object_id_t o, uint32_t n, sai_attribute_t* a) { return ((APIT*)p)->get_##N##_attribute(o, n, a); }, \
     nullptr, nullptr}
#define OBJS(T, API, APIT, N)                                                                                       \
    {#N, SAI_OBJECT_TYPE_##T, API,                                                                                  \
     [](void* p, sai_object_id_t* o, sai_object_id_t s, uint32_t n, const sai_attribute_t* a) { return ((APIT*)p)->create_##N(o, s, n, a); }, \
     [](void* p, sai_object_id_t o) { return ((APIT*)p)->remove_##N(o); },                                        \
     [](void* p, sai_object_id_t o, const sai_attribute_t* a) { return ((APIT*)p)->set_##N##_attribute(o, a); },  \
     [](void* p, sai_object_id_t o, uint32_t n, sai_attribute_t* a) { return ((APIT*)p)->get_##N##_attribute(o, n, a); }, \
     [](void* p, sai_object_id_t o, uint32_t n, const sai_stat_id_t* i, uint64_t* v) { return ((APIT*)p)->get_##N##_stats(o, n, i, v); }, \
     [](void* p, sai_object_id_t o, uint32_t n, const sai_stat_id_t* i) { return ((APIT*)p)->clear_##N##_stats(o, n, i); }}
const ObjType kTypes[] = {
    OBJS(PORT, SAI_API_PORT, sai_port_api_t, port),
    OBJS(VLAN, SAI_API_VLAN, sai_vlan_api_t, vlan),
    OBJ(VLAN_MEMBER, SAI_API_VLAN, sai_vlan_api_t, vlan_member),
    OBJ(BRIDGE, SAI_API_BRIDGE, sai_bridge_api_t, bridge),
    OBJ(BRIDGE_PORT, SAI_API_BRIDGE, sai_bridge_api_t, bridge_port),
    OBJ(STP, SAI_API_STP, sai_stp_api_t, stp),
    OBJ(LAG, SAI_API_LAG, sai_lag_api_t, lag),
    OBJ(LAG_MEMBER, SAI_API_LAG, sai_lag_api_t, lag_member),
    OBJ(HOSTIF, SAI_API_HOSTIF, sai_hostif_api_t, hostif),
    OBJ(HOSTIF_TRAP_GROUP, SAI_API_HOSTIF, sai_hostif_api_t, hostif_trap_group),
    OBJ(HOSTIF_TRAP, SAI_API_HOSTIF, sai_hostif_api_t, hostif_trap),
    OBJ(HOSTIF_TABLE_ENTRY, SAI_API_HOSTIF, sai_hostif_api_t, hostif_table_entry),
    OBJ(VIRTUAL_ROUTER, SAI_API_VIRTUAL_ROUTER, sai_virtual_router_api_t, virtual_router),
    OBJS(ROUTER_INTERFACE, SAI_API_ROUTER_INTERFACE, sai_router_interface_api_t, router_interface),
    OBJ(NEXT_HOP, SAI_API_NEXT_HOP, sai_next_hop_api_t, next_hop),
    OBJ(NEXT_HOP_GROUP, SAI_API_NEXT_HOP_GROUP, sai_next_hop_group_api_t, next_hop_group),
    OBJ(NEXT_HOP_GROUP_MEMBER, SAI_API_NEXT_HOP_GROUP, sai_next_hop_group_api_t, next_hop_group_member),
    OBJS(POLICER, SAI_API_POLICER, sai_policer_api_t, policer),
    OBJ(ACL_TABLE, SAI_API_ACL, sai_acl_api_t, acl_table),
    OBJ(ACL_ENTRY, SAI_API_ACL, sai_acl_api_t, acl_entry),
    OBJ(ACL_COUNTER, SAI_API_ACL, sai_acl_api_t, acl_counter),
    OBJ(ACL_RANGE, SAI_API_ACL, sai_acl_api_t, acl_range),
    OBJ(ACL_TABLE_GROUP, SAI_API_ACL, sai_acl_api_t, acl_table_group),
    OBJ(ACL_TABLE_GROUP_MEMBER, SAI_API_ACL, sai_acl_api_t, acl_table_group_member),
    OBJ(QOS_MAP, SAI_API_QOS_MAP, sai_qos_map_api_t, qos_map),
    OBJ(SCHEDULER, SAI_API_SCHEDULER, sai_scheduler_api_t, scheduler),
    OBJ(SCHEDULER_GROUP, SAI_API_SCHEDULER_GROUP, sai_scheduler_group_api_t, scheduler_group),
    OBJ(WRED, SAI_API_WRED, sai_wred_api_t, wred),
    OBJS(BUFFER_POOL, SAI_API_BUFFER, sai_buffer_api_t, buffer_pool),
    OBJ(BUFFER_PROFILE, SAI_API_BUFFER, sai_buffer_api_t, buffer_profile),
    OBJS(INGRESS_PRIORITY_GROUP, SAI_API_BUFFER, sai_buffer_api_t, ingress_priority_group),
    OBJS(QUEUE, SAI_API_QUEUE, sai_queue_api_t, queue),
    OBJ(MIRROR_SESSION, SAI_API_MIRROR, sai_mirror_api_t, mirror_session),
    OBJ(SAMPLEPACKET, SAI_API_SAMPLEPACKET, sai_samplepacket_api_t, samplepacket),
    OBJ(HASH, SAI_API_HASH, sai_hash_api_t, hash),
    OBJ(MACSEC, SAI_API_MACSEC, sai_macsec_api_t, macsec),
    OBJ(MACSEC_PORT, SAI_API_MACSEC, sai_macsec_api_t, macsec_port),
    OBJS(MACSEC_FLOW, SAI_API_MACSEC, sai_macsec_api_t, macsec_flow),
    OBJ(MACSEC_SC, SAI_API_MACSEC, sai_macsec_api_t, macsec_sc),
    OBJS(MACSEC_SA, SAI_API_MACSEC, sai_macsec_api_t, macsec_sa),
    {"switch", SAI_OBJECT_TYPE_SWITCH, SAI_API_SWITCH, nullptr, nullptr,
     [](void* p, sai_object_id_t o, const sai_attribute_t* a) { return ((sai_switch_api_t*)p)->set_switch_attribute(o, a); },
     [](void* p, sai_object_id_t o, uint32_t n, sai_attribute_t* a) { return ((sai_switch_api_t*)p)->get_switch_attribute(o, n, a); },
     nullptr, nullptr},
    {"fdb_entry", SAI_OBJECT_TYPE_FDB_ENTRY, SAI_API_FDB, nullptr, nullptr, nullptr, nullptr, nullptr, nullptr},
    {"neighbor_entry", SAI_OBJECT_TYPE_NEIGHBOR_ENTRY, SAI_API_NEIGHBOR, nullptr, nullptr, nullptr, nullptr, nullptr, nullptr},
    {"route_entry", SAI_OBJECT_TYPE_ROUTE_ENTRY, SAI_API_ROUTE, nullptr, nullptr, nullptr, nullptr, nullptr, nullptr},
};
#undef OBJ
#undef OBJS
const ObjType* type_named(const std::string& n)
{
    for (auto& t : kTypes)
        if (n == t.name) return &t;
    return nullptr;
}
const ObjType* type_of(sai_object_type_t ot)
{
    for (auto& t : kTypes)
        if (t.ot == ot) return &t;
    return nullptr;
}
std::string type_upper(const ObjType& t) { return upper(t.name); }

void* api_of(sai_api_t api)
{
    static std::map<int, void*> cache;
    auto it = cache.find(api);
    if (it != cache.end()) return it->second;
    void* p = nullptr;
    if (sai_api_query(api, &p) != SAI_STATUS_SUCCESS) p = nullptr;
    cache[api] = p;
    return p;
}

// ---------------------------------------------------------------------------- driver state
struct ParseError {
    std::string what;
};
[[noreturn]] void fail(const std::string& w) { throw ParseError{w}; }

std::map<std::string, std::string> g_profile;
std::map<std::string, std::string>::iterator g_iter;
const char* prof_get(sai_switch_profile_id_t, const char* k)
{
    auto it = g_profile.find(k);
    return it == g_profile.end() ? nullptr : it->second.c_str();
}
int prof_next(sai_switch_profile_id_t, const char** k, const char** v)
{
    if (!k || !v) { g_iter = g_profile.begin(); return 0; }
    if (g_iter == g_profile.end()) return -1;
    *k = g_iter->first.c_str();
    *v = g_iter->second.c_str();
    ++g_iter;
    return 0;
}
sai_service_method_table_t g_smt = {prof_get, prof_next};
bool g_initialized = false;
sai_object_id_t g_sw = 0;

std::map<std::string, sai_object_id_t> g_names;
std::map<sai_object_id_t, std::string> g_oid_names;
std::map<int, int> g_auto;   // object type -> last auto number
void bind_name(const std::string& n, sai_object_id_t o)
{
    g_names[n] = o;
    if (!g_oid_names.count(o)) g_oid_names[o] = n;
}
void forget_names()
{
    g_names.clear();
    g_oid_names.clear();
    g_auto.clear();
}
std::string oid_name(sai_object_id_t o)
{
    if (o == SAI_NULL_OBJECT_ID) return "null";
    auto it = g_oid_names.find(o);
    if (it != g_oid_names.end()) return it->second;
    sai_object_type_t ot = sai_object_type_query(o);
    const ObjType* t = type_of(ot);
    std::string base = t ? t->name : "oid" + std::to_string((int)ot);
    std::string n = base + "#" + std::to_string(++g_auto[(int)ot]);
    bind_name(n, o);
    return n;
}
sai_object_id_t oid_named(const std::string& n)
{
    if (n == "null") return SAI_NULL_OBJECT_ID;
    auto it = g_names.find(n);
    if (it == g_names.end()) fail("unknown name '" + n + "'");
    return it->second;
}

// notifications
std::mutex g_nmu;
std::vector<std::string> g_notes;
void on_port(uint32_t n, const sai_port_oper_status_notification_t* d)
{
    std::lock_guard<std::mutex> g(g_nmu);
    for (uint32_t i = 0; i < n; i++)
        g_notes.push_back("port\t" + std::to_string(d[i].port_id) + "\t" + std::to_string(d[i].port_state));
}
const char* fdb_event_name(int e)
{
    switch (e) {
    case SAI_FDB_EVENT_LEARNED: return "LEARNED";
    case SAI_FDB_EVENT_AGED: return "AGED";
    case SAI_FDB_EVENT_MOVE: return "MOVE";
    case SAI_FDB_EVENT_FLUSHED: return "FLUSHED";
    default: return "?";
    }
}
std::string mac_str(const uint8_t* m)
{
    char b[24];
    std::snprintf(b, sizeof(b), "%02x:%02x:%02x:%02x:%02x:%02x", m[0], m[1], m[2], m[3], m[4], m[5]);
    return b;
}
void on_fdb(uint32_t n, const sai_fdb_event_notification_data_t* d)
{
    std::lock_guard<std::mutex> g(g_nmu);
    for (uint32_t i = 0; i < n; i++) {
        sai_object_id_t bp = 0;
        for (uint32_t a = 0; a < d[i].attr_count; a++)
            if (d[i].attr[a].id == SAI_FDB_ENTRY_ATTR_BRIDGE_PORT_ID) bp = d[i].attr[a].value.oid;
        g_notes.push_back("fdb\t" + std::to_string(d[i].event_type) + "\t" + mac_str(d[i].fdb_entry.mac_address) + "\t" +
                          std::to_string(d[i].fdb_entry.bv_id) + "\t" + std::to_string(bp));
    }
}

// --------------------------------------------------------------------------- word parsing
std::vector<std::string> words(const std::string& line)
{
    std::vector<std::string> w;
    size_t i = 0;
    while (i < line.size()) {
        while (i < line.size() && std::isspace((unsigned char)line[i])) i++;
        if (i >= line.size()) break;
        std::string t;
        while (i < line.size() && !std::isspace((unsigned char)line[i])) {
            if (line[i] == '"') {
                t += line[i++];
                while (i < line.size() && line[i] != '"') t += line[i++];
                if (i < line.size()) t += line[i++];
            } else {
                t += line[i++];
            }
        }
        w.push_back(t);
    }
    return w;
}
std::vector<std::string> split(const std::string& s, char sep)
{
    std::vector<std::string> v;
    std::string cur;
    int depth = 0;
    for (char c : s) {
        if (c == '[') depth++;
        if (c == ']') depth--;
        if (c == sep && depth == 0) { v.push_back(cur); cur.clear(); }
        else cur += c;
    }
    v.push_back(cur);
    return v;
}
long long num(const std::string& s)
{
    if (s.empty()) fail("empty number");
    char* end = nullptr;
    errno = 0;
    long long v = std::strtoll(s.c_str(), &end, 0);
    if (*end || errno) {
        unsigned long long u = std::strtoull(s.c_str(), &end, 0);
        if (*end) fail("bad number '" + s + "'");
        return (long long)u;
    }
    return v;
}
long long hexnum(const std::string& s)
{
    char* end = nullptr;
    long long v = std::strtoll(s.c_str(), &end, 16);
    if (s.empty() || *end) fail("bad hex number '" + s + "'");
    return v;
}
std::vector<std::string> list_items(const std::string& s)
{
    if (s.size() < 2 || s.front() != '[' || s.back() != ']') fail("expected a list [..], got '" + s + "'");
    std::string in = s.substr(1, s.size() - 2);
    if (in.empty()) return {};
    return split(in, ',');
}
void parse_mac(const std::string& s, uint8_t* m)
{
    unsigned b[6];
    char tail;
    if (std::sscanf(s.c_str(), "%x:%x:%x:%x:%x:%x%c", &b[0], &b[1], &b[2], &b[3], &b[4], &b[5], &tail) != 6) fail("bad MAC '" + s + "'");
    for (int i = 0; i < 6; i++) m[i] = (uint8_t)b[i];
}
void parse_ip(const std::string& s, sai_ip_address_t& ip)
{
    std::memset(&ip, 0, sizeof(ip));
    if (s.find(':') != std::string::npos) {
        ip.addr_family = SAI_IP_ADDR_FAMILY_IPV6;
        if (inet_pton(AF_INET6, s.c_str(), ip.addr.ip6) != 1) fail("bad IPv6 address '" + s + "'");
    } else {
        ip.addr_family = SAI_IP_ADDR_FAMILY_IPV4;
        if (inet_pton(AF_INET, s.c_str(), &ip.addr.ip4) != 1) fail("bad IPv4 address '" + s + "'");
    }
}
void parse_prefix(const std::string& s, sai_ip_prefix_t& p)
{
    size_t sl = s.rfind('/');
    if (sl == std::string::npos) fail("bad prefix '" + s + "'");
    sai_ip_address_t a;
    parse_ip(s.substr(0, sl), a);
    long long len = num(s.substr(sl + 1));
    std::memset(&p, 0, sizeof(p));
    p.addr_family = a.addr_family;
    if (a.addr_family == SAI_IP_ADDR_FAMILY_IPV4) {
        if (len < 0 || len > 32) fail("bad prefix length");
        p.addr.ip4 = a.addr.ip4;
        uint32_t m = len ? 0xffffffffu << (32 - len) : 0;
        p.mask.ip4 = htonl(m);
    } else {
        if (len < 0 || len > 128) fail("bad prefix length");
        std::memcpy(p.addr.ip6, a.addr.ip6, 16);
        for (int i = 0; i < 16; i++) {
            int bits = (int)len - 8 * i;
            p.mask.ip6[i] = bits >= 8 ? 0xff : bits <= 0 ? 0 : (uint8_t)(0xff << (8 - bits));
        }
    }
}
std::string ip6_str(const uint8_t* a)
{
    std::string s;
    char b[8];
    for (int i = 0; i < 8; i++) {
        std::snprintf(b, sizeof(b), "%s%x", i ? ":" : "", (unsigned)(a[2 * i] << 8 | a[2 * i + 1]));
        s += b;
    }
    return s;
}
std::string ip4_str(uint32_t net)
{
    const uint8_t* b = (const uint8_t*)&net;
    return std::to_string(b[0]) + "." + std::to_string(b[1]) + "." + std::to_string(b[2]) + "." + std::to_string(b[3]);
}
std::string ip_str(const sai_ip_address_t& ip)
{
    return ip.addr_family == SAI_IP_ADDR_FAMILY_IPV6 ? ip6_str(ip.addr.ip6) : ip4_str(ip.addr.ip4);
}
std::string unquote(const std::string& s)
{
    if (s.size() >= 2 && s.front() == '"' && s.back() == '"') return s.substr(1, s.size() - 2);
    return s;
}
long long enum_in(const std::string& type, const std::string& s)
{
    long long v;
    if (!s.empty() && (std::isdigit((unsigned char)s[0]) || s[0] == '-')) return num(s);
    if (enum_value(type, enum_prefix(type) + s, v)) return v;
    if (enum_value(type, s, v)) return v;
    fail("'" + s + "' is not a " + type);
}
void hex_bytes(const std::string& s, uint8_t* out, size_t n)
{
    if (s.size() != 2 * n) fail("expected " + std::to_string(n) + " hex bytes");
    for (size_t i = 0; i < n; i++) out[i] = (uint8_t)hexnum(s.substr(2 * i, 2));
}

// Lists live until the call returns.
struct Arena {
    std::deque<std::vector<uint64_t>> blocks;
    template <typename T> T* alloc(size_t n)
    {
        blocks.emplace_back((n * sizeof(T) + 7) / 8 + 1, 0);
        return (T*)blocks.back().data();
    }
};

// ---- values in
void qos_params(const std::string& s, sai_qos_map_params_t& p)
{
    std::memset(&p, 0, sizeof(p));
    for (auto& f : split(s, '/')) {
        size_t c = f.find(':');
        if (c == std::string::npos) fail("bad QoS map field '" + f + "'");
        std::string k = f.substr(0, c);
        long long v = num(f.substr(c + 1));
        if (k == "tc") p.tc = (sai_cos_t)v;
        else if (k == "dscp") p.dscp = (uint8_t)v;
        else if (k == "dot1p") p.dot1p = (uint8_t)v;
        else if (k == "prio") p.prio = (uint8_t)v;
        else if (k == "pg") p.pg = (uint8_t)v;
        else if (k == "queue") p.queue_index = (sai_queue_index_t)v;
        else if (k == "color") p.color = (sai_packet_color_t)v;
        else if (k == "mpls_exp") p.mpls_exp = (uint8_t)v;
        else if (k == "fc") p.fc = (uint8_t)v;
        else fail("unknown QoS map field '" + k + "'");
    }
}
template <typename T> void list_in(const std::string& s, T*& list, uint32_t& count, Arena& ar, VK el, const std::string& et)
{
    auto items = list_items(s);
    list = ar.alloc<T>(items.size());
    count = (uint32_t)items.size();
    for (size_t i = 0; i < items.size(); i++) {
        if (el == VK::Oid) list[i] = (T)oid_named(items[i]);
        else if (!et.empty()) list[i] = (T)enum_in(et, items[i]);
        else list[i] = (T)num(items[i]);
    }
}
void field_data_in(VK k, const std::string& et, const std::string& s, sai_acl_field_data_t& f, Arena& ar)
{
    if (s == "off") { f.enable = false; return; }
    f.enable = true;
    std::string d = s, m;
    if (k != VK::OidList) {
        size_t sl = s.find('/');
        if (sl != std::string::npos) { d = s.substr(0, sl); m = s.substr(sl + 1); }
    }
    switch (k) {
    case VK::Bool: f.data.booldata = d == "true"; break;
    case VK::U8: f.data.u8 = (uint8_t)num(d); if (!m.empty()) f.mask.u8 = (uint8_t)num(m); break;
    case VK::I8: f.data.s8 = (int8_t)num(d); if (!m.empty()) f.mask.s8 = (int8_t)num(m); break;
    case VK::U16: f.data.u16 = (uint16_t)num(d); if (!m.empty()) f.mask.u16 = (uint16_t)num(m); break;
    case VK::I16: f.data.s16 = (int16_t)num(d); if (!m.empty()) f.mask.s16 = (int16_t)num(m); break;
    case VK::U32: f.data.u32 = (uint32_t)num(d); if (!m.empty()) f.mask.u32 = (uint32_t)num(m); break;
    case VK::I32: f.data.s32 = (int32_t)num(d); if (!m.empty()) f.mask.s32 = (int32_t)num(m); break;
    case VK::Enum: f.data.s32 = (int32_t)enum_in(et, d); if (!m.empty()) f.mask.s32 = (int32_t)num(m); break;
    case VK::U64: f.data.u64 = (uint64_t)num(d); if (!m.empty()) f.mask.u64 = (uint64_t)num(m); break;
    case VK::Mac: parse_mac(d, f.data.mac); if (!m.empty()) parse_mac(m, f.mask.mac); break;
    case VK::Ip4: {
        sai_ip_address_t a;
        parse_ip(d, a);
        f.data.ip4 = a.addr.ip4;
        if (!m.empty()) { parse_ip(m, a); f.mask.ip4 = a.addr.ip4; }
        break;
    }
    case VK::Ip6: {
        sai_ip_address_t a;
        parse_ip(d, a);
        std::memcpy(f.data.ip6, a.addr.ip6, 16);
        if (!m.empty()) { parse_ip(m, a); std::memcpy(f.mask.ip6, a.addr.ip6, 16); }
        break;
    }
    case VK::Oid: f.data.oid = oid_named(d); break;
    case VK::OidList: list_in(d, f.data.objlist.list, f.data.objlist.count, ar, VK::Oid, ""); break;
    default: fail("unsupported ACL field type");
    }
}
void action_in(VK k, const std::string& et, const std::string& s, sai_acl_action_data_t& a, Arena& ar)
{
    if (s == "off") { a.enable = false; return; }
    a.enable = true;
    if (s == "on") return;
    switch (k) {
    case VK::Bool: a.parameter.booldata = s == "true"; break;
    case VK::U8: a.parameter.u8 = (uint8_t)num(s); break;
    case VK::I8: a.parameter.s8 = (int8_t)num(s); break;
    case VK::U16: a.parameter.u16 = (uint16_t)num(s); break;
    case VK::I16: a.parameter.s16 = (int16_t)num(s); break;
    case VK::U32: a.parameter.u32 = (uint32_t)num(s); break;
    case VK::I32: a.parameter.s32 = (int32_t)num(s); break;
    case VK::Enum: a.parameter.s32 = (int32_t)enum_in(et, s); break;
    case VK::Mac: parse_mac(s, a.parameter.mac); break;
    case VK::Ip4: { sai_ip_address_t x; parse_ip(s, x); a.parameter.ip4 = x.addr.ip4; break; }
    case VK::Ip: parse_ip(s, a.parameter.ipaddr); break;
    case VK::Oid: a.parameter.oid = oid_named(s); break;
    case VK::OidList: list_in(s, a.parameter.objlist.list, a.parameter.objlist.count, ar, VK::Oid, ""); break;
    default: fail("unsupported ACL action type");
    }
}
void value_in(const VT& t, const std::string& s, sai_attribute_value_t& v, Arena& ar)
{
    switch (t.k) {
    case VK::Bool:
        if (s != "true" && s != "false") fail("expected true / false");
        v.booldata = s == "true";
        break;
    case VK::U8: v.u8 = (uint8_t)num(s); break;
    case VK::I8: v.s8 = (int8_t)num(s); break;
    case VK::U16: v.u16 = (uint16_t)num(s); break;
    case VK::I16: v.s16 = (int16_t)num(s); break;
    case VK::U32: v.u32 = (uint32_t)num(s); break;
    case VK::I32: v.s32 = (int32_t)num(s); break;
    case VK::U64: v.u64 = (uint64_t)num(s); break;
    case VK::I64: v.s64 = (int64_t)num(s); break;
    case VK::Enum: v.s32 = (int32_t)enum_in(t.enum_type, s); break;
    case VK::Mac: parse_mac(s, v.mac); break;
    case VK::Ip: parse_ip(s, v.ipaddr); break;
    case VK::Ip4: { sai_ip_address_t a; parse_ip(s, a); v.ip4 = a.addr.ip4; break; }
    case VK::Ip6: { sai_ip_address_t a; parse_ip(s, a); std::memcpy(v.ip6, a.addr.ip6, 16); break; }
    case VK::Oid: v.oid = oid_named(s); break;
    case VK::OidList: list_in(s, v.objlist.list, v.objlist.count, ar, VK::Oid, ""); break;
    case VK::U8List: list_in(s, v.u8list.list, v.u8list.count, ar, VK::U8, ""); break;
    case VK::I8List: list_in(s, v.s8list.list, v.s8list.count, ar, VK::I8, ""); break;
    case VK::U16List: list_in(s, v.u16list.list, v.u16list.count, ar, VK::U16, ""); break;
    case VK::I16List: list_in(s, v.s16list.list, v.s16list.count, ar, VK::I16, ""); break;
    case VK::U32List: list_in(s, v.u32list.list, v.u32list.count, ar, VK::U32, ""); break;
    case VK::I32List: list_in(s, v.s32list.list, v.s32list.count, ar, VK::I32, t.enum_type); break;
    case VK::VlanList: list_in(s, v.vlanlist.list, v.vlanlist.count, ar, VK::U16, ""); break;
    case VK::U32Range: {
        size_t d = s.find('-');
        if (d == std::string::npos) fail("expected <min>-<max>");
        v.u32range.min = (uint32_t)num(s.substr(0, d));
        v.u32range.max = (uint32_t)num(s.substr(d + 1));
        break;
    }
    case VK::Chars: {
        std::string x = unquote(s);
        if (x.size() >= sizeof(v.chardata)) fail("text too long");
        std::memset(v.chardata, 0, sizeof(v.chardata));
        std::memcpy(v.chardata, x.data(), x.size());
        break;
    }
    case VK::QosMapList: {
        auto items = list_items(s);
        v.qosmap.list = ar.alloc<sai_qos_map_t>(items.size());
        v.qosmap.count = (uint32_t)items.size();
        for (size_t i = 0; i < items.size(); i++) {
            size_t eq = items[i].find('=');
            if (eq == std::string::npos) fail("QoS map entry without '='");
            qos_params(items[i].substr(0, eq), v.qosmap.list[i].key);
            qos_params(items[i].substr(eq + 1), v.qosmap.list[i].value);
        }
        break;
    }
    case VK::AclField: field_data_in(t.sub, t.sub_enum, s, v.aclfield, ar); break;
    case VK::AclAction: action_in(t.sub, t.sub_enum, s, v.aclaction, ar); break;
    case VK::MacsecSak: hex_bytes(s, v.macsecsak, sizeof(v.macsecsak)); break;
    case VK::MacsecAuthKey: hex_bytes(s, v.macsecauthkey, sizeof(v.macsecauthkey)); break;
    case VK::MacsecSalt: hex_bytes(s, v.macsecsalt, sizeof(v.macsecsalt)); break;
    default: fail("unsupported attribute type");
    }
}

// ---- values out
template <typename T> std::string list_out(const T* l, uint32_t n, VK el, const std::string& et)
{
    std::string s = "[";
    for (uint32_t i = 0; i < n && i < 256; i++) {
        if (i) s += ",";
        if (el == VK::Oid) s += oid_name((sai_object_id_t)l[i]);
        else if (!et.empty()) s += enum_name(et, (long long)l[i]);
        else s += std::to_string((long long)l[i]);
    }
    return s + "]";
}
std::string qos_params_out(const sai_qos_map_params_t& p)
{
    return std::to_string(p.tc) + "." + std::to_string(p.dscp) + "." + std::to_string(p.dot1p) + "." + std::to_string(p.prio) +
           "." + std::to_string(p.pg) + "." + std::to_string(p.queue_index) + "." + std::to_string(p.color) + "." +
           std::to_string(p.mpls_exp) + "." + std::to_string(p.fc);
}
std::string field_out(VK k, const std::string& et, const sai_acl_field_data_t& f)
{
    if (!f.enable) return "off";
    switch (k) {
    case VK::Bool: return f.data.booldata ? "true" : "false";
    case VK::U8: return std::to_string(f.data.u8) + "/" + std::to_string(f.mask.u8);
    case VK::I8: return std::to_string(f.data.s8) + "/" + std::to_string(f.mask.s8);
    case VK::U16: return std::to_string(f.data.u16) + "/" + std::to_string(f.mask.u16);
    case VK::I16: return std::to_string(f.data.s16) + "/" + std::to_string(f.mask.s16);
    case VK::U32: return std::to_string(f.data.u32) + "/" + std::to_string(f.mask.u32);
    case VK::I32: return std::to_string(f.data.s32) + "/" + std::to_string(f.mask.s32);
    case VK::Enum: return enum_name(et, f.data.s32);
    case VK::U64: return std::to_string(f.data.u64) + "/" + std::to_string(f.mask.u64);
    case VK::Mac: return mac_str(f.data.mac) + "/" + mac_str(f.mask.mac);
    case VK::Ip4: return ip4_str(f.data.ip4) + "/" + ip4_str(f.mask.ip4);
    case VK::Ip6: return ip6_str(f.data.ip6) + "/" + ip6_str(f.mask.ip6);
    case VK::Oid: return oid_name(f.data.oid);
    case VK::OidList: return list_out(f.data.objlist.list, f.data.objlist.count, VK::Oid, "");
    default: return "<unprintable>";
    }
}
std::string action_out(VK k, const std::string& et, const sai_acl_action_data_t& a)
{
    if (!a.enable) return "off";
    switch (k) {
    case VK::Bool: return a.parameter.booldata ? "true" : "false";
    case VK::U8: return std::to_string(a.parameter.u8);
    case VK::I8: return std::to_string(a.parameter.s8);
    case VK::U16: return std::to_string(a.parameter.u16);
    case VK::I16: return std::to_string(a.parameter.s16);
    case VK::U32: return std::to_string(a.parameter.u32);
    case VK::I32: return std::to_string(a.parameter.s32);
    case VK::Enum: return enum_name(et, a.parameter.s32);
    case VK::Mac: return mac_str(a.parameter.mac);
    case VK::Ip4: return ip4_str(a.parameter.ip4);
    case VK::Ip: return ip_str(a.parameter.ipaddr);
    case VK::Oid: return oid_name(a.parameter.oid);
    case VK::OidList: return list_out(a.parameter.objlist.list, a.parameter.objlist.count, VK::Oid, "");
    case VK::Unsupported: return "on";
    default: return "<unprintable>";
    }
}
std::string hex_out(const uint8_t* b, size_t n)
{
    std::string s;
    char x[4];
    for (size_t i = 0; i < n; i++) { std::snprintf(x, sizeof(x), "%02x", b[i]); s += x; }
    return s;
}
std::string value_out(const VT& t, const sai_attribute_value_t& v)
{
    switch (t.k) {
    case VK::Bool: return v.booldata ? "true" : "false";
    case VK::U8: return std::to_string(v.u8);
    case VK::I8: return std::to_string(v.s8);
    case VK::U16: return std::to_string(v.u16);
    case VK::I16: return std::to_string(v.s16);
    case VK::U32: return std::to_string(v.u32);
    case VK::I32: return std::to_string(v.s32);
    case VK::U64: return std::to_string(v.u64);
    case VK::I64: return std::to_string(v.s64);
    case VK::Enum: return enum_name(t.enum_type, v.s32);
    case VK::Mac: return mac_str(v.mac);
    case VK::Ip: return ip_str(v.ipaddr);
    case VK::Ip4: return ip4_str(v.ip4);
    case VK::Ip6: return ip6_str(v.ip6);
    case VK::Oid: return oid_name(v.oid);
    case VK::OidList: return list_out(v.objlist.list, v.objlist.count, VK::Oid, "");
    case VK::U8List: return list_out(v.u8list.list, v.u8list.count, VK::U8, "");
    case VK::I8List: return list_out(v.s8list.list, v.s8list.count, VK::I8, "");
    case VK::U16List: return list_out(v.u16list.list, v.u16list.count, VK::U16, "");
    case VK::I16List: return list_out(v.s16list.list, v.s16list.count, VK::I16, "");
    case VK::U32List: return list_out(v.u32list.list, v.u32list.count, VK::U32, "");
    case VK::I32List: return list_out(v.s32list.list, v.s32list.count, VK::I32, t.enum_type);
    case VK::VlanList: return list_out(v.vlanlist.list, v.vlanlist.count, VK::U16, "");
    case VK::U32Range: return std::to_string(v.u32range.min) + "-" + std::to_string(v.u32range.max);
    case VK::Chars: {
        std::string s(v.chardata, strnlen(v.chardata, sizeof(v.chardata)));
        return "\"" + s + "\"";
    }
    case VK::QosMapList: {
        std::string s = "[";
        for (uint32_t i = 0; i < v.qosmap.count && i < 256; i++)
            s += (i ? "," : "") + qos_params_out(v.qosmap.list[i].key) + "=" + qos_params_out(v.qosmap.list[i].value);
        return s + "]";
    }
    case VK::AclField: return field_out(t.sub, t.sub_enum, v.aclfield);
    case VK::AclAction: return action_out(t.sub, t.sub_enum, v.aclaction);
    case VK::MacsecSak: return hex_out(v.macsecsak, sizeof(v.macsecsak));
    case VK::MacsecAuthKey: return hex_out(v.macsecauthkey, sizeof(v.macsecauthkey));
    case VK::MacsecSalt: return hex_out(v.macsecsalt, sizeof(v.macsecsalt));
    default: return "<unprintable>";
    }
}
bool is_list(const VT& t)
{
    switch (t.k) {
    case VK::OidList: case VK::U8List: case VK::I8List: case VK::U16List: case VK::I16List: case VK::U32List:
    case VK::I32List: case VK::VlanList: case VK::QosMapList: return true;
    case VK::AclField: return t.sub == VK::OidList;
    case VK::AclAction: return t.sub == VK::OidList;
    default: return false;
    }
}
// Point every list a get may fill at a buffer of 256 elements.
void prepare_get(const VT& t, sai_attribute_t& a, Arena& ar)
{
    auto* buf = ar.alloc<uint64_t>(256);   // 256 elements of any scalar list type
    switch (t.k) {
    case VK::QosMapList:
        a.value.qosmap.count = 256;
        a.value.qosmap.list = ar.alloc<sai_qos_map_t>(256);
        break;
    case VK::AclField:
        a.value.aclfield.data.objlist.count = 256;
        a.value.aclfield.data.objlist.list = (sai_object_id_t*)buf;
        break;
    case VK::AclAction:
        a.value.aclaction.parameter.objlist.count = 256;
        a.value.aclaction.parameter.objlist.list = (sai_object_id_t*)buf;
        break;
    default:
        if (is_list(t)) {   // every list type: count + pointer at the start of the value
            a.value.objlist.count = 256;
            a.value.objlist.list = (sai_object_id_t*)buf;
        }
    }
}
uint32_t list_count(const VT& t, const sai_attribute_value_t& v)
{
    if (t.k == VK::QosMapList) return v.qosmap.count;
    if (t.k == VK::AclField) return v.aclfield.data.objlist.count;
    if (t.k == VK::AclAction) return v.aclaction.parameter.objlist.count;
    return v.objlist.count;
}

// ---- attributes of a type
struct Attr {
    sai_attr_id_t id;
    VT t;
};
Attr attr_of(const ObjType& ty, const std::string& short_name)
{
    std::string full = "SAI_" + type_upper(ty) + "_ATTR_" + short_name;
    if (ty.ot == SAI_OBJECT_TYPE_FDB_ENTRY) full = "SAI_FDB_ENTRY_ATTR_" + short_name;
    const AttrType* a = attr_named(full);
    if (!a) fail("no attribute " + full);
    VT t = vt_of(a->type);
    if (t.k == VK::Unsupported) fail(full + ": unsupported type '" + a->type + "'");
    return {(sai_attr_id_t)a->id, t};
}
std::vector<sai_attribute_t> attrs_in(const ObjType& ty, const std::vector<std::string>& w, size_t from, Arena& ar)
{
    std::vector<sai_attribute_t> v;
    for (size_t i = from; i < w.size(); i++) {
        size_t eq = w[i].find('=');
        if (eq == std::string::npos) fail("expected <ATTR>=<value>, got '" + w[i] + "'");
        Attr a = attr_of(ty, w[i].substr(0, eq));
        sai_attribute_t x;
        std::memset(&x, 0, sizeof(x));
        x.id = a.id;
        value_in(a.t, w[i].substr(eq + 1), x.value, ar);
        v.push_back(x);
    }
    return v;
}
sai_stat_id_t stat_of(const ObjType& ty, const std::string& short_name)
{
    std::string et = "sai_" + std::string(ty.name) + "_stat_t";
    long long v;
    if (!enum_value(et, "SAI_" + type_upper(ty) + "_STAT_" + short_name, v)) fail("no counter " + short_name + " of " + ty.name);
    return (sai_stat_id_t)v;
}

// ---- entry keys
struct Key {
    sai_fdb_entry_t fdb;
    sai_neighbor_entry_t nbr;
    sai_route_entry_t route;
};
Key key_of(const ObjType& ty, const std::string& s)
{
    Key k;
    std::memset(&k, 0, sizeof(k));
    size_t at = s.rfind('@');
    if (at == std::string::npos) fail("expected <key>@<name>");
    std::string a = s.substr(0, at), b = s.substr(at + 1);
    if (ty.ot == SAI_OBJECT_TYPE_FDB_ENTRY) {
        k.fdb.switch_id = g_sw;
        parse_mac(a, k.fdb.mac_address);
        k.fdb.bv_id = oid_named(b);
    } else if (ty.ot == SAI_OBJECT_TYPE_NEIGHBOR_ENTRY) {
        k.nbr.switch_id = g_sw;
        parse_ip(a, k.nbr.ip_address);
        k.nbr.rif_id = oid_named(b);
    } else {
        k.route.switch_id = g_sw;
        parse_prefix(a, k.route.destination);
        k.route.vr_id = oid_named(b);
    }
    return k;
}
bool is_entry(const ObjType& t)
{
    return t.ot == SAI_OBJECT_TYPE_FDB_ENTRY || t.ot == SAI_OBJECT_TYPE_NEIGHBOR_ENTRY || t.ot == SAI_OBJECT_TYPE_ROUTE_ENTRY;
}
sai_status_t entry_create(const ObjType& t, const Key& k, const std::vector<sai_attribute_t>& a)
{
    void* p = api_of(t.api);
    if (!p) return SAI_STATUS_NOT_IMPLEMENTED;
    uint32_t n = (uint32_t)a.size();
    if (t.ot == SAI_OBJECT_TYPE_FDB_ENTRY) return ((sai_fdb_api_t*)p)->create_fdb_entry(&k.fdb, n, a.data());
    if (t.ot == SAI_OBJECT_TYPE_NEIGHBOR_ENTRY) return ((sai_neighbor_api_t*)p)->create_neighbor_entry(&k.nbr, n, a.data());
    return ((sai_route_api_t*)p)->create_route_entry(&k.route, n, a.data());
}
sai_status_t entry_remove(const ObjType& t, const Key& k)
{
    void* p = api_of(t.api);
    if (!p) return SAI_STATUS_NOT_IMPLEMENTED;
    if (t.ot == SAI_OBJECT_TYPE_FDB_ENTRY) return ((sai_fdb_api_t*)p)->remove_fdb_entry(&k.fdb);
    if (t.ot == SAI_OBJECT_TYPE_NEIGHBOR_ENTRY) return ((sai_neighbor_api_t*)p)->remove_neighbor_entry(&k.nbr);
    return ((sai_route_api_t*)p)->remove_route_entry(&k.route);
}
sai_status_t entry_set(const ObjType& t, const Key& k, const sai_attribute_t& a)
{
    void* p = api_of(t.api);
    if (!p) return SAI_STATUS_NOT_IMPLEMENTED;
    if (t.ot == SAI_OBJECT_TYPE_FDB_ENTRY) return ((sai_fdb_api_t*)p)->set_fdb_entry_attribute(&k.fdb, &a);
    if (t.ot == SAI_OBJECT_TYPE_NEIGHBOR_ENTRY) return ((sai_neighbor_api_t*)p)->set_neighbor_entry_attribute(&k.nbr, &a);
    return ((sai_route_api_t*)p)->set_route_entry_attribute(&k.route, &a);
}
sai_status_t entry_get(const ObjType& t, const Key& k, uint32_t n, sai_attribute_t* a)
{
    void* p = api_of(t.api);
    if (!p) return SAI_STATUS_NOT_IMPLEMENTED;
    if (t.ot == SAI_OBJECT_TYPE_FDB_ENTRY) return ((sai_fdb_api_t*)p)->get_fdb_entry_attribute(&k.fdb, n, a);
    if (t.ot == SAI_OBJECT_TYPE_NEIGHBOR_ENTRY) return ((sai_neighbor_api_t*)p)->get_neighbor_entry_attribute(&k.nbr, n, a);
    return ((sai_route_api_t*)p)->get_route_entry_attribute(&k.route, n, a);
}

// ------------------------------------------------------------------------------- operations
struct Out {
    std::string status;
    std::vector<std::string> lines;   // indented result lines (attributes, counters)
};
std::string st(sai_status_t s) { return ob::sai_status_name(s); }

const ObjType& need_type(const std::vector<std::string>& w, size_t i)
{
    if (w.size() <= i) fail("missing <type>");
    const ObjType* t = type_named(w[i]);
    if (!t) fail("unknown type '" + w[i] + "'");
    return *t;
}
void need(const std::vector<std::string>& w, size_t n)
{
    if (w.size() < n) fail("too few words");
}
bool valid_name(const std::string& n)
{
    if (n.empty() || !std::islower((unsigned char)n[0])) return false;
    for (char c : n)
        if (!(std::islower((unsigned char)c) || std::isdigit((unsigned char)c) || c == '_' || c == '.' || c == '#')) return false;
    return n != "null";
}

void bind_switch_names()
{
    bind_name("switch", g_sw);
    sai_switch_api_t* sw = (sai_switch_api_t*)api_of(SAI_API_SWITCH);
    auto one = [&](sai_attr_id_t id, const char* n) {
        sai_attribute_t a;
        std::memset(&a, 0, sizeof(a));
        a.id = id;
        if (sw->get_switch_attribute(g_sw, 1, &a) == SAI_STATUS_SUCCESS && a.value.oid) bind_name(n, a.value.oid);
    };
    one(SAI_SWITCH_ATTR_CPU_PORT, "cpu");
    one(SAI_SWITCH_ATTR_DEFAULT_VLAN_ID, "vlan1");
    one(SAI_SWITCH_ATTR_DEFAULT_VIRTUAL_ROUTER_ID, "vr0");
    one(SAI_SWITCH_ATTR_DEFAULT_1Q_BRIDGE_ID, "bridge");
    one(SAI_SWITCH_ATTR_DEFAULT_STP_INST_ID, "stp0");
    one(SAI_SWITCH_ATTR_DEFAULT_TRAP_GROUP, "trapgroup0");
    std::vector<sai_object_id_t> ports(256);
    sai_attribute_t a;
    std::memset(&a, 0, sizeof(a));
    a.id = SAI_SWITCH_ATTR_PORT_LIST;
    a.value.objlist.count = 256;
    a.value.objlist.list = ports.data();
    if (sw->get_switch_attribute(g_sw, 1, &a) != SAI_STATUS_SUCCESS) return;
    ports.resize(a.value.objlist.count);
    for (size_t i = 0; i < ports.size(); i++) bind_name("port" + std::to_string(i), ports[i]);
    // bridge ports of the default .1Q bridge, named after their port
    sai_bridge_api_t* ba = (sai_bridge_api_t*)api_of(SAI_API_BRIDGE);
    if (!ba || !g_names.count("bridge")) return;
    std::vector<sai_object_id_t> bps(512);
    std::memset(&a, 0, sizeof(a));
    a.id = SAI_BRIDGE_ATTR_PORT_LIST;
    a.value.objlist.count = 512;
    a.value.objlist.list = bps.data();
    if (ba->get_bridge_attribute(g_names["bridge"], 1, &a) != SAI_STATUS_SUCCESS) return;
    bps.resize(a.value.objlist.count);
    for (auto bp : bps) {
        sai_attribute_t p;
        std::memset(&p, 0, sizeof(p));
        p.id = SAI_BRIDGE_PORT_ATTR_PORT_ID;
        if (ba->get_bridge_port_attribute(bp, 1, &p) != SAI_STATUS_SUCCESS) continue;
        for (size_t i = 0; i < ports.size(); i++)
            if (ports[i] == p.value.oid) bind_name("bport" + std::to_string(i), bp);
    }
}

int dest_of(const std::string& n)
{
    sai_object_id_t o = oid_named(n);
    int id = tp::backend_id(o);
    if (id < 0) fail("'" + n + "' has no backend id");
    return sai_object_type_query(o) == SAI_OBJECT_TYPE_LAG ? tp::lag_dest(id) : id;
}

Out run(const std::vector<std::string>& w)
{
    Out o;
    Arena ar;
    const std::string& op = w[0];
    if (op == "profile") {
        need(w, 2);
        size_t eq = w[1].find('=');
        if (eq == std::string::npos) fail("expected <KEY>=<VALUE>");
        g_profile[w[1].substr(0, eq)] = unquote(w[1].substr(eq + 1));
        o.status = "SUCCESS";
    } else if (op == "switch") {
        if (!g_initialized) {
            sai_status_t s = sai_api_initialize(0, &g_smt);
            if (s != SAI_STATUS_SUCCESS) { o.status = st(s); return o; }
            g_initialized = true;
        }
        const ObjType& ty = *type_named("switch");
        std::vector<sai_attribute_t> at(3);
        std::memset(at.data(), 0, sizeof(sai_attribute_t) * 3);
        at[0].id = SAI_SWITCH_ATTR_INIT_SWITCH;
        at[0].value.booldata = true;
        at[1].id = SAI_SWITCH_ATTR_PORT_STATE_CHANGE_NOTIFY;
        at[1].value.ptr = (void*)on_port;
        at[2].id = SAI_SWITCH_ATTR_FDB_EVENT_NOTIFY;
        at[2].value.ptr = (void*)on_fdb;
        for (auto& x : attrs_in(ty, w, 1, ar)) at.push_back(x);
        sai_switch_api_t* sw = (sai_switch_api_t*)api_of(SAI_API_SWITCH);
        sai_status_t s = sw ? sw->create_switch(&g_sw, (uint32_t)at.size(), at.data()) : SAI_STATUS_NOT_IMPLEMENTED;
        o.status = st(s);
        if (s == SAI_STATUS_SUCCESS) bind_switch_names();
    } else if (op == "remove_switch") {
        sai_switch_api_t* sw = (sai_switch_api_t*)api_of(SAI_API_SWITCH);
        o.status = st(sw ? sw->remove_switch(g_sw) : SAI_STATUS_UNINITIALIZED);
        forget_names();
        g_sw = 0;
    } else if (op == "create") {
        need(w, 3);
        const ObjType& ty = need_type(w, 1);
        if (is_entry(ty)) {
            Key k = key_of(ty, w[2]);
            o.status = st(entry_create(ty, k, attrs_in(ty, w, 3, ar)));
        } else {
            if (!ty.create) fail(std::string(ty.name) + " has no create");
            if (!valid_name(w[2])) fail("bad name '" + w[2] + "'");
            if (g_names.count(w[2])) fail("name '" + w[2] + "' is bound");
            auto at = attrs_in(ty, w, 3, ar);
            void* p = api_of(ty.api);
            sai_object_id_t id = 0;
            sai_status_t s = p ? ty.create(p, &id, g_sw, (uint32_t)at.size(), at.data()) : SAI_STATUS_NOT_IMPLEMENTED;
            o.status = st(s);
            if (s == SAI_STATUS_SUCCESS) bind_name(w[2], id);
        }
    } else if (op == "remove") {
        need(w, 3);
        const ObjType& ty = need_type(w, 1);
        if (is_entry(ty)) {
            o.status = st(entry_remove(ty, key_of(ty, w[2])));
        } else {
            if (!ty.remove) fail(std::string(ty.name) + " has no remove");
            void* p = api_of(ty.api);
            o.status = st(p ? ty.remove(p, oid_named(w[2])) : SAI_STATUS_NOT_IMPLEMENTED);
        }
    } else if (op == "set") {
        need(w, 4);
        if (w.size() != 4) fail("set takes one attribute");
        const ObjType& ty = need_type(w, 1);
        auto at = attrs_in(ty, w, 3, ar);
        if (is_entry(ty)) {
            o.status = st(entry_set(ty, key_of(ty, w[2]), at[0]));
        } else {
            void* p = api_of(ty.api);
            o.status = st(p ? ty.set(p, oid_named(w[2]), &at[0]) : SAI_STATUS_NOT_IMPLEMENTED);
        }
    } else if (op == "get") {
        need(w, 4);
        const ObjType& ty = need_type(w, 1);
        std::vector<Attr> as;
        std::vector<sai_attribute_t> at;
        for (size_t i = 3; i < w.size(); i++) {
            Attr a = attr_of(ty, w[i]);
            sai_attribute_t x;
            std::memset(&x, 0, sizeof(x));
            x.id = a.id;
            prepare_get(a.t, x, ar);
            as.push_back(a);
            at.push_back(x);
        }
        sai_status_t s;
        if (is_entry(ty)) {
            s = entry_get(ty, key_of(ty, w[2]), (uint32_t)at.size(), at.data());
        } else {
            void* p = api_of(ty.api);
            s = p ? ty.get(p, oid_named(w[2]), (uint32_t)at.size(), at.data()) : SAI_STATUS_NOT_IMPLEMENTED;
        }
        o.status = st(s);
        for (size_t i = 0; i < at.size(); i++) {
            if (s == SAI_STATUS_SUCCESS) o.lines.push_back(w[3 + i] + "=" + value_out(as[i].t, at[i].value));
            else if (s == SAI_STATUS_BUFFER_OVERFLOW && is_list(as[i].t))
                o.lines.push_back(w[3 + i] + " count=" + std::to_string(list_count(as[i].t, at[i].value)));
        }
    } else if (op == "stats" || op == "clear") {
        need(w, 4);
        const ObjType& ty = need_type(w, 1);
        if (!ty.stats) fail(std::string(ty.name) + " has no counters here");
        std::vector<sai_stat_id_t> ids;
        for (size_t i = 3; i < w.size(); i++) ids.push_back(stat_of(ty, w[i]));
        void* p = api_of(ty.api);
        sai_object_id_t obj = oid_named(w[2]);
        if (op == "stats") {
            std::vector<uint64_t> v(ids.size(), 0);
            sai_status_t s = p ? ty.stats(p, obj, (uint32_t)ids.size(), ids.data(), v.data()) : SAI_STATUS_NOT_IMPLEMENTED;
            o.status = st(s);
            if (s == SAI_STATUS_SUCCESS)
                for (size_t i = 0; i < ids.size(); i++) o.lines.push_back(w[3 + i] + "=" + std::to_string(v[i]));
        } else {
            o.status = st(p ? ty.clear(p, obj, (uint32_t)ids.size(), ids.data()) : SAI_STATUS_NOT_IMPLEMENTED);
        }
    } else if (op == "flush") {
        ObjType fl = {"fdb_flush", SAI_OBJECT_TYPE_FDB_FLUSH, SAI_API_FDB, nullptr, nullptr, nullptr, nullptr, nullptr, nullptr};
        auto at = attrs_in(fl, w, 1, ar);
        sai_fdb_api_t* p = (sai_fdb_api_t*)api_of(SAI_API_FDB);
        o.status = st(p ? p->flush_fdb_entries(g_sw, (uint32_t)at.size(), at.data()) : SAI_STATUS_NOT_IMPLEMENTED);
    } else if (op == "inject") {
        need(w, 2);
        if (w[1] == "link") {
            need(w, 4);
            if (w[3] != "up" && w[3] != "down") fail("expected up / down");
            tp::inject_link(dest_of(w[2]), w[3] == "up");
        } else if (w[1] == "fdb") {
            need(w, 6);
            if (w[2] != "learned" && w[2] != "aged") fail("expected learned / aged");
            uint8_t mac[6];
            parse_mac(w[3], mac);
            tp::inject_fdb(w[2] == "learned", mac, (uint16_t)num(w[4]), dest_of(w[5]));
        } else if (w[1] == "stat") {
            need(w, 5);
            const ObjType& pt = *type_named("port");
            tp::set_port_stat(dest_of(w[2]), stat_of(pt, w[3]), (uint64_t)num(w[4]));
        } else {
            fail("inject link | fdb | stat");
        }
        o.status = "SUCCESS";
    } else if (op == "mdio") {
        need(w, 5);
        tp::mdio_set((unsigned)hexnum(w[1]), (unsigned)hexnum(w[2]), (unsigned)hexnum(w[3]), (unsigned)hexnum(w[4]));
        o.status = "SUCCESS";
    } else if (op == "feature") {
        need(w, 3);
        tp::Feature f = w[1] == "qos" ? tp::FEATURE_QOS : w[1] == "mirror" ? tp::FEATURE_MIRROR : tp::FEATURE_L2_EXTRAS;
        if (w[1] != "qos" && w[1] != "mirror" && w[1] != "l2") fail("feature qos | mirror | l2");
        if (w[2] != "on" && w[2] != "off") fail("expected on / off");
        tp::set_feature(f, w[2] == "on");
        o.status = "SUCCESS";
    } else if (op == "linkscan") {
        tp::linkscan();
        o.status = "SUCCESS";
    } else {
        fail("unknown operation '" + op + "'");
    }
    return o;
}

std::vector<std::string> take_notes()
{
    std::lock_guard<std::mutex> g(g_nmu);
    std::vector<std::string> v;
    v.swap(g_notes);
    return v;
}
std::string note_line(const std::string& raw)
{
    std::vector<std::string> f = split(raw, '\t');
    if (f[0] == "port") {
        sai_object_id_t p = std::strtoull(f[1].c_str(), nullptr, 10);
        int s = std::atoi(f[2].c_str());
        return "notify port " + oid_name(p) + " " + (s == SAI_PORT_OPER_STATUS_UP ? "UP" : s == SAI_PORT_OPER_STATUS_DOWN ? "DOWN" : std::to_string(s));
    }
    sai_object_id_t bv = std::strtoull(f[3].c_str(), nullptr, 10), bp = std::strtoull(f[4].c_str(), nullptr, 10);
    return std::string("notify fdb ") + fdb_event_name(std::atoi(f[1].c_str())) + " " + f[2] + "@" + oid_name(bv) +
           " bridge_port=" + oid_name(bp);
}

} // namespace

int main(int argc, char** argv)
{
    std::string path;
    for (int i = 1; i < argc; i++) {
        std::string a = argv[i];
        if (a == "--queries") g_full = true;
        else if (path.empty()) path = a;
        else {
            std::fprintf(stderr, "usage: %s [--queries] [script]\n", argv[0]);
            return 2;
        }
    }
    std::ifstream file;
    std::istream* in = &std::cin;
    if (!path.empty() && path != "-") {
        file.open(path);
        if (!file) {
            std::fprintf(stderr, "%s: cannot read %s\n", argv[0], path.c_str());
            return 2;
        }
        in = &file;
    }
    g_profile["SAI_OPENBCM_BACKEND"] = "openbcm";
    g_profile["SAI_OPENBCM_FAKE_PORTS"] = "8";
    g_profile["SAI_OPENBCM_FAKE_LANES"] = "4";
    g_profile["SAI_OPENBCM_FAKE_SPEED"] = "100000";
    // On a switch the BDE nodes exist; the fake opens no device (as sai-macsec's driver).
    const char* on_hw = std::getenv("SAI_RS_VIRTUAL_ON_HARDWARE");
    if (on_hw && std::strcmp(on_hw, "1") == 0) g_profile["SAI_OPENBCM_FAKE_ON_HARDWARE"] = "1";

    std::string line;
    while (std::getline(*in, line)) {
        std::vector<std::string> w = words(line);
        if (w.empty() || w[0][0] == '#') continue;
        std::string echo;
        for (auto& x : w) echo += (echo.empty() ? "" : " ") + x;
        std::printf("> %s\n", echo.c_str());
        uint64_t raised0 = g_log->events_raised();
        uint64_t delivered0 = tp::events_delivered();
        Out o;
        try {
            o = run(w);
        } catch (const ParseError& e) {
            std::printf("= PARSE_ERROR %s\n", e.what.c_str());
            g_log->take();
            continue;
        }
        // let the library process the events this operation raised (not after remove_switch:
        // its queue is dropped with the switch)
        if (g_initialized && w[0] != "remove_switch") {
            uint64_t want = g_log->events_raised() - raised0;
            for (int i = 0; i < 200 && tp::events_delivered() - delivered0 < want; i++)
                std::this_thread::sleep_for(std::chrono::milliseconds(10));
        }
        std::printf("= %s\n", o.status.c_str());
        for (auto& l : o.lines) std::printf("  %s\n", l.c_str());
        for (auto& l : g_log->take()) std::printf("  be %s\n", l.c_str());
        for (auto& n : take_notes()) std::printf("  %s\n", note_line(n).c_str());
        std::fflush(stdout);
    }
    return 0;
}
