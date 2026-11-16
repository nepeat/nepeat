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

// Field processor (ingress): ACL tables and CoPP policing. A group is one
// SDK field group with a qualifier set; an entry is installed at create
// (destroyed and re-created to change it; the SAI layer carries counters
// over). Data and masks in network order like IpAddr.
enum class FpQual : uint8_t {
    InPorts, EtherType, IpType, IpProtocol, SrcIp, DstIp, SrcIp6, DstIp6, L4SrcPort, L4DstPort,
    TcpFlags, IcmpTypeCode, Tos, DstMac, SrcMac, OuterVlan, Ttl, DstCpu, Count
};
// IpType values (as SAI_ACL_IP_TYPE_*): the backend maps them.
struct FpMatch {
    uint32_t present = 0;           // bit (1 << FpQual)
    uint16_t ether = 0, ether_mask = 0;
    int32_t ip_type = 0;            // sai_acl_ip_type_t
    uint8_t proto = 0, proto_mask = 0;
    IpAddr src, src_mask, dst, dst_mask;
    uint16_t l4_src = 0, l4_src_mask = 0, l4_dst = 0, l4_dst_mask = 0;
    uint8_t tcp = 0, tcp_mask = 0;
    uint16_t icmp = 0, icmp_mask = 0;   // type << 8 | code
    uint8_t tos = 0, tos_mask = 0;      // DSCP << 2 | ECN
    uint8_t dmac[6] = {0}, dmac_mask[6] = {0}, smac[6] = {0}, smac_mask[6] = {0};
    uint16_t vlan = 0, vlan_mask = 0;
    uint8_t ttl = 0, ttl_mask = 0;
    std::vector<int> ports;         // InPorts (SDK logical ports); DstCpu: routed/host hit to the CPU egress
    // Only with Backend::fp_supported (FpCap): L4 port ranges (inclusive),
    // egress ports of an egress table entry (OUT_PORTS). Not in present.
    bool l4_src_range = false, l4_dst_range = false;
    uint16_t l4_src_min = 0, l4_src_max = 0, l4_dst_min = 0, l4_dst_max = 0;
    std::vector<int> out_ports;
    bool has(FpQual q) const { return (present >> (int)q) & 1u; }
    void set(FpQual q) { present |= 1u << (int)q; }
};
struct FpRule {
    uint32_t priority = 0;          // within the group, higher wins
    FpMatch m;
    bool drop = false, drop_cancel = false, copy_cpu = false;
    bool counter = false;           // packets + bytes
    int policer = -1;               // backend policer id
    // Policer colours: drop green / yellow / red packets; with cpu_copy_only
    // only their copy to the CPU is cancelled (CoPP: forwarding untouched).
    bool green_drop = false, yellow_drop = false, red_drop = false, cpu_copy_only = false;
    // Mirror sessions (backend ids from mirror_create) the matched packets
    // are mirrored to; only set when the backend has mirror_supported().
    std::vector<int> mirror_ingress, mirror_egress;
    // Only with Backend::fp_supported (FpCap): redirect to a dest (port /
    // lag_dest) or, with redirect_egress, an L3 egress object (next hop);
    // set the traffic class / DSCP (-1: leave).
    int redirect = -1;
    bool redirect_egress = false;
    int set_tc = -1, set_dscp = -1;
};
// Optional field-processor features (Backend::fp_supported).
enum class FpCap : uint8_t { Ranges, Redirect, SetTc, SetDscp, Egress, OutPorts, EntryEnable, EntryUpdate };
// What a table needs beyond its qualifier set (fp_group_create_ex).
struct FpGroupExtra {
    bool egress = false, src_range = false, dst_range = false, out_ports = false;
    bool redirect = false, set_tc = false, set_dscp = false, mirror = false;
    bool any() const { return egress || src_range || dst_range || out_ports || redirect || set_tc || set_dscp || mirror; }
};
// SDK meter. packets: rates in packets/s and bursts in packets, else bytes/s and bytes.
struct PolicerCfg {
    bool tr_tcm = false;            // else sr_tcm (two colours when pbs == 0)
    bool packets = true;
    bool color_blind = false;       // else colour aware (packets arrive green)
    uint64_t cir = 0, cbs = 0, pir = 0, pbs = 0;
};

// A host-interface trap as SAI configures it: the HOSTIF_TRAP and what its
// trap group and that group's policer say (Backend::trap_program).
struct TrapSpec {
    int32_t type = 0;               // sai_hostif_trap_type_t
    int32_t action = 0;             // sai_packet_action_t
    uint32_t priority = 0;          // TRAP_PRIORITY
    uint32_t queue = 0;             // the group's CPU queue
    bool group_up = true;           // the group's ADMIN_STATE
    int policer = -1;               // backend policer id of the group's POLICER, -1: none
    bool yellow_drop = false, red_drop = false;   // the policer's YELLOW/RED_PACKET_ACTION is DROP
};

// ------------------------------------------------------------------ QoS
// QoS maps, schedulers, WRED and buffers (sai_qos.cpp). Optional: with
// qos_supported() false (the default) the SAI layer keeps these objects
// itself, programs nothing and makes none of the QoS calls (the behaviour
// before they existed). Queues and priority groups: index 0..7 per port.
enum class QosMapKind : uint8_t {
    DscpToTc, Dot1pToTc, TcToQueue, TcToPg, PfcPrioToQueue, PfcPrioToPg,
    TcColorToDscp, TcColorToDot1p, DscpToColor, Dot1pToColor, Count
};
// One map entry: key -> value; colours are SAI_PACKET_COLOR_* (the
// TC_AND_COLOR keys, the *_TO_COLOR values). The SAI layer passes complete
// tables: every key the map type has (64 DSCPs, 8 TCs/priorities), missing
// keys mapped to 0 / green.
struct QosMapEntry { uint32_t key = 0, key_color = 0, value = 0, value_color = 0; };
// A scheduler profile (SAI SCHEDULER): on a queue (its scheduler group) or
// a port (port shaper). Rates: bytes/s and bytes, or packets/s and packets.
struct SchedCfg {
    bool strict = false;            // else DWRR / WRR with weight
    uint32_t weight = 1;
    bool packets = false;
    uint64_t min_rate = 0, min_burst = 0, max_rate = 0, max_burst = 0;   // 0: none
};
// A WRED profile: [0] green, [1] yellow, [2] red; thresholds in bytes,
// probability in percent.
struct WredCfg {
    bool enable[3] = {false, false, false};
    uint32_t min[3] = {0, 0, 0}, max[3] = {0, 0, 0}, prob[3] = {100, 100, 100};
    uint8_t weight = 0;             // queue size averaging
    int32_t ecn = 0;                // sai_ecn_mark_mode_t
};
// A buffer profile with what its pool says.
struct BufferCfg {
    bool ingress = false;           // the pool's TYPE
    bool dynamic = true;            // THRESHOLD_MODE (the pool's when the profile has none)
    int8_t dynamic_th = 0;          // alpha = 2^th
    uint64_t static_th = 0, reserved = 0, xoff = 0, xon = 0, xon_offset = 0;   // bytes
    uint64_t pool_size = 0, pool_xoff = 0;
};

// ---------------------------------------------------- mirror, sampling
// Mirror sessions (SPAN, ERSPAN) and SAMPLEPACKET (sai_mirror.cpp).
// Optional like QoS: mirror_supported() false keeps them in the SAI layer.
struct MirrorCfg {
    bool erspan = false;            // ENHANCED_REMOTE (L3 GRE), else LOCAL (SPAN)
    int monitor = -1;               // monitor port (hw)
    uint16_t truncate = 0;          // 0: whole frames
    uint32_t sample_rate = 0;       // 0 / 1: every frame
    // ERSPAN encapsulation
    uint8_t tos = 0, ttl = 255;
    IpAddr src, dst;
    uint8_t smac[6] = {0}, dmac[6] = {0};
    uint16_t gre_type = 0x88be;
    bool vlan = false;              // VLAN_HEADER_VALID
    uint16_t vlan_tpid = 0x8100, vlan_id = 0;
    uint8_t vlan_pri = 0, vlan_cfi = 0;
};

// Storm control and VLAN flood control kinds (port_storm_set, vlan_flood_set).
enum class StormKind : uint8_t { Broadcast, Multicast, UnknownUnicast };
enum class FloodKind : uint8_t { UnknownUnicast, UnknownMulticast, Broadcast };

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
    // Optional: a backend that steers, queues and polices a trap itself
    // programs it here (remove: take it away). The SAI layer calls it for
    // every change of the trap, its group or the group's policer. The default,
    // NOT_SUPPORTED, keeps the trap_set + CoPP field entry path (sai_acl.cpp).
    virtual sai_status_t trap_program(const TrapSpec& t, bool remove)
    {
        (void)t;
        (void)remove;
        return SAI_STATUS_NOT_SUPPORTED;
    }

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

    // Field processor + meters (see FpRule). fp_group_create: priority orders
    // groups (higher wins on conflicting actions; all groups are looked up).
    virtual sai_status_t fp_group_create(const std::vector<FpQual>& qset, int priority, int& gid) = 0;
    virtual sai_status_t fp_group_destroy(int gid) = 0;
    virtual sai_status_t fp_entry_create(int gid, const FpRule& r, int& eid) = 0;
    virtual sai_status_t fp_entry_destroy(int eid) = 0;
    virtual sai_status_t fp_entry_counter(int eid, uint64_t& packets, uint64_t& bytes) = 0;
    // A policer may be attached to several entries (one shared meter).
    virtual sai_status_t policer_create(const PolicerCfg& cfg, int& pid) = 0;
    virtual sai_status_t policer_set(int pid, const PolicerCfg& cfg) = 0;
    virtual sai_status_t policer_destroy(int pid) = 0;

    // QoS (see QosMapKind). id -1: none / default.
    virtual bool qos_supported() const { return false; }
    virtual sai_status_t qos_map_create(QosMapKind, const std::vector<QosMapEntry>&, int&) { return SAI_STATUS_NOT_SUPPORTED; }
    virtual sai_status_t qos_map_update(int, const std::vector<QosMapEntry>&) { return SAI_STATUS_NOT_SUPPORTED; }
    virtual sai_status_t qos_map_destroy(int) { return SAI_STATUS_NOT_SUPPORTED; }
    virtual sai_status_t port_qos_map_set(int, QosMapKind, int) { return SAI_STATUS_NOT_SUPPORTED; }
    virtual sai_status_t port_default_tc_set(int, uint8_t) { return SAI_STATUS_NOT_SUPPORTED; }
    // PFC per priority bit: combined mode passes tx == rx.
    virtual sai_status_t port_pfc_set(int, bool separate, uint8_t tx, uint8_t rx) { (void)separate; (void)tx; (void)rx; return SAI_STATUS_NOT_SUPPORTED; }
    // nullptr: back to the default (DWRR weight 1, no shaping).
    virtual sai_status_t queue_scheduler_set(int, int, const SchedCfg*) { return SAI_STATUS_NOT_SUPPORTED; }
    virtual sai_status_t port_scheduler_set(int, const SchedCfg*) { return SAI_STATUS_NOT_SUPPORTED; }
    virtual sai_status_t wred_create(const WredCfg&, int&) { return SAI_STATUS_NOT_SUPPORTED; }
    virtual sai_status_t wred_update(int, const WredCfg&) { return SAI_STATUS_NOT_SUPPORTED; }
    virtual sai_status_t wred_destroy(int) { return SAI_STATUS_NOT_SUPPORTED; }
    virtual sai_status_t queue_wred_set(int, int, int) { return SAI_STATUS_NOT_SUPPORTED; }
    // Buffer profiles on a queue / an ingress priority group (nullptr: default).
    virtual sai_status_t queue_buffer_set(int, int, const BufferCfg*) { return SAI_STATUS_NOT_SUPPORTED; }
    virtual sai_status_t pg_buffer_set(int, int, const BufferCfg*) { return SAI_STATUS_NOT_SUPPORTED; }
    // Counters by SAI id (SAI_QUEUE_STAT_*, SAI_INGRESS_PRIORITY_GROUP_STAT_*,
    // SAI_BUFFER_POOL_STAT_*); NOT_SUPPORTED for an id the backend lacks.
    virtual sai_status_t queue_stats_get(int, int, uint32_t, const sai_stat_id_t*, uint64_t*) { return SAI_STATUS_NOT_SUPPORTED; }
    virtual sai_status_t queue_stats_clear(int, int, uint32_t, const sai_stat_id_t*) { return SAI_STATUS_NOT_SUPPORTED; }
    virtual sai_status_t pg_stats_get(int, int, uint32_t, const sai_stat_id_t*, uint64_t*) { return SAI_STATUS_NOT_SUPPORTED; }
    virtual sai_status_t pg_stats_clear(int, int, uint32_t, const sai_stat_id_t*) { return SAI_STATUS_NOT_SUPPORTED; }
    virtual sai_status_t pool_stats_get(bool, uint32_t, const sai_stat_id_t*, uint64_t*) { return SAI_STATUS_NOT_SUPPORTED; }
    // PFC frames per priority (SAI_PORT_STAT_PFC_<prio>_RX_PKTS / TX_PKTS).
    virtual sai_status_t port_pfc_stat_get(int, int, bool, uint64_t&) { return SAI_STATUS_NOT_SUPPORTED; }

    // Mirror sessions, port mirroring, sampling.
    virtual bool mirror_supported() const { return false; }
    virtual sai_status_t mirror_create(const MirrorCfg&, int&) { return SAI_STATUS_NOT_SUPPORTED; }
    virtual sai_status_t mirror_update(int, const MirrorCfg&) { return SAI_STATUS_NOT_SUPPORTED; }
    virtual sai_status_t mirror_destroy(int) { return SAI_STATUS_NOT_SUPPORTED; }
    // The sessions a port mirrors to (empty: none), per direction.
    virtual sai_status_t port_mirror_set(int, bool, const std::vector<int>&) { return SAI_STATUS_NOT_SUPPORTED; }
    // SAMPLEPACKET: 1 in rate frames to the CPU (port_sample_set id -1: off).
    virtual sai_status_t sample_create(uint32_t, int&) { return SAI_STATUS_NOT_SUPPORTED; }
    virtual sai_status_t sample_update(int, uint32_t) { return SAI_STATUS_NOT_SUPPORTED; }
    virtual sai_status_t sample_destroy(int) { return SAI_STATUS_NOT_SUPPORTED; }
    virtual sai_status_t port_sample_set(int, bool, int) { return SAI_STATUS_NOT_SUPPORTED; }
    // Generic-netlink host interface (sFlow samples to "psample") and the
    // traps whose packets go there (HOSTIF_TABLE_ENTRY channel GENETLINK).
    virtual sai_status_t genetlink_create(const std::string&, const std::string&, int&) { return SAI_STATUS_NOT_SUPPORTED; }
    virtual sai_status_t genetlink_destroy(int) { return SAI_STATUS_NOT_SUPPORTED; }
    virtual sai_status_t genetlink_trap_set(int, int32_t, bool) { return SAI_STATUS_NOT_SUPPORTED; }

    // Optional L2 extras (default NOT_SUPPORTED: the SAI layer keeps the
    // attribute without hardware effect, as before these hooks).
    // The LAG or ECMP hash: SAI_NATIVE_HASH_FIELD_* values and the seed
    // (HASH NATIVE_HASH_FIELD_LIST of SWITCH LAG_HASH / ECMP_HASH, SWITCH
    // LAG_/ECMP_DEFAULT_HASH_SEED).
    // hash_supported(): SWITCH LAG_HASH / ECMP_HASH objects exist (else those
    // attributes stay unsupported and the seeds are stubs, as before).
    virtual bool hash_supported() const { return false; }
    virtual sai_status_t hash_set(bool lag, const std::vector<int32_t>& fields, uint32_t seed)
    {
        (void)lag; (void)fields; (void)seed;
        return SAI_STATUS_NOT_SUPPORTED;
    }
    // A policer (backend id; -1: off) on one kind of a port's traffic
    // (PORT BROADCAST/MULTICAST/FLOOD_STORM_CONTROL_POLICER_ID).
    virtual sai_status_t port_storm_set(int hw, StormKind kind, int policer)
    {
        (void)hw; (void)kind; (void)policer;
        return SAI_STATUS_NOT_SUPPORTED;
    }
    // VLAN flooding of one kind on / off (VLAN *_FLOOD_CONTROL_TYPE ALL / NONE).
    virtual sai_status_t vlan_flood_set(uint16_t vid, FloodKind kind, bool on)
    {
        (void)vid; (void)kind; (void)on;
        return SAI_STATUS_NOT_SUPPORTED;
    }

    // ---- Optional L3 / counter / ACL extras. Each default keeps the
    // behaviour from before the hook existed.
    // Router interface counters (SAI_ROUTER_INTERFACE_STAT_*) of an L3
    // interface (l3_intf_create's id); clear: read and reset.
    virtual bool l3_intf_stat_supported(sai_stat_id_t id) const { (void)id; return false; }
    virtual sai_status_t l3_intf_stats(int intf_id, uint32_t n, const sai_stat_id_t* ids, uint64_t* out, bool clear)
    {
        (void)intf_id; (void)n; (void)ids; (void)out; (void)clear;
        return SAI_STATUS_NOT_SUPPORTED;
    }
    // RIF ADMIN_V4/V6_STATE: IPv4 / IPv6 routing on an L3 interface
    // (NOT_SUPPORTED: the SAI layer only stores them).
    virtual sai_status_t l3_intf_admin_set(int intf_id, bool v4, bool v6)
    {
        (void)intf_id; (void)v4; (void)v6;
        return SAI_STATUS_NOT_SUPPORTED;
    }
    // The shared CPU egress of glean: routes to a connected subnet and next
    // hops without a neighbor (NEIGHBOR_MISS). Default: the IP2ME one.
    virtual sai_status_t l3_egress_glean(int& egress_id) { return l3_egress_cpu(egress_id); }
    // Port counters by SAI id beyond PortStat: ids with port_stat_by_id()
    // come from port_stats_by_id, the rest from port_stat_get as before.
    virtual bool port_stat_by_id(sai_stat_id_t id) const { (void)id; return false; }
    virtual sai_status_t port_stats_by_id(int hw, uint32_t n, const sai_stat_id_t* ids, uint64_t* out)
    {
        (void)hw; (void)n; (void)ids; (void)out;
        return SAI_STATUS_NOT_SUPPORTED;
    }
    // Field-processor extras (FpCap, FpGroupExtra, the FpMatch / FpRule
    // members marked "only with fp_supported").
    virtual bool fp_supported(FpCap cap) const { (void)cap; return false; }
    virtual sai_status_t fp_group_create_ex(const std::vector<FpQual>& qset, int priority, const FpGroupExtra& x, int& gid)
    {
        if (x.any()) return SAI_STATUS_NOT_SUPPORTED;
        return fp_group_create(qset, priority, gid);
    }
    // EntryUpdate: change an installed entry in place (its counter keeps
    // counting); EntryEnable: switch it off / on without removing it.
    virtual sai_status_t fp_entry_update(int eid, const FpRule& r)
    {
        (void)eid; (void)r;
        return SAI_STATUS_NOT_SUPPORTED;
    }
    virtual sai_status_t fp_entry_enable(int eid, bool on)
    {
        (void)eid; (void)on;
        return SAI_STATUS_NOT_SUPPORTED;
    }
    // Free entries in a group (ACL_TABLE AVAILABLE_ACL_ENTRY).
    virtual sai_status_t fp_group_free(int gid, uint32_t& entries)
    {
        (void)gid; (void)entries;
        return SAI_STATUS_NOT_SUPPORTED;
    }
};

// Factories. make_openbcm_backend() returns nullptr when this libsai was
// built without the SDK (fake-only build).
Backend* make_fake_backend();
Backend* make_openbcm_backend();

} // namespace ob
