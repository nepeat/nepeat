// SPDX-License-Identifier: Apache-2.0
// sai-bench: the same SAI workload against any libsai (our sai-teralynx on
// arm64, Marvell's isai under qemu-ipd), for comparing init time and per-call
// latency, plus a link mode for the cage loop.
//   sai-bench PROFILE [--bench] [--link SECONDS] [--lanes 209,81]
//             [--sai-log LEVEL] [--sai-serdes CMDFILE] [--lt] [--fec rs|none]
//             [--ifcs016-cmds FILE[:FILE...]]
// PROFILE: key=value lines (the sai.profile syncd would get).
// --bench   32 x 3 port attribute gets, 32 MTU sets, 32 counter reads (4 ids),
//           1000 IPv4 DROP routes created + removed in a new VRF, 100 static FDB
//           entries created + removed (VLAN 100, one bridge port), each timed.
// --link    admin up the --lanes ports (MTU 9100), print oper status and FEC
//           counters every 2 s for SECONDS.
// --sai-serdes  TX taps from a vendor "ifcs set devport <id> tx_eq_* [..]" file,
//           given as SAI_PORT_SERDES TX_FIR_{PRE1,PRE2,PRE3,ATTN,POST,MAIN} per
//           port (devport id = position of the port in the lane-sorted order of
//           --devports, see below).
// --ifcs016-cmds  the same vendor files applied through the IFCS 0.16 API that
//           isai's libsai exports (ifcs_devport_attr_set; isai only: the 0.12
//           libifcs under sai-teralynx has another attribute layout).
// --devports LANE=ID,...  first lane -> IFCS devport id for the two options above.
// Output: "BENCH <name> <calls> <total ms> <us/call> <failed>" and "LINK ..." lines.
extern "C" {
#include <sai.h>
}
#include <dlfcn.h>
#include <time.h>
#include <atomic>
#include <csignal>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <fstream>
#include <map>
#include <sstream>
#include <string>
#include <unistd.h>
#include <arpa/inet.h>
#include <vector>

static std::map<std::string, std::string> g_profile;
static std::map<std::string, std::string>::iterator g_it;
static std::atomic<bool> g_stop{false};
static const char* prof_get(sai_switch_profile_id_t, const char* k) {
    auto it = g_profile.find(k);
    return it == g_profile.end() ? nullptr : it->second.c_str();
}
static int prof_next(sai_switch_profile_id_t, const char** k, const char** v) {
    if (k == nullptr) { g_it = g_profile.begin(); return 0; }
    if (g_it == g_profile.end()) return -1;
    *k = g_it->first.c_str(); *v = g_it->second.c_str(); ++g_it; return 0;
}
static void on_port_state(uint32_t n, const sai_port_oper_status_notification_t* d) {
    for (uint32_t i = 0; i < n; i++)
        printf("event port 0x%lx oper %d\n", (unsigned long)d[i].port_id, (int)d[i].port_state);
    fflush(stdout);
}
static void on_shutdown(sai_object_id_t) { printf("event shutdown request\n"); fflush(stdout); }
static void on_signal(int) { g_stop = true; }
static double now_ms() { timespec t; clock_gettime(CLOCK_MONOTONIC, &t); return t.tv_sec * 1e3 + t.tv_nsec / 1e6; }

struct Timer {
    const char* name; double t0; int n = 0, failed = 0;
    explicit Timer(const char* nm) : name(nm), t0(now_ms()) {}
    void call(sai_status_t s) { n++; if (s != SAI_STATUS_SUCCESS) { if (failed++ < 3) printf("  %s: call %d status %d\n", name, n, (int)s); } }
    void done() { double ms = now_ms() - t0; printf("BENCH %-22s %5d %10.2f %10.1f %d\n", name, n, ms, n ? ms * 1e3 / n : 0.0, failed); fflush(stdout); }
};

// vendor "ifcs set devport <id> <attr> <value|[a,b,...]>" lines
struct Cmd { uint32_t devport; std::string attr; std::vector<long> v; bool list; };
static std::vector<Cmd> read_cmds(const std::string& path) {
    std::vector<Cmd> out; std::ifstream f(path); std::string line;
    while (std::getline(f, line)) {
        std::istringstream is(line); std::string a, b, c, id, attr, rest, t;
        if (!(is >> a >> b >> c >> id >> attr) || a != "ifcs" || b != "set" || c != "devport") continue;
        while (is >> t) rest += t;
        Cmd cmd; cmd.devport = (uint32_t)strtoul(id.c_str(), nullptr, 0); cmd.attr = attr; cmd.list = !rest.empty() && rest[0] == '[';
        std::string body = cmd.list ? rest.substr(1, rest.size() - 2) : rest;
        std::stringstream ls(body); std::string e;
        while (std::getline(ls, e, ',')) cmd.v.push_back(strtol(e.c_str(), nullptr, 0));
        out.push_back(cmd);
    }
    printf("%s: %zu devport commands\n", path.c_str(), out.size());
    return out;
}

// IFCS 0.16 (isai 6.2.0 libsai exports): attribute ids from its pyctypes, value
// kinds as the vendor CLI's setters; ifcs_attr_t is 224 bytes there.
enum Kind { U32, I8L, U8L, U32L };
static const struct { const char* name; uint32_t id; Kind k; } k_ifcs016[] = {
    {"rx_eq_fine_tune", 49, U32}, {"link_training", 21, U32},
    {"tx_eq_pre1", 43, I8L}, {"tx_eq_pre2", 44, I8L}, {"tx_eq_pre3", 45, I8L}, {"tx_eq_attn", 46, I8L}, {"tx_eq_post", 47, I8L},
    {"rx_ctle_lf", 50, U8L}, {"rx_ctle_hf", 51, U8L}, {"rx_ctle_bw", 52, U8L},
    {"rx_gainshape1", 53, U32L}, {"rx_gainshape2", 54, U32L}, {"rx_agc_target", 55, U32L},
    {"rx_eye_disqualify_threshold_25g", 56, U32L},
};
struct alignas(16) Attr016 { uint8_t b[224]; };
struct List8 { uint32_t count; void* arr; };
static int apply_ifcs016(const std::vector<Cmd>& cmds) {
    auto init = (int (*)(void*))dlsym(RTLD_DEFAULT, "ifcs_attr_t_init");
    auto idset = (int (*)(void*, uint32_t))dlsym(RTLD_DEFAULT, "ifcs_attr_t_id_set");
    auto u32 = (int (*)(void*, uint32_t))dlsym(RTLD_DEFAULT, "ifcs_attr_t_value_u32_set");
    auto i8l = (int (*)(void*, const List8*))dlsym(RTLD_DEFAULT, "ifcs_attr_t_value_i8_list_set");
    auto u8l = (int (*)(void*, const List8*))dlsym(RTLD_DEFAULT, "ifcs_attr_t_value_u8_list_set");
    auto u32l = (int (*)(void*, const List8*))dlsym(RTLD_DEFAULT, "ifcs_attr_t_value_u32_list_set");
    auto dset = (int (*)(uint32_t, uint32_t, uint32_t, void*))dlsym(RTLD_DEFAULT, "ifcs_devport_attr_set");
    if (!init || !idset || !u32 || !i8l || !u8l || !u32l || !dset) { printf("ifcs016: IFCS API not found\n"); return -1; }
    int bad = 0, n = 0;
    std::map<std::string, int> per;
    for (auto& c : cmds) {
        int k = -1;
        for (size_t i = 0; i < sizeof k_ifcs016 / sizeof k_ifcs016[0]; i++) if (c.attr == k_ifcs016[i].name) k = (int)i;
        if (k < 0) { per["skip " + c.attr]++; continue; }
        Attr016 a; init(&a); idset(&a, k_ifcs016[k].id);
        int8_t i8[8]; uint8_t u8[8]; uint32_t w32[8];
        List8 l = {(uint32_t)c.v.size(), nullptr};
        for (size_t i = 0; i < c.v.size() && i < 8; i++) { i8[i] = (int8_t)c.v[i]; u8[i] = (uint8_t)c.v[i]; w32[i] = (uint32_t)c.v[i]; }
        int st = 0;
        switch (k_ifcs016[k].k) {
        case U32: st = u32(&a, c.v.empty() ? 0 : (uint32_t)c.v[0]); break;
        case I8L: l.arr = i8; st = i8l(&a, &l); break;
        case U8L: l.arr = u8; st = u8l(&a, &l); break;
        case U32L: l.arr = w32; st = u32l(&a, &l); break;
        }
        if (st == 0) st = dset(0, c.devport, 1, &a);
        n++; per[c.attr]++;
        if (st != 0) { if (bad++ < 5) printf("ifcs016: devport %u %s: status %d\n", c.devport, c.attr.c_str(), st); }
    }
    for (auto& p : per) printf("ifcs016: %-34s %d\n", p.first.c_str(), p.second);
    printf("ifcs016: %d applied, %d failed\n", n, bad);
    return bad;
}

int main(int argc, char** argv) {
    if (argc < 2) { fprintf(stderr, "usage: %s PROFILE [options]\n", argv[0]); return 2; }
    std::ifstream pf(argv[1]);
    for (std::string l; std::getline(pf, l);) {
        auto eq = l.find('=');
        if (l.empty() || l[0] == '#' || eq == std::string::npos) continue;
        g_profile[l.substr(0, eq)] = l.substr(eq + 1);
    }
    bool do_bench = false, lt = false; int link_s = 0; std::string fec = "keep", saiserdes, ifcscmds, sailog;
    std::vector<uint32_t> lanes = {209, 81}; std::map<uint32_t, uint32_t> devport_of = {{209, 1}, {81, 16}};
    for (int i = 2; i < argc; i++) {
        std::string o = argv[i]; auto next = [&]() { return i + 1 < argc ? std::string(argv[++i]) : std::string(); };
        if (o == "--bench") do_bench = true;
        else if (o == "--link") link_s = atoi(next().c_str());
        else if (o == "--lt") lt = true;
        else if (o == "--fec") fec = next();
        else if (o == "--sai-serdes") saiserdes = next();
        else if (o == "--ifcs016-cmds") ifcscmds = next();
        else if (o == "--sai-log") sailog = next();
        else if (o == "--lanes") { lanes.clear(); std::stringstream s(next()); std::string e; while (std::getline(s, e, ',')) lanes.push_back((uint32_t)strtoul(e.c_str(), nullptr, 0)); }
        else if (o == "--devports") { devport_of.clear(); std::stringstream s(next()); std::string e; while (std::getline(s, e, ',')) { auto q = e.find('='); devport_of[(uint32_t)strtoul(e.substr(0, q).c_str(), nullptr, 0)] = (uint32_t)strtoul(e.substr(q + 1).c_str(), nullptr, 0); } }
        else { fprintf(stderr, "unknown option %s\n", o.c_str()); return 2; }
    }
    signal(SIGINT, on_signal); signal(SIGTERM, on_signal);

    sai_service_method_table_t svc = {prof_get, prof_next};
    double t0 = now_ms();
    sai_status_t s = sai_api_initialize(0, &svc);
    double t1 = now_ms();
    printf("BENCH %-22s %5d %10.2f %10.1f %d\n", "sai_api_initialize", 1, t1 - t0, (t1 - t0) * 1e3, s != 0); fflush(stdout);
    if (s) return 1;
    sai_switch_api_t* sw = nullptr; sai_port_api_t* pa = nullptr; sai_route_api_t* ra = nullptr;
    sai_virtual_router_api_t* va = nullptr; sai_vlan_api_t* vl = nullptr; sai_bridge_api_t* br = nullptr; sai_fdb_api_t* fa = nullptr;
    sai_api_query(SAI_API_SWITCH, (void**)&sw); sai_api_query(SAI_API_PORT, (void**)&pa); sai_api_query(SAI_API_ROUTE, (void**)&ra);
    sai_api_query(SAI_API_VIRTUAL_ROUTER, (void**)&va); sai_api_query(SAI_API_VLAN, (void**)&vl);
    sai_api_query(SAI_API_BRIDGE, (void**)&br); sai_api_query(SAI_API_FDB, (void**)&fa);
    if (!sailog.empty()) {
        sai_log_level_t lv = sailog == "error" ? SAI_LOG_LEVEL_ERROR : sailog == "warn" ? SAI_LOG_LEVEL_WARN
            : sailog == "notice" ? SAI_LOG_LEVEL_NOTICE : sailog == "info" ? SAI_LOG_LEVEL_INFO : SAI_LOG_LEVEL_DEBUG;
        int ok = 0;
        for (int api = 1; api < (int)SAI_API_MAX; api++) ok += sai_log_set((sai_api_t)api, lv) == SAI_STATUS_SUCCESS;
        printf("sai_log_set %s on %d apis\n", sailog.c_str(), ok);
    }

    sai_attribute_t at[4] = {};
    at[0].id = SAI_SWITCH_ATTR_INIT_SWITCH; at[0].value.booldata = true;
    at[1].id = SAI_SWITCH_ATTR_SRC_MAC_ADDRESS;
    const uint8_t mac[6] = {0x0e, 0x48, 0xc6, 0x7a, 0x55, 0xce};
    memcpy(at[1].value.mac, mac, 6);
    at[2].id = SAI_SWITCH_ATTR_PORT_STATE_CHANGE_NOTIFY; at[2].value.ptr = (sai_pointer_t)on_port_state;
    at[3].id = SAI_SWITCH_ATTR_SWITCH_SHUTDOWN_REQUEST_NOTIFY; at[3].value.ptr = (sai_pointer_t)on_shutdown;
    sai_object_id_t swid = 0;
    t0 = now_ms();
    s = sw->create_switch(&swid, 4, at);
    t1 = now_ms();
    printf("BENCH %-22s %5d %10.2f %10.1f %d\n", "create_switch", 1, t1 - t0, (t1 - t0) * 1e3, s != 0); fflush(stdout);
    if (s) { sai_api_uninitialize(); return 1; }

    sai_attribute_t a = {};
    std::vector<sai_object_id_t> all(512), ports; std::map<uint32_t, sai_object_id_t> by_lane;
    a.id = SAI_SWITCH_ATTR_PORT_LIST; a.value.objlist.count = all.size(); a.value.objlist.list = all.data();
    sw->get_switch_attribute(swid, 1, &a);
    all.resize(a.value.objlist.count);
    for (auto p : all) {
        uint32_t l[8] = {}; sai_attribute_t q = {};
        q.id = SAI_PORT_ATTR_HW_LANE_LIST; q.value.u32list.count = 8; q.value.u32list.list = l;
        if (pa->get_port_attribute(p, 1, &q) == SAI_STATUS_SUCCESS && q.value.u32list.count == 8) { ports.push_back(p); by_lane[l[0]] = p; }
    }
    printf("ports %zu (8-lane %zu)\n", all.size(), ports.size());

    if (do_bench) {
        { Timer t("port get oper_status"); for (auto p : ports) { a = {}; a.id = SAI_PORT_ATTR_OPER_STATUS; t.call(pa->get_port_attribute(p, 1, &a)); } t.done(); }
        { Timer t("port get speed"); for (auto p : ports) { a = {}; a.id = SAI_PORT_ATTR_SPEED; t.call(pa->get_port_attribute(p, 1, &a)); } t.done(); }
        { Timer t("port get mtu"); for (auto p : ports) { a = {}; a.id = SAI_PORT_ATTR_MTU; t.call(pa->get_port_attribute(p, 1, &a)); } t.done(); }
        { Timer t("port set mtu"); for (auto p : ports) { a = {}; a.id = SAI_PORT_ATTR_MTU; a.value.u32 = 9100; t.call(pa->set_port_attribute(p, &a)); } t.done(); }
        {
            sai_stat_id_t ids[4] = {SAI_PORT_STAT_IF_IN_OCTETS, SAI_PORT_STAT_IF_IN_UCAST_PKTS, SAI_PORT_STAT_IF_OUT_OCTETS, SAI_PORT_STAT_IF_OUT_UCAST_PKTS};
            uint64_t v[4];
            Timer t("port get_stats x4"); for (auto p : ports) t.call(pa->get_port_stats(p, 4, ids, v)); t.done();
        }
        sai_object_id_t vr = 0;
        { Timer t("create vrf"); t.call(va->create_virtual_router(&vr, swid, 0, nullptr)); t.done(); }
        if (vr) {
            std::vector<sai_route_entry_t> re(1000);
            for (int i = 0; i < 1000; i++) {
                re[i] = {}; re[i].switch_id = swid; re[i].vr_id = vr;
                re[i].destination.addr_family = SAI_IP_ADDR_FAMILY_IPV4;
                re[i].destination.addr.ip4 = htonl(0x0a000000u | ((uint32_t)i << 8));
                re[i].destination.mask.ip4 = htonl(0xffffff00u);
            }
            a = {}; a.id = SAI_ROUTE_ENTRY_ATTR_PACKET_ACTION; a.value.s32 = SAI_PACKET_ACTION_DROP;
            { Timer t("route create (drop)"); for (auto& e : re) t.call(ra->create_route_entry(&e, 1, &a)); t.done(); }
            { Timer t("route remove"); for (auto& e : re) t.call(ra->remove_route_entry(&e)); t.done(); }
            { Timer t("remove vrf"); t.call(va->remove_virtual_router(vr)); t.done(); }
        }
        sai_object_id_t vlan = 0, bp = 0, vm = 0; bool own_bp = false;
        a = {}; a.id = SAI_VLAN_ATTR_VLAN_ID; a.value.u16 = 100;
        { Timer t("create vlan"); t.call(vl->create_vlan(&vlan, swid, 1, &a)); t.done(); }
        a = {}; a.id = SAI_SWITCH_ATTR_DEFAULT_1Q_BRIDGE_ID;
        if (sw->get_switch_attribute(swid, 1, &a) == SAI_STATUS_SUCCESS && a.value.oid) {
            sai_object_id_t b = a.value.oid; std::vector<sai_object_id_t> bl(600);
            a = {}; a.id = SAI_BRIDGE_ATTR_PORT_LIST; a.value.objlist.count = bl.size(); a.value.objlist.list = bl.data();
            if (br->get_bridge_attribute(b, 1, &a) == SAI_STATUS_SUCCESS) {
                for (uint32_t i = 0; i < a.value.objlist.count && !bp; i++) {
                    sai_attribute_t q[2] = {}; q[0].id = SAI_BRIDGE_PORT_ATTR_TYPE; q[1].id = SAI_BRIDGE_PORT_ATTR_PORT_ID;
                    if (br->get_bridge_port_attribute(bl[i], 2, q) == SAI_STATUS_SUCCESS && q[0].value.s32 == SAI_BRIDGE_PORT_TYPE_PORT && q[1].value.oid == ports[0]) bp = bl[i];
                }
            }
            printf("default 1Q bridge: %u bridge ports, port 0's %s\n", a.value.objlist.count, bp ? "found" : "not found");
        }
        if (!bp && !ports.empty()) {
            sai_attribute_t q[3] = {};
            q[0].id = SAI_BRIDGE_PORT_ATTR_TYPE; q[0].value.s32 = SAI_BRIDGE_PORT_TYPE_PORT;
            q[1].id = SAI_BRIDGE_PORT_ATTR_PORT_ID; q[1].value.oid = ports[0];
            q[2].id = SAI_BRIDGE_PORT_ATTR_ADMIN_STATE; q[2].value.booldata = true;
            Timer t("create bridge port"); t.call(br->create_bridge_port(&bp, swid, 3, q)); t.done(); own_bp = bp != 0;
        }
        if (vlan && bp) {
            sai_attribute_t q[3] = {};
            q[0].id = SAI_VLAN_MEMBER_ATTR_VLAN_ID; q[0].value.oid = vlan;
            q[1].id = SAI_VLAN_MEMBER_ATTR_BRIDGE_PORT_ID; q[1].value.oid = bp;
            q[2].id = SAI_VLAN_MEMBER_ATTR_VLAN_TAGGING_MODE; q[2].value.s32 = SAI_VLAN_TAGGING_MODE_TAGGED;
            { Timer t("create vlan member"); t.call(vl->create_vlan_member(&vm, swid, 3, q)); t.done(); }
            std::vector<sai_fdb_entry_t> fe(100);
            sai_attribute_t f[3] = {};
            f[0].id = SAI_FDB_ENTRY_ATTR_TYPE; f[0].value.s32 = SAI_FDB_ENTRY_TYPE_STATIC;
            f[1].id = SAI_FDB_ENTRY_ATTR_BRIDGE_PORT_ID; f[1].value.oid = bp;
            f[2].id = SAI_FDB_ENTRY_ATTR_PACKET_ACTION; f[2].value.s32 = SAI_PACKET_ACTION_FORWARD;
            for (int i = 0; i < 100; i++) {
                fe[i] = {}; fe[i].switch_id = swid; fe[i].bv_id = vlan;
                const uint8_t m[6] = {0x02, 0, 0, 0, (uint8_t)(i >> 8), (uint8_t)i}; memcpy(fe[i].mac_address, m, 6);
            }
            { Timer t("fdb create (static)"); for (auto& e : fe) t.call(fa->create_fdb_entry(&e, 3, f)); t.done(); }
            { Timer t("fdb remove"); for (auto& e : fe) t.call(fa->remove_fdb_entry(&e)); t.done(); }
            if (vm) { Timer t("remove vlan member"); t.call(vl->remove_vlan_member(vm)); t.done(); }
        }
        if (own_bp) { Timer t("remove bridge port"); t.call(br->remove_bridge_port(bp)); t.done(); }
        if (vlan) { Timer t("remove vlan"); t.call(vl->remove_vlan(vlan)); t.done(); }
    }

    if (link_s > 0) {
        std::vector<std::pair<uint32_t, sai_object_id_t>> want;
        for (auto l : lanes) { if (by_lane.count(l)) want.push_back({l, by_lane[l]}); else printf("no port with first lane %u\n", l); }
        if (!saiserdes.empty()) {
            auto cmds = read_cmds(saiserdes);
            for (auto& w : want) {
                uint32_t dp = devport_of.count(w.first) ? devport_of[w.first] : 0;
                std::vector<sai_attribute_t> sa; std::vector<std::vector<int32_t>> keep;
                sai_attribute_t x = {}; x.id = SAI_PORT_SERDES_ATTR_PORT_ID; x.value.oid = w.second; sa.push_back(x);
                const std::map<std::string, sai_attr_id_t> m = {{"tx_eq_pre1", SAI_PORT_SERDES_ATTR_TX_FIR_PRE1}, {"tx_eq_pre2", SAI_PORT_SERDES_ATTR_TX_FIR_PRE2},
                    {"tx_eq_pre3", SAI_PORT_SERDES_ATTR_TX_FIR_PRE3}, {"tx_eq_attn", SAI_PORT_SERDES_ATTR_TX_FIR_ATTN},
                    {"tx_eq_post", SAI_PORT_SERDES_ATTR_TX_FIR_POST1}, {"tx_eq_main", SAI_PORT_SERDES_ATTR_TX_FIR_MAIN}};
                keep.reserve(16);
                for (auto& c : cmds) {
                    if (c.devport != dp || !m.count(c.attr)) continue;
                    keep.emplace_back(c.v.begin(), c.v.end());
                    x = {}; x.id = m.at(c.attr); x.value.s32list.count = keep.back().size(); x.value.s32list.list = keep.back().data(); sa.push_back(x);
                    printf("lane %u devport %u %s", w.first, dp, c.attr.c_str()); for (auto v : c.v) printf(" %ld", v); printf("\n");
                }
                sai_object_id_t ps = 0;
                s = pa->create_port_serdes(&ps, swid, sa.size(), sa.data());
                printf("create_port_serdes lane %u (%zu attrs): %d\n", w.first, sa.size() - 1, (int)s);
            }
        }
        if (!ifcscmds.empty()) {
            std::vector<Cmd> cmds; std::stringstream s2(ifcscmds); std::string f;
            while (std::getline(s2, f, ':')) { auto c = read_cmds(f); cmds.insert(cmds.end(), c.begin(), c.end()); }
            std::vector<Cmd> mine;
            for (auto& c : cmds) for (auto& w : want) if (devport_of.count(w.first) && devport_of[w.first] == c.devport) mine.push_back(c);
            apply_ifcs016(mine);
        }
        for (auto& w : want) {
            if (fec != "keep") {
                a = {}; a.id = SAI_PORT_ATTR_FEC_MODE; a.value.s32 = fec == "rs" ? SAI_PORT_FEC_MODE_RS : SAI_PORT_FEC_MODE_NONE;
                printf("lane %u fec %s: %d\n", w.first, fec.c_str(), (int)pa->set_port_attribute(w.second, &a));
            }
            if (lt) { a = {}; a.id = SAI_PORT_ATTR_LINK_TRAINING_ENABLE; a.value.booldata = true; printf("lane %u link training: %d\n", w.first, (int)pa->set_port_attribute(w.second, &a)); }
            a = {}; a.id = SAI_PORT_ATTR_MTU; a.value.u32 = 9100; pa->set_port_attribute(w.second, &a);
            a = {}; a.id = SAI_PORT_ATTR_ADMIN_STATE; a.value.booldata = true;
            printf("lane %u admin up: %d\n", w.first, (int)pa->set_port_attribute(w.second, &a));
        }
        double tu = now_ms();
        for (int t = 0; t < link_s && !g_stop; t += 2) {
            a = {}; a.id = SAI_SWITCH_ATTR_MAX_TEMP; sw->get_switch_attribute(swid, 1, &a);
            printf("LINK t=%d temp %d C", t, (int)a.value.s32);
            for (auto& w : want) {
                sai_attribute_t o = {}; o.id = SAI_PORT_ATTR_OPER_STATUS; pa->get_port_attribute(w.second, 1, &o);
                sai_stat_id_t ids[4] = {SAI_PORT_STAT_IF_IN_FEC_CORRECTABLE_FRAMES, SAI_PORT_STAT_IF_IN_FEC_NOT_CORRECTABLE_FRAMES,
                                        SAI_PORT_STAT_IF_IN_FEC_SYMBOL_ERRORS, SAI_PORT_STAT_IF_IN_ERRORS};
                uint64_t v[4] = {}; sai_status_t ss = pa->get_port_stats(w.second, 4, ids, v);
                printf(" | lane %u oper %d fec(%d) corr %lu uncorr %lu sym %lu inerr %lu", w.first, (int)o.value.s32, (int)ss,
                       (unsigned long)v[0], (unsigned long)v[1], (unsigned long)v[2], (unsigned long)v[3]);
                if (o.value.s32 == SAI_PORT_OPER_STATUS_UP) printf(" (up after %.1f s)", (now_ms() - tu) / 1e3);
            }
            printf("\n"); fflush(stdout);
            for (int i = 0; i < 20 && !g_stop; i++) usleep(100000);
        }
        for (auto& w : want) { a = {}; a.id = SAI_PORT_ATTR_ADMIN_STATE; a.value.booldata = false; pa->set_port_attribute(w.second, &a); }
    }

    t0 = now_ms();
    s = sw->remove_switch(swid);
    t1 = now_ms();
    printf("BENCH %-22s %5d %10.2f %10.1f %d\n", "remove_switch", 1, t1 - t0, (t1 - t0) * 1e3, s != 0);
    s = sai_api_uninitialize();
    printf("sai_api_uninitialize %d\n", (int)s);
    return 0;
}
