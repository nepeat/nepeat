// SPDX-License-Identifier: Apache-2.0
// Host interface (tl_hostif.h).
#include "tl_hostif.h"

#include "tl_acl.h"
#include "tl_sflow.h"

#include <cerrno>
#include <cstdio>

#include <net/if.h>
#include <net/if_arp.h>
#include <sys/ioctl.h>
#include <sys/socket.h>
#include <unistd.h>

namespace ob {
namespace tlb {

static std::string pad4(uint32_t v)
{
    char b[16];
    std::snprintf(b, sizeof(b), "%04u", v);
    return b;
}

static uint32_t tag_action(bool keep_tag)
{
    return keep_tag ? IFCS_NETDEV_VLAN_TAG_ACTION_VLAN_TAG_ORIGINAL : IFCS_NETDEV_VLAN_TAG_ACTION_VLAN_TAG_STRIP;
}

// create (default): ifcs_netdev_create with INTF_NAME, DEVPORTNUM, MACADDR,
// VLAN_TAG; ipd then makes the Linux netdev, RX is demultiplexed by source
// sysport, TX goes straight to that sysport (PIPELINE_BYPASS).
// adopt: the init yaml has netdev auto_create (vendor -k mode); the SDK's
// inno_0_<sysport> netdev is renamed and given the MAC, as knet.py does.
// Carrier: libifcs drives the netdev link state from its link scan (seen
// on the vendor stack, I for netdevs we create); there is no IFCS call
// for it, so netif_carrier_set only records the wish.
sai_status_t HostifArea::netif_create(const std::string& ifname, int hw, uint16_t, const uint8_t mac[6], bool keep_tag,
                                      int& netif_id, int& filter_id)
{
    Guard g(c_.mu);
    if (!c_.front(hw)) return SAI_STATUS_INVALID_PORT_NUMBER;
    if (ifname.empty() || ifname.size() > 15) return SAI_STATUS_INVALID_PARAMETER;
    system_traps_init();
    for (auto& n : netifs_)
        if (n.second.name == ifname) return SAI_STATUS_ITEM_ALREADY_EXISTS;
    Netif n;
    n.name = ifname;
    n.hw = hw;
    n.keep_tag = keep_tag;
    if (netdev_adopt_) {
        n.adopted = true;
        n.orig = "inno_0_" + pad4(IFCS_HANDLE_VALUE(c_.sysport.at(hw)));
        sai_status_t st = rename_netdev(n.orig, ifname, mac);
        if (st != SAI_STATUS_SUCCESS) return st;
        if (keep_tag) OB_WARN(SAI_API_HOSTIF, "%s: adopted netdev keeps the SDK's VLAN tag action", ifname.c_str());
    } else {
        std::string nm = ifname;
        ifcs_attr_t a[4] = {attr_of(IFCS_NETDEV_ATTR_INTF_NAME), attr_of(IFCS_NETDEV_ATTR_DEVPORTNUM),
                            attr_mac(IFCS_NETDEV_ATTR_MACADDR, mac),
                            attr_u32(IFCS_NETDEV_ATTR_VLAN_TAG, tag_action(keep_tag))};
        a[0].value.string.count = (uint32_t)nm.size();
        a[0].value.string.arr = reinterpret_cast<uint8_t*>(&nm[0]);
        a[1].value.devport = (ifcs_devport_t)hw;
        ifcs_handle_t h = IFCS_NULL_HANDLE;
        sai_status_t st = c_.call(c_.L.ifcs_netdev_create(NODE, &h, 4, a), "netdev_create");
        if (st != SAI_STATUS_SUCCESS) return st;
        n.handle = h;
    }
    netif_id = next_netif_++;
    filter_id = netif_id;
    netifs_[netif_id] = n;
    OB_NOTICE(SAI_API_HOSTIF, "netdev %s on devport %d (%s)", ifname.c_str(), hw, n.adopted ? "adopted" : "created");
    return SAI_STATUS_SUCCESS;
}

sai_status_t HostifArea::netif_destroy(int netif_id, int)
{
    Guard g(c_.mu);
    auto it = netifs_.find(netif_id);
    if (it == netifs_.end()) return SAI_STATUS_ITEM_NOT_FOUND;
    sai_status_t st = netif_teardown(it->second);
    netifs_.erase(it);
    return st;
}

sai_status_t HostifArea::netif_carrier_set(const std::string& ifname, bool up)
{
    Guard g(c_.mu);
    for (auto& n : netifs_)
        if (n.second.name == ifname) {
            n.second.carrier_wish = up;
            return SAI_STATUS_SUCCESS;
        }
    return SAI_STATUS_ITEM_NOT_FOUND;
}

sai_status_t HostifArea::netif_set_keep_tag(int netif_id, int, bool keep_tag, int& filter_id)
{
    Guard g(c_.mu);
    auto it = netifs_.find(netif_id);
    if (it == netifs_.end()) return SAI_STATUS_ITEM_NOT_FOUND;
    filter_id = netif_id;
    if (it->second.keep_tag == keep_tag) return SAI_STATUS_SUCCESS;
    if (it->second.adopted) return c_.not_impl("VLAN tag action of an adopted netdev");
    ifcs_attr_t a = attr_u32(IFCS_NETDEV_ATTR_VLAN_TAG, tag_action(keep_tag));
    sai_status_t st = c_.call(c_.L.ifcs_netdev_attr_set(NODE, it->second.handle, 1, &a), "netdev vlan_tag");
    if (st == SAI_STATUS_SUCCESS) it->second.keep_tag = keep_tag;
    return st;
}

// IFCS hostif traps are drop/miss reasons, not protocol matches; protocol
// traps would need ACLs (not mapped). Frames reach the CPU through the
// flood groups (CPU in every VLAN) and through TRAP routes (l3_egress_cpu).
sai_status_t HostifArea::trap_set(TrapKind kind, bool)
{
    static const char* names[] = {"arp-request", "arp-response", "dhcp", "dhcpv6", "igmp", "mld", "nd",
                                  "lldp", "lacp", "stp", "other"};
    std::string w = std::string("trap ") + names[(int)kind];
    c_.log_once(w, "%s: no IFCS control; left as the SDK has it (NOT_SUPPORTED)", w.c_str());
    return SAI_STATUS_NOT_SUPPORTED;
}


// ------------------------------------------------------------- full traps
// The mapping of SAI trap types to CoPP rules (what the CPU should get), in
// the shape isai 6.2.0 programs them on this chip (oracle traces): L2
// protocols by DMAC / ether type, IP protocols by ether type + L4, the
// routed-to-me ones (BGP, SSH, SNMP, BFD) as "IP2ME trap + protocol", IP2ME
// itself as "hostif trap = ip2me", TTL errors as the system trap
// FWD_L3_TTL_CHECK_FAIL (queue + enable + an entry on it for the policer),
// MTU errors as the system trap L3MTU_CHECK_FAIL (queue + enable, no entry:
// not policed, as isai). Priorities: lower wins (I, isai's values).
namespace {
struct TrapDef {
    std::vector<CoppRule> rules;
    uint32_t sys = 0;          // system trap reason (ifcs_hostif_trap_e), 0: a user trap
    bool ip2me = false;        // the trap is the shared IP2ME hostif trap of TRAP routes
    bool glean = false;        // the trap is the glean (neighbor miss) hostif trap
    uint32_t prio = 300;       // offset into the ACL priority range (lower wins)
    bool ok = false;
};
CoppMatch m_bytes(uint32_t type, std::initializer_list<uint8_t> v) { CoppMatch m; m.type = type; m.value = v; return m; }
CoppMatch m_et(uint16_t et) { return m_bytes(IFCS_ACL_MATCH_TYPE_ETHER_TYPE, {(uint8_t)(et >> 8), (uint8_t)et}); }
CoppMatch m_dmac(std::initializer_list<uint8_t> mac) { return m_bytes(IFCS_ACL_MATCH_TYPE_DESTINATION_MAC_ADDRESS, mac); }
CoppMatch m_proto(uint8_t p) { return m_bytes(IFCS_ACL_MATCH_TYPE_L4_PROTOCOL, {p}); }
CoppMatch m_port(bool dst, uint16_t p)
{
    return m_bytes(dst ? IFCS_ACL_MATCH_TYPE_DESTINATION_L4_PORT : IFCS_ACL_MATCH_TYPE_SOURCE_L4_PORT, {(uint8_t)(p >> 8), (uint8_t)p});
}
CoppMatch m_trap(ifcs_handle_t h) { CoppMatch m; m.type = IFCS_ACL_MATCH_TYPE_HOSTIF_TRAP; m.object = h; return m; }

TrapDef trap_def(int32_t type, ifcs_handle_t ip2me, ifcs_handle_t glean)
{
    TrapDef d;
    d.ok = true;
    auto l4 = [&](uint16_t et, uint8_t proto, bool both, uint16_t port, bool me) {
        for (int dst = 1; dst >= (both ? 0 : 1); dst--) {
            CoppRule r = {m_et(et), m_proto(proto), m_port(dst, port)};
            if (me) r.push_back(m_trap(ip2me));
            d.rules.push_back(r);
        }
    };
    switch (type) {
    case SAI_HOSTIF_TRAP_TYPE_LLDP:
        for (uint8_t last : {0x0e, 0x03, 0x00}) d.rules.push_back({m_dmac({0x01, 0x80, 0xc2, 0x00, 0x00, last}), m_et(0x88cc)});
        break;
    case SAI_HOSTIF_TRAP_TYPE_LACP: d.rules.push_back({m_dmac({0x01, 0x80, 0xc2, 0x00, 0x00, 0x02}), m_et(0x8809)}); break;
    case SAI_HOSTIF_TRAP_TYPE_EAPOL: d.rules.push_back({m_et(0x888e)}); break;
    case SAI_HOSTIF_TRAP_TYPE_STP: d.rules.push_back({m_dmac({0x01, 0x80, 0xc2, 0x00, 0x00, 0x00})}); d.prio = 310; break;
    case SAI_HOSTIF_TRAP_TYPE_PVRST: d.rules.push_back({m_dmac({0x01, 0x00, 0x0c, 0xcc, 0xcc, 0xcd})}); break;
    case SAI_HOSTIF_TRAP_TYPE_UDLD: d.rules.push_back({m_dmac({0x01, 0x00, 0x0c, 0xcc, 0xcc, 0xcc})}); break;
    case SAI_HOSTIF_TRAP_TYPE_ARP_REQUEST:
        d.rules.push_back({m_dmac({0xff, 0xff, 0xff, 0xff, 0xff, 0xff}), m_et(0x0806)});
        break;
    case SAI_HOSTIF_TRAP_TYPE_ARP_RESPONSE: d.rules.push_back({m_et(0x0806)}); d.prio = 310; break;   // after the request
    case SAI_HOSTIF_TRAP_TYPE_DHCP: case SAI_HOSTIF_TRAP_TYPE_DHCP_L2:
        for (uint16_t p : {67, 68}) d.rules.push_back({m_et(0x0800), m_proto(17), m_port(true, p)});
        break;
    case SAI_HOSTIF_TRAP_TYPE_DHCPV6: case SAI_HOSTIF_TRAP_TYPE_DHCPV6_L2:
        for (uint16_t p : {546, 547}) d.rules.push_back({m_et(0x86dd), m_proto(17), m_port(true, p)});
        break;
    case SAI_HOSTIF_TRAP_TYPE_IPV6_NEIGHBOR_DISCOVERY:
        for (uint8_t t = 133; t <= 137; t++)
            d.rules.push_back({m_et(0x86dd), m_proto(58), m_bytes(IFCS_ACL_MATCH_TYPE_L4_BYTE1, {t})});
        break;
    case SAI_HOSTIF_TRAP_TYPE_BGP: l4(0x0800, 6, true, 179, true); d.prio = 200; break;
    case SAI_HOSTIF_TRAP_TYPE_BGPV6: l4(0x86dd, 6, true, 179, true); d.prio = 200; break;
    case SAI_HOSTIF_TRAP_TYPE_SSH: l4(0x0800, 6, true, 22, true); l4(0x86dd, 6, true, 22, true); d.prio = 200; break;
    case SAI_HOSTIF_TRAP_TYPE_SNMP: l4(0x0800, 17, false, 161, true); l4(0x86dd, 17, false, 161, true); d.prio = 200; break;
    case SAI_HOSTIF_TRAP_TYPE_BFD:
        for (uint16_t p : {3784, 4784}) l4(0x0800, 17, false, p, true);
        d.prio = 200;
        break;
    case SAI_HOSTIF_TRAP_TYPE_BFDV6:
        for (uint16_t p : {3784, 4784}) l4(0x86dd, 17, false, p, true);
        d.prio = 200;
        break;
    case SAI_HOSTIF_TRAP_TYPE_IP2ME: d.ip2me = true; d.rules.push_back({m_trap(ip2me)}); d.prio = 400; break;
    // isai: a hostif trap of its own (the glean routes copy with it) and an ACE on it
    case SAI_HOSTIF_TRAP_TYPE_NEIGHBOR_MISS: d.glean = true; d.rules.push_back({m_trap(glean)}); d.prio = 400; break;
    // IGMP / MLD: ether type + protocol + the message type in L4 byte 1 (isai's ACEs)
    case SAI_HOSTIF_TRAP_TYPE_IGMP_TYPE_QUERY: case SAI_HOSTIF_TRAP_TYPE_IGMP_TYPE_LEAVE:
    case SAI_HOSTIF_TRAP_TYPE_IGMP_TYPE_V1_REPORT: case SAI_HOSTIF_TRAP_TYPE_IGMP_TYPE_V2_REPORT:
    case SAI_HOSTIF_TRAP_TYPE_IGMP_TYPE_V3_REPORT: {
        uint8_t t = type == SAI_HOSTIF_TRAP_TYPE_IGMP_TYPE_QUERY ? 0x11 : type == SAI_HOSTIF_TRAP_TYPE_IGMP_TYPE_LEAVE ? 0x17
                  : type == SAI_HOSTIF_TRAP_TYPE_IGMP_TYPE_V1_REPORT ? 0x12 : type == SAI_HOSTIF_TRAP_TYPE_IGMP_TYPE_V2_REPORT ? 0x16 : 0x22;
        d.rules.push_back({m_et(0x0800), m_proto(2), m_bytes(IFCS_ACL_MATCH_TYPE_L4_BYTE1, {t})});
        break;
    }
    case SAI_HOSTIF_TRAP_TYPE_IPV6_MLD_V1_V2: case SAI_HOSTIF_TRAP_TYPE_IPV6_MLD_V1_REPORT:
    case SAI_HOSTIF_TRAP_TYPE_IPV6_MLD_V1_DONE: case SAI_HOSTIF_TRAP_TYPE_MLD_V2_REPORT: {
        uint8_t t = type == SAI_HOSTIF_TRAP_TYPE_IPV6_MLD_V1_V2 ? 130 : type == SAI_HOSTIF_TRAP_TYPE_IPV6_MLD_V1_REPORT ? 131
                  : type == SAI_HOSTIF_TRAP_TYPE_IPV6_MLD_V1_DONE ? 132 : 143;
        d.rules.push_back({m_et(0x86dd), m_proto(58), m_bytes(IFCS_ACL_MATCH_TYPE_L4_BYTE1, {t})});
        break;
    }
    case SAI_HOSTIF_TRAP_TYPE_TTL_ERROR:
        d.sys = IFCS_HOSTIF_TRAP_FWD_L3_TTL_CHECK_FAIL;
        d.rules.push_back({m_trap(IFCS_HANDLE_HOSTIF_TRAP(d.sys))});
        break;
    case SAI_HOSTIF_TRAP_TYPE_L3_MTU_ERROR: d.sys = IFCS_HOSTIF_TRAP_L3MTU_CHECK_FAIL; break;
    default: d.ok = false;
    }
    return d;
}

bool to_cpu(int32_t a) { return a == SAI_PACKET_ACTION_TRAP || a == SAI_PACKET_ACTION_COPY || a == SAI_PACKET_ACTION_LOG; }
bool same(const TrapSpec& a, const TrapSpec& b)
{
    return a.type == b.type && a.action == b.action && a.priority == b.priority && a.queue == b.queue &&
           a.group_up == b.group_up && a.policer == b.policer && a.yellow_drop == b.yellow_drop && a.red_drop == b.red_drop;
}
} // namespace

sai_status_t HostifArea::trap_handle_for(Trap& t, uint32_t sys, bool ip2me)
{
    if (t.handle) return SAI_STATUS_SUCCESS;
    if (t.spec.type == SAI_HOSTIF_TRAP_TYPE_NEIGHBOR_MISS) return glean_trap(t.handle);
    if (sys) {
        t.handle = IFCS_HANDLE_HOSTIF_TRAP(sys);
        return SAI_STATUS_SUCCESS;
    }
    if (ip2me) return ip2me_trap(t.handle);
    ifcs_attr_t q = attr_u32(IFCS_HOSTIF_TRAP_ATTR_QUEUE_NUM, t.spec.queue);
    sai_status_t st = c_.call(c_.L.ifcs_hostif_trap_create(NODE, &t.handle, 1, &q), "hostif_trap_create");
    if (st == SAI_STATUS_SUCCESS) t.user = true;
    return st;
}

sai_status_t HostifArea::trap_program(const TrapSpec& s, bool remove)
{
    Guard g(c_.mu);
    system_traps_init();
    // sFlow samples: the MIRROR_TO_CPU system trap + a collector-matched policer entry (tl_sflow)
    if (s.type == SAI_HOSTIF_TRAP_TYPE_SAMPLEPACKET && c_.sflow) return c_.sflow->trap_program(s, remove);
    ifcs_handle_t ip2me = IFCS_NULL_HANDLE, glean = IFCS_NULL_HANDLE;
    sai_status_t st = ip2me_trap(ip2me);
    if (st != SAI_STATUS_SUCCESS) return st;
    if (s.type == SAI_HOSTIF_TRAP_TYPE_NEIGHBOR_MISS) {
        st = glean_trap(glean);
        if (st != SAI_STATUS_SUCCESS) return st;
    }
    TrapDef d = trap_def(s.type, ip2me, glean);
    if (!d.ok) {
        c_.log_once("trap-type:" + std::to_string(s.type), "trap type %d: no IFCS mapping (kept, not programmed)", s.type);
        return SAI_STATUS_NOT_SUPPORTED;
    }
    auto it = traps_.find(s.type);
    if (remove) {
        if (it == traps_.end()) return SAI_STATUS_SUCCESS;
        Trap& t = it->second;
        if (t.copp >= 0) c_.acl->copp_remove(t.copp);
        if (t.user) c_.call(c_.L.ifcs_hostif_trap_delete(NODE, t.handle), "hostif_trap_delete");
        else if (d.sys) {
            ifcs_attr_t e = attr_data(IFCS_HOSTIF_TRAP_ATTR_ENABLE, false);
            c_.call(c_.L.ifcs_hostif_trap_attr_set(NODE, t.handle, 1, &e), "hostif_trap disable");
        } else if (d.ip2me || d.glean) {
            ifcs_attr_t q = attr_u32(IFCS_HOSTIF_TRAP_ATTR_QUEUE_NUM, 0);   // TRAP routes keep it, on queue 0
            c_.call(c_.L.ifcs_hostif_trap_attr_set(NODE, t.handle, 1, &q), "hostif_trap queue (ip2me / glean)");
        }
        traps_.erase(it);
        return SAI_STATUS_SUCCESS;
    }
    if (it != traps_.end() && same(it->second.spec, s)) return SAI_STATUS_SUCCESS;
    bool fresh = it == traps_.end();
    Trap& t = traps_[s.type];
    TrapSpec old = t.spec;
    t.spec = s;
    st = trap_handle_for(t, d.sys, d.ip2me);
    if (st != SAI_STATUS_SUCCESS) {
        if (fresh) traps_.erase(s.type);
        return st;
    }
    bool cpu = to_cpu(s.action) && s.group_up;
    // the hostif trap: queue (and for system traps, whether they fire at all)
    if (!t.user || !fresh) {
        std::vector<ifcs_attr_t> a = {attr_u32(IFCS_HOSTIF_TRAP_ATTR_QUEUE_NUM, s.queue)};
        if (d.sys) a.push_back(attr_data(IFCS_HOSTIF_TRAP_ATTR_ENABLE, cpu));
        if (fresh || old.queue != s.queue || d.sys) {
            st = c_.call(c_.L.ifcs_hostif_trap_attr_set(NODE, t.handle, (uint32_t)a.size(), a.data()), "hostif_trap_attr_set");
            if (st != SAI_STATUS_SUCCESS) return st;
        }
    }
    // entries: re-installed on any change
    if (t.copp >= 0) {
        c_.acl->copp_remove(t.copp);
        t.copp = -1;
    }
    if (d.rules.empty()) return SAI_STATUS_SUCCESS;
    CoppActions act;
    if (cpu) {
        act.trap = t.handle;
        act.drop = s.action == SAI_PACKET_ACTION_TRAP;
        act.policer = s.policer;
        act.yellow_drop = s.yellow_drop;
        act.red_drop = s.red_drop;
    } else if (s.action == SAI_PACKET_ACTION_DROP) {
        act.drop = true;
    } else {
        return SAI_STATUS_SUCCESS;   // FORWARD (or a disabled group): no entry, normal forwarding
    }
    // SAI TRAP_PRIORITY: higher wins; IFCS: lower wins (I)
    uint32_t pr = c_.acl->copp_prio(d.prio * 10 + (s.priority > 9 ? 0 : 9 - s.priority));
    st = c_.acl->copp_install(d.rules, pr, act, t.copp);
    if (st != SAI_STATUS_SUCCESS)
        OB_ERR(SAI_API_HOSTIF, "trap type %d: CoPP entries failed (%d)", s.type, st);
    return st;
}

std::string HostifArea::trap_report()
{
    Guard g(c_.mu);
    std::string out;
    char b[256];
    std::snprintf(b, sizeof(b), "CoPP: %zu traps, %d inno blocks\n", traps_.size(), c_.acl->copp_ibs());
    out += b;
    for (auto& e : traps_) {
        const Trap& t = e.second;
        uint64_t pk = 0, gr = 0, ye = 0, re = 0, rx = 0;
        if (t.copp >= 0) c_.acl->copp_counters(t.copp, pk, gr, ye, re);
        ifcs_hostif_trap_stats_id_t id = IFCS_HOSTIF_TRAP_STATS_ID_RX_PACKET_COUNT;
        uint32_t n = 0;
        bool have_rx = IFCS_STATUS_REASON(c_.L.ifcs_hostif_trap_stats_get(NODE, t.handle, 1, &id, &rx, &n)) == IFCS_SUCCESS;
        std::snprintf(b, sizeof(b), "trap %d: action %d queue %u policer %d hostif 0x%x rx %s%llu | acl pkts %llu green %llu yellow %llu red %llu\n",
                      e.first, t.spec.action, t.spec.queue, t.spec.policer, t.handle, have_rx ? "" : "?",
                      (unsigned long long)rx, (unsigned long long)pk, (unsigned long long)gr, (unsigned long long)ye,
                      (unsigned long long)re);
        out += b;
    }
    // CPU queues: what each one delivered (which queue a trap's frames use).
    for (uint32_t q = 0; q < 8; q++) {
        uint64_t pk = 0, by = 0, dr = 0;
        if (cpu_queue_stats(q, pk, by, dr) != SAI_STATUS_SUCCESS) continue;
        std::snprintf(b, sizeof(b), "cpu queue %u: tx %llu bytes %llu drops %llu\n", q, (unsigned long long)pk,
                      (unsigned long long)by, (unsigned long long)dr);
        out += b;
    }
    // System (drop / exception) traps that delivered packets: which reason
    // sends what to the CPU (handles IFCS_HANDLE(HOSTIF, TRAP, 1..63)).
    for (uint32_t r = 1; r < 64; r++) {
        ifcs_hostif_trap_stats_id_t id = IFCS_HOSTIF_TRAP_STATS_ID_RX_PACKET_COUNT;
        uint64_t rx = 0;
        uint32_t n = 0;
        if (IFCS_STATUS_REASON(c_.L.ifcs_hostif_trap_stats_get(NODE, IFCS_HANDLE_HOSTIF_TRAP(r), 1, &id, &rx, &n)) != IFCS_SUCCESS)
            continue;
        if (!rx) continue;
        std::snprintf(b, sizeof(b), "system trap %u (0x%x): rx %llu\n", r, IFCS_HANDLE_HOSTIF_TRAP(r), (unsigned long long)rx);
        out += b;
    }
    return out;
}

void HostifArea::teardown_all()
{
    for (auto& n : netifs_) netif_teardown(n.second);
    netifs_.clear();
    traps_.clear();   // their IFCS objects go with ifcs_deinit
    trap_ = 0;
    glean_ = 0;
    sys_traps_done_ = false;
}

// One shared hostif trap on CPU queue 0 (trap create as cmds/netdev_test.py).
sai_status_t HostifArea::ip2me_trap(ifcs_handle_t& h)
{
    Guard g(c_.mu);
    if (trap_ == IFCS_NULL_HANDLE) {
        ifcs_attr_t q = attr_u32(IFCS_HOSTIF_TRAP_ATTR_QUEUE_NUM, 0);
        ifcs_handle_t t = IFCS_NULL_HANDLE;
        sai_status_t st = c_.call(c_.L.ifcs_hostif_trap_create(NODE, &t, 1, &q), "hostif_trap_create");
        if (st != SAI_STATUS_SUCCESS) return st;
        trap_ = t;
    }
    h = trap_;
    return SAI_STATUS_SUCCESS;
}

sai_status_t HostifArea::glean_trap(ifcs_handle_t& h)
{
    Guard g(c_.mu);
    if (glean_ == IFCS_NULL_HANDLE) {
        ifcs_attr_t q = attr_u32(IFCS_HOSTIF_TRAP_ATTR_QUEUE_NUM, 0);
        ifcs_handle_t t = IFCS_NULL_HANDLE;
        sai_status_t st = c_.call(c_.L.ifcs_hostif_trap_create(NODE, &t, 1, &q), "hostif_trap_create (glean)");
        if (st != SAI_STATUS_SUCCESS) return st;
        glean_ = t;
    }
    h = glean_;
    return SAI_STATUS_SUCCESS;
}

// isai 6.2.0's create_switch (oracle traps / acl traces): system trap 23
// (L3MTU_CHECK_FAIL) queue 0 + enable, then disable; 17 (TTL) queue 0 +
// enable, enable; 16 (FWD_L2_SRC_MISS_MOVE) queue 0 + enable, then disable;
// 20 (IPv4 options) enable. The same calls in the same order.
void HostifArea::system_traps_init()
{
    Guard g(c_.mu);
    if (sys_traps_done_ || !c_.initialized) return;
    sys_traps_done_ = true;
    if (c_.key("SAI_TERALYNX_SYSTEM_TRAPS", "isai") == "sdk") {
        c_.log_once("systraps", "SAI_TERALYNX_SYSTEM_TRAPS=sdk: system traps left as the SDK has them");
        return;
    }
    struct Step { uint32_t reason; bool queue; bool enable; };
    static const Step steps[] = {
        {IFCS_HOSTIF_TRAP_L3MTU_CHECK_FAIL, true, true},         {IFCS_HOSTIF_TRAP_L3MTU_CHECK_FAIL, false, false},
        {IFCS_HOSTIF_TRAP_FWD_L3_TTL_CHECK_FAIL, true, true},    {IFCS_HOSTIF_TRAP_FWD_L3_TTL_CHECK_FAIL, false, true},
        {IFCS_HOSTIF_TRAP_FWD_L2_SRC_MISS_MOVE, true, true},     {IFCS_HOSTIF_TRAP_FWD_L2_SRC_MISS_MOVE, false, false},
        {IFCS_HOSTIF_TRAP_FWD_IPV4_OPTIONS_DETECTED, false, true},
    };
    for (auto& s : steps) {
        std::vector<ifcs_attr_t> a;
        if (s.queue) a.push_back(attr_u32(IFCS_HOSTIF_TRAP_ATTR_QUEUE_NUM, 0));
        a.push_back(attr_data(IFCS_HOSTIF_TRAP_ATTR_ENABLE, s.enable));
        c_.call(c_.L.ifcs_hostif_trap_attr_set(NODE, IFCS_HANDLE_HOSTIF_TRAP(s.reason), (uint32_t)a.size(), a.data()),
                "hostif_trap_attr_set (system trap)");
    }
    OB_NOTICE(SAI_API_HOSTIF, "system traps as isai: TTL and IPv4 options to the CPU, L2 source miss/move and L3 MTU off");
}

sai_status_t HostifArea::cpu_queue_stats(uint32_t q, uint64_t& packets, uint64_t& bytes, uint64_t& drops)
{
    ifcs_cpu_queue_stats_id_t ids[3] = {IFCS_CPU_QUEUE_STATS_ID_PACKET_TX_COUNT, IFCS_CPU_QUEUE_STATS_ID_BYTE_TX_COUNT,
                                        IFCS_CPU_QUEUE_STATS_ID_PACKET_DROP_COUNT};
    uint64_t v[3] = {0, 0, 0};
    uint32_t n = 0;
    int r = IFCS_STATUS_REASON(c_.L.ifcs_cpu_queue_stats_get(NODE, (ifcs_cpu_queue_t)q, 3, ids, v, &n));
    if (r != IFCS_SUCCESS) return map_reason(r);
    packets = v[0];
    bytes = v[1];
    drops = v[2];
    return SAI_STATUS_SUCCESS;
}

sai_status_t HostifArea::netif_teardown(Netif& n)
{
    if (n.adopted) return rename_netdev(n.name, n.orig, nullptr);
    return c_.call(c_.L.ifcs_netdev_delete(NODE, n.handle), "netdev_delete");
}

sai_status_t HostifArea::rename_netdev(const std::string& from, const std::string& to, const uint8_t* mac)
{
    int s = socket(AF_INET, SOCK_DGRAM | SOCK_CLOEXEC, 0);
    if (s < 0) return SAI_STATUS_FAILURE;
    struct ifreq ifr;
    std::memset(&ifr, 0, sizeof(ifr));
    std::snprintf(ifr.ifr_name, IFNAMSIZ, "%s", from.c_str());
    sai_status_t st = SAI_STATUS_SUCCESS;
    if (ioctl(s, SIOCGIFFLAGS, &ifr) != 0) {
        OB_ERR(SAI_API_HOSTIF, "netdev %s not found (%s); SAI_TERALYNX_NETDEV=adopt needs the init yaml's netdev "
               "auto_create", from.c_str(), std::strerror(errno));
        close(s);
        return SAI_STATUS_ITEM_NOT_FOUND;
    }
    if (ifr.ifr_flags & IFF_UP) {
        ifr.ifr_flags &= ~IFF_UP;
        ioctl(s, SIOCSIFFLAGS, &ifr);
    }
    std::snprintf(ifr.ifr_newname, IFNAMSIZ, "%s", to.c_str());
    if (ioctl(s, SIOCSIFNAME, &ifr) != 0) {
        OB_ERR(SAI_API_HOSTIF, "rename %s -> %s: %s", from.c_str(), to.c_str(), std::strerror(errno));
        st = SAI_STATUS_FAILURE;
    } else if (mac) {
        std::memset(&ifr, 0, sizeof(ifr));
        std::snprintf(ifr.ifr_name, IFNAMSIZ, "%s", to.c_str());
        ifr.ifr_hwaddr.sa_family = ARPHRD_ETHER;
        std::memcpy(ifr.ifr_hwaddr.sa_data, mac, 6);
        if (ioctl(s, SIOCSIFHWADDR, &ifr) != 0)
            OB_WARN(SAI_API_HOSTIF, "%s: set MAC: %s", to.c_str(), std::strerror(errno));
    }
    close(s);
    return st;
}

} // namespace tlb
} // namespace ob
