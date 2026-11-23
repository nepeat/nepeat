// SPDX-License-Identifier: Apache-2.0
// Debug hook: SAI_OPENBCM_DIAG_SOCKET=<path> makes syncd listen on a unix
// socket for SDK diag-shell commands while it owns the chip (no second SDK
// process may attach). One command per connection: the client sends one
// line, gets the command's output, then EOF. Off unless the key is set.
// Root only: the directory is created 0700, the socket 0600, and peers
// whose SO_PEERCRED uid is neither 0 nor syncd's own are refused. Commands run under the SAI
// lock (never beside a SAI call); a few that would end or re-init the SDK
// are refused. Anything else, writes included, runs as typed: a debug tool.
// Client: bin/openbcm-diag (src/openbcm_diag.c).
#include "sai_internal.h"
#include "ext_macsec.h"
#include "ext_phy.h"

#include <cctype>
#include <cerrno>
#include <fcntl.h>
#include <poll.h>
#include <sstream>
#include <sys/socket.h>
#include <sys/stat.h>
#include <sys/un.h>
#include <unistd.h>

namespace ob {

namespace {
struct Diag {
    std::thread th;
    int lfd = -1, stop_rd = -1, stop_wr = -1;
    std::string path;
    Backend* be = nullptr;
};
Diag& g_diag = *new Diag;   // never destroyed: no joinable std::thread at exit()

// Commands that would end syncd's SDK session or the process.
bool refused(const std::string& cmd)
{
    static const char* const bad[] = {"exit", "quit", "init", "deinit", "reset", "detach", "attach",
                                      "reload", "rcload", "shell", "sh", "!", "warmboot", "dbm"};
    std::string w;
    for (char ch : cmd) {
        if (ch == ' ' || ch == '\t' || ch == ';') break;
        w += (char)std::tolower((unsigned char)ch);
    }
    if (cmd.find(';') != std::string::npos) return true;   // one command, no chaining
    for (const char* b : bad)
        if (w == b) return true;
    return !w.empty() && w[0] == '!';
}

// "phy raw ...": MDIO frames as typed, nothing else of the SDK.
bool raw_mdio(const std::string& cmd)
{
    std::string w[2];
    size_t i = 0;
    for (int k = 0; k < 2; k++) {
        while (i < cmd.size() && (cmd[i] == ' ' || cmd[i] == '\t')) i++;
        while (i < cmd.size() && cmd[i] != ' ' && cmd[i] != '\t') w[k] += (char)std::tolower((unsigned char)cmd[i++]);
    }
    return w[0] == "phy" && w[1] == "raw";
}

void say(int fd, const std::string& s) { (void)!::write(fd, s.data(), s.size()); }

void serve(int cfd)
{
    struct ucred cr;
    socklen_t crl = sizeof(cr);
    if (getsockopt(cfd, SOL_SOCKET, SO_PEERCRED, &cr, &crl) != 0 || (cr.uid != 0 && cr.uid != ::geteuid())) {
        say(cfd, "openbcm-diag: root only\n");
        return;
    }
    struct timeval tv = {5, 0};
    setsockopt(cfd, SOL_SOCKET, SO_RCVTIMEO, &tv, sizeof(tv));
    std::string cmd;
    char buf[256];
    while (cmd.size() < 1024 && cmd.find('\n') == std::string::npos) {
        ssize_t n = ::read(cfd, buf, sizeof(buf));
        if (n <= 0) break;
        cmd.append(buf, (size_t)n);
    }
    auto nl = cmd.find('\n');
    if (nl != std::string::npos) cmd.resize(nl);
    while (!cmd.empty() && (cmd.back() == '\r' || cmd.back() == ' ')) cmd.pop_back();
    size_t b = cmd.find_first_not_of(' ');
    cmd = b == std::string::npos ? "" : cmd.substr(b);
    if (cmd.empty() || cmd.size() >= 1000) {
        say(cfd, "openbcm-diag: send one command line (< 1000 characters)\n");
        return;
    }
    if (refused(cmd)) {
        say(cfd, "openbcm-diag: refused (would end or re-init the SDK, or chains commands): " + cmd + "\n");
        return;
    }
    sai_status_t st;
    {
        std::lock_guard<std::recursive_mutex> g(ctx().mu);
        if (!ctx().switch_created || !g_diag.be) {
            say(cfd, "openbcm-diag: no switch\n");
            return;
        }
        if (cmd == "extphy") {   // ours, not the SDK's: the external PHY table's state (ext_phy.h)
            say(cfd, ext_phy_status());
            return;
        }
        if (cmd == "extmacsec") {   // the external MACsec table's state (ext_macsec.h)
            say(cfd, ext_macsec_status());
            return;
        }
        if (cmd == "extmacsec trace" || cmd.compare(0, 16, "extmacsec trace ") == 0) {   // bus operation trace (ext_macsec.h); holds no key
            say(cfd, ext_macsec_trace(cmd.substr(15)));
            return;
        }
        if (cmd.compare(0, 14, "extmacsec run ") == 0) {   // one sequence by hand: extmacsec run <port> <sequence> [<parameter file>]
            std::istringstream is(cmd.substr(14));
            std::string port, seq, file;
            is >> port >> seq >> file;
            OB_NOTICE(SAI_API_SWITCH, "diag socket: extmacsec run %s %s%s", port.c_str(), seq.c_str(), file.empty() ? "" : " <parameter file>");
            say(cfd, ext_macsec_diag_run(port, seq, file));
            return;
        }
        OB_NOTICE(SAI_API_SWITCH, "diag socket: %s", cmd.c_str());
        if (raw_mdio(cmd))   // never inside one of the table's sequences; pauses its link polls
            ext_phy_foreign_mdio([&] { st = g_diag.be->run_shell_to(cmd, cfd); });
        else
            st = g_diag.be->run_shell_to(cmd, cfd);
    }
    if (st != SAI_STATUS_SUCCESS) say(cfd, "openbcm-diag: command failed (" + std::to_string(st) + ")\n");
}

void loop()
{
    for (;;) {
        struct pollfd p[2] = {{g_diag.lfd, POLLIN, 0}, {g_diag.stop_rd, POLLIN, 0}};
        if (::poll(p, 2, -1) < 0) {
            if (errno == EINTR) continue;
            return;
        }
        if (p[1].revents) return;
        if (!(p[0].revents & POLLIN)) continue;
        int cfd = ::accept(g_diag.lfd, nullptr, nullptr);
        if (cfd < 0) continue;
        serve(cfd);
        ::close(cfd);
    }
}
} // namespace

void diag_start(const BackendConfig& cfg, Backend* be)
{
    std::string path = cfg.get("SAI_OPENBCM_DIAG_SOCKET");
    if (path.empty() || g_diag.lfd >= 0) return;
    struct sockaddr_un a;
    if (path.size() >= sizeof(a.sun_path) || path[0] != '/') {
        OB_ERR(SAI_API_SWITCH, "SAI_OPENBCM_DIAG_SOCKET %s: not an absolute path that fits a unix socket", path.c_str());
        return;
    }
    std::string dir = path.substr(0, path.rfind('/'));
    if (!dir.empty() && ::mkdir(dir.c_str(), 0700) != 0 && errno != EEXIST) {
        OB_ERR(SAI_API_SWITCH, "diag socket: mkdir %s: %s", dir.c_str(), strerror(errno));
        return;
    }
    struct stat sb;
    if (::lstat(path.c_str(), &sb) == 0) {
        if (!S_ISSOCK(sb.st_mode)) {
            OB_ERR(SAI_API_SWITCH, "diag socket: %s exists and is not a socket", path.c_str());
            return;
        }
        ::unlink(path.c_str());
    }
    int fd = ::socket(AF_UNIX, SOCK_STREAM | SOCK_CLOEXEC, 0);
    if (fd < 0) return;
    std::memset(&a, 0, sizeof(a));
    a.sun_family = AF_UNIX;
    std::memcpy(a.sun_path, path.c_str(), path.size());
    mode_t old = ::umask(0177);   // socket 0600 from the start
    int rc = ::bind(fd, (struct sockaddr*)&a, sizeof(a));
    ::umask(old);
    int pfd[2];
    if (rc != 0 || ::chmod(path.c_str(), 0600) != 0 || ::listen(fd, 4) != 0 || ::pipe2(pfd, O_CLOEXEC) != 0) {
        OB_ERR(SAI_API_SWITCH, "diag socket %s: %s", path.c_str(), strerror(errno));
        ::close(fd);
        return;
    }
    g_diag.lfd = fd;
    g_diag.stop_rd = pfd[0];
    g_diag.stop_wr = pfd[1];
    g_diag.path = path;
    g_diag.be = be;
    g_diag.th = std::thread(loop);
    OB_NOTICE(SAI_API_SWITCH, "diag socket %s: SDK diag shell commands (root, one per connection)", path.c_str());
}

// Call without the SAI lock (the server takes it per command).
void diag_stop()
{
    if (g_diag.lfd < 0) return;
    (void)!::write(g_diag.stop_wr, "x", 1);
    if (g_diag.th.joinable()) g_diag.th.join();
    ::close(g_diag.lfd);
    ::close(g_diag.stop_rd);
    ::close(g_diag.stop_wr);
    ::unlink(g_diag.path.c_str());
    g_diag = Diag();
}

} // namespace ob
