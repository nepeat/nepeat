// SPDX-License-Identifier: Apache-2.0
// Section accounting for sai-teralynx's test_ifcs (crate sai-teralynx-parity, README there).
// test_ifcs.cpp and t_*.cpp are compiled UNMODIFIED; this unit is linked next to them with
// -Wl,--wrap=<area function> for every area main() calls (the list below), so main()'s calls land
// here, and each wrapper calls the real area. Per area it records the checks run and failed
// (harness.h's g_checks / g_fail) into PARITY_COUNTS_FILE and marks the start and end of the area in
// the tap's stream (tap.c: parity_tap_mark, "@@ begin <area>" / "@@ end <area>"). What main() does
// before the first area (the refusals and create_switch) is the section "main", what it does after
// the last one (sai_api_uninitialize) "uninit". A run that dies inside an area (std::terminate,
// SIGSEGV, ...) still leaves that area's counts so far, marked "incomplete".
//   counts file lines: "section <name> checks <n> failed <n>[ incomplete]"
#include "harness.h"

#include <csignal>
#include <fcntl.h>

extern "C" void parity_tap_mark(const char* text) __attribute__((weak));

namespace {

int g_counts_fd = -1;
const char* g_current = "main";
int g_c0 = 0, g_f0 = 0;
bool g_main_done = false;

void put_counts(const char* name, int checks, int failed, bool incomplete)
{
    if (g_counts_fd < 0) return;
    char b[160];
    int n = std::snprintf(b, sizeof(b), "section %s checks %d failed %d%s\n", name, checks, failed,
                          incomplete ? " incomplete" : "");
    if (n > 0) (void)!write(g_counts_fd, b, (size_t)n);
}

void mark(const char* what, const char* name)
{
    if (!parity_tap_mark) return;
    char b[160];
    std::snprintf(b, sizeof(b), "%s %s", what, name);
    parity_tap_mark(b);
}

void on_fatal(int sig)
{
    put_counts(g_current, g_checks - g_c0, g_fail - g_f0, true);
    mark("died-in", g_current);
    std::signal(sig, SIG_DFL);
    std::raise(sig);
}

void at_exit()
{
    // after the last area: sai_api_uninitialize's check (or main() returning early)
    put_counts(g_current, g_checks - g_c0, g_fail - g_f0, false);
    mark("end", g_current);
}

void start()
{
    if (g_main_done) return;
    g_main_done = true;
    const char* f = std::getenv("PARITY_COUNTS_FILE");
    if (f && *f) g_counts_fd = open(f, O_WRONLY | O_CREAT | O_TRUNC | O_CLOEXEC, 0644);
    for (int s : {SIGSEGV, SIGBUS, SIGABRT, SIGFPE, SIGILL}) std::signal(s, on_fatal);
    // main()'s part so far: the refusals and create_switch
    put_counts("main", g_checks, g_fail, false);
    mark("end", "main");
    std::atexit(at_exit);
}

void enter(const char* name)
{
    start();
    g_current = name;
    g_c0 = g_checks;
    g_f0 = g_fail;
    mark("begin", name);
}

void leave(const char* name)
{
    put_counts(name, g_checks - g_c0, g_fail - g_f0, false);
    mark("end", name);
    g_current = "uninit";   // what main() does after the last area (the parser: lines after an end)
    g_c0 = g_checks;
    g_f0 = g_fail;
}

} // namespace

#define W(sym, name)                                     \
    extern "C" void __real_##sym(Env& e);                \
    extern "C" void __wrap_##sym(Env& e)                 \
    {                                                    \
        enter(name);                                     \
        __real_##sym(e);                                 \
        leave(name);                                     \
    }
#define AREAS \
    W(_Z16test_switch_initR3Env, "switch_init") \
    W(_Z10test_portsR3Env, "ports") \
    W(_Z11test_hostifR3Env, "hostif") \
    W(_Z7test_l2R3Env, "l2") \
    W(_Z7test_l3R3Env, "l3") \
    W(_Z8test_lagR3Env, "lag") \
    W(_Z9test_coppR3Env, "copp") \
    W(_Z8test_aclR3Env, "acl") \
    W(_Z8test_qosR3Env, "qos") \
    W(_Z11test_mirrorR3Env, "mirror") \
    W(_Z10test_sflowR3Env, "sflow") \
    W(_Z13test_countersR3Env, "counters") \
    W(_Z10test_hooksR3Env, "hooks") \
    W(_Z16test_learn_guardR3Env, "learn_guard") \
    W(_Z16test_switch_miscR3Env, "switch_misc") \
    W(_Z18test_switch_removeR3Env, "switch_remove")
AREAS
