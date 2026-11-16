// SPDX-License-Identifier: Apache-2.0
// sai-teralynx end-to-end test on the build host: this libsai (sai-openbcm
// frontend + IFCS backend) driven through the public SAI API, with
// tests/mock_ifcs.c standing in for the vendor libifcs.so. Asserts the IFCS
// call sequence at init, the parsed init commands, and the SAI-visible
// results of ports, events, stats, hostifs, VLANs, L2 and L3.
// Usage: test_ifcs <absolute path of the mock libifcs.so> <scratch dir>
extern "C" {
#include <sai.h>
}

#include <arpa/inet.h>
#include <dlfcn.h>
#include <sys/stat.h>
#include <unistd.h>

#include <chrono>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <fstream>
#include <functional>
#include <map>
#include <mutex>
#include <sstream>
#include <string>
#include <thread>
#include <vector>

static int g_fail = 0, g_checks = 0;
#define CHECK(cond)                                                                            \
    do {                                                                                       \
        g_checks++;                                                                            \
        if (!(cond)) {                                                                         \
            g_fail++;                                                                          \
            std::fprintf(stderr, "FAIL %s:%d: %s\n", __FILE__, __LINE__, #cond);               \
        }                                                                                      \
    } while (0)
#define CHECK_EQ(a, b)                                                                         \
    do {                                                                                       \
        g_checks++;                                                                            \
        auto _a = (a);                                                                         \
        auto _b = (b);                                                                         \
        if (!(_a == _b)) {                                                                     \
            g_fail++;                                                                          \
            std::fprintf(stderr, "FAIL %s:%d: %s == %s (%lld vs %lld)\n", __FILE__, __LINE__, #a, #b, \
                         (long long)_a, (long long)_b);                                        \
        }                                                                                      \
    } while (0)
#define OK(x) CHECK_EQ((x), SAI_STATUS_SUCCESS)

// ---------------------------------------------------------------- profile
static std::map<std::string, std::string> g_profile;
static std::map<std::string, std::string>::iterator g_iter;
static const char* prof_get(sai_switch_profile_id_t, const char* k)
{
    auto it = g_profile.find(k);
    return it == g_profile.end() ? nullptr : it->second.c_str();
}
static int prof_next(sai_switch_profile_id_t, const char** k, const char** v)
{
    if (!k || !v) { g_iter = g_profile.begin(); return 0; }
    if (g_iter == g_profile.end()) return -1;
    *k = g_iter->first.c_str();
    *v = g_iter->second.c_str();
    ++g_iter;
    return 0;
}
static sai_service_method_table_t g_smt = {prof_get, prof_next};

static std::mutex g_mu;
static std::vector<sai_port_oper_status_notification_t> g_port_events;
static std::vector<std::pair<sai_fdb_event_t, sai_fdb_entry_t>> g_fdb_events;
static void on_port(uint32_t n, const sai_port_oper_status_notification_t* d)
{
    std::lock_guard<std::mutex> g(g_mu);
    for (uint32_t i = 0; i < n; i++) g_port_events.push_back(d[i]);
}
static void on_fdb(uint32_t n, const sai_fdb_event_notification_data_t* d)
{
    std::lock_guard<std::mutex> g(g_mu);
    for (uint32_t i = 0; i < n; i++) g_fdb_events.push_back({d[i].event_type, d[i].fdb_entry});
}
static bool wait_for(std::function<bool()> f)
{
    for (int i = 0; i < 300; i++) {
        { std::lock_guard<std::mutex> g(g_mu); if (f()) return true; }
        std::this_thread::sleep_for(std::chrono::milliseconds(10));
    }
    return false;
}

static void put(const std::string& path, const std::string& s)
{
    std::ofstream f(path);
    f << s;
}
static std::string get(const std::string& path)
{
    std::ifstream f(path);
    std::stringstream s;
    s << f.rdbuf();
    return s.str();
}
static std::vector<std::string> lines(const std::string& s)
{
    std::vector<std::string> v;
    std::istringstream is(s);
    std::string l;
    while (std::getline(is, l)) v.push_back(l);
    return v;
}
static bool has(const std::string& log, const std::string& needle) { return log.find(needle) != std::string::npos; }

// mock hooks
static const char* (*m_log)();
static void (*m_log_clear)();
static uint32_t (*m_admin)(uint32_t);
static int (*m_fire_link)(uint32_t, int);
static int (*m_fire_l2)(int, const uint8_t*, uint16_t, uint32_t);
static void (*m_set_stat)(uint32_t, uint32_t, uint64_t);
static int (*m_route_find)(const char*, int*, int*, uint32_t*);
static int (*m_route_count)();
static int (*m_l2_count)();
static int (*m_inited)();
static uint32_t (*m_fec)(uint32_t);
static uint32_t (*m_aging)();

template <typename T> static void hook(void* h, T& fn, const char* name)
{
    fn = reinterpret_cast<T>(dlsym(h, name));
    if (!fn) {
        std::fprintf(stderr, "mock hook %s missing\n", name);
        std::exit(2);
    }
}

int main(int argc, char** argv)
{
    if (argc < 3 || argv[1][0] != '/') {
        std::fprintf(stderr, "usage: test_ifcs /abs/path/mock-libifcs.so scratch-dir\n");
        return 2;
    }
    std::string mock = argv[1], dir = argv[2];
    mkdir(dir.c_str(), 0755);
    std::string init = dir + "/init.yaml", isg = dir + "/isg.yaml", map = dir + "/ports.map";
    std::string cmd1 = dir + "/a.cmd", cmd2 = dir + "/b.cmd", run = dir + "/run";
    put(init, "# generated init config\nifcs:\n  options:\n    log_level: \"info\"\n");
    put(isg, "  isg:\n  - id: \"0\"\n");
    put(map, "# devport lanes speed fec\n"
             "1 249,250,251,252,253,254,255,256 400000 rs\n"
             "2 241,242,243,244,245,246,247,248 400000 rs   # cage 2\n"
             "3 121 10000 none\n");
    put(cmd1, "node create 0\n"
              "ifcs set devport 1 rx_eq_fine_tune 1\n"
              "ifcs set devport 1 tx_eq_pre1 [2,2,2,2,2,2,2,2]\r\n"
              "ifcs set devport 2 rx_ctle_lf [1, 2, 3, 4]\n"
              "pci write led_cfg sck_rate_f 0x8\n"
              "pci write led_refresh_cliff 0x020\n");
    put(cmd2, "ifcs set devport 3 rx_gainshape1 [5]\n"
              "ifcs set devport 1 link_training 1\n"
              "ifcs set devport 1 bogus_attr 3\n"
              "port enable 1\n"
              "# comment\n");

    OK(sai_api_initialize(0, &g_smt));
    sai_switch_api_t* sw;
    sai_port_api_t* pa;
    sai_hostif_api_t* ha;
    sai_vlan_api_t* va;
    sai_bridge_api_t* ba;
    sai_route_api_t* rta;
    sai_router_interface_api_t* ra;
    sai_neighbor_api_t* na;
    sai_next_hop_api_t* nha;
    sai_fdb_api_t* fa;
    sai_lag_api_t* la;
    OK(sai_api_query(SAI_API_SWITCH, (void**)&sw));
    OK(sai_api_query(SAI_API_PORT, (void**)&pa));
    OK(sai_api_query(SAI_API_HOSTIF, (void**)&ha));
    OK(sai_api_query(SAI_API_VLAN, (void**)&va));
    OK(sai_api_query(SAI_API_BRIDGE, (void**)&ba));
    OK(sai_api_query(SAI_API_ROUTE, (void**)&rta));
    OK(sai_api_query(SAI_API_ROUTER_INTERFACE, (void**)&ra));
    OK(sai_api_query(SAI_API_NEIGHBOR, (void**)&na));
    OK(sai_api_query(SAI_API_NEXT_HOP, (void**)&nha));
    OK(sai_api_query(SAI_API_FDB, (void**)&fa));
    OK(sai_api_query(SAI_API_LAG, (void**)&la));

    sai_attribute_t ca[3];
    std::memset(ca, 0, sizeof(ca));
    ca[0].id = SAI_SWITCH_ATTR_INIT_SWITCH;
    ca[0].value.booldata = true;
    ca[1].id = SAI_SWITCH_ATTR_PORT_STATE_CHANGE_NOTIFY;
    ca[1].value.ptr = (void*)&on_port;
    ca[2].id = SAI_SWITCH_ATTR_FDB_EVENT_NOTIFY;
    ca[2].value.ptr = (void*)&on_fdb;
    sai_object_id_t s = 0;

    // ---- refusals before anything is loaded
    g_profile = {{"SAI_OPENBCM_BACKEND", "openbcm"}, {"SAI_INIT_CONFIG_FILE", init},
                 {"SAI_TERALYNX_LIBIFCS", mock}, {"SAI_TERALYNX_RUN_DIR", run}};
    CHECK(sw->create_switch(&s, 3, ca) != SAI_STATUS_SUCCESS);                 // no port map
    g_profile["SAI_TERALYNX_PORT_MAP"] = map;
    g_profile["SAI_TERALYNX_LIBIFCS"] = dir + "/does-not-exist.so";
    CHECK(sw->create_switch(&s, 3, ca) != SAI_STATUS_SUCCESS);                 // dlopen fails
    g_profile["SAI_TERALYNX_LIBIFCS"] = mock;
    g_profile["SAI_INIT_CONFIG_FILE"] = dir + "/missing.yaml";
    CHECK(sw->create_switch(&s, 3, ca) != SAI_STATUS_SUCCESS);                 // no init yaml
    CHECK(dlopen(mock.c_str(), RTLD_NOW | RTLD_NOLOAD) == nullptr);           // nothing loaded yet

    // ---- create_switch
    g_profile["SAI_INIT_CONFIG_FILE"] = init;
    g_profile["SAI_TERALYNX_CONFIG_APPEND"] = isg;
    g_profile["SAI_TERALYNX_INIT_CMDS"] = cmd1 + ":" + cmd2;
    OK(sw->create_switch(&s, 3, ca));
    void* mh = dlopen(mock.c_str(), RTLD_NOW | RTLD_NOLOAD);
    CHECK(mh != nullptr);
    if (!mh) return 1;
    hook(mh, m_log, "mock_ifcs_log");
    hook(mh, m_log_clear, "mock_ifcs_log_clear");
    hook(mh, m_admin, "mock_admin");
    hook(mh, m_fire_link, "mock_fire_link");
    hook(mh, m_fire_l2, "mock_fire_l2");
    hook(mh, m_set_stat, "mock_set_stat");
    hook(mh, m_route_find, "mock_route_find");
    hook(mh, m_route_count, "mock_route_count");
    hook(mh, m_l2_count, "mock_l2_count");
    hook(mh, m_inited, "mock_inited");
    hook(mh, m_fec, "mock_fec");
    hook(mh, m_aging, "mock_aging");

    CHECK(get(run + "/config.yaml") == get(init) + get(isg));
    std::string log = m_log();
    std::vector<std::string> L = lines(log);
    // The vendor innovium.user sequence, then our log hook in between.
    std::vector<std::string> want = {
        "ifcs_init cfg=" + run + "/config.yaml log=sai-teralynx boot=0 first=# generated init config",
        "ifcs_log_function_register",
        "ifcs_node_create node=0 count=0 attrs=null",
        "ifcs_attr_t_init",
        "ifcs_attr_t_id_set id=0",
        "ifcs_attr_t_value_data_set 1",
        "ifcs_node_attr_set node=0 count=1 id=0 u32=1",
    };
    CHECK(L.size() > want.size());
    for (size_t i = 0; i < want.size() && i < L.size(); i++) {
        if (L[i] != want[i]) std::fprintf(stderr, "init call %zu: got '%s' want '%s'\n", i, L[i].c_str(), want[i].c_str());
        CHECK(L[i] == want[i]);
    }
    // ports down before any init command, then the commands in file order
    size_t down3 = log.find("ifcs_devport_attr_set dp=3 id=23 u32=0");
    size_t c1 = log.find("ifcs_devport_attr_set dp=1 id=49 u32=1");
    CHECK(down3 != std::string::npos && c1 != std::string::npos && down3 < c1);
    std::vector<std::string> cmds;
    for (auto& l : L)
        if (l.rfind("ifcs_devport_attr_set", 0) == 0 && l.find("id=23 ") == std::string::npos) cmds.push_back(l);
    std::vector<std::string> want_cmds = {
        "ifcs_devport_attr_set dp=1 id=49 u32=1",
        "ifcs_devport_attr_set dp=1 id=43 i8=[2,2,2,2,2,2,2,2]",
        "ifcs_devport_attr_set dp=2 id=50 u8=[1,2,3,4]",
        "ifcs_devport_attr_set dp=3 id=53 u32l=[5]",
        "ifcs_devport_attr_set dp=1 id=21 u32=1",
    };
    CHECK_EQ(cmds.size(), want_cmds.size());
    for (size_t i = 0; i < want_cmds.size() && i < cmds.size(); i++) {
        if (cmds[i] != want_cmds[i]) std::fprintf(stderr, "cmd %zu: got '%s' want '%s'\n", i, cmds[i].c_str(), want_cmds[i].c_str());
        CHECK(cmds[i] == want_cmds[i]);
    }
    CHECK(!has(log, "id=23 u32=1"));            // "port enable" in a vendor file never enables a port
    CHECK(has(log, "ifcs_linkscan_event_register_notify node=0"));
    CHECK(has(log, "ifcs_l2_event_register_notify node=0"));
    CHECK(has(log, "ifcs_stp_port_add stp=0x100 members=4 attrs=1 state=3"));
    CHECK(has(log, "ifcs_l2vni_create vni=1 count=2 stp=0x100"));
    CHECK(has(log, "ifcs_l2vni_member_add vni=1 n=1 first=0x3e2003e8"));   // CPU sysport 1000 in VLAN 1

    // ---- ports
    sai_attribute_t a;
    sai_object_id_t plist[8];
    a.id = SAI_SWITCH_ATTR_PORT_LIST;
    a.value.objlist.count = 8;
    a.value.objlist.list = plist;
    OK(sw->get_switch_attribute(s, 1, &a));
    CHECK_EQ(a.value.objlist.count, 3u);
    sai_object_id_t p1 = plist[0], p2 = plist[1];
    uint32_t lanes[8];
    a.id = SAI_PORT_ATTR_HW_LANE_LIST;
    a.value.u32list.count = 8;
    a.value.u32list.list = lanes;
    OK(pa->get_port_attribute(p1, 1, &a));
    CHECK_EQ(a.value.u32list.count, 8u);
    CHECK_EQ(lanes[0], 249u);
    CHECK_EQ(lanes[7], 256u);
    a.id = SAI_PORT_ATTR_SPEED;
    OK(pa->get_port_attribute(p1, 1, &a));
    CHECK_EQ(a.value.u32, 400000u);
    a.value.u32 = 100000;
    CHECK(pa->set_port_attribute(p1, &a) != SAI_STATUS_SUCCESS);   // no live speed change
    a.value.u32 = 400000;
    OK(pa->set_port_attribute(p1, &a));
    CHECK_EQ(m_admin(1), 0u);
    a.id = SAI_PORT_ATTR_ADMIN_STATE;
    a.value.booldata = true;
    OK(pa->set_port_attribute(p1, &a));
    CHECK_EQ(m_admin(1), 1u);
    a.id = SAI_PORT_ATTR_FEC_MODE;
    a.value.s32 = SAI_PORT_FEC_MODE_FC;
    OK(pa->set_port_attribute(p1, &a));
    CHECK_EQ(m_fec(1), 3u);
    a.value.s32 = SAI_PORT_FEC_MODE_RS;
    OK(pa->set_port_attribute(p1, &a));
    CHECK_EQ(m_fec(1), 2u);                  // 50G lanes -> RS544 (KP)
    sai_object_id_t p3 = plist[2];
    a.value.s32 = SAI_PORT_FEC_MODE_FC;
    OK(pa->set_port_attribute(p3, &a));
    a.value.s32 = SAI_PORT_FEC_MODE_RS;
    OK(pa->set_port_attribute(p3, &a));
    CHECK_EQ(m_fec(3), 1u);                  // 10G NRZ -> RS528 (KR)
    OK(pa->set_port_attribute(p3, &a));      // already RS: kept
    CHECK_EQ(m_fec(3), 1u);
    m_log_clear();
    a.id = SAI_PORT_ATTR_MTU;
    a.value.u32 = 9100;
    OK(pa->set_port_attribute(p1, &a));
    CHECK(has(m_log(), "ifcs_sysport_attr_set sp=0x3e200001 id=51 u32=9122"));

    // link event -> port state notification
    m_fire_link(1, 1);
    CHECK(wait_for([&] { return !g_port_events.empty(); }));
    {
        std::lock_guard<std::mutex> g(g_mu);
        CHECK(!g_port_events.empty() && g_port_events[0].port_id == p1 &&
              g_port_events[0].port_state == SAI_PORT_OPER_STATUS_UP);
    }
    m_fire_link(77, 1);                       // not a front-panel devport: ignored

    // stats
    m_set_stat(1, 47, 123456);                // RX_BYTES_ALL
    m_set_stat(1, 3, 10);                     // RX unicast
    m_set_stat(1, 4, 5);
    m_set_stat(1, 5, 2);
    sai_stat_id_t ids[3] = {SAI_PORT_STAT_IF_IN_OCTETS, SAI_PORT_STAT_IF_IN_UCAST_PKTS, SAI_PORT_STAT_IF_IN_NON_UCAST_PKTS};
    uint64_t vals[3] = {0, 0, 0};
    OK(pa->get_port_stats(p1, 3, ids, vals));
    CHECK_EQ(vals[0], 123456u);
    CHECK_EQ(vals[1], 10u);
    CHECK_EQ(vals[2], 7u);
    OK(pa->clear_port_all_stats(p1));
    CHECK(has(m_log(), "ifcs_devport_stats_clear dp=1 count=111"));

    // ---- hostif
    sai_attribute_t h[3];
    std::memset(h, 0, sizeof(h));
    h[0].id = SAI_HOSTIF_ATTR_TYPE;
    h[0].value.s32 = SAI_HOSTIF_TYPE_NETDEV;
    h[1].id = SAI_HOSTIF_ATTR_OBJ_ID;
    h[1].value.oid = p1;
    h[2].id = SAI_HOSTIF_ATTR_NAME;
    std::snprintf(h[2].value.chardata, sizeof(h[2].value.chardata), "Ethernet0");
    sai_object_id_t hif = 0;
    OK(ha->create_hostif(&hif, s, 3, h));
    CHECK(has(m_log(), "ifcs_netdev_create h=") && has(m_log(), "name=Ethernet0 devport=1"));
    CHECK(has(m_log(), " tag=1"));            // SAI default STRIP
    a.id = SAI_HOSTIF_ATTR_OPER_STATUS;
    a.value.booldata = true;
    OK(ha->set_hostif_attribute(hif, &a));

    // ---- VLAN 100 with port 1 untagged
    sai_object_id_t vlan = 0;
    a.id = SAI_VLAN_ATTR_VLAN_ID;
    a.value.u16 = 100;
    OK(va->create_vlan(&vlan, s, 1, &a));
    CHECK(has(m_log(), "ifcs_l2vni_create vni=100 count=2"));
    CHECK(has(m_log(), "ifcs_l2vni_member_add vni=100 n=1 first=0x3e2003e8"));
    sai_object_id_t bridge = 0, bps[8];
    a.id = SAI_SWITCH_ATTR_DEFAULT_1Q_BRIDGE_ID;
    OK(sw->get_switch_attribute(s, 1, &a));
    bridge = a.value.oid;
    a.id = SAI_BRIDGE_ATTR_PORT_LIST;
    a.value.objlist.count = 8;
    a.value.objlist.list = bps;
    OK(ba->get_bridge_attribute(bridge, 1, &a));
    sai_object_id_t bp1 = 0;
    for (uint32_t i = 0; i < a.value.objlist.count; i++) {
        sai_attribute_t q;
        q.id = SAI_BRIDGE_PORT_ATTR_PORT_ID;
        if (ba->get_bridge_port_attribute(bps[i], 1, &q) == SAI_STATUS_SUCCESS && q.value.oid == p1) bp1 = bps[i];
    }
    CHECK(bp1 != 0);
    sai_attribute_t vm[3];
    vm[0].id = SAI_VLAN_MEMBER_ATTR_VLAN_ID;
    vm[0].value.oid = vlan;
    vm[1].id = SAI_VLAN_MEMBER_ATTR_BRIDGE_PORT_ID;
    vm[1].value.oid = bp1;
    vm[2].id = SAI_VLAN_MEMBER_ATTR_VLAN_TAGGING_MODE;
    vm[2].value.s32 = SAI_VLAN_TAGGING_MODE_UNTAGGED;
    sai_object_id_t member = 0;
    OK(va->create_vlan_member(&member, s, 3, vm));
    CHECK(has(m_log(), "ifcs_l2vni_member_add vni=100 n=1 first=0x3e200001"));
    CHECK(has(m_log(), "ifcs_sysport_attr_set sp=0x3e200001 id=43 u32=100"));

    // FDB: learned event from the SDK -> SAI notification; static entry + flush
    const uint8_t mac[6] = {0x02, 0x11, 0x22, 0x33, 0x44, 0x55};
    m_fire_l2(0, mac, 100, 1);
    CHECK(wait_for([&] { return !g_fdb_events.empty(); }));
    {
        std::lock_guard<std::mutex> g(g_mu);
        CHECK(!g_fdb_events.empty() && g_fdb_events[0].first == SAI_FDB_EVENT_LEARNED &&
              g_fdb_events[0].second.bv_id == vlan);
    }
    sai_fdb_entry_t fe;
    std::memset(&fe, 0, sizeof(fe));
    fe.switch_id = s;
    fe.bv_id = vlan;
    const uint8_t mac2[6] = {0x02, 0, 0, 0, 0, 0x99};
    std::memcpy(fe.mac_address, mac2, 6);
    sai_attribute_t fa_[2];
    fa_[0].id = SAI_FDB_ENTRY_ATTR_TYPE;
    fa_[0].value.s32 = SAI_FDB_ENTRY_TYPE_STATIC;
    fa_[1].id = SAI_FDB_ENTRY_ATTR_BRIDGE_PORT_ID;
    fa_[1].value.oid = bp1;
    OK(fa->create_fdb_entry(&fe, 2, fa_));
    CHECK_EQ(m_l2_count(), 1);
    a.id = SAI_FDB_FLUSH_ATTR_ENTRY_TYPE;
    a.value.s32 = SAI_FDB_FLUSH_ENTRY_TYPE_ALL;
    OK(fa->flush_fdb_entries(s, 1, &a));
    CHECK_EQ(m_l2_count(), 0);
    a.id = SAI_SWITCH_ATTR_FDB_AGING_TIME;
    a.value.u32 = 600;
    OK(sw->set_switch_attribute(s, &a));
    CHECK_EQ(m_aging(), 600u);

    // ---- L3: the default routes orchagent installs first (DROP)
    a.id = SAI_SWITCH_ATTR_DEFAULT_VIRTUAL_ROUTER_ID;
    OK(sw->get_switch_attribute(s, 1, &a));
    sai_object_id_t vr = a.value.oid;
    sai_route_entry_t re;
    std::memset(&re, 0, sizeof(re));
    re.switch_id = s;
    re.vr_id = vr;
    re.destination.addr_family = SAI_IP_ADDR_FAMILY_IPV4;
    a.id = SAI_ROUTE_ENTRY_ATTR_PACKET_ACTION;
    a.value.s32 = SAI_PACKET_ACTION_DROP;
    OK(rta->create_route_entry(&re, 1, &a));
    sai_route_entry_t re6 = re;
    re6.destination.addr_family = SAI_IP_ADDR_FAMILY_IPV6;
    OK(rta->create_route_entry(&re6, 1, &a));
    CHECK_EQ(m_route_count(), 2);
    int fwd = -1, ctc = -1;
    uint32_t nh = 0;
    const std::string l3vni0 = "26200001";   // IFCS_HANDLE_L3VNI(1) = VRF 0
    CHECK(m_route_find((l3vni0 + "/4/00000000/00000000").c_str(), &fwd, &ctc, &nh) && fwd == 1 && ctc == 0);
    CHECK(m_route_find((l3vni0 + "/6/00000000000000000000000000000000/00000000000000000000000000000000").c_str(), &fwd,
                       &ctc, &nh) && fwd == 1);

    // port RIF on port 2, neighbor, next hop, a route, an ip2me route
    sai_attribute_t ri[4];
    ri[0].id = SAI_ROUTER_INTERFACE_ATTR_VIRTUAL_ROUTER_ID;
    ri[0].value.oid = vr;
    ri[1].id = SAI_ROUTER_INTERFACE_ATTR_TYPE;
    ri[1].value.s32 = SAI_ROUTER_INTERFACE_TYPE_PORT;
    ri[2].id = SAI_ROUTER_INTERFACE_ATTR_PORT_ID;
    ri[2].value.oid = p2;
    ri[3].id = SAI_ROUTER_INTERFACE_ATTR_SRC_MAC_ADDRESS;
    const uint8_t rmac[6] = {0x0c, 0x48, 0xc6, 0x7a, 0x55, 0xce};
    std::memcpy(ri[3].value.mac, rmac, 6);
    sai_object_id_t rif = 0;
    OK(ra->create_router_interface(&rif, s, 4, ri));
    CHECK(has(m_log(), "ifcs_intf_create") && has(m_log(), "type=1 fwd=0x26200001"));
    sai_neighbor_entry_t ne;
    std::memset(&ne, 0, sizeof(ne));
    ne.switch_id = s;
    ne.rif_id = rif;
    ne.ip_address.addr_family = SAI_IP_ADDR_FAMILY_IPV4;
    ne.ip_address.addr.ip4 = htonl(0x0a000002);
    a.id = SAI_NEIGHBOR_ENTRY_ATTR_DST_MAC_ADDRESS;
    const uint8_t nmac[6] = {0x02, 0xaa, 0xbb, 0xcc, 0xdd, 0xee};
    std::memcpy(a.value.mac, nmac, 6);
    OK(na->create_neighbor_entry(&ne, 1, &a));
    CHECK(has(m_log(), "ifcs_nexthop_create") && has(m_log(), "dest=0x3e200002"));
    CHECK(m_route_find((l3vni0 + "/4/0a000002/ffffffff").c_str(), &fwd, &ctc, &nh) && fwd == 0 && nh != 0);
    sai_attribute_t nhat[3];
    nhat[0].id = SAI_NEXT_HOP_ATTR_TYPE;
    nhat[0].value.s32 = SAI_NEXT_HOP_TYPE_IP;
    nhat[1].id = SAI_NEXT_HOP_ATTR_IP;
    nhat[1].value.ipaddr = ne.ip_address;
    nhat[2].id = SAI_NEXT_HOP_ATTR_ROUTER_INTERFACE_ID;
    nhat[2].value.oid = rif;
    sai_object_id_t nhid = 0;
    OK(nha->create_next_hop(&nhid, s, 3, nhat));
    sai_route_entry_t r24 = re;
    r24.destination.addr.ip4 = htonl(0x0a010000);
    r24.destination.mask.ip4 = htonl(0xffffff00);
    a.id = SAI_ROUTE_ENTRY_ATTR_NEXT_HOP_ID;
    a.value.oid = nhid;
    OK(rta->create_route_entry(&r24, 1, &a));
    CHECK(m_route_find((l3vni0 + "/4/0a010000/ffffff00").c_str(), &fwd, &ctc, &nh) && fwd == 0 && nh != 0);
    a.id = SAI_SWITCH_ATTR_CPU_PORT;
    OK(sw->get_switch_attribute(s, 1, &a));
    sai_object_id_t cpu = a.value.oid;
    sai_route_entry_t me = re;
    me.destination.addr.ip4 = htonl(0x0a000001);
    me.destination.mask.ip4 = 0xffffffff;
    a.id = SAI_ROUTE_ENTRY_ATTR_NEXT_HOP_ID;
    a.value.oid = cpu;
    OK(rta->create_route_entry(&me, 1, &a));
    CHECK(has(m_log(), "ifcs_hostif_trap_create") && has(m_log(), "queue=0"));
    CHECK(m_route_find((l3vni0 + "/4/0a000001/ffffffff").c_str(), &fwd, &ctc, &nh) && fwd == 1 && ctc == 1);
    // route re-pointed to drop: kind change = delete + create
    a.id = SAI_ROUTE_ENTRY_ATTR_PACKET_ACTION;
    a.value.s32 = SAI_PACKET_ACTION_DROP;
    OK(rta->set_route_entry_attribute(&r24, &a));
    CHECK(m_route_find((l3vni0 + "/4/0a010000/ffffff00").c_str(), &fwd, &ctc, &nh) && fwd == 1);
    OK(rta->remove_route_entry(&r24));
    CHECK(!m_route_find((l3vni0 + "/4/0a010000/ffffff00").c_str(), &fwd, &ctc, &nh));

    // ---- NOT_IMPLEMENTED stays honest
    sai_object_id_t lag = 0;
    CHECK(la->create_lag(&lag, s, 0, nullptr) != SAI_STATUS_SUCCESS);

    // ---- temperature: master 55.5, remotes 54.0 53.5 (-300 left out) 52.0
    int32_t temps[8];
    a.id = SAI_SWITCH_ATTR_TEMP_LIST;
    a.value.s32list.count = 8;
    a.value.s32list.list = temps;
    OK(sw->get_switch_attribute(s, 1, &a));
    CHECK_EQ(a.value.s32list.count, 4u);
    a.id = SAI_SWITCH_ATTR_MAX_TEMP;
    OK(sw->get_switch_attribute(s, 1, &a));
    CHECK(a.value.s32 == 55 || a.value.s32 == 56);
    a.id = SAI_SWITCH_ATTR_AVAILABLE_IPV4_ROUTE_ENTRY;
    OK(sw->get_switch_attribute(s, 1, &a));
    CHECK(a.value.u32 > 0 && a.value.u32 < 24576u);

    // TEST_IFCS_DUMP=1: print every IFCS call the backend made (before removal)
    if (std::getenv("TEST_IFCS_DUMP")) std::fputs(m_log(), stderr);

    // ---- remove: ports down, callbacks gone, deinit
    m_log_clear();
    OK(sw->remove_switch(s));
    log = m_log();
    size_t d1 = log.find("ifcs_devport_attr_set dp=1 id=23 u32=0");
    size_t un = log.find("ifcs_linkscan_event_unregister_notify");
    size_t de = log.find("ifcs_deinit type=0");
    CHECK(d1 != std::string::npos && un != std::string::npos && de != std::string::npos && d1 < un && un < de);
    CHECK(has(log, "ifcs_netdev_delete"));
    CHECK_EQ(m_inited(), 0);
    OK(sai_api_uninitialize());

    std::printf("test_ifcs: %d checks, %d failed\n", g_checks, g_fail);
    if (g_fail) std::fprintf(stderr, "--- mock log at failure ---\n%s", m_log());
    return g_fail ? 1 : 0;
}
