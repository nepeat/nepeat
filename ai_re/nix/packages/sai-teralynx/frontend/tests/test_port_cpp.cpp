// SPDX-License-Identifier: Apache-2.0
// tests/test_port.h over the C++ sai-openbcm internals: the library's context (ob::ctx()) and
// its fake backend (ob::FakeBackend), also when a RecordingBackend wraps it
// (tests/frontend_driver.cpp). The only test source besides test_units.cpp that includes
// library-internal headers.
#include "test_port.h"

#include "ext_macsec.h"
#include "ext_phy.h"
#include "fake_backend.h"
#include "recording_backend.h"
#include "sai_internal.h"

#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <mutex>

using namespace ob;

namespace tp {
namespace {

FakeBackend& fake()
{
    Backend* b = ctx().be.get();
    if (auto* r = dynamic_cast<RecordingBackend*>(b)) b = &r->inner();
    auto* f = dynamic_cast<FakeBackend*>(b);
    if (!f) {
        std::fprintf(stderr, "test_port: no fake backend (is a switch with SAI_OPENBCM_BACKEND=fake created?)\n");
        std::abort();
    }
    return *f;
}

Ip ip_of(const IpAddr& a)
{
    Ip r;
    r.v6 = a.v6;
    std::memcpy(r.addr, a.addr, 16);
    return r;
}

int sai_fec(Fec f) { return f == Fec::Rs ? SAI_PORT_FEC_MODE_RS : f == Fec::Fc ? SAI_PORT_FEC_MODE_FC : SAI_PORT_FEC_MODE_NONE; }
int sai_lb(Loopback l) { return l == Loopback::Mac ? SAI_PORT_LOOPBACK_MODE_MAC : l == Loopback::Phy ? SAI_PORT_LOOPBACK_MODE_PHY : SAI_PORT_LOOPBACK_MODE_NONE; }
int tp_learn(ob::Learn l)
{
    switch (l) {
    case ob::Learn::Disable: return LEARN_DISABLE;
    case ob::Learn::Drop: return LEARN_DROP;
    case ob::Learn::Cpu: return LEARN_CPU;
    default: return LEARN_HW;
    }
}

Port port_of(const FakeBackend::Port& p)
{
    Port r;
    r.hw = p.info.hw;
    r.admin = p.admin;
    r.link = p.link;
    r.fec = sai_fec(p.fec);
    r.autoneg = p.autoneg;
    r.iface = p.iface;
    r.ifg = p.ifg;
    r.quirk_calls = p.quirk_calls;
    r.loopback = sai_lb(p.lb);
    r.pvid = p.pvid;
    r.frame_max = p.frame_max;
    r.learn = tp_learn(p.learn);
    return r;
}

// SAI_PORT_STAT_IF_* -> the backend's PortStat (as sai_port.cpp maps them)
bool port_stat_of(sai_stat_id_t id, PortStat& s)
{
    static const std::pair<sai_stat_id_t, PortStat> m[] = {
        {SAI_PORT_STAT_IF_IN_OCTETS, PortStat::InOctets}, {SAI_PORT_STAT_IF_IN_UCAST_PKTS, PortStat::InUcastPkts},
        {SAI_PORT_STAT_IF_IN_NON_UCAST_PKTS, PortStat::InNonUcastPkts}, {SAI_PORT_STAT_IF_IN_DISCARDS, PortStat::InDiscards},
        {SAI_PORT_STAT_IF_IN_ERRORS, PortStat::InErrors}, {SAI_PORT_STAT_IF_IN_UNKNOWN_PROTOS, PortStat::InUnknownProtos},
        {SAI_PORT_STAT_IF_IN_BROADCAST_PKTS, PortStat::InBroadcastPkts}, {SAI_PORT_STAT_IF_IN_MULTICAST_PKTS, PortStat::InMulticastPkts},
        {SAI_PORT_STAT_IF_OUT_OCTETS, PortStat::OutOctets}, {SAI_PORT_STAT_IF_OUT_UCAST_PKTS, PortStat::OutUcastPkts},
        {SAI_PORT_STAT_IF_OUT_NON_UCAST_PKTS, PortStat::OutNonUcastPkts}, {SAI_PORT_STAT_IF_OUT_DISCARDS, PortStat::OutDiscards},
        {SAI_PORT_STAT_IF_OUT_ERRORS, PortStat::OutErrors}, {SAI_PORT_STAT_IF_OUT_BROADCAST_PKTS, PortStat::OutBroadcastPkts},
        {SAI_PORT_STAT_IF_OUT_MULTICAST_PKTS, PortStat::OutMulticastPkts},
    };
    for (auto& e : m)
        if (e.first == id) { s = e.second; return true; }
    return false;
}

bool qos_kind_of(int32_t sai_type, QosMapKind& k)
{
    static const std::pair<int32_t, QosMapKind> m[] = {
        {SAI_QOS_MAP_TYPE_DSCP_TO_TC, QosMapKind::DscpToTc}, {SAI_QOS_MAP_TYPE_DOT1P_TO_TC, QosMapKind::Dot1pToTc},
        {SAI_QOS_MAP_TYPE_TC_TO_QUEUE, QosMapKind::TcToQueue}, {SAI_QOS_MAP_TYPE_TC_TO_PRIORITY_GROUP, QosMapKind::TcToPg},
        {SAI_QOS_MAP_TYPE_PFC_PRIORITY_TO_QUEUE, QosMapKind::PfcPrioToQueue},
        {SAI_QOS_MAP_TYPE_PFC_PRIORITY_TO_PRIORITY_GROUP, QosMapKind::PfcPrioToPg},
        {SAI_QOS_MAP_TYPE_TC_AND_COLOR_TO_DSCP, QosMapKind::TcColorToDscp},
        {SAI_QOS_MAP_TYPE_TC_AND_COLOR_TO_DOT1P, QosMapKind::TcColorToDot1p},
        {SAI_QOS_MAP_TYPE_DSCP_TO_COLOR, QosMapKind::DscpToColor}, {SAI_QOS_MAP_TYPE_DOT1P_TO_COLOR, QosMapKind::Dot1pToColor},
    };
    for (auto& e : m)
        if (e.first == sai_type) { k = e.second; return true; }
    return false;
}
int32_t sai_qos_type(QosMapKind k)
{
    for (int32_t t = 0; t < 64; t++) {
        QosMapKind x;
        if (qos_kind_of(t, x) && x == k) return t;
    }
    return -1;
}

bool storm_kind_of(sai_attr_id_t a, StormKind& k)
{
    if (a == SAI_PORT_ATTR_BROADCAST_STORM_CONTROL_POLICER_ID) k = StormKind::Broadcast;
    else if (a == SAI_PORT_ATTR_MULTICAST_STORM_CONTROL_POLICER_ID) k = StormKind::Multicast;
    else if (a == SAI_PORT_ATTR_FLOOD_STORM_CONTROL_POLICER_ID) k = StormKind::UnknownUnicast;
    else return false;
    return true;
}
bool flood_kind_of(sai_attr_id_t a, FloodKind& k)
{
    if (a == SAI_VLAN_ATTR_UNKNOWN_UNICAST_FLOOD_CONTROL_TYPE) k = FloodKind::UnknownUnicast;
    else if (a == SAI_VLAN_ATTR_UNKNOWN_MULTICAST_FLOOD_CONTROL_TYPE) k = FloodKind::UnknownMulticast;
    else if (a == SAI_VLAN_ATTR_BROADCAST_FLOOD_CONTROL_TYPE) k = FloodKind::Broadcast;
    else return false;
    return true;
}

template <typename M, typename K> auto find_or_null(M& m, const K& k) -> decltype(&m.begin()->second)
{
    auto it = m.find(k);
    return it == m.end() ? nullptr : &it->second;
}

FpEntry entry_of(int eid, const FakeBackend::FpEntry& e)
{
    FpEntry r;
    r.eid = eid;
    r.gid = e.gid;
    const FpRule& x = e.rule;
    r.priority = x.priority;
    r.present = x.m.present;
    r.in_ports = x.m.ports;
    r.ether = x.m.ether;
    r.ip_type = x.m.ip_type;
    r.l4_src = x.m.l4_src;
    r.l4_dst = x.m.l4_dst;
    r.dst = ip_of(x.m.dst);
    r.dst_mask = ip_of(x.m.dst_mask);
    r.drop = x.drop;
    r.drop_cancel = x.drop_cancel;
    r.copy_cpu = x.copy_cpu;
    r.counter = x.counter;
    r.policer = x.policer;
    r.red_drop = x.red_drop;
    r.cpu_copy_only = x.cpu_copy_only;
    r.mirror_ingress = x.mirror_ingress;
    r.mirror_egress = x.mirror_egress;
    return r;
}

std::vector<uint8_t> macv(const uint8_t m[6]) { return std::vector<uint8_t>(m, m + 6); }

} // namespace

// ================================================================== library and OID layout
bool has_hardware_backend()
{
    // Not destroyed, as before the test port: a hardware backend's destructor may touch the SDK.
    return make_openbcm_backend() != nullptr;
}
bool switch_created()
{
    std::lock_guard<std::recursive_mutex> g(ctx().mu);
    return ctx().switch_created;
}
sai_object_id_t make_oid(sai_object_type_t type, uint64_t index, uint32_t sw) { return ob::make_oid(type, index, sw); }
sai_object_type_t oid_type(sai_object_id_t oid) { return ob::oid_type(oid); }
uint64_t oid_index(sai_object_id_t oid) { return ob::oid_index(oid); }
uint32_t oid_switch(sai_object_id_t oid) { return ob::oid_switch(oid); }
uint64_t oid_index_mask() { return ob::OID_INDEX_MASK; }

std::vector<AttrCap> attr_caps()
{
    std::vector<AttrCap> v;
    for (auto& c : ob::attr_caps()) v.push_back({c.ot, c.id, c.create, c.set, c.get});
    return v;
}

// ======================================================= the library's view of its objects
int backend_id(sai_object_id_t oid)
{
    auto& c = ctx();
    std::lock_guard<std::recursive_mutex> g(c.mu);
    switch (ob::oid_type(oid)) {
    case SAI_OBJECT_TYPE_PORT: { auto* p = find_or_null(c.ports, oid); return p ? p->hw : -1; }
    case SAI_OBJECT_TYPE_LAG: { auto* p = find_or_null(c.lags, oid); return p ? p->tid : -1; }
    case SAI_OBJECT_TYPE_VIRTUAL_ROUTER: { auto* p = find_or_null(c.vrs, oid); return p ? p->vrf : -1; }
    case SAI_OBJECT_TYPE_ROUTER_INTERFACE: { auto* p = find_or_null(c.rifs, oid); return p ? p->intf : -1; }
    case SAI_OBJECT_TYPE_POLICER: { auto* p = find_or_null(c.policers, oid); return p ? p->pid : -1; }
    case SAI_OBJECT_TYPE_ACL_TABLE: { auto* p = find_or_null(c.acl_tables, oid); return p ? p->gid : -1; }
    case SAI_OBJECT_TYPE_ACL_ENTRY: { auto* p = find_or_null(c.acl_entries, oid); return p ? p->eid : -1; }
    case SAI_OBJECT_TYPE_QOS_MAP: { auto* p = find_or_null(c.qos_maps, oid); return p ? p->id : -1; }
    case SAI_OBJECT_TYPE_WRED: { auto* p = find_or_null(c.wreds, oid); return p ? p->id : -1; }
    case SAI_OBJECT_TYPE_MIRROR_SESSION: { auto* p = find_or_null(c.mirrors, oid); return p ? p->id : -1; }
    case SAI_OBJECT_TYPE_SAMPLEPACKET: { auto* p = find_or_null(c.samplepackets, oid); return p ? p->id : -1; }
    case SAI_OBJECT_TYPE_HOSTIF: {
        auto* p = find_or_null(c.hostifs, oid);
        if (!p) return -1;
        return p->type == SAI_HOSTIF_TYPE_GENETLINK ? p->genl : p->netif;
    }
    default: return -1;
    }
}
Rif rif(sai_object_id_t oid)
{
    auto& c = ctx();
    std::lock_guard<std::recursive_mutex> g(c.mu);
    Rif r;
    if (auto* p = find_or_null(c.rifs, oid)) { r.intf = p->intf; r.vrf = p->vrf; r.vid = p->vid; }
    return r;
}
sai_object_id_t port_lag(sai_object_id_t port)
{
    auto& c = ctx();
    std::lock_guard<std::recursive_mutex> g(c.mu);
    auto* p = find_or_null(c.ports, port);
    return p ? p->lag : SAI_NULL_OBJECT_ID;
}
bool trap_programmed(sai_object_id_t trap)
{
    auto& c = ctx();
    std::lock_guard<std::recursive_mutex> g(c.mu);
    auto* t = find_or_null(c.traps, trap);
    return t && t->programmed;
}
size_t object_count(sai_object_type_t type)
{
    auto& c = ctx();
    std::lock_guard<std::recursive_mutex> g(c.mu);
    switch (type) {
    case SAI_OBJECT_TYPE_VIRTUAL_ROUTER: return c.vrs.size();
    case SAI_OBJECT_TYPE_ACL_TABLE: return c.acl_tables.size();
    case SAI_OBJECT_TYPE_ACL_ENTRY: return c.acl_entries.size();
    default:
        std::fprintf(stderr, "test_port: object_count(%d) not implemented\n", (int)type);
        std::abort();
    }
}
size_t acl_binding_count()
{
    std::lock_guard<std::recursive_mutex> g(ctx().mu);
    return ctx().acl_bind.size();
}
int copp_group()
{
    std::lock_guard<std::recursive_mutex> g(ctx().mu);
    return ctx().copp_gid;
}
size_t copp_rule_count()
{
    std::lock_guard<std::recursive_mutex> g(ctx().mu);
    return ctx().copp.size();
}
int neighbor_egress(sai_object_id_t rif, const sai_ip_address_t& ip)
{
    auto& c = ctx();
    std::lock_guard<std::recursive_mutex> g(c.mu);
    auto it = c.neighbors.find(NeighborKey{rif, to_ip(ip)});
    return it == c.neighbors.end() ? -1 : it->second.egress;
}
uint64_t events_delivered() { return ctx().events_delivered.load(); }

// ======================================================== library services (diag, logging)
void diag_start(const std::string& socket_path)
{
    BackendConfig cfg;
    if (!socket_path.empty()) cfg.profile["SAI_OPENBCM_DIAG_SOCKET"] = socket_path;
    ob::diag_start(cfg, ctx().be.get());
}
void diag_stop() { ob::diag_stop(); }
void log_to_stderr(bool on) { log_configure(on); }
bool ext_phy_active() { return ob::ext_phy_active(); }
std::string ext_phy_status() { return ob::ext_phy_status(); }
bool ext_macsec_active() { return ob::ext_macsec_active(); }
std::string ext_macsec_status() { return ob::ext_macsec_status(); }

// ================================================================ virtual backend: hooks
void set_feature(Feature f, bool on)
{
    FakeBackend& b = fake();
    std::lock_guard<std::mutex> g(b.mu);
    if (f == FEATURE_QOS) b.qos_on = on;
    else if (f == FEATURE_MIRROR) b.mirror_on = on;
    else b.l2x_on = on;
}
void inject_link(int hw, bool up) { fake().inject_link(hw, up); }
void linkscan() { fake().linkscan(); }
void inject_fdb(bool learned, const uint8_t mac[6], uint16_t vid, int dest)
{
    FdbEvent ev;
    ev.learned = learned;
    std::memcpy(ev.mac, mac, 6);
    ev.vid = vid;
    ev.hw_port = dest;
    fake().inject_fdb(ev);
}
void set_port_stat(int hw, sai_stat_id_t id, uint64_t value)
{
    PortStat s;
    int prio = 0;
    bool tx = false;
    if (port_stat_of(id, s)) {
        fake().set_stat(hw, s, value);
    } else if (port_pfc_stat(id, prio, tx)) {
        FakeBackend& b = fake();
        std::lock_guard<std::mutex> g(b.mu);
        b.pfc_stats[std::make_pair(hw, prio * 2 + (tx ? 1 : 0))] = value;
    } else {
        std::fprintf(stderr, "test_port: set_port_stat(%u): no such backend counter\n", id);
        std::abort();
    }
}
void set_queue_stat(int hw, int queue, sai_stat_id_t id, uint64_t value)
{
    FakeBackend& b = fake();
    std::lock_guard<std::mutex> g(b.mu);
    b.queue_stats[std::make_pair(std::make_pair(hw, queue), id)] = value;
}
void set_pg_stat(int hw, int pg, sai_stat_id_t id, uint64_t value)
{
    FakeBackend& b = fake();
    std::lock_guard<std::mutex> g(b.mu);
    b.pg_stats[std::make_pair(std::make_pair(hw, pg), id)] = value;
}
void fp_hit(int eid, uint64_t packets, uint64_t bytes) { fake().fp_hit(eid, packets, bytes); }
void set_temperatures(const std::vector<int>& tenths)
{
    FakeBackend& b = fake();
    std::lock_guard<std::mutex> g(b.mu);
    b.temps = tenths;
}
void set_temperature_fail(bool fail)
{
    FakeBackend& b = fake();
    std::lock_guard<std::mutex> g(b.mu);
    b.temp_fail = fail;
}
void mdio_set(unsigned phy, unsigned dev, unsigned reg, unsigned value)
{
    FakeBackend& b = fake();
    std::lock_guard<std::mutex> g(b.mdio_mu);
    b.mdio_regs[std::make_tuple(phy, dev, reg)] = value;
}
void mdio_set_dead(unsigned phy, bool dead)
{
    FakeBackend& b = fake();
    std::lock_guard<std::mutex> g(b.mdio_mu);
    if (dead) b.mdio_dead.insert(phy);
    else b.mdio_dead.erase(phy);
}
std::vector<MdioOp> mdio_log(bool clear)
{
    FakeBackend& b = fake();
    std::lock_guard<std::mutex> g(b.mdio_mu);
    std::vector<MdioOp> v;
    for (auto& o : b.mdio_log) v.push_back({o.op, o.phy, o.dev, o.reg, o.val});
    if (clear) b.mdio_log.clear();
    return v;
}
std::vector<std::string> shell_log()
{
    FakeBackend& b = fake();
    std::lock_guard<std::mutex> g(b.mu);
    return b.shell_log;
}
void backend_host_insert(int vrf, const sai_ip_address_t& ip, int egress)
{
    FakeBackend& b = fake();
    std::lock_guard<std::mutex> g(b.mu);
    b.hosts[{vrf, to_ip(ip)}] = egress;
}
void backend_host_erase(int vrf, const sai_ip_address_t& ip)
{
    FakeBackend& b = fake();
    std::lock_guard<std::mutex> g(b.mu);
    b.hosts.erase({vrf, to_ip(ip)});
}
void backend_l2_insert(const uint8_t mac[6], uint16_t vid, int dest)
{
    FakeBackend& b = fake();
    std::lock_guard<std::mutex> g(b.mu);
    b.l2[{macv(mac), vid}] = FakeBackend::L2{dest, false, false};
}
void backend_l2_erase(const uint8_t mac[6], uint16_t vid)
{
    FakeBackend& b = fake();
    std::lock_guard<std::mutex> g(b.mu);
    b.l2.erase({macv(mac), vid});
}
int drop_egress()
{
    int d = -1;
    fake().l3_egress_drop(d);
    return d;
}
sai_status_t backend_egress_destroy(int egress) { return fake().l3_egress_destroy(egress); }
int lag_dest(int tid) { return ob::lag_dest(tid); }

// ================================================================ virtual backend: state
size_t count(Table t)
{
    FakeBackend& b = fake();
    std::lock_guard<std::mutex> g(b.mu);
    switch (t) {
    case T_STATIONS: return b.stations.size();
    case T_QOS_MAPS: return b.qmaps.size();
    case T_PORT_QOS_MAPS: return b.port_qmap.size();
    case T_QUEUE_SCHEDULERS: return b.queue_sched.size();
    case T_PORT_SCHEDULERS: return b.port_sched.size();
    case T_QUEUE_BUFFERS: return b.queue_buf.size();
    case T_PG_BUFFERS: return b.pg_buf.size();
    case T_MIRRORS: return b.mirrors.size();
    case T_PORT_MIRRORS: return b.port_mirrors.size();
    case T_SAMPLES: return b.samples.size();
    case T_PORT_SAMPLES: return b.port_samples.size();
    case T_STORM: return b.storm.size();
    case T_FLOOD: return b.flood.size();
    case T_FP_GROUPS: return b.fp_groups.size();
    case T_FP_ENTRIES: return b.fp_entries.size();
    case T_POLICERS: return b.policers.size();
    }
    return 0;
}

std::vector<Port> ports()
{
    FakeBackend& b = fake();
    std::lock_guard<std::mutex> g(b.mu);
    std::vector<Port> v;
    for (auto& kv : b.port_state) v.push_back(port_of(kv.second));
    return v;
}
Port port(int hw)
{
    FakeBackend& b = fake();
    std::lock_guard<std::mutex> g(b.mu);
    auto it = b.port_state.find(hw);
    if (it == b.port_state.end()) {
        std::fprintf(stderr, "test_port: port(%d): no such backend port\n", hw);
        std::abort();
    }
    return port_of(it->second);
}
void port_forget_quirks(int hw)
{
    FakeBackend& b = fake();
    std::lock_guard<std::mutex> g(b.mu);
    auto it = b.port_state.find(hw);
    if (it == b.port_state.end()) return;
    it->second.ifg = 0;
    it->second.iface.clear();
}
uint32_t l2_age_seconds()
{
    FakeBackend& b = fake();
    std::lock_guard<std::mutex> g(b.mu);
    return b.age_seconds;
}
bool vlan_exists(uint16_t vid)
{
    FakeBackend& b = fake();
    std::lock_guard<std::mutex> g(b.mu);
    return b.vlans.count(vid) != 0;
}
std::vector<VlanMember> vlan_members(uint16_t vid)
{
    FakeBackend& b = fake();
    std::lock_guard<std::mutex> g(b.mu);
    std::vector<VlanMember> v;
    auto it = b.vlans.find(vid);
    if (it != b.vlans.end())
        for (auto& m : it->second) v.push_back({m.first, m.second});
    return v;
}
std::vector<L2Entry> l2_entries()
{
    FakeBackend& b = fake();
    std::lock_guard<std::mutex> g(b.mu);
    std::vector<L2Entry> v;
    for (auto& kv : b.l2) {
        L2Entry e;
        std::memcpy(e.mac, kv.first.first.data(), 6);
        e.vid = kv.first.second;
        e.dest = kv.second.hw;
        e.is_static = kv.second.is_static;
        e.discard = kv.second.discard;
        v.push_back(e);
    }
    return v;
}
std::vector<Netif> netifs()
{
    FakeBackend& b = fake();
    std::lock_guard<std::mutex> g(b.mu);
    std::vector<Netif> v;
    for (auto& kv : b.netifs) {
        Netif n;
        n.id = kv.first;
        n.name = kv.second.name;
        n.hw = kv.second.hw;
        n.vid = kv.second.vid;
        std::memcpy(n.mac, kv.second.mac, 6);
        n.keep_tag = kv.second.keep_tag;
        n.carrier = kv.second.carrier;
        n.filter = kv.second.filter;
        v.push_back(n);
    }
    return v;
}
bool trap_to_cpu(int32_t sai_trap_type)
{
    TrapKind k = ob::trap_kind(sai_trap_type);   // the library's own classification
    if (k == TrapKind::Other) return false;
    FakeBackend& b = fake();
    std::lock_guard<std::mutex> g(b.mu);
    auto it = b.traps.find(k);
    return it != b.traps.end() && it->second;
}
std::vector<L3Intf> l3_intfs()
{
    FakeBackend& b = fake();
    std::lock_guard<std::mutex> g(b.mu);
    std::vector<L3Intf> v;
    for (auto& kv : b.intfs) {
        L3Intf i;
        i.id = kv.first;
        std::memcpy(i.mac, kv.second.mac, 6);
        i.vid = kv.second.vid;
        i.mtu = kv.second.mtu;
        i.vrf = kv.second.vrf;
        v.push_back(i);
    }
    return v;
}
static Egress egress_of(int id, const FakeBackend::Egress& e)
{
    Egress r;
    r.id = id;
    r.intf = e.intf;
    std::memcpy(r.mac, e.mac, 6);
    r.dest = e.hw;
    r.vid = e.vid;
    r.cpu = e.cpu;
    r.drop = e.drop;
    return r;
}
std::vector<Egress> egress_objects()
{
    FakeBackend& b = fake();
    std::lock_guard<std::mutex> g(b.mu);
    std::vector<Egress> v;
    for (auto& kv : b.egress) v.push_back(egress_of(kv.first, kv.second));
    return v;
}
bool egress(int id, Egress& out)
{
    FakeBackend& b = fake();
    std::lock_guard<std::mutex> g(b.mu);
    auto it = b.egress.find(id);
    if (it == b.egress.end()) return false;
    out = egress_of(id, it->second);
    return true;
}
std::vector<Host> hosts()
{
    FakeBackend& b = fake();
    std::lock_guard<std::mutex> g(b.mu);
    std::vector<Host> v;
    for (auto& kv : b.hosts) v.push_back({kv.first.first, ip_of(kv.first.second), kv.second});
    return v;
}
int host_egress(int vrf, const sai_ip_address_t& ip)
{
    FakeBackend& b = fake();
    std::lock_guard<std::mutex> g(b.mu);
    auto it = b.hosts.find({vrf, to_ip(ip)});
    return it == b.hosts.end() ? -1 : it->second;
}
std::vector<Route> routes()
{
    FakeBackend& b = fake();
    std::lock_guard<std::mutex> g(b.mu);
    std::vector<Route> v;
    for (auto& kv : b.routes) {
        IpAddr mask = kv.first.second.mask;
        mask.v6 = kv.first.second.addr.v6;
        v.push_back({kv.first.first, ip_of(kv.first.second.addr), ip_of(mask), kv.second});
    }
    return v;
}
int route_target(int vrf, const sai_ip_prefix_t& prefix)
{
    FakeBackend& b = fake();
    std::lock_guard<std::mutex> g(b.mu);
    auto it = b.routes.find({vrf, to_prefix(prefix)});
    return it == b.routes.end() ? -1 : it->second;
}
std::vector<Ecmp> ecmp_groups()
{
    FakeBackend& b = fake();
    std::lock_guard<std::mutex> g(b.mu);
    std::vector<Ecmp> v;
    for (auto& kv : b.ecmps) v.push_back({kv.first, kv.second});
    return v;
}
bool lag_members(int tid, std::vector<LagMember>& out)
{
    FakeBackend& b = fake();
    std::lock_guard<std::mutex> g(b.mu);
    auto it = b.lags.find(tid);
    if (it == b.lags.end()) return false;
    out.clear();
    for (auto& m : it->second) out.push_back({m.hw, m.egress_disable, m.ingress_disable});
    return true;
}
bool fp_group(int gid, FpGroup& out)
{
    FakeBackend& b = fake();
    std::lock_guard<std::mutex> g(b.mu);
    auto it = b.fp_groups.find(gid);
    if (it == b.fp_groups.end()) return false;
    out = {it->second.qset, it->second.priority};
    return true;
}
std::vector<FpEntry> fp_entries()
{
    FakeBackend& b = fake();
    std::lock_guard<std::mutex> g(b.mu);
    std::vector<FpEntry> v;
    for (auto& kv : b.fp_entries) v.push_back(entry_of(kv.first, kv.second));
    return v;
}
bool fp_entry(int eid, FpEntry& out)
{
    FakeBackend& b = fake();
    std::lock_guard<std::mutex> g(b.mu);
    auto it = b.fp_entries.find(eid);
    if (eid < 0 || it == b.fp_entries.end()) return false;
    out = entry_of(eid, it->second);
    return true;
}
bool policer(int pid, Policer& out)
{
    FakeBackend& b = fake();
    std::lock_guard<std::mutex> g(b.mu);
    auto it = b.policers.find(pid);
    if (it == b.policers.end()) return false;
    const PolicerCfg& c = it->second.cfg;
    out = {c.tr_tcm, c.packets, c.color_blind, c.cir, c.cbs, c.pir, c.pbs};
    return true;
}
bool qos_map(int id, QosMap& out)
{
    FakeBackend& b = fake();
    std::lock_guard<std::mutex> g(b.mu);
    auto it = b.qmaps.find(id);
    if (it == b.qmaps.end()) return false;
    out.type = sai_qos_type(it->second.kind);
    out.entries.clear();
    for (auto& e : it->second.e) out.entries.push_back({e.key, e.key_color, e.value, e.value_color});
    return true;
}
int port_qos_map(int hw, int32_t sai_qos_map_type)
{
    QosMapKind k;
    if (!qos_kind_of(sai_qos_map_type, k)) return -1;
    FakeBackend& b = fake();
    std::lock_guard<std::mutex> g(b.mu);
    auto it = b.port_qmap.find(std::make_pair(hw, (int)k));
    return it == b.port_qmap.end() ? -1 : it->second;
}
int port_default_tc(int hw)
{
    FakeBackend& b = fake();
    std::lock_guard<std::mutex> g(b.mu);
    auto it = b.default_tc.find(hw);
    return it == b.default_tc.end() ? -1 : it->second;
}
bool port_pfc(int hw, Pfc& out)
{
    FakeBackend& b = fake();
    std::lock_guard<std::mutex> g(b.mu);
    auto it = b.pfc.find(hw);
    if (it == b.pfc.end()) return false;
    out = {std::get<0>(it->second), std::get<1>(it->second), std::get<2>(it->second)};
    return true;
}
static Sched sched_of(const SchedCfg& s) { return {s.strict, s.weight, s.packets, s.min_rate, s.min_burst, s.max_rate, s.max_burst}; }
bool queue_scheduler(int hw, int queue, Sched& out)
{
    FakeBackend& b = fake();
    std::lock_guard<std::mutex> g(b.mu);
    auto it = b.queue_sched.find(std::make_pair(hw, queue));
    if (it == b.queue_sched.end()) return false;
    out = sched_of(it->second);
    return true;
}
bool port_scheduler(int hw, Sched& out)
{
    FakeBackend& b = fake();
    std::lock_guard<std::mutex> g(b.mu);
    auto it = b.port_sched.find(hw);
    if (it == b.port_sched.end()) return false;
    out = sched_of(it->second);
    return true;
}
bool wred(int id, Wred& out)
{
    FakeBackend& b = fake();
    std::lock_guard<std::mutex> g(b.mu);
    auto it = b.wreds.find(id);
    if (it == b.wreds.end()) return false;
    const WredCfg& w = it->second;
    for (int i = 0; i < 3; i++) {
        out.enable[i] = w.enable[i];
        out.min[i] = w.min[i];
        out.max[i] = w.max[i];
        out.prob[i] = w.prob[i];
    }
    out.weight = w.weight;
    out.ecn = w.ecn;
    return true;
}
int queue_wred(int hw, int queue)
{
    FakeBackend& b = fake();
    std::lock_guard<std::mutex> g(b.mu);
    auto it = b.queue_wred.find(std::make_pair(hw, queue));
    return it == b.queue_wred.end() ? -1 : it->second;
}
static Buffer buffer_of(const BufferCfg& c)
{
    return {c.ingress, c.dynamic, c.dynamic_th, c.static_th, c.reserved, c.xoff, c.xon, c.xon_offset, c.pool_size, c.pool_xoff};
}
bool queue_buffer(int hw, int queue, Buffer& out)
{
    FakeBackend& b = fake();
    std::lock_guard<std::mutex> g(b.mu);
    auto it = b.queue_buf.find(std::make_pair(hw, queue));
    if (it == b.queue_buf.end()) return false;
    out = buffer_of(it->second);
    return true;
}
bool pg_buffer(int hw, int pg, Buffer& out)
{
    FakeBackend& b = fake();
    std::lock_guard<std::mutex> g(b.mu);
    auto it = b.pg_buf.find(std::make_pair(hw, pg));
    if (it == b.pg_buf.end()) return false;
    out = buffer_of(it->second);
    return true;
}
bool mirror(int id, Mirror& out)
{
    FakeBackend& b = fake();
    std::lock_guard<std::mutex> g(b.mu);
    auto it = b.mirrors.find(id);
    if (it == b.mirrors.end()) return false;
    const MirrorCfg& m = it->second;
    out.erspan = m.erspan;
    out.monitor = m.monitor;
    out.truncate = m.truncate;
    out.sample_rate = m.sample_rate;
    out.tos = m.tos;
    out.ttl = m.ttl;
    out.src = ip_of(m.src);
    out.dst = ip_of(m.dst);
    std::memcpy(out.smac, m.smac, 6);
    std::memcpy(out.dmac, m.dmac, 6);
    out.gre_type = m.gre_type;
    out.vlan = m.vlan;
    out.vlan_tpid = m.vlan_tpid;
    out.vlan_id = m.vlan_id;
    out.vlan_pri = m.vlan_pri;
    out.vlan_cfi = m.vlan_cfi;
    return true;
}
std::vector<int> port_mirrors(int hw, bool ingress)
{
    FakeBackend& b = fake();
    std::lock_guard<std::mutex> g(b.mu);
    auto it = b.port_mirrors.find(std::make_pair(hw, ingress));
    return it == b.port_mirrors.end() ? std::vector<int>() : it->second;
}
uint32_t sample_rate(int id)
{
    FakeBackend& b = fake();
    std::lock_guard<std::mutex> g(b.mu);
    auto it = b.samples.find(id);
    return it == b.samples.end() ? 0 : it->second;
}
int port_sample(int hw, bool ingress)
{
    FakeBackend& b = fake();
    std::lock_guard<std::mutex> g(b.mu);
    auto it = b.port_samples.find(std::make_pair(hw, ingress));
    return it == b.port_samples.end() ? -1 : it->second;
}
bool genetlink(int id, Genl& out)
{
    FakeBackend& b = fake();
    std::lock_guard<std::mutex> g(b.mu);
    auto it = b.genls.find(id);
    if (it == b.genls.end()) return false;
    out.name = it->second.name;
    out.mcgrp = it->second.mcgrp;
    out.traps.assign(it->second.traps.begin(), it->second.traps.end());
    return true;
}
bool hash(bool lag, Hash& out)
{
    FakeBackend& b = fake();
    std::lock_guard<std::mutex> g(b.mu);
    auto it = b.hash.find(lag);
    if (it == b.hash.end()) return false;
    out.fields = it->second.first;
    out.seed = it->second.second;
    return true;
}
int port_storm(int hw, sai_attr_id_t kind)
{
    StormKind k;
    if (!storm_kind_of(kind, k)) return -1;
    FakeBackend& b = fake();
    std::lock_guard<std::mutex> g(b.mu);
    auto it = b.storm.find(std::make_pair(hw, (int)k));
    return it == b.storm.end() ? -1 : it->second;
}
int vlan_flood(uint16_t vid, sai_attr_id_t kind)
{
    FloodKind k;
    if (!flood_kind_of(kind, k)) return -1;
    FakeBackend& b = fake();
    std::lock_guard<std::mutex> g(b.mu);
    auto it = b.flood.find(std::make_pair(vid, (int)k));
    return it == b.flood.end() ? -1 : (it->second ? 1 : 0);
}

} // namespace tp
