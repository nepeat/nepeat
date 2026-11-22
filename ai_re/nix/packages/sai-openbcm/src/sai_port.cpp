// SPDX-License-Identifier: Apache-2.0
// PORT objects (QUEUE, INGRESS_PRIORITY_GROUP and the port QoS attributes:
// sai_qos.cpp; mirror / sampling attributes: sai_mirror.cpp). Ports and their lanes come
// from the backend at switch creation; ports cannot be created or removed.
#include "sai_internal.h"

#include <algorithm>
#include <cstdlib>

namespace ob {

static const StubDef port_stubs[] = {
    {SAI_PORT_ATTR_MEDIA_TYPE, Kind::S32, SAI_PORT_MEDIA_TYPE_NOT_PRESENT, false},
    {SAI_PORT_ATTR_INTERFACE_TYPE, Kind::S32, SAI_PORT_INTERFACE_TYPE_NONE, false},
    {SAI_PORT_ATTR_DEFAULT_VLAN_PRIORITY, Kind::U8, 0, false},
    {SAI_PORT_ATTR_DROP_UNTAGGED, Kind::Bool, 0, false},
    {SAI_PORT_ATTR_DROP_TAGGED, Kind::Bool, 0, false},
    {SAI_PORT_ATTR_LINK_TRAINING_ENABLE, Kind::Bool, 0, false},
    {SAI_PORT_ATTR_GLOBAL_FLOW_CONTROL_MODE, Kind::S32, SAI_PORT_FLOW_CONTROL_MODE_DISABLE, false},
    {SAI_PORT_ATTR_PRIORITY_FLOW_CONTROL, Kind::U8, 0, false},
    {SAI_PORT_ATTR_UPDATE_DSCP, Kind::Bool, 0, false},
    {SAI_PORT_ATTR_FULL_DUPLEX_MODE, Kind::Bool, 1, false},
    {SAI_PORT_ATTR_IPG, Kind::U32, 96, false},
    {SAI_PORT_ATTR_PKT_TX_ENABLE, Kind::Bool, 1, false},
    {SAI_PORT_ATTR_HOST_TX_SIGNAL_ENABLE, Kind::Bool, 0, false},
    {SAI_PORT_ATTR_AUTO_NEG_FEC_MODE_OVERRIDE, Kind::Bool, 0, false},
};
static constexpr size_t N_PORT_STUBS = sizeof(port_stubs) / sizeof(port_stubs[0]);

// ------------------------------------------------------------------- stats
static bool map_stat(sai_stat_id_t id, PortStat& s)
{
    switch (id) {
    case SAI_PORT_STAT_IF_IN_OCTETS: s = PortStat::InOctets; return true;
    case SAI_PORT_STAT_IF_IN_UCAST_PKTS: s = PortStat::InUcastPkts; return true;
    case SAI_PORT_STAT_IF_IN_NON_UCAST_PKTS: s = PortStat::InNonUcastPkts; return true;
    case SAI_PORT_STAT_IF_IN_DISCARDS: s = PortStat::InDiscards; return true;
    case SAI_PORT_STAT_IF_IN_ERRORS: s = PortStat::InErrors; return true;
    case SAI_PORT_STAT_IF_IN_UNKNOWN_PROTOS: s = PortStat::InUnknownProtos; return true;
    case SAI_PORT_STAT_IF_IN_BROADCAST_PKTS: s = PortStat::InBroadcastPkts; return true;
    case SAI_PORT_STAT_IF_IN_MULTICAST_PKTS: s = PortStat::InMulticastPkts; return true;
    case SAI_PORT_STAT_IF_OUT_OCTETS: s = PortStat::OutOctets; return true;
    case SAI_PORT_STAT_IF_OUT_UCAST_PKTS: s = PortStat::OutUcastPkts; return true;
    case SAI_PORT_STAT_IF_OUT_NON_UCAST_PKTS: s = PortStat::OutNonUcastPkts; return true;
    case SAI_PORT_STAT_IF_OUT_DISCARDS: s = PortStat::OutDiscards; return true;
    case SAI_PORT_STAT_IF_OUT_ERRORS: s = PortStat::OutErrors; return true;
    case SAI_PORT_STAT_IF_OUT_BROADCAST_PKTS: s = PortStat::OutBroadcastPkts; return true;
    case SAI_PORT_STAT_IF_OUT_MULTICAST_PKTS: s = PortStat::OutMulticastPkts; return true;
    default: return false;
    }
}

bool port_stat_supported(sai_stat_id_t id)
{
    PortStat s;
    int prio;
    bool tx;
    if (ctx().be && ctx().be->port_stat_by_id(id)) return true;
    if (port_pfc_stat(id, prio, tx)) return ctx().be && ctx().be->qos_supported();
    return map_stat(id, s);
}

sai_status_t port_stats_get(int hw, uint32_t n, const sai_stat_id_t* ids, uint64_t* out)
{
    // Ids the backend reads by SAI id: one call for all of them.
    std::vector<sai_stat_id_t> by_id;
    std::vector<uint32_t> at;
    for (uint32_t i = 0; i < n; i++)
        if (ctx().be->port_stat_by_id(ids[i])) { by_id.push_back(ids[i]); at.push_back(i); }
    if (!by_id.empty()) {
        std::vector<uint64_t> v(by_id.size());
        sai_status_t st = ctx().be->port_stats_by_id(hw, (uint32_t)by_id.size(), by_id.data(), v.data());
        if (st != SAI_STATUS_SUCCESS) return st;
        for (size_t k = 0; k < at.size(); k++) out[at[k]] = v[k];
    }
    for (uint32_t i = 0; i < n; i++) {
        if (!by_id.empty() && ctx().be->port_stat_by_id(ids[i])) continue;
        PortStat s;
        int prio;
        bool tx;
        if (port_pfc_stat(ids[i], prio, tx)) {   // PFC frames: a QoS call
            if (!ctx().be->qos_supported()) return SAI_STATUS_NOT_SUPPORTED;
            sai_status_t st = ctx().be->port_pfc_stat_get(hw, prio, tx, out[i]);
            if (st != SAI_STATUS_SUCCESS) return st;
            continue;
        }
        if (!map_stat(ids[i], s)) return SAI_STATUS_NOT_SUPPORTED;
        sai_status_t st = ctx().be->port_stat_get(hw, s, out[i]);
        if (st != SAI_STATUS_SUCCESS) return st;
    }
    return SAI_STATUS_SUCCESS;
}

// ----------------------------------------------------------- board quirks
sai_status_t port_quirks_parse(const BackendConfig& cfg, PortQuirks& q)
{
    q = PortQuirks();
    std::string v = cfg.get("SAI_OPENBCM_PORT_INTERFACE");
    if (!v.empty()) {
        const auto& names = port_interface_names();
        if (std::find(names.begin(), names.end(), v) == names.end()) {
            OB_ERR(SAI_API_SWITCH, "SAI_OPENBCM_PORT_INTERFACE '%s': unknown interface type", v.c_str());
            return SAI_STATUS_INVALID_PARAMETER;
        }
        q.interface = v;
    }
    v = cfg.get("SAI_OPENBCM_PORT_FEC");
    if (v == "none") q.fec = (int)Fec::None;
    else if (v == "rs") q.fec = (int)Fec::Rs;
    else if (v == "fc") q.fec = (int)Fec::Fc;
    else if (!v.empty()) {
        OB_ERR(SAI_API_SWITCH, "SAI_OPENBCM_PORT_FEC '%s': none|rs|fc", v.c_str());
        return SAI_STATUS_INVALID_PARAMETER;
    }
    v = cfg.get("SAI_OPENBCM_PORT_AUTONEG");
    if (v == "off") q.autoneg = 0;
    else if (v == "on") q.autoneg = 1;
    else if (!v.empty()) {
        OB_ERR(SAI_API_SWITCH, "SAI_OPENBCM_PORT_AUTONEG '%s': off|on", v.c_str());
        return SAI_STATUS_INVALID_PARAMETER;
    }
    v = cfg.get("SAI_OPENBCM_PORT_IFG");
    if (!v.empty()) {
        char* end = nullptr;
        long n = std::strtol(v.c_str(), &end, 10);
        if (*end || n < 64 || n > 504 || n % 8) {
            OB_ERR(SAI_API_SWITCH, "SAI_OPENBCM_PORT_IFG '%s': bit times, a multiple of 8 in 64..504", v.c_str());
            return SAI_STATUS_INVALID_PARAMETER;
        }
        q.ifg = (uint32_t)n;
    }
    if (!q.interface.empty() || q.fec >= 0 || q.autoneg >= 0 || q.ifg)
        OB_NOTICE(SAI_API_SWITCH, "port quirks: interface %s, ASIC FEC %s, autoneg %s, IFG %s%s",
                  q.interface.empty() ? "(SDK)" : q.interface.c_str(),
                  q.fec < 0 ? "from SAI" : q.fec == (int)Fec::Rs ? "fixed rs" : q.fec == (int)Fec::Fc ? "fixed fc" : "fixed none",
                  q.autoneg < 0 ? "from SAI" : q.autoneg ? "fixed on" : "fixed off",
                  q.ifg ? std::to_string(q.ifg).c_str() : "(SDK)", q.ifg ? " bits" : "");
    return SAI_STATUS_SUCCESS;
}

// Order as the diag shell's "port ... an=off if=... " does it: autoneg,
// interface, then FEC and the inter-frame gap.
void port_quirks_apply(PortObj& p)
{
    PortQuirks& q = ctx().port_quirks;
    Backend& be = *ctx().be;
    auto check = [&](const char* what, sai_status_t st) {
        if (st == SAI_STATUS_SUCCESS) return;
        OB_ERR(SAI_API_PORT, "port %s: quirk %s not applied (%d)", p.sdk_name.c_str(), what, st);
        q.failed = true;
    };
    if (p.cpu) return;
    if (q.autoneg >= 0) check("autoneg", be.port_autoneg_set(p.hw, q.autoneg != 0));
    if (!q.interface.empty()) check("interface", be.port_interface_set(p.hw, q.interface));
    if (q.fec >= 0) check("fec", be.port_fec_set(p.hw, (Fec)q.fec));
    if (q.ifg) check("ifg", be.port_ifg_set(p.hw, q.ifg));
}

// ------------------------------------------------------------------- ports
static PortObj* port_of(sai_object_id_t oid)
{
    if (oid_type(oid) != SAI_OBJECT_TYPE_PORT) return nullptr;
    return lookup(ctx().ports, oid);
}

static Fec to_fec(int32_t m) { return m == SAI_PORT_FEC_MODE_RS ? Fec::Rs : m == SAI_PORT_FEC_MODE_FC ? Fec::Fc : Fec::None; }

static sai_status_t port_set_one(PortObj& p, const sai_attribute_t& a, uint32_t i)
{
    Backend& be = *ctx().be;
    sai_status_t st;
    if (p.cpu) return attr_not_supported(i);
    switch (a.id) {
    case SAI_PORT_ATTR_ADMIN_STATE:
        st = be.port_admin_set(p.hw, a.value.booldata);
        if (st == SAI_STATUS_SUCCESS) {
            p.admin = a.value.booldata;
            OB_NOTICE(SAI_API_PORT, "port %s admin %s", p.sdk_name.c_str(), p.admin ? "up" : "down");
        }
        return attr_backend_status(st, i);
    case SAI_PORT_ATTR_SPEED:
        if (a.value.u32 == 0) return attr_invalid(i);
        st = be.port_speed_set(p.hw, a.value.u32);
        // A speed change re-does the MAC/SerDes setup: board quirks again.
        if (st == SAI_STATUS_SUCCESS) port_quirks_apply(p);
        return attr_backend_status(st, i);
    case SAI_PORT_ATTR_MTU:
        if (a.value.u32 < 68 || a.value.u32 > 9408) return attr_invalid(i);
        // SAI MTU excludes FCS and VLAN tag; the SDK limit is the full frame.
        st = be.port_frame_max_set(p.hw, a.value.u32 + 8);
        if (st == SAI_STATUS_SUCCESS) p.mtu = a.value.u32;
        return attr_backend_status(st, i);
    case SAI_PORT_ATTR_FEC_MODE:
        if (a.value.s32 != SAI_PORT_FEC_MODE_NONE && a.value.s32 != SAI_PORT_FEC_MODE_RS &&
            a.value.s32 != SAI_PORT_FEC_MODE_FC)
            return attr_invalid(i);
        if (ctx().port_quirks.fec >= 0) {   // FEC is another chip's job on this board: kept, not programmed
            p.fec = a.value.s32;
            OB_INFO(SAI_API_PORT, "port %s FEC mode %d kept (SAI_OPENBCM_PORT_FEC fixes the ASIC side)",
                    p.sdk_name.c_str(), a.value.s32);
            return SAI_STATUS_SUCCESS;
        }
        st = be.port_fec_set(p.hw, to_fec(a.value.s32));
        if (st == SAI_STATUS_SUCCESS) p.fec = a.value.s32;
        return attr_backend_status(st, i);
    case SAI_PORT_ATTR_AUTO_NEG_MODE:
        if (ctx().port_quirks.autoneg >= 0) {
            p.autoneg = a.value.booldata;
            OB_INFO(SAI_API_PORT, "port %s autoneg %d kept (SAI_OPENBCM_PORT_AUTONEG fixes the ASIC side)",
                    p.sdk_name.c_str(), (int)a.value.booldata);
            return SAI_STATUS_SUCCESS;
        }
        st = be.port_autoneg_set(p.hw, a.value.booldata);
        if (st == SAI_STATUS_SUCCESS) p.autoneg = a.value.booldata;
        return attr_backend_status(st, i);
    case SAI_PORT_ATTR_PORT_VLAN_ID:
        if (a.value.u16 < 1 || a.value.u16 > 4094) return attr_invalid(i);
        st = be.port_pvid_set(p.hw, a.value.u16);
        if (st == SAI_STATUS_SUCCESS) p.pvid = a.value.u16;
        return attr_backend_status(st, i);
    case SAI_PORT_ATTR_LOOPBACK_MODE: {
        Loopback lb;
        if (a.value.s32 == SAI_PORT_LOOPBACK_MODE_NONE) lb = Loopback::None;
        else if (a.value.s32 == SAI_PORT_LOOPBACK_MODE_MAC) lb = Loopback::Mac;
        else if (a.value.s32 == SAI_PORT_LOOPBACK_MODE_PHY) lb = Loopback::Phy;
        else return attr_invalid(i);
        st = be.port_loopback_set(p.hw, lb);
        if (st == SAI_STATUS_SUCCESS) p.loopback = a.value.s32;
        return attr_backend_status(st, i);
    }
    case SAI_PORT_ATTR_INGRESS_ACL:
    case SAI_PORT_ATTR_EGRESS_ACL:
        if (p.cpu) return attr_not_supported(i);
        return acl_bind_set(p.oid, a.id == SAI_PORT_ATTR_INGRESS_ACL, a.value.oid, i);
    default:
        break;
    }
    bool handled = false;
    st = port_qos_set(p, a, i, handled);
    if (handled) return st;
    st = port_storm_attr_set(p, a, i, handled);
    if (handled) return st;
    st = port_mirror_set(p, a, i, handled);
    if (handled) return st;
    if (find_stub(port_stubs, N_PORT_STUBS, a.id)) {
        p.stub[a.id] = a.value;
        OB_INFO(SAI_API_PORT, "port %s attr %d stored (not programmed)", p.sdk_name.c_str(), a.id);
        return SAI_STATUS_SUCCESS;
    }
    OB_WARN(SAI_API_PORT, "port attr %d not supported", a.id);
    return attr_not_supported(i);
}

static sai_status_t port_get_one(PortObj& p, sai_attribute_t& a, uint32_t i)
{
    Backend& be = *ctx().be;
    switch (a.id) {
    case SAI_PORT_ATTR_TYPE: a.value.s32 = p.cpu ? SAI_PORT_TYPE_CPU : SAI_PORT_TYPE_LOGICAL; return SAI_STATUS_SUCCESS;
    case SAI_PORT_ATTR_HW_LANE_LIST: return fill_list(a.value.u32list, p.lanes);
    case SAI_PORT_ATTR_ADMIN_STATE: a.value.booldata = p.admin; return SAI_STATUS_SUCCESS;
    case SAI_PORT_ATTR_OPER_STATUS: {
        if (p.cpu) { a.value.s32 = SAI_PORT_OPER_STATUS_UP; return SAI_STATUS_SUCCESS; }
        bool up = false;
        sai_status_t st = be.port_link_get(p.hw, up);
        if (st != SAI_STATUS_SUCCESS) return st;
        a.value.s32 = up ? SAI_PORT_OPER_STATUS_UP : SAI_PORT_OPER_STATUS_DOWN;
        return SAI_STATUS_SUCCESS;
    }
    case SAI_PORT_ATTR_SPEED:
    case SAI_PORT_ATTR_OPER_SPEED: {
        if (p.cpu) return attr_not_supported(i);
        uint32_t s = 0;
        sai_status_t st = be.port_speed_get(p.hw, s);
        if (st != SAI_STATUS_SUCCESS) return st;
        a.value.u32 = s;
        return SAI_STATUS_SUCCESS;
    }
    case SAI_PORT_ATTR_SUPPORTED_SPEED: {
        if (p.cpu) return attr_not_supported(i);
        std::vector<uint32_t> v;
        sai_status_t st = be.port_supported_speeds(p.hw, v);
        if (st != SAI_STATUS_SUCCESS) return st;
        return fill_list(a.value.u32list, v);
    }
    case SAI_PORT_ATTR_MTU: a.value.u32 = p.mtu; return SAI_STATUS_SUCCESS;
    case SAI_PORT_ATTR_FEC_MODE:
    case SAI_PORT_ATTR_OPER_PORT_FEC_MODE: a.value.s32 = p.fec; return SAI_STATUS_SUCCESS;
    case SAI_PORT_ATTR_SUPPORTED_FEC_MODE: {
        std::vector<int32_t> v = {SAI_PORT_FEC_MODE_NONE, SAI_PORT_FEC_MODE_RS, SAI_PORT_FEC_MODE_FC};
        return fill_list(a.value.s32list, v);
    }
    case SAI_PORT_ATTR_AUTO_NEG_MODE: a.value.booldata = p.autoneg; return SAI_STATUS_SUCCESS;
    case SAI_PORT_ATTR_SUPPORTED_AUTO_NEG_MODE: a.value.booldata = !p.cpu; return SAI_STATUS_SUCCESS;
    case SAI_PORT_ATTR_PORT_VLAN_ID: a.value.u16 = p.pvid; return SAI_STATUS_SUCCESS;
    case SAI_PORT_ATTR_LOOPBACK_MODE: a.value.s32 = p.loopback; return SAI_STATUS_SUCCESS;
    case SAI_PORT_ATTR_QOS_NUMBER_OF_QUEUES: a.value.u32 = (uint32_t)p.queues.size(); return SAI_STATUS_SUCCESS;
    case SAI_PORT_ATTR_QOS_QUEUE_LIST: return fill_list(a.value.objlist, p.queues);
    case SAI_PORT_ATTR_NUMBER_OF_INGRESS_PRIORITY_GROUPS: a.value.u32 = (uint32_t)p.ipgs.size(); return SAI_STATUS_SUCCESS;
    case SAI_PORT_ATTR_INGRESS_PRIORITY_GROUP_LIST: return fill_list(a.value.objlist, p.ipgs);
    case SAI_PORT_ATTR_INGRESS_ACL:
    case SAI_PORT_ATTR_EGRESS_ACL: a.value.oid = acl_bind_get(p.oid, a.id == SAI_PORT_ATTR_INGRESS_ACL); return SAI_STATUS_SUCCESS;
    default:
        break;
    }
    bool handled = false;
    sai_status_t qst = port_qos_get(p, a, i, handled);
    if (handled) return qst;
    qst = port_storm_attr_get(p, a, handled);
    if (handled) return qst;
    qst = port_mirror_get(p, a, i, handled);
    if (handled) return qst;
    if (const StubDef* d = find_stub(port_stubs, N_PORT_STUBS, a.id)) {
        auto it = p.stub.find(a.id);
        if (it != p.stub.end()) a.value = it->second;
        else stub_default(*d, a.value);
        return SAI_STATUS_SUCCESS;
    }
    return attr_not_supported(i);
}

static sai_status_t create_port(_Out_ sai_object_id_t*, _In_ sai_object_id_t, _In_ uint32_t, _In_ const sai_attribute_t*)
{
    OB_ERR(SAI_API_PORT, "dynamic port creation (breakout) is not supported; ports come from the SDK config");
    return SAI_STATUS_NOT_SUPPORTED;
}

static sai_status_t remove_port(_In_ sai_object_id_t)
{
    OB_ERR(SAI_API_PORT, "port removal is not supported");
    return SAI_STATUS_NOT_SUPPORTED;
}

static sai_status_t set_port_attribute(_In_ sai_object_id_t port_id, _In_ const sai_attribute_t* attr)
{
    return guarded(SAI_API_PORT, __func__, [&]() -> sai_status_t {
        PortObj* p = port_of(port_id);
        if (!p) return SAI_STATUS_INVALID_OBJECT_ID;
        if (!attr) return SAI_STATUS_INVALID_PARAMETER;
        return port_set_one(*p, *attr, 0);
    });
}

static sai_status_t get_port_attribute(_In_ sai_object_id_t port_id, _In_ uint32_t attr_count,
                                       _Inout_ sai_attribute_t* attr_list)
{
    return guarded(SAI_API_PORT, __func__, [&]() -> sai_status_t {
        PortObj* p = port_of(port_id);
        if (!p) return SAI_STATUS_INVALID_OBJECT_ID;
        if (attr_count && !attr_list) return SAI_STATUS_INVALID_PARAMETER;
        for (uint32_t i = 0; i < attr_count; i++) {
            sai_status_t st = port_get_one(*p, attr_list[i], i);
            if (st != SAI_STATUS_SUCCESS) return st;
        }
        return SAI_STATUS_SUCCESS;
    });
}

static sai_status_t get_port_stats(_In_ sai_object_id_t port_id, _In_ uint32_t n, _In_ const sai_stat_id_t* ids,
                                   _Out_ uint64_t* counters)
{
    return guarded(SAI_API_PORT, __func__, [&]() -> sai_status_t {
        PortObj* p = port_of(port_id);
        if (!p || p->cpu) return SAI_STATUS_INVALID_OBJECT_ID;
        if (n && (!ids || !counters)) return SAI_STATUS_INVALID_PARAMETER;
        return port_stats_get(p->hw, n, ids, counters);
    });
}

static sai_status_t get_port_stats_ext(_In_ sai_object_id_t port_id, _In_ uint32_t n, _In_ const sai_stat_id_t* ids,
                                       _In_ sai_stats_mode_t mode, _Out_ uint64_t* counters)
{
    if (mode != SAI_STATS_MODE_READ) return SAI_STATUS_NOT_SUPPORTED;   // no per-counter clear in the SDK API
    return get_port_stats(port_id, n, ids, counters);
}

static sai_status_t clear_port_stats(_In_ sai_object_id_t, _In_ uint32_t, _In_ const sai_stat_id_t*)
{
    return SAI_STATUS_NOT_SUPPORTED;
}

static sai_status_t clear_port_all_stats(_In_ sai_object_id_t port_id)
{
    return guarded(SAI_API_PORT, __func__, [&]() -> sai_status_t {
        PortObj* p = port_of(port_id);
        if (!p || p->cpu) return SAI_STATUS_INVALID_OBJECT_ID;
        return ctx().be->port_stat_clear(p->hw);
    });
}

static sai_port_api_t make_port_api()
{
    sai_port_api_t a;
    std::memset(&a, 0, sizeof(a));
    a.create_port = create_port;
    a.remove_port = remove_port;
    a.set_port_attribute = set_port_attribute;
    a.get_port_attribute = get_port_attribute;
    a.get_port_stats = get_port_stats;
    a.get_port_stats_ext = get_port_stats_ext;
    a.clear_port_stats = clear_port_stats;
    a.clear_port_all_stats = clear_port_all_stats;
    return a;
}
const sai_port_api_t port_api = make_port_api();

} // namespace ob
