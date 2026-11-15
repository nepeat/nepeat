// SPDX-License-Identifier: Apache-2.0
// LAG and LAG_MEMBER on SDK trunks. LACP itself runs in the host (teamd on
// the member ports' KNET netdevs; the members' hostifs are untouched). A LAG
// can carry a bridge port (VLAN members, FDB entries) and a port router
// interface; per-port state (VLAN membership, PVID, learning) is applied to
// every member, and to a port when it joins, undone when it leaves.
#include "sai_internal.h"

#include <algorithm>

namespace ob {

static const StubDef lag_stubs[] = {
    {SAI_LAG_ATTR_DEFAULT_VLAN_PRIORITY, Kind::U8, 0, false},
    {SAI_LAG_ATTR_DROP_UNTAGGED, Kind::Bool, 0, false},
    {SAI_LAG_ATTR_DROP_TAGGED, Kind::Bool, 0, false},
    {SAI_LAG_ATTR_TPID, Kind::U16, 0x8100, false},
};
static const StubDef member_stubs[] = {
    {SAI_LAG_MEMBER_ATTR_WEIGHT, Kind::U32, 1, false},
};
#define NSTUB(x) (sizeof(x) / sizeof((x)[0]))

static LagObj* lag_of(sai_object_id_t oid)
{
    if (oid_type(oid) != SAI_OBJECT_TYPE_LAG) return nullptr;
    return lookup(ctx().lags, oid);
}

// --------------------------------------------------------- attachments
bool attach_valid(sai_object_id_t obj)
{
    if (lag_of(obj)) return true;
    if (oid_type(obj) != SAI_OBJECT_TYPE_PORT) return false;
    PortObj* p = lookup(ctx().ports, obj);
    return p && !p->cpu && !p->lag;
}

int attach_dest(sai_object_id_t obj)
{
    if (LagObj* l = lag_of(obj)) return lag_dest(l->tid);
    PortObj* p = oid_type(obj) == SAI_OBJECT_TYPE_PORT ? lookup(ctx().ports, obj) : nullptr;
    return p ? p->hw : -1;
}

std::vector<int> attach_phys(sai_object_id_t obj)
{
    auto& c = ctx();
    std::vector<int> v;
    if (LagObj* l = lag_of(obj)) {
        for (auto m : l->members)
            if (PortObj* p = lookup(c.ports, c.lag_members[m].port)) v.push_back(p->hw);
    } else if (PortObj* p = oid_type(obj) == SAI_OBJECT_TYPE_PORT ? lookup(c.ports, obj) : nullptr) {
        v.push_back(p->hw);
    }
    return v;
}

sai_status_t attach_pvid_set(sai_object_id_t obj, uint16_t vid)
{
    auto& c = ctx();
    sai_status_t res = SAI_STATUS_SUCCESS;
    if (LagObj* l = lag_of(obj)) {
        l->pvid = vid;
        for (auto m : l->members)
            if (PortObj* p = lookup(c.ports, c.lag_members[m].port)) {
                sai_status_t st = c.be->port_pvid_set(p->hw, vid);
                if (st == SAI_STATUS_SUCCESS) p->pvid = vid;
                else res = st;
            }
        return res;
    }
    PortObj* p = oid_type(obj) == SAI_OBJECT_TYPE_PORT ? lookup(c.ports, obj) : nullptr;
    if (!p) return SAI_STATUS_INVALID_PARAMETER;
    res = c.be->port_pvid_set(p->hw, vid);
    if (res == SAI_STATUS_SUCCESS) p->pvid = vid;
    return res;
}

// The SDK member list of a LAG, optionally without one member.
static std::vector<LagMember> sdk_members(const LagObj& l, sai_object_id_t skip = 0)
{
    auto& c = ctx();
    std::vector<LagMember> v;
    for (auto m : l.members) {
        if (m == skip) continue;
        LagMemberObj& o = c.lag_members[m];
        PortObj* p = lookup(c.ports, o.port);
        if (!p) continue;
        LagMember x;
        x.hw = p->hw;
        x.egress_disable = o.egress_disable;
        x.ingress_disable = o.ingress_disable;
        v.push_back(x);
    }
    return v;
}

template <size_t N>
static sai_status_t stub_store(const StubDef (&defs)[N], StubMap& m, const sai_attribute_t& a, uint32_t i)
{
    if (!find_stub(defs, N, a.id)) return attr_not_supported(i);
    m[a.id] = a.value;
    return SAI_STATUS_SUCCESS;
}

template <size_t N>
static sai_status_t stub_load(const StubDef (&defs)[N], StubMap& m, sai_attribute_t& a, uint32_t i)
{
    const StubDef* d = find_stub(defs, N, a.id);
    if (!d) return attr_not_supported(i);
    auto it = m.find(a.id);
    if (it != m.end()) a.value = it->second;
    else stub_default(*d, a.value);
    return SAI_STATUS_SUCCESS;
}

// -------------------------------------------------------------------- LAG
static sai_status_t create_lag(_Out_ sai_object_id_t* id, _In_ sai_object_id_t, _In_ uint32_t n,
                               _In_ const sai_attribute_t* attrs)
{
    return guarded(SAI_API_LAG, __func__, [&]() -> sai_status_t {
        auto& c = ctx();
        if (!id || (n && !attrs)) return SAI_STATUS_INVALID_PARAMETER;
        LagObj l;
        for (uint32_t i = 0; i < n; i++) {
            const sai_attribute_t& a = attrs[i];
            switch (a.id) {
            case SAI_LAG_ATTR_PORT_VLAN_ID:
                if (a.value.u16 < 1 || a.value.u16 > 4094) return attr_invalid(i);
                l.pvid = a.value.u16;
                break;
            case SAI_LAG_ATTR_INGRESS_ACL:
            case SAI_LAG_ATTR_EGRESS_ACL:
                if (a.value.oid != SAI_NULL_OBJECT_ID) return attr_not_supported(i);   // no ACLs
                break;
            default: {
                sai_status_t st = stub_store(lag_stubs, l.stub, a, i);
                if (st != SAI_STATUS_SUCCESS) return st;
            }
            }
        }
        sai_status_t st = c.be->lag_create(l.tid);
        if (st != SAI_STATUS_SUCCESS) return st;
        l.oid = c.alloc(SAI_OBJECT_TYPE_LAG);
        c.lags[l.oid] = l;
        *id = l.oid;
        OB_NOTICE(SAI_API_LAG, "LAG created (trunk %d)", l.tid);
        return SAI_STATUS_SUCCESS;
    });
}

static sai_status_t remove_lag(_In_ sai_object_id_t id)
{
    return guarded(SAI_API_LAG, __func__, [&]() -> sai_status_t {
        auto& c = ctx();
        LagObj* l = lag_of(id);
        if (!l) return SAI_STATUS_INVALID_OBJECT_ID;
        if (!l->members.empty() || bridge_port_of(id)) return SAI_STATUS_OBJECT_IN_USE;
        for (auto& r : c.rifs)
            if (r.second.port == id) return SAI_STATUS_OBJECT_IN_USE;
        sai_status_t st = c.be->lag_destroy(l->tid);
        if (st != SAI_STATUS_SUCCESS && st != SAI_STATUS_ITEM_NOT_FOUND) return st;
        c.lags.erase(id);
        return SAI_STATUS_SUCCESS;
    });
}

static sai_status_t set_lag_attribute(_In_ sai_object_id_t id, _In_ const sai_attribute_t* attr)
{
    return guarded(SAI_API_LAG, __func__, [&]() -> sai_status_t {
        LagObj* l = lag_of(id);
        if (!l) return SAI_STATUS_INVALID_OBJECT_ID;
        if (!attr) return SAI_STATUS_INVALID_PARAMETER;
        switch (attr->id) {
        case SAI_LAG_ATTR_PORT_VLAN_ID:
            if (attr->value.u16 < 1 || attr->value.u16 > 4094) return attr_invalid(0);
            return attr_backend_status(attach_pvid_set(id, attr->value.u16), 0);
        case SAI_LAG_ATTR_INGRESS_ACL:
        case SAI_LAG_ATTR_EGRESS_ACL:
            return attr->value.oid == SAI_NULL_OBJECT_ID ? SAI_STATUS_SUCCESS : attr_not_supported(0);
        case SAI_LAG_ATTR_PORT_LIST: return attr_invalid(0);   // read-only
        default: return stub_store(lag_stubs, l->stub, *attr, 0);
        }
    });
}

static sai_status_t get_lag_attribute(_In_ sai_object_id_t id, _In_ uint32_t n, _Inout_ sai_attribute_t* attrs)
{
    return guarded(SAI_API_LAG, __func__, [&]() -> sai_status_t {
        LagObj* l = lag_of(id);
        if (!l) return SAI_STATUS_INVALID_OBJECT_ID;
        if (n && !attrs) return SAI_STATUS_INVALID_PARAMETER;
        for (uint32_t i = 0; i < n; i++) {
            sai_attribute_t& a = attrs[i];
            sai_status_t st = SAI_STATUS_SUCCESS;
            switch (a.id) {
            case SAI_LAG_ATTR_PORT_LIST: st = fill_list(a.value.objlist, l->members); break;
            case SAI_LAG_ATTR_PORT_VLAN_ID: a.value.u16 = l->pvid; break;
            case SAI_LAG_ATTR_INGRESS_ACL:
            case SAI_LAG_ATTR_EGRESS_ACL: a.value.oid = SAI_NULL_OBJECT_ID; break;
            default: st = stub_load(lag_stubs, l->stub, a, i); break;
            }
            if (st != SAI_STATUS_SUCCESS) return st;
        }
        return SAI_STATUS_SUCCESS;
    });
}

// ------------------------------------------------------------- members
static sai_status_t create_lag_member(_Out_ sai_object_id_t* id, _In_ sai_object_id_t, _In_ uint32_t n,
                                      _In_ const sai_attribute_t* attrs)
{
    return guarded(SAI_API_LAG, __func__, [&]() -> sai_status_t {
        auto& c = ctx();
        if (!id || (n && !attrs)) return SAI_STATUS_INVALID_PARAMETER;
        LagMemberObj m;
        uint32_t lag_i = 0, port_i = 0;
        for (uint32_t i = 0; i < n; i++) {
            const sai_attribute_t& a = attrs[i];
            switch (a.id) {
            case SAI_LAG_MEMBER_ATTR_LAG_ID: m.lag = a.value.oid; lag_i = i; break;
            case SAI_LAG_MEMBER_ATTR_PORT_ID: m.port = a.value.oid; port_i = i; break;
            case SAI_LAG_MEMBER_ATTR_EGRESS_DISABLE: m.egress_disable = a.value.booldata; break;
            case SAI_LAG_MEMBER_ATTR_INGRESS_DISABLE: m.ingress_disable = a.value.booldata; break;
            default: {
                sai_status_t st = stub_store(member_stubs, m.stub, a, i);
                if (st != SAI_STATUS_SUCCESS) return st;
            }
            }
        }
        if (!m.lag || !m.port) return SAI_STATUS_MANDATORY_ATTRIBUTE_MISSING;
        LagObj* l = lag_of(m.lag);
        if (!l) return attr_invalid(lag_i);
        PortObj* p = oid_type(m.port) == SAI_OBJECT_TYPE_PORT ? lookup(c.ports, m.port) : nullptr;
        if (!p || p->cpu) return attr_invalid(port_i);
        if (p->lag) return SAI_STATUS_ITEM_ALREADY_EXISTS;
        // A member carries no L2/L3 state of its own (orchagent removes the
        // default bridge ports and VLAN 1 members first).
        bool in_use = bridge_port_of(m.port) != 0;
        for (auto& r : c.rifs)
            if (r.second.port == m.port) in_use = true;
        if (in_use) {
            OB_ERR(SAI_API_LAG, "LAG member: port %s still has a bridge port or router interface", p->sdk_name.c_str());
            return SAI_STATUS_OBJECT_IN_USE;
        }

        sai_object_id_t oid = c.alloc(SAI_OBJECT_TYPE_LAG_MEMBER);
        c.lag_members[oid] = m;
        l->members.push_back(oid);
        sai_status_t st = c.be->lag_set_members(l->tid, sdk_members(*l));
        if (st == SAI_STATUS_SUCCESS) {
            p->lag = m.lag;
            st = bridge_port_member_join(m.lag, p->hw);
            if (st == SAI_STATUS_SUCCESS) st = rif_member_join(m.lag, p->hw);
            if (st == SAI_STATUS_SUCCESS) {
                st = c.be->port_pvid_set(p->hw, l->pvid);
                if (st == SAI_STATUS_SUCCESS) p->pvid = l->pvid;
            }
            if (st != SAI_STATUS_SUCCESS) {
                rif_member_leave(m.lag, p->hw);
                bridge_port_member_leave(m.lag, p->hw);
                p->lag = 0;
                l->members.pop_back();
                c.be->lag_set_members(l->tid, sdk_members(*l));
                c.lag_members.erase(oid);
                return st;
            }
        } else {
            l->members.pop_back();
            c.lag_members.erase(oid);
            return st;
        }
        *id = oid;
        OB_NOTICE(SAI_API_LAG, "LAG trunk %d: member %s (egress %s, ingress %s)", l->tid, p->sdk_name.c_str(),
                  m.egress_disable ? "off" : "on", m.ingress_disable ? "off" : "on");
        return SAI_STATUS_SUCCESS;
    });
}

static sai_status_t remove_lag_member(_In_ sai_object_id_t id)
{
    return guarded(SAI_API_LAG, __func__, [&]() -> sai_status_t {
        auto& c = ctx();
        LagMemberObj* m = lookup(c.lag_members, id);
        if (!m) return SAI_STATUS_INVALID_OBJECT_ID;
        LagObj* l = lag_of(m->lag);
        PortObj* p = lookup(c.ports, m->port);
        if (l) {
            sai_status_t st = c.be->lag_set_members(l->tid, sdk_members(*l, id));
            if (st != SAI_STATUS_SUCCESS) return st;
            l->members.erase(std::remove(l->members.begin(), l->members.end(), id), l->members.end());
        }
        if (p) {
            rif_member_leave(m->lag, p->hw);
            bridge_port_member_leave(m->lag, p->hw);
            if (c.be->port_pvid_set(p->hw, 1) == SAI_STATUS_SUCCESS) p->pvid = 1;
            p->lag = 0;
        }
        c.lag_members.erase(id);
        return SAI_STATUS_SUCCESS;
    });
}

static sai_status_t set_lag_member_attribute(_In_ sai_object_id_t id, _In_ const sai_attribute_t* attr)
{
    return guarded(SAI_API_LAG, __func__, [&]() -> sai_status_t {
        auto& c = ctx();
        LagMemberObj* m = lookup(c.lag_members, id);
        if (!m) return SAI_STATUS_INVALID_OBJECT_ID;
        if (!attr) return SAI_STATUS_INVALID_PARAMETER;
        bool* flag = nullptr;
        switch (attr->id) {
        case SAI_LAG_MEMBER_ATTR_EGRESS_DISABLE: flag = &m->egress_disable; break;
        case SAI_LAG_MEMBER_ATTR_INGRESS_DISABLE: flag = &m->ingress_disable; break;
        case SAI_LAG_MEMBER_ATTR_LAG_ID:
        case SAI_LAG_MEMBER_ATTR_PORT_ID: return attr_invalid(0);   // create-only
        default: return stub_store(member_stubs, m->stub, *attr, 0);
        }
        LagObj* l = lag_of(m->lag);
        if (!l) return SAI_STATUS_FAILURE;
        bool old = *flag;
        *flag = attr->value.booldata;
        sai_status_t st = c.be->lag_set_members(l->tid, sdk_members(*l));
        if (st != SAI_STATUS_SUCCESS) *flag = old;
        return attr_backend_status(st, 0);
    });
}

static sai_status_t get_lag_member_attribute(_In_ sai_object_id_t id, _In_ uint32_t n, _Inout_ sai_attribute_t* attrs)
{
    return guarded(SAI_API_LAG, __func__, [&]() -> sai_status_t {
        LagMemberObj* m = lookup(ctx().lag_members, id);
        if (!m) return SAI_STATUS_INVALID_OBJECT_ID;
        if (n && !attrs) return SAI_STATUS_INVALID_PARAMETER;
        for (uint32_t i = 0; i < n; i++) {
            sai_attribute_t& a = attrs[i];
            sai_status_t st = SAI_STATUS_SUCCESS;
            switch (a.id) {
            case SAI_LAG_MEMBER_ATTR_LAG_ID: a.value.oid = m->lag; break;
            case SAI_LAG_MEMBER_ATTR_PORT_ID: a.value.oid = m->port; break;
            case SAI_LAG_MEMBER_ATTR_EGRESS_DISABLE: a.value.booldata = m->egress_disable; break;
            case SAI_LAG_MEMBER_ATTR_INGRESS_DISABLE: a.value.booldata = m->ingress_disable; break;
            default: st = stub_load(member_stubs, m->stub, a, i); break;
            }
            if (st != SAI_STATUS_SUCCESS) return st;
        }
        return SAI_STATUS_SUCCESS;
    });
}

static sai_lag_api_t make_lag_api()
{
    sai_lag_api_t a;
    std::memset(&a, 0, sizeof(a));
    a.create_lag = create_lag;
    a.remove_lag = remove_lag;
    a.set_lag_attribute = set_lag_attribute;
    a.get_lag_attribute = get_lag_attribute;
    a.create_lag_member = create_lag_member;
    a.remove_lag_member = remove_lag_member;
    a.set_lag_member_attribute = set_lag_member_attribute;
    a.get_lag_member_attribute = get_lag_member_attribute;
    return a;
}
const sai_lag_api_t lag_api = make_lag_api();

} // namespace ob
