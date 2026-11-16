// SPDX-License-Identifier: Apache-2.0
// sai-script: run a JSON list of SAI calls against any libsai (generic SAI,
// one source for our SAI 1.19 build and isai's 1.16 headers; only
// attributes and values both have are named). For the oracle (README.md):
// before and after every step it appends a marker line to IFCS_TRACE_FILE,
// so the libifcs trace can be cut per SAI call.
//   sai-script PROFILE SCRIPT.json
// PROFILE: key=value lines (the SAI profile). SCRIPT: see oracle/README.md.
extern "C" {
#include <sai.h>
}

#include <nlohmann/json.hpp>

#include <arpa/inet.h>
#include <fcntl.h>
#include <sys/socket.h>
#include <sys/un.h>
#include <unistd.h>

#include <chrono>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <fstream>
#include <map>
#include <string>
#include <thread>
#include <vector>

using json = nlohmann::json;

static std::map<std::string, std::string> g_profile;
static std::map<std::string, std::string>::iterator g_it;
static const char* prof_get(sai_switch_profile_id_t, const char* k)
{
    auto it = g_profile.find(k);
    return it == g_profile.end() ? nullptr : it->second.c_str();
}
static int prof_next(sai_switch_profile_id_t, const char** k, const char** v)
{
    if (!k || !v) { g_it = g_profile.begin(); return 0; }
    if (g_it == g_profile.end()) return -1;
    *k = g_it->first.c_str();
    *v = g_it->second.c_str();
    ++g_it;
    return 0;
}
static void on_port_state(uint32_t n, const sai_port_oper_status_notification_t* d)
{
    for (uint32_t i = 0; i < n; i++)
        printf("event port 0x%lx oper %d\n", (unsigned long)d[i].port_id, (int)d[i].port_state);
    fflush(stdout);
}
static void on_shutdown(sai_object_id_t) { printf("event shutdown request\n"); fflush(stdout); }
static void on_fdb(uint32_t n, const sai_fdb_event_notification_data_t* d)
{
    for (uint32_t i = 0; i < n; i++) printf("event fdb %d\n", (int)d[i].event_type);
    fflush(stdout);
}

static int g_mark = -1;
static void mark(const std::string& s)
{
    if (g_mark < 0) return;
    std::string l = "M " + s + "\n";
    (void)!write(g_mark, l.data(), l.size());
}
static double now_ms()
{
    return std::chrono::duration<double, std::milli>(std::chrono::steady_clock::now().time_since_epoch()).count();
}

// ------------------------------------------------------------ name tables
struct Name { const char* n; int64_t v; };
#define N(prefix, x) {#x, prefix##x}
static const Name k_types[] = {
    N(SAI_OBJECT_TYPE_, SWITCH), N(SAI_OBJECT_TYPE_, PORT), N(SAI_OBJECT_TYPE_, HOSTIF), N(SAI_OBJECT_TYPE_, HOSTIF_TRAP_GROUP),
    N(SAI_OBJECT_TYPE_, HOSTIF_TRAP), N(SAI_OBJECT_TYPE_, HOSTIF_TABLE_ENTRY), N(SAI_OBJECT_TYPE_, POLICER),
    N(SAI_OBJECT_TYPE_, VLAN), N(SAI_OBJECT_TYPE_, VLAN_MEMBER), N(SAI_OBJECT_TYPE_, ROUTER_INTERFACE),
    N(SAI_OBJECT_TYPE_, NEXT_HOP), N(SAI_OBJECT_TYPE_, VIRTUAL_ROUTER), N(SAI_OBJECT_TYPE_, BRIDGE_PORT),
};
static const Name k_attrs_switch[] = {
    N(SAI_SWITCH_ATTR_, INIT_SWITCH), N(SAI_SWITCH_ATTR_, PORT_LIST), N(SAI_SWITCH_ATTR_, PORT_NUMBER),
    N(SAI_SWITCH_ATTR_, CPU_PORT), N(SAI_SWITCH_ATTR_, DEFAULT_VIRTUAL_ROUTER_ID), N(SAI_SWITCH_ATTR_, DEFAULT_1Q_BRIDGE_ID),
    N(SAI_SWITCH_ATTR_, DEFAULT_TRAP_GROUP), N(SAI_SWITCH_ATTR_, SRC_MAC_ADDRESS), N(SAI_SWITCH_ATTR_, FDB_AGING_TIME),
    N(SAI_SWITCH_ATTR_, DEFAULT_VLAN_ID), N(SAI_SWITCH_ATTR_, MAX_TEMP), N(SAI_SWITCH_ATTR_, PORT_STATE_CHANGE_NOTIFY),
    N(SAI_SWITCH_ATTR_, SWITCH_SHUTDOWN_REQUEST_NOTIFY), N(SAI_SWITCH_ATTR_, FDB_EVENT_NOTIFY),
};
static const Name k_attrs_port[] = {
    N(SAI_PORT_ATTR_, ADMIN_STATE), N(SAI_PORT_ATTR_, OPER_STATUS), N(SAI_PORT_ATTR_, SPEED), N(SAI_PORT_ATTR_, MTU),
    N(SAI_PORT_ATTR_, FEC_MODE), N(SAI_PORT_ATTR_, HW_LANE_LIST), N(SAI_PORT_ATTR_, LINK_TRAINING_ENABLE),
    N(SAI_PORT_ATTR_, PORT_VLAN_ID), N(SAI_PORT_ATTR_, AUTO_NEG_MODE),
};
static const Name k_attrs_hostif[] = {
    N(SAI_HOSTIF_ATTR_, TYPE), N(SAI_HOSTIF_ATTR_, OBJ_ID), N(SAI_HOSTIF_ATTR_, NAME), N(SAI_HOSTIF_ATTR_, OPER_STATUS),
    N(SAI_HOSTIF_ATTR_, QUEUE), N(SAI_HOSTIF_ATTR_, VLAN_TAG),
};
static const Name k_attrs_trap_group[] = {
    N(SAI_HOSTIF_TRAP_GROUP_ATTR_, ADMIN_STATE), N(SAI_HOSTIF_TRAP_GROUP_ATTR_, QUEUE), N(SAI_HOSTIF_TRAP_GROUP_ATTR_, POLICER),
};
static const Name k_attrs_trap[] = {
    N(SAI_HOSTIF_TRAP_ATTR_, TRAP_TYPE), N(SAI_HOSTIF_TRAP_ATTR_, PACKET_ACTION), N(SAI_HOSTIF_TRAP_ATTR_, TRAP_PRIORITY),
    N(SAI_HOSTIF_TRAP_ATTR_, TRAP_GROUP),
};
static const Name k_attrs_table_entry[] = {
    N(SAI_HOSTIF_TABLE_ENTRY_ATTR_, TYPE), N(SAI_HOSTIF_TABLE_ENTRY_ATTR_, OBJ_ID), N(SAI_HOSTIF_TABLE_ENTRY_ATTR_, TRAP_ID),
    N(SAI_HOSTIF_TABLE_ENTRY_ATTR_, CHANNEL_TYPE), N(SAI_HOSTIF_TABLE_ENTRY_ATTR_, HOST_IF),
};
static const Name k_attrs_policer[] = {
    N(SAI_POLICER_ATTR_, METER_TYPE), N(SAI_POLICER_ATTR_, MODE), N(SAI_POLICER_ATTR_, COLOR_SOURCE), N(SAI_POLICER_ATTR_, CBS),
    N(SAI_POLICER_ATTR_, CIR), N(SAI_POLICER_ATTR_, PBS), N(SAI_POLICER_ATTR_, PIR), N(SAI_POLICER_ATTR_, GREEN_PACKET_ACTION),
    N(SAI_POLICER_ATTR_, YELLOW_PACKET_ACTION), N(SAI_POLICER_ATTR_, RED_PACKET_ACTION),
};
static const Name k_attrs_vlan[] = {N(SAI_VLAN_ATTR_, VLAN_ID), N(SAI_VLAN_ATTR_, MEMBER_LIST)};
static const Name k_attrs_vlan_member[] = {
    N(SAI_VLAN_MEMBER_ATTR_, VLAN_ID), N(SAI_VLAN_MEMBER_ATTR_, BRIDGE_PORT_ID), N(SAI_VLAN_MEMBER_ATTR_, VLAN_TAGGING_MODE),
};
static const Name k_attrs_rif[] = {
    N(SAI_ROUTER_INTERFACE_ATTR_, VIRTUAL_ROUTER_ID), N(SAI_ROUTER_INTERFACE_ATTR_, TYPE), N(SAI_ROUTER_INTERFACE_ATTR_, PORT_ID),
    N(SAI_ROUTER_INTERFACE_ATTR_, VLAN_ID), N(SAI_ROUTER_INTERFACE_ATTR_, SRC_MAC_ADDRESS), N(SAI_ROUTER_INTERFACE_ATTR_, MTU),
};
static const Name k_attrs_route[] = {N(SAI_ROUTE_ENTRY_ATTR_, PACKET_ACTION), N(SAI_ROUTE_ENTRY_ATTR_, NEXT_HOP_ID)};
static const Name k_attrs_neighbor[] = {N(SAI_NEIGHBOR_ENTRY_ATTR_, DST_MAC_ADDRESS)};
static const Name k_attrs_nexthop[] = {
    N(SAI_NEXT_HOP_ATTR_, TYPE), N(SAI_NEXT_HOP_ATTR_, IP), N(SAI_NEXT_HOP_ATTR_, ROUTER_INTERFACE_ID),
};
static const Name k_attrs_bridge_port[] = {N(SAI_BRIDGE_PORT_ATTR_, PORT_ID), N(SAI_BRIDGE_PORT_ATTR_, TYPE)};
static const Name k_values[] = {
    // hostif trap types
    N(SAI_HOSTIF_TRAP_TYPE_, STP), N(SAI_HOSTIF_TRAP_TYPE_, LACP), N(SAI_HOSTIF_TRAP_TYPE_, EAPOL), N(SAI_HOSTIF_TRAP_TYPE_, LLDP),
    N(SAI_HOSTIF_TRAP_TYPE_, PVRST), N(SAI_HOSTIF_TRAP_TYPE_, IGMP_TYPE_QUERY), N(SAI_HOSTIF_TRAP_TYPE_, UDLD),
    N(SAI_HOSTIF_TRAP_TYPE_, SAMPLEPACKET), N(SAI_HOSTIF_TRAP_TYPE_, ARP_REQUEST), N(SAI_HOSTIF_TRAP_TYPE_, ARP_RESPONSE),
    N(SAI_HOSTIF_TRAP_TYPE_, DHCP), N(SAI_HOSTIF_TRAP_TYPE_, IPV6_NEIGHBOR_DISCOVERY), N(SAI_HOSTIF_TRAP_TYPE_, DHCPV6),
    N(SAI_HOSTIF_TRAP_TYPE_, IP2ME), N(SAI_HOSTIF_TRAP_TYPE_, SSH), N(SAI_HOSTIF_TRAP_TYPE_, SNMP),
    N(SAI_HOSTIF_TRAP_TYPE_, BGP), N(SAI_HOSTIF_TRAP_TYPE_, BGPV6), N(SAI_HOSTIF_TRAP_TYPE_, BFD), N(SAI_HOSTIF_TRAP_TYPE_, BFDV6),
    N(SAI_HOSTIF_TRAP_TYPE_, L3_MTU_ERROR), N(SAI_HOSTIF_TRAP_TYPE_, TTL_ERROR), N(SAI_HOSTIF_TRAP_TYPE_, NEIGHBOR_MISS),
    // packet actions
    N(SAI_PACKET_ACTION_, DROP), N(SAI_PACKET_ACTION_, FORWARD), N(SAI_PACKET_ACTION_, COPY), N(SAI_PACKET_ACTION_, TRAP),
    N(SAI_PACKET_ACTION_, LOG),
    // hostif
    N(SAI_HOSTIF_TYPE_, NETDEV), N(SAI_HOSTIF_VLAN_TAG_, STRIP), N(SAI_HOSTIF_VLAN_TAG_, KEEP), N(SAI_HOSTIF_VLAN_TAG_, ORIGINAL),
    N(SAI_HOSTIF_TABLE_ENTRY_TYPE_, WILDCARD), N(SAI_HOSTIF_TABLE_ENTRY_TYPE_, TRAP_ID),
    N(SAI_HOSTIF_TABLE_ENTRY_CHANNEL_TYPE_, NETDEV_PHYSICAL_PORT), N(SAI_HOSTIF_TABLE_ENTRY_CHANNEL_TYPE_, NETDEV_L3),
    N(SAI_HOSTIF_TABLE_ENTRY_CHANNEL_TYPE_, CB), N(SAI_HOSTIF_TABLE_ENTRY_CHANNEL_TYPE_, GENETLINK),
    // policer
    N(SAI_METER_TYPE_, PACKETS), N(SAI_METER_TYPE_, BYTES), N(SAI_POLICER_MODE_, SR_TCM), N(SAI_POLICER_MODE_, TR_TCM),
    N(SAI_POLICER_MODE_, STORM_CONTROL), N(SAI_POLICER_COLOR_SOURCE_, BLIND), N(SAI_POLICER_COLOR_SOURCE_, AWARE),
    // port, L2, L3
    N(SAI_PORT_FEC_MODE_, NONE), N(SAI_PORT_FEC_MODE_, RS), N(SAI_PORT_FEC_MODE_, FC),
    N(SAI_VLAN_TAGGING_MODE_, UNTAGGED), N(SAI_VLAN_TAGGING_MODE_, TAGGED),
    N(SAI_ROUTER_INTERFACE_TYPE_, PORT), N(SAI_ROUTER_INTERFACE_TYPE_, VLAN), N(SAI_ROUTER_INTERFACE_TYPE_, LOOPBACK),
    N(SAI_NEXT_HOP_TYPE_, IP),
};
static const Name k_port_stats[] = {
    N(SAI_PORT_STAT_, IF_IN_OCTETS), N(SAI_PORT_STAT_, IF_IN_UCAST_PKTS), N(SAI_PORT_STAT_, IF_IN_NON_UCAST_PKTS),
    N(SAI_PORT_STAT_, IF_IN_DISCARDS), N(SAI_PORT_STAT_, IF_IN_ERRORS), N(SAI_PORT_STAT_, IF_IN_BROADCAST_PKTS),
    N(SAI_PORT_STAT_, IF_IN_MULTICAST_PKTS), N(SAI_PORT_STAT_, IF_OUT_OCTETS), N(SAI_PORT_STAT_, IF_OUT_UCAST_PKTS),
    N(SAI_PORT_STAT_, IF_OUT_NON_UCAST_PKTS), N(SAI_PORT_STAT_, IF_OUT_DISCARDS), N(SAI_PORT_STAT_, IF_OUT_ERRORS),
    N(SAI_PORT_STAT_, IF_OUT_BROADCAST_PKTS), N(SAI_PORT_STAT_, IF_OUT_MULTICAST_PKTS),
    N(SAI_PORT_STAT_, IF_IN_FEC_CORRECTABLE_FRAMES), N(SAI_PORT_STAT_, IF_IN_FEC_NOT_CORRECTABLE_FRAMES),
};
#undef N
template <size_t K> static bool lookup(const Name (&t)[K], const std::string& s, int64_t& v)
{
    for (auto& n : t)
        if (s == n.n) { v = n.v; return true; }
    return false;
}

struct Fail { std::string what; };
static int64_t num(const json& j, const Name* t = nullptr, size_t k = 0)
{
    if (j.is_number_integer()) return j.get<int64_t>();
    if (j.is_boolean()) return j.get<bool>() ? 1 : 0;
    std::string s = j.get<std::string>();
    for (size_t i = 0; t && i < k; i++)
        if (s == t[i].n) return t[i].v;
    int64_t v;
    if (lookup(k_values, s, v)) return v;
    try { return std::stoll(s, nullptr, 0); } catch (...) {}
    throw Fail{"unknown value " + s};
}

static std::map<std::string, sai_object_id_t> g_obj;
static std::map<std::string, std::vector<sai_object_id_t>> g_lists;
static sai_object_id_t g_switch = SAI_NULL_OBJECT_ID;

static sai_object_id_t oid(const json& j)
{
    if (j.is_number_integer()) return j.get<uint64_t>();
    std::string s = j.get<std::string>();
    if (s == "switch") return g_switch;
    if (s == "null") return SAI_NULL_OBJECT_ID;
    if (!s.empty() && s[0] == '$') s = s.substr(1);
    auto b = s.find('[');
    if (b != std::string::npos) {
        std::string n = s.substr(0, b);
        size_t i = std::stoul(s.substr(b + 1));
        auto it = g_lists.find(n);
        if (it == g_lists.end() || i >= it->second.size()) throw Fail{"no list element " + s};
        return it->second[i];
    }
    auto it = g_obj.find(s);
    if (it == g_obj.end()) throw Fail{"unknown object " + s};
    return it->second;
}

static void parse_mac(const std::string& s, sai_mac_t m)
{
    unsigned b[6] = {0};
    if (sscanf(s.c_str(), "%x:%x:%x:%x:%x:%x", &b[0], &b[1], &b[2], &b[3], &b[4], &b[5]) != 6) throw Fail{"bad mac " + s};
    for (int i = 0; i < 6; i++) m[i] = (uint8_t)b[i];
}
static sai_ip_address_t parse_ip(const std::string& s)
{
    sai_ip_address_t a;
    std::memset(&a, 0, sizeof(a));
    if (s.find(':') != std::string::npos) {
        a.addr_family = SAI_IP_ADDR_FAMILY_IPV6;
        if (inet_pton(AF_INET6, s.c_str(), a.addr.ip6) != 1) throw Fail{"bad ip " + s};
    } else {
        a.addr_family = SAI_IP_ADDR_FAMILY_IPV4;
        if (inet_pton(AF_INET, s.c_str(), &a.addr.ip4) != 1) throw Fail{"bad ip " + s};
    }
    return a;
}
static sai_ip_prefix_t parse_prefix(const std::string& s)
{
    sai_ip_prefix_t p;
    std::memset(&p, 0, sizeof(p));
    auto sl = s.find('/');
    sai_ip_address_t a = parse_ip(s.substr(0, sl));
    int len = sl == std::string::npos ? (a.addr_family == SAI_IP_ADDR_FAMILY_IPV4 ? 32 : 128) : std::stoi(s.substr(sl + 1));
    p.addr_family = a.addr_family;
    if (a.addr_family == SAI_IP_ADDR_FAMILY_IPV4) {
        p.addr.ip4 = a.addr.ip4;
        p.mask.ip4 = htonl(len == 0 ? 0 : 0xffffffffu << (32 - len));
    } else {
        std::memcpy(p.addr.ip6, a.addr.ip6, 16);
        for (int i = 0; i < 16; i++) {
            int bits = len - i * 8;
            p.mask.ip6[i] = bits >= 8 ? 0xff : bits <= 0 ? 0 : (uint8_t)(0xff << (8 - bits));
        }
    }
    return p;
}

struct Storage { std::vector<std::vector<sai_object_id_t>> oids; std::vector<std::vector<uint32_t>> u32s; };

static const Name* attr_table(sai_object_type_t t, size_t& k)
{
#define T(x) k = sizeof(x) / sizeof(x[0]); return x
    switch (t) {
    case SAI_OBJECT_TYPE_SWITCH: T(k_attrs_switch);
    case SAI_OBJECT_TYPE_PORT: T(k_attrs_port);
    case SAI_OBJECT_TYPE_HOSTIF: T(k_attrs_hostif);
    case SAI_OBJECT_TYPE_HOSTIF_TRAP_GROUP: T(k_attrs_trap_group);
    case SAI_OBJECT_TYPE_HOSTIF_TRAP: T(k_attrs_trap);
    case SAI_OBJECT_TYPE_HOSTIF_TABLE_ENTRY: T(k_attrs_table_entry);
    case SAI_OBJECT_TYPE_POLICER: T(k_attrs_policer);
    case SAI_OBJECT_TYPE_VLAN: T(k_attrs_vlan);
    case SAI_OBJECT_TYPE_VLAN_MEMBER: T(k_attrs_vlan_member);
    case SAI_OBJECT_TYPE_ROUTER_INTERFACE: T(k_attrs_rif);
    case SAI_OBJECT_TYPE_ROUTE_ENTRY: T(k_attrs_route);
    case SAI_OBJECT_TYPE_NEIGHBOR_ENTRY: T(k_attrs_neighbor);
    case SAI_OBJECT_TYPE_NEXT_HOP: T(k_attrs_nexthop);
    case SAI_OBJECT_TYPE_BRIDGE_PORT: T(k_attrs_bridge_port);
    default: k = 0; return nullptr;
    }
#undef T
}

// One attribute ["NAME", "type", value]. Types: bool u8 u16 u32 s32 u64 oid
// mac ip chardata oidlist(n) u32list([..]) ptr(port_state|fdb|shutdown).
static sai_attribute_t make_attr(sai_object_type_t t, const json& a, Storage& st)
{
    sai_attribute_t x;
    std::memset(&x, 0, sizeof(x));
    size_t k = 0;
    const Name* tab = attr_table(t, k);
    int64_t id = 0;
    std::string an = a[0].is_string() ? a[0].get<std::string>() : "";
    bool found = false;
    for (size_t i = 0; tab && i < k; i++)
        if (an == tab[i].n) { id = tab[i].v; found = true; }
    if (!found) id = num(a[0]);
    x.id = (sai_attr_id_t)id;
    std::string ty = a[1].get<std::string>();
    const json v = a.size() > 2 ? a[2] : json();
    if (v.is_null() && ty != "oidlist" && ty != "u32list") return x;   // a get: no value
    if (ty == "bool") x.value.booldata = v.get<bool>();
    else if (ty == "u8") x.value.u8 = (uint8_t)num(v);
    else if (ty == "u16") x.value.u16 = (uint16_t)num(v);
    else if (ty == "u32") x.value.u32 = (uint32_t)num(v);
    else if (ty == "s32") x.value.s32 = (int32_t)num(v);
    else if (ty == "u64") x.value.u64 = (uint64_t)num(v);
    else if (ty == "oid") x.value.oid = oid(v);
    else if (ty == "mac") parse_mac(v.get<std::string>(), x.value.mac);
    else if (ty == "ip") x.value.ipaddr = parse_ip(v.get<std::string>());
    else if (ty == "chardata") std::snprintf(x.value.chardata, sizeof(x.value.chardata), "%s", v.get<std::string>().c_str());
    else if (ty == "oidlist") {
        st.oids.emplace_back(v.is_number() ? v.get<size_t>() : 256);
        x.value.objlist.count = (uint32_t)st.oids.back().size();
        x.value.objlist.list = st.oids.back().data();
    } else if (ty == "u32list") {
        st.u32s.emplace_back();
        if (v.is_array()) for (auto& e : v) st.u32s.back().push_back((uint32_t)num(e));
        else st.u32s.back().resize(v.is_number() ? v.get<size_t>() : 64);
        x.value.u32list.count = (uint32_t)st.u32s.back().size();
        x.value.u32list.list = st.u32s.back().data();
    } else if (ty == "ptr") {
        std::string p = v.get<std::string>();
        x.value.ptr = p == "port_state" ? (sai_pointer_t)on_port_state : p == "fdb" ? (sai_pointer_t)on_fdb
                    : p == "shutdown" ? (sai_pointer_t)on_shutdown : nullptr;
    } else throw Fail{"unknown attr type " + ty};
    return x;
}

static std::string show(sai_object_type_t, const sai_attribute_t& x, const std::string& ty)
{
    char b[256];
    if (ty == "bool") return x.value.booldata ? "true" : "false";
    if (ty == "s32") { snprintf(b, sizeof(b), "%d", x.value.s32); return b; }
    if (ty == "u32") { snprintf(b, sizeof(b), "%u", x.value.u32); return b; }
    if (ty == "u64") { snprintf(b, sizeof(b), "%llu", (unsigned long long)x.value.u64); return b; }
    if (ty == "oid") { snprintf(b, sizeof(b), "0x%llx", (unsigned long long)x.value.oid); return b; }
    if (ty == "oidlist") { snprintf(b, sizeof(b), "%u oids", x.value.objlist.count); return b; }
    if (ty == "u32list") {
        std::string s = "[";
        for (uint32_t i = 0; i < x.value.u32list.count; i++) s += (i ? "," : "") + std::to_string(x.value.u32list.list[i]);
        return s + "]";
    }
    if (ty == "mac") {
        snprintf(b, sizeof(b), "%02x:%02x:%02x:%02x:%02x:%02x", x.value.mac[0], x.value.mac[1], x.value.mac[2],
                 x.value.mac[3], x.value.mac[4], x.value.mac[5]);
        return b;
    }
    return "?";
}

struct Apis {
    sai_switch_api_t* sw; sai_port_api_t* port; sai_hostif_api_t* hostif; sai_policer_api_t* policer;
    sai_vlan_api_t* vlan; sai_router_interface_api_t* rif; sai_route_api_t* route; sai_neighbor_api_t* neighbor;
    sai_next_hop_api_t* nexthop; sai_bridge_api_t* bridge; sai_virtual_router_api_t* vr;
};
static Apis A;

static sai_object_type_t type_of(const std::string& s)
{
    int64_t v;
    if (!lookup(k_types, s, v)) throw Fail{"unknown object type " + s};
    return (sai_object_type_t)v;
}

static sai_status_t do_create(sai_object_type_t t, sai_object_id_t* o, uint32_t n, const sai_attribute_t* a)
{
    switch (t) {
    case SAI_OBJECT_TYPE_HOSTIF: return A.hostif->create_hostif(o, g_switch, n, a);
    case SAI_OBJECT_TYPE_HOSTIF_TRAP_GROUP: return A.hostif->create_hostif_trap_group(o, g_switch, n, a);
    case SAI_OBJECT_TYPE_HOSTIF_TRAP: return A.hostif->create_hostif_trap(o, g_switch, n, a);
    case SAI_OBJECT_TYPE_HOSTIF_TABLE_ENTRY: return A.hostif->create_hostif_table_entry(o, g_switch, n, a);
    case SAI_OBJECT_TYPE_POLICER: return A.policer->create_policer(o, g_switch, n, a);
    case SAI_OBJECT_TYPE_VLAN: return A.vlan->create_vlan(o, g_switch, n, a);
    case SAI_OBJECT_TYPE_VLAN_MEMBER: return A.vlan->create_vlan_member(o, g_switch, n, a);
    case SAI_OBJECT_TYPE_ROUTER_INTERFACE: return A.rif->create_router_interface(o, g_switch, n, a);
    case SAI_OBJECT_TYPE_NEXT_HOP: return A.nexthop->create_next_hop(o, g_switch, n, a);
    case SAI_OBJECT_TYPE_VIRTUAL_ROUTER: return A.vr->create_virtual_router(o, g_switch, n, a);
    default: return SAI_STATUS_NOT_SUPPORTED;
    }
}
static sai_status_t do_remove(sai_object_id_t o)
{
    switch (sai_object_type_query(o)) {
    case SAI_OBJECT_TYPE_HOSTIF: return A.hostif->remove_hostif(o);
    case SAI_OBJECT_TYPE_HOSTIF_TRAP_GROUP: return A.hostif->remove_hostif_trap_group(o);
    case SAI_OBJECT_TYPE_HOSTIF_TRAP: return A.hostif->remove_hostif_trap(o);
    case SAI_OBJECT_TYPE_HOSTIF_TABLE_ENTRY: return A.hostif->remove_hostif_table_entry(o);
    case SAI_OBJECT_TYPE_POLICER: return A.policer->remove_policer(o);
    case SAI_OBJECT_TYPE_VLAN: return A.vlan->remove_vlan(o);
    case SAI_OBJECT_TYPE_VLAN_MEMBER: return A.vlan->remove_vlan_member(o);
    case SAI_OBJECT_TYPE_ROUTER_INTERFACE: return A.rif->remove_router_interface(o);
    case SAI_OBJECT_TYPE_NEXT_HOP: return A.nexthop->remove_next_hop(o);
    case SAI_OBJECT_TYPE_VIRTUAL_ROUTER: return A.vr->remove_virtual_router(o);
    default: return SAI_STATUS_NOT_SUPPORTED;
    }
}
static sai_status_t do_set(sai_object_id_t o, const sai_attribute_t* a)
{
    switch (sai_object_type_query(o)) {
    case SAI_OBJECT_TYPE_SWITCH: return A.sw->set_switch_attribute(o, a);
    case SAI_OBJECT_TYPE_PORT: return A.port->set_port_attribute(o, a);
    case SAI_OBJECT_TYPE_HOSTIF: return A.hostif->set_hostif_attribute(o, a);
    case SAI_OBJECT_TYPE_HOSTIF_TRAP_GROUP: return A.hostif->set_hostif_trap_group_attribute(o, a);
    case SAI_OBJECT_TYPE_HOSTIF_TRAP: return A.hostif->set_hostif_trap_attribute(o, a);
    case SAI_OBJECT_TYPE_POLICER: return A.policer->set_policer_attribute(o, a);
    case SAI_OBJECT_TYPE_ROUTER_INTERFACE: return A.rif->set_router_interface_attribute(o, a);
    default: return SAI_STATUS_NOT_SUPPORTED;
    }
}
static sai_status_t do_get(sai_object_id_t o, uint32_t n, sai_attribute_t* a)
{
    switch (sai_object_type_query(o)) {
    case SAI_OBJECT_TYPE_SWITCH: return A.sw->get_switch_attribute(o, n, a);
    case SAI_OBJECT_TYPE_PORT: return A.port->get_port_attribute(o, n, a);
    case SAI_OBJECT_TYPE_HOSTIF: return A.hostif->get_hostif_attribute(o, n, a);
    case SAI_OBJECT_TYPE_HOSTIF_TRAP_GROUP: return A.hostif->get_hostif_trap_group_attribute(o, n, a);
    case SAI_OBJECT_TYPE_HOSTIF_TRAP: return A.hostif->get_hostif_trap_attribute(o, n, a);
    case SAI_OBJECT_TYPE_POLICER: return A.policer->get_policer_attribute(o, n, a);
    case SAI_OBJECT_TYPE_BRIDGE_PORT: return A.bridge->get_bridge_port_attribute(o, n, a);
    case SAI_OBJECT_TYPE_VLAN: return A.vlan->get_vlan_attribute(o, n, a);
    case SAI_OBJECT_TYPE_ROUTER_INTERFACE: return A.rif->get_router_interface_attribute(o, n, a);
    default: return SAI_STATUS_NOT_SUPPORTED;
    }
}

static int g_failed = 0;
static void run(const json& steps, int depth);

static void step(const json& s, size_t idx, int depth)
{
    std::string op = s.at("op").get<std::string>();
    std::string desc = s.value("desc", std::string());
    std::string tag = std::to_string(depth) + "." + std::to_string(idx) + " " + op + (desc.empty() ? "" : " " + desc);
    if (op == "repeat") {
        int n = s.at("n").get<int>();
        for (int i = 0; i < n; i++) run(s.at("steps"), depth + 1);
        return;
    }
    if (op == "foreach") {   // {"op":"foreach","list":"ports","as":"p","steps":[...]}: $p = each element
        auto it = g_lists.find(s.at("list").get<std::string>());
        if (it == g_lists.end()) throw Fail{"no list " + s.at("list").get<std::string>()};
        size_t n = s.value("max", it->second.size());
        for (size_t i = 0; i < it->second.size() && i < n; i++) {
            g_obj[s.at("as").get<std::string>()] = it->second[i];
            run(s.at("steps"), depth + 1);
        }
        return;
    }
    if (op == "sleep") {
        std::this_thread::sleep_for(std::chrono::milliseconds(s.at("ms").get<int>()));
        return;
    }
    if (op == "print") { printf("%s\n", s.at("text").get<std::string>().c_str()); return; }
    if (op == "exec") {   // a shell command while the switch is up (frame tests); its output goes to our stdout
        std::string cmd = s.at("cmd").get<std::string>();
        mark("exec " + cmd);
        fflush(stdout);
        int rc = std::system(cmd.c_str());
        printf("EXEC rc=%d %s\n", rc, cmd.c_str());
        fflush(stdout);
        if (rc != 0 && !s.value("may_fail", false)) g_failed++;
        return;
    }
    if (op == "diag") {   // one command on the SAI's diag socket (SAI_OPENBCM_DIAG_SOCKET)
        auto it = g_profile.find("SAI_OPENBCM_DIAG_SOCKET");
        if (it == g_profile.end()) throw Fail{"diag: no SAI_OPENBCM_DIAG_SOCKET in the profile"};
        int fd = socket(AF_UNIX, SOCK_STREAM, 0);
        sockaddr_un sa;
        std::memset(&sa, 0, sizeof(sa));
        sa.sun_family = AF_UNIX;
        std::snprintf(sa.sun_path, sizeof(sa.sun_path), "%s", it->second.c_str());
        if (fd < 0 || connect(fd, (sockaddr*)&sa, sizeof(sa)) != 0) throw Fail{"diag: cannot connect " + it->second};
        std::string cmd = s.at("cmd").get<std::string>() + "\n";
        (void)!write(fd, cmd.data(), cmd.size());
        shutdown(fd, SHUT_WR);
        char b[4096];
        ssize_t n;
        printf("DIAG %s", cmd.c_str());
        while ((n = read(fd, b, sizeof(b))) > 0) fwrite(b, 1, (size_t)n, stdout);
        close(fd);
        fflush(stdout);
        return;
    }
    Storage st;
    std::vector<sai_attribute_t> attrs;
    sai_object_type_t t = SAI_OBJECT_TYPE_NULL;
    sai_object_id_t target = SAI_NULL_OBJECT_ID;
    if (s.contains("type")) t = type_of(s["type"].get<std::string>());
    if (s.contains("obj")) {
        target = oid(s["obj"]);
        t = sai_object_type_query(target);
        if (t == SAI_OBJECT_TYPE_NULL && s["obj"] == "switch") t = SAI_OBJECT_TYPE_SWITCH;
    }
    if (op == "create_switch") t = SAI_OBJECT_TYPE_SWITCH;
    if (op == "create_route" || op == "remove_route" || op == "set_route") t = SAI_OBJECT_TYPE_ROUTE_ENTRY;
    if (op == "create_neighbor" || op == "remove_neighbor") t = SAI_OBJECT_TYPE_NEIGHBOR_ENTRY;
    for (auto& a : s.value("attrs", json::array())) attrs.push_back(make_attr(t, a, st));
    if (s.contains("attr")) attrs.push_back(make_attr(t, s["attr"], st));

    sai_status_t r = SAI_STATUS_SUCCESS;
    std::string out;
    mark("step " + tag);
    double t0 = now_ms();
    if (op == "create_switch") {
        r = A.sw->create_switch(&g_switch, (uint32_t)attrs.size(), attrs.data());
    } else if (op == "remove_switch") {
        r = A.sw->remove_switch(g_switch);
    } else if (op == "create") {
        sai_object_id_t o = SAI_NULL_OBJECT_ID;
        r = do_create(t, &o, (uint32_t)attrs.size(), attrs.data());
        if (s.contains("as")) g_obj[s["as"].get<std::string>()] = o;
        char b[64];
        snprintf(b, sizeof(b), "0x%llx", (unsigned long long)o);
        out = b;
    } else if (op == "remove") {
        r = do_remove(target);
    } else if (op == "set") {
        r = do_set(target, attrs.data());
    } else if (op == "get") {
        r = do_get(target, (uint32_t)attrs.size(), attrs.data());
        auto list = s.value("attrs", json::array());
        if (s.contains("attr")) list.push_back(s["attr"]);
        for (size_t i = 0; r == SAI_STATUS_SUCCESS && i < attrs.size(); i++) {
            out += (i ? " " : "") + show(t, attrs[i], list[i][1].get<std::string>());
            if (list[i][1] == "oidlist" && s.contains("as"))
                g_lists[s["as"].get<std::string>()].assign(attrs[i].value.objlist.list,
                                                           attrs[i].value.objlist.list + attrs[i].value.objlist.count);
            if (list[i][1] == "oid" && s.contains("as")) g_obj[s["as"].get<std::string>()] = attrs[i].value.oid;
        }
    } else if (op == "stats") {
        std::vector<sai_stat_id_t> ids;
        for (auto& x : s.at("ids")) {
            int64_t v;
            if (!x.is_string() || !lookup(k_port_stats, x.get<std::string>(), v)) v = num(x);
            ids.push_back((sai_stat_id_t)v);
        }
        std::vector<uint64_t> vals(ids.size());
        r = A.port->get_port_stats(target, (uint32_t)ids.size(), ids.data(), vals.data());
        for (size_t i = 0; r == SAI_STATUS_SUCCESS && i < vals.size(); i++) out += (i ? " " : "") + std::to_string(vals[i]);
    } else if (op == "create_route" || op == "remove_route" || op == "set_route") {
        sai_route_entry_t e;
        std::memset(&e, 0, sizeof(e));
        e.switch_id = g_switch;
        e.vr_id = oid(s.at("vr"));
        e.destination = parse_prefix(s.at("prefix").get<std::string>());
        if (op == "create_route") r = A.route->create_route_entry(&e, (uint32_t)attrs.size(), attrs.data());
        else if (op == "set_route") r = A.route->set_route_entry_attribute(&e, attrs.data());
        else r = A.route->remove_route_entry(&e);
    } else if (op == "create_neighbor" || op == "remove_neighbor") {
        sai_neighbor_entry_t e;
        std::memset(&e, 0, sizeof(e));
        e.switch_id = g_switch;
        e.rif_id = oid(s.at("rif"));
        e.ip_address = parse_ip(s.at("ip").get<std::string>());
        if (op == "create_neighbor") r = A.neighbor->create_neighbor_entry(&e, (uint32_t)attrs.size(), attrs.data());
        else r = A.neighbor->remove_neighbor_entry(&e);
    } else {
        throw Fail{"unknown op " + op};
    }
    double dt = now_ms() - t0;
    char b[64];
    snprintf(b, sizeof(b), " status=%d ms=%.3f", (int)r, dt);
    mark("end " + tag + b);
    printf("STEP %-40s %d %10.3f ms %s\n", tag.c_str(), (int)r, dt, out.c_str());
    fflush(stdout);
    bool expect_fail = s.value("may_fail", false);
    if (r != SAI_STATUS_SUCCESS && !expect_fail) g_failed++;
}

static void run(const json& steps, int depth)
{
    for (size_t i = 0; i < steps.size(); i++) step(steps[i], i, depth);
}

int main(int argc, char** argv)
{
    if (argc < 3) {
        fprintf(stderr, "usage: %s PROFILE SCRIPT.json\n", argv[0]);
        return 2;
    }
    std::ifstream pf(argv[1]);
    std::string line;
    while (std::getline(pf, line)) {
        auto eq = line.find('=');
        if (line.empty() || line[0] == '#' || eq == std::string::npos) continue;
        g_profile[line.substr(0, eq)] = line.substr(eq + 1);
    }
    json script;
    try {
        std::ifstream sf(argv[2]);
        sf >> script;
    } catch (std::exception& e) {
        fprintf(stderr, "%s: %s\n", argv[2], e.what());
        return 2;
    }
    if (const char* f = getenv("IFCS_TRACE_FILE")) g_mark = open(f, O_WRONLY | O_CREAT | O_APPEND | O_CLOEXEC, 0644);
    mark("script " + script.value("name", std::string(argv[2])));
    static sai_service_method_table_t svc = {prof_get, prof_next};
    sai_status_t r = sai_api_initialize(0, &svc);
    if (r != SAI_STATUS_SUCCESS) { fprintf(stderr, "sai_api_initialize %d\n", (int)r); return 1; }
    sai_api_query(SAI_API_SWITCH, (void**)&A.sw);
    sai_api_query(SAI_API_PORT, (void**)&A.port);
    sai_api_query(SAI_API_HOSTIF, (void**)&A.hostif);
    sai_api_query(SAI_API_POLICER, (void**)&A.policer);
    sai_api_query(SAI_API_VLAN, (void**)&A.vlan);
    sai_api_query(SAI_API_ROUTER_INTERFACE, (void**)&A.rif);
    sai_api_query(SAI_API_ROUTE, (void**)&A.route);
    sai_api_query(SAI_API_NEIGHBOR, (void**)&A.neighbor);
    sai_api_query(SAI_API_NEXT_HOP, (void**)&A.nexthop);
    sai_api_query(SAI_API_BRIDGE, (void**)&A.bridge);
    sai_api_query(SAI_API_VIRTUAL_ROUTER, (void**)&A.vr);
    int rc = 0;
    try {
        run(script.at("steps"), 0);
    } catch (Fail& f) {
        fprintf(stderr, "script error: %s\n", f.what.c_str());
        rc = 2;
    } catch (std::exception& e) {
        fprintf(stderr, "script error: %s\n", e.what());
        rc = 2;
    }
    mark("done failed=" + std::to_string(g_failed));
    sai_api_uninitialize();
    printf("sai-script: %d failed steps\n", g_failed);
    return rc ? rc : g_failed ? 1 : 0;
}
