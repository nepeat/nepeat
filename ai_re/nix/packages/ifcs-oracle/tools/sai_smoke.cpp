// SPDX-License-Identifier: Apache-2.0
// sai-teralynx-smoke: drive this libsai directly on the switch (no redis,
// syncd or orchagent), for the first runs on the chip.
//   sai-teralynx-smoke PROFILE SECONDS [FIRST_LANE[:IFNAME] ...]
// PROFILE: key=value lines (the sai.profile syncd would get). Brings the
// ports whose first lane is FIRST_LANE admin up (MTU 9100), optionally with a
// host interface IFNAME, then prints oper state, link events, counters and
// the ASIC temperature every 2 s for SECONDS (SIGINT/SIGTERM end it early),
// then admin down, hostifs removed, remove_switch, uninitialize.
extern "C" {
#include <sai.h>
}
#include <atomic>
#include <csignal>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <fstream>
#include <map>
#include <string>
#include <unistd.h>
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

#define CHECK(what, st) do { sai_status_t s_ = (st); printf("%-28s %d\n", what, (int)s_); fflush(stdout); \
    if (s_ != SAI_STATUS_SUCCESS) { fail = true; } } while (0)

int main(int argc, char** argv) {
    if (argc < 3) { fprintf(stderr, "usage: %s PROFILE SECONDS [FIRST_LANE[:IFNAME] ...]\n", argv[0]); return 2; }
    std::ifstream pf(argv[1]);
    for (std::string l; std::getline(pf, l);) {
        auto eq = l.find('=');
        if (l.empty() || l[0] == '#' || eq == std::string::npos) continue;
        g_profile[l.substr(0, eq)] = l.substr(eq + 1);
    }
    int seconds = atoi(argv[2]);
    struct Want { uint32_t lane; std::string ifname; sai_object_id_t port = 0, hostif = 0; };
    std::vector<Want> want;
    for (int i = 3; i < argc; i++) {
        std::string a = argv[i]; auto c = a.find(':');
        want.push_back({(uint32_t)strtoul(a.substr(0, c).c_str(), nullptr, 0), c == std::string::npos ? "" : a.substr(c + 1)});
    }
    signal(SIGINT, on_signal); signal(SIGTERM, on_signal);
    bool fail = false;

    sai_service_method_table_t svc = {prof_get, prof_next};
    CHECK("sai_api_initialize", sai_api_initialize(0, &svc));
    sai_switch_api_t* sw = nullptr; sai_port_api_t* pa = nullptr; sai_hostif_api_t* ha = nullptr;
    CHECK("query switch", sai_api_query(SAI_API_SWITCH, (void**)&sw));
    CHECK("query port", sai_api_query(SAI_API_PORT, (void**)&pa));
    CHECK("query hostif", sai_api_query(SAI_API_HOSTIF, (void**)&ha));
    if (fail) return 1;

    sai_attribute_t at[4] = {};
    at[0].id = SAI_SWITCH_ATTR_INIT_SWITCH; at[0].value.booldata = true;
    at[1].id = SAI_SWITCH_ATTR_SRC_MAC_ADDRESS;
    const uint8_t mac[6] = {0x0e, 0x48, 0xc6, 0x7a, 0x55, 0xce};
    memcpy(at[1].value.mac, mac, 6);
    at[2].id = SAI_SWITCH_ATTR_PORT_STATE_CHANGE_NOTIFY; at[2].value.ptr = (sai_pointer_t)on_port_state;
    at[3].id = SAI_SWITCH_ATTR_SWITCH_SHUTDOWN_REQUEST_NOTIFY; at[3].value.ptr = (sai_pointer_t)on_shutdown;
    sai_object_id_t swid = 0;
    CHECK("create_switch", sw->create_switch(&swid, 4, at));
    if (fail) { sai_api_uninitialize(); return 1; }

    sai_attribute_t a = {};
    std::vector<sai_object_id_t> ports(512);
    a.id = SAI_SWITCH_ATTR_PORT_LIST; a.value.objlist.count = ports.size(); a.value.objlist.list = ports.data();
    CHECK("get port list", sw->get_switch_attribute(swid, 1, &a));
    ports.resize(a.value.objlist.count);
    printf("ports %zu\n", ports.size());
    for (auto p : ports) {
        uint32_t lanes[8] = {}; sai_attribute_t q[2] = {};
        q[0].id = SAI_PORT_ATTR_HW_LANE_LIST; q[0].value.u32list.count = 8; q[0].value.u32list.list = lanes;
        q[1].id = SAI_PORT_ATTR_SPEED;
        sai_status_t st = pa->get_port_attribute(p, 2, q);
        printf("port 0x%lx st %d speed %u lanes", (unsigned long)p, (int)st, q[1].value.u32);
        for (uint32_t i = 0; i < q[0].value.u32list.count && i < 8; i++) printf(" %u", lanes[i]);
        printf("\n");
        for (auto& w : want) if (st == SAI_STATUS_SUCCESS && q[0].value.u32list.count && lanes[0] == w.lane) w.port = p;
    }
    for (auto& w : want) {
        if (!w.port) { printf("no port with first lane %u\n", w.lane); fail = true; continue; }
        a = {}; a.id = SAI_PORT_ATTR_MTU; a.value.u32 = 9100;
        CHECK("set mtu 9100", pa->set_port_attribute(w.port, &a));
        if (!w.ifname.empty()) {
            sai_attribute_t h[3] = {};
            h[0].id = SAI_HOSTIF_ATTR_TYPE; h[0].value.s32 = SAI_HOSTIF_TYPE_NETDEV;
            h[1].id = SAI_HOSTIF_ATTR_OBJ_ID; h[1].value.oid = w.port;
            h[2].id = SAI_HOSTIF_ATTR_NAME; strncpy(h[2].value.chardata, w.ifname.c_str(), SAI_HOSTIF_NAME_SIZE - 1);
            CHECK(("hostif " + w.ifname).c_str(), ha->create_hostif(&w.hostif, swid, 3, h));
        }
        a = {}; a.id = SAI_PORT_ATTR_ADMIN_STATE; a.value.booldata = true;
        CHECK("admin up", pa->set_port_attribute(w.port, &a));
    }
    for (int t = 0; t < seconds && !g_stop; t += 2) {
        a = {}; a.id = SAI_SWITCH_ATTR_MAX_TEMP;
        sai_status_t ts = sw->get_switch_attribute(swid, 1, &a);
        printf("t=%d temp st %d %d C", t, (int)ts, (int)a.value.s32);
        for (auto& w : want) {
            if (!w.port) continue;
            sai_attribute_t o = {}; o.id = SAI_PORT_ATTR_OPER_STATUS;
            pa->get_port_attribute(w.port, 1, &o);
            sai_stat_id_t ids[4] = {SAI_PORT_STAT_IF_IN_UCAST_PKTS, SAI_PORT_STAT_IF_OUT_UCAST_PKTS,
                                    SAI_PORT_STAT_IF_IN_ERRORS, SAI_PORT_STAT_IF_IN_NON_UCAST_PKTS};
            uint64_t v[4] = {};
            sai_status_t ss = pa->get_port_stats(w.port, 4, ids, v);
            printf(" | lane %u oper %d stats(%d) in %lu out %lu err %lu nonuc %lu", w.lane, (int)o.value.s32, (int)ss,
                   (unsigned long)v[0], (unsigned long)v[1], (unsigned long)v[2], (unsigned long)v[3]);
        }
        printf("\n"); fflush(stdout);
        for (int i = 0; i < 20 && !g_stop; i++) usleep(100000);
    }
    for (auto& w : want) {
        if (!w.port) continue;
        a = {}; a.id = SAI_PORT_ATTR_ADMIN_STATE; a.value.booldata = false;
        CHECK("admin down", pa->set_port_attribute(w.port, &a));
        if (w.hostif) CHECK("remove hostif", ha->remove_hostif(w.hostif));
    }
    CHECK("remove_switch", sw->remove_switch(swid));
    CHECK("sai_api_uninitialize", sai_api_uninitialize());
    return fail ? 1 : 0;
}
