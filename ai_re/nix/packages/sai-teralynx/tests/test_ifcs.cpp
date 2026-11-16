// SPDX-License-Identifier: Apache-2.0
// sai-teralynx end-to-end test on the build host: this libsai (sai-openbcm
// frontend + IFCS backend) driven through the public SAI API, with the mock
// libifcs (tests/mock/) standing in for the vendor libifcs.so. main() writes
// the profile files, checks the refusals and creates the switch; the areas
// (tests/t_<area>.cpp, harness.h) then run in order on that switch.
// Usage: test_ifcs <absolute path of the mock libifcs.so> <scratch dir>
#include "harness.h"

int g_fail = 0, g_checks = 0;

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

std::mutex g_mu;
std::vector<sai_port_oper_status_notification_t> g_port_events;
std::vector<std::pair<sai_fdb_event_t, sai_fdb_entry_t>> g_fdb_events;
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
void* g_mock;
// mock hooks
const char* (*m_log)();
void (*m_log_clear)();
uint32_t (*m_admin)(uint32_t);
int (*m_fire_link)(uint32_t, int);
int (*m_fire_l2)(int, const uint8_t*, uint16_t, uint32_t);
void (*m_set_stat)(uint32_t, uint32_t, uint64_t);
int (*m_route_find)(const char*, int*, int*, uint32_t*);
int (*m_route_count)();
int (*m_l2_count)();
int (*m_inited)();
uint32_t (*m_fec)(uint32_t);
uint32_t (*m_aging)();

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
    Env e;
    e.dir = dir;
    e.init = init;
    e.isg = isg;
    e.run = run;
    OK(sai_api_query(SAI_API_SWITCH, (void**)&e.sw));
    OK(sai_api_query(SAI_API_PORT, (void**)&e.pa));
    OK(sai_api_query(SAI_API_HOSTIF, (void**)&e.ha));
    OK(sai_api_query(SAI_API_VLAN, (void**)&e.va));
    OK(sai_api_query(SAI_API_BRIDGE, (void**)&e.ba));
    OK(sai_api_query(SAI_API_ROUTE, (void**)&e.rta));
    OK(sai_api_query(SAI_API_ROUTER_INTERFACE, (void**)&e.ra));
    OK(sai_api_query(SAI_API_NEIGHBOR, (void**)&e.na));
    OK(sai_api_query(SAI_API_NEXT_HOP, (void**)&e.nha));
    OK(sai_api_query(SAI_API_FDB, (void**)&e.fa));
    OK(sai_api_query(SAI_API_LAG, (void**)&e.la));
    OK(sai_api_query(SAI_API_POLICER, (void**)&e.pola));
    OK(sai_api_query(SAI_API_ACL, (void**)&e.acla));
    sai_switch_api_t* sw = e.sw;

    sai_attribute_t ca[3];
    std::memset(ca, 0, sizeof(ca));
    ca[0].id = SAI_SWITCH_ATTR_INIT_SWITCH;
    ca[0].value.booldata = true;
    ca[1].id = SAI_SWITCH_ATTR_PORT_STATE_CHANGE_NOTIFY;
    ca[1].value.ptr = (void*)&on_port;
    ca[2].id = SAI_SWITCH_ATTR_FDB_EVENT_NOTIFY;
    ca[2].value.ptr = (void*)&on_fdb;
    sai_object_id_t& s = e.s;

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
    g_profile["SAI_TERALYNX_LINK_STATUS"] = "cached";   // t_port.cpp tests the cache (the default reads IFCS)
    g_profile["SAI_TERALYNX_NETDEV"] = "create";       // t_hostif.cpp tests create mode (the default adopts)
    if ((dir + "/diag.sock").size() < 100) g_profile["SAI_OPENBCM_DIAG_SOCKET"] = dir + "/diag.sock";   // t_counters.cpp
    g_profile["SAI_TERALYNX_HASH_FIELDS"] = "src_ip,dst_ip,ip_protocol,l4_src_port,l4_dst_port,in_port";   // t_lag.cpp
    g_profile["SAI_TERALYNX_HASH_SEED"] = "10";
    OK(sw->create_switch(&s, 3, ca));
    void* mh = g_mock = dlopen(mock.c_str(), RTLD_NOW | RTLD_NOLOAD);
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


    test_switch_init(e);
    test_ports(e);
    test_hostif(e);
    test_l2(e);
    test_l3(e);
    test_lag(e);
    test_copp(e);
    test_acl(e);
    test_qos(e);
    test_mirror(e);
    test_sflow(e);
    test_counters(e);
    test_hooks(e);
    test_switch_misc(e);

    // TEST_IFCS_DUMP=1: print every IFCS call the backend made (before removal)
    if (std::getenv("TEST_IFCS_DUMP")) std::fputs(m_log(), stderr);

    test_switch_remove(e);
    OK(sai_api_uninitialize());

    std::printf("test_ifcs: %d checks, %d failed\n", g_checks, g_fail);
    if (g_fail) std::fprintf(stderr, "--- mock log at failure ---\n%s", m_log());
    return g_fail ? 1 : 0;
}
