// SPDX-License-Identifier: Apache-2.0
// HOSTIF (type NETDEV on a port, backed by a KNET netif + RX filter),
// HOSTIF_TRAP_GROUP, HOSTIF_TRAP (a few kinds programmed, the rest kept),
// HOSTIF_TABLE_ENTRY (kept; KNET per-port filters already deliver to the
// port's netdev, which is what SONiC's wildcard entry asks for).
#include "sai_internal.h"

namespace ob {

TrapKind trap_kind(int32_t t)
{
    switch (t) {
    case SAI_HOSTIF_TRAP_TYPE_ARP_REQUEST: return TrapKind::ArpRequest;
    case SAI_HOSTIF_TRAP_TYPE_ARP_RESPONSE: return TrapKind::ArpResponse;
    case SAI_HOSTIF_TRAP_TYPE_DHCP: case SAI_HOSTIF_TRAP_TYPE_DHCP_L2: return TrapKind::Dhcp;
    case SAI_HOSTIF_TRAP_TYPE_DHCPV6: case SAI_HOSTIF_TRAP_TYPE_DHCPV6_L2: return TrapKind::Dhcpv6;
    case SAI_HOSTIF_TRAP_TYPE_IGMP_TYPE_QUERY: case SAI_HOSTIF_TRAP_TYPE_IGMP_TYPE_LEAVE:
    case SAI_HOSTIF_TRAP_TYPE_IGMP_TYPE_V1_REPORT: case SAI_HOSTIF_TRAP_TYPE_IGMP_TYPE_V2_REPORT:
    case SAI_HOSTIF_TRAP_TYPE_IGMP_TYPE_V3_REPORT: return TrapKind::Igmp;
    case SAI_HOSTIF_TRAP_TYPE_IPV6_MLD_V1_V2: case SAI_HOSTIF_TRAP_TYPE_IPV6_MLD_V1_REPORT:
    case SAI_HOSTIF_TRAP_TYPE_IPV6_MLD_V1_DONE: case SAI_HOSTIF_TRAP_TYPE_MLD_V2_REPORT: return TrapKind::Mld;
    case SAI_HOSTIF_TRAP_TYPE_IPV6_NEIGHBOR_DISCOVERY: case SAI_HOSTIF_TRAP_TYPE_IPV6_NEIGHBOR_SOLICITATION:
    case SAI_HOSTIF_TRAP_TYPE_IPV6_NEIGHBOR_ADVERTISEMENT: return TrapKind::Nd;
    case SAI_HOSTIF_TRAP_TYPE_LLDP: return TrapKind::Lldp;
    case SAI_HOSTIF_TRAP_TYPE_LACP: return TrapKind::Lacp;
    case SAI_HOSTIF_TRAP_TYPE_STP: case SAI_HOSTIF_TRAP_TYPE_PVRST: return TrapKind::Stp;
    default: return TrapKind::Other;
    }
}

static bool action_to_cpu(int32_t a)
{
    return a == SAI_PACKET_ACTION_TRAP || a == SAI_PACKET_ACTION_COPY || a == SAI_PACKET_ACTION_LOG;
}

// ------------------------------------------------------------------ hostif
static sai_status_t create_hostif(_Out_ sai_object_id_t* id, _In_ sai_object_id_t, _In_ uint32_t n,
                                  _In_ const sai_attribute_t* attrs)
{
    return guarded(SAI_API_HOSTIF, __func__, [&]() -> sai_status_t {
        auto& c = ctx();
        if (!id || (n && !attrs)) return SAI_STATUS_INVALID_PARAMETER;
        HostifObj h;
        bool ht = false, hn = false;
        uint32_t type_i = 0;
        for (uint32_t i = 0; i < n; i++) {
            const sai_attribute_t& a = attrs[i];
            switch (a.id) {
            case SAI_HOSTIF_ATTR_TYPE:
                if (a.value.s32 != SAI_HOSTIF_TYPE_NETDEV && a.value.s32 != SAI_HOSTIF_TYPE_GENETLINK) {
                    OB_ERR(SAI_API_HOSTIF, "hostif type %d not supported (NETDEV, GENETLINK)", a.value.s32);
                    return attr_not_supported(i);
                }
                h.type = a.value.s32;
                type_i = i;
                ht = true;
                break;
            case SAI_HOSTIF_ATTR_GENETLINK_MCGRP_NAME: {
                char nm[SAI_HOSTIF_GENETLINK_MCGRP_NAME_SIZE + 1] = {0};
                std::memcpy(nm, a.value.chardata, SAI_HOSTIF_GENETLINK_MCGRP_NAME_SIZE);
                h.mcgrp = nm;
                break;
            }
            case SAI_HOSTIF_ATTR_OBJ_ID: h.obj = a.value.oid; break;
            case SAI_HOSTIF_ATTR_NAME: {
                char nm[SAI_HOSTIF_NAME_SIZE + 1] = {0};
                std::memcpy(nm, a.value.chardata, SAI_HOSTIF_NAME_SIZE);
                h.name = nm;
                hn = true;
                break;
            }
            case SAI_HOSTIF_ATTR_OPER_STATUS: h.oper = a.value.booldata; break;
            case SAI_HOSTIF_ATTR_QUEUE: h.queue = a.value.u32; break;
            case SAI_HOSTIF_ATTR_VLAN_TAG: h.vlan_tag = a.value.s32; break;
            default: return attr_not_supported(i);
            }
        }
        if (ht && hn && h.type == SAI_HOSTIF_TYPE_GENETLINK) {
            // A generic-netlink family (sFlow: "psample" / "packets"), when the backend has one.
            if (!c.be->mirror_supported()) {
                OB_ERR(SAI_API_HOSTIF, "hostif %s: GENETLINK not supported by the backend", h.name.c_str());
                return attr_not_supported(type_i);
            }
            if (h.name.empty() || h.mcgrp.empty()) return SAI_STATUS_MANDATORY_ATTRIBUTE_MISSING;
            for (auto& o : c.hostifs)
                if (o.second.name == h.name) return SAI_STATUS_ITEM_ALREADY_EXISTS;
            sai_status_t st = c.be->genetlink_create(h.name, h.mcgrp, h.genl);
            if (st == SAI_STATUS_NOT_SUPPORTED) {
                OB_ERR(SAI_API_HOSTIF, "hostif %s: GENETLINK not supported by the backend", h.name.c_str());
                return attr_not_supported(type_i);
            }
            if (st != SAI_STATUS_SUCCESS) return st;
            sai_object_id_t oid = c.alloc(SAI_OBJECT_TYPE_HOSTIF);
            c.hostifs[oid] = h;
            *id = oid;
            OB_NOTICE(SAI_API_HOSTIF, "hostif %s: genetlink, group %s (backend %d)", h.name.c_str(), h.mcgrp.c_str(), h.genl);
            return SAI_STATUS_SUCCESS;
        }
        if (!ht || !hn || !h.obj) return SAI_STATUS_MANDATORY_ATTRIBUTE_MISSING;
        if (h.name.empty() || h.name.size() > 15) return SAI_STATUS_INVALID_PARAMETER;
        PortObj* p = oid_type(h.obj) == SAI_OBJECT_TYPE_PORT ? lookup(c.ports, h.obj) : nullptr;
        if (!p || p->cpu) {
            OB_ERR(SAI_API_HOSTIF, "hostif %s: only front-panel PORT objects are supported", h.name.c_str());
            return SAI_STATUS_NOT_SUPPORTED;
        }
        for (auto& o : c.hostifs)
            if (o.second.name == h.name || o.second.obj == h.obj) return SAI_STATUS_ITEM_ALREADY_EXISTS;
        sai_status_t st = c.be->netif_create(h.name, p->hw, p->pvid, c.src_mac,
                                             h.vlan_tag != SAI_HOSTIF_VLAN_TAG_STRIP, h.netif, h.filter);
        if (st != SAI_STATUS_SUCCESS) {
            OB_ERR(SAI_API_HOSTIF, "hostif %s: netif create failed (%d)", h.name.c_str(), st);
            return st;
        }
        c.be->netif_carrier_set(h.name, h.oper);
        sai_object_id_t oid = c.alloc(SAI_OBJECT_TYPE_HOSTIF);
        c.hostifs[oid] = h;
        *id = oid;
        OB_NOTICE(SAI_API_HOSTIF, "hostif %s on %s (netif %d, filter %d)", h.name.c_str(), p->sdk_name.c_str(),
                  h.netif, h.filter);
        return SAI_STATUS_SUCCESS;
    });
}

static sai_status_t remove_hostif(_In_ sai_object_id_t id)
{
    return guarded(SAI_API_HOSTIF, __func__, [&]() -> sai_status_t {
        auto& c = ctx();
        HostifObj* h = lookup(c.hostifs, id);
        if (!h) return SAI_STATUS_INVALID_OBJECT_ID;
        for (auto& t : c.table_entries)
            if (t.second.hostif == id) return SAI_STATUS_OBJECT_IN_USE;
        if (h->type == SAI_HOSTIF_TYPE_GENETLINK) {
            sai_status_t st = c.be->genetlink_destroy(h->genl);
            if (st != SAI_STATUS_SUCCESS && st != SAI_STATUS_ITEM_NOT_FOUND) return st;
            c.hostifs.erase(id);
            return SAI_STATUS_SUCCESS;
        }
        sai_status_t st = c.be->netif_destroy(h->netif, h->filter);
        if (st != SAI_STATUS_SUCCESS && st != SAI_STATUS_ITEM_NOT_FOUND) return st;
        c.hostifs.erase(id);
        return SAI_STATUS_SUCCESS;
    });
}

static sai_status_t set_hostif_attribute(_In_ sai_object_id_t id, _In_ const sai_attribute_t* attr)
{
    return guarded(SAI_API_HOSTIF, __func__, [&]() -> sai_status_t {
        auto& c = ctx();
        HostifObj* h = lookup(c.hostifs, id);
        if (!h) return SAI_STATUS_INVALID_OBJECT_ID;
        if (!attr) return SAI_STATUS_INVALID_PARAMETER;
        switch (attr->id) {
        case SAI_HOSTIF_ATTR_OPER_STATUS: {
            if (h->type == SAI_HOSTIF_TYPE_GENETLINK) { h->oper = attr->value.booldata; return SAI_STATUS_SUCCESS; }
            sai_status_t st = c.be->netif_carrier_set(h->name, attr->value.booldata);
            if (st == SAI_STATUS_SUCCESS) h->oper = attr->value.booldata;
            return st;
        }
        case SAI_HOSTIF_ATTR_VLAN_TAG: {
            int32_t t = attr->value.s32;
            if (t != SAI_HOSTIF_VLAN_TAG_STRIP && t != SAI_HOSTIF_VLAN_TAG_KEEP && t != SAI_HOSTIF_VLAN_TAG_ORIGINAL)
                return attr_invalid(0);
            PortObj* p = lookup(c.ports, h->obj);
            if (!p) return SAI_STATUS_FAILURE;
            int filter = -1;
            sai_status_t st = c.be->netif_set_keep_tag(h->netif, p->hw, t != SAI_HOSTIF_VLAN_TAG_STRIP, filter);
            if (st != SAI_STATUS_SUCCESS) return st;
            h->filter = filter;
            h->vlan_tag = t;
            return SAI_STATUS_SUCCESS;
        }
        case SAI_HOSTIF_ATTR_QUEUE: h->queue = attr->value.u32; return SAI_STATUS_SUCCESS;
        default: return attr_not_supported(0);
        }
    });
}

static sai_status_t get_hostif_attribute(_In_ sai_object_id_t id, _In_ uint32_t n, _Inout_ sai_attribute_t* attrs)
{
    return guarded(SAI_API_HOSTIF, __func__, [&]() -> sai_status_t {
        HostifObj* h = lookup(ctx().hostifs, id);
        if (!h) return SAI_STATUS_INVALID_OBJECT_ID;
        if (n && !attrs) return SAI_STATUS_INVALID_PARAMETER;
        for (uint32_t i = 0; i < n; i++) {
            sai_attribute_t& a = attrs[i];
            switch (a.id) {
            case SAI_HOSTIF_ATTR_TYPE: a.value.s32 = h->type; break;
            case SAI_HOSTIF_ATTR_OBJ_ID: a.value.oid = h->obj; break;
            case SAI_HOSTIF_ATTR_NAME:
                std::memset(a.value.chardata, 0, sizeof(a.value.chardata));
                std::memcpy(a.value.chardata, h->name.data(), std::min(h->name.size(), (size_t)SAI_HOSTIF_NAME_SIZE));
                break;
            case SAI_HOSTIF_ATTR_OPER_STATUS: a.value.booldata = h->oper; break;
            case SAI_HOSTIF_ATTR_QUEUE: a.value.u32 = h->queue; break;
            case SAI_HOSTIF_ATTR_VLAN_TAG: a.value.s32 = h->vlan_tag; break;
            case SAI_HOSTIF_ATTR_GENETLINK_MCGRP_NAME:
                std::memset(a.value.chardata, 0, sizeof(a.value.chardata));
                std::memcpy(a.value.chardata, h->mcgrp.data(), std::min(h->mcgrp.size(), (size_t)SAI_HOSTIF_GENETLINK_MCGRP_NAME_SIZE));
                break;
            default: return attr_not_supported(i);
            }
        }
        return SAI_STATUS_SUCCESS;
    });
}

// --------------------------------------------------------- trap groups
static sai_status_t trap_group_attr(TrapGroupObj& g, const sai_attribute_t& a, uint32_t i)
{
    switch (a.id) {
    case SAI_HOSTIF_TRAP_GROUP_ATTR_ADMIN_STATE: g.admin = a.value.booldata; return SAI_STATUS_SUCCESS;
    case SAI_HOSTIF_TRAP_GROUP_ATTR_QUEUE: g.queue = a.value.u32; return SAI_STATUS_SUCCESS;
    case SAI_HOSTIF_TRAP_GROUP_ATTR_POLICER:   // CoPP: field entries per trap (sai_acl.cpp)
        if (a.value.oid != SAI_NULL_OBJECT_ID && !policer_valid(a.value.oid)) return attr_invalid(i);
        g.policer = a.value.oid;
        return SAI_STATUS_SUCCESS;
    default: return attr_not_supported(i);
    }
}

static sai_status_t create_hostif_trap_group(_Out_ sai_object_id_t* id, _In_ sai_object_id_t, _In_ uint32_t n,
                                             _In_ const sai_attribute_t* attrs)
{
    return guarded(SAI_API_HOSTIF, __func__, [&]() -> sai_status_t {
        if (!id || (n && !attrs)) return SAI_STATUS_INVALID_PARAMETER;
        TrapGroupObj g;
        for (uint32_t i = 0; i < n; i++) {
            sai_status_t st = trap_group_attr(g, attrs[i], i);
            if (st != SAI_STATUS_SUCCESS) return st;
        }
        sai_object_id_t oid = ctx().alloc(SAI_OBJECT_TYPE_HOSTIF_TRAP_GROUP);
        ctx().trap_groups[oid] = g;
        *id = oid;
        return SAI_STATUS_SUCCESS;
    });
}

static sai_status_t remove_hostif_trap_group(_In_ sai_object_id_t id)
{
    return guarded(SAI_API_HOSTIF, __func__, [&]() -> sai_status_t {
        auto& c = ctx();
        if (!c.trap_groups.count(id)) return SAI_STATUS_INVALID_OBJECT_ID;
        if (id == c.default_trap_group) return SAI_STATUS_OBJECT_IN_USE;
        for (auto& t : c.traps)
            if (t.second.group == id) return SAI_STATUS_OBJECT_IN_USE;
        c.trap_groups.erase(id);
        return SAI_STATUS_SUCCESS;
    });
}

static sai_status_t set_hostif_trap_group_attribute(_In_ sai_object_id_t id, _In_ const sai_attribute_t* attr)
{
    return guarded(SAI_API_HOSTIF, __func__, [&]() -> sai_status_t {
        TrapGroupObj* g = lookup(ctx().trap_groups, id);
        if (!g) return SAI_STATUS_INVALID_OBJECT_ID;
        if (!attr) return SAI_STATUS_INVALID_PARAMETER;
        sai_status_t st = trap_group_attr(*g, *attr, 0);
        if (st == SAI_STATUS_SUCCESS && attr->id == SAI_HOSTIF_TRAP_GROUP_ATTR_POLICER) copp_refresh();
        else if (st == SAI_STATUS_SUCCESS) {   // queue / admin: only full traps carry them to the backend
            for (auto& t : ctx().traps)
                if (t.second.full && t.second.group == id) { copp_refresh(); break; }
        }
        return st;
    });
}

static sai_status_t get_hostif_trap_group_attribute(_In_ sai_object_id_t id, _In_ uint32_t n, _Inout_ sai_attribute_t* attrs)
{
    return guarded(SAI_API_HOSTIF, __func__, [&]() -> sai_status_t {
        TrapGroupObj* g = lookup(ctx().trap_groups, id);
        if (!g) return SAI_STATUS_INVALID_OBJECT_ID;
        if (n && !attrs) return SAI_STATUS_INVALID_PARAMETER;
        for (uint32_t i = 0; i < n; i++) {
            switch (attrs[i].id) {
            case SAI_HOSTIF_TRAP_GROUP_ATTR_ADMIN_STATE: attrs[i].value.booldata = g->admin; break;
            case SAI_HOSTIF_TRAP_GROUP_ATTR_QUEUE: attrs[i].value.u32 = g->queue; break;
            case SAI_HOSTIF_TRAP_GROUP_ATTR_POLICER: attrs[i].value.oid = g->policer; break;
            default: return attr_not_supported(i);
            }
        }
        return SAI_STATUS_SUCCESS;
    });
}

// ------------------------------------------------------------------ traps
TrapSpec trap_spec(const TrapObj& t)
{
    auto& c = ctx();
    TrapSpec s;
    s.type = t.type;
    s.action = t.action;
    s.priority = t.priority;
    if (TrapGroupObj* g = lookup(c.trap_groups, t.group)) {
        s.queue = g->queue;
        s.group_up = g->admin;
        if (PolicerObj* p = g->policer ? lookup(c.policers, g->policer) : nullptr) {
            s.policer = p->pid;
            s.yellow_drop = p->yellow == SAI_PACKET_ACTION_DROP;
            s.red_drop = p->red == SAI_PACKET_ACTION_DROP;
        }
    }
    return s;
}

// A backend with trap_program takes the whole trap (full); otherwise the
// trap kind's switch control (trap_set) and CoPP field entries.
static sai_status_t trap_program(TrapObj& t, int32_t action)
{
    TrapObj want = t;
    want.action = action;
    sai_status_t fs = ctx().be->trap_program(trap_spec(want), false);
    if (fs != SAI_STATUS_NOT_SUPPORTED) {
        if (fs == SAI_STATUS_SUCCESS) t.programmed = t.full = true;
        return fs;
    }
    TrapKind k = trap_kind(t.type);
    sai_status_t st = ctx().be->trap_set(k, action_to_cpu(action));
    if (st == SAI_STATUS_NOT_SUPPORTED) {
        t.programmed = false;
        OB_NOTICE(SAI_API_HOSTIF, "trap type %d kept but not programmed (no SDK control for it)", t.type);
        return SAI_STATUS_SUCCESS;
    }
    if (st == SAI_STATUS_SUCCESS) t.programmed = true;
    return st;
}

static sai_status_t create_hostif_trap(_Out_ sai_object_id_t* id, _In_ sai_object_id_t, _In_ uint32_t n,
                                       _In_ const sai_attribute_t* attrs)
{
    return guarded(SAI_API_HOSTIF, __func__, [&]() -> sai_status_t {
        auto& c = ctx();
        if (!id || (n && !attrs)) return SAI_STATUS_INVALID_PARAMETER;
        TrapObj t;
        t.group = c.default_trap_group;
        bool ht = false, ha = false;
        for (uint32_t i = 0; i < n; i++) {
            const sai_attribute_t& a = attrs[i];
            switch (a.id) {
            case SAI_HOSTIF_TRAP_ATTR_TRAP_TYPE: t.type = a.value.s32; ht = true; break;
            case SAI_HOSTIF_TRAP_ATTR_PACKET_ACTION: t.action = a.value.s32; ha = true; break;
            case SAI_HOSTIF_TRAP_ATTR_TRAP_PRIORITY: t.priority = a.value.u32; break;
            case SAI_HOSTIF_TRAP_ATTR_TRAP_GROUP:
                if (a.value.oid && !c.trap_groups.count(a.value.oid)) return attr_invalid(i);
                t.group = a.value.oid ? a.value.oid : c.default_trap_group;
                break;
            default: return attr_not_supported(i);
            }
        }
        if (!ht || !ha) return SAI_STATUS_MANDATORY_ATTRIBUTE_MISSING;
        for (auto& o : c.traps)
            if (o.second.type == t.type) return SAI_STATUS_ITEM_ALREADY_EXISTS;
        sai_status_t st = trap_program(t, t.action);
        if (st != SAI_STATUS_SUCCESS) return st;
        sai_object_id_t oid = c.alloc(SAI_OBJECT_TYPE_HOSTIF_TRAP);
        c.traps[oid] = t;
        *id = oid;
        copp_refresh();
        return SAI_STATUS_SUCCESS;
    });
}

static sai_status_t remove_hostif_trap(_In_ sai_object_id_t id)
{
    return guarded(SAI_API_HOSTIF, __func__, [&]() -> sai_status_t {
        auto& c = ctx();
        TrapObj* t = lookup(c.traps, id);
        if (!t) return SAI_STATUS_INVALID_OBJECT_ID;
        if (t->full) c.be->trap_program(trap_spec(*t), true);
        else if (t->programmed) c.be->trap_set(trap_kind(t->type), false);
        c.traps.erase(id);
        copp_refresh();
        return SAI_STATUS_SUCCESS;
    });
}

static sai_status_t set_hostif_trap_attribute(_In_ sai_object_id_t id, _In_ const sai_attribute_t* attr)
{
    return guarded(SAI_API_HOSTIF, __func__, [&]() -> sai_status_t {
        auto& c = ctx();
        TrapObj* t = lookup(c.traps, id);
        if (!t) return SAI_STATUS_INVALID_OBJECT_ID;
        if (!attr) return SAI_STATUS_INVALID_PARAMETER;
        switch (attr->id) {
        case SAI_HOSTIF_TRAP_ATTR_PACKET_ACTION: {
            sai_status_t st = trap_program(*t, attr->value.s32);
            if (st == SAI_STATUS_SUCCESS) t->action = attr->value.s32;
            copp_refresh();
            return st;
        }
        case SAI_HOSTIF_TRAP_ATTR_TRAP_PRIORITY:
            t->priority = attr->value.u32;
            if (t->full) copp_refresh();
            return SAI_STATUS_SUCCESS;
        case SAI_HOSTIF_TRAP_ATTR_TRAP_GROUP:
            if (attr->value.oid && !c.trap_groups.count(attr->value.oid)) return attr_invalid(0);
            t->group = attr->value.oid ? attr->value.oid : c.default_trap_group;
            copp_refresh();
            return SAI_STATUS_SUCCESS;
        default: return attr_not_supported(0);
        }
    });
}

static sai_status_t get_hostif_trap_attribute(_In_ sai_object_id_t id, _In_ uint32_t n, _Inout_ sai_attribute_t* attrs)
{
    return guarded(SAI_API_HOSTIF, __func__, [&]() -> sai_status_t {
        TrapObj* t = lookup(ctx().traps, id);
        if (!t) return SAI_STATUS_INVALID_OBJECT_ID;
        if (n && !attrs) return SAI_STATUS_INVALID_PARAMETER;
        for (uint32_t i = 0; i < n; i++) {
            switch (attrs[i].id) {
            case SAI_HOSTIF_TRAP_ATTR_TRAP_TYPE: attrs[i].value.s32 = t->type; break;
            case SAI_HOSTIF_TRAP_ATTR_PACKET_ACTION: attrs[i].value.s32 = t->action; break;
            case SAI_HOSTIF_TRAP_ATTR_TRAP_PRIORITY: attrs[i].value.u32 = t->priority; break;
            case SAI_HOSTIF_TRAP_ATTR_TRAP_GROUP: attrs[i].value.oid = t->group; break;
            default: return attr_not_supported(i);
            }
        }
        return SAI_STATUS_SUCCESS;
    });
}

// --------------------------------------------------------- table entries
static sai_status_t create_hostif_table_entry(_Out_ sai_object_id_t* id, _In_ sai_object_id_t, _In_ uint32_t n,
                                              _In_ const sai_attribute_t* attrs)
{
    return guarded(SAI_API_HOSTIF, __func__, [&]() -> sai_status_t {
        auto& c = ctx();
        if (!id || (n && !attrs)) return SAI_STATUS_INVALID_PARAMETER;
        HostifTableEntryObj e;
        bool ht = false, hc = false;
        for (uint32_t i = 0; i < n; i++) {
            const sai_attribute_t& a = attrs[i];
            switch (a.id) {
            case SAI_HOSTIF_TABLE_ENTRY_ATTR_TYPE: e.type = a.value.s32; ht = true; break;
            case SAI_HOSTIF_TABLE_ENTRY_ATTR_OBJ_ID: e.obj = a.value.oid; break;
            case SAI_HOSTIF_TABLE_ENTRY_ATTR_TRAP_ID: e.trap = a.value.oid; break;
            case SAI_HOSTIF_TABLE_ENTRY_ATTR_CHANNEL_TYPE: e.channel = a.value.s32; hc = true; break;
            case SAI_HOSTIF_TABLE_ENTRY_ATTR_HOST_IF: e.hostif = a.value.oid; break;
            default: return attr_not_supported(i);
            }
        }
        if (!ht || !hc) return SAI_STATUS_MANDATORY_ATTRIBUTE_MISSING;
        if (e.channel == SAI_HOSTIF_TABLE_ENTRY_CHANNEL_TYPE_GENETLINK) {
            // A trap's packets to a genetlink hostif (sFlow samples to psample).
            HostifObj* h = lookup(c.hostifs, e.hostif);
            TrapObj* t = lookup(c.traps, e.trap);
            if (!h || h->type != SAI_HOSTIF_TYPE_GENETLINK || e.type != SAI_HOSTIF_TABLE_ENTRY_TYPE_TRAP_ID || !t)
                return SAI_STATUS_INVALID_PARAMETER;
            sai_status_t st = c.be->genetlink_trap_set(h->genl, t->type, true);
            if (st != SAI_STATUS_SUCCESS) return st;
            sai_object_id_t oid = c.alloc(SAI_OBJECT_TYPE_HOSTIF_TABLE_ENTRY);
            c.table_entries[oid] = e;
            *id = oid;
            return SAI_STATUS_SUCCESS;
        }
        bool native = e.type == SAI_HOSTIF_TABLE_ENTRY_TYPE_WILDCARD &&
                      (e.channel == SAI_HOSTIF_TABLE_ENTRY_CHANNEL_TYPE_NETDEV_PHYSICAL_PORT ||
                       e.channel == SAI_HOSTIF_TABLE_ENTRY_CHANNEL_TYPE_NETDEV_LOGICAL_PORT);
        if (!native)
            OB_NOTICE(SAI_API_HOSTIF, "hostif table entry type %d channel %d kept but not programmed", e.type, e.channel);
        sai_object_id_t oid = c.alloc(SAI_OBJECT_TYPE_HOSTIF_TABLE_ENTRY);
        c.table_entries[oid] = e;
        *id = oid;
        return SAI_STATUS_SUCCESS;
    });
}

static sai_status_t remove_hostif_table_entry(_In_ sai_object_id_t id)
{
    return guarded(SAI_API_HOSTIF, __func__, [&]() -> sai_status_t {
        auto& c = ctx();
        HostifTableEntryObj* e = lookup(c.table_entries, id);
        if (!e) return SAI_STATUS_INVALID_OBJECT_ID;
        if (e->channel == SAI_HOSTIF_TABLE_ENTRY_CHANNEL_TYPE_GENETLINK) {
            HostifObj* h = lookup(c.hostifs, e->hostif);
            TrapObj* t = lookup(c.traps, e->trap);
            if (h && t) {
                sai_status_t st = c.be->genetlink_trap_set(h->genl, t->type, false);
                if (st != SAI_STATUS_SUCCESS && st != SAI_STATUS_ITEM_NOT_FOUND) return st;
            }
        }
        c.table_entries.erase(id);
        return SAI_STATUS_SUCCESS;
    });
}

static sai_status_t set_hostif_table_entry_attribute(_In_ sai_object_id_t, _In_ const sai_attribute_t*)
{
    return attr_invalid(0);   // all attributes are create-only
}

static sai_status_t get_hostif_table_entry_attribute(_In_ sai_object_id_t id, _In_ uint32_t n, _Inout_ sai_attribute_t* attrs)
{
    return guarded(SAI_API_HOSTIF, __func__, [&]() -> sai_status_t {
        HostifTableEntryObj* e = lookup(ctx().table_entries, id);
        if (!e) return SAI_STATUS_INVALID_OBJECT_ID;
        if (n && !attrs) return SAI_STATUS_INVALID_PARAMETER;
        for (uint32_t i = 0; i < n; i++) {
            switch (attrs[i].id) {
            case SAI_HOSTIF_TABLE_ENTRY_ATTR_TYPE: attrs[i].value.s32 = e->type; break;
            case SAI_HOSTIF_TABLE_ENTRY_ATTR_OBJ_ID: attrs[i].value.oid = e->obj; break;
            case SAI_HOSTIF_TABLE_ENTRY_ATTR_TRAP_ID: attrs[i].value.oid = e->trap; break;
            case SAI_HOSTIF_TABLE_ENTRY_ATTR_CHANNEL_TYPE: attrs[i].value.s32 = e->channel; break;
            case SAI_HOSTIF_TABLE_ENTRY_ATTR_HOST_IF: attrs[i].value.oid = e->hostif; break;
            default: return attr_not_supported(i);
            }
        }
        return SAI_STATUS_SUCCESS;
    });
}

static sai_status_t create_user_trap(_Out_ sai_object_id_t*, _In_ sai_object_id_t, _In_ uint32_t, _In_ const sai_attribute_t*)
{
    return SAI_STATUS_NOT_SUPPORTED;
}

static sai_hostif_api_t make_hostif_api()
{
    sai_hostif_api_t a;
    std::memset(&a, 0, sizeof(a));
    a.create_hostif = create_hostif;
    a.remove_hostif = remove_hostif;
    a.set_hostif_attribute = set_hostif_attribute;
    a.get_hostif_attribute = get_hostif_attribute;
    a.create_hostif_table_entry = create_hostif_table_entry;
    a.remove_hostif_table_entry = remove_hostif_table_entry;
    a.set_hostif_table_entry_attribute = set_hostif_table_entry_attribute;
    a.get_hostif_table_entry_attribute = get_hostif_table_entry_attribute;
    a.create_hostif_trap_group = create_hostif_trap_group;
    a.remove_hostif_trap_group = remove_hostif_trap_group;
    a.set_hostif_trap_group_attribute = set_hostif_trap_group_attribute;
    a.get_hostif_trap_group_attribute = get_hostif_trap_group_attribute;
    a.create_hostif_trap = create_hostif_trap;
    a.remove_hostif_trap = remove_hostif_trap;
    a.set_hostif_trap_attribute = set_hostif_trap_attribute;
    a.get_hostif_trap_attribute = get_hostif_trap_attribute;
    a.create_hostif_user_defined_trap = create_user_trap;
    return a;
}
const sai_hostif_api_t hostif_api = make_hostif_api();

} // namespace ob
