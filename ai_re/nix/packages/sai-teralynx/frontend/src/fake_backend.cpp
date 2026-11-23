// SPDX-License-Identifier: Apache-2.0
#include "fake_backend.h"

#include <algorithm>
#include <cerrno>
#include <cstdlib>
#include <cstring>

#include <arpa/inet.h>
#include <fcntl.h>
#include <linux/if_ether.h>
#include <linux/if_packet.h>
#include <linux/if_tun.h>
#include <net/if.h>
#include <net/if_arp.h>
#include <poll.h>
#include <sys/ioctl.h>
#include <sys/socket.h>
#include <unistd.h>

#include "sai_internal.h"

namespace ob {

static std::vector<uint8_t> macv(const uint8_t m[6]) { return std::vector<uint8_t>(m, m + 6); }

// Table capacities of the fake (made up, round numbers; the openbcm backend
// reports the SDK's). Free = capacity - what the model holds.
static constexpr uint32_t FAKE_V4_ROUTES = 16384, FAKE_V6_ROUTES = 8192, FAKE_HOSTS = 16384,
                          FAKE_EGRESS = 32768, FAKE_FDB = 32768;
// VRF ids 1..FAKE_VRF_MAX; ECMP: groups, members per group, member entries.
static constexpr uint32_t FAKE_VRF_MAX = 1023, FAKE_ECMP_GROUPS = 4096, FAKE_ECMP_PATHS = 64,
                          FAKE_ECMP_MEMBERS = 16384;
// Chip limits: the BCM56960's (20165 cells x 4 XPEs x 208 B; SDK field entry
// priorities BCM_FIELD_ENTRY_PRIO_LOWEST..HIGHEST).
static constexpr uint64_t FAKE_BUFFER_BYTES = 20165ull * 4 * 208;
static constexpr uint32_t FAKE_ACL_PRIO_MIN = 0, FAKE_ACL_PRIO_MAX = 0x7fffffff;

// "<netif>:<ifname>,..." -> map; false on a malformed list.
static bool parse_wire(const std::string& s, std::map<std::string, std::string>& out)
{
    out.clear();
    size_t i = 0;
    while (i < s.size()) {
        size_t j = s.find(',', i);
        std::string item = s.substr(i, j == std::string::npos ? std::string::npos : j - i);
        size_t c = item.find(':');
        if (c == std::string::npos || c == 0 || c + 1 >= item.size() || c > 15 || item.size() - c - 1 > 15)
            return false;
        std::string a = item.substr(0, c), b = item.substr(c + 1);
        if (out.count(a)) return false;
        for (auto& kv : out)
            if (kv.second == b) return false;   // one host interface per netif
        out[a] = b;
        if (j == std::string::npos) break;
        i = j + 1;
    }
    return true;
}

FakeBackend::~FakeBackend() { shutdown(); }

sai_status_t FakeBackend::init(const BackendConfig& cfg, LinkCallback link_cb, FdbCallback fdb_cb)
{
    std::lock_guard<std::mutex> g(mu);
    if (initialized) return SAI_STATUS_FAILURE;
    int n = std::atoi(cfg.get("SAI_OPENBCM_FAKE_PORTS", "32").c_str());
    int lanes = std::atoi(cfg.get("SAI_OPENBCM_FAKE_LANES", "4").c_str());
    long speed = std::atol(cfg.get("SAI_OPENBCM_FAKE_SPEED", "100000").c_str());
    if (n < 1 || n > 256 || lanes < 1 || lanes > 8 || speed <= 0) return SAI_STATUS_INVALID_PARAMETER;
    link_follows_admin_ = cfg.get("SAI_OPENBCM_FAKE_LINK", "admin") != "none";
    std::string nd = cfg.get("SAI_OPENBCM_FAKE_NETDEV", "none");
    if (nd != "none" && nd != "tap") return SAI_STATUS_INVALID_PARAMETER;
    tap_mode = nd == "tap";
    if (!parse_wire(cfg.get("SAI_OPENBCM_FAKE_WIRE"), wire)) return SAI_STATUS_INVALID_PARAMETER;
    if (!wire.empty() && !tap_mode) return SAI_STATUS_INVALID_PARAMETER;   // a wire needs the TAP
    if (tap_mode && access("/dev/net/tun", R_OK | W_OK) != 0) return SAI_STATUS_FAILURE;
    link_cb_ = std::move(link_cb);
    fdb_cb_ = std::move(fdb_cb);
    link_gates_.clear();
    { std::lock_guard<std::mutex> mg(mdio_mu); mdio_regs.clear(); mdio_log.clear(); mdio_dead.clear(); }
    port_state.clear(); vlans.clear(); l2.clear(); netifs.clear(); traps.clear();
    intfs.clear(); stations.clear(); egress.clear(); hosts.clear(); routes.clear(); ecmps.clear(); lags.clear();
    fp_groups.clear(); fp_entries.clear(); policers.clear();
    temps.clear();
    for (int i = 0; i < 8; i++) temps.push_back(400 + 15 * i);
    temp_fail = false;
    for (int i = 0; i < n; i++) {
        Port p;
        p.info.hw = 1 + i * lanes;               // like the SDK: gaps between logical ports
        for (int l = 0; l < lanes; l++) p.info.lanes.push_back((uint32_t)(i * lanes + l + 1));
        p.info.speed = (uint32_t)speed;
        p.info.name = (lanes >= 4 ? "ce" : "xe") + std::to_string(i);
        vlans[1][p.info.hw] = true;              // SDK default: all ports untagged in VLAN 1
        port_state[p.info.hw] = p;
    }
    vlans[1][0] = false;                         // and the CPU, tagged
    initialized = true;
    return SAI_STATUS_SUCCESS;
}

void FakeBackend::shutdown()
{
    std::map<int, std::unique_ptr<Tap>> taps;
    {
        std::lock_guard<std::mutex> g(mu);
        initialized = false;
        link_cb_ = nullptr;
        fdb_cb_ = nullptr;
        taps.swap(taps_);
    }
    for (auto& t : taps) tap_close(t.second);   // TAP netdevs go with their fd
}

sai_status_t FakeBackend::run_shell(const std::string& cmd)
{
    std::lock_guard<std::mutex> g(mu);
    shell_log.push_back(cmd);
    return SAI_STATUS_SUCCESS;
}

sai_status_t FakeBackend::run_shell_to(const std::string& cmd, int fd)
{
    {
        std::lock_guard<std::mutex> g(mu);
        shell_log.push_back(cmd);
    }
    std::string out = "fake: " + cmd + "\n";
    return ::write(fd, out.data(), out.size()) == (ssize_t)out.size() ? SAI_STATUS_SUCCESS : SAI_STATUS_FAILURE;
}

std::vector<PortInfo> FakeBackend::ports()
{
    std::lock_guard<std::mutex> g(mu);
    std::vector<PortInfo> v;
    for (auto& kv : port_state) v.push_back(kv.second.info);
    return v;
}

FakeBackend::Port* FakeBackend::port(int hw)
{
    auto it = port_state.find(hw);
    return it == port_state.end() ? nullptr : &it->second;
}

bool FakeBackend::dest_ok(int d)
{
    return is_lag_dest(d) ? lags.count(lag_tid(d)) != 0 : port(d) != nullptr;
}

sai_status_t FakeBackend::asic_temperatures(std::vector<int>& cur, std::vector<int>& peak)
{
    std::lock_guard<std::mutex> g(mu);
    if (temp_fail) return SAI_STATUS_FAILURE;
    cur = temps;
    peak.clear();
    for (int t : temps) peak.push_back(t + 5);
    return SAI_STATUS_SUCCESS;
}

sai_status_t FakeBackend::table_usage(TableUsage& u)
{
    std::lock_guard<std::mutex> g(mu);
    uint32_t v4 = 0, v6 = 0, host_units = 0;
    for (auto& r : routes) (r.first.second.addr.v6 ? v6 : v4)++;
    for (auto& h : hosts) host_units += h.first.second.v6 ? 2 : 1;
    size_t members = 0;
    for (auto& e : ecmps) members += e.second.size();
    auto left = [](uint32_t cap, size_t used) { return used >= cap ? 0u : cap - (uint32_t)used; };
    u.v4_routes_free = left(FAKE_V4_ROUTES, v4);
    u.v6_routes_free = left(FAKE_V6_ROUTES, v6);
    u.hosts_free = left(FAKE_HOSTS, host_units);
    u.egress_free = left(FAKE_EGRESS, egress.size());
    u.fdb_free = left(FAKE_FDB, l2.size());
    u.ecmp_groups_free = left(FAKE_ECMP_GROUPS, ecmps.size());
    u.ecmp_members_free = left(FAKE_ECMP_MEMBERS, members);
    return SAI_STATUS_SUCCESS;
}

sai_status_t FakeBackend::chip_limits(ChipLimits& l)
{
    l.buffer_bytes = FAKE_BUFFER_BYTES;
    l.acl_prio_min = FAKE_ACL_PRIO_MIN;
    l.acl_prio_max = FAKE_ACL_PRIO_MAX;
    l.vrf_max = FAKE_VRF_MAX;
    l.ecmp_groups = FAKE_ECMP_GROUPS;
    l.ecmp_paths = FAKE_ECMP_PATHS;
    l.ecmp_members = FAKE_ECMP_MEMBERS;
    return SAI_STATUS_SUCCESS;
}

sai_status_t FakeBackend::lag_create(int& tid)
{
    std::lock_guard<std::mutex> g(mu);
    if (lags.size() >= 128) return SAI_STATUS_INSUFFICIENT_RESOURCES;   // fake limit
    tid = next_tid_++;
    lags[tid];
    return SAI_STATUS_SUCCESS;
}

sai_status_t FakeBackend::lag_destroy(int tid)
{
    std::lock_guard<std::mutex> g(mu);
    if (!lags.erase(tid)) return SAI_STATUS_ITEM_NOT_FOUND;
    return SAI_STATUS_SUCCESS;
}

sai_status_t FakeBackend::lag_set_members(int tid, const std::vector<LagMember>& members)
{
    std::lock_guard<std::mutex> g(mu);
    auto it = lags.find(tid);
    if (it == lags.end()) return SAI_STATUS_ITEM_NOT_FOUND;
    if (members.size() > 64) return SAI_STATUS_INVALID_PARAMETER;
    for (auto& m : members) {
        if (!port(m.hw)) return SAI_STATUS_INVALID_PORT_NUMBER;
        for (auto& o : lags)   // a port is in one trunk at most
            if (o.first != tid)
                for (auto& x : o.second)
                    if (x.hw == m.hw) return SAI_STATUS_INVALID_PARAMETER;
    }
    it->second = members;
    return SAI_STATUS_SUCCESS;
}

void FakeBackend::update_link(Port& p, std::vector<std::pair<int, bool>>& changes)
{
    bool up = p.admin && (p.lb != Loopback::None || link_follows_admin_);
    if (up) {   // as the SDK's link scan: the gate first, the ASIC link only if it has no opinion
        auto it = link_gates_.find(p.info.hw);
        if (it != link_gates_.end() && it->second && it->second() == 0) up = false;
    }
    if (up != p.link) {
        p.link = up;
        changes.push_back({p.info.hw, up});
    }
}

void FakeBackend::report(const std::vector<std::pair<int, bool>>& changes)
{
    LinkCallback cb;
    { std::lock_guard<std::mutex> g(mu); cb = link_cb_; }
    if (cb) for (auto& c : changes) cb(c.first, c.second);
}

#define FAKE_PORT(hw)                                   \
    Port* p = port(hw);                                 \
    if (!p) return SAI_STATUS_INVALID_PORT_NUMBER;

sai_status_t FakeBackend::port_admin_set(int hw, bool up)
{
    std::vector<std::pair<int, bool>> ch;
    {
        std::lock_guard<std::mutex> g(mu);
        FAKE_PORT(hw);
        p->admin = up;
        update_link(*p, ch);
    }
    report(ch);
    return SAI_STATUS_SUCCESS;
}

sai_status_t FakeBackend::port_link_get(int hw, bool& up)
{
    std::lock_guard<std::mutex> g(mu);
    FAKE_PORT(hw);
    up = p->link;
    return SAI_STATUS_SUCCESS;
}

sai_status_t FakeBackend::port_supported_speeds(int hw, std::vector<uint32_t>& out)
{
    std::lock_guard<std::mutex> g(mu);
    FAKE_PORT(hw);
    size_t n = p->info.lanes.size();
    out.clear();
    if (n >= 4) out = {40000, 100000};
    else if (n == 2) out = {50000};
    else out = {10000, 25000};
    return SAI_STATUS_SUCCESS;
}

sai_status_t FakeBackend::port_speed_set(int hw, uint32_t mbps)
{
    std::vector<uint32_t> ok;
    sai_status_t st = port_supported_speeds(hw, ok);
    if (st != SAI_STATUS_SUCCESS) return st;
    if (std::find(ok.begin(), ok.end(), mbps) == ok.end()) return SAI_STATUS_NOT_SUPPORTED;
    std::lock_guard<std::mutex> g(mu);
    port(hw)->info.speed = mbps;
    return SAI_STATUS_SUCCESS;
}

sai_status_t FakeBackend::port_speed_get(int hw, uint32_t& mbps)
{
    std::lock_guard<std::mutex> g(mu);
    FAKE_PORT(hw);
    mbps = p->info.speed;
    return SAI_STATUS_SUCCESS;
}

sai_status_t FakeBackend::port_frame_max_set(int hw, uint32_t bytes)
{
    std::lock_guard<std::mutex> g(mu);
    FAKE_PORT(hw);
    if (bytes < 64 || bytes > 9416) return SAI_STATUS_INVALID_PARAMETER;
    p->frame_max = bytes;
    return SAI_STATUS_SUCCESS;
}

sai_status_t FakeBackend::port_fec_set(int hw, Fec f)
{
    std::lock_guard<std::mutex> g(mu);
    FAKE_PORT(hw);
    p->fec = f;
    return SAI_STATUS_SUCCESS;
}

sai_status_t FakeBackend::port_autoneg_set(int hw, bool on)
{
    std::lock_guard<std::mutex> g(mu);
    FAKE_PORT(hw);
    p->autoneg = on;
    return SAI_STATUS_SUCCESS;
}

sai_status_t FakeBackend::port_interface_set(int hw, const std::string& name)
{
    std::lock_guard<std::mutex> g(mu);
    FAKE_PORT(hw);
    // As the chip: a 4-lane interface type needs a 4-lane port.
    bool four = name == "caui" || name == "caui4" || name == "caui_c2c" || name == "caui_c2m" || name == "xlaui" ||
                name == "cr4" || name == "sr4" || name == "lr4" || name == "kr4";
    if (four && p->info.lanes.size() != 4) return SAI_STATUS_INVALID_PARAMETER;
    p->iface = name;
    p->quirk_calls++;
    return SAI_STATUS_SUCCESS;
}

sai_status_t FakeBackend::port_ifg_set(int hw, uint32_t bits)
{
    std::lock_guard<std::mutex> g(mu);
    FAKE_PORT(hw);
    p->ifg = bits;
    p->quirk_calls++;
    return SAI_STATUS_SUCCESS;
}

sai_status_t FakeBackend::port_loopback_set(int hw, Loopback lb)
{
    std::vector<std::pair<int, bool>> ch;
    {
        std::lock_guard<std::mutex> g(mu);
        FAKE_PORT(hw);
        p->lb = lb;
        update_link(*p, ch);
    }
    report(ch);
    return SAI_STATUS_SUCCESS;
}

sai_status_t FakeBackend::port_pvid_set(int hw, uint16_t vid)
{
    std::lock_guard<std::mutex> g(mu);
    FAKE_PORT(hw);
    if (vid < 1 || vid > 4094) return SAI_STATUS_INVALID_PARAMETER;
    p->pvid = vid;
    return SAI_STATUS_SUCCESS;
}

sai_status_t FakeBackend::port_learn_set(int hw, Learn mode)
{
    std::lock_guard<std::mutex> g(mu);
    FAKE_PORT(hw);
    p->learn = mode;
    return SAI_STATUS_SUCCESS;
}

sai_status_t FakeBackend::port_stat_get(int hw, PortStat s, uint64_t& v)
{
    std::lock_guard<std::mutex> g(mu);
    FAKE_PORT(hw);
    if ((int)s < 0 || s >= PortStat::Count) return SAI_STATUS_NOT_SUPPORTED;
    v = p->stats[(int)s];
    return SAI_STATUS_SUCCESS;
}

sai_status_t FakeBackend::port_stat_clear(int hw)
{
    std::lock_guard<std::mutex> g(mu);
    FAKE_PORT(hw);
    std::memset(p->stats, 0, sizeof(p->stats));
    return SAI_STATUS_SUCCESS;
}

sai_status_t FakeBackend::vlan_create(uint16_t vid)
{
    std::lock_guard<std::mutex> g(mu);
    if (vid < 1 || vid > 4094) return SAI_STATUS_INVALID_PARAMETER;
    if (vlans.count(vid)) return SAI_STATUS_ITEM_ALREADY_EXISTS;
    vlans[vid][0] = false;                       // CPU tagged member
    return SAI_STATUS_SUCCESS;
}

sai_status_t FakeBackend::vlan_destroy(uint16_t vid)
{
    std::lock_guard<std::mutex> g(mu);
    if (!vlans.count(vid)) return SAI_STATUS_ITEM_NOT_FOUND;
    if (vid == 1) return SAI_STATUS_OBJECT_IN_USE;       // SDK refuses to destroy the default VLAN
    vlans.erase(vid);
    return SAI_STATUS_SUCCESS;
}

sai_status_t FakeBackend::vlan_port_add(uint16_t vid, int hw, bool untagged)
{
    std::lock_guard<std::mutex> g(mu);
    if (!vlans.count(vid)) return SAI_STATUS_ITEM_NOT_FOUND;
    if (!port(hw)) return SAI_STATUS_INVALID_PORT_NUMBER;
    vlans[vid][hw] = untagged;
    return SAI_STATUS_SUCCESS;
}

sai_status_t FakeBackend::vlan_port_remove(uint16_t vid, int hw)
{
    std::lock_guard<std::mutex> g(mu);
    auto it = vlans.find(vid);
    if (it == vlans.end()) return SAI_STATUS_ITEM_NOT_FOUND;
    it->second.erase(hw);
    return SAI_STATUS_SUCCESS;
}

sai_status_t FakeBackend::vlan_members(uint16_t vid, std::vector<std::pair<int, bool>>& out)
{
    std::lock_guard<std::mutex> g(mu);
    auto it = vlans.find(vid);
    if (it == vlans.end()) return SAI_STATUS_ITEM_NOT_FOUND;
    out.clear();
    for (auto& m : it->second) if (m.first != 0) out.push_back(m);
    return SAI_STATUS_SUCCESS;
}

sai_status_t FakeBackend::l2_add(const uint8_t mac[6], uint16_t vid, int hw, bool is_static, bool discard)
{
    std::lock_guard<std::mutex> g(mu);
    if (!vlans.count(vid)) return SAI_STATUS_INVALID_PARAMETER;   // SDK: BCM_E_PARAM for unknown VLAN
    if (hw != 0 && !dest_ok(hw)) return SAI_STATUS_INVALID_PORT_NUMBER;
    l2[{macv(mac), vid}] = L2{hw, is_static, discard};
    return SAI_STATUS_SUCCESS;
}

sai_status_t FakeBackend::l2_delete(const uint8_t mac[6], uint16_t vid)
{
    std::lock_guard<std::mutex> g(mu);
    if (!l2.erase({macv(mac), vid})) return SAI_STATUS_ITEM_NOT_FOUND;
    return SAI_STATUS_SUCCESS;
}

sai_status_t FakeBackend::l2_flush(int hw, int vid, bool statics, bool dynamics)
{
    std::lock_guard<std::mutex> g(mu);
    for (auto it = l2.begin(); it != l2.end();) {
        bool match = (hw < 0 || it->second.hw == hw) && (vid < 0 || it->first.second == vid) &&
                     (it->second.is_static ? statics : dynamics);
        it = match ? l2.erase(it) : std::next(it);
    }
    return SAI_STATUS_SUCCESS;
}

sai_status_t FakeBackend::l2_age_set(uint32_t seconds)
{
    std::lock_guard<std::mutex> g(mu);
    age_seconds = seconds;
    return SAI_STATUS_SUCCESS;
}

sai_status_t FakeBackend::netif_create(const std::string& ifname, int hw, uint16_t vid, const uint8_t mac[6],
                                       bool keep_tag, int& netif_id, int& filter_id)
{
    std::lock_guard<std::mutex> g(mu);
    if (!port(hw)) return SAI_STATUS_INVALID_PORT_NUMBER;
    if (ifname.empty() || ifname.size() > 15) return SAI_STATUS_INVALID_PARAMETER;
    for (auto& n : netifs)
        if (n.second.name == ifname) return SAI_STATUS_ITEM_ALREADY_EXISTS;
    std::unique_ptr<Tap> tap;
    if (tap_mode) {
        sai_status_t st = tap_open(ifname, hw, mac, tap);
        if (st != SAI_STATUS_SUCCESS) return st;
    }
    Netif n{ifname, hw, vid, {0}, keep_tag, false, next_filter_++};
    std::memcpy(n.mac, mac, 6);
    netif_id = next_netif_++;
    filter_id = n.filter;
    netifs[netif_id] = n;
    if (tap) taps_[netif_id] = std::move(tap);
    return SAI_STATUS_SUCCESS;
}

sai_status_t FakeBackend::netif_destroy(int netif_id, int /*filter_id*/)
{
    std::unique_ptr<Tap> tap;
    {
        std::lock_guard<std::mutex> g(mu);
        if (!netifs.erase(netif_id)) return SAI_STATUS_ITEM_NOT_FOUND;
        auto it = taps_.find(netif_id);
        if (it != taps_.end()) {
            tap = std::move(it->second);
            taps_.erase(it);
        }
    }
    if (tap) tap_close(tap);
    return SAI_STATUS_SUCCESS;
}

sai_status_t FakeBackend::netif_carrier_set(const std::string& ifname, bool up)
{
    std::lock_guard<std::mutex> g(mu);
    for (auto& n : netifs)
        if (n.second.name == ifname) {
            n.second.carrier = up;
            auto t = taps_.find(n.first);
            if (t != taps_.end()) {
                int on = up ? 1 : 0;
                if (ioctl(t->second->fd, TUNSETCARRIER, &on) != 0) {
                    OB_ERR(SAI_API_HOSTIF, "fake: TAP %s carrier: %s", ifname.c_str(), std::strerror(errno));
                    return SAI_STATUS_FAILURE;
                }
            }
            return SAI_STATUS_SUCCESS;
        }
    return SAI_STATUS_ITEM_NOT_FOUND;
}

// ---- TAP netdevs and wires (tap mode only; never used by the unit tests)
sai_status_t FakeBackend::tap_open(const std::string& ifname, int hw, const uint8_t mac[6], std::unique_ptr<Tap>& out)
{
    auto t = std::make_unique<Tap>();
    t->hw = hw;
    auto fail = [&](const char* what) {
        OB_ERR(SAI_API_HOSTIF, "fake: TAP %s: %s: %s", ifname.c_str(), what, std::strerror(errno));
        tap_close(t);
        return SAI_STATUS_FAILURE;
    };
    t->fd = open("/dev/net/tun", O_RDWR | O_CLOEXEC);
    if (t->fd < 0) return fail("open /dev/net/tun");
    struct ifreq ifr;
    std::memset(&ifr, 0, sizeof(ifr));
    ifr.ifr_flags = IFF_TAP | IFF_NO_PI;
    std::strncpy(ifr.ifr_name, ifname.c_str(), IFNAMSIZ - 1);
    if (ioctl(t->fd, TUNSETIFF, &ifr) != 0) return fail("TUNSETIFF");
    int off = 0;
    if (ioctl(t->fd, TUNSETCARRIER, &off) != 0) return fail("TUNSETCARRIER");   // like KNET: down until OPER_STATUS
    int s = socket(AF_INET, SOCK_DGRAM | SOCK_CLOEXEC, 0);
    if (s < 0) return fail("socket");
    ifr.ifr_hwaddr.sa_family = ARPHRD_ETHER;
    std::memcpy(ifr.ifr_hwaddr.sa_data, mac, 6);
    int rc = ioctl(s, SIOCSIFHWADDR, &ifr);
    close(s);
    if (rc != 0) return fail("SIOCSIFHWADDR");
    auto w = wire.find(ifname);
    if (w != wire.end()) {
        unsigned idx = if_nametoindex(w->second.c_str());
        if (idx == 0) return fail(("wire interface " + w->second).c_str());
        t->wire_fd = socket(AF_PACKET, SOCK_RAW | SOCK_CLOEXEC, htons(ETH_P_ALL));
        if (t->wire_fd < 0) return fail("AF_PACKET socket");
        struct sockaddr_ll sll;
        std::memset(&sll, 0, sizeof(sll));
        sll.sll_family = AF_PACKET;
        sll.sll_protocol = htons(ETH_P_ALL);
        sll.sll_ifindex = (int)idx;
        if (bind(t->wire_fd, (struct sockaddr*)&sll, sizeof(sll)) != 0) return fail("bind wire");
        struct packet_mreq mr;
        std::memset(&mr, 0, sizeof(mr));
        mr.mr_ifindex = (int)idx;
        mr.mr_type = PACKET_MR_PROMISC;
        if (setsockopt(t->wire_fd, SOL_PACKET, PACKET_ADD_MEMBERSHIP, &mr, sizeof(mr)) != 0) return fail("promisc");
        int one = 1;
        if (setsockopt(t->wire_fd, SOL_PACKET, PACKET_IGNORE_OUTGOING, &one, sizeof(one)) != 0)
            return fail("PACKET_IGNORE_OUTGOING");
        int p[2];
        if (pipe2(p, O_CLOEXEC) != 0) return fail("pipe");
        t->stop_rd = p[0];
        t->stop_wr = p[1];
        Tap* raw = t.get();
        t->pump = std::thread([this, raw] { pump(raw); });
        OB_NOTICE(SAI_API_HOSTIF, "fake: TAP %s wired to %s", ifname.c_str(), w->second.c_str());
    }
    out = std::move(t);
    return SAI_STATUS_SUCCESS;
}

void FakeBackend::tap_close(std::unique_ptr<Tap>& t)
{
    if (!t) return;
    if (t->pump.joinable()) {
        char c = 0;
        if (write(t->stop_wr, &c, 1) < 0) { /* pump exits on its next wakeup anyway */ }
        t->pump.join();
    }
    for (int fd : {t->fd, t->wire_fd, t->stop_rd, t->stop_wr})
        if (fd >= 0) close(fd);
    t.reset();
}

bool FakeBackend::port_up(int hw)
{
    std::lock_guard<std::mutex> g(mu);
    Port* p = port(hw);
    return p && p->admin && p->link;
}

// Frames TAP -> wire and wire -> TAP while the port is up (KNET: TX to the
// port, RX of what reaches the CPU from it). No lock held while blocking.
void FakeBackend::pump(Tap* t)
{
    std::vector<uint8_t> buf(65536);
    struct pollfd pf[3] = {{t->fd, POLLIN, 0}, {t->wire_fd, POLLIN, 0}, {t->stop_rd, POLLIN, 0}};
    for (;;) {
        if (poll(pf, 3, -1) < 0) {
            if (errno == EINTR) continue;
            return;
        }
        if (pf[2].revents) return;
        if (pf[0].revents & POLLIN) {
            ssize_t n = read(t->fd, buf.data(), buf.size());
            if (n > 0 && port_up(t->hw)) (void)send(t->wire_fd, buf.data(), (size_t)n, 0);
        }
        if (pf[1].revents & POLLIN) {
            ssize_t n = recv(t->wire_fd, buf.data(), buf.size(), 0);
            if (n > 0 && port_up(t->hw) && write(t->fd, buf.data(), (size_t)n) < 0) { /* TAP queue full: drop */ }
        }
        if ((pf[0].revents | pf[1].revents) & (POLLERR | POLLHUP | POLLNVAL)) return;
    }
}

sai_status_t FakeBackend::netif_set_keep_tag(int netif_id, int hw, bool keep_tag, int& filter_id)
{
    std::lock_guard<std::mutex> g(mu);
    auto it = netifs.find(netif_id);
    if (it == netifs.end() || it->second.hw != hw) return SAI_STATUS_ITEM_NOT_FOUND;
    it->second.keep_tag = keep_tag;
    it->second.filter = filter_id = next_filter_++;
    return SAI_STATUS_SUCCESS;
}

sai_status_t FakeBackend::trap_set(TrapKind kind, bool to_cpu)
{
    std::lock_guard<std::mutex> g(mu);
    if (kind == TrapKind::Other) return SAI_STATUS_NOT_SUPPORTED;
    traps[kind] = to_cpu;
    return SAI_STATUS_SUCCESS;
}

sai_status_t FakeBackend::l3_intf_create(const uint8_t mac[6], uint16_t vid, uint32_t mtu, int vrf, int& intf_id)
{
    std::lock_guard<std::mutex> g(mu);
    if (!vlans.count(vid)) return SAI_STATUS_INVALID_PARAMETER;
    if (vrf < 0 || (uint32_t)vrf > FAKE_VRF_MAX) return SAI_STATUS_INVALID_PARAMETER;
    Intf i{{0}, vid, mtu, vrf};
    std::memcpy(i.mac, mac, 6);
    intf_id = next_intf_++;
    intfs[intf_id] = i;
    return SAI_STATUS_SUCCESS;
}

sai_status_t FakeBackend::l3_intf_update(int intf_id, const uint8_t mac[6], uint16_t vid, uint32_t mtu, int vrf)
{
    std::lock_guard<std::mutex> g(mu);
    auto it = intfs.find(intf_id);
    if (it == intfs.end()) return SAI_STATUS_ITEM_NOT_FOUND;
    std::memcpy(it->second.mac, mac, 6);
    it->second.vid = vid;
    it->second.mtu = mtu;
    it->second.vrf = vrf;
    return SAI_STATUS_SUCCESS;
}

sai_status_t FakeBackend::l3_intf_destroy(int intf_id)
{
    std::lock_guard<std::mutex> g(mu);
    for (auto& e : egress) if (e.second.intf == intf_id) return SAI_STATUS_OBJECT_IN_USE;
    if (!intfs.erase(intf_id)) return SAI_STATUS_ITEM_NOT_FOUND;
    return SAI_STATUS_SUCCESS;
}

sai_status_t FakeBackend::l3_station_add(const uint8_t mac[6], int& station_id)
{
    std::lock_guard<std::mutex> g(mu);
    station_id = next_station_++;
    stations[station_id] = macv(mac);
    return SAI_STATUS_SUCCESS;
}

sai_status_t FakeBackend::l3_station_delete(int station_id)
{
    std::lock_guard<std::mutex> g(mu);
    if (!stations.erase(station_id)) return SAI_STATUS_ITEM_NOT_FOUND;
    return SAI_STATUS_SUCCESS;
}

sai_status_t FakeBackend::l3_egress_create(int intf_id, const uint8_t dmac[6], int hw, uint16_t vid, int& egress_id)
{
    std::lock_guard<std::mutex> g(mu);
    if (!intfs.count(intf_id)) return SAI_STATUS_INVALID_PARAMETER;
    if (!dest_ok(hw)) return SAI_STATUS_INVALID_PORT_NUMBER;
    Egress e{intf_id, {0}, hw, vid, false, false};
    std::memcpy(e.mac, dmac, 6);
    egress_id = next_egress_++;
    egress[egress_id] = e;
    return SAI_STATUS_SUCCESS;
}

sai_status_t FakeBackend::l3_egress_update(int egress_id, int intf_id, const uint8_t dmac[6], int hw, uint16_t vid)
{
    std::lock_guard<std::mutex> g(mu);
    auto it = egress.find(egress_id);
    if (it == egress.end() || it->second.cpu || it->second.drop) return SAI_STATUS_ITEM_NOT_FOUND;
    if (!intfs.count(intf_id)) return SAI_STATUS_INVALID_PARAMETER;
    if (!dest_ok(hw)) return SAI_STATUS_INVALID_PORT_NUMBER;
    it->second.intf = intf_id;
    std::memcpy(it->second.mac, dmac, 6);
    it->second.hw = hw;
    it->second.vid = vid;
    return SAI_STATUS_SUCCESS;
}

sai_status_t FakeBackend::l3_egress_destroy(int egress_id)
{
    std::lock_guard<std::mutex> g(mu);
    if (egress_id == cpu_egress_ || egress_id == drop_egress_) return SAI_STATUS_INVALID_PARAMETER;
    for (auto& h : hosts) if (h.second == egress_id) return SAI_STATUS_OBJECT_IN_USE;
    for (auto& r : routes) if (r.second == egress_id) return SAI_STATUS_OBJECT_IN_USE;
    for (auto& e : ecmps)
        for (int m : e.second) if (m == egress_id) return SAI_STATUS_OBJECT_IN_USE;
    if (!egress.erase(egress_id)) return SAI_STATUS_ITEM_NOT_FOUND;
    return SAI_STATUS_SUCCESS;
}

sai_status_t FakeBackend::l3_egress_cpu(int& egress_id)
{
    std::lock_guard<std::mutex> g(mu);
    if (cpu_egress_ < 0) {
        cpu_egress_ = next_egress_++;
        egress[cpu_egress_] = Egress{0, {0}, 0, 0, true, false};
    }
    egress_id = cpu_egress_;
    return SAI_STATUS_SUCCESS;
}

sai_status_t FakeBackend::l3_egress_drop(int& egress_id)
{
    std::lock_guard<std::mutex> g(mu);
    if (drop_egress_ < 0) {
        drop_egress_ = next_egress_++;
        egress[drop_egress_] = Egress{0, {0}, 0, 0, false, true};
    }
    egress_id = drop_egress_;
    return SAI_STATUS_SUCCESS;
}

sai_status_t FakeBackend::l3_host_add(int vrf, const IpAddr& ip, int egress_id)
{
    std::lock_guard<std::mutex> g(mu);
    if (!egress.count(egress_id)) return SAI_STATUS_INVALID_PARAMETER;
    if (vrf < 0 || (uint32_t)vrf > FAKE_VRF_MAX) return SAI_STATUS_INVALID_PARAMETER;
    if (hosts.count({vrf, ip})) return SAI_STATUS_ITEM_ALREADY_EXISTS;
    hosts[{vrf, ip}] = egress_id;
    return SAI_STATUS_SUCCESS;
}

sai_status_t FakeBackend::l3_host_delete(int vrf, const IpAddr& ip)
{
    std::lock_guard<std::mutex> g(mu);
    if (!hosts.erase({vrf, ip})) return SAI_STATUS_ITEM_NOT_FOUND;
    return SAI_STATUS_SUCCESS;
}

sai_status_t FakeBackend::l3_route_add(int vrf, const IpPrefix& p, int target, bool ecmp, bool replace)
{
    std::lock_guard<std::mutex> g(mu);
    if (ecmp ? !ecmps.count(target) : !egress.count(target)) return SAI_STATUS_INVALID_PARAMETER;
    if (vrf < 0 || (uint32_t)vrf > FAKE_VRF_MAX) return SAI_STATUS_INVALID_PARAMETER;
    RouteKey k{vrf, p};
    if (routes.count(k) && !replace) return SAI_STATUS_ITEM_ALREADY_EXISTS;
    if (!routes.count(k) && replace) return SAI_STATUS_ITEM_NOT_FOUND;
    routes[k] = target;
    return SAI_STATUS_SUCCESS;
}

sai_status_t FakeBackend::l3_route_delete(int vrf, const IpPrefix& p)
{
    std::lock_guard<std::mutex> g(mu);
    if (!routes.erase({vrf, p})) return SAI_STATUS_ITEM_NOT_FOUND;
    return SAI_STATUS_SUCCESS;
}

// The SDK's rules as far as the SAI layer can hit them: members are existing
// egress objects that forward (not the CPU / drop ones), 1..paths of them.
sai_status_t FakeBackend::l3_ecmp_create(const std::vector<int>& members, int& ecmp_id)
{
    std::lock_guard<std::mutex> g(mu);
    if (members.empty() || members.size() > FAKE_ECMP_PATHS) return SAI_STATUS_INVALID_PARAMETER;
    for (int m : members) {
        auto it = egress.find(m);
        if (it == egress.end() || it->second.cpu || it->second.drop) return SAI_STATUS_INVALID_PARAMETER;
    }
    size_t used = 0;
    for (auto& e : ecmps) used += e.second.size();
    if (ecmps.size() >= FAKE_ECMP_GROUPS || used + members.size() > FAKE_ECMP_MEMBERS) return SAI_STATUS_TABLE_FULL;
    ecmp_id = next_ecmp_++;
    ecmps[ecmp_id] = members;
    return SAI_STATUS_SUCCESS;
}

sai_status_t FakeBackend::l3_ecmp_update(int ecmp_id, const std::vector<int>& members)
{
    std::lock_guard<std::mutex> g(mu);
    auto it = ecmps.find(ecmp_id);
    if (it == ecmps.end()) return SAI_STATUS_ITEM_NOT_FOUND;
    if (members.empty() || members.size() > FAKE_ECMP_PATHS) return SAI_STATUS_INVALID_PARAMETER;
    for (int m : members) {
        auto e = egress.find(m);
        if (e == egress.end() || e->second.cpu || e->second.drop) return SAI_STATUS_INVALID_PARAMETER;
    }
    it->second = members;
    return SAI_STATUS_SUCCESS;
}

sai_status_t FakeBackend::l3_ecmp_destroy(int ecmp_id)
{
    std::lock_guard<std::mutex> g(mu);
    for (auto& r : routes) if (r.second == ecmp_id) return SAI_STATUS_OBJECT_IN_USE;
    if (!ecmps.erase(ecmp_id)) return SAI_STATUS_ITEM_NOT_FOUND;
    return SAI_STATUS_SUCCESS;
}

// Field processor: Tomahawk-like limits (12 IFP slices -> groups, entries
// in all), qualifiers checked against the group's set, ports must exist.
static constexpr size_t FAKE_FP_GROUPS = 12, FAKE_FP_ENTRIES = 2048, FAKE_POLICERS = 1024;

sai_status_t FakeBackend::fp_group_create(const std::vector<FpQual>& qset, int priority, int& gid)
{
    std::lock_guard<std::mutex> g(mu);
    if (fp_groups.size() >= FAKE_FP_GROUPS) return SAI_STATUS_INSUFFICIENT_RESOURCES;
    FpGroup grp;
    for (auto q : qset) grp.qset |= 1u << (int)q;
    grp.priority = priority;
    gid = next_fp_group_++;
    fp_groups[gid] = grp;
    return SAI_STATUS_SUCCESS;
}

sai_status_t FakeBackend::fp_group_destroy(int gid)
{
    std::lock_guard<std::mutex> g(mu);
    for (auto& e : fp_entries) if (e.second.gid == gid) return SAI_STATUS_OBJECT_IN_USE;
    return fp_groups.erase(gid) ? SAI_STATUS_SUCCESS : SAI_STATUS_ITEM_NOT_FOUND;
}

sai_status_t FakeBackend::fp_entry_create(int gid, const FpRule& r, int& eid)
{
    std::lock_guard<std::mutex> g(mu);
    auto grp = fp_groups.find(gid);
    if (grp == fp_groups.end()) return SAI_STATUS_ITEM_NOT_FOUND;
    if (r.m.present & ~grp->second.qset) return SAI_STATUS_NOT_SUPPORTED;   // qualifier not in the group
    if (fp_entries.size() >= FAKE_FP_ENTRIES) return SAI_STATUS_TABLE_FULL;
    if (r.policer >= 0 && !policers.count(r.policer)) return SAI_STATUS_INVALID_PARAMETER;
    for (int p : r.m.ports) if (!port(p)) return SAI_STATUS_INVALID_PORT_NUMBER;
    if (r.m.has(FpQual::InPorts) && r.m.ports.empty()) return SAI_STATUS_INVALID_PARAMETER;
    eid = next_fp_entry_++;
    fp_entries[eid] = FpEntry{gid, r, 0, 0};
    return SAI_STATUS_SUCCESS;
}

sai_status_t FakeBackend::fp_entry_destroy(int eid)
{
    std::lock_guard<std::mutex> g(mu);
    return fp_entries.erase(eid) ? SAI_STATUS_SUCCESS : SAI_STATUS_ITEM_NOT_FOUND;
}

sai_status_t FakeBackend::fp_entry_counter(int eid, uint64_t& packets, uint64_t& bytes)
{
    std::lock_guard<std::mutex> g(mu);
    auto it = fp_entries.find(eid);
    if (it == fp_entries.end()) return SAI_STATUS_ITEM_NOT_FOUND;
    if (!it->second.rule.counter) return SAI_STATUS_NOT_SUPPORTED;
    packets = it->second.packets;
    bytes = it->second.bytes;
    return SAI_STATUS_SUCCESS;
}

static bool policer_cfg_ok(const PolicerCfg& c)
{
    if (c.cir == 0 && c.cbs == 0) return false;          // a meter that passes nothing is a mistake
    return !c.tr_tcm || c.pir >= c.cir;                    // RFC 2698: PIR >= CIR
}

sai_status_t FakeBackend::policer_create(const PolicerCfg& cfg, int& pid)
{
    std::lock_guard<std::mutex> g(mu);
    if (!policer_cfg_ok(cfg)) return SAI_STATUS_INVALID_PARAMETER;
    if (policers.size() >= FAKE_POLICERS) return SAI_STATUS_INSUFFICIENT_RESOURCES;
    pid = next_policer_++;
    policers[pid] = Policer{cfg};
    return SAI_STATUS_SUCCESS;
}

sai_status_t FakeBackend::policer_set(int pid, const PolicerCfg& cfg)
{
    std::lock_guard<std::mutex> g(mu);
    auto it = policers.find(pid);
    if (it == policers.end()) return SAI_STATUS_ITEM_NOT_FOUND;
    if (!policer_cfg_ok(cfg)) return SAI_STATUS_INVALID_PARAMETER;
    it->second.cfg = cfg;
    return SAI_STATUS_SUCCESS;
}

sai_status_t FakeBackend::policer_destroy(int pid)
{
    std::lock_guard<std::mutex> g(mu);
    for (auto& e : fp_entries) if (e.second.rule.policer == pid) return SAI_STATUS_OBJECT_IN_USE;
    return policers.erase(pid) ? SAI_STATUS_SUCCESS : SAI_STATUS_ITEM_NOT_FOUND;
}

void FakeBackend::fp_hit(int eid, uint64_t packets, uint64_t bytes)
{
    std::lock_guard<std::mutex> g(mu);
    auto it = fp_entries.find(eid);
    if (it != fp_entries.end()) { it->second.packets += packets; it->second.bytes += bytes; }
}

void FakeBackend::linkscan()
{
    std::vector<std::pair<int, bool>> ch;
    {
        std::lock_guard<std::mutex> g(mu);
        for (auto& kv : port_state) update_link(kv.second, ch);
    }
    report(ch);
}

sai_status_t FakeBackend::mdio_c45_read(unsigned phy, unsigned dev, unsigned reg, unsigned& val)
{
    std::lock_guard<std::mutex> g(mdio_mu);
    if (mdio_dead.count(phy)) return SAI_STATUS_FAILURE;
    auto it = mdio_regs.find(std::make_tuple(phy, dev, reg));
    val = it == mdio_regs.end() ? 0 : it->second;
    mdio_log.push_back({'R', phy, dev, reg, val});
    return SAI_STATUS_SUCCESS;
}

sai_status_t FakeBackend::mdio_c45_write(unsigned phy, unsigned dev, unsigned reg, unsigned val)
{
    std::lock_guard<std::mutex> g(mdio_mu);
    if (mdio_dead.count(phy)) return SAI_STATUS_FAILURE;
    mdio_regs[std::make_tuple(phy, dev, reg)] = val;
    mdio_log.push_back({'W', phy, dev, reg, val});
    return SAI_STATUS_SUCCESS;
}

sai_status_t FakeBackend::port_link_gate(int hw, std::function<int()> gate)
{
    std::lock_guard<std::mutex> g(mu);
    FAKE_PORT(hw);
    if (gate) link_gates_[hw] = std::move(gate);
    else link_gates_.erase(hw);
    return SAI_STATUS_SUCCESS;
}

void FakeBackend::inject_link(int hw, bool up)
{
    std::vector<std::pair<int, bool>> ch;
    {
        std::lock_guard<std::mutex> g(mu);
        Port* p = port(hw);
        if (!p || p->link == up) return;
        p->link = up;
        ch.push_back({hw, up});
    }
    report(ch);
}

void FakeBackend::inject_fdb(const FdbEvent& ev)
{
    FdbCallback cb;
    {
        std::lock_guard<std::mutex> g(mu);
        if (ev.learned) l2[{macv(ev.mac), ev.vid}] = L2{ev.hw_port, false, false};
        else l2.erase({macv(ev.mac), ev.vid});
        cb = fdb_cb_;
    }
    if (cb) cb(ev);
}

void FakeBackend::set_stat(int hw, PortStat s, uint64_t v)
{
    std::lock_guard<std::mutex> g(mu);
    if (Port* p = port(hw)) p->stats[(int)s] = v;
}

Backend* make_fake_backend() { return new FakeBackend(); }

// IpAddr / IpPrefix ordering (used as map keys everywhere).
bool IpAddr::operator<(const IpAddr& o) const
{
    if (v6 != o.v6) return v6 < o.v6;
    return std::memcmp(addr, o.addr, v6 ? 16 : 4) < 0;
}
bool IpAddr::operator==(const IpAddr& o) const
{
    return v6 == o.v6 && std::memcmp(addr, o.addr, v6 ? 16 : 4) == 0;
}
bool IpPrefix::operator<(const IpPrefix& o) const
{
    if (addr < o.addr) return true;
    if (o.addr < addr) return false;
    return mask < o.mask;
}

// ------------------------------------------------------------- QoS, mirror
// A model of what the SAI layer asked for (fake_backend.h); counters are
// what a test set. Ids are shared by every QoS / mirror object kind.
sai_status_t FakeBackend::qos_map_create(QosMapKind k, const std::vector<QosMapEntry>& e, int& id)
{
    std::lock_guard<std::mutex> g(mu);
    id = next_qos_++;
    qmaps[id] = QMap{k, e};
    return SAI_STATUS_SUCCESS;
}
sai_status_t FakeBackend::qos_map_update(int id, const std::vector<QosMapEntry>& e)
{
    std::lock_guard<std::mutex> g(mu);
    auto it = qmaps.find(id);
    if (it == qmaps.end()) return SAI_STATUS_ITEM_NOT_FOUND;
    it->second.e = e;
    return SAI_STATUS_SUCCESS;
}
sai_status_t FakeBackend::qos_map_destroy(int id)
{
    std::lock_guard<std::mutex> g(mu);
    for (auto& p : port_qmap)
        if (p.second == id) return SAI_STATUS_OBJECT_IN_USE;
    return qmaps.erase(id) ? SAI_STATUS_SUCCESS : SAI_STATUS_ITEM_NOT_FOUND;
}
sai_status_t FakeBackend::port_qos_map_set(int hw, QosMapKind k, int id)
{
    std::lock_guard<std::mutex> g(mu);
    if (!port(hw)) return SAI_STATUS_INVALID_PORT_NUMBER;
    if (id >= 0 && !qmaps.count(id)) return SAI_STATUS_ITEM_NOT_FOUND;
    if (id < 0) port_qmap.erase({hw, (int)k});
    else port_qmap[{hw, (int)k}] = id;
    return SAI_STATUS_SUCCESS;
}
sai_status_t FakeBackend::port_default_tc_set(int hw, uint8_t tc)
{
    std::lock_guard<std::mutex> g(mu);
    if (!port(hw)) return SAI_STATUS_INVALID_PORT_NUMBER;
    default_tc[hw] = tc;
    return SAI_STATUS_SUCCESS;
}
sai_status_t FakeBackend::port_pfc_set(int hw, bool separate, uint8_t tx, uint8_t rx)
{
    std::lock_guard<std::mutex> g(mu);
    if (!port(hw)) return SAI_STATUS_INVALID_PORT_NUMBER;
    pfc[hw] = std::make_tuple(separate, tx, rx);
    return SAI_STATUS_SUCCESS;
}
sai_status_t FakeBackend::queue_scheduler_set(int hw, int q, const SchedCfg* s)
{
    std::lock_guard<std::mutex> g(mu);
    if (!port(hw) || q < 0 || q > 7) return SAI_STATUS_INVALID_PARAMETER;
    if (s) queue_sched[{hw, q}] = *s;
    else queue_sched.erase({hw, q});
    return SAI_STATUS_SUCCESS;
}
sai_status_t FakeBackend::port_scheduler_set(int hw, const SchedCfg* s)
{
    std::lock_guard<std::mutex> g(mu);
    if (!port(hw)) return SAI_STATUS_INVALID_PORT_NUMBER;
    if (s) port_sched[hw] = *s;
    else port_sched.erase(hw);
    return SAI_STATUS_SUCCESS;
}
sai_status_t FakeBackend::wred_create(const WredCfg& w, int& id)
{
    std::lock_guard<std::mutex> g(mu);
    id = next_qos_++;
    wreds[id] = w;
    return SAI_STATUS_SUCCESS;
}
sai_status_t FakeBackend::wred_update(int id, const WredCfg& w)
{
    std::lock_guard<std::mutex> g(mu);
    if (!wreds.count(id)) return SAI_STATUS_ITEM_NOT_FOUND;
    wreds[id] = w;
    return SAI_STATUS_SUCCESS;
}
sai_status_t FakeBackend::wred_destroy(int id)
{
    std::lock_guard<std::mutex> g(mu);
    for (auto& q : queue_wred)
        if (q.second == id) return SAI_STATUS_OBJECT_IN_USE;
    return wreds.erase(id) ? SAI_STATUS_SUCCESS : SAI_STATUS_ITEM_NOT_FOUND;
}
sai_status_t FakeBackend::queue_wred_set(int hw, int q, int id)
{
    std::lock_guard<std::mutex> g(mu);
    if (!port(hw) || q < 0 || q > 7) return SAI_STATUS_INVALID_PARAMETER;
    if (id >= 0 && !wreds.count(id)) return SAI_STATUS_ITEM_NOT_FOUND;
    if (id < 0) queue_wred.erase({hw, q});
    else queue_wred[{hw, q}] = id;
    return SAI_STATUS_SUCCESS;
}
sai_status_t FakeBackend::queue_buffer_set(int hw, int q, const BufferCfg* b)
{
    std::lock_guard<std::mutex> g(mu);
    if (!port(hw) || q < 0 || q > 7) return SAI_STATUS_INVALID_PARAMETER;
    if (b) queue_buf[{hw, q}] = *b;
    else queue_buf.erase({hw, q});
    return SAI_STATUS_SUCCESS;
}
sai_status_t FakeBackend::pg_buffer_set(int hw, int pg, const BufferCfg* b)
{
    std::lock_guard<std::mutex> g(mu);
    if (!port(hw) || pg < 0 || pg > 7) return SAI_STATUS_INVALID_PARAMETER;
    if (b) pg_buf[{hw, pg}] = *b;
    else pg_buf.erase({hw, pg});
    return SAI_STATUS_SUCCESS;
}
// Counters: PACKETS / BYTES / DROPPED_PACKETS (and the PG's PACKETS / BYTES)
// as a test set them; any other id NOT_SUPPORTED.
static bool fake_queue_stat(sai_stat_id_t id)
{
    return id == SAI_QUEUE_STAT_PACKETS || id == SAI_QUEUE_STAT_BYTES || id == SAI_QUEUE_STAT_DROPPED_PACKETS;
}
static bool fake_pg_stat(sai_stat_id_t id)
{
    return id == SAI_INGRESS_PRIORITY_GROUP_STAT_PACKETS || id == SAI_INGRESS_PRIORITY_GROUP_STAT_BYTES;
}
sai_status_t FakeBackend::queue_stats_get(int hw, int q, uint32_t n, const sai_stat_id_t* ids, uint64_t* out)
{
    std::lock_guard<std::mutex> g(mu);
    if (!port(hw)) return SAI_STATUS_INVALID_PORT_NUMBER;
    for (uint32_t i = 0; i < n; i++) {
        if (!fake_queue_stat(ids[i])) return SAI_STATUS_NOT_SUPPORTED;
        auto it = queue_stats.find({{hw, q}, ids[i]});
        out[i] = it == queue_stats.end() ? 0 : it->second;
    }
    return SAI_STATUS_SUCCESS;
}
sai_status_t FakeBackend::queue_stats_clear(int hw, int q, uint32_t n, const sai_stat_id_t* ids)
{
    std::lock_guard<std::mutex> g(mu);
    for (uint32_t i = 0; i < n; i++) {
        if (!fake_queue_stat(ids[i])) return SAI_STATUS_NOT_SUPPORTED;
        queue_stats.erase({{hw, q}, ids[i]});
    }
    return SAI_STATUS_SUCCESS;
}
sai_status_t FakeBackend::pg_stats_get(int hw, int pg, uint32_t n, const sai_stat_id_t* ids, uint64_t* out)
{
    std::lock_guard<std::mutex> g(mu);
    if (!port(hw)) return SAI_STATUS_INVALID_PORT_NUMBER;
    for (uint32_t i = 0; i < n; i++) {
        if (!fake_pg_stat(ids[i])) return SAI_STATUS_NOT_SUPPORTED;
        auto it = pg_stats.find({{hw, pg}, ids[i]});
        out[i] = it == pg_stats.end() ? 0 : it->second;
    }
    return SAI_STATUS_SUCCESS;
}
sai_status_t FakeBackend::pg_stats_clear(int hw, int pg, uint32_t n, const sai_stat_id_t* ids)
{
    std::lock_guard<std::mutex> g(mu);
    for (uint32_t i = 0; i < n; i++) {
        if (!fake_pg_stat(ids[i])) return SAI_STATUS_NOT_SUPPORTED;
        pg_stats.erase({{hw, pg}, ids[i]});
    }
    return SAI_STATUS_SUCCESS;
}
sai_status_t FakeBackend::port_pfc_stat_get(int hw, int prio, bool tx, uint64_t& v)
{
    std::lock_guard<std::mutex> g(mu);
    auto it = pfc_stats.find({hw, prio * 2 + (tx ? 1 : 0)});
    v = it == pfc_stats.end() ? 0 : it->second;
    return SAI_STATUS_SUCCESS;
}
sai_status_t FakeBackend::mirror_create(const MirrorCfg& m, int& id)
{
    std::lock_guard<std::mutex> g(mu);
    if (!port(m.monitor)) return SAI_STATUS_INVALID_PORT_NUMBER;
    id = next_qos_++;
    mirrors[id] = m;
    return SAI_STATUS_SUCCESS;
}
sai_status_t FakeBackend::mirror_update(int id, const MirrorCfg& m)
{
    std::lock_guard<std::mutex> g(mu);
    if (!mirrors.count(id)) return SAI_STATUS_ITEM_NOT_FOUND;
    if (!port(m.monitor)) return SAI_STATUS_INVALID_PORT_NUMBER;
    mirrors[id] = m;
    return SAI_STATUS_SUCCESS;
}
sai_status_t FakeBackend::mirror_destroy(int id)
{
    std::lock_guard<std::mutex> g(mu);
    for (auto& p : port_mirrors)
        for (int x : p.second)
            if (x == id) return SAI_STATUS_OBJECT_IN_USE;
    return mirrors.erase(id) ? SAI_STATUS_SUCCESS : SAI_STATUS_ITEM_NOT_FOUND;
}
sai_status_t FakeBackend::port_mirror_set(int hw, bool ingress, const std::vector<int>& ids)
{
    std::lock_guard<std::mutex> g(mu);
    if (!port(hw)) return SAI_STATUS_INVALID_PORT_NUMBER;
    for (int x : ids)
        if (!mirrors.count(x)) return SAI_STATUS_ITEM_NOT_FOUND;
    if (ids.empty()) port_mirrors.erase({hw, ingress});
    else port_mirrors[{hw, ingress}] = ids;
    return SAI_STATUS_SUCCESS;
}
sai_status_t FakeBackend::sample_create(uint32_t rate, int& id)
{
    std::lock_guard<std::mutex> g(mu);
    id = next_qos_++;
    samples[id] = rate;
    return SAI_STATUS_SUCCESS;
}
sai_status_t FakeBackend::sample_update(int id, uint32_t rate)
{
    std::lock_guard<std::mutex> g(mu);
    if (!samples.count(id)) return SAI_STATUS_ITEM_NOT_FOUND;
    samples[id] = rate;
    return SAI_STATUS_SUCCESS;
}
sai_status_t FakeBackend::sample_destroy(int id)
{
    std::lock_guard<std::mutex> g(mu);
    for (auto& p : port_samples)
        if (p.second == id) return SAI_STATUS_OBJECT_IN_USE;
    return samples.erase(id) ? SAI_STATUS_SUCCESS : SAI_STATUS_ITEM_NOT_FOUND;
}
sai_status_t FakeBackend::port_sample_set(int hw, bool ingress, int id)
{
    std::lock_guard<std::mutex> g(mu);
    if (!port(hw)) return SAI_STATUS_INVALID_PORT_NUMBER;
    if (id >= 0 && !samples.count(id)) return SAI_STATUS_ITEM_NOT_FOUND;
    if (id < 0) port_samples.erase({hw, ingress});
    else port_samples[{hw, ingress}] = id;
    return SAI_STATUS_SUCCESS;
}
sai_status_t FakeBackend::genetlink_create(const std::string& name, const std::string& mcgrp, int& id)
{
    std::lock_guard<std::mutex> g(mu);
    if (!mirror_on) return SAI_STATUS_NOT_SUPPORTED;   // with the mirror model only
    id = next_qos_++;
    genls[id] = Genl{name, mcgrp, {}};
    return SAI_STATUS_SUCCESS;
}
sai_status_t FakeBackend::genetlink_destroy(int id)
{
    std::lock_guard<std::mutex> g(mu);
    return genls.erase(id) ? SAI_STATUS_SUCCESS : SAI_STATUS_ITEM_NOT_FOUND;
}
sai_status_t FakeBackend::genetlink_trap_set(int id, int32_t trap_type, bool add)
{
    std::lock_guard<std::mutex> g(mu);
    auto it = genls.find(id);
    if (it == genls.end()) return SAI_STATUS_ITEM_NOT_FOUND;
    if (add) it->second.traps.insert(trap_type);
    else it->second.traps.erase(trap_type);
    return SAI_STATUS_SUCCESS;
}

sai_status_t FakeBackend::hash_set(bool lag, const std::vector<int32_t>& fields, uint32_t seed)
{
    std::lock_guard<std::mutex> g(mu);
    if (!l2x_on) return SAI_STATUS_NOT_SUPPORTED;
    hash[lag] = std::make_pair(fields, seed);
    return SAI_STATUS_SUCCESS;
}
sai_status_t FakeBackend::port_storm_set(int hw, StormKind kind, int policer)
{
    std::lock_guard<std::mutex> g(mu);
    if (!l2x_on) return SAI_STATUS_NOT_SUPPORTED;
    if (!port(hw)) return SAI_STATUS_INVALID_PORT_NUMBER;
    if (policer >= 0 && !policers.count(policer)) return SAI_STATUS_ITEM_NOT_FOUND;
    if (policer < 0) storm.erase({hw, (int)kind});
    else storm[{hw, (int)kind}] = policer;
    return SAI_STATUS_SUCCESS;
}
sai_status_t FakeBackend::vlan_flood_set(uint16_t vid, FloodKind kind, bool on)
{
    std::lock_guard<std::mutex> g(mu);
    if (!l2x_on) return SAI_STATUS_NOT_SUPPORTED;
    if (!vlans.count(vid)) return SAI_STATUS_ITEM_NOT_FOUND;
    flood[{vid, (int)kind}] = on;
    return SAI_STATUS_SUCCESS;
}

} // namespace ob
