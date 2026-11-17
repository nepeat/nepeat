// SPDX-License-Identifier: Apache-2.0
// Shared harness of the sai-teralynx end-to-end test (tests/test_ifcs.cpp
// runs the areas in order on one switch; tests/t_<area>.cpp hold them, like
// src/tl_<area>.cpp): check macros, SAI API tables and the objects areas
// hand to each other (Env), notifications, helpers and the mock hooks.
#pragma once

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

extern int g_fail, g_checks;
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

// The switch under test: API tables, the switch, and what one area creates
// for a later one (ports from the port test, the VLAN and bridge port from L2).
struct Env {
    sai_switch_api_t* sw = nullptr;
    sai_port_api_t* pa = nullptr;
    sai_hostif_api_t* ha = nullptr;
    sai_vlan_api_t* va = nullptr;
    sai_bridge_api_t* ba = nullptr;
    sai_route_api_t* rta = nullptr;
    sai_router_interface_api_t* ra = nullptr;
    sai_neighbor_api_t* na = nullptr;
    sai_next_hop_api_t* nha = nullptr;
    sai_fdb_api_t* fa = nullptr;
    sai_lag_api_t* la = nullptr;
    sai_policer_api_t* pola = nullptr;
    sai_acl_api_t* acla = nullptr;
    sai_object_id_t s = 0, p1 = 0, p2 = 0, p3 = 0, hif = 0, vlan = 0, bp1 = 0;
    std::string dir, init, isg, run;   // scratch dir and the files main() wrote
};
// Names the area bodies use (as in the original single-file test).
#define USE_ENV(e)                                                                              \
    auto* sw = (e).sw; auto* pa = (e).pa; auto* ha = (e).ha; auto* va = (e).va; auto* ba = (e).ba; \
    auto* rta = (e).rta; auto* ra = (e).ra; auto* na = (e).na; auto* nha = (e).nha; auto* fa = (e).fa; \
    auto* la = (e).la;                                                                          \
    sai_object_id_t &s = (e).s, &p1 = (e).p1, &p2 = (e).p2, &p3 = (e).p3, &hif = (e).hif,       \
                    &vlan = (e).vlan, &bp1 = (e).bp1;                                           \
    (void)sw; (void)pa; (void)ha; (void)va; (void)ba; (void)rta; (void)ra; (void)na; (void)nha;   \
    (void)fa; (void)la; (void)s; (void)p1; (void)p2; (void)p3; (void)hif; (void)vlan; (void)bp1

// notifications (test_ifcs.cpp)
extern std::mutex g_mu;
extern std::vector<sai_port_oper_status_notification_t> g_port_events;
extern std::vector<std::pair<sai_fdb_event_t, sai_fdb_entry_t>> g_fdb_events;

inline bool wait_for(std::function<bool()> f)
{
    for (int i = 0; i < 300; i++) {
        { std::lock_guard<std::mutex> g(g_mu); if (f()) return true; }
        std::this_thread::sleep_for(std::chrono::milliseconds(10));
    }
    return false;
}
inline void put(const std::string& path, const std::string& s)
{
    std::ofstream f(path);
    f << s;
}
inline std::string get(const std::string& path)
{
    std::ifstream f(path);
    std::stringstream s;
    s << f.rdbuf();
    return s.str();
}
inline std::vector<std::string> lines(const std::string& s)
{
    std::vector<std::string> v;
    std::istringstream is(s);
    std::string l;
    while (std::getline(is, l)) v.push_back(l);
    return v;
}
inline bool has(const std::string& log, const std::string& needle) { return log.find(needle) != std::string::npos; }

// mock hooks (resolved in test_ifcs.cpp after create_switch)
extern void* g_mock;
extern const char* (*m_log)();
extern void (*m_log_clear)();
extern uint32_t (*m_admin)(uint32_t);
extern int (*m_fire_link)(uint32_t, int);
extern int (*m_fire_l2)(int, const uint8_t*, uint16_t, uint32_t);
extern void (*m_set_stat)(uint32_t, uint32_t, uint64_t);
extern int (*m_route_find)(const char*, int*, int*, uint32_t*);
extern int (*m_route_count)();
extern int (*m_l2_count)();
extern int (*m_inited)();
extern uint32_t (*m_fec)(uint32_t);
extern uint32_t (*m_aging)();

// Resolve an optional mock hook of an area test (exits when missing).
template <typename T> void hook(void* h, T& fn, const char* name)
{
    fn = reinterpret_cast<T>(dlsym(h, name));
    if (!fn) {
        std::fprintf(stderr, "mock hook %s missing\n", name);
        std::exit(2);
    }
}

// The areas, in the order main() runs them.
void test_switch_init(Env& e);      // t_switch.cpp: init call sequence, init commands, L2 base
void test_ports(Env& e);            // t_port.cpp
void test_hostif(Env& e);           // t_hostif.cpp
void test_l2(Env& e);               // t_l2.cpp
void test_l3(Env& e);               // t_l3.cpp
void test_lag(Env& e);              // t_lag.cpp
void test_copp(Env& e);
void test_qos(Env& e);             // t_qos.cpp: QoS maps, schedulers, WRED, buffers, queue / PG counters
void test_sflow(Env& e);           // t_sflow.cpp: genetlink hostif / SAMPLEPACKET table entry -> ipd sFlow netlink
void test_mirror(Env& e);          // t_mirror.cpp: SPAN, ERSPAN, SAMPLEPACKET             // t_copp.cpp: hostif traps, trap groups, policers (CoPP)
void test_acl(Env& e);              // t_acl.cpp: SAI ACL tables, entries, counters (fp groups)
void test_counters(Env& e);         // t_counters.cpp: port counters by SAI id, CPU queues, diag shell
void test_hooks(Env& e);            // t_hooks.cpp: RIF counters / admin, glean, hash, flood + storm control, ACL extras
void test_learn_guard(Env& e);      // t_learn.cpp: the shim's learn-ring read guard (ifcs_guard.c)
void test_switch_misc(Env& e);      // t_switch.cpp: temperatures, table usage
void test_switch_remove(Env& e);    // t_switch.cpp: remove_switch order
