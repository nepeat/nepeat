// SPDX-License-Identifier: Apache-2.0
// MIRROR_SESSION (SPAN = LOCAL, ERSPAN = ENHANCED_REMOTE with L3 GRE),
// SAMPLEPACKET, and the port attributes that use them (INGRESS/EGRESS
// _MIRROR_SESSION, INGRESS/EGRESS_SAMPLEPACKET_ENABLE). ACL entries mirror
// through FpRule::mirror_* (sai_acl.cpp). With a backend that has
// mirror_supported() every change goes through the mirror calls of
// backend.h; otherwise the objects are kept here only.
#include "sai_internal.h"

namespace ob {

static bool mirror_be() { return ctx().be && ctx().be->mirror_supported(); }

static MirrorObj* mirror_of(sai_object_id_t oid) { return lookup(ctx().mirrors, oid); }
bool mirror_valid(sai_object_id_t s) { return mirror_of(s) != nullptr; }
int mirror_backend_id(sai_object_id_t s)
{
    MirrorObj* m = mirror_of(s);
    return m ? m->id : -1;
}

static bool front_port(sai_object_id_t oid, int& hw)
{
    PortObj* p = oid_type(oid) == SAI_OBJECT_TYPE_PORT ? lookup(ctx().ports, oid) : nullptr;
    if (!p || p->cpu) return false;
    hw = p->hw;
    return true;
}

static sai_status_t mirror_attr(MirrorObj& m, const sai_attribute_t& a, uint32_t i, bool create)
{
    MirrorCfg& c = m.cfg;
    switch (a.id) {
    case SAI_MIRROR_SESSION_ATTR_TYPE:
        if (!create) return attr_invalid(i);
        if (a.value.s32 != SAI_MIRROR_SESSION_TYPE_LOCAL && a.value.s32 != SAI_MIRROR_SESSION_TYPE_ENHANCED_REMOTE)
            return attr_not_supported(i);   // RSPAN, sFlow-type sessions: not implemented
        m.type = a.value.s32;
        c.erspan = m.type == SAI_MIRROR_SESSION_TYPE_ENHANCED_REMOTE;
        return SAI_STATUS_SUCCESS;
    case SAI_MIRROR_SESSION_ATTR_MONITOR_PORT: {
        int hw;
        if (!front_port(a.value.oid, hw)) return attr_invalid(i);   // a LAG / the CPU: not as a monitor port
        m.monitor = a.value.oid;
        c.monitor = hw;
        return SAI_STATUS_SUCCESS;
    }
    case SAI_MIRROR_SESSION_ATTR_TRUNCATE_SIZE: c.truncate = a.value.u16; return SAI_STATUS_SUCCESS;
    case SAI_MIRROR_SESSION_ATTR_SAMPLE_RATE: c.sample_rate = a.value.u32; return SAI_STATUS_SUCCESS;
    case SAI_MIRROR_SESSION_ATTR_CONGESTION_MODE: m.congestion = a.value.s32; return SAI_STATUS_SUCCESS;
    case SAI_MIRROR_SESSION_ATTR_TC: m.tc = a.value.u8; return SAI_STATUS_SUCCESS;   // kept: mirror copies use the queue the SDK picks
    case SAI_MIRROR_SESSION_ATTR_VLAN_TPID: c.vlan_tpid = a.value.u16; return SAI_STATUS_SUCCESS;
    case SAI_MIRROR_SESSION_ATTR_VLAN_ID: c.vlan_id = a.value.u16; return SAI_STATUS_SUCCESS;
    case SAI_MIRROR_SESSION_ATTR_VLAN_PRI: c.vlan_pri = a.value.u8; return SAI_STATUS_SUCCESS;
    case SAI_MIRROR_SESSION_ATTR_VLAN_CFI: c.vlan_cfi = a.value.u8; return SAI_STATUS_SUCCESS;
    case SAI_MIRROR_SESSION_ATTR_VLAN_HEADER_VALID: c.vlan = a.value.booldata; return SAI_STATUS_SUCCESS;
    case SAI_MIRROR_SESSION_ATTR_ERSPAN_ENCAPSULATION_TYPE:
        if (!create) return attr_invalid(i);
        if (a.value.s32 != SAI_ERSPAN_ENCAPSULATION_TYPE_MIRROR_L3_GRE_TUNNEL) return attr_invalid(i);
        return SAI_STATUS_SUCCESS;
    case SAI_MIRROR_SESSION_ATTR_IPHDR_VERSION:
        if (a.value.u8 != 4 && a.value.u8 != 6) return attr_invalid(i);
        return SAI_STATUS_SUCCESS;   // follows the addresses
    case SAI_MIRROR_SESSION_ATTR_TOS: c.tos = a.value.u8; return SAI_STATUS_SUCCESS;
    case SAI_MIRROR_SESSION_ATTR_TTL: c.ttl = a.value.u8; return SAI_STATUS_SUCCESS;
    case SAI_MIRROR_SESSION_ATTR_SRC_IP_ADDRESS: c.src = to_ip(a.value.ipaddr); return SAI_STATUS_SUCCESS;
    case SAI_MIRROR_SESSION_ATTR_DST_IP_ADDRESS: c.dst = to_ip(a.value.ipaddr); return SAI_STATUS_SUCCESS;
    case SAI_MIRROR_SESSION_ATTR_SRC_MAC_ADDRESS: std::memcpy(c.smac, a.value.mac, 6); return SAI_STATUS_SUCCESS;
    case SAI_MIRROR_SESSION_ATTR_DST_MAC_ADDRESS: std::memcpy(c.dmac, a.value.mac, 6); return SAI_STATUS_SUCCESS;
    case SAI_MIRROR_SESSION_ATTR_GRE_PROTOCOL_TYPE: c.gre_type = a.value.u16; return SAI_STATUS_SUCCESS;
    case SAI_MIRROR_SESSION_ATTR_POLICER:
        if (a.value.oid && !policer_valid(a.value.oid)) return attr_invalid(i);
        m.policer = a.value.oid;   // kept: mirror copies are not policed
        return SAI_STATUS_SUCCESS;
    default:
        return attr_not_supported(i);
    }
}

static bool session_used(sai_object_id_t s)
{
    auto& c = ctx();
    for (auto& p : c.ports) {
        for (auto x : p.second.mirror_in) if (x == s) return true;
        for (auto x : p.second.mirror_eg) if (x == s) return true;
    }
    for (auto& e : c.acl_entries)
        for (auto& act : e.second.actions)
            if (act.first == SAI_ACL_ENTRY_ATTR_ACTION_MIRROR_INGRESS || act.first == SAI_ACL_ENTRY_ATTR_ACTION_MIRROR_EGRESS) {
                auto it = e.second.action_objs.find(act.first);
                if (it != e.second.action_objs.end())
                    for (auto x : it->second) if (x == s) return true;
            }
    return false;
}

static sai_status_t create_mirror_session(_Out_ sai_object_id_t* id, _In_ sai_object_id_t sw, _In_ uint32_t n,
                                          _In_ const sai_attribute_t* attrs)
{
    return guarded(SAI_API_MIRROR, __func__, [&]() -> sai_status_t {
        auto& c = ctx();
        if (!id || (n && !attrs)) return SAI_STATUS_INVALID_PARAMETER;
        if (sw != c.switch_oid) return SAI_STATUS_INVALID_OBJECT_ID;
        MirrorObj m;
        bool type = false;
        for (uint32_t i = 0; i < n; i++) {
            sai_status_t st = mirror_attr(m, attrs[i], i, true);
            if (st != SAI_STATUS_SUCCESS) return st;
            type |= attrs[i].id == SAI_MIRROR_SESSION_ATTR_TYPE;
        }
        if (!type || !m.monitor) return SAI_STATUS_MANDATORY_ATTRIBUTE_MISSING;
        if (mirror_be()) {
            sai_status_t st = c.be->mirror_create(m.cfg, m.id);
            if (st != SAI_STATUS_SUCCESS) return st;
        }
        *id = c.alloc(SAI_OBJECT_TYPE_MIRROR_SESSION);
        c.mirrors[*id] = m;
        OB_INFO(SAI_API_MIRROR, "mirror session 0x%lx %s monitor %d, backend id %d", (unsigned long)*id,
                m.cfg.erspan ? "ERSPAN" : "SPAN", m.cfg.monitor, m.id);
        return SAI_STATUS_SUCCESS;
    });
}

static sai_status_t remove_mirror_session(_In_ sai_object_id_t id)
{
    return guarded(SAI_API_MIRROR, __func__, [&]() -> sai_status_t {
        auto& c = ctx();
        MirrorObj* m = mirror_of(id);
        if (!m) return SAI_STATUS_INVALID_OBJECT_ID;
        if (session_used(id)) return SAI_STATUS_OBJECT_IN_USE;
        if (m->id >= 0) {
            sai_status_t st = c.be->mirror_destroy(m->id);
            if (st != SAI_STATUS_SUCCESS) return st;
        }
        c.mirrors.erase(id);
        return SAI_STATUS_SUCCESS;
    });
}

static sai_status_t set_mirror_session_attribute(_In_ sai_object_id_t id, _In_ const sai_attribute_t* a)
{
    return guarded(SAI_API_MIRROR, __func__, [&]() -> sai_status_t {
        MirrorObj* m = mirror_of(id);
        if (!m) return SAI_STATUS_INVALID_OBJECT_ID;
        if (!a) return SAI_STATUS_INVALID_PARAMETER;
        MirrorObj nm = *m;
        sai_status_t st = mirror_attr(nm, *a, 0, false);
        if (st != SAI_STATUS_SUCCESS) return st;
        if (m->id >= 0 && (st = ctx().be->mirror_update(m->id, nm.cfg)) != SAI_STATUS_SUCCESS)
            return attr_backend_status(st, 0);
        *m = nm;
        return SAI_STATUS_SUCCESS;
    });
}

static sai_status_t get_mirror_session_attribute(_In_ sai_object_id_t id, _In_ uint32_t n,
                                                 _Inout_ sai_attribute_t* attrs)
{
    return guarded(SAI_API_MIRROR, __func__, [&]() -> sai_status_t {
        MirrorObj* m = mirror_of(id);
        if (!m) return SAI_STATUS_INVALID_OBJECT_ID;
        if (n && !attrs) return SAI_STATUS_INVALID_PARAMETER;
        const MirrorCfg& c = m->cfg;
        for (uint32_t i = 0; i < n; i++) {
            sai_attribute_t& a = attrs[i];
            switch (a.id) {
            case SAI_MIRROR_SESSION_ATTR_TYPE: a.value.s32 = m->type; break;
            case SAI_MIRROR_SESSION_ATTR_MONITOR_PORT: a.value.oid = m->monitor; break;
            case SAI_MIRROR_SESSION_ATTR_TRUNCATE_SIZE: a.value.u16 = c.truncate; break;
            case SAI_MIRROR_SESSION_ATTR_SAMPLE_RATE: a.value.u32 = c.sample_rate; break;
            case SAI_MIRROR_SESSION_ATTR_CONGESTION_MODE: a.value.s32 = m->congestion; break;
            case SAI_MIRROR_SESSION_ATTR_TC: a.value.u8 = m->tc; break;
            case SAI_MIRROR_SESSION_ATTR_VLAN_TPID: a.value.u16 = c.vlan_tpid; break;
            case SAI_MIRROR_SESSION_ATTR_VLAN_ID: a.value.u16 = c.vlan_id; break;
            case SAI_MIRROR_SESSION_ATTR_VLAN_PRI: a.value.u8 = c.vlan_pri; break;
            case SAI_MIRROR_SESSION_ATTR_VLAN_CFI: a.value.u8 = c.vlan_cfi; break;
            case SAI_MIRROR_SESSION_ATTR_VLAN_HEADER_VALID: a.value.booldata = c.vlan; break;
            case SAI_MIRROR_SESSION_ATTR_ERSPAN_ENCAPSULATION_TYPE:
                a.value.s32 = SAI_ERSPAN_ENCAPSULATION_TYPE_MIRROR_L3_GRE_TUNNEL;
                break;
            case SAI_MIRROR_SESSION_ATTR_IPHDR_VERSION: a.value.u8 = c.dst.v6 ? 6 : 4; break;
            case SAI_MIRROR_SESSION_ATTR_TOS: a.value.u8 = c.tos; break;
            case SAI_MIRROR_SESSION_ATTR_TTL: a.value.u8 = c.ttl; break;
            case SAI_MIRROR_SESSION_ATTR_SRC_IP_ADDRESS: a.value.ipaddr = from_ip(c.src); break;
            case SAI_MIRROR_SESSION_ATTR_DST_IP_ADDRESS: a.value.ipaddr = from_ip(c.dst); break;
            case SAI_MIRROR_SESSION_ATTR_SRC_MAC_ADDRESS: std::memcpy(a.value.mac, c.smac, 6); break;
            case SAI_MIRROR_SESSION_ATTR_DST_MAC_ADDRESS: std::memcpy(a.value.mac, c.dmac, 6); break;
            case SAI_MIRROR_SESSION_ATTR_GRE_PROTOCOL_TYPE: a.value.u16 = c.gre_type; break;
            case SAI_MIRROR_SESSION_ATTR_POLICER: a.value.oid = m->policer; break;
            default: return attr_not_supported(i);
            }
        }
        return SAI_STATUS_SUCCESS;
    });
}

static sai_mirror_api_t make_mirror_api()
{
    sai_mirror_api_t a;
    std::memset(&a, 0, sizeof(a));
    a.create_mirror_session = create_mirror_session;
    a.remove_mirror_session = remove_mirror_session;
    a.set_mirror_session_attribute = set_mirror_session_attribute;
    a.get_mirror_session_attribute = get_mirror_session_attribute;
    return a;
}
const sai_mirror_api_t mirror_api = make_mirror_api();

// ------------------------------------------------------------ samplepacket
static SampleObj* sample_of(sai_object_id_t oid) { return lookup(ctx().samplepackets, oid); }

static sai_status_t create_samplepacket(_Out_ sai_object_id_t* id, _In_ sai_object_id_t sw, _In_ uint32_t n,
                                        _In_ const sai_attribute_t* attrs)
{
    return guarded(SAI_API_SAMPLEPACKET, __func__, [&]() -> sai_status_t {
        auto& c = ctx();
        if (!id || (n && !attrs)) return SAI_STATUS_INVALID_PARAMETER;
        if (sw != c.switch_oid) return SAI_STATUS_INVALID_OBJECT_ID;
        SampleObj s;
        bool rate = false;
        for (uint32_t i = 0; i < n; i++) {
            const sai_attribute_t& a = attrs[i];
            switch (a.id) {
            case SAI_SAMPLEPACKET_ATTR_SAMPLE_RATE:
                if (a.value.u32 == 0) return attr_invalid(i);
                s.rate = a.value.u32;
                rate = true;
                break;
            case SAI_SAMPLEPACKET_ATTR_TYPE:
                if (a.value.s32 != SAI_SAMPLEPACKET_TYPE_SLOW_PATH) return attr_not_supported(i);
                s.type = a.value.s32;
                break;
            case SAI_SAMPLEPACKET_ATTR_MODE:
                if (a.value.s32 != SAI_SAMPLEPACKET_MODE_EXCLUSIVE && a.value.s32 != SAI_SAMPLEPACKET_MODE_SHARED)
                    return attr_invalid(i);
                s.mode = a.value.s32;
                break;
            default: return attr_not_supported(i);
            }
        }
        if (!rate) return SAI_STATUS_MANDATORY_ATTRIBUTE_MISSING;
        if (mirror_be()) {
            sai_status_t st = c.be->sample_create(s.rate, s.id);
            if (st != SAI_STATUS_SUCCESS) return st;
        }
        *id = c.alloc(SAI_OBJECT_TYPE_SAMPLEPACKET);
        c.samplepackets[*id] = s;
        return SAI_STATUS_SUCCESS;
    });
}

static sai_status_t remove_samplepacket(_In_ sai_object_id_t id)
{
    return guarded(SAI_API_SAMPLEPACKET, __func__, [&]() -> sai_status_t {
        auto& c = ctx();
        SampleObj* s = sample_of(id);
        if (!s) return SAI_STATUS_INVALID_OBJECT_ID;
        for (auto& p : c.ports)
            if (p.second.sample_in == id || p.second.sample_eg == id) return SAI_STATUS_OBJECT_IN_USE;
        if (s->id >= 0) {
            sai_status_t st = c.be->sample_destroy(s->id);
            if (st != SAI_STATUS_SUCCESS) return st;
        }
        c.samplepackets.erase(id);
        return SAI_STATUS_SUCCESS;
    });
}

static sai_status_t set_samplepacket_attribute(_In_ sai_object_id_t id, _In_ const sai_attribute_t* a)
{
    return guarded(SAI_API_SAMPLEPACKET, __func__, [&]() -> sai_status_t {
        SampleObj* s = sample_of(id);
        if (!s) return SAI_STATUS_INVALID_OBJECT_ID;
        if (!a) return SAI_STATUS_INVALID_PARAMETER;
        if (a->id != SAI_SAMPLEPACKET_ATTR_SAMPLE_RATE) return attr_invalid(0);   // TYPE, MODE: create only
        if (a->value.u32 == 0) return attr_invalid(0);
        if (s->id >= 0) {
            sai_status_t st = ctx().be->sample_update(s->id, a->value.u32);
            if (st != SAI_STATUS_SUCCESS) return attr_backend_status(st, 0);
        }
        s->rate = a->value.u32;
        return SAI_STATUS_SUCCESS;
    });
}

static sai_status_t get_samplepacket_attribute(_In_ sai_object_id_t id, _In_ uint32_t n, _Inout_ sai_attribute_t* attrs)
{
    return guarded(SAI_API_SAMPLEPACKET, __func__, [&]() -> sai_status_t {
        SampleObj* s = sample_of(id);
        if (!s) return SAI_STATUS_INVALID_OBJECT_ID;
        if (n && !attrs) return SAI_STATUS_INVALID_PARAMETER;
        for (uint32_t i = 0; i < n; i++) {
            sai_attribute_t& a = attrs[i];
            switch (a.id) {
            case SAI_SAMPLEPACKET_ATTR_SAMPLE_RATE: a.value.u32 = s->rate; break;
            case SAI_SAMPLEPACKET_ATTR_TYPE: a.value.s32 = s->type; break;
            case SAI_SAMPLEPACKET_ATTR_MODE: a.value.s32 = s->mode; break;
            default: return attr_not_supported(i);
            }
        }
        return SAI_STATUS_SUCCESS;
    });
}

static sai_samplepacket_api_t make_samplepacket_api()
{
    sai_samplepacket_api_t a;
    std::memset(&a, 0, sizeof(a));
    a.create_samplepacket = create_samplepacket;
    a.remove_samplepacket = remove_samplepacket;
    a.set_samplepacket_attribute = set_samplepacket_attribute;
    a.get_samplepacket_attribute = get_samplepacket_attribute;
    return a;
}
const sai_samplepacket_api_t samplepacket_api = make_samplepacket_api();

// --------------------------------------------------------- port attributes
static std::vector<int> backend_ids(const std::vector<sai_object_id_t>& sessions)
{
    std::vector<int> v;
    for (auto s : sessions) {
        int id = mirror_backend_id(s);
        if (id >= 0) v.push_back(id);
    }
    return v;
}

sai_status_t port_mirror_set(PortObj& p, const sai_attribute_t& a, uint32_t i, bool& handled)
{
    auto& c = ctx();
    handled = mirror_be();   // a backend without mirroring: unsupported, as before
    if (!handled) return SAI_STATUS_SUCCESS;
    switch (a.id) {
    case SAI_PORT_ATTR_INGRESS_MIRROR_SESSION:
    case SAI_PORT_ATTR_EGRESS_MIRROR_SESSION: {
        bool in = a.id == SAI_PORT_ATTR_INGRESS_MIRROR_SESSION;
        if (p.cpu) return attr_not_supported(i);
        if (a.value.objlist.count && !a.value.objlist.list) return attr_invalid(i);
        std::vector<sai_object_id_t> v(a.value.objlist.list, a.value.objlist.list + a.value.objlist.count);
        for (auto s : v) if (!mirror_valid(s)) return attr_invalid(i);
        if (mirror_be()) {
            sai_status_t st = c.be->port_mirror_set(p.hw, in, backend_ids(v));
            if (st != SAI_STATUS_SUCCESS) return attr_backend_status(st, i);
        }
        (in ? p.mirror_in : p.mirror_eg) = v;
        return SAI_STATUS_SUCCESS;
    }
    case SAI_PORT_ATTR_INGRESS_SAMPLEPACKET_ENABLE:
    case SAI_PORT_ATTR_EGRESS_SAMPLEPACKET_ENABLE: {
        bool in = a.id == SAI_PORT_ATTR_INGRESS_SAMPLEPACKET_ENABLE;
        if (p.cpu) return attr_not_supported(i);
        SampleObj* s = a.value.oid ? sample_of(a.value.oid) : nullptr;
        if (a.value.oid && !s) return attr_invalid(i);
        if (mirror_be()) {
            sai_status_t st = c.be->port_sample_set(p.hw, in, s ? s->id : -1);
            if (st != SAI_STATUS_SUCCESS) return attr_backend_status(st, i);
        }
        (in ? p.sample_in : p.sample_eg) = a.value.oid;
        return SAI_STATUS_SUCCESS;
    }
    default:
        handled = false;
        return SAI_STATUS_SUCCESS;
    }
}

sai_status_t port_mirror_get(PortObj& p, sai_attribute_t& a, uint32_t i, bool& handled)
{
    (void)i;
    handled = mirror_be();
    if (!handled) return SAI_STATUS_SUCCESS;
    switch (a.id) {
    case SAI_PORT_ATTR_INGRESS_MIRROR_SESSION: return fill_list(a.value.objlist, p.mirror_in);
    case SAI_PORT_ATTR_EGRESS_MIRROR_SESSION: return fill_list(a.value.objlist, p.mirror_eg);
    case SAI_PORT_ATTR_INGRESS_SAMPLEPACKET_ENABLE: a.value.oid = p.sample_in; return SAI_STATUS_SUCCESS;
    case SAI_PORT_ATTR_EGRESS_SAMPLEPACKET_ENABLE: a.value.oid = p.sample_eg; return SAI_STATUS_SUCCESS;
    default:
        handled = false;
        return SAI_STATUS_SUCCESS;
    }
}

void mirror_clear_objects()
{
    auto& c = ctx();   // the backend's objects went with its shutdown
    c.mirrors.clear();
    c.samplepackets.clear();
}

} // namespace ob
