// SPDX-License-Identifier: Apache-2.0
// Backend interface: exactly the switch-SDK operations the SAI layer needs.
// Two implementations: "fake" (in-memory, deterministic, no hardware) and
// "openbcm" (Broadcom OpenBCM SDK 6.5.27 via libsdk.so). Everything above
// this interface is hardware-agnostic and is unit-tested against the fake.
//
// Conventions: ports are SDK logical port numbers ("hw" ports), VLANs are
// 802.1Q VIDs, MACs are 6 bytes in network order, IP addresses are in
// network byte order inside IpAddr (as in SAI). Every call returns a
// sai_status_t; the openbcm backend maps BCM_E_* codes (see sdk_glue.c).
//
// Destinations: L2 entries, L2 flushes, L3 egress objects and FDB events
// take a "dest": an SDK logical port, or lag_dest(trunk id) for a LAG.
// Per-port settings (VLAN membership, PVID, learning) stay per physical
// port; the SAI layer applies them to every LAG member.
#pragma once

extern "C" {
#include <sai.h>
}

#include <cstdint>
#include <functional>
#include <map>
#include <string>
#include <vector>

namespace ob {

struct PortInfo {
    int hw = -1;                    // SDK logical port
    std::vector<uint32_t> lanes;    // physical lanes (SDK physical port numbers)
    uint32_t speed = 0;             // Mb/s after SDK init
    std::string name;               // SDK name (ce0, xe12, ...) for logs
    bool autoneg = false;           // as the SDK left it after init
};

struct IpAddr {                     // v4: addr[0..3] used, network order
    bool v6 = false;
    uint8_t addr[16] = {0};
    bool operator<(const IpAddr& o) const;
    bool operator==(const IpAddr& o) const;
};

struct IpPrefix {
    IpAddr addr;
    IpAddr mask;
    bool operator<(const IpPrefix& o) const;
};

enum class Fec { None, Rs, Fc };
enum class Loopback { None, Mac, Phy };
enum class Learn { Hw, Disable, Drop, Cpu };

// Port counters the SAI layer can ask for (mapped to SAI_PORT_STAT_* above).
enum class PortStat {
    InOctets, InUcastPkts, InNonUcastPkts, InDiscards, InErrors, InUnknownProtos,
    InBroadcastPkts, InMulticastPkts, OutOctets, OutUcastPkts, OutNonUcastPkts,
    OutDiscards, OutErrors, OutBroadcastPkts, OutMulticastPkts,
    Count
};

// Host-interface trap kinds the backend may know how to steer to the CPU.
enum class TrapKind { ArpRequest, ArpResponse, Dhcp, Dhcpv6, Igmp, Mld, Nd, Lldp, Lacp, Stp, Other };

constexpr int LAG_DEST_BIT = 0x40000000;   // = OB_LAG_DEST_BIT (sdk_glue.h)
inline int lag_dest(int tid) { return LAG_DEST_BIT | tid; }
inline bool is_lag_dest(int d) { return d >= 0 && (d & LAG_DEST_BIT) != 0; }
inline int lag_tid(int d) { return d & ~LAG_DEST_BIT; }

struct LagMember {
    int hw = -1;
    bool egress_disable = false;    // not in the distribution (hash) set
    bool ingress_disable = false;   // SDK: received as a non-trunk port (not a drop)
};

struct FdbEvent {
    bool learned = true;            // false = aged / deleted by hardware
    uint8_t mac[6] = {0};
    uint16_t vid = 0;
    int hw_port = -1;               // dest (port or lag_dest)
};

struct BackendConfig {
    // Values read from the SAI profile (sai.profile) by the SAI layer.
    std::map<std::string, std::string> profile;
    std::string get(const std::string& k, const std::string& def = "") const {
        auto it = profile.find(k);
        return it == profile.end() ? def : it->second;
    }
};

// Free entries in the forwarding tables, from the backend's own accounting
// (openbcm: bcm_switch_object_count_get / bcm_l3_info). SWITCH AVAILABLE_*.
struct TableUsage {
    uint32_t v4_routes_free = 0;    // LPM, IPv4 prefixes
    uint32_t v6_routes_free = 0;    // LPM, IPv6 prefixes up to /64
    uint32_t hosts_free = 0;        // host table in IPv4 entries (an IPv6 host takes 2)
    uint32_t egress_free = 0;       // L3 egress objects (one per neighbor)
    uint32_t fdb_free = 0;          // L2 table
    uint32_t ecmp_groups_free = 0;  // ECMP (multipath egress) groups
    uint32_t ecmp_members_free = 0; // ECMP member table entries
};

// Fixed properties of the chip.
struct ChipLimits {
    uint64_t buffer_bytes = 0;      // shared packet buffer (MMU)
    uint32_t acl_prio_min = 0;      // field entry priority range (higher wins)
    uint32_t acl_prio_max = 0;
    uint32_t vrf_max = 0;           // highest VRF id (0 = default VRF only)
    uint32_t ecmp_groups = 0;       // ECMP groups
    uint32_t ecmp_paths = 0;        // members per group
    uint32_t ecmp_members = 0;      // member entries across all groups
};

using LinkCallback = std::function<void(int hw_port, bool up)>;
using FdbCallback = std::function<void(const FdbEvent&)>;

class Backend {
public:
    virtual ~Backend() = default;
    virtual const char* name() const = 0;
    virtual bool is_fake() const = 0;

    // Bring the switch up (SDK init) / tear it down. Callbacks may be invoked
    // from SDK threads at any time between init() and shutdown(); they must
    // only queue work (the SAI layer's callbacks do).
    virtual sai_status_t init(const BackendConfig& cfg, LinkCallback link_cb, FdbCallback fdb_cb) = 0;
    virtual void shutdown() = 0;
    virtual sai_status_t run_shell(const std::string& cmd) = 0;   // diag shell command
    // The same with the command's output written to fd (SAI_OPENBCM_DIAG_SOCKET).
    virtual sai_status_t run_shell_to(const std::string& cmd, int fd) = 0;

    virtual int cpu_port() const = 0;
    virtual std::vector<PortInfo> ports() = 0;                   // front panel, SDK order
    virtual std::string hardware_info() = 0;
    // ASIC die temperature monitors, tenths of a degree C (current, and the
    // peak since the previous read). Thread-safe: the temperature publisher
    // calls it without the SAI lock.
    virtual sai_status_t asic_temperatures(std::vector<int>& cur, std::vector<int>& peak) = 0;
    virtual sai_status_t table_usage(TableUsage& u) = 0;
    virtual sai_status_t chip_limits(ChipLimits& l) = 0;

    // ports
    virtual sai_status_t port_admin_set(int hw, bool up) = 0;
    virtual sai_status_t port_link_get(int hw, bool& up) = 0;
    virtual sai_status_t port_speed_set(int hw, uint32_t mbps) = 0;
    virtual sai_status_t port_speed_get(int hw, uint32_t& mbps) = 0;
    virtual sai_status_t port_supported_speeds(int hw, std::vector<uint32_t>& out) = 0;
    virtual sai_status_t port_frame_max_set(int hw, uint32_t bytes) = 0;
    virtual sai_status_t port_fec_set(int hw, Fec f) = 0;
    virtual sai_status_t port_autoneg_set(int hw, bool on) = 0;
    virtual sai_status_t port_loopback_set(int hw, Loopback lb) = 0;
    virtual sai_status_t port_pvid_set(int hw, uint16_t vid) = 0;
    virtual sai_status_t port_learn_set(int hw, Learn mode) = 0;
    virtual sai_status_t port_stat_get(int hw, PortStat s, uint64_t& v) = 0;
    virtual sai_status_t port_stat_clear(int hw) = 0;

    // VLANs. The CPU port is a tagged member of every VLAN the SAI layer
    // creates (so floods reach KNET), never reported as a VLAN member.
    virtual sai_status_t vlan_create(uint16_t vid) = 0;
    virtual sai_status_t vlan_destroy(uint16_t vid) = 0;
    virtual sai_status_t vlan_port_add(uint16_t vid, int hw, bool untagged) = 0;
    virtual sai_status_t vlan_port_remove(uint16_t vid, int hw) = 0;
    // Front-panel members of a VLAN as the SDK has them (for the default VLAN).
    virtual sai_status_t vlan_members(uint16_t vid, std::vector<std::pair<int, bool>>& out) = 0;

    // LAGs (SDK trunks). lag_set_members replaces the whole member list.
    virtual sai_status_t lag_create(int& tid) = 0;
    virtual sai_status_t lag_destroy(int tid) = 0;
    virtual sai_status_t lag_set_members(int tid, const std::vector<LagMember>& members) = 0;

    // L2 table (hw: dest)
    virtual sai_status_t l2_add(const uint8_t mac[6], uint16_t vid, int hw, bool is_static, bool discard) = 0;
    virtual sai_status_t l2_delete(const uint8_t mac[6], uint16_t vid) = 0;
    // dest/vid < 0: any. Static and/or dynamic entries.
    virtual sai_status_t l2_flush(int hw, int vid, bool statics, bool dynamics) = 0;
    virtual sai_status_t l2_age_set(uint32_t seconds) = 0;

    // KNET: one Linux netdev per port (TX straight to the port, RX via a
    // filter on the ingress port).
    virtual sai_status_t netif_create(const std::string& ifname, int hw, uint16_t vid,
                                      const uint8_t mac[6], bool keep_tag,
                                      int& netif_id, int& filter_id) = 0;
    virtual sai_status_t netif_destroy(int netif_id, int filter_id) = 0;
    virtual sai_status_t netif_carrier_set(const std::string& ifname, bool up) = 0;
    // Re-create the RX filter of a netif with/without VLAN tag stripping
    // (the Linux netdev itself stays).
    virtual sai_status_t netif_set_keep_tag(int netif_id, int hw, bool keep_tag, int& filter_id) = 0;

    // Traps: steer a protocol to the CPU (or stop doing so). NOT_SUPPORTED
    // when the backend has no control for that kind (the SAI layer keeps the
    // object and reports it as not programmed).
    virtual sai_status_t trap_set(TrapKind kind, bool to_cpu) = 0;

    // L3 (SDK "egress mode": egress objects referenced by host/route entries).
    // vrf: SDK VRF id, 0 = the default virtual router; an L3 interface
    // binds its VLAN (ingress) to its VRF. Host and route entries are per VRF;
    // egress objects and ECMP groups are not (a route may leak to another VRF).
    virtual sai_status_t l3_intf_create(const uint8_t mac[6], uint16_t vid, uint32_t mtu, int vrf, int& intf_id) = 0;
    virtual sai_status_t l3_intf_update(int intf_id, const uint8_t mac[6], uint16_t vid, uint32_t mtu, int vrf) = 0;
    virtual sai_status_t l3_intf_destroy(int intf_id) = 0;
    virtual sai_status_t l3_station_add(const uint8_t mac[6], int& station_id) = 0;
    virtual sai_status_t l3_station_delete(int station_id) = 0;
    // hw: dest (port or LAG)
    virtual sai_status_t l3_egress_create(int intf_id, const uint8_t dmac[6], int hw, uint16_t vid, int& egress_id) = 0;
    virtual sai_status_t l3_egress_update(int egress_id, int intf_id, const uint8_t dmac[6], int hw, uint16_t vid) = 0;
    virtual sai_status_t l3_egress_destroy(int egress_id) = 0;
    virtual sai_status_t l3_egress_cpu(int& egress_id) = 0;     // shared, never destroyed
    virtual sai_status_t l3_egress_drop(int& egress_id) = 0;    // shared, never destroyed
    virtual sai_status_t l3_host_add(int vrf, const IpAddr& ip, int egress_id) = 0;
    virtual sai_status_t l3_host_delete(int vrf, const IpAddr& ip) = 0;
    // target: an egress object, or an ECMP group when ecmp is set.
    virtual sai_status_t l3_route_add(int vrf, const IpPrefix& p, int target, bool ecmp, bool replace) = 0;
    virtual sai_status_t l3_route_delete(int vrf, const IpPrefix& p) = 0;
    // ECMP groups (SDK multipath egress objects) over egress objects; a
    // repeated egress object weighs more. Never empty: the SAI layer keeps an
    // empty next-hop group without an SDK group.
    virtual sai_status_t l3_ecmp_create(const std::vector<int>& egress, int& ecmp_id) = 0;
    virtual sai_status_t l3_ecmp_update(int ecmp_id, const std::vector<int>& egress) = 0;
    virtual sai_status_t l3_ecmp_destroy(int ecmp_id) = 0;
};

// Factories. make_openbcm_backend() returns nullptr when this libsai was
// built without the SDK (fake-only build).
Backend* make_fake_backend();
Backend* make_openbcm_backend();

} // namespace ob
