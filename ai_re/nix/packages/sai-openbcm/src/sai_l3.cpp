// SPDX-License-Identifier: Apache-2.0
// Milestone 2: VIRTUAL_ROUTER (default only), ROUTER_INTERFACE (PORT, VLAN,
// LOOPBACK), NEIGHBOR_ENTRY, NEXT_HOP (IP), ROUTE_ENTRY on SDK L3 egress
// objects. Next-hop groups are not implemented.
//
// SDK mapping: a port RIF gets an internal VLAN (counting down from
// SAI_OPENBCM_RIF_VLAN_BASE, default 4094) with the port as untagged member
// and PVID, and an L3 interface on that VLAN; a VLAN RIF is an L3 interface
// on the VLAN. The router MAC is a my-station entry. A neighbor is an egress
// object (+ host entry unless NO_HOST_ROUTE); a next hop reuses its
// neighbor's egress object (or the shared CPU egress until the neighbor
// exists); routes point at an egress object (CPU for IP2ME / connected /
// TRAP, drop for DROP). A port RIF may sit on a LAG: every member port is
// in the internal VLAN, the egress objects point at the trunk.
#include "sai_internal.h"

#include <cstdlib>

namespace ob {

static const StubDef rif_stubs[] = {
    {SAI_ROUTER_INTERFACE_ATTR_NEIGHBOR_MISS_PACKET_ACTION, Kind::S32, SAI_PACKET_ACTION_TRAP, false},
    {SAI_ROUTER_INTERFACE_ATTR_V4_MCAST_ENABLE, Kind::Bool, 0, false},
    {SAI_ROUTER_INTERFACE_ATTR_V6_MCAST_ENABLE, Kind::Bool, 0, false},
    {SAI_ROUTER_INTERFACE_ATTR_LOOPBACK_PACKET_ACTION, Kind::S32, SAI_PACKET_ACTION_FORWARD, false},
    {SAI_ROUTER_INTERFACE_ATTR_IS_VIRTUAL, Kind::Bool, 0, true},
    {SAI_ROUTER_INTERFACE_ATTR_NAT_ZONE_ID, Kind::U8, 0, false},
    {SAI_ROUTER_INTERFACE_ATTR_DISABLE_DECREMENT_TTL, Kind::Bool, 0, false},
    {SAI_ROUTER_INTERFACE_ATTR_ADMIN_MPLS_STATE, Kind::Bool, 0, false},
};
static const StubDef route_stubs[] = {
    {SAI_ROUTE_ENTRY_ATTR_META_DATA, Kind::U32, 0, false},
};
#define NSTUB(x) (sizeof(x) / sizeof((x)[0]))

static bool mac_eq(const uint8_t a[6], const uint8_t b[6]) { return std::memcmp(a, b, 6) == 0; }

// ---------------------------------------------------------- my-station
static sai_status_t ensure_switch_station()
{
    auto& c = ctx();
    if (c.switch_station >= 0) return SAI_STATUS_SUCCESS;
    return c.be->l3_station_add(c.src_mac, c.switch_station);
}

sai_status_t l3_switch_mac_changed(const uint8_t old_mac[6])
{
    auto& c = ctx();
    if (c.switch_station < 0 || mac_eq(old_mac, c.src_mac)) return SAI_STATUS_SUCCESS;
    int nid = -1;
    sai_status_t st = c.be->l3_station_add(c.src_mac, nid);
    if (st != SAI_STATUS_SUCCESS) return st;
    c.be->l3_station_delete(c.switch_station);
    c.switch_station = nid;
    return SAI_STATUS_SUCCESS;
}

// ------------------------------------------------------- virtual router
static sai_status_t create_vr(_Out_ sai_object_id_t*, _In_ sai_object_id_t, _In_ uint32_t, _In_ const sai_attribute_t*)
{
    OB_ERR(SAI_API_VIRTUAL_ROUTER, "only the default virtual router exists (VRFs not supported)");
    return SAI_STATUS_NOT_SUPPORTED;
}
static sai_status_t remove_vr(_In_ sai_object_id_t) { return SAI_STATUS_OBJECT_IN_USE; }
static sai_status_t set_vr(_In_ sai_object_id_t id, _In_ const sai_attribute_t* attr)
{
    return guarded(SAI_API_VIRTUAL_ROUTER, __func__, [&]() -> sai_status_t {
        if (id != ctx().default_vr) return SAI_STATUS_INVALID_OBJECT_ID;
        if (!attr) return SAI_STATUS_INVALID_PARAMETER;
        if ((attr->id == SAI_VIRTUAL_ROUTER_ATTR_ADMIN_V4_STATE || attr->id == SAI_VIRTUAL_ROUTER_ATTR_ADMIN_V6_STATE) &&
            attr->value.booldata)
            return SAI_STATUS_SUCCESS;
        return attr_not_supported(0);
    });
}
static sai_status_t get_vr(_In_ sai_object_id_t id, _In_ uint32_t n, _Inout_ sai_attribute_t* attrs)
{
    return guarded(SAI_API_VIRTUAL_ROUTER, __func__, [&]() -> sai_status_t {
        auto& c = ctx();
        if (id != c.default_vr) return SAI_STATUS_INVALID_OBJECT_ID;
        if (n && !attrs) return SAI_STATUS_INVALID_PARAMETER;
        for (uint32_t i = 0; i < n; i++) {
            sai_attribute_t& a = attrs[i];
            switch (a.id) {
            case SAI_VIRTUAL_ROUTER_ATTR_ADMIN_V4_STATE:
            case SAI_VIRTUAL_ROUTER_ATTR_ADMIN_V6_STATE: a.value.booldata = true; break;
            case SAI_VIRTUAL_ROUTER_ATTR_SRC_MAC_ADDRESS: std::memcpy(a.value.mac, c.src_mac, 6); break;
            case SAI_VIRTUAL_ROUTER_ATTR_VIOLATION_TTL1_PACKET_ACTION:
            case SAI_VIRTUAL_ROUTER_ATTR_VIOLATION_IP_OPTIONS_PACKET_ACTION: a.value.s32 = SAI_PACKET_ACTION_TRAP; break;
            case SAI_VIRTUAL_ROUTER_ATTR_UNKNOWN_L3_MULTICAST_PACKET_ACTION: a.value.s32 = SAI_PACKET_ACTION_DROP; break;
            default: return attr_not_supported(i);
            }
        }
        return SAI_STATUS_SUCCESS;
    });
}

static sai_virtual_router_api_t make_vr_api()
{
    sai_virtual_router_api_t a;
    std::memset(&a, 0, sizeof(a));
    a.create_virtual_router = create_vr;
    a.remove_virtual_router = remove_vr;
    a.set_virtual_router_attribute = set_vr;
    a.get_virtual_router_attribute = get_vr;
    return a;
}
const sai_virtual_router_api_t vr_api = make_vr_api();

// ------------------------------------------------------ router interface
static int alloc_internal_vlan()
{
    auto& c = ctx();
    int base = std::atoi(profile_get("SAI_OPENBCM_RIF_VLAN_BASE", "4094").c_str());
    if (base < 2 || base > 4094) base = 4094;
    for (int v = base; v >= 2; v--)
        if (!c.vid2vlan.count((uint16_t)v) && !c.internal_vlans.count((uint16_t)v)) return v;
    return -1;
}

static void rif_undo_port_vlan(RifObj& r)
{
    auto& c = ctx();
    if (!r.internal_vlan) return;
    for (int hw : attach_phys(r.port)) c.be->vlan_port_remove(r.vid, hw);
    attach_pvid_set(r.port, 1);
    c.be->vlan_destroy(r.vid);
    c.internal_vlans.erase(r.vid);
    r.internal_vlan = false;
}

// A port joins / leaves a LAG with a port RIF: internal VLAN membership
// (the PVID is the LAG's, set by the LAG code).
sai_status_t rif_member_join(sai_object_id_t lag, int hw)
{
    auto& c = ctx();
    for (auto& r : c.rifs)
        if (r.second.type == SAI_ROUTER_INTERFACE_TYPE_PORT && r.second.port == lag && r.second.internal_vlan)
            return c.be->vlan_port_add(r.second.vid, hw, true);
    return SAI_STATUS_SUCCESS;
}

void rif_member_leave(sai_object_id_t lag, int hw)
{
    auto& c = ctx();
    for (auto& r : c.rifs)
        if (r.second.type == SAI_ROUTER_INTERFACE_TYPE_PORT && r.second.port == lag && r.second.internal_vlan)
            c.be->vlan_port_remove(r.second.vid, hw);
}

static sai_status_t create_rif(_Out_ sai_object_id_t* id, _In_ sai_object_id_t, _In_ uint32_t n,
                               _In_ const sai_attribute_t* attrs)
{
    return guarded(SAI_API_ROUTER_INTERFACE, __func__, [&]() -> sai_status_t {
        auto& c = ctx();
        if (!id || (n && !attrs)) return SAI_STATUS_INVALID_PARAMETER;
        RifObj r;
        std::memcpy(r.mac, c.src_mac, 6);
        bool hvr = false, ht = false;
        for (uint32_t i = 0; i < n; i++) {
            const sai_attribute_t& a = attrs[i];
            switch (a.id) {
            case SAI_ROUTER_INTERFACE_ATTR_VIRTUAL_ROUTER_ID:
                if (a.value.oid != c.default_vr) return attr_invalid(i);
                r.vr = a.value.oid;
                hvr = true;
                break;
            case SAI_ROUTER_INTERFACE_ATTR_TYPE: r.type = a.value.s32; ht = true; break;
            case SAI_ROUTER_INTERFACE_ATTR_PORT_ID: r.port = a.value.oid; break;
            case SAI_ROUTER_INTERFACE_ATTR_VLAN_ID: r.vlan = a.value.oid; break;
            case SAI_ROUTER_INTERFACE_ATTR_SRC_MAC_ADDRESS: std::memcpy(r.mac, a.value.mac, 6); break;
            case SAI_ROUTER_INTERFACE_ATTR_MTU: r.mtu = a.value.u32; break;
            case SAI_ROUTER_INTERFACE_ATTR_ADMIN_V4_STATE: r.v4 = a.value.booldata; break;
            case SAI_ROUTER_INTERFACE_ATTR_ADMIN_V6_STATE: r.v6 = a.value.booldata; break;
            default:
                if (!find_stub(rif_stubs, NSTUB(rif_stubs), a.id)) return attr_not_supported(i);
                r.stub[a.id] = a.value;
            }
        }
        if (!hvr || !ht) return SAI_STATUS_MANDATORY_ATTRIBUTE_MISSING;
        if (r.type == SAI_ROUTER_INTERFACE_TYPE_PORT) {
            if (!r.port) return SAI_STATUS_MANDATORY_ATTRIBUTE_MISSING;
            if (!attach_valid(r.port)) return SAI_STATUS_INVALID_PARAMETER;   // CPU, LAG member, unknown
            for (auto& o : c.rifs)
                if (o.second.type == SAI_ROUTER_INTERFACE_TYPE_PORT && o.second.port == r.port)
                    return SAI_STATUS_ITEM_ALREADY_EXISTS;
            int v = alloc_internal_vlan();
            if (v < 0) return SAI_STATUS_INSUFFICIENT_RESOURCES;
            sai_status_t st = c.be->vlan_create((uint16_t)v);
            if (st != SAI_STATUS_SUCCESS) return st;
            r.vid = (uint16_t)v;
            r.internal_vlan = true;
            c.internal_vlans.insert(r.vid);
            for (int hw : attach_phys(r.port))   // a LAG: every member port
                if (st == SAI_STATUS_SUCCESS) st = c.be->vlan_port_add(r.vid, hw, true);
            if (st == SAI_STATUS_SUCCESS) st = attach_pvid_set(r.port, r.vid);
            if (st != SAI_STATUS_SUCCESS) {
                rif_undo_port_vlan(r);
                return st;
            }
        } else if (r.type == SAI_ROUTER_INTERFACE_TYPE_VLAN) {
            if (!r.vlan) return SAI_STATUS_MANDATORY_ATTRIBUTE_MISSING;
            VlanObj* v = oid_type(r.vlan) == SAI_OBJECT_TYPE_VLAN ? lookup(c.vlans, r.vlan) : nullptr;
            if (!v) return SAI_STATUS_INVALID_PARAMETER;
            for (auto& o : c.rifs)
                if (o.second.vlan == r.vlan) return SAI_STATUS_ITEM_ALREADY_EXISTS;
            r.vid = v->vid;
        } else if (r.type == SAI_ROUTER_INTERFACE_TYPE_LOOPBACK) {
            // No SDK object: loopback RIFs only anchor IP2ME routes.
        } else {
            OB_ERR(SAI_API_ROUTER_INTERFACE, "router interface type %d not supported", r.type);
            return SAI_STATUS_NOT_SUPPORTED;
        }
        if (r.type != SAI_ROUTER_INTERFACE_TYPE_LOOPBACK) {
            sai_status_t st = ensure_switch_station();
            if (st == SAI_STATUS_SUCCESS && !mac_eq(r.mac, c.src_mac)) st = c.be->l3_station_add(r.mac, r.station);
            if (st == SAI_STATUS_SUCCESS) st = c.be->l3_intf_create(r.mac, r.vid, r.mtu, r.intf);
            if (st != SAI_STATUS_SUCCESS) {
                if (r.station >= 0) c.be->l3_station_delete(r.station);
                rif_undo_port_vlan(r);
                return st;
            }
        }
        sai_object_id_t oid = c.alloc(SAI_OBJECT_TYPE_ROUTER_INTERFACE);
        c.rifs[oid] = r;
        *id = oid;
        OB_NOTICE(SAI_API_ROUTER_INTERFACE, "rif type %d vid %u intf %d", r.type, r.vid, r.intf);
        return SAI_STATUS_SUCCESS;
    });
}

static sai_status_t remove_rif(_In_ sai_object_id_t id)
{
    return guarded(SAI_API_ROUTER_INTERFACE, __func__, [&]() -> sai_status_t {
        auto& c = ctx();
        RifObj* r = lookup(c.rifs, id);
        if (!r) return SAI_STATUS_INVALID_OBJECT_ID;
        for (auto& nb : c.neighbors)
            if (nb.first.rif == id) return SAI_STATUS_OBJECT_IN_USE;
        for (auto& nh : c.next_hops)
            if (nh.second.rif == id) return SAI_STATUS_OBJECT_IN_USE;
        for (auto& rt : c.routes)
            if (rt.second.nh == id) return SAI_STATUS_OBJECT_IN_USE;
        if (r->intf >= 0) {
            sai_status_t st = c.be->l3_intf_destroy(r->intf);
            if (st != SAI_STATUS_SUCCESS) return st;
        }
        if (r->station >= 0) c.be->l3_station_delete(r->station);
        rif_undo_port_vlan(*r);
        c.rifs.erase(id);
        return SAI_STATUS_SUCCESS;
    });
}

static sai_status_t set_rif(_In_ sai_object_id_t id, _In_ const sai_attribute_t* attr)
{
    return guarded(SAI_API_ROUTER_INTERFACE, __func__, [&]() -> sai_status_t {
        auto& c = ctx();
        RifObj* r = lookup(c.rifs, id);
        if (!r) return SAI_STATUS_INVALID_OBJECT_ID;
        if (!attr) return SAI_STATUS_INVALID_PARAMETER;
        switch (attr->id) {
        case SAI_ROUTER_INTERFACE_ATTR_MTU: {
            if (r->intf >= 0) {
                sai_status_t st = c.be->l3_intf_update(r->intf, r->mac, r->vid, attr->value.u32);
                if (st != SAI_STATUS_SUCCESS) return st;
            }
            r->mtu = attr->value.u32;
            return SAI_STATUS_SUCCESS;
        }
        case SAI_ROUTER_INTERFACE_ATTR_SRC_MAC_ADDRESS: {
            if (r->intf >= 0) {
                int nst = -1;
                if (!mac_eq(attr->value.mac, c.src_mac)) {
                    sai_status_t st = c.be->l3_station_add(attr->value.mac, nst);
                    if (st != SAI_STATUS_SUCCESS) return st;
                }
                sai_status_t st = c.be->l3_intf_update(r->intf, attr->value.mac, r->vid, r->mtu);
                if (st != SAI_STATUS_SUCCESS) {
                    if (nst >= 0) c.be->l3_station_delete(nst);
                    return st;
                }
                if (r->station >= 0) c.be->l3_station_delete(r->station);
                r->station = nst;
            }
            std::memcpy(r->mac, attr->value.mac, 6);
            return SAI_STATUS_SUCCESS;
        }
        case SAI_ROUTER_INTERFACE_ATTR_ADMIN_V4_STATE: r->v4 = attr->value.booldata; return SAI_STATUS_SUCCESS;
        case SAI_ROUTER_INTERFACE_ATTR_ADMIN_V6_STATE: r->v6 = attr->value.booldata; return SAI_STATUS_SUCCESS;
        default: {
            const StubDef* d = find_stub(rif_stubs, NSTUB(rif_stubs), attr->id);
            if (!d) return attr_not_supported(0);
            if (d->create_only) return attr_invalid(0);
            r->stub[attr->id] = attr->value;
            return SAI_STATUS_SUCCESS;
        }
        }
    });
}

static sai_status_t get_rif(_In_ sai_object_id_t id, _In_ uint32_t n, _Inout_ sai_attribute_t* attrs)
{
    return guarded(SAI_API_ROUTER_INTERFACE, __func__, [&]() -> sai_status_t {
        RifObj* r = lookup(ctx().rifs, id);
        if (!r) return SAI_STATUS_INVALID_OBJECT_ID;
        if (n && !attrs) return SAI_STATUS_INVALID_PARAMETER;
        for (uint32_t i = 0; i < n; i++) {
            sai_attribute_t& a = attrs[i];
            switch (a.id) {
            case SAI_ROUTER_INTERFACE_ATTR_VIRTUAL_ROUTER_ID: a.value.oid = r->vr; break;
            case SAI_ROUTER_INTERFACE_ATTR_TYPE: a.value.s32 = r->type; break;
            case SAI_ROUTER_INTERFACE_ATTR_PORT_ID: a.value.oid = r->port; break;
            case SAI_ROUTER_INTERFACE_ATTR_VLAN_ID: a.value.oid = r->vlan; break;
            case SAI_ROUTER_INTERFACE_ATTR_SRC_MAC_ADDRESS: std::memcpy(a.value.mac, r->mac, 6); break;
            case SAI_ROUTER_INTERFACE_ATTR_MTU: a.value.u32 = r->mtu; break;
            case SAI_ROUTER_INTERFACE_ATTR_ADMIN_V4_STATE: a.value.booldata = r->v4; break;
            case SAI_ROUTER_INTERFACE_ATTR_ADMIN_V6_STATE: a.value.booldata = r->v6; break;
            default: {
                const StubDef* d = find_stub(rif_stubs, NSTUB(rif_stubs), a.id);
                if (!d) return attr_not_supported(i);
                auto it = r->stub.find(a.id);
                if (it != r->stub.end()) a.value = it->second;
                else stub_default(*d, a.value);
            }
            }
        }
        return SAI_STATUS_SUCCESS;
    });
}

static sai_status_t no_rif_stats(_In_ sai_object_id_t, _In_ uint32_t, _In_ const sai_stat_id_t*, _Out_ uint64_t*)
{
    return SAI_STATUS_NOT_SUPPORTED;
}

static sai_router_interface_api_t make_rif_api()
{
    sai_router_interface_api_t a;
    std::memset(&a, 0, sizeof(a));
    a.create_router_interface = create_rif;
    a.remove_router_interface = remove_rif;
    a.set_router_interface_attribute = set_rif;
    a.get_router_interface_attribute = get_rif;
    a.get_router_interface_stats = no_rif_stats;
    return a;
}
const sai_router_interface_api_t rif_api = make_rif_api();

// --------------------------------------------------------------- routes
static void reprogram_routes_using(sai_object_id_t nh, int egress)
{
    auto& c = ctx();
    for (auto& r : c.routes)
        if (r.second.nh == nh && r.second.action == SAI_PACKET_ACTION_FORWARD && r.second.egress != egress) {
            if (c.be->l3_route_add(r.first.p, egress, true) == SAI_STATUS_SUCCESS) r.second.egress = egress;
        }
}

// ------------------------------------------------------------- neighbors
static sai_status_t neighbor_egress(const NeighborKey& k, NeighborObj& o, int32_t action)
{
    auto& c = ctx();
    RifObj* r = lookup(c.rifs, k.rif);
    if (!r || r->intf < 0) return SAI_STATUS_INVALID_PARAMETER;
    if (action == SAI_PACKET_ACTION_DROP) return c.be->l3_egress_drop(o.egress);
    if (action == SAI_PACKET_ACTION_TRAP) return c.be->l3_egress_cpu(o.egress);
    int hw = -1;   // dest: port or LAG
    if (r->type == SAI_ROUTER_INTERFACE_TYPE_PORT) {
        hw = attach_dest(r->port);
    } else {
        FdbKey fk;
        std::memcpy(fk.mac, o.mac, 6);
        fk.bv = r->vlan;
        auto it = c.fdbs.find(fk);
        if (it != c.fdbs.end()) hw = bridge_port_dest(it->second.bport);
    }
    if (hw < 0) {
        OB_WARN(SAI_API_NEIGHBOR, "neighbor %s: egress port unknown (no FDB entry yet); sending to CPU",
                mac_str(o.mac).c_str());
        return c.be->l3_egress_cpu(o.egress);
    }
    o.hw = hw;
    return c.be->l3_egress_create(r->intf, o.mac, hw, r->vid, o.egress);
}

static bool egress_shared(int egress)
{
    int cpu = -1, drop = -1;
    ctx().be->l3_egress_cpu(cpu);
    ctx().be->l3_egress_drop(drop);
    return egress == cpu || egress == drop;
}

static sai_status_t create_neighbor_entry(_In_ const sai_neighbor_entry_t* e, _In_ uint32_t n,
                                          _In_ const sai_attribute_t* attrs)
{
    return guarded(SAI_API_NEIGHBOR, __func__, [&]() -> sai_status_t {
        auto& c = ctx();
        if (!e || (n && !attrs)) return SAI_STATUS_INVALID_PARAMETER;
        NeighborKey k{e->rif_id, to_ip(e->ip_address)};
        if (c.neighbors.count(k)) return SAI_STATUS_ITEM_ALREADY_EXISTS;
        RifObj* r = lookup(c.rifs, k.rif);
        if (!r) return SAI_STATUS_INVALID_PARAMETER;
        if (r->type == SAI_ROUTER_INTERFACE_TYPE_LOOPBACK) return SAI_STATUS_NOT_SUPPORTED;
        NeighborObj o;
        int32_t action = SAI_PACKET_ACTION_FORWARD;
        bool hm = false;
        for (uint32_t i = 0; i < n; i++) {
            switch (attrs[i].id) {
            case SAI_NEIGHBOR_ENTRY_ATTR_DST_MAC_ADDRESS: std::memcpy(o.mac, attrs[i].value.mac, 6); hm = true; break;
            case SAI_NEIGHBOR_ENTRY_ATTR_PACKET_ACTION:
                action = attrs[i].value.s32;
                if (action != SAI_PACKET_ACTION_FORWARD && action != SAI_PACKET_ACTION_DROP &&
                    action != SAI_PACKET_ACTION_TRAP)
                    return attr_not_supported(i);
                break;
            case SAI_NEIGHBOR_ENTRY_ATTR_NO_HOST_ROUTE: o.no_host = attrs[i].value.booldata; break;
            default: return attr_not_supported(i);
            }
        }
        if (!hm) return SAI_STATUS_MANDATORY_ATTRIBUTE_MISSING;
        sai_status_t st = neighbor_egress(k, o, action);
        if (st != SAI_STATUS_SUCCESS) return st;
        if (!o.no_host) {
            st = c.be->l3_host_add(k.ip, o.egress);
            if (st != SAI_STATUS_SUCCESS) {
                if (!egress_shared(o.egress)) c.be->l3_egress_destroy(o.egress);
                return st;
            }
            o.host = true;
        }
        c.neighbors[k] = o;
        // Next hops created before this neighbor pointed at the CPU.
        for (auto& nh : c.next_hops)
            if (nh.second.rif == k.rif && nh.second.ip == k.ip) {
                nh.second.egress = o.egress;
                reprogram_routes_using(nh.first, o.egress);
            }
        return SAI_STATUS_SUCCESS;
    });
}

static sai_status_t remove_neighbor_entry(_In_ const sai_neighbor_entry_t* e)
{
    return guarded(SAI_API_NEIGHBOR, __func__, [&]() -> sai_status_t {
        auto& c = ctx();
        if (!e) return SAI_STATUS_INVALID_PARAMETER;
        NeighborKey k{e->rif_id, to_ip(e->ip_address)};
        auto it = c.neighbors.find(k);
        if (it == c.neighbors.end()) return SAI_STATUS_ITEM_NOT_FOUND;
        for (auto& nh : c.next_hops)
            if (nh.second.rif == k.rif && nh.second.ip == k.ip) return SAI_STATUS_OBJECT_IN_USE;
        if (it->second.host) {
            sai_status_t st = c.be->l3_host_delete(k.ip);
            if (st != SAI_STATUS_SUCCESS && st != SAI_STATUS_ITEM_NOT_FOUND) return st;
        }
        if (!egress_shared(it->second.egress)) c.be->l3_egress_destroy(it->second.egress);
        c.neighbors.erase(it);
        return SAI_STATUS_SUCCESS;
    });
}

static sai_status_t set_neighbor_entry_attribute(_In_ const sai_neighbor_entry_t* e, _In_ const sai_attribute_t* attr)
{
    return guarded(SAI_API_NEIGHBOR, __func__, [&]() -> sai_status_t {
        auto& c = ctx();
        if (!e || !attr) return SAI_STATUS_INVALID_PARAMETER;
        NeighborKey k{e->rif_id, to_ip(e->ip_address)};
        auto it = c.neighbors.find(k);
        if (it == c.neighbors.end()) return SAI_STATUS_ITEM_NOT_FOUND;
        if (attr->id != SAI_NEIGHBOR_ENTRY_ATTR_DST_MAC_ADDRESS) return attr_not_supported(0);
        NeighborObj& o = it->second;
        RifObj* r = lookup(c.rifs, k.rif);
        if (!r) return SAI_STATUS_FAILURE;
        if (egress_shared(o.egress) || o.hw < 0) return attr_not_supported(0);
        sai_status_t st = c.be->l3_egress_update(o.egress, r->intf, attr->value.mac, o.hw, r->vid);
        if (st == SAI_STATUS_SUCCESS) std::memcpy(o.mac, attr->value.mac, 6);
        return st;
    });
}

static sai_status_t get_neighbor_entry_attribute(_In_ const sai_neighbor_entry_t* e, _In_ uint32_t n,
                                                 _Inout_ sai_attribute_t* attrs)
{
    return guarded(SAI_API_NEIGHBOR, __func__, [&]() -> sai_status_t {
        if (!e || (n && !attrs)) return SAI_STATUS_INVALID_PARAMETER;
        auto it = ctx().neighbors.find(NeighborKey{e->rif_id, to_ip(e->ip_address)});
        if (it == ctx().neighbors.end()) return SAI_STATUS_ITEM_NOT_FOUND;
        for (uint32_t i = 0; i < n; i++) {
            switch (attrs[i].id) {
            case SAI_NEIGHBOR_ENTRY_ATTR_DST_MAC_ADDRESS: std::memcpy(attrs[i].value.mac, it->second.mac, 6); break;
            case SAI_NEIGHBOR_ENTRY_ATTR_NO_HOST_ROUTE: attrs[i].value.booldata = it->second.no_host; break;
            default: return attr_not_supported(i);
            }
        }
        return SAI_STATUS_SUCCESS;
    });
}

static sai_status_t remove_all_neighbor_entries(_In_ sai_object_id_t) { return SAI_STATUS_NOT_SUPPORTED; }

static sai_neighbor_api_t make_neighbor_api()
{
    sai_neighbor_api_t a;
    std::memset(&a, 0, sizeof(a));
    a.create_neighbor_entry = create_neighbor_entry;
    a.remove_neighbor_entry = remove_neighbor_entry;
    a.set_neighbor_entry_attribute = set_neighbor_entry_attribute;
    a.get_neighbor_entry_attribute = get_neighbor_entry_attribute;
    a.remove_all_neighbor_entries = remove_all_neighbor_entries;
    return a;
}
const sai_neighbor_api_t neighbor_api = make_neighbor_api();

// -------------------------------------------------------------- next hops
static sai_status_t create_next_hop(_Out_ sai_object_id_t* id, _In_ sai_object_id_t, _In_ uint32_t n,
                                    _In_ const sai_attribute_t* attrs)
{
    return guarded(SAI_API_NEXT_HOP, __func__, [&]() -> sai_status_t {
        auto& c = ctx();
        if (!id || (n && !attrs)) return SAI_STATUS_INVALID_PARAMETER;
        NextHopObj o;
        bool ht = false, hip = false, hr = false;
        for (uint32_t i = 0; i < n; i++) {
            switch (attrs[i].id) {
            case SAI_NEXT_HOP_ATTR_TYPE:
                if (attrs[i].value.s32 != SAI_NEXT_HOP_TYPE_IP) return attr_not_supported(i);
                ht = true;
                break;
            case SAI_NEXT_HOP_ATTR_IP: o.ip = to_ip(attrs[i].value.ipaddr); hip = true; break;
            case SAI_NEXT_HOP_ATTR_ROUTER_INTERFACE_ID: o.rif = attrs[i].value.oid; hr = true; break;
            default: return attr_not_supported(i);
            }
        }
        if (!ht || !hip || !hr) return SAI_STATUS_MANDATORY_ATTRIBUTE_MISSING;
        if (!c.rifs.count(o.rif)) return SAI_STATUS_INVALID_PARAMETER;
        auto nb = c.neighbors.find(NeighborKey{o.rif, o.ip});
        if (nb != c.neighbors.end()) o.egress = nb->second.egress;
        else {
            OB_INFO(SAI_API_NEXT_HOP, "next hop without neighbor: points at the CPU until it resolves");
            sai_status_t st = c.be->l3_egress_cpu(o.egress);
            if (st != SAI_STATUS_SUCCESS) return st;
        }
        sai_object_id_t oid = c.alloc(SAI_OBJECT_TYPE_NEXT_HOP);
        c.next_hops[oid] = o;
        *id = oid;
        return SAI_STATUS_SUCCESS;
    });
}

static sai_status_t remove_next_hop(_In_ sai_object_id_t id)
{
    return guarded(SAI_API_NEXT_HOP, __func__, [&]() -> sai_status_t {
        auto& c = ctx();
        if (!c.next_hops.count(id)) return SAI_STATUS_INVALID_OBJECT_ID;
        for (auto& r : c.routes)
            if (r.second.nh == id) return SAI_STATUS_OBJECT_IN_USE;
        c.next_hops.erase(id);
        return SAI_STATUS_SUCCESS;
    });
}

static sai_status_t set_next_hop(_In_ sai_object_id_t, _In_ const sai_attribute_t*) { return attr_not_supported(0); }

static sai_status_t get_next_hop(_In_ sai_object_id_t id, _In_ uint32_t n, _Inout_ sai_attribute_t* attrs)
{
    return guarded(SAI_API_NEXT_HOP, __func__, [&]() -> sai_status_t {
        NextHopObj* o = lookup(ctx().next_hops, id);
        if (!o) return SAI_STATUS_INVALID_OBJECT_ID;
        if (n && !attrs) return SAI_STATUS_INVALID_PARAMETER;
        for (uint32_t i = 0; i < n; i++) {
            switch (attrs[i].id) {
            case SAI_NEXT_HOP_ATTR_TYPE: attrs[i].value.s32 = o->type; break;
            case SAI_NEXT_HOP_ATTR_IP: attrs[i].value.ipaddr = from_ip(o->ip); break;
            case SAI_NEXT_HOP_ATTR_ROUTER_INTERFACE_ID: attrs[i].value.oid = o->rif; break;
            default: return attr_not_supported(i);
            }
        }
        return SAI_STATUS_SUCCESS;
    });
}

static sai_next_hop_api_t make_next_hop_api()
{
    sai_next_hop_api_t a;
    std::memset(&a, 0, sizeof(a));
    a.create_next_hop = create_next_hop;
    a.remove_next_hop = remove_next_hop;
    a.set_next_hop_attribute = set_next_hop;
    a.get_next_hop_attribute = get_next_hop;
    return a;
}
const sai_next_hop_api_t next_hop_api = make_next_hop_api();

// ----------------------------------------------------------------- routes
static sai_status_t route_egress(int32_t action, sai_object_id_t nh, int& egress)
{
    auto& c = ctx();
    if (action == SAI_PACKET_ACTION_DROP || action == SAI_PACKET_ACTION_DENY) return c.be->l3_egress_drop(egress);
    if (action == SAI_PACKET_ACTION_TRAP) return c.be->l3_egress_cpu(egress);
    if (action != SAI_PACKET_ACTION_FORWARD) return SAI_STATUS_NOT_SUPPORTED;
    switch (oid_type(nh)) {
    case SAI_OBJECT_TYPE_NULL: return c.be->l3_egress_drop(egress);   // forward to nothing
    case SAI_OBJECT_TYPE_NEXT_HOP: {
        NextHopObj* o = lookup(c.next_hops, nh);
        if (!o) return SAI_STATUS_INVALID_PARAMETER;
        egress = o->egress;
        return SAI_STATUS_SUCCESS;
    }
    case SAI_OBJECT_TYPE_ROUTER_INTERFACE:   // directly connected subnet: glean via the CPU
        if (!c.rifs.count(nh)) return SAI_STATUS_INVALID_PARAMETER;
        return c.be->l3_egress_cpu(egress);
    case SAI_OBJECT_TYPE_PORT:               // IP2ME: next hop is the CPU port
        if (nh != c.cpu_port) return SAI_STATUS_INVALID_PARAMETER;
        return c.be->l3_egress_cpu(egress);
    case SAI_OBJECT_TYPE_NEXT_HOP_GROUP:
        OB_ERR(SAI_API_ROUTE, "next hop groups (ECMP) not supported");
        return SAI_STATUS_NOT_SUPPORTED;
    default:
        return SAI_STATUS_INVALID_PARAMETER;
    }
}

static sai_status_t create_route_entry(_In_ const sai_route_entry_t* e, _In_ uint32_t n, _In_ const sai_attribute_t* attrs)
{
    return guarded(SAI_API_ROUTE, __func__, [&]() -> sai_status_t {
        auto& c = ctx();
        if (!e || (n && !attrs)) return SAI_STATUS_INVALID_PARAMETER;
        if (e->vr_id != c.default_vr) return SAI_STATUS_INVALID_PARAMETER;
        RouteKey k{e->vr_id, to_prefix(e->destination)};
        if (c.routes.count(k)) return SAI_STATUS_ITEM_ALREADY_EXISTS;
        RouteObj o;
        for (uint32_t i = 0; i < n; i++) {
            switch (attrs[i].id) {
            case SAI_ROUTE_ENTRY_ATTR_PACKET_ACTION: o.action = attrs[i].value.s32; break;
            case SAI_ROUTE_ENTRY_ATTR_NEXT_HOP_ID: o.nh = attrs[i].value.oid; break;
            default:
                if (!find_stub(route_stubs, NSTUB(route_stubs), attrs[i].id)) return attr_not_supported(i);
                o.stub[attrs[i].id] = attrs[i].value;
            }
        }
        sai_status_t st = route_egress(o.action, o.nh, o.egress);
        if (st != SAI_STATUS_SUCCESS) return st;
        st = c.be->l3_route_add(k.p, o.egress, false);
        if (st != SAI_STATUS_SUCCESS) return st;
        c.routes[k] = o;
        return SAI_STATUS_SUCCESS;
    });
}

static sai_status_t remove_route_entry(_In_ const sai_route_entry_t* e)
{
    return guarded(SAI_API_ROUTE, __func__, [&]() -> sai_status_t {
        auto& c = ctx();
        if (!e) return SAI_STATUS_INVALID_PARAMETER;
        auto it = c.routes.find(RouteKey{e->vr_id, to_prefix(e->destination)});
        if (it == c.routes.end()) return SAI_STATUS_ITEM_NOT_FOUND;
        sai_status_t st = c.be->l3_route_delete(it->first.p);
        if (st != SAI_STATUS_SUCCESS && st != SAI_STATUS_ITEM_NOT_FOUND) return st;
        c.routes.erase(it);
        return SAI_STATUS_SUCCESS;
    });
}

static sai_status_t set_route_entry_attribute(_In_ const sai_route_entry_t* e, _In_ const sai_attribute_t* attr)
{
    return guarded(SAI_API_ROUTE, __func__, [&]() -> sai_status_t {
        auto& c = ctx();
        if (!e || !attr) return SAI_STATUS_INVALID_PARAMETER;
        auto it = c.routes.find(RouteKey{e->vr_id, to_prefix(e->destination)});
        if (it == c.routes.end()) return SAI_STATUS_ITEM_NOT_FOUND;
        RouteObj o = it->second;
        if (attr->id == SAI_ROUTE_ENTRY_ATTR_PACKET_ACTION) o.action = attr->value.s32;
        else if (attr->id == SAI_ROUTE_ENTRY_ATTR_NEXT_HOP_ID) o.nh = attr->value.oid;
        else if (find_stub(route_stubs, NSTUB(route_stubs), attr->id)) {
            it->second.stub[attr->id] = attr->value;
            return SAI_STATUS_SUCCESS;
        } else return attr_not_supported(0);
        sai_status_t st = route_egress(o.action, o.nh, o.egress);
        if (st != SAI_STATUS_SUCCESS) return st;
        st = c.be->l3_route_add(it->first.p, o.egress, true);
        if (st != SAI_STATUS_SUCCESS) return st;
        it->second = o;
        return SAI_STATUS_SUCCESS;
    });
}

static sai_status_t get_route_entry_attribute(_In_ const sai_route_entry_t* e, _In_ uint32_t n, _Inout_ sai_attribute_t* attrs)
{
    return guarded(SAI_API_ROUTE, __func__, [&]() -> sai_status_t {
        if (!e || (n && !attrs)) return SAI_STATUS_INVALID_PARAMETER;
        auto it = ctx().routes.find(RouteKey{e->vr_id, to_prefix(e->destination)});
        if (it == ctx().routes.end()) return SAI_STATUS_ITEM_NOT_FOUND;
        for (uint32_t i = 0; i < n; i++) {
            switch (attrs[i].id) {
            case SAI_ROUTE_ENTRY_ATTR_PACKET_ACTION: attrs[i].value.s32 = it->second.action; break;
            case SAI_ROUTE_ENTRY_ATTR_NEXT_HOP_ID: attrs[i].value.oid = it->second.nh; break;
            default: {
                const StubDef* d = find_stub(route_stubs, NSTUB(route_stubs), attrs[i].id);
                if (!d) return attr_not_supported(i);
                auto s = it->second.stub.find(attrs[i].id);
                if (s != it->second.stub.end()) attrs[i].value = s->second;
                else stub_default(*d, attrs[i].value);
            }
            }
        }
        return SAI_STATUS_SUCCESS;
    });
}

static sai_route_api_t make_route_api()
{
    sai_route_api_t a;
    std::memset(&a, 0, sizeof(a));
    a.create_route_entry = create_route_entry;
    a.remove_route_entry = remove_route_entry;
    a.set_route_entry_attribute = set_route_entry_attribute;
    a.get_route_entry_attribute = get_route_entry_attribute;
    return a;
}
const sai_route_api_t route_api = make_route_api();


} // namespace ob
