// SPDX-License-Identifier: Apache-2.0
// sai-teralynx test, sFlow samples to psample (tl_sflow.cpp) on the mock
// libifcs + the mock ipd ioctls (tests/mock/mock_sflow.c): a GENETLINK host
// interface alone reaches no driver; the SAMPLEPACKET table entry creates
// the driver's sFlow netlink (sample GRE protocol, then the netlink, then
// every sampled port's rate); sampler changes push the rate; refusals
// (other trap types, a second sFlow netlink, failing ioctls); removal
// deletes the netlink; the SAMPLEPACKET trap: system trap MIRROR_TO_CPU (isai)
// and its group's policer as the meter of the samples' CPU queue, following
// policer, group, queue and action changes (harness: tests/harness.h).
#include "harness.h"

static int (*m_ipd_calls)();
static void (*m_ipd_fail)(unsigned, int);
static void (*m_egress)(int);

// The SAMPLEPACKET trap (t_copp.cpp keeps one): ours if creating works, else
// found among the hostif trap ids below a fresh one.
static sai_object_id_t samplepacket_trap(Env& e, sai_object_id_t grp, bool& ours)
{
    sai_attribute_t t[3];
    t[0].id = SAI_HOSTIF_TRAP_ATTR_TRAP_TYPE;
    t[0].value.s32 = SAI_HOSTIF_TRAP_TYPE_SAMPLEPACKET;
    t[1].id = SAI_HOSTIF_TRAP_ATTR_PACKET_ACTION;
    t[1].value.s32 = SAI_PACKET_ACTION_TRAP;
    t[2].id = SAI_HOSTIF_TRAP_ATTR_TRAP_GROUP;
    t[2].value.oid = grp;
    sai_object_id_t id = 0;
    sai_status_t st = e.ha->create_hostif_trap(&id, e.s, 3, t);
    ours = st == SAI_STATUS_SUCCESS;
    if (ours) return id;
    CHECK_EQ(st, SAI_STATUS_ITEM_ALREADY_EXISTS);
    t[0].value.s32 = SAI_HOSTIF_TRAP_TYPE_PIM;   // unmapped: kept, not programmed
    sai_object_id_t probe = 0;
    OK(e.ha->create_hostif_trap(&probe, e.s, 3, t));
    sai_object_id_t found = 0;
    for (sai_object_id_t o = probe - 1; o + 4096 > probe && (o & 0xffffffffffffull); o--) {
        sai_attribute_t a;
        a.id = SAI_HOSTIF_TRAP_ATTR_TRAP_TYPE;
        if (e.ha->get_hostif_trap_attribute(o, 1, &a) == SAI_STATUS_SUCCESS && a.value.s32 == SAI_HOSTIF_TRAP_TYPE_SAMPLEPACKET) {
            found = o;
            break;
        }
    }
    OK(e.ha->remove_hostif_trap(probe));
    CHECK(found != 0);
    return found;
}

static sai_status_t genl_hostif(Env& e, const char* name, const char* grp, sai_object_id_t& id)
{
    sai_attribute_t h[3];
    h[0].id = SAI_HOSTIF_ATTR_TYPE;
    h[0].value.s32 = SAI_HOSTIF_TYPE_GENETLINK;
    h[1].id = SAI_HOSTIF_ATTR_NAME;
    std::snprintf(h[1].value.chardata, sizeof(h[1].value.chardata), "%s", name);
    h[2].id = SAI_HOSTIF_ATTR_GENETLINK_MCGRP_NAME;
    std::snprintf(h[2].value.chardata, sizeof(h[2].value.chardata), "%s", grp);
    return e.ha->create_hostif(&id, e.s, grp ? 3 : 2, h);
}

static sai_status_t table_entry(Env& e, sai_object_id_t trap, sai_object_id_t hif, sai_object_id_t& id)
{
    sai_attribute_t t[4];
    t[0].id = SAI_HOSTIF_TABLE_ENTRY_ATTR_TYPE;
    t[0].value.s32 = SAI_HOSTIF_TABLE_ENTRY_TYPE_TRAP_ID;
    t[1].id = SAI_HOSTIF_TABLE_ENTRY_ATTR_TRAP_ID;
    t[1].value.oid = trap;
    t[2].id = SAI_HOSTIF_TABLE_ENTRY_ATTR_CHANNEL_TYPE;
    t[2].value.s32 = SAI_HOSTIF_TABLE_ENTRY_CHANNEL_TYPE_GENETLINK;
    t[3].id = SAI_HOSTIF_TABLE_ENTRY_ATTR_HOST_IF;
    t[3].value.oid = hif;
    return e.ha->create_hostif_table_entry(&id, e.s, 4, t);
}

static unsigned count(const std::string& s, const std::string& n)
{
    unsigned c = 0;
    for (size_t p = s.find(n); p != std::string::npos; p = s.find(n, p + 1)) c++;
    return c;
}

void test_sflow(Env& e)
{
    USE_ENV(e);
    hook(g_mock, m_ipd_calls, "mock_ipd_calls");
    hook(g_mock, m_ipd_fail, "mock_ipd_fail");
    hook(g_mock, m_egress, "mock_sflow_egress");
    sai_samplepacket_api_t* spa;
    OK(sai_api_query(SAI_API_SAMPLEPACKET, (void**)&spa));
    sai_attribute_t a;

    // ---- the genetlink host interface alone: recorded, no driver call
    sai_object_id_t gh = 0, gx = 0;
    CHECK_EQ(genl_hostif(e, "psample", nullptr, gx), SAI_STATUS_MANDATORY_ATTRIBUTE_MISSING);   // no group (frontend)
    int calls0 = m_ipd_calls();
    OK(genl_hostif(e, "psample", "packets", gh));
    CHECK_EQ(m_ipd_calls(), calls0);
    CHECK_EQ(genl_hostif(e, "psample", "packets", gx), SAI_STATUS_ITEM_ALREADY_EXISTS);
    a.id = SAI_HOSTIF_ATTR_GENETLINK_MCGRP_NAME;
    OK(ha->get_hostif_attribute(gh, 1, &a));
    CHECK(std::string(a.value.chardata) == "packets");

    // ---- trap group (queue 2, as SONiC's sflow group) + the SAMPLEPACKET trap
    sai_attribute_t g;
    g.id = SAI_HOSTIF_TRAP_GROUP_ATTR_QUEUE;
    g.value.u32 = 2;
    sai_object_id_t grp = 0;
    OK(ha->create_hostif_trap_group(&grp, s, 1, &g));
    bool own_trap = false;
    sai_object_id_t trap = samplepacket_trap(e, grp, own_trap);

    // ---- a sampler on p1 before the table entry: policy only, no rate push
    sai_attribute_t sp;
    sp.id = SAI_SAMPLEPACKET_ATTR_SAMPLE_RATE;
    sp.value.u32 = 1000;
    sai_object_id_t smp = 0;
    OK(spa->create_samplepacket(&smp, s, 1, &sp));
    m_log_clear();
    a.id = SAI_PORT_ATTR_INGRESS_SAMPLEPACKET_ENABLE;
    a.value.oid = smp;
    OK(pa->set_port_attribute(p1, &a));
    CHECK(has(m_log(), "ifcs_sysport_attr_set sp=0x3e200001 id=25 u32=0"));
    CHECK(!has(m_log(), "ipd_ioctl"));

    // ---- the table entry: GRE protocol, the netlink, p1's rate, in that order
    m_log_clear();
    sai_object_id_t te = 0;
    OK(table_entry(e, trap, gh, te));
    std::string log = m_log();
    size_t a1 = log.find("ipd_ioctl SET_PARAMS shim_gre_proto=0x8818");
    size_t a2 = log.find("ipd_ioctl CREATE_NETLINK type=16 app=1 name=psample mcgrp=packets");
    size_t a3 = log.find("ipd_ioctl SET_NETDEV id=8 sysport=1 rate=1000");
    CHECK(a1 != std::string::npos && a2 != std::string::npos && a3 != std::string::npos);
    CHECK(a1 < a2 && a2 < a3);
    CHECK_EQ(count(log, "ipd_ioctl"), 3u);

    // ---- sampler changes while active: the rate follows (update, second port, off)
    m_log_clear();
    sp.value.u32 = 500;
    OK(spa->set_samplepacket_attribute(smp, &sp));
    CHECK(has(m_log(), "ipd_ioctl SET_NETDEV id=8 sysport=1 rate=500"));
    m_log_clear();
    OK(pa->set_port_attribute(p2, &a));
    CHECK(has(m_log(), "ipd_ioctl SET_NETDEV id=8 sysport=2 rate=500"));
    m_log_clear();
    sp.value.u32 = 4096;
    OK(spa->set_samplepacket_attribute(smp, &sp));
    CHECK_EQ(count(m_log(), "ipd_ioctl SET_NETDEV id=8"), 2u);
    CHECK(has(m_log(), "sysport=1 rate=4096") && has(m_log(), "sysport=2 rate=4096"));
    m_log_clear();
    a.value.oid = 0;
    OK(pa->set_port_attribute(p1, &a));
    CHECK(has(m_log(), "ifcs_sysport_attr_set sp=0x3e200001 id=25 u32=1") && has(m_log(), "ipd_ioctl SET_NETDEV id=8 sysport=1 rate=0"));
    // a driver without the rate ioctl (unpatched, legacy mode): sampling still works
    m_ipd_fail(26, EPERM);
    m_log_clear();
    a.value.oid = smp;
    OK(pa->set_port_attribute(p1, &a));
    CHECK(has(m_log(), "id=25 u32=0") && has(m_log(), "SET_NETDEV id=8 sysport=1 rate=4096"));
    // egress sampling stays refused (no rate pushed for it)
    m_log_clear();
    a.id = SAI_PORT_ATTR_EGRESS_SAMPLEPACKET_ENABLE;
    CHECK_EQ(pa->set_port_attribute(p1, &a), SAI_STATUS_ATTR_NOT_SUPPORTED_0);
    CHECK(!has(m_log(), "ipd_ioctl"));
    // with node EGRESS_SFLOW_ENABLE (SAI_TERALYNX_EGRESS_SFLOW=on at init): an egress collector,
    // SFLOW_EGRESS (26) on the sysport, the egress rate (id 9) to the driver; off again
    m_egress(1);
    m_log_clear();
    OK(pa->set_port_attribute(p1, &a));
    log = m_log();
    CHECK(has(log, "ifcs_collector_create") && has(log, "ifcs_sysport_attr_set sp=0x3e200001 id=26 u32=0"));
    CHECK(has(log, "ipd_ioctl SET_NETDEV id=9 sysport=1 rate=4096"));
    m_log_clear();
    a.value.oid = 0;
    OK(pa->set_port_attribute(p1, &a));
    CHECK(has(m_log(), "id=26 u32=1") && has(m_log(), "SET_NETDEV id=9 sysport=1 rate=0") && has(m_log(), "ifcs_collector_delete"));
    m_egress(0);
    a.value.oid = smp;

    // ---- refusals: another trap type to a genetlink hostif; a second sFlow netlink
    sai_object_id_t ttl = 0, te2 = 0;
    {
        sai_attribute_t t[3];
        t[0].id = SAI_HOSTIF_TRAP_ATTR_TRAP_TYPE;
        t[0].value.s32 = SAI_HOSTIF_TRAP_TYPE_OSPF;   // unmapped: kept
        t[1].id = SAI_HOSTIF_TRAP_ATTR_PACKET_ACTION;
        t[1].value.s32 = SAI_PACKET_ACTION_TRAP;
        t[2].id = SAI_HOSTIF_TRAP_ATTR_TRAP_GROUP;
        t[2].value.oid = grp;
        OK(ha->create_hostif_trap(&ttl, s, 3, t));
    }
    CHECK_EQ(table_entry(e, ttl, gh, te2), SAI_STATUS_NOT_SUPPORTED);
    sai_object_id_t gh2 = 0;
    OK(genl_hostif(e, "psample2", "packets", gh2));
    m_log_clear();
    CHECK_EQ(table_entry(e, trap, gh2, te2), SAI_STATUS_ITEM_ALREADY_EXISTS);
    CHECK(!has(m_log(), "ipd_ioctl"));
    CHECK_EQ(ha->remove_hostif(gh), SAI_STATUS_OBJECT_IN_USE);   // the table entry uses it (frontend)

    // ---- removing the entry deletes the netlink; rates are no longer pushed
    m_log_clear();
    OK(ha->remove_hostif_table_entry(te));
    CHECK(has(m_log(), "ipd_ioctl DELETE_NETLINK type=16 app=1 name=psample mcgrp=packets"));
    m_log_clear();
    sp.value.u32 = 2000;
    OK(spa->set_samplepacket_attribute(smp, &sp));
    CHECK(!has(m_log(), "ipd_ioctl"));

    // ---- the second hostif can take over; a failing CREATE_NETLINK leaves nothing active
    m_ipd_fail(27, EEXIST);
    m_log_clear();
    CHECK(table_entry(e, trap, gh2, te2) != SAI_STATUS_SUCCESS);
    CHECK(has(m_log(), "CREATE_NETLINK") && !has(m_log(), "SET_NETDEV"));
    m_log_clear();
    OK(table_entry(e, trap, gh2, te2));
    log = m_log();
    CHECK(has(log, "CREATE_NETLINK type=16 app=1 name=psample2") && has(log, "SET_NETDEV id=8 sysport=1 rate=2000") &&
          has(log, "SET_NETDEV id=8 sysport=2 rate=2000"));
    // removing the hostif with its entry gone deletes the netlink (once)
    OK(ha->remove_hostif_table_entry(te2));
    m_log_clear();
    OK(ha->remove_hostif(gh2));
    CHECK(!has(m_log(), "ipd_ioctl"));
    OK(ha->remove_hostif(gh));

    // ---- sample policing: SONiC's sflow group (queue 2, 1000 pps sr_tcm, red drop) on the trap.
    //      The samples' queue is 2 (test profile SAI_TERALYNX_SFLOW_CPU_QUEUE; the mock has no node
    //      TRAFFIC_MONITOR_CPU_QUEUE): CPU queue 2's token-bucket meter, 1000 x 84 B x 8 = 672 kbit/s, 84000 B
    sai_attribute_t pp[5];
    pp[0].id = SAI_POLICER_ATTR_METER_TYPE; pp[0].value.s32 = SAI_METER_TYPE_PACKETS;
    pp[1].id = SAI_POLICER_ATTR_MODE; pp[1].value.s32 = SAI_POLICER_MODE_SR_TCM;
    pp[2].id = SAI_POLICER_ATTR_CIR; pp[2].value.u64 = 1000;
    pp[3].id = SAI_POLICER_ATTR_CBS; pp[3].value.u64 = 1000;
    pp[4].id = SAI_POLICER_ATTR_RED_PACKET_ACTION; pp[4].value.s32 = SAI_PACKET_ACTION_DROP;
    sai_object_id_t pol = 0;
    OK(e.pola->create_policer(&pol, s, 5, pp));
    sai_attribute_t gg[2];
    gg[0].id = SAI_HOSTIF_TRAP_GROUP_ATTR_QUEUE; gg[0].value.u32 = 2;
    gg[1].id = SAI_HOSTIF_TRAP_GROUP_ATTR_POLICER; gg[1].value.oid = pol;
    sai_object_id_t gp = 0;
    OK(ha->create_hostif_trap_group(&gp, s, 2, gg));
    const char* m2c = "00200 id=1 u32=";   // ifcs_hostif_trap_attr_set on system trap 512 (MIRROR_TO_CPU), ENABLE
    sai_attribute_t ta;
    ta.id = SAI_HOSTIF_TRAP_ATTR_TRAP_GROUP;
    ta.value.oid = gp;
    m_log_clear();
    OK(ha->set_hostif_trap_attribute(trap, &ta));
    log = m_log();
    CHECK(!has(log, "id=25 "));                                          // METER_MODE: refused by the SDK, not set
    CHECK(has(log, "ifcs_cpu_queue_attr_set q=2 id=28 u32=672"));      // CIR, kbit/s
    CHECK(has(log, "ifcs_cpu_queue_attr_set q=2 id=29 u32=84000"));    // CBS, bytes
    CHECK(has(log, "ifcs_cpu_queue_attr_set q=2 id=27 u32=1"));        // CIR enable
    CHECK(log.find("id=28 u32=672") < log.find("q=2 id=27 u32=1"));    // enabled last
    CHECK(!has(log, "ifcs_acl_") && !has(log, "ifcs_ace_"));            // no ACL objects (0.12.41: no MIRROR match)
    // policer update: the queue meter follows
    m_log_clear();
    pp[2].value.u64 = 2000;
    OK(e.pola->set_policer_attribute(pol, &pp[2]));
    CHECK(has(m_log(), "ifcs_cpu_queue_attr_set q=2 id=28 u32=1344"));
    // group policer off: meter off; on again: back
    sai_attribute_t gpol;
    gpol.id = SAI_HOSTIF_TRAP_GROUP_ATTR_POLICER;
    gpol.value.oid = 0;
    m_log_clear();
    OK(ha->set_hostif_trap_group_attribute(gp, &gpol));
    CHECK(has(m_log(), "ifcs_cpu_queue_attr_set q=2 id=27 u32=0") && !has(m_log(), "id=27 u32=1"));
    gpol.value.oid = pol;
    m_log_clear();
    OK(ha->set_hostif_trap_group_attribute(gp, &gpol));
    CHECK(has(m_log(), "ifcs_cpu_queue_attr_set q=2 id=28 u32=1344") && has(m_log(), "q=2 id=27 u32=1"));
    // action DROP: MIRROR_TO_CPU off (isai); TRAP: on again
    sai_attribute_t act;
    act.id = SAI_HOSTIF_TRAP_ATTR_PACKET_ACTION;
    act.value.s32 = SAI_PACKET_ACTION_DROP;
    m_log_clear();
    OK(ha->set_hostif_trap_attribute(trap, &act));
    CHECK(has(m_log(), std::string(m2c) + "0"));
    act.value.s32 = SAI_PACKET_ACTION_TRAP;
    m_log_clear();
    OK(ha->set_hostif_trap_attribute(trap, &act));
    CHECK(has(m_log(), std::string(m2c) + "1"));
    // samplers do not touch the meter
    a.id = SAI_PORT_ATTR_INGRESS_SAMPLEPACKET_ENABLE;
    a.value.oid = 0;
    m_log_clear();
    OK(pa->set_port_attribute(p1, &a));
    CHECK(!has(m_log(), "ifcs_cpu_queue_attr_set"));
    a.value.oid = smp;
    OK(pa->set_port_attribute(p1, &a));
    // the group's queue is not the samples' queue: no meter there, ours goes
    sai_attribute_t gq;
    gq.id = SAI_HOSTIF_TRAP_GROUP_ATTR_QUEUE;
    gq.value.u32 = 3;
    m_log_clear();
    OK(ha->set_hostif_trap_group_attribute(gp, &gq));
    CHECK(has(m_log(), "ifcs_cpu_queue_attr_set q=2 id=27 u32=0") && !has(m_log(), "q=3 "));
    gq.value.u32 = 2;
    m_log_clear();
    OK(ha->set_hostif_trap_group_attribute(gp, &gq));
    CHECK(has(m_log(), "q=2 id=27 u32=1"));
    // the trap back to its first group; ours and its policer go (meter off)
    ta.value.oid = grp;
    m_log_clear();
    OK(ha->set_hostif_trap_attribute(trap, &ta));
    CHECK(has(m_log(), "ifcs_cpu_queue_attr_set q=2 id=27 u32=0"));
    OK(ha->remove_hostif_trap_group(gp));
    OK(e.pola->remove_policer(pol));

    // ---- clean up: samplers off, objects gone
    a.id = SAI_PORT_ATTR_INGRESS_SAMPLEPACKET_ENABLE;
    a.value.oid = 0;
    OK(pa->set_port_attribute(p1, &a));
    OK(pa->set_port_attribute(p2, &a));
    OK(spa->remove_samplepacket(smp));
    OK(ha->remove_hostif_trap(ttl));
    if (own_trap) OK(ha->remove_hostif_trap(trap));
    sai_status_t rg = ha->remove_hostif_trap_group(grp);
    CHECK(rg == SAI_STATUS_SUCCESS || (!own_trap && rg == SAI_STATUS_OBJECT_IN_USE));
}
