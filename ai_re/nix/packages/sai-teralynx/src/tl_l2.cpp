// SPDX-License-Identifier: Apache-2.0
// L2 (tl_l2.h).
#include "tl_l2.h"

namespace ob {
namespace tlb {

// L2 base, as SDK/cmds/knet.py builds it (without its per-port VLANs).
sai_status_t L2Area::base_init(const std::vector<uint32_t>& dps)
{
    Guard g(c_.mu);
    ifcs_attr_t none = attr_of(0);
    stp_ = IFCS_NULL_HANDLE;
    sai_status_t st = c_.call(c_.L.ifcs_stp_create(NODE, &stp_, 0, &none), "stp_create");
    if (st != SAI_STATUS_SUCCESS) return st;
    std::vector<ifcs_handle_t> sps;
    for (uint32_t dp : dps) sps.push_back(c_.sysport[(int)dp]);
    ifcs_attr_t fwd = attr_u32(IFCS_STP_PORT_ATTR_STP_STATE, IFCS_STP_PORT_STATE_FORWARDING);
    st = c_.call(c_.L.ifcs_stp_port_add(NODE, stp_, (uint32_t)sps.size(), sps.data(), 1, &fwd), "stp_port_add");
    if (st != SAI_STATUS_SUCCESS) return st;
    mcdest_.clear();
    for (uint32_t dp : dps) {
        ifcs_attr_t a = attr_handle(IFCS_MC_DEST_ATTR_LOCAL_DESTINATION, c_.sysport[(int)dp]);
        ifcs_handle_t h = IFCS_NULL_HANDLE;
        st = c_.call(c_.L.ifcs_mc_dest_create(NODE, &h, 1, &a), "mc_dest_create");
        if (st != SAI_STATUS_SUCCESS) return st;
        mcdest_[(int)dp] = h;
    }
    vlans_.clear();
    return vlan_make(1);
}

// A VLAN is the knet pattern (SDK/cmds/knet.py): an L2VNI with handle
// IFCS_HANDLE_L2VNI(vid), the shared STP instance, and its own flood group
// (MDG) of per-port mc_dests. The CPU sysport is a member of every L2VNI and
// its mc_dest is in every flood group (floods reach the netdevs), never
// reported as a member. Untagged egress is per sysport
// (EGRESS_UNTAGGED_VLAN_ID[_ENABLE]): one untagged VLAN per port.
sai_status_t L2Area::vlan_create(uint16_t vid)
{
    Guard g(c_.mu);
    if (vid < 1 || vid > 4094) return SAI_STATUS_INVALID_PARAMETER;
    if (vlans_.count(vid)) return SAI_STATUS_ITEM_ALREADY_EXISTS;
    return vlan_make(vid);
}

sai_status_t L2Area::vlan_destroy(uint16_t vid)
{
    Guard g(c_.mu);
    auto it = vlans_.find(vid);
    if (it == vlans_.end()) return SAI_STATUS_ITEM_NOT_FOUND;
    if (vid == 1) return SAI_STATUS_OBJECT_IN_USE;
    for (auto m : std::vector<std::pair<int, bool>>(it->second.members.begin(), it->second.members.end()))
        vlan_port_remove(vid, m.first);
    Vlan& v = it->second;
    ifcs_handle_t cpu_sp = c_.sysport.at(CPU_DEVPORT), cpu_mcd = mcdest_.at(CPU_DEVPORT);
    c_.call(c_.L.ifcs_mdg_mc_dest_remove(NODE, v.mdg, 1, &cpu_mcd), "mdg_mc_dest_remove(cpu)");
    c_.call(c_.L.ifcs_l2vni_member_remove(NODE, v.l2vni, 1, &cpu_sp), "l2vni_member_remove(cpu)");
    sai_status_t st = c_.call(c_.L.ifcs_l2vni_delete(NODE, v.l2vni), "l2vni_delete");
    if (st != SAI_STATUS_SUCCESS) return st;
    c_.call(c_.L.ifcs_mdg_delete(NODE, v.mdg), "mdg_delete");
    vlans_.erase(it);
    return SAI_STATUS_SUCCESS;
}

sai_status_t L2Area::vlan_port_add(uint16_t vid, int hw, bool untagged)
{
    Guard g(c_.mu);
    auto it = vlans_.find(vid);
    if (it == vlans_.end()) return SAI_STATUS_ITEM_NOT_FOUND;
    if (is_lag_dest(hw)) return c_.not_impl("LAG as VLAN member");
    if (!c_.front(hw)) return SAI_STATUS_INVALID_PORT_NUMBER;
    if (untagged) {
        auto u = untagged_vid_.find(hw);
        if (u != untagged_vid_.end() && u->second != vid) {
            OB_ERR(SAI_API_VLAN, "devport %d is already untagged in VLAN %u; IFCS has one untagged VLAN per port",
                   hw, u->second);
            return SAI_STATUS_NOT_SUPPORTED;
        }
    }
    Vlan& v = it->second;
    if (!v.members.count(hw)) {
        ifcs_handle_t sp = c_.sysport.at(hw), mcd = mcdest_.at(hw);
        ifcs_attr_t none = attr_of(0);
        sai_status_t st = c_.call(c_.L.ifcs_l2vni_member_add(NODE, v.l2vni, 1, &sp, 0, &none), "l2vni_member_add");
        if (st != SAI_STATUS_SUCCESS) return st;
        st = c_.call(c_.L.ifcs_mdg_mc_dest_add(NODE, v.mdg, 1, &mcd, 0, &none), "mdg_mc_dest_add");
        if (st != SAI_STATUS_SUCCESS) {
            c_.L.ifcs_l2vni_member_remove(NODE, v.l2vni, 1, &sp);
            return st;
        }
    }
    if (untagged) {
        ifcs_attr_t a[2] = {attr_u16(IFCS_SYSPORT_ATTR_EGRESS_UNTAGGED_VLAN_ID, vid),
                            attr_data(IFCS_SYSPORT_ATTR_EGRESS_UNTAGGED_VLAN_ID_ENABLE, true)};
        sai_status_t st = c_.call(c_.L.ifcs_sysport_attr_set(NODE, c_.sysport.at(hw), 2, a), "sysport egress untagged");
        if (st != SAI_STATUS_SUCCESS) return st;
        untagged_vid_[hw] = vid;
    } else if (untagged_vid_.count(hw) && untagged_vid_[hw] == vid) {
        untagged_off(hw);
    }
    v.members[hw] = untagged;
    return SAI_STATUS_SUCCESS;
}

sai_status_t L2Area::vlan_port_remove(uint16_t vid, int hw)
{
    Guard g(c_.mu);
    auto it = vlans_.find(vid);
    if (it == vlans_.end()) return SAI_STATUS_ITEM_NOT_FOUND;
    Vlan& v = it->second;
    if (!v.members.count(hw)) return SAI_STATUS_SUCCESS;
    ifcs_handle_t sp = c_.sysport.at(hw), mcd = mcdest_.at(hw);
    c_.call(c_.L.ifcs_mdg_mc_dest_remove(NODE, v.mdg, 1, &mcd), "mdg_mc_dest_remove");
    sai_status_t st = c_.call(c_.L.ifcs_l2vni_member_remove(NODE, v.l2vni, 1, &sp), "l2vni_member_remove");
    if (untagged_vid_.count(hw) && untagged_vid_[hw] == vid) untagged_off(hw);
    v.members.erase(hw);
    return st;
}

sai_status_t L2Area::vlan_members(uint16_t vid, std::vector<std::pair<int, bool>>& out)
{
    Guard g(c_.mu);
    auto it = vlans_.find(vid);
    if (it == vlans_.end()) return SAI_STATUS_ITEM_NOT_FOUND;
    out.assign(it->second.members.begin(), it->second.members.end());
    return SAI_STATUS_SUCCESS;
}

bool L2Area::vlan_l2vni(uint16_t vid, ifcs_handle_t& l2vni) const
{
    auto it = vlans_.find(vid);
    if (it == vlans_.end()) return false;
    l2vni = it->second.l2vni;
    return true;
}

// ifcs_l2_entry_create with a MAC_L2VNI key built through the key
// accessors, attrs ENTRY_DEST + ENTRY_TYPE (SDK/scripts/test_l2.py).
// discard: ENTRY_FWD_POLICY = DROP (I).
sai_status_t L2Area::l2_add(const uint8_t mac[6], uint16_t vid, int hw, bool is_static, bool discard)
{
    Guard g(c_.mu);
    if (!vlans_.count(vid)) return SAI_STATUS_INVALID_PARAMETER;
    if (is_lag_dest(hw)) return c_.not_impl("FDB entry on a LAG");
    auto sp = c_.sysport.find(hw);
    if (sp == c_.sysport.end()) return SAI_STATUS_INVALID_PORT_NUMBER;
    ifcs_l2_entry_key_t k;
    l2_key(k, mac, vid);
    ifcs_attr_t a[3] = {attr_handle(IFCS_L2_ENTRY_ATTR_ENTRY_DEST, sp->second),
                        attr_u32(IFCS_L2_ENTRY_ATTR_ENTRY_TYPE,
                                 is_static ? IFCS_L2_ENTRY_TYPE_STATIC : IFCS_L2_ENTRY_TYPE_DYNAMIC),
                        attr_fwd(IFCS_L2_ENTRY_ATTR_ENTRY_FWD_POLICY, IFCS_FWD_ACTION_DROP)};
    return c_.call(c_.L.ifcs_l2_entry_create(NODE, &k, discard ? 3 : 2, a), "l2_entry_create");
}

sai_status_t L2Area::l2_delete(const uint8_t mac[6], uint16_t vid)
{
    Guard g(c_.mu);
    ifcs_l2_entry_key_t k;
    l2_key(k, mac, vid);
    return c_.call(c_.L.ifcs_l2_entry_delete(NODE, &k), "l2_entry_delete");
}

// Walk the table (ifcs_l2_entry_get_all, as SDK/ifcs_cmds/l2_entry.py),
// read each entry's type and destination, delete the matching ones one by
// one. No bulk delete-by-filter: we only remove what we checked.
sai_status_t L2Area::l2_flush(int hw, int vid, bool statics, bool dynamics)
{
    Guard g(c_.mu);
    if (hw >= 0 && is_lag_dest(hw)) return c_.not_impl("FDB flush on a LAG");
    std::vector<ifcs_l2_entry_key_t> keys;
    uint32_t n = 0;
    sai_status_t st = c_.call(c_.L.ifcs_l2_entry_get_all(NODE, nullptr, 0, nullptr, &L2Area::on_l2_entry, &keys, &n),
                              "l2_entry_get_all");
    if (st != SAI_STATUS_SUCCESS) return st;
    ifcs_handle_t want_sp = 0;
    if (hw >= 0) {
        auto sp = c_.sysport.find(hw);
        if (sp == c_.sysport.end()) return SAI_STATUS_INVALID_PORT_NUMBER;
        want_sp = sp->second;
    }
    for (auto& k : keys) {
        if (k.key_type != IFCS_L2_ENTRY_KEY_TYPE_MAC_L2VNI) continue;
        if (vid >= 0 && k.key.mac_l2vni.l2vni != IFCS_HANDLE_L2VNI((uint32_t)vid)) continue;
        ifcs_attr_t a[2] = {attr_of(IFCS_L2_ENTRY_ATTR_ENTRY_TYPE), attr_of(IFCS_L2_ENTRY_ATTR_ENTRY_DEST)};
        uint32_t got = 0;
        if (IFCS_STATUS_REASON(c_.L.ifcs_l2_entry_attr_get(NODE, &k, 2, a, &got)) != IFCS_SUCCESS) continue;
        bool is_static = a[0].value.u32 == IFCS_L2_ENTRY_TYPE_STATIC;
        if (is_static ? !statics : !dynamics) continue;
        if (hw >= 0 && a[1].value.handle != want_sp) continue;
        c_.call(c_.L.ifcs_l2_entry_delete(NODE, &k), "l2_entry_delete (flush)");
    }
    return SAI_STATUS_SUCCESS;
}

// IFCS_NODE_ATTR_L2_ENTRY_AGING_INTERVAL (u32, read back as u32 by the
// vendor's utils/ifcs_utils.py). Units taken as seconds (I).
sai_status_t L2Area::l2_age_set(uint32_t seconds)
{
    Guard g(c_.mu);
    ifcs_attr_t a = attr_u32(IFCS_NODE_ATTR_L2_ENTRY_AGING_INTERVAL, seconds);
    return c_.call(c_.L.ifcs_node_attr_set(NODE, 1, &a), "node l2_entry_aging_interval");
}

sai_status_t L2Area::vlan_make(uint16_t vid)
{
    ifcs_attr_t none = attr_of(0);
    Vlan v;
    v.mdg = IFCS_NULL_HANDLE;
    sai_status_t st = c_.call(c_.L.ifcs_mdg_create(NODE, &v.mdg, 0, &none), "mdg_create");
    if (st != SAI_STATUS_SUCCESS) return st;
    v.l2vni = IFCS_HANDLE_L2VNI(vid);
    ifcs_attr_t a[2] = {attr_handle(IFCS_L2VNI_ATTR_STP_INSTANCE, stp_), attr_handle(IFCS_L2VNI_ATTR_FLOOD_GROUP, v.mdg)};
    ifcs_handle_t h = v.l2vni;
    int r = IFCS_STATUS_REASON(c_.L.ifcs_l2vni_create(NODE, &h, 2, a));
    if (r == IFCS_EXIST) {
        // An L2VNI the SDK made itself (VLAN 1?): take it over.
        OB_NOTICE(SAI_API_VLAN, "L2VNI %u exists already; setting its STP instance and flood group", vid);
        r = IFCS_STATUS_REASON(c_.L.ifcs_l2vni_attr_set(NODE, v.l2vni, 2, a));
    }
    if (r != IFCS_SUCCESS) {
        c_.L.ifcs_mdg_delete(NODE, v.mdg);
        return c_.fail(r, "l2vni_create");
    }
    ifcs_handle_t cpu_sp = c_.sysport.at(CPU_DEVPORT), cpu_mcd = mcdest_.at(CPU_DEVPORT);
    r = IFCS_STATUS_REASON(c_.L.ifcs_l2vni_member_add(NODE, v.l2vni, 1, &cpu_sp, 0, &none));
    if (r != IFCS_SUCCESS && r != IFCS_EXIST) return c_.fail(r, "l2vni_member_add(cpu)");
    st = c_.call(c_.L.ifcs_mdg_mc_dest_add(NODE, v.mdg, 1, &cpu_mcd, 0, &none), "mdg_mc_dest_add(cpu)");
    if (st != SAI_STATUS_SUCCESS) return st;
    vlans_[vid] = v;
    return SAI_STATUS_SUCCESS;
}

void L2Area::untagged_off(int hw)
{
    ifcs_attr_t a = attr_data(IFCS_SYSPORT_ATTR_EGRESS_UNTAGGED_VLAN_ID_ENABLE, false);
    c_.call(c_.L.ifcs_sysport_attr_set(NODE, c_.sysport.at(hw), 1, &a), "sysport egress untagged off");
    untagged_vid_.erase(hw);
}

// The key accessors in the vendor's order (scripts/test_l2.py): init,
// key type, select the mac_l2vni member, then its MAC and L2VNI.
void L2Area::l2_key(ifcs_l2_entry_key_t& k, const uint8_t mac[6], uint16_t vid)
{
    std::memset(&k, 0, sizeof(k));
    c_.L.ifcs_l2_entry_key_t_init(&k);
    c_.L.ifcs_l2_entry_key_t_key_type_set(&k, IFCS_L2_ENTRY_KEY_TYPE_MAC_L2VNI);
    c_.L.ifcs_l2_entry_key_t_mac_l2vni_set(&k, k.key.mac_l2vni);
    ifcs_mac_t m;
    std::memcpy(m, mac, 6);
    c_.L.ifcs_l2_entry_key_mac_l2vni_t_mac_addr_set(&k.key.mac_l2vni, &m);
    c_.L.ifcs_l2_entry_key_mac_l2vni_t_l2vni_set(&k.key.mac_l2vni, IFCS_HANDLE_L2VNI(vid));
}

// Only translate and queue (the frontend's callback pushes to its queue).
void L2Area::on_l2(ifcs_node_id_t, void* user, ifcs_l2_event_info_t* info)
{
    auto* c = static_cast<Core*>(user);
    if (!c || !info || !c->fdb) return;
    if (info->l2_entry.key_type != IFCS_L2_ENTRY_KEY_TYPE_MAC_L2VNI) return;
    if (info->event_type == IFCS_L2_EVENT_TYPE_FLUSHED) return;   // our own flushes; the frontend knows
    FdbEvent e;
    e.learned = info->event_type != IFCS_L2_EVENT_TYPE_AGED;
    std::memcpy(e.mac, info->l2_entry.key.mac_l2vni.mac_addr, 6);
    ifcs_handle_t vni = info->l2_entry.key.mac_l2vni.l2vni;
    if ((vni >> IFCS_HANDLE_MODULE_SHIFT) != IFCS_MOD_L2VNI) return;
    e.vid = (uint16_t)IFCS_HANDLE_VALUE(vni);
    e.hw_port = -1;
    for (uint32_t i = 0; info->attr_p && i < info->attr_count; i++)
        if (info->attr_p[i].id == IFCS_L2_ENTRY_ATTR_ENTRY_DEST) {
            auto it = c->dev_of_sysport.find(info->attr_p[i].value.handle);
            if (it != c->dev_of_sysport.end()) e.hw_port = it->second;
        }
    if (e.learned && e.hw_port < 0) return;
    c->fdb(e);
}

void L2Area::on_l2_entry(ifcs_node_id_t, ifcs_l2_entry_key_t* k, uint32_t, ifcs_attr_t*, void* user)
{
    if (k && user) static_cast<std::vector<ifcs_l2_entry_key_t>*>(user)->push_back(*k);
}

} // namespace tlb
} // namespace ob
