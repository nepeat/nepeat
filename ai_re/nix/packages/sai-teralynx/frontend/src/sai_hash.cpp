// SPDX-License-Identifier: Apache-2.0
// SAI HASH (the switch's LAG_HASH and ECMP_HASH objects: NATIVE_HASH_FIELD_LIST)
// with SWITCH LAG_/ECMP_DEFAULT_HASH_SEED, PORT storm control policers, VLAN
// flood control. Backend hooks: hash_set, port_storm_set, vlan_flood_set
// (backend.h); a backend without them (NOT_SUPPORTED) gets the old
// behaviour: the attributes are kept, nothing is programmed.
#include "sai_internal.h"

namespace ob {

static uint32_t seed_of(bool lag)
{
    auto& s = ctx().switch_stub;
    auto it = s.find(lag ? SAI_SWITCH_ATTR_LAG_DEFAULT_HASH_SEED : SAI_SWITCH_ATTR_ECMP_DEFAULT_HASH_SEED);
    return it == s.end() ? 0 : it->second.u32;
}

void hash_build_objects()
{
    auto& c = ctx();
    c.lag_hash = make_oid(SAI_OBJECT_TYPE_HASH, 1);
    c.ecmp_hash = make_oid(SAI_OBJECT_TYPE_HASH, 2);
    c.next_index[SAI_OBJECT_TYPE_HASH] = 2;
    HashObj l, e;
    l.lag = true;
    c.hashes[c.lag_hash] = l;
    c.hashes[c.ecmp_hash] = e;
}

// NOT_SUPPORTED from the backend: kept (the caller stores the value).
static sai_status_t apply_hash(bool lag, const std::vector<int32_t>& fields, uint32_t seed, bool& programmed)
{
    sai_status_t st = ctx().be->hash_set(lag, fields, seed);
    programmed = st == SAI_STATUS_SUCCESS;
    if (st == SAI_STATUS_NOT_SUPPORTED) {
        OB_INFO(SAI_API_HASH, "%s hash kept, not programmed (backend has no hash control)", lag ? "LAG" : "ECMP");
        return SAI_STATUS_SUCCESS;
    }
    return st;
}

static bool hash_be() { return ctx().be && ctx().be->hash_supported(); }

sai_status_t switch_hash_set(const sai_attribute_t& a, bool& handled)
{
    handled = hash_be() && (a.id == SAI_SWITCH_ATTR_LAG_DEFAULT_HASH_SEED || a.id == SAI_SWITCH_ATTR_ECMP_DEFAULT_HASH_SEED);
    if (!handled) return SAI_STATUS_SUCCESS;
    auto& c = ctx();
    bool lag = a.id == SAI_SWITCH_ATTR_LAG_DEFAULT_HASH_SEED;
    HashObj* h = lookup(c.hashes, lag ? c.lag_hash : c.ecmp_hash);
    bool programmed = false;
    sai_status_t st = apply_hash(lag, h ? h->fields : std::vector<int32_t>(), a.value.u32, programmed);
    if (st != SAI_STATUS_SUCCESS) return attr_backend_status(st, 0);
    c.switch_stub[a.id] = a.value;   // the stub table still answers the get
    return SAI_STATUS_SUCCESS;
}

sai_status_t switch_hash_get(sai_attribute_t& a, bool& handled)
{
    handled = hash_be();
    if (!handled) return SAI_STATUS_SUCCESS;
    if (a.id == SAI_SWITCH_ATTR_LAG_HASH) a.value.oid = ctx().lag_hash;
    else if (a.id == SAI_SWITCH_ATTR_ECMP_HASH) a.value.oid = ctx().ecmp_hash;
    else handled = false;
    return SAI_STATUS_SUCCESS;
}

static sai_status_t create_hash(_Out_ sai_object_id_t*, _In_ sai_object_id_t, _In_ uint32_t, _In_ const sai_attribute_t*)
{
    OB_WARN(SAI_API_HASH, "hash objects are the switch's LAG_HASH / ECMP_HASH; create refused");
    return SAI_STATUS_NOT_SUPPORTED;
}

static sai_status_t remove_hash(_In_ sai_object_id_t) { return SAI_STATUS_NOT_SUPPORTED; }

static sai_status_t set_hash_attribute(_In_ sai_object_id_t id, _In_ const sai_attribute_t* a)
{
    return guarded(SAI_API_HASH, __func__, [&]() -> sai_status_t {
        HashObj* h = hash_be() ? lookup(ctx().hashes, id) : nullptr;
        if (!h) return SAI_STATUS_INVALID_OBJECT_ID;
        if (!a) return SAI_STATUS_INVALID_PARAMETER;
        if (a->id != SAI_HASH_ATTR_NATIVE_HASH_FIELD_LIST) return attr_not_supported(0);
        if (a->value.s32list.count && !a->value.s32list.list) return attr_invalid(0);
        std::vector<int32_t> f(a->value.s32list.list, a->value.s32list.list + a->value.s32list.count);
        bool programmed = false;
        sai_status_t st = apply_hash(h->lag, f, seed_of(h->lag), programmed);
        if (st != SAI_STATUS_SUCCESS) return attr_backend_status(st, 0);
        h->fields = f;
        return SAI_STATUS_SUCCESS;
    });
}

static sai_status_t get_hash_attribute(_In_ sai_object_id_t id, _In_ uint32_t n, _Inout_ sai_attribute_t* attrs)
{
    return guarded(SAI_API_HASH, __func__, [&]() -> sai_status_t {
        HashObj* h = hash_be() ? lookup(ctx().hashes, id) : nullptr;
        if (!h) return SAI_STATUS_INVALID_OBJECT_ID;
        if (n && !attrs) return SAI_STATUS_INVALID_PARAMETER;
        for (uint32_t i = 0; i < n; i++) {
            if (attrs[i].id != SAI_HASH_ATTR_NATIVE_HASH_FIELD_LIST) return attr_not_supported(i);
            sai_status_t st = fill_list(attrs[i].value.s32list, h->fields);
            if (st != SAI_STATUS_SUCCESS) return st;
        }
        return SAI_STATUS_SUCCESS;
    });
}

static sai_hash_api_t make_hash_api()
{
    sai_hash_api_t a;
    std::memset(&a, 0, sizeof(a));
    a.create_hash = create_hash;
    a.remove_hash = remove_hash;
    a.set_hash_attribute = set_hash_attribute;
    a.get_hash_attribute = get_hash_attribute;
    return a;
}
const sai_hash_api_t hash_api = make_hash_api();

// ------------------------------------------------------------ storm control
static bool storm_kind(sai_attr_id_t id, StormKind& k)
{
    switch (id) {
    case SAI_PORT_ATTR_BROADCAST_STORM_CONTROL_POLICER_ID: k = StormKind::Broadcast; return true;
    case SAI_PORT_ATTR_MULTICAST_STORM_CONTROL_POLICER_ID: k = StormKind::Multicast; return true;
    case SAI_PORT_ATTR_FLOOD_STORM_CONTROL_POLICER_ID: k = StormKind::UnknownUnicast; return true;
    default: return false;
    }
}

sai_status_t port_storm_attr_set(PortObj& p, const sai_attribute_t& a, uint32_t i, bool& handled)
{
    StormKind k;
    handled = storm_kind(a.id, k);
    if (!handled) return SAI_STATUS_SUCCESS;
    auto& c = ctx();
    PolicerObj* po = a.value.oid ? lookup(c.policers, a.value.oid) : nullptr;
    if (a.value.oid && !po) return attr_invalid(i);
    sai_status_t st = c.be->port_storm_set(p.hw, k, po ? po->pid : -1);
    if (st == SAI_STATUS_NOT_SUPPORTED) return attr_not_supported(i);   // as before the hook
    if (st != SAI_STATUS_SUCCESS) return attr_backend_status(st, i);
    p.storm[(int)k] = a.value.oid;
    return SAI_STATUS_SUCCESS;
}

sai_status_t port_storm_attr_get(PortObj& p, sai_attribute_t& a, bool& handled)
{
    StormKind k;
    handled = storm_kind(a.id, k);
    if (handled) a.value.oid = p.storm[(int)k];
    return SAI_STATUS_SUCCESS;
}

// --------------------------------------------------------- VLAN flood control
bool vlan_flood_attr(sai_attr_id_t id)
{
    return id == SAI_VLAN_ATTR_UNKNOWN_UNICAST_FLOOD_CONTROL_TYPE || id == SAI_VLAN_ATTR_UNKNOWN_MULTICAST_FLOOD_CONTROL_TYPE ||
           id == SAI_VLAN_ATTR_BROADCAST_FLOOD_CONTROL_TYPE;
}

sai_status_t vlan_flood_apply(uint16_t vid, const sai_attribute_t& a, uint32_t i, bool& keep)
{
    keep = true;
    FloodKind k = a.id == SAI_VLAN_ATTR_UNKNOWN_UNICAST_FLOOD_CONTROL_TYPE ? FloodKind::UnknownUnicast
                : a.id == SAI_VLAN_ATTR_UNKNOWN_MULTICAST_FLOOD_CONTROL_TYPE ? FloodKind::UnknownMulticast
                                                                              : FloodKind::Broadcast;
    if (a.value.s32 != SAI_VLAN_FLOOD_CONTROL_TYPE_ALL && a.value.s32 != SAI_VLAN_FLOOD_CONTROL_TYPE_NONE)
        return SAI_STATUS_SUCCESS;   // L2MC_GROUP / COMBINED: kept, not programmed (as before the hook)
    sai_status_t st = ctx().be->vlan_flood_set(vid, k, a.value.s32 == SAI_VLAN_FLOOD_CONTROL_TYPE_ALL);
    if (st == SAI_STATUS_NOT_SUPPORTED) return SAI_STATUS_SUCCESS;   // kept, as before the hook
    return st == SAI_STATUS_SUCCESS ? SAI_STATUS_SUCCESS : attr_backend_status(st, i);
}

} // namespace ob
