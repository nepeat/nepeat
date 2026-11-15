// SPDX-License-Identifier: Apache-2.0
#include "fake_backend.h"

#include <algorithm>
#include <cstdlib>
#include <cstring>

namespace ob {

static std::vector<uint8_t> macv(const uint8_t m[6]) { return std::vector<uint8_t>(m, m + 6); }

sai_status_t FakeBackend::init(const BackendConfig& cfg, LinkCallback link_cb, FdbCallback fdb_cb)
{
    std::lock_guard<std::mutex> g(mu);
    if (initialized) return SAI_STATUS_FAILURE;
    int n = std::atoi(cfg.get("SAI_OPENBCM_FAKE_PORTS", "32").c_str());
    int lanes = std::atoi(cfg.get("SAI_OPENBCM_FAKE_LANES", "4").c_str());
    long speed = std::atol(cfg.get("SAI_OPENBCM_FAKE_SPEED", "100000").c_str());
    if (n < 1 || n > 256 || lanes < 1 || lanes > 8 || speed <= 0) return SAI_STATUS_INVALID_PARAMETER;
    link_follows_admin_ = cfg.get("SAI_OPENBCM_FAKE_LINK", "admin") != "none";
    link_cb_ = std::move(link_cb);
    fdb_cb_ = std::move(fdb_cb);
    port_state.clear(); vlans.clear(); l2.clear(); netifs.clear(); traps.clear();
    intfs.clear(); stations.clear(); egress.clear(); hosts.clear(); routes.clear();
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
    std::lock_guard<std::mutex> g(mu);
    initialized = false;
    link_cb_ = nullptr;
    fdb_cb_ = nullptr;
}

sai_status_t FakeBackend::run_shell(const std::string& cmd)
{
    std::lock_guard<std::mutex> g(mu);
    shell_log.push_back(cmd);
    return SAI_STATUS_SUCCESS;
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

void FakeBackend::update_link(Port& p, std::vector<std::pair<int, bool>>& changes)
{
    bool up = p.admin && (p.lb != Loopback::None || link_follows_admin_);
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
    if (hw != 0 && !port(hw)) return SAI_STATUS_INVALID_PORT_NUMBER;
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
    Netif n{ifname, hw, vid, {0}, keep_tag, false, next_filter_++};
    std::memcpy(n.mac, mac, 6);
    netif_id = next_netif_++;
    filter_id = n.filter;
    netifs[netif_id] = n;
    return SAI_STATUS_SUCCESS;
}

sai_status_t FakeBackend::netif_destroy(int netif_id, int /*filter_id*/)
{
    std::lock_guard<std::mutex> g(mu);
    if (!netifs.erase(netif_id)) return SAI_STATUS_ITEM_NOT_FOUND;
    return SAI_STATUS_SUCCESS;
}

sai_status_t FakeBackend::netif_carrier_set(const std::string& ifname, bool up)
{
    std::lock_guard<std::mutex> g(mu);
    for (auto& n : netifs)
        if (n.second.name == ifname) { n.second.carrier = up; return SAI_STATUS_SUCCESS; }
    return SAI_STATUS_ITEM_NOT_FOUND;
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

sai_status_t FakeBackend::l3_intf_create(const uint8_t mac[6], uint16_t vid, uint32_t mtu, int& intf_id)
{
    std::lock_guard<std::mutex> g(mu);
    if (!vlans.count(vid)) return SAI_STATUS_INVALID_PARAMETER;
    Intf i{{0}, vid, mtu};
    std::memcpy(i.mac, mac, 6);
    intf_id = next_intf_++;
    intfs[intf_id] = i;
    return SAI_STATUS_SUCCESS;
}

sai_status_t FakeBackend::l3_intf_update(int intf_id, const uint8_t mac[6], uint16_t vid, uint32_t mtu)
{
    std::lock_guard<std::mutex> g(mu);
    auto it = intfs.find(intf_id);
    if (it == intfs.end()) return SAI_STATUS_ITEM_NOT_FOUND;
    std::memcpy(it->second.mac, mac, 6);
    it->second.vid = vid;
    it->second.mtu = mtu;
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
    if (!port(hw)) return SAI_STATUS_INVALID_PORT_NUMBER;
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

sai_status_t FakeBackend::l3_host_add(const IpAddr& ip, int egress_id)
{
    std::lock_guard<std::mutex> g(mu);
    if (!egress.count(egress_id)) return SAI_STATUS_INVALID_PARAMETER;
    if (hosts.count(ip)) return SAI_STATUS_ITEM_ALREADY_EXISTS;
    hosts[ip] = egress_id;
    return SAI_STATUS_SUCCESS;
}

sai_status_t FakeBackend::l3_host_delete(const IpAddr& ip)
{
    std::lock_guard<std::mutex> g(mu);
    if (!hosts.erase(ip)) return SAI_STATUS_ITEM_NOT_FOUND;
    return SAI_STATUS_SUCCESS;
}

sai_status_t FakeBackend::l3_route_add(const IpPrefix& p, int egress_id, bool replace)
{
    std::lock_guard<std::mutex> g(mu);
    if (!egress.count(egress_id)) return SAI_STATUS_INVALID_PARAMETER;
    if (routes.count(p) && !replace) return SAI_STATUS_ITEM_ALREADY_EXISTS;
    if (!routes.count(p) && replace) return SAI_STATUS_ITEM_NOT_FOUND;
    routes[p] = egress_id;
    return SAI_STATUS_SUCCESS;
}

sai_status_t FakeBackend::l3_route_delete(const IpPrefix& p)
{
    std::lock_guard<std::mutex> g(mu);
    if (!routes.erase(p)) return SAI_STATUS_ITEM_NOT_FOUND;
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

} // namespace ob
