// SPDX-License-Identifier: Apache-2.0
// In-memory switch model behind the backend interface. Deterministic: no
// timers, no threads; link changes are reported synchronously from the call
// that causes them (or from inject_link()). Profile keys:
//   SAI_OPENBCM_FAKE_PORTS   number of front-panel ports (default 32)
//   SAI_OPENBCM_FAKE_LANES   lanes per port (default 4)
//   SAI_OPENBCM_FAKE_SPEED   initial speed in Mb/s (default 100000)
//   SAI_OPENBCM_FAKE_LINK    "admin": link follows admin state (default);
//                            "none": links never come up
//   SAI_OPENBCM_FAKE_NETDEV  "none" (default): netifs exist only in this model;
//                            "tap": each netif is also a Linux TAP netdev of
//                            that name (needs CAP_NET_ADMIN), carrier = the
//                            hostif's OPER_STATUS, like a KNET netif
//   SAI_OPENBCM_FAKE_WIRE    "<netif>:<ifname>,...": (tap only) frames the
//                            TAP sends go out of host interface <ifname>, and
//                            everything <ifname> receives comes up the TAP,
//                            while the port is admin up: the CPU path of that
//                            port (what KNET carries), no switching. One
//                            thread per wired netif.
// Tables: fixed capacities (FAKE_* in fake_backend.cpp) minus what the model
// holds; chip limits: the Tomahawk's values (VRF and ECMP sizes: fake
// constants of the same order; the openbcm backend reads the SDK's).
// Temperatures: 8 monitors, current 40.0, 41.5, ... 50.5 C, peak = current
// + 0.5 C (test hooks: temps, temp_fail).
#pragma once
#include "backend.h"

#include <memory>
#include <mutex>
#include <set>
#include <thread>
#include <tuple>

namespace ob {

class FakeBackend : public Backend {
public:
    struct Port {
        PortInfo info;
        bool admin = true;          // SDK post-init state: enabled
        bool link = false;
        Fec fec = Fec::None;
        bool autoneg = false;
        Loopback lb = Loopback::None;
        uint16_t pvid = 1;
        uint32_t frame_max = 9412;
        Learn learn = Learn::Hw;
        uint64_t stats[(int)PortStat::Count] = {0};
    };
    struct L2 { int hw; bool is_static; bool discard; };
    struct Netif { std::string name; int hw; uint16_t vid; uint8_t mac[6]; bool keep_tag; bool carrier; int filter; };
    struct Egress { int intf; uint8_t mac[6]; int hw; uint16_t vid; bool cpu; bool drop; };
    struct Intf { uint8_t mac[6]; uint16_t vid; uint32_t mtu; int vrf; };
    using HostKey = std::pair<int, IpAddr>;            // vrf, address
    using RouteKey = std::pair<int, IpPrefix>;         // vrf, prefix

    ~FakeBackend() override;
    const char* name() const override { return "fake"; }
    bool is_fake() const override { return true; }
    sai_status_t init(const BackendConfig& cfg, LinkCallback link_cb, FdbCallback fdb_cb) override;
    void shutdown() override;
    sai_status_t run_shell(const std::string& cmd) override;
    sai_status_t run_shell_to(const std::string& cmd, int fd) override;   // logs it, writes "fake: <cmd>"
    int cpu_port() const override { return 0; }
    std::vector<PortInfo> ports() override;
    std::string hardware_info() override { return "fake"; }
    sai_status_t asic_temperatures(std::vector<int>& cur, std::vector<int>& peak) override;
    sai_status_t table_usage(TableUsage& u) override;
    sai_status_t chip_limits(ChipLimits& l) override;

    sai_status_t lag_create(int& tid) override;
    sai_status_t lag_destroy(int tid) override;
    sai_status_t lag_set_members(int tid, const std::vector<LagMember>& members) override;

    sai_status_t port_admin_set(int hw, bool up) override;
    sai_status_t port_link_get(int hw, bool& up) override;
    sai_status_t port_speed_set(int hw, uint32_t mbps) override;
    sai_status_t port_speed_get(int hw, uint32_t& mbps) override;
    sai_status_t port_supported_speeds(int hw, std::vector<uint32_t>& out) override;
    sai_status_t port_frame_max_set(int hw, uint32_t bytes) override;
    sai_status_t port_fec_set(int hw, Fec f) override;
    sai_status_t port_autoneg_set(int hw, bool on) override;
    sai_status_t port_loopback_set(int hw, Loopback lb) override;
    sai_status_t port_pvid_set(int hw, uint16_t vid) override;
    sai_status_t port_learn_set(int hw, Learn mode) override;
    sai_status_t port_stat_get(int hw, PortStat s, uint64_t& v) override;
    sai_status_t port_stat_clear(int hw) override;

    sai_status_t vlan_create(uint16_t vid) override;
    sai_status_t vlan_destroy(uint16_t vid) override;
    sai_status_t vlan_port_add(uint16_t vid, int hw, bool untagged) override;
    sai_status_t vlan_port_remove(uint16_t vid, int hw) override;
    sai_status_t vlan_members(uint16_t vid, std::vector<std::pair<int, bool>>& out) override;

    sai_status_t l2_add(const uint8_t mac[6], uint16_t vid, int hw, bool is_static, bool discard) override;
    sai_status_t l2_delete(const uint8_t mac[6], uint16_t vid) override;
    sai_status_t l2_flush(int hw, int vid, bool statics, bool dynamics) override;
    sai_status_t l2_age_set(uint32_t seconds) override;

    sai_status_t netif_create(const std::string& ifname, int hw, uint16_t vid, const uint8_t mac[6],
                              bool keep_tag, int& netif_id, int& filter_id) override;
    sai_status_t netif_destroy(int netif_id, int filter_id) override;
    sai_status_t netif_carrier_set(const std::string& ifname, bool up) override;
    sai_status_t netif_set_keep_tag(int netif_id, int hw, bool keep_tag, int& filter_id) override;

    sai_status_t trap_set(TrapKind kind, bool to_cpu) override;

    sai_status_t l3_intf_create(const uint8_t mac[6], uint16_t vid, uint32_t mtu, int vrf, int& intf_id) override;
    sai_status_t l3_intf_update(int intf_id, const uint8_t mac[6], uint16_t vid, uint32_t mtu, int vrf) override;
    sai_status_t l3_intf_destroy(int intf_id) override;
    sai_status_t l3_station_add(const uint8_t mac[6], int& station_id) override;
    sai_status_t l3_station_delete(int station_id) override;
    sai_status_t l3_egress_create(int intf_id, const uint8_t dmac[6], int hw, uint16_t vid, int& egress_id) override;
    sai_status_t l3_egress_update(int egress_id, int intf_id, const uint8_t dmac[6], int hw, uint16_t vid) override;
    sai_status_t l3_egress_destroy(int egress_id) override;
    sai_status_t l3_egress_cpu(int& egress_id) override;
    sai_status_t l3_egress_drop(int& egress_id) override;
    sai_status_t l3_host_add(int vrf, const IpAddr& ip, int egress_id) override;
    sai_status_t l3_host_delete(int vrf, const IpAddr& ip) override;
    sai_status_t l3_route_add(int vrf, const IpPrefix& p, int target, bool ecmp, bool replace) override;
    sai_status_t l3_route_delete(int vrf, const IpPrefix& p) override;
    sai_status_t l3_ecmp_create(const std::vector<int>& egress, int& ecmp_id) override;
    sai_status_t l3_ecmp_update(int ecmp_id, const std::vector<int>& egress) override;
    sai_status_t l3_ecmp_destroy(int ecmp_id) override;

    // ---- test hooks (not part of the backend interface) ----
    void inject_link(int hw, bool up);                 // e.g. cable pulled
    void inject_fdb(const FdbEvent& ev);               // hardware learn/age
    void set_stat(int hw, PortStat s, uint64_t v);
    std::mutex mu;                                     // guards everything below
    std::vector<int> temps;                            // current, tenths of C
    bool temp_fail = false;                            // reads fail (sensor gone)
    std::map<int, std::vector<LagMember>> lags;        // tid -> members
    std::map<int, Port> port_state;
    std::map<uint16_t, std::map<int, bool>> vlans;     // vid -> hw -> untagged (cpu = 0)
    std::map<std::pair<std::vector<uint8_t>, uint16_t>, L2> l2;
    std::map<int, Netif> netifs;
    std::map<TrapKind, bool> traps;
    std::map<int, Intf> intfs;
    std::map<int, std::vector<uint8_t>> stations;
    std::map<int, Egress> egress;
    std::map<HostKey, int> hosts;
    std::map<RouteKey, int> routes;                    // -> egress id, or ECMP id (ecmps)
    std::map<int, std::vector<int>> ecmps;             // ECMP id -> egress ids
    uint32_t age_seconds = 300;
    bool tap_mode = false;                             // SAI_OPENBCM_FAKE_NETDEV=tap
    std::map<std::string, std::string> wire;           // netif name -> host ifname
    std::vector<std::string> shell_log;
    bool initialized = false;

private:
    // A netif's TAP netdev (tap mode) and its wire, see the profile keys.
    struct Tap {
        int fd = -1, wire_fd = -1, stop_rd = -1, stop_wr = -1;
        int hw = -1;
        std::thread pump;
    };
    std::map<int, std::unique_ptr<Tap>> taps_;         // netif id -> TAP (guarded by mu)
    sai_status_t tap_open(const std::string& ifname, int hw, const uint8_t mac[6], std::unique_ptr<Tap>& out);
    void tap_close(std::unique_ptr<Tap>& t);           // call without mu (joins the pump)
    void pump(Tap* t);
    bool port_up(int hw);
    Port* port(int hw);
    bool dest_ok(int d);                               // a port, or an existing LAG
    void update_link(Port& p, std::vector<std::pair<int, bool>>& changes);
    void report(const std::vector<std::pair<int, bool>>& changes);
    LinkCallback link_cb_;
    FdbCallback fdb_cb_;
    bool link_follows_admin_ = true;
    int next_tid_ = 1;
    int next_netif_ = 1, next_filter_ = 2, next_intf_ = 1, next_station_ = 1, next_egress_ = 100002;
    int next_ecmp_ = 200000;                           // the SDK's multipath id range
    int cpu_egress_ = -1, drop_egress_ = -1;
};

} // namespace ob
