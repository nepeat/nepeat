// SPDX-License-Identifier: Apache-2.0
// section_trace: per-section check counts of an unmodified test_sai, by ptrace (x86_64 Linux).
//
//   section_trace [--timeout SEC] --syms FILE --checks ADDR --fails ADDR --events OUT -- PROG ARGS...
//
// FILE: "<hex address> <name>" per line, the entry of every section function (nm of PROG; for a
// PIE the load base is added). ADDR: the int counters g_checks / g_fail (nm addresses).
// A breakpoint at each entry; on a hit the return address gets one too, so every check is counted
// in the innermost running section (exclusive counts, nested sections work). Events written to OUT
// (tab separated, counters read from the tracee at that moment):
//   enter NAME CHECKS FAILS | leave NAME CHECKS FAILS
//   signal SIGNO CHECKS FAILS RIP WHERE     a fatal signal reached the test thread (delivered)
//   fail PC CHECKS FAILS                    g_fail was written by the instruction at PC (a hardware
//                                           write watchpoint, x86 DR0: every failure's own site, also
//                                           while the suite has fd 2 redirected; absent if the debug
//                                           registers cannot be set: "nowatch" is written instead)
//   exit WAITSTATUS CHECKS FAILS            the test thread is exiting (PTRACE_EVENT_EXIT)
//   timeout SEC                             killed after SEC seconds
// The last two fields of every line are the counters (-1: unreadable).
// At each enter / leave the line "@@tp enter|leave NAME" goes to this program's stderr, which the
// test inherits: test stderr lines then sit between the markers of the section that printed them.
// Only the test's main thread is traced (the library's threads run free; none runs test code).
// Exit status: the test's (128 + signal if killed); 3: could not trace (ptrace refused, not
// x86_64, a breakpoint could not be set): nothing ran, the caller runs the test untraced; 2: usage.
#include <cerrno>
#include <csignal>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <fstream>
#include <map>
#include <sstream>
#include <string>
#include <vector>

#include <fcntl.h>
#include <sys/wait.h>
#include <unistd.h>

#if defined(__x86_64__) && defined(__linux__)
#include <cstddef>
#include <sys/ptrace.h>
#include <sys/user.h>
#define HAVE_TRACE 1
#endif

namespace {

constexpr int CANNOT_TRACE = 3;

[[noreturn]] void usage()
{
    std::fprintf(stderr, "usage: section_trace [--timeout SEC] --syms FILE --checks ADDR --fails ADDR --events OUT -- PROG ARGS...\n");
    std::exit(2);
}

bool parse_hex(const std::string& s, uint64_t& out)
{
    if (s.empty()) return false;
    char* end = nullptr;
    errno = 0;
    unsigned long long v = std::strtoull(s.c_str(), &end, 16);
    if (errno || !end || *end) return false;
    out = v;
    return true;
}

} // namespace

#ifndef HAVE_TRACE
int main()
{
    std::fprintf(stderr, "section_trace: x86_64 Linux only\n");
    return CANNOT_TRACE;
}
#else

namespace {

volatile sig_atomic_t g_alarm = 0;
void on_alarm(int) { g_alarm = 1; }

struct Tracee {
    pid_t pid = -1;
    FILE* ev = nullptr;

    bool peek(uint64_t addr, uint64_t& word) const
    {
        errno = 0;
        long v = ptrace(PTRACE_PEEKDATA, pid, (void*)addr, nullptr);
        if (errno) return false;
        word = (uint64_t)v;
        return true;
    }
    bool poke(uint64_t addr, uint64_t word) const { return ptrace(PTRACE_POKEDATA, pid, (void*)addr, (void*)word) == 0; }
    int32_t counter(uint64_t addr) const
    {
        uint64_t w = 0;
        return peek(addr, w) ? (int32_t)(uint32_t)(w & 0xffffffffu) : -1;
    }
};

// x86 debug registers of the traced thread (struct user's u_debugreg).
bool set_dr(pid_t pid, int i, uint64_t v)
{
    return ptrace(PTRACE_POKEUSER, pid, (void*)(offsetof(struct user, u_debugreg) + i * sizeof(long)), (void*)v) == 0;
}
bool get_dr(pid_t pid, int i, uint64_t& v)
{
    errno = 0;
    long r = ptrace(PTRACE_PEEKUSER, pid, (void*)(offsetof(struct user, u_debugreg) + i * sizeof(long)), nullptr);
    if (errno) return false;
    v = (uint64_t)r;
    return true;
}

struct Bp {
    uint64_t orig = 0;   // the word before the int3 went in
    int refs = 0;        // return breakpoints: frames waiting on it; entries: 1 (permanent)
    bool entry = false;
    std::string name;
};

struct Frame {
    std::string name;
    uint64_t ret, sp;   // return address, rsp after the return
};

// "path+0xoffset" of an address, from /proc/PID/maps.
std::string where(pid_t pid, uint64_t addr)
{
    std::ifstream m("/proc/" + std::to_string(pid) + "/maps");
    std::string line;
    while (std::getline(m, line)) {
        std::istringstream is(line);
        std::string range, perms, off, dev, inode, path;
        is >> range >> perms >> off >> dev >> inode;
        std::getline(is, path);
        size_t p0 = path.find_first_not_of(' ');
        path = p0 == std::string::npos ? "" : path.substr(p0);
        size_t dash = range.find('-');
        uint64_t lo = 0, hi = 0, o = 0;
        if (dash == std::string::npos || !parse_hex(range.substr(0, dash), lo) || !parse_hex(range.substr(dash + 1), hi) ||
            !parse_hex(off, o))
            continue;
        if (addr >= lo && addr < hi) {
            char b[64];
            std::snprintf(b, sizeof b, "+0x%llx", (unsigned long long)(addr - lo + o));
            return (path.empty() ? "[anon]" : path) + b;
        }
    }
    return "?";
}

// The load base of a PIE (0 for ET_EXEC).
bool load_base(pid_t pid, uint64_t& base)
{
    char exe[4096];
    ssize_t n = readlink(("/proc/" + std::to_string(pid) + "/exe").c_str(), exe, sizeof exe - 1);
    if (n <= 0) return false;
    exe[n] = 0;
    int fd = open(exe, O_RDONLY | O_CLOEXEC);
    if (fd < 0) return false;
    unsigned char h[18];
    bool ok = read(fd, h, sizeof h) == (ssize_t)sizeof h;
    close(fd);
    if (!ok || std::memcmp(h, "\x7f" "ELF", 4) != 0) return false;
    unsigned type = h[16] | (h[17] << 8);
    base = 0;
    if (type == 2) return true;   // ET_EXEC: nm addresses are absolute
    std::ifstream m("/proc/" + std::to_string(pid) + "/maps");
    std::string line;
    while (std::getline(m, line)) {
        std::istringstream is(line);
        std::string range, perms, off, dev, inode, path;
        is >> range >> perms >> off >> dev >> inode >> path;
        uint64_t o = 1;
        if (path == exe && parse_hex(off, o) && o == 0) return parse_hex(range.substr(0, range.find('-')), base);
    }
    return false;
}

} // namespace

int main(int argc, char** argv)
{
    std::string syms, events;
    uint64_t a_checks = 0, a_fails = 0;
    unsigned timeout = 0;
    int i = 1;
    for (; i < argc; i++) {
        std::string a = argv[i];
        if (a == "--") { i++; break; }
        if (i + 1 >= argc) usage();
        std::string v = argv[++i];
        if (a == "--syms") syms = v;
        else if (a == "--events") events = v;
        else if (a == "--checks") { if (!parse_hex(v, a_checks)) usage(); }
        else if (a == "--fails") { if (!parse_hex(v, a_fails)) usage(); }
        else if (a == "--timeout") timeout = (unsigned)std::strtoul(v.c_str(), nullptr, 10);
        else usage();
    }
    if (i >= argc || syms.empty() || events.empty() || !a_checks || !a_fails) usage();

    std::vector<std::pair<uint64_t, std::string>> entries;
    {
        std::ifstream f(syms);
        std::string line;
        while (std::getline(f, line)) {
            std::istringstream is(line);
            std::string addr, name;
            uint64_t a = 0;
            if (!(is >> addr >> name) || !parse_hex(addr, a)) continue;
            entries.emplace_back(a, name);
        }
        if (entries.empty()) {
            std::fprintf(stderr, "section_trace: no symbols in %s\n", syms.c_str());
            return 2;
        }
    }

    int pfd[2];
    if (pipe2(pfd, O_CLOEXEC) != 0) return CANNOT_TRACE;
    pid_t pid = fork();
    if (pid < 0) return CANNOT_TRACE;
    if (pid == 0) {
        close(pfd[0]);
        int what = 1;
        if (ptrace(PTRACE_TRACEME, 0, nullptr, nullptr) != 0) {
            int e[2] = {what, errno};
            (void)!write(pfd[1], e, sizeof e);
            _exit(126);
        }
        execvp(argv[i], argv + i);
        int e[2] = {2, errno};
        (void)!write(pfd[1], e, sizeof e);
        _exit(127);
    }
    close(pfd[1]);
    int err[2] = {0, 0};
    ssize_t got = read(pfd[0], err, sizeof err);
    close(pfd[0]);
    if (got == (ssize_t)sizeof err) {
        waitpid(pid, nullptr, 0);
        std::fprintf(stderr, "section_trace: %s: %s\n", err[0] == 1 ? "PTRACE_TRACEME" : argv[i], std::strerror(err[1]));
        return err[0] == 1 ? CANNOT_TRACE : 127;
    }
    int st = 0;
    if (waitpid(pid, &st, 0) != pid || !WIFSTOPPED(st) || WSTOPSIG(st) != SIGTRAP) {
        std::fprintf(stderr, "section_trace: no exec stop (status 0x%x)\n", st);
        kill(pid, SIGKILL);
        return CANNOT_TRACE;
    }
    Tracee t;
    t.pid = pid;
    auto give_up = [&](const char* why) {
        std::fprintf(stderr, "section_trace: %s: %s\n", why, std::strerror(errno));
        kill(pid, SIGKILL);
        waitpid(pid, nullptr, 0);
        return CANNOT_TRACE;
    };
    if (ptrace(PTRACE_SETOPTIONS, pid, nullptr, (void*)(long)(PTRACE_O_TRACEEXIT | PTRACE_O_EXITKILL)) != 0)
        return give_up("PTRACE_SETOPTIONS");
    uint64_t base = 0;
    if (!load_base(pid, base)) return give_up("load base");
    a_checks += base;
    a_fails += base;
    std::map<uint64_t, Bp> bps;
    for (auto& e : entries) {
        uint64_t addr = e.first + base, w = 0;
        if (bps.count(addr)) continue;
        if (!t.peek(addr, w) || !t.poke(addr, (w & ~0xffull) | 0xcc)) return give_up("breakpoint");
        Bp b;
        b.orig = w;
        b.refs = 1;
        b.entry = true;
        b.name = e.second;
        bps[addr] = b;
    }
    t.ev = std::fopen(events.c_str(), "w");
    if (!t.ev) return give_up(events.c_str());
    auto event = [&](const char* kind, const std::string& rest) {
        std::fprintf(t.ev, "%s\t%s\t%d\t%d\n", kind, rest.c_str(), t.counter(a_checks), t.counter(a_fails));
        std::fflush(t.ev);
    };
    // DR0 = &g_fail, DR7: L0 enabled, RW0 = 01 (data writes), LEN0 = 11 (4 bytes)
    bool watch = (a_fails & 3) == 0 && set_dr(pid, 0, a_fails) && set_dr(pid, 7, 1ull | (1ull << 16) | (3ull << 18));
    if (!watch) event("nowatch", std::strerror(errno));
    // A SIGTRAP stop: was it the watchpoint? Then record the failing site and clear DR6.
    auto watch_hit = [&](uint64_t rip) -> bool {
        uint64_t dr6 = 0;
        if (!watch || !get_dr(pid, 6, dr6) || !(dr6 & 1)) return false;
        set_dr(pid, 6, 0);
        char b[32];
        std::snprintf(b, sizeof b, "0x%llx", (unsigned long long)(rip - 1));   // inside the writing instruction
        event("fail", b);
        return true;
    };
    auto marker = [&](const char* kind, const std::string& name) {
        std::fprintf(stderr, "@@tp %s %s\n", kind, name.c_str());
        std::fflush(stderr);
    };

    if (timeout) {
        struct sigaction sa;
        std::memset(&sa, 0, sizeof sa);
        sa.sa_handler = on_alarm;   // no SA_RESTART: waitpid returns EINTR
        sigaction(SIGALRM, &sa, nullptr);
        alarm(timeout);
    }

    std::vector<Frame> stack;
    int deliver = 0;
    bool killed = false;
    // Executes the original instruction at addr (rip already reset to addr) and puts int3 back
    // if keep. Other signals arriving meanwhile are delivered at the next continue.
    auto step_over = [&](uint64_t addr, bool keep) -> bool {
        Bp& b = bps[addr];
        if (!t.poke(addr, b.orig)) return false;
        if (!keep) {
            bps.erase(addr);
            return true;
        }
        if (ptrace(PTRACE_SINGLESTEP, pid, nullptr, nullptr) != 0) return false;
        int s = 0;
        for (;;) {
            if (waitpid(pid, &s, 0) != pid) {
                if (errno == EINTR) continue;
                return false;
            }
            if (!WIFSTOPPED(s)) return false;
            if (WSTOPSIG(s) == SIGTRAP && (s >> 16) == 0) {
                struct user_regs_struct sr;
                if (ptrace(PTRACE_GETREGS, pid, nullptr, &sr) == 0) watch_hit(sr.rip);
                break;
            }
            if ((s >> 16) == PTRACE_EVENT_EXIT) return false;
            deliver = WSTOPSIG(s);   // keep the latest, deliver after the step
            if (ptrace(PTRACE_SINGLESTEP, pid, nullptr, nullptr) != 0) return false;
        }
        return t.poke(addr, (b.orig & ~0xffull) | 0xcc);
    };

    int result = 0;
    for (;;) {
        if (ptrace(PTRACE_CONT, pid, nullptr, (void*)(long)deliver) != 0 && errno == ESRCH) {
            // gone (killed meanwhile): collect it below
        }
        deliver = 0;
        int s = 0;
        pid_t w = waitpid(pid, &s, 0);
        if (w < 0 && errno == EINTR) {
            if (g_alarm && !killed) {
                std::fprintf(t.ev, "timeout\t%u\t-1\t-1\n", timeout);
                std::fflush(t.ev);
                kill(pid, SIGKILL);
                killed = true;
            }
            w = waitpid(pid, &s, 0);
            if (w < 0 && errno == EINTR) w = waitpid(pid, &s, 0);
        }
        if (w != pid) {
            std::fprintf(stderr, "section_trace: waitpid: %s\n", std::strerror(errno));
            result = 1;
            break;
        }
        if (WIFEXITED(s)) { result = WEXITSTATUS(s); break; }
        if (WIFSIGNALED(s)) { result = 128 + WTERMSIG(s); break; }
        if (!WIFSTOPPED(s)) continue;
        int sig = WSTOPSIG(s);
        if (sig == SIGTRAP && (s >> 16) == PTRACE_EVENT_EXIT) {
            unsigned long msg = 0;
            ptrace(PTRACE_GETEVENTMSG, pid, nullptr, &msg);
            event("exit", std::to_string(msg));
            continue;
        }
        if (sig == SIGTRAP && (s >> 16) == 0) {
            struct user_regs_struct r;
            if (ptrace(PTRACE_GETREGS, pid, nullptr, &r) != 0) { result = 1; break; }
            if (watch_hit(r.rip)) continue;
            uint64_t addr = r.rip - 1;
            auto it = bps.find(addr);
            if (it == bps.end()) { deliver = SIGTRAP; continue; }   // not ours
            r.rip = addr;
            if (ptrace(PTRACE_SETREGS, pid, nullptr, &r) != 0) { result = 1; break; }
            bool keep = true;
            if (it->second.entry) {
                const std::string name = it->second.name;
                event("enter", name);
                marker("enter", name);
                uint64_t ret = 0;
                if (t.peek(r.rsp, ret)) {
                    stack.push_back({name, ret, r.rsp + 8});
                    auto rb = bps.find(ret);
                    if (rb != bps.end()) {
                        rb->second.refs++;
                    } else {
                        uint64_t word = 0;
                        if (t.peek(ret, word) && t.poke(ret, (word & ~0xffull) | 0xcc)) {
                            Bp b;
                            b.orig = word;
                            b.refs = 1;
                            bps[ret] = b;
                        }
                    }
                }
            } else {
                if (!stack.empty() && stack.back().ret == addr && stack.back().sp == r.rsp) {
                    event("leave", stack.back().name);
                    marker("leave", stack.back().name);
                    stack.pop_back();
                    keep = --it->second.refs > 0;
                }
            }
            if (!step_over(addr, keep)) { result = 1; break; }
            continue;
        }
        if (sig == SIGSEGV || sig == SIGBUS || sig == SIGABRT || sig == SIGFPE || sig == SIGILL) {
            struct user_regs_struct r;
            std::memset(&r, 0, sizeof r);
            ptrace(PTRACE_GETREGS, pid, nullptr, &r);
            char b[32];
            std::snprintf(b, sizeof b, "0x%llx", (unsigned long long)r.rip);
            event("signal", std::to_string(sig) + "\t" + b + "\t" + where(pid, r.rip));
            std::fprintf(stderr, "@@tp signal %d\n", sig);
        }
        deliver = sig;
    }
    std::fclose(t.ev);
    return result;
}
#endif
