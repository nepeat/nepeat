// SPDX-License-Identifier: Apache-2.0
// POLICER (SDK meters), ACL (ingress field processor) and CoPP policing.
//
// ACL: a table is one SDK field group (qualifier set = the table's match
// fields + in-ports), an entry one SDK field entry. An entry applies to the
// ports its table is bound to (PORT / LAG INGRESS_ACL = a table group with
// the table as a member, or the table itself; LAG: every member port); an
// unbound table's entries are not installed. Tables in one group are looked
// up in parallel (SDK groups always are). Entries are re-created when they
// change (binding, LAG membership, attributes); counters carry over.
//
// CoPP: a trap group's POLICER meters the CPU-bound traffic of the traps in
// that group with field entries in one CoPP group: the trap's protocol as
// match, the policer attached (one shared meter per SAI policer), red /
// yellow / green packets per the policer's actions. Traps the CPU gets as a
// copy (switch controls: ARP, DHCP, ND, IGMP, MLD) lose only the copy; traps
// whose CPU-bound packets go only to the CPU (LACP, LLDP, STP, EAPOL, and
// traffic routed to the CPU: IP2ME, BGP, SSH, ...) are dropped. Trap types
// without a match here are not policed (logged once per refresh).
#include "sai_internal.h"

#include <algorithm>
#include <iterator>

namespace ob {

static bool to_cpu(int32_t a)
{
    return a == SAI_PACKET_ACTION_TRAP || a == SAI_PACKET_ACTION_COPY || a == SAI_PACKET_ACTION_LOG;
}

// ------------------------------------------------------------------ keys
// A canonical byte string of a rule (CoPP diff, "did this entry change").
template <typename T> static void put(std::string& s, const T& v) { s.append((const char*)&v, sizeof(v)); }
static void put_ip(std::string& s, const IpAddr& a) { put(s, a.v6); s.append((const char*)a.addr, 16); }
static std::string rule_key(const FpRule& r)
{
    std::string s;
    const FpMatch& m = r.m;
    put(s, r.priority); put(s, m.present); put(s, m.ether); put(s, m.ether_mask); put(s, m.ip_type);
    put(s, m.proto); put(s, m.proto_mask); put_ip(s, m.src); put_ip(s, m.src_mask); put_ip(s, m.dst);
    put_ip(s, m.dst_mask); put(s, m.l4_src); put(s, m.l4_src_mask); put(s, m.l4_dst); put(s, m.l4_dst_mask);
    put(s, m.tcp); put(s, m.tcp_mask); put(s, m.icmp); put(s, m.icmp_mask); put(s, m.tos); put(s, m.tos_mask);
    s.append((const char*)m.dmac, 6); s.append((const char*)m.dmac_mask, 6);
    s.append((const char*)m.smac, 6); s.append((const char*)m.smac_mask, 6);
    put(s, m.vlan); put(s, m.vlan_mask); put(s, m.ttl); put(s, m.ttl_mask);
    for (int p : m.ports) put(s, p);
    put(s, r.drop); put(s, r.drop_cancel); put(s, r.copy_cpu); put(s, r.counter); put(s, r.policer);
    put(s, r.green_drop); put(s, r.yellow_drop); put(s, r.red_drop); put(s, r.cpu_copy_only);
    for (int x : r.mirror_ingress) put(s, x);
    put(s, -1);
    for (int x : r.mirror_egress) put(s, x);
    put(s, m.l4_src_range); put(s, m.l4_src_min); put(s, m.l4_src_max);
    put(s, m.l4_dst_range); put(s, m.l4_dst_min); put(s, m.l4_dst_max);
    put(s, -2);
    for (int p : m.out_ports) put(s, p);
    put(s, r.redirect); put(s, r.redirect_egress); put(s, r.set_tc); put(s, r.set_dscp);
    return s;
}

static void apply_colors(FpRule& r, const PolicerObj& p, bool cpu_copy_only)
{
    r.policer = p.pid;
    r.green_drop = p.green == SAI_PACKET_ACTION_DROP;
    r.yellow_drop = p.yellow == SAI_PACKET_ACTION_DROP;
    r.red_drop = p.red == SAI_PACKET_ACTION_DROP;
    r.cpu_copy_only = cpu_copy_only;
}

// ------------------------------------------------------------- policers
bool policer_valid(sai_object_id_t oid) { return oid_type(oid) == SAI_OBJECT_TYPE_POLICER && ctx().policers.count(oid); }

static PolicerCfg policer_cfg(const PolicerObj& p)
{
    PolicerCfg c;
    c.tr_tcm = p.mode == SAI_POLICER_MODE_TR_TCM;
    c.packets = p.meter == SAI_METER_TYPE_PACKETS;
    c.color_blind = p.color == SAI_POLICER_COLOR_SOURCE_BLIND;
    c.cir = p.cir; c.cbs = p.cbs; c.pir = p.pir; c.pbs = p.pbs;
    return c;
}

static bool color_action_ok(int32_t a) { return a == SAI_PACKET_ACTION_FORWARD || a == SAI_PACKET_ACTION_DROP; }

static sai_status_t policer_attr(PolicerObj& p, const sai_attribute_t& a, uint32_t i, bool create)
{
    switch (a.id) {
    case SAI_POLICER_ATTR_METER_TYPE:
        if (!create) return attr_invalid(i);
        if (a.value.s32 != SAI_METER_TYPE_PACKETS && a.value.s32 != SAI_METER_TYPE_BYTES) return attr_not_supported(i);
        p.meter = a.value.s32;
        return SAI_STATUS_SUCCESS;
    case SAI_POLICER_ATTR_MODE:
        if (!create) return attr_invalid(i);
        if (a.value.s32 != SAI_POLICER_MODE_SR_TCM && a.value.s32 != SAI_POLICER_MODE_TR_TCM) return attr_not_supported(i);
        p.mode = a.value.s32;
        return SAI_STATUS_SUCCESS;
    case SAI_POLICER_ATTR_COLOR_SOURCE:
        if (!create) return attr_invalid(i);
        if (a.value.s32 != SAI_POLICER_COLOR_SOURCE_BLIND && a.value.s32 != SAI_POLICER_COLOR_SOURCE_AWARE) return attr_invalid(i);
        p.color = a.value.s32;
        return SAI_STATUS_SUCCESS;
    case SAI_POLICER_ATTR_CBS: p.cbs = a.value.u64; return SAI_STATUS_SUCCESS;
    case SAI_POLICER_ATTR_CIR: p.cir = a.value.u64; return SAI_STATUS_SUCCESS;
    case SAI_POLICER_ATTR_PBS: p.pbs = a.value.u64; return SAI_STATUS_SUCCESS;
    case SAI_POLICER_ATTR_PIR: p.pir = a.value.u64; return SAI_STATUS_SUCCESS;
    case SAI_POLICER_ATTR_GREEN_PACKET_ACTION:
    case SAI_POLICER_ATTR_YELLOW_PACKET_ACTION:
    case SAI_POLICER_ATTR_RED_PACKET_ACTION:
        if (!color_action_ok(a.value.s32)) return attr_not_supported(i);   // FORWARD / DROP only
        (a.id == SAI_POLICER_ATTR_GREEN_PACKET_ACTION ? p.green
         : a.id == SAI_POLICER_ATTR_YELLOW_PACKET_ACTION ? p.yellow : p.red) = a.value.s32;
        return SAI_STATUS_SUCCESS;
    default: return attr_not_supported(i);
    }
}

static sai_status_t create_policer(_Out_ sai_object_id_t* id, _In_ sai_object_id_t, _In_ uint32_t n,
                                   _In_ const sai_attribute_t* attrs)
{
    return guarded(SAI_API_POLICER, __func__, [&]() -> sai_status_t {
        auto& c = ctx();
        if (!id || (n && !attrs)) return SAI_STATUS_INVALID_PARAMETER;
        PolicerObj p;
        if (!find_attr(n, attrs, SAI_POLICER_ATTR_METER_TYPE) || !find_attr(n, attrs, SAI_POLICER_ATTR_MODE))
            return SAI_STATUS_MANDATORY_ATTRIBUTE_MISSING;
        for (uint32_t i = 0; i < n; i++) {
            sai_status_t st = policer_attr(p, attrs[i], i, true);
            if (st != SAI_STATUS_SUCCESS) return st;
        }
        sai_status_t st = c.be->policer_create(policer_cfg(p), p.pid);
        if (st != SAI_STATUS_SUCCESS) {
            OB_ERR(SAI_API_POLICER, "policer (%s, %s, cir %llu cbs %llu pir %llu pbs %llu): SDK create failed (%d)",
                   p.mode == SAI_POLICER_MODE_TR_TCM ? "tr_tcm" : "sr_tcm",
                   p.meter == SAI_METER_TYPE_PACKETS ? "packets" : "bytes", (unsigned long long)p.cir,
                   (unsigned long long)p.cbs, (unsigned long long)p.pir, (unsigned long long)p.pbs, st);
            return st;
        }
        sai_object_id_t oid = c.alloc(SAI_OBJECT_TYPE_POLICER);
        c.policers[oid] = p;
        *id = oid;
        OB_NOTICE(SAI_API_POLICER, "policer %d: %s %s cir %llu cbs %llu pir %llu pbs %llu", p.pid,
                  p.mode == SAI_POLICER_MODE_TR_TCM ? "tr_tcm" : "sr_tcm",
                  p.meter == SAI_METER_TYPE_PACKETS ? "packets" : "bytes", (unsigned long long)p.cir,
                  (unsigned long long)p.cbs, (unsigned long long)p.pir, (unsigned long long)p.pbs);
        return SAI_STATUS_SUCCESS;
    });
}

static bool policer_in_use(sai_object_id_t oid)
{
    auto& c = ctx();
    for (auto& g : c.trap_groups) if (g.second.policer == oid) return true;
    for (auto& p : c.ports)
        for (auto s : p.second.storm) if (s == oid) return true;   // storm control (sai_hash.cpp)
    for (auto& e : c.acl_entries) {
        auto it = e.second.actions.find(SAI_ACL_ENTRY_ATTR_ACTION_SET_POLICER);
        if (it != e.second.actions.end() && it->second.parameter.oid == oid) return true;
    }
    return false;
}

static sai_status_t remove_policer(_In_ sai_object_id_t id)
{
    return guarded(SAI_API_POLICER, __func__, [&]() -> sai_status_t {
        auto& c = ctx();
        PolicerObj* p = lookup(c.policers, id);
        if (!p) return SAI_STATUS_INVALID_OBJECT_ID;
        if (policer_in_use(id)) return SAI_STATUS_OBJECT_IN_USE;
        sai_status_t st = c.be->policer_destroy(p->pid);
        if (st != SAI_STATUS_SUCCESS && st != SAI_STATUS_ITEM_NOT_FOUND) return st;
        c.policers.erase(id);
        return SAI_STATUS_SUCCESS;
    });
}

static sai_status_t set_policer_attribute(_In_ sai_object_id_t id, _In_ const sai_attribute_t* attr)
{
    return guarded(SAI_API_POLICER, __func__, [&]() -> sai_status_t {
        auto& c = ctx();
        PolicerObj* p = lookup(c.policers, id);
        if (!p) return SAI_STATUS_INVALID_OBJECT_ID;
        if (!attr) return SAI_STATUS_INVALID_PARAMETER;
        PolicerObj n = *p;
        sai_status_t st = policer_attr(n, *attr, 0, false);
        if (st != SAI_STATUS_SUCCESS) return st;
        bool colors = n.green != p->green || n.yellow != p->yellow || n.red != p->red;
        st = c.be->policer_set(p->pid, policer_cfg(n));
        if (st != SAI_STATUS_SUCCESS) return attr_backend_status(st, 0);
        *p = n;
        if (colors) {   // colour actions are entry actions in the SDK
            copp_refresh();
            acl_reprogram();
        }
        return SAI_STATUS_SUCCESS;
    });
}

static sai_status_t get_policer_attribute(_In_ sai_object_id_t id, _In_ uint32_t n, _Inout_ sai_attribute_t* attrs)
{
    return guarded(SAI_API_POLICER, __func__, [&]() -> sai_status_t {
        PolicerObj* p = lookup(ctx().policers, id);
        if (!p) return SAI_STATUS_INVALID_OBJECT_ID;
        if (n && !attrs) return SAI_STATUS_INVALID_PARAMETER;
        for (uint32_t i = 0; i < n; i++) {
            sai_attribute_t& a = attrs[i];
            switch (a.id) {
            case SAI_POLICER_ATTR_METER_TYPE: a.value.s32 = p->meter; break;
            case SAI_POLICER_ATTR_MODE: a.value.s32 = p->mode; break;
            case SAI_POLICER_ATTR_COLOR_SOURCE: a.value.s32 = p->color; break;
            case SAI_POLICER_ATTR_CBS: a.value.u64 = p->cbs; break;
            case SAI_POLICER_ATTR_CIR: a.value.u64 = p->cir; break;
            case SAI_POLICER_ATTR_PBS: a.value.u64 = p->pbs; break;
            case SAI_POLICER_ATTR_PIR: a.value.u64 = p->pir; break;
            case SAI_POLICER_ATTR_GREEN_PACKET_ACTION: a.value.s32 = p->green; break;
            case SAI_POLICER_ATTR_YELLOW_PACKET_ACTION: a.value.s32 = p->yellow; break;
            case SAI_POLICER_ATTR_RED_PACKET_ACTION: a.value.s32 = p->red; break;
            default: return attr_not_supported(i);
            }
        }
        return SAI_STATUS_SUCCESS;
    });
}

static sai_status_t get_policer_stats(_In_ sai_object_id_t, _In_ uint32_t, _In_ const sai_stat_id_t*, _Out_ uint64_t*)
{
    return SAI_STATUS_NOT_SUPPORTED;   // no per-colour meter counters (sai_query_stats_capability: none)
}
static sai_status_t get_policer_stats_ext(_In_ sai_object_id_t, _In_ uint32_t, _In_ const sai_stat_id_t*,
                                          _In_ sai_stats_mode_t, _Out_ uint64_t*)
{
    return SAI_STATUS_NOT_SUPPORTED;
}
static sai_status_t clear_policer_stats(_In_ sai_object_id_t, _In_ uint32_t, _In_ const sai_stat_id_t*)
{
    return SAI_STATUS_NOT_SUPPORTED;
}

static sai_policer_api_t make_policer_api()
{
    sai_policer_api_t a;
    std::memset(&a, 0, sizeof(a));
    a.create_policer = create_policer;
    a.remove_policer = remove_policer;
    a.set_policer_attribute = set_policer_attribute;
    a.get_policer_attribute = get_policer_attribute;
    a.get_policer_stats = get_policer_stats;
    a.get_policer_stats_ext = get_policer_stats_ext;
    a.clear_policer_stats = clear_policer_stats;
    return a;
}
const sai_policer_api_t policer_api = make_policer_api();

// ------------------------------------------------------------------ CoPP
static void l4(FpRule& r, uint16_t ether, uint8_t proto, bool dst, uint16_t port)
{
    r.m.set(FpQual::EtherType); r.m.ether = ether; r.m.ether_mask = 0xffff;
    r.m.set(FpQual::IpProtocol); r.m.proto = proto; r.m.proto_mask = 0xff;
    if (dst) { r.m.set(FpQual::L4DstPort); r.m.l4_dst = port; r.m.l4_dst_mask = 0xffff; }
    else { r.m.set(FpQual::L4SrcPort); r.m.l4_src = port; r.m.l4_src_mask = 0xffff; }
}
static FpRule ether_rule(uint16_t ether)
{
    FpRule r;
    r.m.set(FpQual::EtherType); r.m.ether = ether; r.m.ether_mask = 0xffff;
    return r;
}
static FpRule icmp6_rule(uint8_t type)
{
    FpRule r = ether_rule(0x86dd);
    r.m.set(FpQual::IpProtocol); r.m.proto = 58; r.m.proto_mask = 0xff;
    r.m.set(FpQual::IcmpTypeCode); r.m.icmp = (uint16_t)(type << 8); r.m.icmp_mask = 0xff00;
    return r;
}

// Matches of a trap type's CPU-bound packets; copy: the CPU gets a copy
// (cancel it), else the packet goes only to the CPU (drop it).
static std::vector<FpRule> copp_rules(int32_t type, bool& copy)
{
    std::vector<FpRule> v;
    copy = false;
    auto ip2me = [](FpRule r) { r.m.set(FpQual::DstCpu); return r; };
    auto both_ports = [&](uint16_t ether, uint8_t proto, uint16_t port) {
        FpRule a, b;
        l4(a, ether, proto, true, port);
        l4(b, ether, proto, false, port);
        v.push_back(ip2me(a));
        v.push_back(ip2me(b));
    };
    switch (type) {
    case SAI_HOSTIF_TRAP_TYPE_ARP_REQUEST:
    case SAI_HOSTIF_TRAP_TYPE_ARP_RESPONSE: {
        FpRule r;
        r.m.set(FpQual::IpType);
        r.m.ip_type = type == SAI_HOSTIF_TRAP_TYPE_ARP_REQUEST ? SAI_ACL_IP_TYPE_ARP_REQUEST : SAI_ACL_IP_TYPE_ARP_REPLY;
        v.push_back(r);
        copy = true;
        break;
    }
    case SAI_HOSTIF_TRAP_TYPE_IPV6_NEIGHBOR_DISCOVERY:
        for (uint8_t t = 133; t <= 137; t++) v.push_back(icmp6_rule(t));
        copy = true;
        break;
    case SAI_HOSTIF_TRAP_TYPE_IPV6_NEIGHBOR_SOLICITATION: v.push_back(icmp6_rule(135)); copy = true; break;
    case SAI_HOSTIF_TRAP_TYPE_IPV6_NEIGHBOR_ADVERTISEMENT: v.push_back(icmp6_rule(136)); copy = true; break;
    case SAI_HOSTIF_TRAP_TYPE_IPV6_MLD_V1_V2:
        for (uint8_t t : {130, 131, 132, 143}) v.push_back(icmp6_rule(t));
        copy = true;
        break;
    case SAI_HOSTIF_TRAP_TYPE_IPV6_MLD_V1_REPORT: v.push_back(icmp6_rule(131)); copy = true; break;
    case SAI_HOSTIF_TRAP_TYPE_IPV6_MLD_V1_DONE: v.push_back(icmp6_rule(132)); copy = true; break;
    case SAI_HOSTIF_TRAP_TYPE_MLD_V2_REPORT: v.push_back(icmp6_rule(143)); copy = true; break;
    case SAI_HOSTIF_TRAP_TYPE_DHCP:
    case SAI_HOSTIF_TRAP_TYPE_DHCP_L2:
        for (uint16_t p : {67, 68}) { FpRule r; l4(r, 0x0800, 17, true, p); v.push_back(r); }
        copy = true;
        break;
    case SAI_HOSTIF_TRAP_TYPE_DHCPV6:
    case SAI_HOSTIF_TRAP_TYPE_DHCPV6_L2:
        for (uint16_t p : {546, 547}) { FpRule r; l4(r, 0x86dd, 17, true, p); v.push_back(r); }
        copy = true;
        break;
    case SAI_HOSTIF_TRAP_TYPE_IGMP_TYPE_QUERY: case SAI_HOSTIF_TRAP_TYPE_IGMP_TYPE_LEAVE:
    case SAI_HOSTIF_TRAP_TYPE_IGMP_TYPE_V1_REPORT: case SAI_HOSTIF_TRAP_TYPE_IGMP_TYPE_V2_REPORT:
    case SAI_HOSTIF_TRAP_TYPE_IGMP_TYPE_V3_REPORT: {
        FpRule r = ether_rule(0x0800);
        r.m.set(FpQual::IpProtocol); r.m.proto = 2; r.m.proto_mask = 0xff;
        v.push_back(r);
        copy = true;
        break;
    }
    case SAI_HOSTIF_TRAP_TYPE_LACP: v.push_back(ether_rule(0x8809)); break;   // slow protocols
    case SAI_HOSTIF_TRAP_TYPE_LLDP: v.push_back(ether_rule(0x88cc)); break;
    case SAI_HOSTIF_TRAP_TYPE_EAPOL: v.push_back(ether_rule(0x888e)); break;
    case SAI_HOSTIF_TRAP_TYPE_STP:
    case SAI_HOSTIF_TRAP_TYPE_PVRST: {
        FpRule r;
        static const uint8_t bpdu[6] = {0x01, 0x80, 0xc2, 0x00, 0x00, 0x00};
        r.m.set(FpQual::DstMac);
        std::memcpy(r.m.dmac, bpdu, 6);
        std::memset(r.m.dmac_mask, 0xff, 6);
        v.push_back(r);
        break;
    }
    case SAI_HOSTIF_TRAP_TYPE_BGP: both_ports(0x0800, 6, 179); break;
    case SAI_HOSTIF_TRAP_TYPE_BGPV6: both_ports(0x86dd, 6, 179); break;
    case SAI_HOSTIF_TRAP_TYPE_SSH: both_ports(0x0800, 6, 22); both_ports(0x86dd, 6, 22); break;
    case SAI_HOSTIF_TRAP_TYPE_SNMP: { FpRule a, b; l4(a, 0x0800, 17, true, 161); l4(b, 0x86dd, 17, true, 161);
        v.push_back(ip2me(a)); v.push_back(ip2me(b)); break; }
    case SAI_HOSTIF_TRAP_TYPE_BFD:
    case SAI_HOSTIF_TRAP_TYPE_BFDV6:
        for (uint16_t p : {3784, 4784}) {
            FpRule r;
            l4(r, type == SAI_HOSTIF_TRAP_TYPE_BFD ? 0x0800 : 0x86dd, 17, true, p);
            v.push_back(ip2me(r));
        }
        break;
    case SAI_HOSTIF_TRAP_TYPE_IP2ME: { FpRule r; v.push_back(ip2me(r)); break; }
    default: break;
    }
    // Specific protocols before IP2ME (one CoPP group: the highest entry wins).
    for (auto& r : v) r.priority = r.m.present == (1u << (int)FpQual::DstCpu) ? 100 : r.m.has(FpQual::DstCpu) ? 150 : 200;
    return v;
}

static const std::vector<FpQual> copp_qset_base = {
    FpQual::EtherType, FpQual::IpType, FpQual::IpProtocol, FpQual::L4SrcPort, FpQual::L4DstPort,
    FpQual::IcmpTypeCode, FpQual::DstMac};

void copp_refresh()
{
    auto& c = ctx();
    if (!c.be) return;
    // Traps the backend programs itself (Backend::trap_program): hand it the
    // current trap / group / policer state; no field entries for them.
    for (auto& t : c.traps) {
        if (!t.second.full) continue;
        sai_status_t st = c.be->trap_program(trap_spec(t.second), false);
        if (st != SAI_STATUS_SUCCESS) OB_ERR(SAI_API_HOSTIF, "trap type %d: backend reprogram failed (%d)", t.second.type, st);
    }
    struct Want { FpRule r; sai_object_id_t pol; };
    std::map<std::string, Want> want;
    std::vector<int32_t> unpoliced;
    bool need_cpu = false;
    for (auto& t : c.traps) {
        if (t.second.full || !to_cpu(t.second.action)) continue;
        TrapGroupObj* g = lookup(c.trap_groups, t.second.group);
        if (!g || !g->policer) continue;
        PolicerObj* p = lookup(c.policers, g->policer);
        if (!p) continue;
        bool copy = false;
        auto rules = copp_rules(t.second.type, copy);
        if (rules.empty()) unpoliced.push_back(t.second.type);
        for (auto& r : rules) {
            apply_colors(r, *p, copy);
            if (r.m.has(FpQual::DstCpu)) need_cpu = true;
            want.emplace(rule_key(r), Want{r, g->policer});   // first trap wins a shared match
        }
    }
    if (!want.empty() && c.copp_gid < 0) {
        std::vector<FpQual> q = copp_qset_base;
        q.push_back(FpQual::DstCpu);
        int gid = -1;
        sai_status_t st = c.be->fp_group_create(q, 1000, gid);
        c.copp_dst_cpu = st == SAI_STATUS_SUCCESS;
        if (st != SAI_STATUS_SUCCESS) {
            st = c.be->fp_group_create(copp_qset_base, 1000, gid);
            if (st == SAI_STATUS_SUCCESS)
                OB_WARN(SAI_API_HOSTIF, "CoPP: field group without the routed-to-CPU qualifier: IP2ME, BGP, SSH, "
                                        "SNMP, BFD are not policed");
        }
        if (st != SAI_STATUS_SUCCESS) {
            OB_ERR(SAI_API_HOSTIF, "CoPP: field group create failed (%d): traps run unpoliced", st);
            return;
        }
        c.copp_gid = gid;
    }
    if (need_cpu && !c.copp_dst_cpu)
        for (auto it = want.begin(); it != want.end();)
            it = it->second.r.m.has(FpQual::DstCpu) ? want.erase(it) : std::next(it);
    for (auto it = c.copp.begin(); it != c.copp.end();) {
        if (want.count(it->first)) { ++it; continue; }
        c.be->fp_entry_destroy(it->second.eid);
        it = c.copp.erase(it);
    }
    for (auto& w : want) {
        if (c.copp.count(w.first)) continue;
        int eid = -1;
        sai_status_t st = c.be->fp_entry_create(c.copp_gid, w.second.r, eid);
        if (st != SAI_STATUS_SUCCESS) {
            OB_ERR(SAI_API_HOSTIF, "CoPP: field entry create failed (%d)", st);
            continue;
        }
        c.copp[w.first] = CoppEntry{eid, w.second.pol};
    }
    if (c.copp.empty() && c.copp_gid >= 0) {
        c.be->fp_group_destroy(c.copp_gid);
        c.copp_gid = -1;
    }
    static size_t last = (size_t)-1;
    if (c.copp.size() != last) {
        std::set<sai_object_id_t> pols;
        for (auto& e : c.copp) pols.insert(e.second.policer);
        OB_NOTICE(SAI_API_HOSTIF, "CoPP: %zu field entries, %zu policers (field group %d)", c.copp.size(), pols.size(),
                  c.copp_gid);
        last = c.copp.size();
    }
    if (!unpoliced.empty()) {
        std::string s;
        for (int32_t t : unpoliced) s += (s.empty() ? "" : ",") + std::to_string(t);
        OB_INFO(SAI_API_HOSTIF, "CoPP: trap types %s have a policer but no match here: not policed", s.c_str());
    }
}

// ------------------------------------------------------------------ ACL
// Table match fields this SAI programs (sai_caps.cpp advertises the same).
static FpQual field_qual(sai_attr_id_t f, bool& ok)
{
    ok = true;
    switch (f) {
    case SAI_ACL_TABLE_ATTR_FIELD_SRC_IP: return FpQual::SrcIp;
    case SAI_ACL_TABLE_ATTR_FIELD_DST_IP: return FpQual::DstIp;
    case SAI_ACL_TABLE_ATTR_FIELD_SRC_IPV6: return FpQual::SrcIp6;
    case SAI_ACL_TABLE_ATTR_FIELD_DST_IPV6: return FpQual::DstIp6;
    case SAI_ACL_TABLE_ATTR_FIELD_L4_SRC_PORT: return FpQual::L4SrcPort;
    case SAI_ACL_TABLE_ATTR_FIELD_L4_DST_PORT: return FpQual::L4DstPort;
    case SAI_ACL_TABLE_ATTR_FIELD_IP_PROTOCOL:
    case SAI_ACL_TABLE_ATTR_FIELD_IPV6_NEXT_HEADER: return FpQual::IpProtocol;
    case SAI_ACL_TABLE_ATTR_FIELD_ETHER_TYPE: return FpQual::EtherType;
    case SAI_ACL_TABLE_ATTR_FIELD_TCP_FLAGS: return FpQual::TcpFlags;
    case SAI_ACL_TABLE_ATTR_FIELD_ICMP_TYPE: case SAI_ACL_TABLE_ATTR_FIELD_ICMP_CODE:
    case SAI_ACL_TABLE_ATTR_FIELD_ICMPV6_TYPE: case SAI_ACL_TABLE_ATTR_FIELD_ICMPV6_CODE: return FpQual::IcmpTypeCode;
    case SAI_ACL_TABLE_ATTR_FIELD_DSCP: case SAI_ACL_TABLE_ATTR_FIELD_ECN: case SAI_ACL_TABLE_ATTR_FIELD_TOS: return FpQual::Tos;
    case SAI_ACL_TABLE_ATTR_FIELD_ACL_IP_TYPE: return FpQual::IpType;
    case SAI_ACL_TABLE_ATTR_FIELD_SRC_MAC: return FpQual::SrcMac;
    case SAI_ACL_TABLE_ATTR_FIELD_DST_MAC: return FpQual::DstMac;
    case SAI_ACL_TABLE_ATTR_FIELD_OUTER_VLAN_ID: return FpQual::OuterVlan;
    case SAI_ACL_TABLE_ATTR_FIELD_TTL: return FpQual::Ttl;
    case SAI_ACL_TABLE_ATTR_FIELD_IN_PORTS: case SAI_ACL_TABLE_ATTR_FIELD_IN_PORT: return FpQual::InPorts;
    default: ok = false; return FpQual::Count;
    }
}
static bool fp_cap(FpCap k) { return ctx().be && ctx().be->fp_supported(k); }

// Fields outside FpQual (FpCap::OutPorts): the egress port of an egress table.
static bool extra_field(sai_attr_id_t f)
{
    return (f == SAI_ACL_TABLE_ATTR_FIELD_OUT_PORTS || f == SAI_ACL_TABLE_ATTR_FIELD_OUT_PORT) && fp_cap(FpCap::OutPorts);
}
bool acl_table_field_supported(sai_attr_id_t f) { bool ok; field_qual(f, ok); return ok || extra_field(f); }

static const int32_t acl_actions[] = {SAI_ACL_ACTION_TYPE_PACKET_ACTION, SAI_ACL_ACTION_TYPE_COUNTER,
                                      SAI_ACL_ACTION_TYPE_SET_POLICER};
std::vector<int32_t> acl_ingress_actions()
{
    std::vector<int32_t> v(std::begin(acl_actions), std::end(acl_actions));
    if (ctx().be && ctx().be->mirror_supported()) {
        v.push_back(SAI_ACL_ACTION_TYPE_MIRROR_INGRESS);
        v.push_back(SAI_ACL_ACTION_TYPE_MIRROR_EGRESS);
    }
    if (fp_cap(FpCap::Redirect)) v.push_back(SAI_ACL_ACTION_TYPE_REDIRECT);
    if (fp_cap(FpCap::SetTc)) v.push_back(SAI_ACL_ACTION_TYPE_SET_TC);
    if (fp_cap(FpCap::SetDscp)) v.push_back(SAI_ACL_ACTION_TYPE_SET_DSCP);
    return v;
}
// Egress tables (FpCap::Egress): drop / forward and counters.
std::vector<int32_t> acl_egress_actions()
{
    if (!fp_cap(FpCap::Egress)) return {};
    return {SAI_ACL_ACTION_TYPE_PACKET_ACTION, SAI_ACL_ACTION_TYPE_COUNTER};
}

static bool is_table_field(sai_attr_id_t id)
{
    return id >= SAI_ACL_TABLE_ATTR_FIELD_START && id <= SAI_ACL_TABLE_ATTR_FIELD_END;
}

static sai_status_t create_acl_table(_Out_ sai_object_id_t* id, _In_ sai_object_id_t, _In_ uint32_t n,
                                     _In_ const sai_attribute_t* attrs)
{
    return guarded(SAI_API_ACL, __func__, [&]() -> sai_status_t {
        auto& c = ctx();
        if (!id || (n && !attrs)) return SAI_STATUS_INVALID_PARAMETER;
        if (macsec_acl_stage(n, attrs)) return macsec_acl_table_create(id, n, attrs);   // an external PHY's, not the ASIC's
        AclTableObj t;
        bool has_stage = false;
        for (uint32_t i = 0; i < n; i++) {
            const sai_attribute_t& a = attrs[i];
            if (is_table_field(a.id)) {
                if (a.id == SAI_ACL_TABLE_ATTR_FIELD_ACL_RANGE_TYPE) {
                    // AclOrch adds L4 port ranges to every ingress table; accepted and
                    // kept, but ACL_RANGE objects are NOT_SUPPORTED (rules using one fail)
                    if (a.value.s32list.count && !a.value.s32list.list) return attr_invalid(i);
                    t.range_types.assign(a.value.s32list.list, a.value.s32list.list + a.value.s32list.count);
                    continue;
                }
                if (!a.value.booldata) continue;
                if (!acl_table_field_supported(a.id)) {
                    OB_ERR(SAI_API_ACL, "ACL table: match field %d not supported", a.id);
                    return attr_not_supported(i);
                }
                t.fields.insert(a.id);
                continue;
            }
            switch (a.id) {
            case SAI_ACL_TABLE_ATTR_ACL_STAGE:
                if (a.value.s32 != SAI_ACL_STAGE_INGRESS && !(a.value.s32 == SAI_ACL_STAGE_EGRESS && fp_cap(FpCap::Egress))) {
                    OB_ERR(SAI_API_ACL, "ACL table: stage %d not supported (ingress only)", a.value.s32);
                    return attr_not_supported(i);
                }
                t.stage = a.value.s32;
                has_stage = true;
                break;
            case SAI_ACL_TABLE_ATTR_ACL_BIND_POINT_TYPE_LIST:
                if (a.value.s32list.count && !a.value.s32list.list) return attr_invalid(i);
                t.bind_types.assign(a.value.s32list.list, a.value.s32list.list + a.value.s32list.count);
                for (int32_t b : t.bind_types)
                    if (b != SAI_ACL_BIND_POINT_TYPE_PORT && b != SAI_ACL_BIND_POINT_TYPE_LAG) return attr_not_supported(i);
                break;
            case SAI_ACL_TABLE_ATTR_ACL_ACTION_TYPE_LIST:
                if (a.value.s32list.count && !a.value.s32list.list) return attr_invalid(i);
                t.action_types.assign(a.value.s32list.list, a.value.s32list.list + a.value.s32list.count);
                break;
            case SAI_ACL_TABLE_ATTR_SIZE: t.size = a.value.u32; break;
            default: return attr_not_supported(i);
            }
        }
        if (!has_stage) return SAI_STATUS_MANDATORY_ATTRIBUTE_MISSING;
        bool egress = t.stage == SAI_ACL_STAGE_EGRESS;
        std::vector<int32_t> allowed = egress ? acl_egress_actions() : acl_ingress_actions();
        if (!fp_cap(FpCap::Redirect) && !fp_cap(FpCap::SetTc) && !fp_cap(FpCap::SetDscp) && !egress)
            allowed.assign(std::begin(acl_actions), std::end(acl_actions));   // the original rule
        for (size_t k = 0; k < t.action_types.size(); k++)
            if (std::find(allowed.begin(), allowed.end(), t.action_types[k]) == allowed.end()) {
                OB_ERR(SAI_API_ACL, "ACL table: action type %d not supported", t.action_types[k]);
                uint32_t ai = 0;
                find_attr(n, attrs, SAI_ACL_TABLE_ATTR_ACL_ACTION_TYPE_LIST, &ai);
                return attr_not_supported(ai);
            }
        FpGroupExtra x;
        x.egress = egress;
        if (fp_cap(FpCap::Ranges))
            for (int32_t rt : t.range_types) {
                if (rt == SAI_ACL_RANGE_TYPE_L4_SRC_PORT_RANGE) x.src_range = true;
                if (rt == SAI_ACL_RANGE_TYPE_L4_DST_PORT_RANGE) x.dst_range = true;
            }
        x.out_ports = t.fields.count(SAI_ACL_TABLE_ATTR_FIELD_OUT_PORTS) || t.fields.count(SAI_ACL_TABLE_ATTR_FIELD_OUT_PORT);
        for (int32_t act : t.action_types) {
            if (act == SAI_ACL_ACTION_TYPE_REDIRECT) x.redirect = true;
            if (act == SAI_ACL_ACTION_TYPE_SET_TC) x.set_tc = true;
            if (act == SAI_ACL_ACTION_TYPE_SET_DSCP) x.set_dscp = true;
            if (act == SAI_ACL_ACTION_TYPE_MIRROR_INGRESS || act == SAI_ACL_ACTION_TYPE_MIRROR_EGRESS) x.mirror = true;
        }        std::set<FpQual> q = {FpQual::InPorts};
        for (auto f : t.fields) {
            if (extra_field(f)) continue;
            bool ok;
            q.insert(field_qual(f, ok));
        }
        int prio = c.acl_next_prio++;
        sai_status_t st = c.be->fp_group_create_ex(std::vector<FpQual>(q.begin(), q.end()), prio, x, t.gid);
        if (st != SAI_STATUS_SUCCESS) {
            OB_ERR(SAI_API_ACL, "ACL table (%zu match fields): SDK field group create failed (%d)", t.fields.size(), st);
            return st;
        }
        sai_object_id_t oid = c.alloc(SAI_OBJECT_TYPE_ACL_TABLE);
        c.acl_tables[oid] = t;
        *id = oid;
        OB_NOTICE(SAI_API_ACL, "ACL table: SDK field group %d, %zu match fields", t.gid, t.fields.size());
        return SAI_STATUS_SUCCESS;
    });
}

static bool acl_referenced(sai_object_id_t acl)
{
    auto& c = ctx();
    for (auto& b : c.acl_bind) if (b.second == acl) return true;
    return false;
}

static sai_status_t remove_acl_table(_In_ sai_object_id_t id)
{
    return guarded(SAI_API_ACL, __func__, [&]() -> sai_status_t {
        auto& c = ctx();
        if (macsec_acl_table(id)) return macsec_acl_table_remove(id);
        AclTableObj* t = lookup(c.acl_tables, id);
        if (!t) return SAI_STATUS_INVALID_OBJECT_ID;
        if (!t->entries.empty() || acl_referenced(id)) return SAI_STATUS_OBJECT_IN_USE;
        for (auto& m : c.acl_group_members) if (m.second.table == id) return SAI_STATUS_OBJECT_IN_USE;
        for (auto& k : c.acl_counters) if (k.second.table == id) return SAI_STATUS_OBJECT_IN_USE;
        sai_status_t st = c.be->fp_group_destroy(t->gid);
        if (st != SAI_STATUS_SUCCESS && st != SAI_STATUS_ITEM_NOT_FOUND) return st;
        c.acl_tables.erase(id);
        return SAI_STATUS_SUCCESS;
    });
}

static sai_status_t set_acl_table_attribute(_In_ sai_object_id_t id, _In_ const sai_attribute_t* attr)
{
    return guarded(SAI_API_ACL, __func__, [&]() -> sai_status_t {
        if (!ctx().acl_tables.count(id)) return SAI_STATUS_INVALID_OBJECT_ID;
        return attr_invalid(0);   // all table attributes are create-only
    });
}

static sai_status_t get_acl_table_attribute(_In_ sai_object_id_t id, _In_ uint32_t n, _Inout_ sai_attribute_t* attrs)
{
    return guarded(SAI_API_ACL, __func__, [&]() -> sai_status_t {
        if (n && !attrs) return SAI_STATUS_INVALID_PARAMETER;
        if (macsec_acl_table(id)) return macsec_acl_table_get(id, n, attrs);
        AclTableObj* t = lookup(ctx().acl_tables, id);
        if (!t) return SAI_STATUS_INVALID_OBJECT_ID;
        for (uint32_t i = 0; i < n; i++) {
            sai_attribute_t& a = attrs[i];
            sai_status_t st = SAI_STATUS_SUCCESS;
            if (a.id == SAI_ACL_TABLE_ATTR_FIELD_ACL_RANGE_TYPE) {
                st = fill_list(a.value.s32list, t->range_types);
                if (st != SAI_STATUS_SUCCESS) return st;
                continue;
            }
            if (is_table_field(a.id)) {
                a.value.booldata = t->fields.count(a.id) != 0;
                continue;
            }
            switch (a.id) {
            case SAI_ACL_TABLE_ATTR_ACL_STAGE: a.value.s32 = t->stage; break;
            case SAI_ACL_TABLE_ATTR_ACL_BIND_POINT_TYPE_LIST: st = fill_list(a.value.s32list, t->bind_types); break;
            case SAI_ACL_TABLE_ATTR_ACL_ACTION_TYPE_LIST: st = fill_list(a.value.s32list, t->action_types); break;
            case SAI_ACL_TABLE_ATTR_SIZE: a.value.u32 = t->size; break;
            case SAI_ACL_TABLE_ATTR_ENTRY_LIST: st = fill_list(a.value.objlist, t->entries); break;
            case SAI_ACL_TABLE_ATTR_AVAILABLE_ACL_ENTRY: {
                uint32_t free = 0;
                st = ctx().be->fp_group_free(t->gid, free);   // NOT_SUPPORTED: CRM marks it unsupported
                if (st == SAI_STATUS_NOT_SUPPORTED) return attr_not_supported(i);
                if (st != SAI_STATUS_SUCCESS) return st;
                a.value.u32 = free;
                break;
            }
            default: return attr_not_supported(i);   // AVAILABLE_ACL_COUNTER: CRM marks it unsupported
            }
            if (st != SAI_STATUS_SUCCESS) return st;
        }
        return SAI_STATUS_SUCCESS;
    });
}

// ----------------------------------------------------------- entries
// Ports an ACL table applies to: everything bound to it directly or through
// a table group it is a member of (LAGs: their member ports).
static std::set<int> table_ports(sai_object_id_t table)
{
    auto& c = ctx();
    std::set<int> out;
    const AclTableObj* t = lookup(c.acl_tables, table);
    const auto& binds = t && t->stage == SAI_ACL_STAGE_EGRESS ? c.acl_bind_egress : c.acl_bind;
    for (auto& b : binds) {
        bool hit = b.second == table;
        if (!hit && oid_type(b.second) == SAI_OBJECT_TYPE_ACL_TABLE_GROUP)
            for (auto& m : c.acl_group_members)
                if (m.second.group == b.second && m.second.table == table) { hit = true; break; }
        if (!hit) continue;
        for (int hw : attach_phys(b.first)) out.insert(hw);
    }
    return out;
}

static void field_ip(const sai_acl_field_data_t& d, bool v6, IpAddr& a, IpAddr& m)
{
    a.v6 = m.v6 = v6;
    if (v6) { std::memcpy(a.addr, d.data.ip6, 16); std::memcpy(m.addr, d.mask.ip6, 16); }
    else { std::memcpy(a.addr, &d.data.ip4, 4); std::memcpy(m.addr, &d.mask.ip4, 4); }
}

// The SDK rule of an entry; false = nothing to install (admin down, or no
// port it applies to).
static bool entry_rule(const AclEntryObj& e, FpRule& r)
{
    auto& c = ctx();
    if (!e.admin && !fp_cap(FpCap::EntryEnable)) return false;   // else installed and switched off
    std::set<int> ports = table_ports(e.table);
    FpMatch& m = r.m;
    r.priority = e.priority;
    for (auto& f : e.fields) {
        const sai_acl_field_data_t& d = f.second.d;
        switch (f.first) {
        case SAI_ACL_ENTRY_ATTR_FIELD_SRC_IP: m.set(FpQual::SrcIp); field_ip(d, false, m.src, m.src_mask); break;
        case SAI_ACL_ENTRY_ATTR_FIELD_DST_IP: m.set(FpQual::DstIp); field_ip(d, false, m.dst, m.dst_mask); break;
        case SAI_ACL_ENTRY_ATTR_FIELD_SRC_IPV6: m.set(FpQual::SrcIp6); field_ip(d, true, m.src, m.src_mask); break;
        case SAI_ACL_ENTRY_ATTR_FIELD_DST_IPV6: m.set(FpQual::DstIp6); field_ip(d, true, m.dst, m.dst_mask); break;
        case SAI_ACL_ENTRY_ATTR_FIELD_L4_SRC_PORT: m.set(FpQual::L4SrcPort); m.l4_src = d.data.u16; m.l4_src_mask = d.mask.u16; break;
        case SAI_ACL_ENTRY_ATTR_FIELD_L4_DST_PORT: m.set(FpQual::L4DstPort); m.l4_dst = d.data.u16; m.l4_dst_mask = d.mask.u16; break;
        case SAI_ACL_ENTRY_ATTR_FIELD_IP_PROTOCOL:
        case SAI_ACL_ENTRY_ATTR_FIELD_IPV6_NEXT_HEADER: m.set(FpQual::IpProtocol); m.proto = d.data.u8; m.proto_mask = d.mask.u8; break;
        case SAI_ACL_ENTRY_ATTR_FIELD_ETHER_TYPE: m.set(FpQual::EtherType); m.ether = d.data.u16; m.ether_mask = d.mask.u16; break;
        case SAI_ACL_ENTRY_ATTR_FIELD_TCP_FLAGS: m.set(FpQual::TcpFlags); m.tcp = d.data.u8; m.tcp_mask = d.mask.u8; break;
        case SAI_ACL_ENTRY_ATTR_FIELD_ICMP_TYPE: case SAI_ACL_ENTRY_ATTR_FIELD_ICMPV6_TYPE:
            m.set(FpQual::IcmpTypeCode);
            m.icmp = (uint16_t)((m.icmp & 0x00ff) | (d.data.u8 << 8));
            m.icmp_mask = (uint16_t)((m.icmp_mask & 0x00ff) | (d.mask.u8 << 8));
            break;
        case SAI_ACL_ENTRY_ATTR_FIELD_ICMP_CODE: case SAI_ACL_ENTRY_ATTR_FIELD_ICMPV6_CODE:
            m.set(FpQual::IcmpTypeCode);
            m.icmp = (uint16_t)((m.icmp & 0xff00) | d.data.u8);
            m.icmp_mask = (uint16_t)((m.icmp_mask & 0xff00) | d.mask.u8);
            break;
        case SAI_ACL_ENTRY_ATTR_FIELD_DSCP:
            m.set(FpQual::Tos);
            m.tos = (uint8_t)((m.tos & 0x03) | ((d.data.u8 & 0x3f) << 2));
            m.tos_mask = (uint8_t)((m.tos_mask & 0x03) | ((d.mask.u8 & 0x3f) << 2));
            break;
        case SAI_ACL_ENTRY_ATTR_FIELD_ECN:
            m.set(FpQual::Tos);
            m.tos = (uint8_t)((m.tos & 0xfc) | (d.data.u8 & 0x03));
            m.tos_mask = (uint8_t)((m.tos_mask & 0xfc) | (d.mask.u8 & 0x03));
            break;
        case SAI_ACL_ENTRY_ATTR_FIELD_TOS: m.set(FpQual::Tos); m.tos = d.data.u8; m.tos_mask = d.mask.u8; break;
        case SAI_ACL_ENTRY_ATTR_FIELD_ACL_IP_TYPE:
            if (d.data.s32 == SAI_ACL_IP_TYPE_ANY) break;
            m.set(FpQual::IpType);
            m.ip_type = d.data.s32;
            break;
        case SAI_ACL_ENTRY_ATTR_FIELD_SRC_MAC: m.set(FpQual::SrcMac); std::memcpy(m.smac, d.data.mac, 6); std::memcpy(m.smac_mask, d.mask.mac, 6); break;
        case SAI_ACL_ENTRY_ATTR_FIELD_DST_MAC: m.set(FpQual::DstMac); std::memcpy(m.dmac, d.data.mac, 6); std::memcpy(m.dmac_mask, d.mask.mac, 6); break;
        case SAI_ACL_ENTRY_ATTR_FIELD_OUTER_VLAN_ID: m.set(FpQual::OuterVlan); m.vlan = d.data.u16 & 0xfff; m.vlan_mask = d.mask.u16 & 0xfff; break;
        case SAI_ACL_ENTRY_ATTR_FIELD_TTL: m.set(FpQual::Ttl); m.ttl = d.data.u8; m.ttl_mask = d.mask.u8; break;
        case SAI_ACL_ENTRY_ATTR_FIELD_ACL_RANGE_TYPE:
            for (auto o : f.second.objs) {
                const AclRangeObj* rg = lookup(c.acl_ranges, o);
                if (!rg) continue;
                if (rg->type == SAI_ACL_RANGE_TYPE_L4_SRC_PORT_RANGE) {
                    m.l4_src_range = true; m.l4_src_min = (uint16_t)rg->min; m.l4_src_max = (uint16_t)rg->max;
                } else if (rg->type == SAI_ACL_RANGE_TYPE_L4_DST_PORT_RANGE) {
                    m.l4_dst_range = true; m.l4_dst_min = (uint16_t)rg->min; m.l4_dst_max = (uint16_t)rg->max;
                }
            }
            break;
        case SAI_ACL_ENTRY_ATTR_FIELD_OUT_PORTS:
        case SAI_ACL_ENTRY_ATTR_FIELD_OUT_PORT: {
            std::set<int> o;
            for (auto p : f.second.objs) for (int hw : attach_phys(p)) o.insert(hw);
            m.out_ports.assign(o.begin(), o.end());
            break;
        }
        case SAI_ACL_ENTRY_ATTR_FIELD_IN_PORTS:
        case SAI_ACL_ENTRY_ATTR_FIELD_IN_PORT: {
            std::set<int> in;
            for (auto p : f.second.objs) for (int hw : attach_phys(p)) in.insert(hw);
            std::set<int> both;
            std::set_intersection(ports.begin(), ports.end(), in.begin(), in.end(), std::inserter(both, both.begin()));
            ports.swap(both);
            break;
        }
        default: break;
        }
    }
    if (ports.empty()) return false;
    m.set(FpQual::InPorts);
    m.ports.assign(ports.begin(), ports.end());
    for (auto& a : e.actions) {
        switch (a.first) {
        case SAI_ACL_ENTRY_ATTR_ACTION_PACKET_ACTION:
            switch (a.second.parameter.s32) {
            case SAI_PACKET_ACTION_DROP: case SAI_PACKET_ACTION_DENY: r.drop = true; break;
            case SAI_PACKET_ACTION_FORWARD: case SAI_PACKET_ACTION_TRANSIT: r.drop_cancel = true; break;
            case SAI_PACKET_ACTION_COPY: case SAI_PACKET_ACTION_LOG: r.copy_cpu = true; break;
            case SAI_PACKET_ACTION_TRAP: r.copy_cpu = r.drop = true; break;
            default: break;
            }
            break;
        case SAI_ACL_ENTRY_ATTR_ACTION_COUNTER: r.counter = a.second.parameter.oid != SAI_NULL_OBJECT_ID; break;
        case SAI_ACL_ENTRY_ATTR_ACTION_REDIRECT: {
            sai_object_id_t o = a.second.parameter.oid;
            if (oid_type(o) == SAI_OBJECT_TYPE_NEXT_HOP) {
                if (NextHopObj* nh = lookup(c.next_hops, o)) { r.redirect = nh->egress; r.redirect_egress = true; }
            } else {
                r.redirect = attach_dest(o);   // port or LAG
            }
            break;
        }
        case SAI_ACL_ENTRY_ATTR_ACTION_SET_TC: r.set_tc = a.second.parameter.u8; break;
        case SAI_ACL_ENTRY_ATTR_ACTION_SET_DSCP: r.set_dscp = a.second.parameter.u8 & 0x3f; break;
        case SAI_ACL_ENTRY_ATTR_ACTION_SET_POLICER:
            if (PolicerObj* p = lookup(c.policers, a.second.parameter.oid)) apply_colors(r, *p, false);
            break;
        case SAI_ACL_ENTRY_ATTR_ACTION_MIRROR_INGRESS:
        case SAI_ACL_ENTRY_ATTR_ACTION_MIRROR_EGRESS: {
            auto it = e.action_objs.find(a.first);
            std::vector<int>& ids = a.first == SAI_ACL_ENTRY_ATTR_ACTION_MIRROR_INGRESS ? r.mirror_ingress : r.mirror_egress;
            if (it != e.action_objs.end())
                for (auto s : it->second) {
                    int id = mirror_backend_id(s);
                    if (id >= 0) ids.push_back(id);
                }
            break;
        }
        default: break;
        }
    }
    return true;
}

static sai_object_id_t entry_counter(const AclEntryObj& e)
{
    auto it = e.actions.find(SAI_ACL_ENTRY_ATTR_ACTION_COUNTER);
    return it == e.actions.end() ? SAI_NULL_OBJECT_ID : it->second.parameter.oid;
}

// Fold the SDK counter of an entry into its SAI counter (before the entry goes).
static void counter_fold(AclEntryObj& e)
{
    auto& c = ctx();
    AclCounterObj* k = lookup(c.acl_counters, entry_counter(e));
    if (!k || e.eid < 0) return;
    uint64_t p = 0, b = 0;
    if (c.be->fp_entry_counter(e.eid, p, b) == SAI_STATUS_SUCCESS) {
        k->base_packets += p;
        k->base_bytes += b;
    }
}

static void entry_uninstall(AclEntryObj& e)
{
    if (e.eid < 0) return;
    counter_fold(e);
    ctx().be->fp_entry_destroy(e.eid);
    e.eid = -1;
    e.key.clear();
}

// Install / re-install / remove an entry's SDK rule to match its state.
static sai_status_t entry_program(AclEntryObj& e)
{
    auto& c = ctx();
    FpRule r;
    if (!entry_rule(e, r)) { entry_uninstall(e); return SAI_STATUS_SUCCESS; }
    std::string key = rule_key(r);
    bool want_on = e.admin;
    if (e.eid >= 0 && key == e.key) {
        if (fp_cap(FpCap::EntryEnable) && want_on != e.hw_enabled) {
            sai_status_t st = c.be->fp_entry_enable(e.eid, want_on);
            if (st != SAI_STATUS_SUCCESS) return st;
            e.hw_enabled = want_on;
        }
        return SAI_STATUS_SUCCESS;
    }
    AclTableObj* t = lookup(c.acl_tables, e.table);
    if (!t) return SAI_STATUS_INVALID_OBJECT_ID;
    if (e.eid >= 0 && fp_cap(FpCap::EntryUpdate)) {   // in place: the SDK counter keeps counting
        sai_status_t st = c.be->fp_entry_update(e.eid, r);
        if (st == SAI_STATUS_SUCCESS) {
            e.key = key;
            if (fp_cap(FpCap::EntryEnable) && want_on != e.hw_enabled) {
                st = c.be->fp_entry_enable(e.eid, want_on);
                if (st != SAI_STATUS_SUCCESS) return st;
                e.hw_enabled = want_on;
            }
            return SAI_STATUS_SUCCESS;
        }
        OB_WARN(SAI_API_ACL, "ACL entry: in-place update failed (%d), re-creating it", st);
    }
    int eid = -1;
    entry_uninstall(e);   // the SDK entry is re-created (counters folded in)
    sai_status_t st = c.be->fp_entry_create(t->gid, r, eid);
    if (st != SAI_STATUS_SUCCESS) {
        OB_ERR(SAI_API_ACL, "ACL entry (priority %u, %zu ports): SDK field entry create failed (%d)", e.priority,
               r.m.ports.size(), st);
        return st;
    }
    e.eid = eid;
    e.key = key;
    e.hw_enabled = true;
    if (!want_on) {   // FpCap::EntryEnable (entry_rule installs admin-down entries only then)
        sai_status_t es = c.be->fp_entry_enable(eid, false);
        if (es == SAI_STATUS_SUCCESS) e.hw_enabled = false;
        else OB_ERR(SAI_API_ACL, "ACL entry: disabling SDK entry %d failed (%d)", eid, es);
    }
    OB_NOTICE(SAI_API_ACL, "ACL entry: SDK entry %d in group %d, priority %u, %zu ports%s%s%s", eid, t->gid, e.priority,
              r.m.ports.size(), r.drop ? ", drop" : "", r.copy_cpu ? ", copy to CPU" : "", r.counter ? ", counted" : "");
    return SAI_STATUS_SUCCESS;
}

void acl_reprogram()
{
    for (auto& e : ctx().acl_entries) {
        sai_status_t st = entry_program(e.second);
        if (st != SAI_STATUS_SUCCESS)
            OB_ERR(SAI_API_ACL, "ACL entry %llx: re-program failed (%d)", (unsigned long long)e.first, st);
    }
}

static bool is_entry_field(sai_attr_id_t id) { return id >= SAI_ACL_ENTRY_ATTR_FIELD_START && id <= SAI_ACL_ENTRY_ATTR_FIELD_END; }
static bool is_entry_action(sai_attr_id_t id) { return id >= SAI_ACL_ENTRY_ATTR_ACTION_START && id <= SAI_ACL_ENTRY_ATTR_ACTION_END; }
static sai_attr_id_t entry_to_table_field(sai_attr_id_t id)
{
    return id - SAI_ACL_ENTRY_ATTR_FIELD_START + SAI_ACL_TABLE_ATTR_FIELD_START;
}

// Apply one entry attribute to the model (validated against the table).
static sai_status_t entry_attr(AclEntryObj& e, sai_object_id_t self, const AclTableObj* t, const sai_attribute_t& a,
                               uint32_t i)
{
    auto& c = ctx();
    if (a.id == SAI_ACL_ENTRY_ATTR_FIELD_ACL_RANGE_TYPE) {   // ACL_RANGE objects (FpCap::Ranges)
        if (!fp_cap(FpCap::Ranges)) return attr_not_supported(i);
        if (!a.value.aclfield.enable) { e.fields.erase(a.id); return SAI_STATUS_SUCCESS; }
        const sai_object_list_t& l = a.value.aclfield.data.objlist;
        if (l.count && !l.list) return attr_invalid(i);
        AclFieldVal v;
        v.d = a.value.aclfield;
        v.d.data.objlist.list = nullptr;
        v.objs.assign(l.list, l.list + l.count);
        for (auto o : v.objs) {
            const AclRangeObj* rg = lookup(c.acl_ranges, o);
            if (!rg) return attr_invalid(i);
            if (t && std::find(t->range_types.begin(), t->range_types.end(), rg->type) == t->range_types.end()) {
                OB_ERR(SAI_API_ACL, "ACL entry: range type %d is not one of its table's", rg->type);
                return attr_invalid(i);
            }
        }
        e.fields[a.id] = v;
        return SAI_STATUS_SUCCESS;
    }
    if (is_entry_field(a.id)) {
        sai_attr_id_t tf = entry_to_table_field(a.id);
        if (!acl_table_field_supported(tf)) return attr_not_supported(i);
        if (!a.value.aclfield.enable) { e.fields.erase(a.id); return SAI_STATUS_SUCCESS; }
        if (t && !t->fields.count(tf)) {
            OB_ERR(SAI_API_ACL, "ACL entry: match field %d is not a field of its table", a.id);
            return attr_invalid(i);
        }
        AclFieldVal v;
        v.d = a.value.aclfield;
        if (a.id == SAI_ACL_ENTRY_ATTR_FIELD_IN_PORTS) {
            const sai_object_list_t& l = a.value.aclfield.data.objlist;
            if (l.count && !l.list) return attr_invalid(i);
            v.objs.assign(l.list, l.list + l.count);
        } else if (a.id == SAI_ACL_ENTRY_ATTR_FIELD_IN_PORT) {
            v.objs.push_back(a.value.aclfield.data.oid);
        } else if (a.id == SAI_ACL_ENTRY_ATTR_FIELD_OUT_PORTS) {
            const sai_object_list_t& l = a.value.aclfield.data.objlist;
            if (l.count && !l.list) return attr_invalid(i);
            v.objs.assign(l.list, l.list + l.count);
        } else if (a.id == SAI_ACL_ENTRY_ATTR_FIELD_OUT_PORT) {
            v.objs.push_back(a.value.aclfield.data.oid);
        }
        for (auto o : v.objs) {   // front-panel ports (LAG members too) or LAGs
            PortObj* p = oid_type(o) == SAI_OBJECT_TYPE_PORT ? lookup(c.ports, o) : nullptr;
            if (!(p && !p->cpu) && !(oid_type(o) == SAI_OBJECT_TYPE_LAG && c.lags.count(o))) return attr_invalid(i);
        }
        if (a.id == SAI_ACL_ENTRY_ATTR_FIELD_IN_PORTS || a.id == SAI_ACL_ENTRY_ATTR_FIELD_OUT_PORTS)
            v.d.data.objlist.list = nullptr;   // never keep the caller's pointer
        e.fields[a.id] = v;
        return SAI_STATUS_SUCCESS;
    }
    if (is_entry_action(a.id)) {
        if (!a.value.aclaction.enable) { e.actions.erase(a.id); e.action_objs.erase(a.id); return SAI_STATUS_SUCCESS; }
        switch (a.id) {
        case SAI_ACL_ENTRY_ATTR_ACTION_PACKET_ACTION:
            switch (a.value.aclaction.parameter.s32) {
            case SAI_PACKET_ACTION_DROP: case SAI_PACKET_ACTION_DENY: case SAI_PACKET_ACTION_FORWARD:
            case SAI_PACKET_ACTION_TRANSIT: case SAI_PACKET_ACTION_COPY: case SAI_PACKET_ACTION_LOG:
            case SAI_PACKET_ACTION_TRAP: break;
            default: return attr_not_supported(i);
            }
            break;
        case SAI_ACL_ENTRY_ATTR_ACTION_COUNTER: {
            sai_object_id_t k = a.value.aclaction.parameter.oid;
            AclCounterObj* ko = lookup(c.acl_counters, k);
            if (k && !ko) return attr_invalid(i);
            // one entry per counter (the SDK counter is the entry's)
            if (ko && ko->entry && ko->entry != self && c.acl_entries.count(ko->entry)) return SAI_STATUS_OBJECT_IN_USE;
            break;
        }
        case SAI_ACL_ENTRY_ATTR_ACTION_SET_POLICER:
            if (a.value.aclaction.parameter.oid && !policer_valid(a.value.aclaction.parameter.oid)) return attr_invalid(i);
            break;
        case SAI_ACL_ENTRY_ATTR_ACTION_MIRROR_INGRESS:
        case SAI_ACL_ENTRY_ATTR_ACTION_MIRROR_EGRESS: {
            // Mirror sessions (sai_mirror.cpp): only with a backend that mirrors.
            if (!c.be->mirror_supported()) return attr_not_supported(i);
            const sai_object_list_t& l = a.value.aclaction.parameter.objlist;
            if (l.count && !l.list) return attr_invalid(i);
            std::vector<sai_object_id_t> v(l.list, l.list + l.count);
            for (auto s : v) if (!mirror_valid(s)) return attr_invalid(i);
            e.action_objs[a.id] = v;
            sai_acl_action_data_t d = a.value.aclaction;
            d.parameter.objlist.list = nullptr;   // never keep the caller's pointer
            e.actions[a.id] = d;
            return SAI_STATUS_SUCCESS;
        }
        case SAI_ACL_ENTRY_ATTR_ACTION_REDIRECT: {
            if (!fp_cap(FpCap::Redirect)) return attr_not_supported(i);
            sai_object_id_t o = a.value.aclaction.parameter.oid;
            bool ok = (oid_type(o) == SAI_OBJECT_TYPE_NEXT_HOP && c.next_hops.count(o)) ||
                      ((oid_type(o) == SAI_OBJECT_TYPE_PORT || oid_type(o) == SAI_OBJECT_TYPE_LAG) && attach_valid(o));
            if (!ok) return attr_invalid(i);
            break;
        }
        case SAI_ACL_ENTRY_ATTR_ACTION_SET_TC:
            if (!fp_cap(FpCap::SetTc)) return attr_not_supported(i);
            if (a.value.aclaction.parameter.u8 > 7) return attr_invalid(i);
            break;
        case SAI_ACL_ENTRY_ATTR_ACTION_SET_DSCP:
            if (!fp_cap(FpCap::SetDscp)) return attr_not_supported(i);
            if (a.value.aclaction.parameter.u8 > 63) return attr_invalid(i);
            break;
        default:
            OB_ERR(SAI_API_ACL, "ACL entry: action %d not supported", a.id);
            return attr_not_supported(i);
        }
        e.actions[a.id] = a.value.aclaction;
        return SAI_STATUS_SUCCESS;
    }
    switch (a.id) {
    case SAI_ACL_ENTRY_ATTR_PRIORITY: {
        ChipLimits l;
        if (c.be->chip_limits(l) == SAI_STATUS_SUCCESS && (a.value.u32 < l.acl_prio_min || a.value.u32 > l.acl_prio_max))
            return attr_invalid(i);
        e.priority = a.value.u32;
        return SAI_STATUS_SUCCESS;
    }
    case SAI_ACL_ENTRY_ATTR_ADMIN_STATE: e.admin = a.value.booldata; return SAI_STATUS_SUCCESS;
    default: return attr_not_supported(i);
    }
}

static void counter_link(AclEntryObj& e, sai_object_id_t self, sai_object_id_t old)
{
    auto& c = ctx();
    sai_object_id_t now = entry_counter(e);
    if (old == now) return;
    if (AclCounterObj* k = lookup(c.acl_counters, old)) k->entry = 0;
    if (AclCounterObj* k = lookup(c.acl_counters, now)) k->entry = self;
}

static sai_status_t create_acl_entry(_Out_ sai_object_id_t* id, _In_ sai_object_id_t, _In_ uint32_t n,
                                     _In_ const sai_attribute_t* attrs)
{
    return guarded(SAI_API_ACL, __func__, [&]() -> sai_status_t {
        auto& c = ctx();
        if (!id || (n && !attrs)) return SAI_STATUS_INVALID_PARAMETER;
        uint32_t ti = 0;
        const sai_attribute_t* ta = find_attr(n, attrs, SAI_ACL_ENTRY_ATTR_TABLE_ID, &ti);
        if (!ta) return SAI_STATUS_MANDATORY_ATTRIBUTE_MISSING;
        if (macsec_acl_table(ta->value.oid)) return macsec_acl_entry_create(id, n, attrs);
        AclTableObj* t = lookup(c.acl_tables, ta->value.oid);
        if (!t) return attr_invalid(ti);
        AclEntryObj e;
        e.table = ta->value.oid;
        for (uint32_t i = 0; i < n; i++) {
            if (attrs[i].id == SAI_ACL_ENTRY_ATTR_TABLE_ID) continue;
            sai_status_t st = entry_attr(e, SAI_NULL_OBJECT_ID, t, attrs[i], i);
            if (st != SAI_STATUS_SUCCESS) return st;
        }
        sai_status_t st = entry_program(e);
        if (st != SAI_STATUS_SUCCESS) return st;
        sai_object_id_t oid = c.alloc(SAI_OBJECT_TYPE_ACL_ENTRY);
        c.acl_entries[oid] = e;
        counter_link(c.acl_entries[oid], oid, SAI_NULL_OBJECT_ID);
        t->entries.push_back(oid);
        *id = oid;
        return SAI_STATUS_SUCCESS;
    });
}

static sai_status_t remove_acl_entry(_In_ sai_object_id_t id)
{
    return guarded(SAI_API_ACL, __func__, [&]() -> sai_status_t {
        auto& c = ctx();
        if (macsec_acl_entry(id)) return macsec_acl_entry_remove(id);
        AclEntryObj* e = lookup(c.acl_entries, id);
        if (!e) return SAI_STATUS_INVALID_OBJECT_ID;
        entry_uninstall(*e);
        if (AclCounterObj* k = lookup(c.acl_counters, entry_counter(*e))) k->entry = 0;
        if (AclTableObj* t = lookup(c.acl_tables, e->table))
            t->entries.erase(std::remove(t->entries.begin(), t->entries.end(), id), t->entries.end());
        c.acl_entries.erase(id);
        return SAI_STATUS_SUCCESS;
    });
}

static sai_status_t set_acl_entry_attribute(_In_ sai_object_id_t id, _In_ const sai_attribute_t* attr)
{
    return guarded(SAI_API_ACL, __func__, [&]() -> sai_status_t {
        auto& c = ctx();
        if (!attr) return SAI_STATUS_INVALID_PARAMETER;
        if (macsec_acl_entry(id)) return attr->id == SAI_ACL_ENTRY_ATTR_TABLE_ID ? attr_invalid(0) : macsec_acl_entry_set(id, attr);
        AclEntryObj* e = lookup(c.acl_entries, id);
        if (!e) return SAI_STATUS_INVALID_OBJECT_ID;
        if (attr->id == SAI_ACL_ENTRY_ATTR_TABLE_ID) return attr_invalid(0);
        AclEntryObj n = *e;
        sai_status_t st = entry_attr(n, id, lookup(c.acl_tables, e->table), *attr, 0);
        if (st != SAI_STATUS_SUCCESS) return st;
        sai_object_id_t old = entry_counter(*e);
        if (old != entry_counter(n)) entry_uninstall(*e);   // fold into the old counter first
        n.eid = e->eid;
        n.key = e->key;
        counter_link(n, id, old);
        st = entry_program(n);
        *e = n;   // the model follows the request; a failed program leaves the entry uninstalled
        return st == SAI_STATUS_SUCCESS ? st : attr_backend_status(st, 0);
    });
}

static sai_status_t get_acl_entry_attribute(_In_ sai_object_id_t id, _In_ uint32_t n, _Inout_ sai_attribute_t* attrs)
{
    return guarded(SAI_API_ACL, __func__, [&]() -> sai_status_t {
        if (n && !attrs) return SAI_STATUS_INVALID_PARAMETER;
        if (macsec_acl_entry(id)) return macsec_acl_entry_get(id, n, attrs);
        AclEntryObj* e = lookup(ctx().acl_entries, id);
        if (!e) return SAI_STATUS_INVALID_OBJECT_ID;
        for (uint32_t i = 0; i < n; i++) {
            sai_attribute_t& a = attrs[i];
            if (is_entry_field(a.id)) {
                bool ranges = a.id == SAI_ACL_ENTRY_ATTR_FIELD_ACL_RANGE_TYPE && fp_cap(FpCap::Ranges);
                if (!ranges && !acl_table_field_supported(entry_to_table_field(a.id))) return attr_not_supported(i);
                auto it = e->fields.find(a.id);
                if (it == e->fields.end()) { a.value.aclfield.enable = false; continue; }
                sai_object_list_t keep = a.value.aclfield.data.objlist;
                a.value.aclfield = it->second.d;
                if (a.id == SAI_ACL_ENTRY_ATTR_FIELD_IN_PORTS || a.id == SAI_ACL_ENTRY_ATTR_FIELD_OUT_PORTS || ranges) {
                    a.value.aclfield.data.objlist = keep;
                    sai_status_t st = fill_list(a.value.aclfield.data.objlist, it->second.objs);
                    if (st != SAI_STATUS_SUCCESS) return st;
                }
                continue;
            }
            if (is_entry_action(a.id)) {
                auto it = e->actions.find(a.id);
                if (it == e->actions.end()) { a.value.aclaction.enable = false; continue; }
                auto ob = e->action_objs.find(a.id);
                if (ob != e->action_objs.end()) {
                    sai_object_list_t keep = a.value.aclaction.parameter.objlist;
                    a.value.aclaction = it->second;
                    a.value.aclaction.parameter.objlist = keep;
                    sai_status_t st = fill_list(a.value.aclaction.parameter.objlist, ob->second);
                    if (st != SAI_STATUS_SUCCESS) return st;
                    continue;
                }
                a.value.aclaction = it->second;
                continue;
            }
            switch (a.id) {
            case SAI_ACL_ENTRY_ATTR_TABLE_ID: a.value.oid = e->table; break;
            case SAI_ACL_ENTRY_ATTR_PRIORITY: a.value.u32 = e->priority; break;
            case SAI_ACL_ENTRY_ATTR_ADMIN_STATE: a.value.booldata = e->admin; break;
            default: return attr_not_supported(i);
            }
        }
        return SAI_STATUS_SUCCESS;
    });
}

// ----------------------------------------------------------- counters
static sai_status_t create_acl_counter(_Out_ sai_object_id_t* id, _In_ sai_object_id_t, _In_ uint32_t n,
                                       _In_ const sai_attribute_t* attrs)
{
    return guarded(SAI_API_ACL, __func__, [&]() -> sai_status_t {
        auto& c = ctx();
        if (!id || (n && !attrs)) return SAI_STATUS_INVALID_PARAMETER;
        AclCounterObj k;
        bool ht = false;
        for (uint32_t i = 0; i < n; i++) {
            const sai_attribute_t& a = attrs[i];
            switch (a.id) {
            case SAI_ACL_COUNTER_ATTR_TABLE_ID:
                if (!c.acl_tables.count(a.value.oid)) return attr_invalid(i);
                k.table = a.value.oid;
                ht = true;
                break;
            case SAI_ACL_COUNTER_ATTR_ENABLE_PACKET_COUNT: k.packets = a.value.booldata; break;
            case SAI_ACL_COUNTER_ATTR_ENABLE_BYTE_COUNT: k.bytes = a.value.booldata; break;
            case SAI_ACL_COUNTER_ATTR_PACKETS: k.base_packets = a.value.u64; break;
            case SAI_ACL_COUNTER_ATTR_BYTES: k.base_bytes = a.value.u64; break;
            default: return attr_not_supported(i);
            }
        }
        if (!ht) return SAI_STATUS_MANDATORY_ATTRIBUTE_MISSING;
        sai_object_id_t oid = c.alloc(SAI_OBJECT_TYPE_ACL_COUNTER);
        c.acl_counters[oid] = k;
        *id = oid;
        return SAI_STATUS_SUCCESS;
    });
}

static sai_status_t remove_acl_counter(_In_ sai_object_id_t id)
{
    return guarded(SAI_API_ACL, __func__, [&]() -> sai_status_t {
        auto& c = ctx();
        AclCounterObj* k = lookup(c.acl_counters, id);
        if (!k) return SAI_STATUS_INVALID_OBJECT_ID;
        if (k->entry && c.acl_entries.count(k->entry)) return SAI_STATUS_OBJECT_IN_USE;
        c.acl_counters.erase(id);
        return SAI_STATUS_SUCCESS;
    });
}

static void counter_read(const AclCounterObj& k, uint64_t& p, uint64_t& b)
{
    auto& c = ctx();
    p = k.base_packets;
    b = k.base_bytes;
    auto it = c.acl_entries.find(k.entry);
    if (it == c.acl_entries.end() || it->second.eid < 0) return;
    uint64_t hp = 0, hb = 0;
    if (c.be->fp_entry_counter(it->second.eid, hp, hb) == SAI_STATUS_SUCCESS) { p += hp; b += hb; }
}

static sai_status_t set_acl_counter_attribute(_In_ sai_object_id_t id, _In_ const sai_attribute_t* attr)
{
    return guarded(SAI_API_ACL, __func__, [&]() -> sai_status_t {
        AclCounterObj* k = lookup(ctx().acl_counters, id);
        if (!k) return SAI_STATUS_INVALID_OBJECT_ID;
        if (!attr) return SAI_STATUS_INVALID_PARAMETER;
        if (attr->id != SAI_ACL_COUNTER_ATTR_PACKETS && attr->id != SAI_ACL_COUNTER_ATTR_BYTES) return attr_invalid(0);
        uint64_t p, b;
        counter_read(*k, p, b);   // set = make the counter read this value from now on
        if (attr->id == SAI_ACL_COUNTER_ATTR_PACKETS) k->base_packets += attr->value.u64 - p;
        else k->base_bytes += attr->value.u64 - b;
        return SAI_STATUS_SUCCESS;
    });
}

static sai_status_t get_acl_counter_attribute(_In_ sai_object_id_t id, _In_ uint32_t n, _Inout_ sai_attribute_t* attrs)
{
    return guarded(SAI_API_ACL, __func__, [&]() -> sai_status_t {
        AclCounterObj* k = lookup(ctx().acl_counters, id);
        if (!k) return SAI_STATUS_INVALID_OBJECT_ID;
        if (n && !attrs) return SAI_STATUS_INVALID_PARAMETER;
        uint64_t p, b;
        counter_read(*k, p, b);
        for (uint32_t i = 0; i < n; i++) {
            sai_attribute_t& a = attrs[i];
            switch (a.id) {
            case SAI_ACL_COUNTER_ATTR_TABLE_ID: a.value.oid = k->table; break;
            case SAI_ACL_COUNTER_ATTR_ENABLE_PACKET_COUNT: a.value.booldata = k->packets; break;
            case SAI_ACL_COUNTER_ATTR_ENABLE_BYTE_COUNT: a.value.booldata = k->bytes; break;
            case SAI_ACL_COUNTER_ATTR_PACKETS: a.value.u64 = p; break;
            case SAI_ACL_COUNTER_ATTR_BYTES: a.value.u64 = b; break;
            default: return attr_not_supported(i);
            }
        }
        return SAI_STATUS_SUCCESS;
    });
}

// ------------------------------------------------------ table groups
static sai_status_t create_acl_table_group(_Out_ sai_object_id_t* id, _In_ sai_object_id_t, _In_ uint32_t n,
                                           _In_ const sai_attribute_t* attrs)
{
    return guarded(SAI_API_ACL, __func__, [&]() -> sai_status_t {
        auto& c = ctx();
        if (!id || (n && !attrs)) return SAI_STATUS_INVALID_PARAMETER;
        AclGroupObj g;
        bool hs = false;
        for (uint32_t i = 0; i < n; i++) {
            const sai_attribute_t& a = attrs[i];
            switch (a.id) {
            case SAI_ACL_TABLE_GROUP_ATTR_ACL_STAGE:
                if (a.value.s32 != SAI_ACL_STAGE_INGRESS && !(a.value.s32 == SAI_ACL_STAGE_EGRESS && fp_cap(FpCap::Egress)))
                    return attr_not_supported(i);
                g.stage = a.value.s32;
                hs = true;
                break;
            case SAI_ACL_TABLE_GROUP_ATTR_ACL_BIND_POINT_TYPE_LIST:
                // kept, not checked: portsorch passes an object list here, which a
                // big-endian sairedis reads as {0} (PORT) for every bind type
                if (a.value.s32list.count && !a.value.s32list.list) return attr_invalid(i);
                g.bind_types.assign(a.value.s32list.list, a.value.s32list.list + a.value.s32list.count);
                break;
            case SAI_ACL_TABLE_GROUP_ATTR_TYPE:
                if (a.value.s32 != SAI_ACL_TABLE_GROUP_TYPE_PARALLEL) return attr_not_supported(i);
                g.type = a.value.s32;
                break;
            default: return attr_not_supported(i);
            }
        }
        if (!hs) return SAI_STATUS_MANDATORY_ATTRIBUTE_MISSING;
        sai_object_id_t oid = c.alloc(SAI_OBJECT_TYPE_ACL_TABLE_GROUP);
        c.acl_groups[oid] = g;
        *id = oid;
        return SAI_STATUS_SUCCESS;
    });
}

static sai_status_t remove_acl_table_group(_In_ sai_object_id_t id)
{
    return guarded(SAI_API_ACL, __func__, [&]() -> sai_status_t {
        auto& c = ctx();
        AclGroupObj* g = lookup(c.acl_groups, id);
        if (!g) return SAI_STATUS_INVALID_OBJECT_ID;
        if (!g->members.empty() || acl_referenced(id)) return SAI_STATUS_OBJECT_IN_USE;
        c.acl_groups.erase(id);
        return SAI_STATUS_SUCCESS;
    });
}

static sai_status_t set_acl_table_group_attribute(_In_ sai_object_id_t id, _In_ const sai_attribute_t*)
{
    return guarded(SAI_API_ACL, __func__, [&]() -> sai_status_t {
        if (!ctx().acl_groups.count(id)) return SAI_STATUS_INVALID_OBJECT_ID;
        return attr_invalid(0);
    });
}

static sai_status_t get_acl_table_group_attribute(_In_ sai_object_id_t id, _In_ uint32_t n, _Inout_ sai_attribute_t* attrs)
{
    return guarded(SAI_API_ACL, __func__, [&]() -> sai_status_t {
        AclGroupObj* g = lookup(ctx().acl_groups, id);
        if (!g) return SAI_STATUS_INVALID_OBJECT_ID;
        if (n && !attrs) return SAI_STATUS_INVALID_PARAMETER;
        for (uint32_t i = 0; i < n; i++) {
            sai_attribute_t& a = attrs[i];
            sai_status_t st = SAI_STATUS_SUCCESS;
            switch (a.id) {
            case SAI_ACL_TABLE_GROUP_ATTR_ACL_STAGE: a.value.s32 = g->stage; break;
            case SAI_ACL_TABLE_GROUP_ATTR_ACL_BIND_POINT_TYPE_LIST: st = fill_list(a.value.s32list, g->bind_types); break;
            case SAI_ACL_TABLE_GROUP_ATTR_TYPE: a.value.s32 = g->type; break;
            case SAI_ACL_TABLE_GROUP_ATTR_MEMBER_LIST: st = fill_list(a.value.objlist, g->members); break;
            default: return attr_not_supported(i);
            }
            if (st != SAI_STATUS_SUCCESS) return st;
        }
        return SAI_STATUS_SUCCESS;
    });
}

static sai_status_t create_acl_table_group_member(_Out_ sai_object_id_t* id, _In_ sai_object_id_t, _In_ uint32_t n,
                                                  _In_ const sai_attribute_t* attrs)
{
    return guarded(SAI_API_ACL, __func__, [&]() -> sai_status_t {
        auto& c = ctx();
        if (!id || (n && !attrs)) return SAI_STATUS_INVALID_PARAMETER;
        AclGroupMemberObj m;
        for (uint32_t i = 0; i < n; i++) {
            const sai_attribute_t& a = attrs[i];
            switch (a.id) {
            case SAI_ACL_TABLE_GROUP_MEMBER_ATTR_ACL_TABLE_GROUP_ID:
                if (!c.acl_groups.count(a.value.oid)) return attr_invalid(i);
                m.group = a.value.oid;
                break;
            case SAI_ACL_TABLE_GROUP_MEMBER_ATTR_ACL_TABLE_ID:
                if (!c.acl_tables.count(a.value.oid)) return attr_invalid(i);
                m.table = a.value.oid;
                break;
            case SAI_ACL_TABLE_GROUP_MEMBER_ATTR_PRIORITY: m.priority = a.value.u32; break;
            default: return attr_not_supported(i);
            }
        }
        if (!m.group || !m.table) return SAI_STATUS_MANDATORY_ATTRIBUTE_MISSING;
        for (auto& o : c.acl_group_members)
            if (o.second.group == m.group && o.second.table == m.table) return SAI_STATUS_ITEM_ALREADY_EXISTS;
        sai_object_id_t oid = c.alloc(SAI_OBJECT_TYPE_ACL_TABLE_GROUP_MEMBER);
        c.acl_group_members[oid] = m;
        c.acl_groups[m.group].members.push_back(oid);
        *id = oid;
        acl_reprogram();
        return SAI_STATUS_SUCCESS;
    });
}

static sai_status_t remove_acl_table_group_member(_In_ sai_object_id_t id)
{
    return guarded(SAI_API_ACL, __func__, [&]() -> sai_status_t {
        auto& c = ctx();
        AclGroupMemberObj* m = lookup(c.acl_group_members, id);
        if (!m) return SAI_STATUS_INVALID_OBJECT_ID;
        if (AclGroupObj* g = lookup(c.acl_groups, m->group))
            g->members.erase(std::remove(g->members.begin(), g->members.end(), id), g->members.end());
        c.acl_group_members.erase(id);
        acl_reprogram();
        return SAI_STATUS_SUCCESS;
    });
}

static sai_status_t set_acl_table_group_member_attribute(_In_ sai_object_id_t id, _In_ const sai_attribute_t*)
{
    return guarded(SAI_API_ACL, __func__, [&]() -> sai_status_t {
        if (!ctx().acl_group_members.count(id)) return SAI_STATUS_INVALID_OBJECT_ID;
        return attr_invalid(0);
    });
}

static sai_status_t get_acl_table_group_member_attribute(_In_ sai_object_id_t id, _In_ uint32_t n,
                                                         _Inout_ sai_attribute_t* attrs)
{
    return guarded(SAI_API_ACL, __func__, [&]() -> sai_status_t {
        AclGroupMemberObj* m = lookup(ctx().acl_group_members, id);
        if (!m) return SAI_STATUS_INVALID_OBJECT_ID;
        if (n && !attrs) return SAI_STATUS_INVALID_PARAMETER;
        for (uint32_t i = 0; i < n; i++) {
            switch (attrs[i].id) {
            case SAI_ACL_TABLE_GROUP_MEMBER_ATTR_ACL_TABLE_GROUP_ID: attrs[i].value.oid = m->group; break;
            case SAI_ACL_TABLE_GROUP_MEMBER_ATTR_ACL_TABLE_ID: attrs[i].value.oid = m->table; break;
            case SAI_ACL_TABLE_GROUP_MEMBER_ATTR_PRIORITY: attrs[i].value.u32 = m->priority; break;
            default: return attr_not_supported(i);
            }
        }
        return SAI_STATUS_SUCCESS;
    });
}

// ------------------------------------------------------------ binding
sai_status_t acl_bind_set(sai_object_id_t obj, bool ingress, sai_object_id_t acl, uint32_t i)
{
    auto& c = ctx();
    if (!ingress && !fp_cap(FpCap::Egress))
        return acl == SAI_NULL_OBJECT_ID ? SAI_STATUS_SUCCESS : attr_not_supported(i);   // no egress ACL
    if (acl != SAI_NULL_OBJECT_ID && !c.acl_groups.count(acl) && !c.acl_tables.count(acl)) return attr_invalid(i);
    if (acl != SAI_NULL_OBJECT_ID) {   // the ACL's stage must be the binding's
        int32_t want = ingress ? SAI_ACL_STAGE_INGRESS : SAI_ACL_STAGE_EGRESS;
        const AclGroupObj* g = lookup(c.acl_groups, acl);
        const AclTableObj* t = lookup(c.acl_tables, acl);
        if ((g && g->stage != want) || (t && t->stage != want)) return attr_invalid(i);
    }
    auto& binds = ingress ? c.acl_bind : c.acl_bind_egress;
    if (acl == SAI_NULL_OBJECT_ID) binds.erase(obj);
    else binds[obj] = acl;
    acl_reprogram();
    return SAI_STATUS_SUCCESS;
}

sai_object_id_t acl_bind_get(sai_object_id_t obj, bool ingress)
{
    const auto& binds = ingress ? ctx().acl_bind : ctx().acl_bind_egress;
    auto it = binds.find(obj);
    return it == binds.end() ? SAI_NULL_OBJECT_ID : it->second;
}

void acl_clear_objects()
{
    auto& c = ctx();
    c.policers.clear(); c.acl_tables.clear(); c.acl_entries.clear(); c.acl_counters.clear();
    c.acl_groups.clear(); c.acl_group_members.clear(); c.acl_bind.clear(); c.copp.clear();
    c.acl_bind_egress.clear(); c.acl_ranges.clear();
    c.acl_next_prio = 1;
    c.copp_gid = -1;
    c.copp_dst_cpu = false;
}

// ------------------------------------------------------------ ranges
// L4 port ranges: only with a backend that has them (FpCap::Ranges); an
// entry's FIELD_ACL_RANGE_TYPE lists them (FpMatch l4_src/dst_range).
static sai_status_t create_acl_range(_Out_ sai_object_id_t* id, _In_ sai_object_id_t, _In_ uint32_t n,
                                     _In_ const sai_attribute_t* attrs)
{
    return guarded(SAI_API_ACL, __func__, [&]() -> sai_status_t {
        auto& c = ctx();
        if (!fp_cap(FpCap::Ranges)) return SAI_STATUS_NOT_SUPPORTED;
        if (!id || (n && !attrs)) return SAI_STATUS_INVALID_PARAMETER;
        AclRangeObj r;
        bool ht = false, hl = false;
        for (uint32_t i = 0; i < n; i++) {
            switch (attrs[i].id) {
            case SAI_ACL_RANGE_ATTR_TYPE:
                if (attrs[i].value.s32 != SAI_ACL_RANGE_TYPE_L4_SRC_PORT_RANGE &&
                    attrs[i].value.s32 != SAI_ACL_RANGE_TYPE_L4_DST_PORT_RANGE)
                    return attr_not_supported(i);
                r.type = attrs[i].value.s32;
                ht = true;
                break;
            case SAI_ACL_RANGE_ATTR_LIMIT:
                r.min = attrs[i].value.u32range.min;
                r.max = attrs[i].value.u32range.max;
                if (r.min > r.max || r.max > 0xffff) return attr_invalid(i);
                hl = true;
                break;
            default: return attr_not_supported(i);
            }
        }
        if (!ht || !hl) return SAI_STATUS_MANDATORY_ATTRIBUTE_MISSING;
        sai_object_id_t oid = c.alloc(SAI_OBJECT_TYPE_ACL_RANGE);
        c.acl_ranges[oid] = r;
        *id = oid;
        return SAI_STATUS_SUCCESS;
    });
}
static sai_status_t remove_acl_range(_In_ sai_object_id_t id)
{
    return guarded(SAI_API_ACL, __func__, [&]() -> sai_status_t {
        auto& c = ctx();
        if (!c.acl_ranges.count(id)) return SAI_STATUS_INVALID_OBJECT_ID;
        for (auto& e : c.acl_entries) {
            auto f = e.second.fields.find(SAI_ACL_ENTRY_ATTR_FIELD_ACL_RANGE_TYPE);
            if (f != e.second.fields.end() && std::count(f->second.objs.begin(), f->second.objs.end(), id))
                return SAI_STATUS_OBJECT_IN_USE;
        }
        c.acl_ranges.erase(id);
        return SAI_STATUS_SUCCESS;
    });
}
static sai_status_t set_acl_range_attribute(_In_ sai_object_id_t id, _In_ const sai_attribute_t*)
{
    return guarded(SAI_API_ACL, __func__, [&]() -> sai_status_t {
        if (!ctx().acl_ranges.count(id)) return SAI_STATUS_INVALID_OBJECT_ID;
        return attr_invalid(0);   // create only
    });
}
static sai_status_t get_acl_range_attribute(_In_ sai_object_id_t id, _In_ uint32_t n, _Inout_ sai_attribute_t* attrs)
{
    return guarded(SAI_API_ACL, __func__, [&]() -> sai_status_t {
        AclRangeObj* r = lookup(ctx().acl_ranges, id);
        if (!r) return SAI_STATUS_INVALID_OBJECT_ID;
        if (n && !attrs) return SAI_STATUS_INVALID_PARAMETER;
        for (uint32_t i = 0; i < n; i++) {
            switch (attrs[i].id) {
            case SAI_ACL_RANGE_ATTR_TYPE: attrs[i].value.s32 = r->type; break;
            case SAI_ACL_RANGE_ATTR_LIMIT: attrs[i].value.u32range.min = r->min; attrs[i].value.u32range.max = r->max; break;
            default: return attr_not_supported(i);
            }
        }
        return SAI_STATUS_SUCCESS;
    });
}
static sai_status_t create_chain_group(_Out_ sai_object_id_t*, _In_ sai_object_id_t, _In_ uint32_t, _In_ const sai_attribute_t*)
{
    return SAI_STATUS_NOT_SUPPORTED;
}
static sai_status_t remove_chain_group(_In_ sai_object_id_t) { return SAI_STATUS_INVALID_OBJECT_ID; }
static sai_status_t set_chain_group(_In_ sai_object_id_t, _In_ const sai_attribute_t*) { return SAI_STATUS_INVALID_OBJECT_ID; }
static sai_status_t get_chain_group(_In_ sai_object_id_t, _In_ uint32_t, _Inout_ sai_attribute_t*) { return SAI_STATUS_INVALID_OBJECT_ID; }

static sai_acl_api_t make_acl_api()
{
    sai_acl_api_t a;
    std::memset(&a, 0, sizeof(a));
    a.create_acl_table = create_acl_table;
    a.remove_acl_table = remove_acl_table;
    a.set_acl_table_attribute = set_acl_table_attribute;
    a.get_acl_table_attribute = get_acl_table_attribute;
    a.create_acl_entry = create_acl_entry;
    a.remove_acl_entry = remove_acl_entry;
    a.set_acl_entry_attribute = set_acl_entry_attribute;
    a.get_acl_entry_attribute = get_acl_entry_attribute;
    a.create_acl_counter = create_acl_counter;
    a.remove_acl_counter = remove_acl_counter;
    a.set_acl_counter_attribute = set_acl_counter_attribute;
    a.get_acl_counter_attribute = get_acl_counter_attribute;
    a.create_acl_range = create_acl_range;
    a.remove_acl_range = remove_acl_range;
    a.set_acl_range_attribute = set_acl_range_attribute;
    a.get_acl_range_attribute = get_acl_range_attribute;
    a.create_acl_table_group = create_acl_table_group;
    a.remove_acl_table_group = remove_acl_table_group;
    a.set_acl_table_group_attribute = set_acl_table_group_attribute;
    a.get_acl_table_group_attribute = get_acl_table_group_attribute;
    a.create_acl_table_group_member = create_acl_table_group_member;
    a.remove_acl_table_group_member = remove_acl_table_group_member;
    a.set_acl_table_group_member_attribute = set_acl_table_group_member_attribute;
    a.get_acl_table_group_member_attribute = get_acl_table_group_member_attribute;
    a.create_acl_table_chain_group = create_chain_group;
    a.remove_acl_table_chain_group = remove_chain_group;
    a.set_acl_table_chain_group_attribute = set_chain_group;
    a.get_acl_table_chain_group_attribute = get_chain_group;
    return a;
}
const sai_acl_api_t acl_api = make_acl_api();

} // namespace ob
