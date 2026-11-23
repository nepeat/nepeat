// SPDX-License-Identifier: Apache-2.0
// A recording ob::Backend: forwards every operation to an inner backend and appends one text
// line per call (operation, arguments, returned status and outputs), plus one line per event
// the inner backend raises through the init callbacks. The C++ half of the differential oracle
// (tests/frontend_driver.cpp); the Rust virtual backend writes the same lines.
//
// Line format (the reference, shared with the Rust side):
//   <op>(<arg>=<value>, ...) -> <STATUS>[ <out>=<value>]...      one per backend call
//   event link(hw=<int>, up=<bool>)                               one per link callback
//   event fdb(learned=<bool>, mac=<mac>, vid=<int>, hw_port=<int>)   one per FDB callback
// A call's line is written when it returns, so lines of nested calls (a link gate doing MDIO
// inside port_admin_set) and events raised during a call come before it. Status names are the
// SAI ones without "SAI_STATUS_"; outputs only on SUCCESS; values: decimal integers, true /
// false, quoted strings, lower-case MACs, dotted IPv4, uncompressed lower-case IPv6, prefixes
// as addr/len, enums by their C++ enumerator name, vectors [a,b], structs {field=value,...}.
// The full grammar (every op's argument names, struct field lists, the FpMatch and QoS-map
// forms) is the comment block at the top of recording_backend.cpp.
//
// Queries (answers fixed for the life of the backend: name, is_fake, cpu_port, hardware_info,
// chip_limits, qos_supported, mirror_supported, hash_supported, fp_supported, port_stat_by_id,
// l3_intf_stat_supported) are logged only in Mode::Full.
//
// Test-only: not part of libsai (the library's Makefile rules do not list it). Thread-safe.
#pragma once
#include "backend.h"

#include <cstdint>
#include <memory>
#include <utility>
#include <mutex>
#include <string>
#include <vector>

namespace ob {

// The SAI status name as the call log prints it ("SUCCESS", "INVALID_ATTR_VALUE_2", ...).
std::string sai_status_name(sai_status_t st);

// Where the lines go: may outlive the backend (remove_switch destroys it).
class CallLog {
public:
    void add(std::string line);
    void event(std::string line);   // an event line; counts it
    // The lines recorded since the last take(), in order; empties the log.
    std::vector<std::string> take();
    // Events (link + FDB callbacks) the inner backends have raised so far.
    uint64_t events_raised() const;

private:
    mutable std::mutex mu_;
    std::vector<std::string> lines_;
    uint64_t events_ = 0;
};

class RecordingBackend : public Backend {
public:
    enum class Mode { Changes, Full };
    RecordingBackend(Backend* inner, std::shared_ptr<CallLog> log, Mode mode = Mode::Changes)
        : inner_(inner), log_(std::move(log)), mode_(mode) {}
    ~RecordingBackend() override = default;

    Backend& inner() { return *inner_; }
    CallLog& log() { return *log_; }

    const char* name() const override;
    bool is_fake() const override;
    sai_status_t init(const BackendConfig& cfg, LinkCallback link_cb, FdbCallback fdb_cb) override;
    void shutdown() override;
    sai_status_t run_shell(const std::string& cmd) override;
    sai_status_t run_shell_to(const std::string& cmd, int fd) override;
    int cpu_port() const override;
    std::vector<PortInfo> ports() override;
    std::string hardware_info() override;
    sai_status_t asic_temperatures(std::vector<int>& cur, std::vector<int>& peak) override;
    sai_status_t table_usage(TableUsage& u) override;
    sai_status_t chip_limits(ChipLimits& l) override;

    sai_status_t port_admin_set(int hw, bool up) override;
    sai_status_t port_link_get(int hw, bool& up) override;
    sai_status_t mdio_c45_read(unsigned phy, unsigned dev, unsigned reg, unsigned& val) override;
    sai_status_t mdio_c45_write(unsigned phy, unsigned dev, unsigned reg, unsigned val) override;
    sai_status_t port_link_gate(int hw, std::function<int()> gate) override;
    sai_status_t port_speed_set(int hw, uint32_t mbps) override;
    sai_status_t port_speed_get(int hw, uint32_t& mbps) override;
    sai_status_t port_supported_speeds(int hw, std::vector<uint32_t>& out) override;
    sai_status_t port_frame_max_set(int hw, uint32_t bytes) override;
    sai_status_t port_fec_set(int hw, Fec f) override;
    sai_status_t port_autoneg_set(int hw, bool on) override;
    sai_status_t port_interface_set(int hw, const std::string& name) override;
    sai_status_t port_ifg_set(int hw, uint32_t bits) override;
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

    sai_status_t lag_create(int& tid) override;
    sai_status_t lag_destroy(int tid) override;
    sai_status_t lag_set_members(int tid, const std::vector<LagMember>& members) override;

    sai_status_t l2_add(const uint8_t mac[6], uint16_t vid, int hw, bool is_static, bool discard) override;
    sai_status_t l2_delete(const uint8_t mac[6], uint16_t vid) override;
    sai_status_t l2_flush(int hw, int vid, bool statics, bool dynamics) override;
    sai_status_t l2_age_set(uint32_t seconds) override;

    sai_status_t netif_create(const std::string& ifname, int hw, uint16_t vid, const uint8_t mac[6], bool keep_tag,
                              int& netif_id, int& filter_id) override;
    sai_status_t netif_destroy(int netif_id, int filter_id) override;
    sai_status_t netif_carrier_set(const std::string& ifname, bool up) override;
    sai_status_t netif_set_keep_tag(int netif_id, int hw, bool keep_tag, int& filter_id) override;

    sai_status_t trap_set(TrapKind kind, bool to_cpu) override;
    sai_status_t trap_program(const TrapSpec& t, bool remove) override;

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

    sai_status_t fp_group_create(const std::vector<FpQual>& qset, int priority, int& gid) override;
    sai_status_t fp_group_destroy(int gid) override;
    sai_status_t fp_entry_create(int gid, const FpRule& r, int& eid) override;
    sai_status_t fp_entry_destroy(int eid) override;
    sai_status_t fp_entry_counter(int eid, uint64_t& packets, uint64_t& bytes) override;
    sai_status_t policer_create(const PolicerCfg& cfg, int& pid) override;
    sai_status_t policer_set(int pid, const PolicerCfg& cfg) override;
    sai_status_t policer_destroy(int pid) override;

    bool qos_supported() const override;
    sai_status_t qos_map_create(QosMapKind kind, const std::vector<QosMapEntry>& entries, int& id) override;
    sai_status_t qos_map_update(int id, const std::vector<QosMapEntry>& entries) override;
    sai_status_t qos_map_destroy(int id) override;
    sai_status_t port_qos_map_set(int hw, QosMapKind kind, int id) override;
    sai_status_t port_default_tc_set(int hw, uint8_t tc) override;
    sai_status_t port_pfc_set(int hw, bool separate, uint8_t tx, uint8_t rx) override;
    sai_status_t queue_scheduler_set(int hw, int queue, const SchedCfg* cfg) override;
    sai_status_t port_scheduler_set(int hw, const SchedCfg* cfg) override;
    sai_status_t wred_create(const WredCfg& cfg, int& id) override;
    sai_status_t wred_update(int id, const WredCfg& cfg) override;
    sai_status_t wred_destroy(int id) override;
    sai_status_t queue_wred_set(int hw, int queue, int id) override;
    sai_status_t queue_buffer_set(int hw, int queue, const BufferCfg* cfg) override;
    sai_status_t pg_buffer_set(int hw, int pg, const BufferCfg* cfg) override;
    sai_status_t queue_stats_get(int hw, int queue, uint32_t n, const sai_stat_id_t* ids, uint64_t* out) override;
    sai_status_t queue_stats_clear(int hw, int queue, uint32_t n, const sai_stat_id_t* ids) override;
    sai_status_t pg_stats_get(int hw, int pg, uint32_t n, const sai_stat_id_t* ids, uint64_t* out) override;
    sai_status_t pg_stats_clear(int hw, int pg, uint32_t n, const sai_stat_id_t* ids) override;
    sai_status_t pool_stats_get(bool ingress, uint32_t n, const sai_stat_id_t* ids, uint64_t* out) override;
    sai_status_t port_pfc_stat_get(int hw, int prio, bool tx, uint64_t& v) override;

    bool mirror_supported() const override;
    sai_status_t mirror_create(const MirrorCfg& cfg, int& id) override;
    sai_status_t mirror_update(int id, const MirrorCfg& cfg) override;
    sai_status_t mirror_destroy(int id) override;
    sai_status_t port_mirror_set(int hw, bool ingress, const std::vector<int>& ids) override;
    sai_status_t sample_create(uint32_t rate, int& id) override;
    sai_status_t sample_update(int id, uint32_t rate) override;
    sai_status_t sample_destroy(int id) override;
    sai_status_t port_sample_set(int hw, bool ingress, int id) override;
    sai_status_t genetlink_create(const std::string& name, const std::string& mcgrp, int& id) override;
    sai_status_t genetlink_destroy(int id) override;
    sai_status_t genetlink_trap_set(int id, int32_t trap_type, bool add) override;

    bool hash_supported() const override;
    sai_status_t hash_set(bool lag, const std::vector<int32_t>& fields, uint32_t seed) override;
    sai_status_t port_storm_set(int hw, StormKind kind, int policer) override;
    sai_status_t vlan_flood_set(uint16_t vid, FloodKind kind, bool on) override;

    bool l3_intf_stat_supported(sai_stat_id_t id) const override;
    sai_status_t l3_intf_stats(int intf_id, uint32_t n, const sai_stat_id_t* ids, uint64_t* out, bool clear) override;
    sai_status_t l3_intf_admin_set(int intf_id, bool v4, bool v6) override;
    sai_status_t l3_egress_glean(int& egress_id) override;
    bool port_stat_by_id(sai_stat_id_t id) const override;
    sai_status_t port_stats_by_id(int hw, uint32_t n, const sai_stat_id_t* ids, uint64_t* out) override;
    bool fp_supported(FpCap cap) const override;
    sai_status_t fp_group_create_ex(const std::vector<FpQual>& qset, int priority, const FpGroupExtra& x, int& gid) override;
    sai_status_t fp_entry_update(int eid, const FpRule& r) override;
    sai_status_t fp_entry_enable(int eid, bool on) override;
    sai_status_t fp_group_free(int gid, uint32_t& entries) override;

private:
    void add(std::string line, bool query = false) const;
    std::unique_ptr<Backend> inner_;
    std::shared_ptr<CallLog> log_;
    Mode mode_;
};

} // namespace ob
