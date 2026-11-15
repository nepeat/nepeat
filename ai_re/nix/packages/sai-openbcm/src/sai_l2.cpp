// SPDX-License-Identifier: Apache-2.0
// VLAN, VLAN_MEMBER, BRIDGE (default .1Q only), BRIDGE_PORT (type PORT),
// FDB_ENTRY and STP (default instance only).
#include "sai_internal.h"

namespace ob {

static const StubDef vlan_stubs[] = {
    {SAI_VLAN_ATTR_LEARN_DISABLE, Kind::Bool, 0, false},
    {SAI_VLAN_ATTR_MAX_LEARNED_ADDRESSES, Kind::U32, 0, false},
    {SAI_VLAN_ATTR_UNKNOWN_UNICAST_FLOOD_CONTROL_TYPE, Kind::S32, SAI_VLAN_FLOOD_CONTROL_TYPE_ALL, false},
    {SAI_VLAN_ATTR_UNKNOWN_MULTICAST_FLOOD_CONTROL_TYPE, Kind::S32, SAI_VLAN_FLOOD_CONTROL_TYPE_ALL, false},
    {SAI_VLAN_ATTR_BROADCAST_FLOOD_CONTROL_TYPE, Kind::S32, SAI_VLAN_FLOOD_CONTROL_TYPE_ALL, false},
    {SAI_VLAN_ATTR_IPV4_MCAST_LOOKUP_KEY_TYPE, Kind::S32, SAI_VLAN_MCAST_LOOKUP_KEY_TYPE_MAC_DA, false},
    {SAI_VLAN_ATTR_IPV6_MCAST_LOOKUP_KEY_TYPE, Kind::S32, SAI_VLAN_MCAST_LOOKUP_KEY_TYPE_MAC_DA, false},
    {SAI_VLAN_ATTR_META_DATA, Kind::U32, 0, false},
    {SAI_VLAN_ATTR_CUSTOM_IGMP_SNOOPING_ENABLE, Kind::Bool, 0, false},
};
static const StubDef bridge_stubs[] = {
    {SAI_BRIDGE_ATTR_MAX_LEARNED_ADDRESSES, Kind::U32, 0, false},
    {SAI_BRIDGE_ATTR_LEARN_DISABLE, Kind::Bool, 0, false},
    {SAI_BRIDGE_ATTR_UNKNOWN_UNICAST_FLOOD_CONTROL_TYPE, Kind::S32, SAI_BRIDGE_FLOOD_CONTROL_TYPE_SUB_PORTS, false},
    {SAI_BRIDGE_ATTR_UNKNOWN_MULTICAST_FLOOD_CONTROL_TYPE, Kind::S32, SAI_BRIDGE_FLOOD_CONTROL_TYPE_SUB_PORTS, false},
    {SAI_BRIDGE_ATTR_BROADCAST_FLOOD_CONTROL_TYPE, Kind::S32, SAI_BRIDGE_FLOOD_CONTROL_TYPE_SUB_PORTS, false},
};
static const StubDef bport_stubs[] = {
    {SAI_BRIDGE_PORT_ATTR_MAX_LEARNED_ADDRESSES, Kind::U32, 0, false},
    {SAI_BRIDGE_PORT_ATTR_FDB_LEARNING_LIMIT_VIOLATION_PACKET_ACTION, Kind::S32, SAI_PACKET_ACTION_DROP, false},
    {SAI_BRIDGE_PORT_ATTR_INGRESS_FILTERING, Kind::Bool, 0, false},
    {SAI_BRIDGE_PORT_ATTR_EGRESS_FILTERING, Kind::Bool, 0, false},
    {SAI_BRIDGE_PORT_ATTR_TAGGING_MODE, Kind::S32, SAI_BRIDGE_PORT_TAGGING_MODE_TAGGED, false},
};
static const StubDef fdb_stubs[] = {
    {SAI_FDB_ENTRY_ATTR_META_DATA, Kind::U32, 0, false},
    {SAI_FDB_ENTRY_ATTR_ALLOW_MAC_MOVE, Kind::Bool, 0, false},
};
#define NSTUB(x) (sizeof(x) / sizeof((x)[0]))

// ---------------------------------------------------------------- helpers
int bridge_port_hw(sai_object_id_t bport)
{
    auto& c = ctx();
    BridgePortObj* b = lookup(c.bridge_ports, bport);
    if (!b || b->type != SAI_BRIDGE_PORT_TYPE_PORT) return -1;
    PortObj* p = lookup(c.ports, b->port);
    return p ? p->hw : -1;
}

sai_object_id_t bridge_port_of_hw(int hw)
{
    auto& c = ctx();
    auto it = c.hw2port.find(hw);
    if (it == c.hw2port.end()) return 0;
    for (auto& b : c.bridge_ports)
        if (b.second.type == SAI_BRIDGE_PORT_TYPE_PORT && b.second.port == it->second) return b.first;
    return 0;
}

static Learn to_learn(int32_t m)
{
    switch (m) {
    case SAI_BRIDGE_PORT_FDB_LEARNING_MODE_DISABLE: return Learn::Disable;
    case SAI_BRIDGE_PORT_FDB_LEARNING_MODE_DROP: return Learn::Drop;
    case SAI_BRIDGE_PORT_FDB_LEARNING_MODE_CPU_TRAP:
    case SAI_BRIDGE_PORT_FDB_LEARNING_MODE_CPU_LOG: return Learn::Cpu;
    default: return Learn::Hw;
    }
}

static VlanObj* vlan_of(sai_object_id_t oid)
{
    if (oid_type(oid) != SAI_OBJECT_TYPE_VLAN) return nullptr;
    return lookup(ctx().vlans, oid);
}

template <size_t N>
static sai_status_t stub_get(const StubDef (&defs)[N], StubMap& m, sai_attribute_t& a, uint32_t i)
{
    if (const StubDef* d = find_stub(defs, N, a.id)) {
        auto it = m.find(a.id);
        if (it != m.end()) a.value = it->second;
        else stub_default(*d, a.value);
        return SAI_STATUS_SUCCESS;
    }
    return attr_not_supported(i);
}

template <size_t N>
static sai_status_t stub_set(const StubDef (&defs)[N], StubMap& m, const sai_attribute_t& a, uint32_t i)
{
    const StubDef* d = find_stub(defs, N, a.id);
    if (!d) return attr_not_supported(i);
    m[a.id] = a.value;
    return SAI_STATUS_SUCCESS;
}

// ------------------------------------------------------------------- VLAN
static sai_status_t create_vlan(_Out_ sai_object_id_t* vlan_id, _In_ sai_object_id_t, _In_ uint32_t n,
                                _In_ const sai_attribute_t* attrs)
{
    return guarded(SAI_API_VLAN, __func__, [&]() -> sai_status_t {
        auto& c = ctx();
        if (!vlan_id || (n && !attrs)) return SAI_STATUS_INVALID_PARAMETER;
        uint32_t vi = 0;
        const sai_attribute_t* va = find_attr(n, attrs, SAI_VLAN_ATTR_VLAN_ID, &vi);
        if (!va) return SAI_STATUS_MANDATORY_ATTRIBUTE_MISSING;
        uint16_t vid = va->value.u16;
        if (vid < 1 || vid > 4094) return attr_invalid(vi);
        if (c.vid2vlan.count(vid)) return SAI_STATUS_ITEM_ALREADY_EXISTS;
        if (c.internal_vlans.count(vid)) {
            OB_ERR(SAI_API_VLAN, "VLAN %u is in use as an internal router-interface VLAN", vid);
            return SAI_STATUS_ITEM_ALREADY_EXISTS;
        }
        VlanObj v;
        v.vid = vid;
        v.oid = make_oid(SAI_OBJECT_TYPE_VLAN, vid);
        for (uint32_t i = 0; i < n; i++) {
            if (attrs[i].id == SAI_VLAN_ATTR_VLAN_ID) continue;
            if (attrs[i].id == SAI_VLAN_ATTR_STP_INSTANCE) {
                if (attrs[i].value.oid != c.default_stp) return attr_invalid(i);
                continue;
            }
            sai_status_t st = stub_set(vlan_stubs, v.stub, attrs[i], i);
            if (st != SAI_STATUS_SUCCESS) return st;
        }
        sai_status_t st = c.be->vlan_create(vid);
        if (st != SAI_STATUS_SUCCESS) return st;
        c.vlans[v.oid] = v;
        c.vid2vlan[vid] = v.oid;
        *vlan_id = v.oid;
        OB_INFO(SAI_API_VLAN, "VLAN %u created", vid);
        return SAI_STATUS_SUCCESS;
    });
}

static sai_status_t remove_vlan(_In_ sai_object_id_t vlan_id)
{
    return guarded(SAI_API_VLAN, __func__, [&]() -> sai_status_t {
        auto& c = ctx();
        VlanObj* v = vlan_of(vlan_id);
        if (!v) return SAI_STATUS_INVALID_OBJECT_ID;
        if (v->vid == 1) return SAI_STATUS_OBJECT_IN_USE;   // default VLAN
        if (!v->members.empty()) return SAI_STATUS_OBJECT_IN_USE;
        for (auto& r : c.rifs)
            if (r.second.vlan == vlan_id) return SAI_STATUS_OBJECT_IN_USE;
        for (auto& f : c.fdbs)
            if (f.first.bv == vlan_id) return SAI_STATUS_OBJECT_IN_USE;
        sai_status_t st = c.be->vlan_destroy(v->vid);
        if (st != SAI_STATUS_SUCCESS) return st;
        c.vid2vlan.erase(v->vid);
        c.vlans.erase(vlan_id);
        return SAI_STATUS_SUCCESS;
    });
}

static sai_status_t set_vlan_attribute(_In_ sai_object_id_t vlan_id, _In_ const sai_attribute_t* attr)
{
    return guarded(SAI_API_VLAN, __func__, [&]() -> sai_status_t {
        VlanObj* v = vlan_of(vlan_id);
        if (!v) return SAI_STATUS_INVALID_OBJECT_ID;
        if (!attr) return SAI_STATUS_INVALID_PARAMETER;
        if (attr->id == SAI_VLAN_ATTR_VLAN_ID) return attr_invalid(0);   // create-only
        if (attr->id == SAI_VLAN_ATTR_STP_INSTANCE)
            return attr->value.oid == ctx().default_stp ? SAI_STATUS_SUCCESS : attr_invalid(0);
        return stub_set(vlan_stubs, v->stub, *attr, 0);
    });
}

static sai_status_t get_vlan_attribute(_In_ sai_object_id_t vlan_id, _In_ uint32_t n, _Inout_ sai_attribute_t* attrs)
{
    return guarded(SAI_API_VLAN, __func__, [&]() -> sai_status_t {
        VlanObj* v = vlan_of(vlan_id);
        if (!v) return SAI_STATUS_INVALID_OBJECT_ID;
        if (n && !attrs) return SAI_STATUS_INVALID_PARAMETER;
        for (uint32_t i = 0; i < n; i++) {
            sai_attribute_t& a = attrs[i];
            sai_status_t st = SAI_STATUS_SUCCESS;
            switch (a.id) {
            case SAI_VLAN_ATTR_VLAN_ID: a.value.u16 = v->vid; break;
            case SAI_VLAN_ATTR_MEMBER_LIST: {
                std::vector<sai_object_id_t> m(v->members.begin(), v->members.end());
                st = fill_list(a.value.objlist, m);
                break;
            }
            case SAI_VLAN_ATTR_STP_INSTANCE: a.value.oid = ctx().default_stp; break;
            default: st = stub_get(vlan_stubs, v->stub, a, i); break;
            }
            if (st != SAI_STATUS_SUCCESS) return st;
        }
        return SAI_STATUS_SUCCESS;
    });
}

sai_status_t vlan_member_create_internal(sai_object_id_t vlan, sai_object_id_t bport, int32_t tagging, bool program,
                                         sai_object_id_t* out)
{
    auto& c = ctx();
    VlanObj* v = vlan_of(vlan);
    BridgePortObj* b = lookup(c.bridge_ports, bport);
    if (!v || !b) return SAI_STATUS_INVALID_PARAMETER;
    int hw = bridge_port_hw(bport);
    if (hw < 0) return SAI_STATUS_NOT_SUPPORTED;
    for (auto m : v->members)
        if (c.vlan_members[m].bport == bport) return SAI_STATUS_ITEM_ALREADY_EXISTS;
    if (program) {
        sai_status_t st = c.be->vlan_port_add(v->vid, hw, tagging != SAI_VLAN_TAGGING_MODE_TAGGED);
        if (st != SAI_STATUS_SUCCESS) return st;
    }
    sai_object_id_t oid = c.alloc(SAI_OBJECT_TYPE_VLAN_MEMBER);
    VlanMemberObj m;
    m.vlan = vlan;
    m.bport = bport;
    m.tagging = tagging;
    c.vlan_members[oid] = m;
    v->members.insert(oid);
    b->vlan_members.insert(oid);
    if (out) *out = oid;
    return SAI_STATUS_SUCCESS;
}

static sai_status_t create_vlan_member(_Out_ sai_object_id_t* id, _In_ sai_object_id_t, _In_ uint32_t n,
                                       _In_ const sai_attribute_t* attrs)
{
    return guarded(SAI_API_VLAN, __func__, [&]() -> sai_status_t {
        if (!id || (n && !attrs)) return SAI_STATUS_INVALID_PARAMETER;
        sai_object_id_t vlan = 0, bport = 0;
        int32_t tagging = SAI_VLAN_TAGGING_MODE_UNTAGGED;
        bool hv = false, hb = false;
        for (uint32_t i = 0; i < n; i++) {
            switch (attrs[i].id) {
            case SAI_VLAN_MEMBER_ATTR_VLAN_ID: vlan = attrs[i].value.oid; hv = true; break;
            case SAI_VLAN_MEMBER_ATTR_BRIDGE_PORT_ID: bport = attrs[i].value.oid; hb = true; break;
            case SAI_VLAN_MEMBER_ATTR_VLAN_TAGGING_MODE:
                tagging = attrs[i].value.s32;
                if (tagging == SAI_VLAN_TAGGING_MODE_PRIORITY_TAGGED) return attr_not_supported(i);
                if (tagging != SAI_VLAN_TAGGING_MODE_UNTAGGED && tagging != SAI_VLAN_TAGGING_MODE_TAGGED)
                    return attr_invalid(i);
                break;
            default: return attr_not_supported(i);
            }
        }
        if (!hv || !hb) return SAI_STATUS_MANDATORY_ATTRIBUTE_MISSING;
        return vlan_member_create_internal(vlan, bport, tagging, true, id);
    });
}

static sai_status_t remove_vlan_member(_In_ sai_object_id_t id)
{
    return guarded(SAI_API_VLAN, __func__, [&]() -> sai_status_t {
        auto& c = ctx();
        VlanMemberObj* m = lookup(c.vlan_members, id);
        if (!m) return SAI_STATUS_INVALID_OBJECT_ID;
        VlanObj* v = vlan_of(m->vlan);
        int hw = bridge_port_hw(m->bport);
        if (v && hw >= 0) {
            sai_status_t st = c.be->vlan_port_remove(v->vid, hw);
            if (st != SAI_STATUS_SUCCESS) return st;
        }
        if (v) v->members.erase(id);
        if (BridgePortObj* b = lookup(c.bridge_ports, m->bport)) b->vlan_members.erase(id);
        c.vlan_members.erase(id);
        return SAI_STATUS_SUCCESS;
    });
}

static sai_status_t set_vlan_member_attribute(_In_ sai_object_id_t id, _In_ const sai_attribute_t* attr)
{
    return guarded(SAI_API_VLAN, __func__, [&]() -> sai_status_t {
        auto& c = ctx();
        VlanMemberObj* m = lookup(c.vlan_members, id);
        if (!m) return SAI_STATUS_INVALID_OBJECT_ID;
        if (!attr) return SAI_STATUS_INVALID_PARAMETER;
        if (attr->id != SAI_VLAN_MEMBER_ATTR_VLAN_TAGGING_MODE) return attr_invalid(0);
        int32_t t = attr->value.s32;
        if (t != SAI_VLAN_TAGGING_MODE_UNTAGGED && t != SAI_VLAN_TAGGING_MODE_TAGGED) return attr_not_supported(0);
        VlanObj* v = vlan_of(m->vlan);
        int hw = bridge_port_hw(m->bport);
        if (!v || hw < 0) return SAI_STATUS_FAILURE;
        sai_status_t st = c.be->vlan_port_add(v->vid, hw, t == SAI_VLAN_TAGGING_MODE_UNTAGGED);
        if (st == SAI_STATUS_SUCCESS) m->tagging = t;
        return st;
    });
}

static sai_status_t get_vlan_member_attribute(_In_ sai_object_id_t id, _In_ uint32_t n, _Inout_ sai_attribute_t* attrs)
{
    return guarded(SAI_API_VLAN, __func__, [&]() -> sai_status_t {
        VlanMemberObj* m = lookup(ctx().vlan_members, id);
        if (!m) return SAI_STATUS_INVALID_OBJECT_ID;
        if (n && !attrs) return SAI_STATUS_INVALID_PARAMETER;
        for (uint32_t i = 0; i < n; i++) {
            switch (attrs[i].id) {
            case SAI_VLAN_MEMBER_ATTR_VLAN_ID: attrs[i].value.oid = m->vlan; break;
            case SAI_VLAN_MEMBER_ATTR_BRIDGE_PORT_ID: attrs[i].value.oid = m->bport; break;
            case SAI_VLAN_MEMBER_ATTR_VLAN_TAGGING_MODE: attrs[i].value.s32 = m->tagging; break;
            default: return attr_not_supported(i);
            }
        }
        return SAI_STATUS_SUCCESS;
    });
}

static sai_status_t no_vlan_stats(_In_ sai_object_id_t, _In_ uint32_t, _In_ const sai_stat_id_t*, _Out_ uint64_t*)
{
    return SAI_STATUS_NOT_SUPPORTED;
}

static sai_vlan_api_t make_vlan_api()
{
    sai_vlan_api_t a;
    std::memset(&a, 0, sizeof(a));
    a.create_vlan = create_vlan;
    a.remove_vlan = remove_vlan;
    a.set_vlan_attribute = set_vlan_attribute;
    a.get_vlan_attribute = get_vlan_attribute;
    a.create_vlan_member = create_vlan_member;
    a.remove_vlan_member = remove_vlan_member;
    a.set_vlan_member_attribute = set_vlan_member_attribute;
    a.get_vlan_member_attribute = get_vlan_member_attribute;
    a.get_vlan_stats = no_vlan_stats;
    return a;
}
const sai_vlan_api_t vlan_api = make_vlan_api();

// ----------------------------------------------------------------- BRIDGE
static sai_status_t create_bridge(_Out_ sai_object_id_t*, _In_ sai_object_id_t, _In_ uint32_t, _In_ const sai_attribute_t*)
{
    OB_ERR(SAI_API_BRIDGE, "only the default .1Q bridge exists (.1D bridges not supported)");
    return SAI_STATUS_NOT_SUPPORTED;
}

static sai_status_t remove_bridge(_In_ sai_object_id_t id)
{
    return guarded(SAI_API_BRIDGE, __func__, [&]() -> sai_status_t {
        return id == ctx().default_bridge ? SAI_STATUS_OBJECT_IN_USE : SAI_STATUS_INVALID_OBJECT_ID;
    });
}

static sai_status_t set_bridge_attribute(_In_ sai_object_id_t id, _In_ const sai_attribute_t* attr)
{
    return guarded(SAI_API_BRIDGE, __func__, [&]() -> sai_status_t {
        BridgeObj* b = lookup(ctx().bridges, id);
        if (!b) return SAI_STATUS_INVALID_OBJECT_ID;
        if (!attr) return SAI_STATUS_INVALID_PARAMETER;
        return stub_set(bridge_stubs, b->stub, *attr, 0);
    });
}

static sai_status_t get_bridge_attribute(_In_ sai_object_id_t id, _In_ uint32_t n, _Inout_ sai_attribute_t* attrs)
{
    return guarded(SAI_API_BRIDGE, __func__, [&]() -> sai_status_t {
        BridgeObj* b = lookup(ctx().bridges, id);
        if (!b) return SAI_STATUS_INVALID_OBJECT_ID;
        if (n && !attrs) return SAI_STATUS_INVALID_PARAMETER;
        for (uint32_t i = 0; i < n; i++) {
            sai_status_t st = SAI_STATUS_SUCCESS;
            switch (attrs[i].id) {
            case SAI_BRIDGE_ATTR_TYPE: attrs[i].value.s32 = b->type; break;
            case SAI_BRIDGE_ATTR_PORT_LIST: {
                std::vector<sai_object_id_t> v(b->ports.begin(), b->ports.end());
                st = fill_list(attrs[i].value.objlist, v);
                break;
            }
            default: st = stub_get(bridge_stubs, b->stub, attrs[i], i); break;
            }
            if (st != SAI_STATUS_SUCCESS) return st;
        }
        return SAI_STATUS_SUCCESS;
    });
}

static sai_status_t create_bridge_port(_Out_ sai_object_id_t* id, _In_ sai_object_id_t, _In_ uint32_t n,
                                       _In_ const sai_attribute_t* attrs)
{
    return guarded(SAI_API_BRIDGE, __func__, [&]() -> sai_status_t {
        auto& c = ctx();
        if (!id || (n && !attrs)) return SAI_STATUS_INVALID_PARAMETER;
        BridgePortObj b;
        b.bridge = c.default_bridge;
        bool ht = false;
        for (uint32_t i = 0; i < n; i++) {
            const sai_attribute_t& a = attrs[i];
            switch (a.id) {
            case SAI_BRIDGE_PORT_ATTR_TYPE:
                if (a.value.s32 != SAI_BRIDGE_PORT_TYPE_PORT) {
                    OB_ERR(SAI_API_BRIDGE, "bridge port type %d not supported (PORT only)", a.value.s32);
                    return attr_not_supported(i);
                }
                ht = true;
                break;
            case SAI_BRIDGE_PORT_ATTR_PORT_ID: b.port = a.value.oid; break;
            case SAI_BRIDGE_PORT_ATTR_BRIDGE_ID:
                if (a.value.oid != c.default_bridge) return attr_invalid(i);
                break;
            case SAI_BRIDGE_PORT_ATTR_ADMIN_STATE: b.admin = a.value.booldata; break;
            case SAI_BRIDGE_PORT_ATTR_FDB_LEARNING_MODE: b.learn = a.value.s32; break;
            default: {
                sai_status_t st = stub_set(bport_stubs, b.stub, a, i);
                if (st != SAI_STATUS_SUCCESS) return st;
            }
            }
        }
        if (!ht || !b.port) return SAI_STATUS_MANDATORY_ATTRIBUTE_MISSING;
        PortObj* p = lookup(c.ports, b.port);
        if (!p || p->cpu) return SAI_STATUS_INVALID_PARAMETER;
        if (bridge_port_of_hw(p->hw)) return SAI_STATUS_ITEM_ALREADY_EXISTS;
        sai_status_t st = c.be->port_learn_set(p->hw, to_learn(b.learn));
        if (st != SAI_STATUS_SUCCESS) return st;
        sai_object_id_t oid = c.alloc(SAI_OBJECT_TYPE_BRIDGE_PORT);
        c.bridge_ports[oid] = b;
        c.bridges[c.default_bridge].ports.insert(oid);
        *id = oid;
        return SAI_STATUS_SUCCESS;
    });
}

static sai_status_t remove_bridge_port(_In_ sai_object_id_t id)
{
    return guarded(SAI_API_BRIDGE, __func__, [&]() -> sai_status_t {
        auto& c = ctx();
        BridgePortObj* b = lookup(c.bridge_ports, id);
        if (!b) return SAI_STATUS_INVALID_OBJECT_ID;
        if (!b->vlan_members.empty()) return SAI_STATUS_OBJECT_IN_USE;
        for (auto& f : c.fdbs)
            if (f.second.bport == id && f.second.type == SAI_FDB_ENTRY_TYPE_STATIC) return SAI_STATUS_OBJECT_IN_USE;
        int hw = bridge_port_hw(id);
        if (hw >= 0) c.be->l2_flush(hw, -1, false, true);
        for (auto it = c.fdbs.begin(); it != c.fdbs.end();)
            it = it->second.bport == id ? c.fdbs.erase(it) : std::next(it);
        c.bridges[b->bridge].ports.erase(id);
        c.bridge_ports.erase(id);
        return SAI_STATUS_SUCCESS;
    });
}

static sai_status_t set_bridge_port_attribute(_In_ sai_object_id_t id, _In_ const sai_attribute_t* attr)
{
    return guarded(SAI_API_BRIDGE, __func__, [&]() -> sai_status_t {
        auto& c = ctx();
        BridgePortObj* b = lookup(c.bridge_ports, id);
        if (!b) return SAI_STATUS_INVALID_OBJECT_ID;
        if (!attr) return SAI_STATUS_INVALID_PARAMETER;
        switch (attr->id) {
        case SAI_BRIDGE_PORT_ATTR_ADMIN_STATE: b->admin = attr->value.booldata; return SAI_STATUS_SUCCESS;
        case SAI_BRIDGE_PORT_ATTR_FDB_LEARNING_MODE: {
            int hw = bridge_port_hw(id);
            sai_status_t st = hw < 0 ? SAI_STATUS_FAILURE : c.be->port_learn_set(hw, to_learn(attr->value.s32));
            if (st == SAI_STATUS_SUCCESS) b->learn = attr->value.s32;
            return attr_backend_status(st, 0);
        }
        case SAI_BRIDGE_PORT_ATTR_TYPE:
        case SAI_BRIDGE_PORT_ATTR_PORT_ID:
        case SAI_BRIDGE_PORT_ATTR_BRIDGE_ID: return attr_invalid(0);
        default: return stub_set(bport_stubs, b->stub, *attr, 0);
        }
    });
}

static sai_status_t get_bridge_port_attribute(_In_ sai_object_id_t id, _In_ uint32_t n, _Inout_ sai_attribute_t* attrs)
{
    return guarded(SAI_API_BRIDGE, __func__, [&]() -> sai_status_t {
        BridgePortObj* b = lookup(ctx().bridge_ports, id);
        if (!b) return SAI_STATUS_INVALID_OBJECT_ID;
        if (n && !attrs) return SAI_STATUS_INVALID_PARAMETER;
        for (uint32_t i = 0; i < n; i++) {
            sai_attribute_t& a = attrs[i];
            sai_status_t st = SAI_STATUS_SUCCESS;
            switch (a.id) {
            case SAI_BRIDGE_PORT_ATTR_TYPE: a.value.s32 = b->type; break;
            case SAI_BRIDGE_PORT_ATTR_PORT_ID: a.value.oid = b->port; break;
            case SAI_BRIDGE_PORT_ATTR_BRIDGE_ID: a.value.oid = b->bridge; break;
            case SAI_BRIDGE_PORT_ATTR_ADMIN_STATE: a.value.booldata = b->admin; break;
            case SAI_BRIDGE_PORT_ATTR_FDB_LEARNING_MODE: a.value.s32 = b->learn; break;
            default: st = stub_get(bport_stubs, b->stub, a, i); break;
            }
            if (st != SAI_STATUS_SUCCESS) return st;
        }
        return SAI_STATUS_SUCCESS;
    });
}

static sai_status_t no_bridge_stats(_In_ sai_object_id_t, _In_ uint32_t, _In_ const sai_stat_id_t*, _Out_ uint64_t*)
{
    return SAI_STATUS_NOT_SUPPORTED;
}

static sai_bridge_api_t make_bridge_api()
{
    sai_bridge_api_t a;
    std::memset(&a, 0, sizeof(a));
    a.create_bridge = create_bridge;
    a.remove_bridge = remove_bridge;
    a.set_bridge_attribute = set_bridge_attribute;
    a.get_bridge_attribute = get_bridge_attribute;
    a.get_bridge_stats = no_bridge_stats;
    a.create_bridge_port = create_bridge_port;
    a.remove_bridge_port = remove_bridge_port;
    a.set_bridge_port_attribute = set_bridge_port_attribute;
    a.get_bridge_port_attribute = get_bridge_port_attribute;
    a.get_bridge_port_stats = no_bridge_stats;
    return a;
}
const sai_bridge_api_t bridge_api = make_bridge_api();

// -------------------------------------------------------------------- FDB
static FdbKey key_of(const sai_fdb_entry_t& e)
{
    FdbKey k;
    std::memcpy(k.mac, e.mac_address, 6);
    k.bv = e.bv_id;
    return k;
}

static sai_status_t fdb_program(const FdbKey& k, const FdbObj& o)
{
    auto& c = ctx();
    VlanObj* v = vlan_of(k.bv);
    if (!v) return SAI_STATUS_INVALID_PARAMETER;
    bool drop = o.action == SAI_PACKET_ACTION_DROP;
    int hw = o.bport ? bridge_port_hw(o.bport) : -1;
    if (!drop && hw < 0) return SAI_STATUS_INVALID_PARAMETER;
    if (drop && hw < 0) hw = c.be->cpu_port();   // discard entry needs some port
    return c.be->l2_add(k.mac, v->vid, hw, o.type == SAI_FDB_ENTRY_TYPE_STATIC, drop);
}

static sai_status_t fdb_attr(FdbObj& o, const sai_attribute_t& a, uint32_t i)
{
    switch (a.id) {
    case SAI_FDB_ENTRY_ATTR_TYPE:
        if (a.value.s32 != SAI_FDB_ENTRY_TYPE_STATIC && a.value.s32 != SAI_FDB_ENTRY_TYPE_DYNAMIC) return attr_invalid(i);
        o.type = a.value.s32;
        return SAI_STATUS_SUCCESS;
    case SAI_FDB_ENTRY_ATTR_BRIDGE_PORT_ID:
        if (a.value.oid && bridge_port_hw(a.value.oid) < 0) return attr_invalid(i);
        o.bport = a.value.oid;
        return SAI_STATUS_SUCCESS;
    case SAI_FDB_ENTRY_ATTR_PACKET_ACTION:
        if (a.value.s32 != SAI_PACKET_ACTION_FORWARD && a.value.s32 != SAI_PACKET_ACTION_DROP) return attr_not_supported(i);
        o.action = a.value.s32;
        return SAI_STATUS_SUCCESS;
    default:
        return stub_set(fdb_stubs, o.stub, a, i);
    }
}

static sai_status_t create_fdb_entry(_In_ const sai_fdb_entry_t* e, _In_ uint32_t n, _In_ const sai_attribute_t* attrs)
{
    return guarded(SAI_API_FDB, __func__, [&]() -> sai_status_t {
        auto& c = ctx();
        if (!e || (n && !attrs)) return SAI_STATUS_INVALID_PARAMETER;
        if (oid_type(e->bv_id) == SAI_OBJECT_TYPE_BRIDGE) return SAI_STATUS_NOT_SUPPORTED;   // .1D
        if (!vlan_of(e->bv_id)) return SAI_STATUS_INVALID_PARAMETER;
        FdbKey k = key_of(*e);
        auto it = c.fdbs.find(k);
        if (it != c.fdbs.end() && it->second.type == SAI_FDB_ENTRY_TYPE_STATIC) return SAI_STATUS_ITEM_ALREADY_EXISTS;
        if (!find_attr(n, attrs, SAI_FDB_ENTRY_ATTR_TYPE)) return SAI_STATUS_MANDATORY_ATTRIBUTE_MISSING;
        FdbObj o;
        for (uint32_t i = 0; i < n; i++) {
            sai_status_t st = fdb_attr(o, attrs[i], i);
            if (st != SAI_STATUS_SUCCESS) return st;
        }
        sai_status_t st = fdb_program(k, o);
        if (st != SAI_STATUS_SUCCESS) return st;
        c.fdbs[k] = o;
        OB_INFO(SAI_API_FDB, "fdb %s vlan %u added", mac_str(k.mac).c_str(), c.vlans[k.bv].vid);
        return SAI_STATUS_SUCCESS;
    });
}

static sai_status_t remove_fdb_entry(_In_ const sai_fdb_entry_t* e)
{
    return guarded(SAI_API_FDB, __func__, [&]() -> sai_status_t {
        auto& c = ctx();
        if (!e) return SAI_STATUS_INVALID_PARAMETER;
        FdbKey k = key_of(*e);
        auto it = c.fdbs.find(k);
        if (it == c.fdbs.end()) return SAI_STATUS_ITEM_NOT_FOUND;
        VlanObj* v = vlan_of(k.bv);
        if (v) {
            sai_status_t st = c.be->l2_delete(k.mac, v->vid);
            if (st != SAI_STATUS_SUCCESS && st != SAI_STATUS_ITEM_NOT_FOUND) return st;
        }
        c.fdbs.erase(it);
        return SAI_STATUS_SUCCESS;
    });
}

static sai_status_t set_fdb_entry_attribute(_In_ const sai_fdb_entry_t* e, _In_ const sai_attribute_t* attr)
{
    return guarded(SAI_API_FDB, __func__, [&]() -> sai_status_t {
        auto& c = ctx();
        if (!e || !attr) return SAI_STATUS_INVALID_PARAMETER;
        FdbKey k = key_of(*e);
        auto it = c.fdbs.find(k);
        if (it == c.fdbs.end()) return SAI_STATUS_ITEM_NOT_FOUND;
        FdbObj o = it->second;
        sai_status_t st = fdb_attr(o, *attr, 0);
        if (st != SAI_STATUS_SUCCESS) return st;
        if (attr->id == SAI_FDB_ENTRY_ATTR_TYPE || attr->id == SAI_FDB_ENTRY_ATTR_BRIDGE_PORT_ID ||
            attr->id == SAI_FDB_ENTRY_ATTR_PACKET_ACTION) {
            st = fdb_program(k, o);
            if (st != SAI_STATUS_SUCCESS) return st;
        }
        it->second = o;
        return SAI_STATUS_SUCCESS;
    });
}

static sai_status_t get_fdb_entry_attribute(_In_ const sai_fdb_entry_t* e, _In_ uint32_t n, _Inout_ sai_attribute_t* attrs)
{
    return guarded(SAI_API_FDB, __func__, [&]() -> sai_status_t {
        if (!e || (n && !attrs)) return SAI_STATUS_INVALID_PARAMETER;
        auto it = ctx().fdbs.find(key_of(*e));
        if (it == ctx().fdbs.end()) return SAI_STATUS_ITEM_NOT_FOUND;
        FdbObj& o = it->second;
        for (uint32_t i = 0; i < n; i++) {
            sai_status_t st = SAI_STATUS_SUCCESS;
            switch (attrs[i].id) {
            case SAI_FDB_ENTRY_ATTR_TYPE: attrs[i].value.s32 = o.type; break;
            case SAI_FDB_ENTRY_ATTR_BRIDGE_PORT_ID: attrs[i].value.oid = o.bport; break;
            case SAI_FDB_ENTRY_ATTR_PACKET_ACTION: attrs[i].value.s32 = o.action; break;
            default: st = stub_get(fdb_stubs, o.stub, attrs[i], i); break;
            }
            if (st != SAI_STATUS_SUCCESS) return st;
        }
        return SAI_STATUS_SUCCESS;
    });
}

static sai_status_t flush_fdb_entries(_In_ sai_object_id_t, _In_ uint32_t n, _In_ const sai_attribute_t* attrs)
{
    return guarded(SAI_API_FDB, __func__, [&]() -> sai_status_t {
        auto& c = ctx();
        if (n && !attrs) return SAI_STATUS_INVALID_PARAMETER;
        sai_object_id_t bport = 0, bv = 0;
        int32_t type = SAI_FDB_FLUSH_ENTRY_TYPE_DYNAMIC;
        for (uint32_t i = 0; i < n; i++) {
            switch (attrs[i].id) {
            case SAI_FDB_FLUSH_ATTR_BRIDGE_PORT_ID: bport = attrs[i].value.oid; break;
            case SAI_FDB_FLUSH_ATTR_BV_ID: bv = attrs[i].value.oid; break;
            case SAI_FDB_FLUSH_ATTR_ENTRY_TYPE: type = attrs[i].value.s32; break;
            default: return attr_not_supported(i);
            }
        }
        int hw = -1, vid = -1;
        if (bport) {
            hw = bridge_port_hw(bport);
            if (hw < 0) return SAI_STATUS_INVALID_PARAMETER;
        }
        if (bv) {
            VlanObj* v = vlan_of(bv);
            if (!v) return SAI_STATUS_INVALID_PARAMETER;
            vid = v->vid;
        }
        bool statics = type == SAI_FDB_FLUSH_ENTRY_TYPE_STATIC || type == SAI_FDB_FLUSH_ENTRY_TYPE_ALL;
        bool dynamics = type == SAI_FDB_FLUSH_ENTRY_TYPE_DYNAMIC || type == SAI_FDB_FLUSH_ENTRY_TYPE_ALL;
        sai_status_t st = c.be->l2_flush(hw, vid, statics, dynamics);
        if (st != SAI_STATUS_SUCCESS) return st;
        size_t removed = 0;
        for (auto it = c.fdbs.begin(); it != c.fdbs.end();) {
            bool st_ok = it->second.type == SAI_FDB_ENTRY_TYPE_STATIC ? statics : dynamics;
            bool match = st_ok && (!bport || it->second.bport == bport) && (!bv || it->first.bv == bv);
            if (match) { it = c.fdbs.erase(it); removed++; }
            else ++it;
        }
        OB_INFO(SAI_API_FDB, "flush: %zu entries removed", removed);
        return SAI_STATUS_SUCCESS;
    });
}

static sai_fdb_api_t make_fdb_api()
{
    sai_fdb_api_t a;
    std::memset(&a, 0, sizeof(a));
    a.create_fdb_entry = create_fdb_entry;
    a.remove_fdb_entry = remove_fdb_entry;
    a.set_fdb_entry_attribute = set_fdb_entry_attribute;
    a.get_fdb_entry_attribute = get_fdb_entry_attribute;
    a.flush_fdb_entries = flush_fdb_entries;
    return a;
}
const sai_fdb_api_t fdb_api = make_fdb_api();

// -------------------------------------------------------------------- STP
static sai_status_t create_stp(_Out_ sai_object_id_t*, _In_ sai_object_id_t, _In_ uint32_t, _In_ const sai_attribute_t*)
{
    return SAI_STATUS_NOT_SUPPORTED;
}
static sai_status_t remove_stp(_In_ sai_object_id_t) { return SAI_STATUS_NOT_SUPPORTED; }
static sai_status_t set_stp_attribute(_In_ sai_object_id_t, _In_ const sai_attribute_t*) { return attr_not_supported(0); }

static sai_status_t get_stp_attribute(_In_ sai_object_id_t id, _In_ uint32_t n, _Inout_ sai_attribute_t* attrs)
{
    return guarded(SAI_API_STP, __func__, [&]() -> sai_status_t {
        auto& c = ctx();
        if (id != c.default_stp) return SAI_STATUS_INVALID_OBJECT_ID;
        if (n && !attrs) return SAI_STATUS_INVALID_PARAMETER;
        for (uint32_t i = 0; i < n; i++) {
            sai_status_t st = SAI_STATUS_SUCCESS;
            switch (attrs[i].id) {
            case SAI_STP_ATTR_VLAN_LIST: {
                std::vector<sai_vlan_id_t> v;
                for (auto& x : c.vid2vlan) v.push_back(x.first);
                st = fill_list(attrs[i].value.vlanlist, v);
                break;
            }
            case SAI_STP_ATTR_BRIDGE_ID: attrs[i].value.oid = c.default_bridge; break;
            case SAI_STP_ATTR_PORT_LIST: {
                std::vector<sai_object_id_t> none;
                st = fill_list(attrs[i].value.objlist, none);
                break;
            }
            default: st = attr_not_supported(i); break;
            }
            if (st != SAI_STATUS_SUCCESS) return st;
        }
        return SAI_STATUS_SUCCESS;
    });
}

static sai_status_t create_stp_port(_Out_ sai_object_id_t*, _In_ sai_object_id_t, _In_ uint32_t, _In_ const sai_attribute_t*)
{
    return SAI_STATUS_NOT_SUPPORTED;
}
static sai_status_t remove_stp_port(_In_ sai_object_id_t) { return SAI_STATUS_NOT_SUPPORTED; }
static sai_status_t set_stp_port_attribute(_In_ sai_object_id_t, _In_ const sai_attribute_t*) { return SAI_STATUS_NOT_SUPPORTED; }
static sai_status_t get_stp_port_attribute(_In_ sai_object_id_t, _In_ uint32_t, _Inout_ sai_attribute_t*)
{
    return SAI_STATUS_INVALID_OBJECT_ID;
}

static sai_stp_api_t make_stp_api()
{
    sai_stp_api_t a;
    std::memset(&a, 0, sizeof(a));
    a.create_stp = create_stp;
    a.remove_stp = remove_stp;
    a.set_stp_attribute = set_stp_attribute;
    a.get_stp_attribute = get_stp_attribute;
    a.create_stp_port = create_stp_port;
    a.remove_stp_port = remove_stp_port;
    a.set_stp_port_attribute = set_stp_port_attribute;
    a.get_stp_port_attribute = get_stp_port_attribute;
    return a;
}
const sai_stp_api_t stp_api = make_stp_api();

} // namespace ob
