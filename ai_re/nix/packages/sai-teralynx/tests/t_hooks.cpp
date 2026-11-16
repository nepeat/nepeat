// SPDX-License-Identifier: Apache-2.0
// sai-teralynx test, the optional backend hooks of the integration phase
// (backend.h: RIF counters and admin state, the glean egress, port counters
// by SAI id, hash objects, VLAN flood control, storm control, ACL ranges /
// REDIRECT / SET_TC / SET_DSCP / MIRROR / egress tables + OUT_PORTS, entry
// admin state and in-place update, AVAILABLE_ACL_ENTRY) through the public
// SAI API against the mock libifcs (harness: tests/harness.h).
#include "harness.h"

static uint32_t (*m_route_trap)(const char*);
static void (*m_set_intf_stat)(uint32_t, int, uint64_t);
static int (*m_cmps)();
static int (*m_aces)();

static size_t count(const std::string& log, const std::string& needle)
{
    size_t n = 0;
    for (size_t p = log.find(needle); p != std::string::npos; p = log.find(needle, p + 1)) n++;
    return n;
}
static sai_attribute_t at(sai_attr_id_t id)
{
    sai_attribute_t a;
    std::memset(&a, 0, sizeof(a));
    a.id = id;
    return a;
}
static sai_attribute_t at_b(sai_attr_id_t id, bool v) { sai_attribute_t a = at(id); a.value.booldata = v; return a; }
static sai_attribute_t at_s32(sai_attr_id_t id, int32_t v) { sai_attribute_t a = at(id); a.value.s32 = v; return a; }
static sai_attribute_t at_u32(sai_attr_id_t id, uint32_t v) { sai_attribute_t a = at(id); a.value.u32 = v; return a; }
static sai_attribute_t at_oid(sai_attr_id_t id, sai_object_id_t v) { sai_attribute_t a = at(id); a.value.oid = v; return a; }
static sai_attribute_t act(sai_attr_id_t id) { sai_attribute_t a = at(id); a.value.aclaction.enable = true; return a; }
// the handle after "key" in the first log line that has it ("h=0x...")
static std::string handle_after(const std::string& log, const std::string& key)
{
    size_t p = log.find(key);
    if (p == std::string::npos) return "";
    p += key.size();
    size_t q = log.find_first_of(" \n", p);
    return log.substr(p, q - p);
}

static void hooks_l3(Env& e, sai_object_id_t vr, sai_object_id_t cpu)
{
    USE_ENV(e);
    // ---- port RIF on p3: L3 interface counters, admin state
    m_log_clear();
    sai_attribute_t ra_[3] = {at_oid(SAI_ROUTER_INTERFACE_ATTR_VIRTUAL_ROUTER_ID, vr),
                              at_s32(SAI_ROUTER_INTERFACE_ATTR_TYPE, SAI_ROUTER_INTERFACE_TYPE_PORT),
                              at_oid(SAI_ROUTER_INTERFACE_ATTR_PORT_ID, p3)};
    sai_object_id_t rif = 0;
    OK(ra->create_router_interface(&rif, s, 3, ra_));
    std::string ih = handle_after(m_log(), "ifcs_intf_create h=");
    CHECK(!ih.empty());
    uint32_t ihv = (uint32_t)std::strtoul(ih.c_str(), nullptr, 16);
    sai_stat_capability_t caps[64];
    sai_stat_capability_list_t cl{64, caps};
    OK(sai_query_stats_capability(s, SAI_OBJECT_TYPE_ROUTER_INTERFACE, &cl));
    CHECK_EQ(cl.count, 4u);
    m_set_intf_stat(ihv, 24, 70);   // IFCS L3INTF_INGRESS_IPV4_PKTS (ingress mode 4 = IPV4_IPV6_ANY)
    m_set_intf_stat(ihv, 26, 7);    // L3INTF_INGRESS_IPV6_PKTS
    m_set_intf_stat(ihv, 28, 5);    // L3INTF_EGRESS_IPV4_PKTS
    m_set_intf_stat(ihv, 30, 6);    // L3INTF_EGRESS_IPV6_PKTS
    sai_stat_id_t ids[3] = {SAI_ROUTER_INTERFACE_STAT_IN_PACKETS, SAI_ROUTER_INTERFACE_STAT_OUT_PACKETS,
                            SAI_ROUTER_INTERFACE_STAT_OUT_OCTETS};
    uint64_t v[3] = {0, 0, 0};
    OK(ra->get_router_interface_stats(rif, 3, ids, v));
    CHECK_EQ(v[0], 77u);
    CHECK_EQ(v[1], 11u);   // IPv4 + IPv6 egress
    sai_stat_id_t bad = SAI_ROUTER_INTERFACE_STAT_IN_ERROR_PACKETS;
    CHECK_EQ(ra->get_router_interface_stats(rif, 1, &bad, v), SAI_STATUS_NOT_SUPPORTED);
    m_log_clear();
    OK(ra->get_router_interface_stats_ext(rif, 1, ids, SAI_STATS_MODE_READ_AND_CLEAR, v));
    CHECK(has(m_log(), "ifcs_intf_stats_clear"));
    m_log_clear();
    OK(ra->clear_router_interface_stats(rif, 1, ids));
    CHECK(has(m_log(), "ifcs_intf_stats_clear"));
    m_log_clear();
    sai_attribute_t a = at_b(SAI_ROUTER_INTERFACE_ATTR_ADMIN_V4_STATE, false);
    OK(ra->set_router_interface_attribute(rif, &a));
    CHECK(has(m_log(), "ifcs_intf_attr_set h=0x" + ih.substr(2) + " count=2 id=18"));
    m_log_clear();
    OK(ra->set_router_interface_attribute(rif, &a));   // no change: no IFCS call
    CHECK(!has(m_log(), "ifcs_intf_attr_set"));
    a.value.booldata = true;
    OK(ra->set_router_interface_attribute(rif, &a));
    a = at(SAI_ROUTER_INTERFACE_ATTR_ADMIN_V4_STATE);
    OK(ra->get_router_interface_attribute(rif, 1, &a));
    CHECK(a.value.booldata);

    // ---- glean: a connected subnet copies to its own hostif trap, IP2ME to another
    sai_route_entry_t re;
    std::memset(&re, 0, sizeof(re));
    re.switch_id = s;
    re.vr_id = vr;
    re.destination.addr_family = SAI_IP_ADDR_FAMILY_IPV4;
    inet_pton(AF_INET, "10.77.0.0", &re.destination.addr.ip4);
    inet_pton(AF_INET, "255.255.255.0", &re.destination.mask.ip4);
    a = at_oid(SAI_ROUTE_ENTRY_ATTR_NEXT_HOP_ID, rif);
    OK(rta->create_route_entry(&re, 1, &a));
    sai_route_entry_t me = re;
    inet_pton(AF_INET, "10.77.0.1", &me.destination.addr.ip4);
    inet_pton(AF_INET, "255.255.255.255", &me.destination.mask.ip4);
    a = at_oid(SAI_ROUTE_ENTRY_ATTR_NEXT_HOP_ID, cpu);
    OK(rta->create_route_entry(&me, 1, &a));
    uint32_t glean = m_route_trap("26200001/4/0a4d0000/ffffff00"), ip2me = m_route_trap("26200001/4/0a4d0001/ffffffff");
    CHECK(glean != 0 && ip2me != 0 && glean != ip2me);
    OK(rta->remove_route_entry(&me));
    OK(rta->remove_route_entry(&re));
    OK(ra->remove_router_interface(rif));
}

static void hooks_hash_flood_storm(Env& e)
{
    USE_ENV(e);
    // ---- port counters by SAI id: the counters area's map (stats capability too)
    sai_stat_capability_t caps[512];
    sai_stat_capability_list_t cl{512, caps};
    OK(sai_query_stats_capability(s, SAI_OBJECT_TYPE_PORT, &cl));
    bool b64 = false, fec = false;
    for (uint32_t i = 0; i < cl.count; i++) {
        if (caps[i].stat_enum == SAI_PORT_STAT_ETHER_STATS_PKTS_64_OCTETS) b64 = true;
        if (caps[i].stat_enum == SAI_PORT_STAT_IF_IN_FEC_CORRECTABLE_FRAMES) fec = true;
    }
    CHECK(b64 && fec && cl.count > 50);
    sai_stat_id_t pid[2] = {SAI_PORT_STAT_ETHER_STATS_PKTS_64_OCTETS, SAI_PORT_STAT_IF_IN_OCTETS};
    uint64_t pv[2] = {0, 0};
    m_log_clear();
    OK(pa->get_port_stats(p1, 2, pid, pv));
    CHECK_EQ(count(m_log(), "ifcs_devport_stats_get dp=1"), 1u);   // one call for both

    // ---- hash: SWITCH ECMP_HASH / LAG_HASH objects, fields and seed
    sai_hash_api_t* hapi = nullptr;
    OK(sai_api_query(SAI_API_HASH, (void**)&hapi));
    sai_attribute_t a = at(SAI_SWITCH_ATTR_ECMP_HASH);
    OK(sw->get_switch_attribute(s, 1, &a));
    sai_object_id_t eh = a.value.oid;
    CHECK(eh != SAI_NULL_OBJECT_ID);
    int32_t f[2] = {SAI_NATIVE_HASH_FIELD_SRC_IP, SAI_NATIVE_HASH_FIELD_DST_IP};
    a = at(SAI_HASH_ATTR_NATIVE_HASH_FIELD_LIST);
    a.value.s32list.count = 2;
    a.value.s32list.list = f;
    m_log_clear();
    OK(hapi->set_hash_attribute(eh, &a));
    CHECK_EQ(count(m_log(), "ifcs_extended_field_list_create"), 4u);   // one per IP packet type (non-IP: no IP field)
    int32_t inner[1] = {SAI_NATIVE_HASH_FIELD_INNER_SRC_IP};
    a.value.s32list.count = 1;
    a.value.s32list.list = inner;
    CHECK(hapi->set_hash_attribute(eh, &a) != SAI_STATUS_SUCCESS);   // no IFCS element
    a = at_u32(SAI_SWITCH_ATTR_ECMP_DEFAULT_HASH_SEED, 7);
    m_log_clear();
    OK(sw->set_switch_attribute(s, &a));
    CHECK(has(m_log(), "u32=7"));

    // ---- VLAN flood control: all three kinds off = an empty flood group
    sai_attribute_t va_ = at_u32(SAI_VLAN_ATTR_VLAN_ID, 300);
    sai_object_id_t v300 = 0;
    OK(va->create_vlan(&v300, s, 1, &va_));
    m_log_clear();
    a = at_s32(SAI_VLAN_ATTR_UNKNOWN_UNICAST_FLOOD_CONTROL_TYPE, SAI_VLAN_FLOOD_CONTROL_TYPE_NONE);
    OK(va->set_vlan_attribute(v300, &a));
    CHECK(!has(m_log(), "ifcs_l2vni_attr_set"));                    // mixed: recorded only
    a.id = SAI_VLAN_ATTR_UNKNOWN_MULTICAST_FLOOD_CONTROL_TYPE;
    OK(va->set_vlan_attribute(v300, &a));
    a.id = SAI_VLAN_ATTR_BROADCAST_FLOOD_CONTROL_TYPE;
    OK(va->set_vlan_attribute(v300, &a));
    CHECK(has(m_log(), "ifcs_mdg_create") && has(m_log(), "ifcs_l2vni_attr_set vni=300 count=1"));
    m_log_clear();
    for (sai_attr_id_t id : {SAI_VLAN_ATTR_UNKNOWN_UNICAST_FLOOD_CONTROL_TYPE, SAI_VLAN_ATTR_UNKNOWN_MULTICAST_FLOOD_CONTROL_TYPE,
                             SAI_VLAN_ATTR_BROADCAST_FLOOD_CONTROL_TYPE}) {
        a = at_s32(id, SAI_VLAN_FLOOD_CONTROL_TYPE_ALL);
        OK(va->set_vlan_attribute(v300, &a));
    }
    CHECK_EQ(count(m_log(), "ifcs_l2vni_attr_set vni=300"), 1u);
    m_log_clear();
    OK(va->remove_vlan(v300));
    CHECK_EQ(count(m_log(), "ifcs_mdg_delete"), 2u);                  // members' and the empty one

    // ---- storm control: an ACL entry per port and kind, the policer's meter
    sai_attribute_t pa_[4] = {at_s32(SAI_POLICER_ATTR_METER_TYPE, SAI_METER_TYPE_PACKETS),
                              at_s32(SAI_POLICER_ATTR_MODE, SAI_POLICER_MODE_SR_TCM), at(SAI_POLICER_ATTR_CIR),
                              at(SAI_POLICER_ATTR_CBS)};
    pa_[2].value.u64 = 100;
    pa_[3].value.u64 = 100;
    sai_object_id_t pol = 0;
    OK(e.pola->create_policer(&pol, s, 4, pa_));
    int aces = m_aces();
    m_log_clear();
    a = at_oid(SAI_PORT_ATTR_BROADCAST_STORM_CONTROL_POLICER_ID, pol);
    OK(pa->set_port_attribute(p2, &a));
    std::string log = m_log();
    CHECK(has(log, "ifcs_acl_table_create") && has(log, "ifcs_acl_meter_create"));
    CHECK(has(log, "type=17 value=ffffffffffff object=0x0 mask=ffffffffffff"));
    CHECK_EQ(m_aces(), aces + 1);
    a = at_oid(SAI_PORT_ATTR_FLOOD_STORM_CONTROL_POLICER_ID, pol);
    m_log_clear();
    OK(pa->set_port_attribute(p2, &a));
    CHECK(has(m_log(), "type=82 value=00"));                            // DMAC missed the L2 table
    CHECK_EQ(m_aces(), aces + 2);
    a = at(SAI_PORT_ATTR_BROADCAST_STORM_CONTROL_POLICER_ID);
    OK(pa->get_port_attribute(p2, 1, &a));
    CHECK_EQ(a.value.oid, pol);
    for (sai_attr_id_t id : {SAI_PORT_ATTR_BROADCAST_STORM_CONTROL_POLICER_ID, SAI_PORT_ATTR_FLOOD_STORM_CONTROL_POLICER_ID}) {
        a = at_oid(id, SAI_NULL_OBJECT_ID);
        OK(pa->set_port_attribute(p2, &a));
    }
    CHECK_EQ(m_aces(), aces);
    OK(e.pola->remove_policer(pol));
}

static void hooks_acl(Env& e)
{
    USE_ENV(e);
    sai_acl_api_t* acl = e.acla;
    sai_mirror_api_t* ma = nullptr;
    OK(sai_api_query(SAI_API_MIRROR, (void**)&ma));
    // ---- an ingress table with ranges and the extra actions
    int32_t bind[1] = {SAI_ACL_BIND_POINT_TYPE_PORT};
    int32_t rtypes[1] = {SAI_ACL_RANGE_TYPE_L4_DST_PORT_RANGE};
    int32_t acts[6] = {SAI_ACL_ACTION_TYPE_PACKET_ACTION, SAI_ACL_ACTION_TYPE_COUNTER, SAI_ACL_ACTION_TYPE_REDIRECT,
                       SAI_ACL_ACTION_TYPE_SET_TC, SAI_ACL_ACTION_TYPE_MIRROR_INGRESS, SAI_ACL_ACTION_TYPE_SET_DSCP};
    std::vector<sai_attribute_t> t = {at_s32(SAI_ACL_TABLE_ATTR_ACL_STAGE, SAI_ACL_STAGE_INGRESS),
                                      at(SAI_ACL_TABLE_ATTR_ACL_BIND_POINT_TYPE_LIST), at(SAI_ACL_TABLE_ATTR_FIELD_ACL_RANGE_TYPE),
                                      at(SAI_ACL_TABLE_ATTR_ACL_ACTION_TYPE_LIST), at_b(SAI_ACL_TABLE_ATTR_FIELD_DST_IP, true),
                                      at_b(SAI_ACL_TABLE_ATTR_FIELD_IP_PROTOCOL, true)};
    t[1].value.s32list = {1, bind};
    t[2].value.s32list = {1, rtypes};
    t[3].value.s32list = {6, acts};
    sai_object_id_t tab = 0;
    CHECK(acl->create_acl_table(&tab, s, (uint32_t)t.size(), t.data()) != SAI_STATUS_SUCCESS);   // SET_DSCP: refused by 0.12.41
    t[3].value.s32list = {5, acts};
    m_log_clear();
    OK(acl->create_acl_table(&tab, s, (uint32_t)t.size(), t.data()));
    CHECK(has(m_log(), "ifcs_acl_action_profile_create") && has(m_log(), "types=8"));   // drop, copy, copy override, counter, meter + redirect, tc, mirror
    sai_attribute_t b = at_oid(SAI_PORT_ATTR_INGRESS_ACL, tab);
    OK(pa->set_port_attribute(p1, &b));
    sai_attribute_t rg[2] = {at_s32(SAI_ACL_RANGE_ATTR_TYPE, SAI_ACL_RANGE_TYPE_L4_DST_PORT_RANGE), at(SAI_ACL_RANGE_ATTR_LIMIT)};
    rg[1].value.u32range.min = 1000;
    rg[1].value.u32range.max = 2000;
    sai_object_id_t range = 0;
    OK(acl->create_acl_range(&range, s, 2, rg));
    sai_object_id_t k1 = 0;
    sai_attribute_t kc[3] = {at_oid(SAI_ACL_COUNTER_ATTR_TABLE_ID, tab), at_b(SAI_ACL_COUNTER_ATTR_ENABLE_PACKET_COUNT, true),
                             at_b(SAI_ACL_COUNTER_ATTR_ENABLE_BYTE_COUNT, true)};
    OK(acl->create_acl_counter(&k1, s, 3, kc));
    // entry 1: dst port 1000-2000 -> TC 5, counted
    std::vector<sai_attribute_t> e1 = {at_oid(SAI_ACL_ENTRY_ATTR_TABLE_ID, tab), at_u32(SAI_ACL_ENTRY_ATTR_PRIORITY, 10),
                                       at(SAI_ACL_ENTRY_ATTR_FIELD_ACL_RANGE_TYPE), act(SAI_ACL_ENTRY_ATTR_ACTION_SET_TC),
                                       act(SAI_ACL_ENTRY_ATTR_ACTION_COUNTER)};
    e1[2].value.aclfield.enable = true;
    e1[2].value.aclfield.data.objlist = {1, &range};
    e1[3].value.aclaction.parameter.u8 = 5;
    e1[4].value.aclaction.parameter.oid = k1;
    m_log_clear();
    int cmps = m_cmps();
    sai_object_id_t en1 = 0;
    OK(acl->create_acl_entry(&en1, s, (uint32_t)e1.size(), e1.data()));
    std::string log = m_log();
    CHECK(has(log, "ifcs_acl_comparator_create") && has(log, "type=0 op=7 value=1000 hi=2000 dir=0"));
    CHECK(has(log, "ifcs_acl_comparator_group_create") && has(log, "n=1"));
    CHECK(has(log, "type=67 value= object=0x"));
    CHECK(has(log, "type=set_tc green=1 tc=5 yellow=5 red=5 dirty=3f"));
    CHECK_EQ(m_cmps(), cmps + 1);
    sai_attribute_t ga = at(SAI_ACL_ENTRY_ATTR_FIELD_ACL_RANGE_TYPE);
    sai_object_id_t got[2] = {0, 0};
    ga.value.aclfield.data.objlist = {2, got};
    OK(acl->get_acl_entry_attribute(en1, 1, &ga));
    CHECK(ga.value.aclfield.data.objlist.count == 1 && got[0] == range);
    CHECK_EQ(acl->remove_acl_range(range), SAI_STATUS_OBJECT_IN_USE);
    // admin down / up without re-creation; priority change in place (counter kept)
    m_log_clear();
    sai_attribute_t ad = at_b(SAI_ACL_ENTRY_ATTR_ADMIN_STATE, false);
    OK(acl->set_acl_entry_attribute(en1, &ad));
    CHECK(has(m_log(), "ifcs_ace_attr_set") && has(m_log(), "id=3 data=0") && !has(m_log(), "ifcs_ace_delete"));
    ad.value.booldata = true;
    OK(acl->set_acl_entry_attribute(en1, &ad));
    CHECK(has(m_log(), "id=3 data=1"));
    m_log_clear();
    sai_attribute_t pr = at_u32(SAI_ACL_ENTRY_ATTR_PRIORITY, 20);
    OK(acl->set_acl_entry_attribute(en1, &pr));
    CHECK(has(m_log(), "ifcs_ace_delete") && has(m_log(), "ifcs_ace_create"));
    CHECK(!has(m_log(), "ifcs_acl_counter_delete") && !has(m_log(), "ifcs_acl_counter_create"));
    // entry 2: redirect to p2 (SET_DSCP refused); entry 3: mirror to a SPAN session on p2
    std::vector<sai_attribute_t> e2 = {at_oid(SAI_ACL_ENTRY_ATTR_TABLE_ID, tab), at_u32(SAI_ACL_ENTRY_ATTR_PRIORITY, 30),
                                       act(SAI_ACL_ENTRY_ATTR_ACTION_REDIRECT), act(SAI_ACL_ENTRY_ATTR_ACTION_SET_DSCP)};
    e2[2].value.aclaction.parameter.oid = p2;
    e2[3].value.aclaction.parameter.u8 = 46;
    sai_object_id_t en2 = 0;
    CHECK(acl->create_acl_entry(&en2, s, (uint32_t)e2.size(), e2.data()) != SAI_STATUS_SUCCESS);
    e2.pop_back();
    m_log_clear();
    OK(acl->create_acl_entry(&en2, s, (uint32_t)e2.size(), e2.data()));
    CHECK(has(m_log(), "type=10 object=0x3e200002"));                     // sysport of devport 2
    sai_attribute_t ms[2] = {at_s32(SAI_MIRROR_SESSION_ATTR_TYPE, SAI_MIRROR_SESSION_TYPE_LOCAL),
                             at_oid(SAI_MIRROR_SESSION_ATTR_MONITOR_PORT, p2)};
    sai_object_id_t sess = 0;
    OK(ma->create_mirror_session(&sess, s, 2, ms));
    std::vector<sai_attribute_t> e3 = {at_oid(SAI_ACL_ENTRY_ATTR_TABLE_ID, tab), at_u32(SAI_ACL_ENTRY_ATTR_PRIORITY, 40),
                                       act(SAI_ACL_ENTRY_ATTR_ACTION_MIRROR_INGRESS)};
    e3[2].value.aclaction.parameter.objlist = {1, &sess};
    m_log_clear();
    sai_object_id_t en3 = 0;
    OK(acl->create_acl_entry(&en3, s, (uint32_t)e3.size(), e3.data()));
    CHECK(has(m_log(), "ifcs_collector_set_create") && has(m_log(), "type=7 object=0x"));
    sai_attribute_t av = at(SAI_ACL_TABLE_ATTR_AVAILABLE_ACL_ENTRY);
    OK(acl->get_acl_table_attribute(tab, 1, &av));
    CHECK_EQ(av.value.u32, 253u);
    OK(acl->remove_acl_entry(en3));
    OK(ma->remove_mirror_session(sess));
    OK(acl->remove_acl_entry(en2));
    OK(acl->remove_acl_entry(en1));
    CHECK_EQ(m_cmps(), cmps);
    OK(acl->remove_acl_range(range));
    OK(acl->remove_acl_counter(k1));
    b.value.oid = SAI_NULL_OBJECT_ID;
    OK(pa->set_port_attribute(p1, &b));
    OK(acl->remove_acl_table(tab));

    // ---- an egress table with OUT_PORTS, bound through an egress group
    int32_t eacts[2] = {SAI_ACL_ACTION_TYPE_PACKET_ACTION, SAI_ACL_ACTION_TYPE_COUNTER};
    std::vector<sai_attribute_t> et = {at_s32(SAI_ACL_TABLE_ATTR_ACL_STAGE, SAI_ACL_STAGE_EGRESS),
                                       at(SAI_ACL_TABLE_ATTR_ACL_BIND_POINT_TYPE_LIST), at(SAI_ACL_TABLE_ATTR_ACL_ACTION_TYPE_LIST),
                                       at_b(SAI_ACL_TABLE_ATTR_FIELD_DST_IP, true), at_b(SAI_ACL_TABLE_ATTR_FIELD_OUT_PORTS, true)};
    et[1].value.s32list = {1, bind};
    et[2].value.s32list = {2, eacts};
    sai_object_id_t etab = 0;
    OK(acl->create_acl_table(&etab, s, (uint32_t)et.size(), et.data()));
    sai_attribute_t gr[3] = {at_s32(SAI_ACL_TABLE_GROUP_ATTR_ACL_STAGE, SAI_ACL_STAGE_EGRESS),
                             at(SAI_ACL_TABLE_GROUP_ATTR_ACL_BIND_POINT_TYPE_LIST),
                             at_s32(SAI_ACL_TABLE_GROUP_ATTR_TYPE, SAI_ACL_TABLE_GROUP_TYPE_PARALLEL)};
    gr[1].value.s32list = {1, bind};
    sai_object_id_t eg = 0, egm = 0;
    OK(acl->create_acl_table_group(&eg, s, 3, gr));
    sai_attribute_t gm[2] = {at_oid(SAI_ACL_TABLE_GROUP_MEMBER_ATTR_ACL_TABLE_GROUP_ID, eg),
                             at_oid(SAI_ACL_TABLE_GROUP_MEMBER_ATTR_ACL_TABLE_ID, etab)};
    OK(acl->create_acl_table_group_member(&egm, s, 2, gm));
    sai_attribute_t wrong = at_oid(SAI_PORT_ATTR_INGRESS_ACL, eg);
    CHECK(pa->set_port_attribute(p2, &wrong) != SAI_STATUS_SUCCESS);   // an egress group as ingress ACL
    sai_attribute_t eb = at_oid(SAI_PORT_ATTR_EGRESS_ACL, eg);
    OK(pa->set_port_attribute(p2, &eb));
    sai_attribute_t g2 = at(SAI_PORT_ATTR_EGRESS_ACL);
    OK(pa->get_port_attribute(p2, 1, &g2));
    CHECK_EQ(g2.value.oid, eg);
    std::vector<sai_attribute_t> ee = {at_oid(SAI_ACL_ENTRY_ATTR_TABLE_ID, etab), at_u32(SAI_ACL_ENTRY_ATTR_PRIORITY, 10),
                                       at(SAI_ACL_ENTRY_ATTR_FIELD_OUT_PORTS), act(SAI_ACL_ENTRY_ATTR_ACTION_PACKET_ACTION)};
    ee[2].value.aclfield.enable = true;
    ee[2].value.aclfield.data.objlist = {1, &p2};
    ee[3].value.aclaction.parameter.s32 = SAI_PACKET_ACTION_DROP;
    m_log_clear();
    sai_object_id_t een = 0;
    OK(acl->create_acl_entry(&een, s, (uint32_t)ee.size(), ee.data()));
    CHECK(has(m_log(), "ifcs_acl_port_add") && has(m_log(), "sysport=0x3e200002"));
    CHECK(has(m_log(), "type=drop green=1 yellow=1 red=1"));
    OK(acl->remove_acl_entry(een));
    eb.value.oid = SAI_NULL_OBJECT_ID;
    OK(pa->set_port_attribute(p2, &eb));
    OK(acl->remove_acl_table_group_member(egm));
    OK(acl->remove_acl_table_group(eg));
    OK(acl->remove_acl_table(etab));
}

void test_hooks(Env& e)
{
    USE_ENV(e);
    hook(g_mock, m_route_trap, "mock_route_trap");
    hook(g_mock, m_set_intf_stat, "mock_set_intf_stat");
    hook(g_mock, m_cmps, "mock_acl_comparators");
    hook(g_mock, m_aces, "mock_acl_aces");
    sai_attribute_t a[2];
    std::memset(a, 0, sizeof(a));
    a[0].id = SAI_SWITCH_ATTR_DEFAULT_VIRTUAL_ROUTER_ID;
    a[1].id = SAI_SWITCH_ATTR_CPU_PORT;
    OK(sw->get_switch_attribute(s, 2, a));
    hooks_l3(e, a[0].value.oid, a[1].value.oid);
    hooks_hash_flood_storm(e);
    hooks_acl(e);
}
