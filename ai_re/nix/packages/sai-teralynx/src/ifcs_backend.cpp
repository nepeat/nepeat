// SPDX-License-Identifier: Apache-2.0
// The IFCS backend: sai-openbcm's backend interface (backend.h) on the
// Innovium/Marvell IFCS 0.12.41 SDK of the Teralynx 7 (CS8210-32X). The SAI
// frontend is sai-openbcm's, unmodified; this is its "openbcm" backend slot
// (make_openbcm_backend), selected with SAI_OPENBCM_BACKEND=openbcm.
//
// libifcs.so is dlopen'ed at init (SAI_TERALYNX_LIBIFCS); every call goes
// through the IfcsLib table. Call forms follow the vendor's own code (the CLI
// python and sample scripts of the SDK, the innovium.user disassembly); where
// a form is inferred instead, the function says so and README.md lists it.
//
// This file only owns the shared Core and one object per area and maps the
// Backend interface onto them; the work is in tl_<area>.cpp (README.md,
// "Source layout").
#include "tl_acl.h"
#include "tl_core.h"
#include "tl_counters.h"
#include "tl_hostif.h"
#include "tl_l2.h"
#include "tl_l3.h"
#include "tl_lag.h"
#include "tl_mirror.h"
#include "tl_port.h"
#include "tl_qos.h"
#include "tl_switch.h"

namespace ob {

using namespace tlb;

class IfcsBackend : public Backend {
public:
    IfcsBackend()
        : sw_(c_), port_(c_), l2_(c_), l3_(c_), hostif_(c_), lag_(c_), acl_(c_), qos_(c_), mirror_(c_)
    {
        c_.sw = &sw_;
        c_.port = &port_;
        c_.l2 = &l2_;
        c_.l3 = &l3_;
        c_.hostif = &hostif_;
        c_.lag = &lag_;
        c_.acl = &acl_;
        c_.qos = &qos_;
        c_.mirror = &mirror_;
    }
    ~IfcsBackend() override
    {
        if (c_.initialized) sw_.shutdown();
    }
    const char* name() const override { return "teralynx"; }
    bool is_fake() const override { return false; }

    // switch
    sai_status_t init(const BackendConfig& cfg, LinkCallback link_cb, FdbCallback fdb_cb) override
    {
        return sw_.init(cfg, std::move(link_cb), std::move(fdb_cb));
    }
    void shutdown() override { sw_.shutdown(); }
    sai_status_t run_shell(const std::string& cmd) override { return sw_.run_shell(cmd); }
    sai_status_t run_shell_to(const std::string& cmd, int fd) override { return sw_.run_shell_to(cmd, fd); }
    int cpu_port() const override { return CPU_DEVPORT; }
    std::vector<PortInfo> ports() override { return port_.infos(); }
    std::string hardware_info() override { return sw_.hardware_info(); }
    sai_status_t asic_temperatures(std::vector<int>& cur, std::vector<int>& peak) override
    {
        return sw_.asic_temperatures(cur, peak);
    }
    sai_status_t table_usage(TableUsage& u) override { return sw_.table_usage(u); }
    sai_status_t chip_limits(ChipLimits& l) override { return sw_.chip_limits(l); }

    // ports
    sai_status_t port_admin_set(int hw, bool up) override { return port_.admin_set(hw, up); }
    sai_status_t port_link_get(int hw, bool& up) override { return port_.link_get(hw, up); }
    sai_status_t port_speed_set(int hw, uint32_t mbps) override { return port_.speed_set(hw, mbps); }
    sai_status_t port_speed_get(int hw, uint32_t& mbps) override { return port_.speed_get(hw, mbps); }
    sai_status_t port_supported_speeds(int hw, std::vector<uint32_t>& out) override
    {
        return port_.supported_speeds(hw, out);
    }
    sai_status_t port_frame_max_set(int hw, uint32_t bytes) override { return port_.frame_max_set(hw, bytes); }
    sai_status_t port_fec_set(int hw, Fec f) override { return port_.fec_set(hw, f); }
    sai_status_t port_autoneg_set(int hw, bool on) override { return port_.autoneg_set(hw, on); }
    sai_status_t port_loopback_set(int hw, Loopback lb) override { return port_.loopback_set(hw, lb); }
    sai_status_t port_pvid_set(int hw, uint16_t vid) override { return port_.pvid_set(hw, vid); }
    sai_status_t port_learn_set(int hw, Learn mode) override { return port_.learn_set(hw, mode); }
    sai_status_t port_stat_get(int hw, PortStat s, uint64_t& v) override { return port_.stat_get(hw, s, v); }
    sai_status_t port_stat_clear(int hw) override { return port_.stat_clear(hw); }
    bool port_stat_by_id(sai_stat_id_t id) const override { return port_stat_mapped(id); }
    sai_status_t port_stats_by_id(int hw, uint32_t n, const sai_stat_id_t* ids, uint64_t* out) override
    {
        return tlb::port_stats_by_id(c_, hw, n, ids, out);
    }

    // L2
    sai_status_t vlan_create(uint16_t vid) override { return l2_.vlan_create(vid); }
    sai_status_t vlan_destroy(uint16_t vid) override { return l2_.vlan_destroy(vid); }
    sai_status_t vlan_port_add(uint16_t vid, int hw, bool untagged) override { return l2_.vlan_port_add(vid, hw, untagged); }
    sai_status_t vlan_port_remove(uint16_t vid, int hw) override { return l2_.vlan_port_remove(vid, hw); }
    sai_status_t vlan_members(uint16_t vid, std::vector<std::pair<int, bool>>& out) override
    {
        return l2_.vlan_members(vid, out);
    }
    sai_status_t l2_add(const uint8_t mac[6], uint16_t vid, int hw, bool is_static, bool discard) override
    {
        return l2_.l2_add(mac, vid, hw, is_static, discard);
    }
    sai_status_t l2_delete(const uint8_t mac[6], uint16_t vid) override { return l2_.l2_delete(mac, vid); }
    sai_status_t l2_flush(int hw, int vid, bool statics, bool dynamics) override
    {
        return l2_.l2_flush(hw, vid, statics, dynamics);
    }
    sai_status_t l2_age_set(uint32_t seconds) override { return l2_.l2_age_set(seconds); }
    sai_status_t vlan_flood_set(uint16_t vid, FloodKind kind, bool on) override { return l2_.vlan_flood_set(vid, kind, on); }

    // LAG
    sai_status_t lag_create(int& tid) override { return lag_.create(tid); }
    sai_status_t lag_destroy(int tid) override { return lag_.destroy(tid); }
    sai_status_t lag_set_members(int tid, const std::vector<LagMember>& m) override { return lag_.set_members(tid, m); }
    bool hash_supported() const override { return true; }
    sai_status_t hash_set(bool lag, const std::vector<int32_t>& fields, uint32_t seed) override
    {
        return lag_.hash_set(lag, fields, seed);
    }

    // host interface
    sai_status_t netif_create(const std::string& ifname, int hw, uint16_t vid, const uint8_t mac[6], bool keep_tag,
                              int& netif_id, int& filter_id) override
    {
        return hostif_.netif_create(ifname, hw, vid, mac, keep_tag, netif_id, filter_id);
    }
    sai_status_t netif_destroy(int netif_id, int filter_id) override { return hostif_.netif_destroy(netif_id, filter_id); }
    sai_status_t netif_carrier_set(const std::string& ifname, bool up) override
    {
        return hostif_.netif_carrier_set(ifname, up);
    }
    sai_status_t netif_set_keep_tag(int netif_id, int hw, bool keep_tag, int& filter_id) override
    {
        return hostif_.netif_set_keep_tag(netif_id, hw, keep_tag, filter_id);
    }
    sai_status_t trap_set(TrapKind kind, bool to_cpu) override { return hostif_.trap_set(kind, to_cpu); }
    sai_status_t trap_program(const TrapSpec& t, bool remove) override { return hostif_.trap_program(t, remove); }

    // L3
    sai_status_t l3_intf_create(const uint8_t mac[6], uint16_t vid, uint32_t mtu, int vrf, int& intf_id) override
    {
        return l3_.intf_create(mac, vid, mtu, vrf, intf_id);
    }
    sai_status_t l3_intf_update(int intf_id, const uint8_t mac[6], uint16_t vid, uint32_t mtu, int vrf) override
    {
        return l3_.intf_update(intf_id, mac, vid, mtu, vrf);
    }
    sai_status_t l3_intf_destroy(int intf_id) override { return l3_.intf_destroy(intf_id); }
    sai_status_t l3_station_add(const uint8_t mac[6], int& station_id) override { return l3_.station_add(mac, station_id); }
    sai_status_t l3_station_delete(int station_id) override { return l3_.station_delete(station_id); }
    sai_status_t l3_egress_create(int intf_id, const uint8_t dmac[6], int hw, uint16_t vid, int& egress_id) override
    {
        return l3_.egress_create(intf_id, dmac, hw, vid, egress_id);
    }
    sai_status_t l3_egress_update(int egress_id, int intf_id, const uint8_t dmac[6], int hw, uint16_t vid) override
    {
        return l3_.egress_update(egress_id, intf_id, dmac, hw, vid);
    }
    sai_status_t l3_egress_destroy(int egress_id) override { return l3_.egress_destroy(egress_id); }
    sai_status_t l3_egress_cpu(int& egress_id) override { return l3_.egress_cpu(egress_id); }
    sai_status_t l3_egress_drop(int& egress_id) override { return l3_.egress_drop(egress_id); }
    sai_status_t l3_egress_glean(int& egress_id) override { return l3_.egress_glean(egress_id); }
    bool l3_intf_stat_supported(sai_stat_id_t id) const override { return L3Area::intf_stat_supported(id); }
    sai_status_t l3_intf_stats(int intf_id, uint32_t n, const sai_stat_id_t* ids, uint64_t* out, bool clear) override
    {
        return l3_.intf_stats(intf_id, n, ids, out, clear);
    }
    sai_status_t l3_intf_admin_set(int intf_id, bool v4, bool v6) override { return l3_.intf_admin_set(intf_id, v4, v6); }
    sai_status_t l3_host_add(int vrf, const IpAddr& ip, int egress_id) override { return l3_.host_add(vrf, ip, egress_id); }
    sai_status_t l3_host_delete(int vrf, const IpAddr& ip) override { return l3_.host_delete(vrf, ip); }
    sai_status_t l3_route_add(int vrf, const IpPrefix& p, int target, bool ecmp, bool replace) override
    {
        return l3_.route_add(vrf, p, target, ecmp, replace);
    }
    sai_status_t l3_route_delete(int vrf, const IpPrefix& p) override { return l3_.route_delete(vrf, p); }
    sai_status_t l3_ecmp_create(const std::vector<int>& egress, int& ecmp_id) override
    {
        return l3_.ecmp_create(egress, ecmp_id);
    }
    sai_status_t l3_ecmp_update(int ecmp_id, const std::vector<int>& egress) override
    {
        return l3_.ecmp_update(ecmp_id, egress);
    }
    sai_status_t l3_ecmp_destroy(int ecmp_id) override { return l3_.ecmp_destroy(ecmp_id); }

    // field processor / policers
    sai_status_t fp_group_create(const std::vector<FpQual>& qset, int priority, int& gid) override
    {
        return acl_.group_create(qset, priority, gid);
    }
    sai_status_t fp_group_destroy(int gid) override { return acl_.group_destroy(gid); }
    sai_status_t fp_entry_create(int gid, const FpRule& r, int& eid) override { return acl_.entry_create(gid, r, eid); }
    sai_status_t fp_entry_destroy(int eid) override { return acl_.entry_destroy(eid); }
    sai_status_t fp_entry_counter(int eid, uint64_t& packets, uint64_t& bytes) override
    {
        return acl_.entry_counter(eid, packets, bytes);
    }
    sai_status_t policer_create(const PolicerCfg& cfg, int& pid) override { return acl_.policer_create(cfg, pid); }
    sai_status_t policer_set(int pid, const PolicerCfg& cfg) override { return acl_.policer_set(pid, cfg); }
    sai_status_t policer_destroy(int pid) override { return acl_.policer_destroy(pid); }
    // Every FpCap but SET_DSCP: IFCS 0.12.41 refuses that action type in an
    // action profile ("Action not implemented/supported yet 16", chip 2026-11-16).
    bool fp_supported(FpCap k) const override { return k != FpCap::SetDscp; }
    sai_status_t fp_group_create_ex(const std::vector<FpQual>& qset, int priority, const FpGroupExtra& x, int& gid) override
    {
        return acl_.group_create_ex(qset, priority, x, gid);
    }
    sai_status_t fp_entry_update(int eid, const FpRule& r) override { return acl_.entry_update(eid, r); }
    sai_status_t fp_entry_enable(int eid, bool on) override { return acl_.entry_enable(eid, on); }
    sai_status_t fp_group_free(int gid, uint32_t& entries) override { return acl_.group_free(gid, entries); }
    sai_status_t port_storm_set(int hw, StormKind kind, int policer) override { return acl_.storm_set(hw, kind, policer); }

    // QoS / buffers (tl_qos.cpp)
    bool qos_supported() const override { return true; }
    sai_status_t qos_map_create(QosMapKind k, const std::vector<QosMapEntry>& e, int& id) override { return qos_.map_create(k, e, id); }
    sai_status_t qos_map_update(int id, const std::vector<QosMapEntry>& e) override { return qos_.map_update(id, e); }
    sai_status_t qos_map_destroy(int id) override { return qos_.map_destroy(id); }
    sai_status_t port_qos_map_set(int hw, QosMapKind k, int id) override { return qos_.port_map_set(hw, k, id); }
    sai_status_t port_default_tc_set(int hw, uint8_t tc) override { return qos_.default_tc_set(hw, tc); }
    sai_status_t port_pfc_set(int hw, bool separate, uint8_t tx, uint8_t rx) override { return qos_.pfc_set(hw, separate, tx, rx); }
    sai_status_t queue_scheduler_set(int hw, int q, const SchedCfg* s) override { return qos_.queue_sched_set(hw, q, s); }
    sai_status_t port_scheduler_set(int hw, const SchedCfg* s) override { return qos_.port_sched_set(hw, s); }
    sai_status_t wred_create(const WredCfg& w, int& id) override { return qos_.wred_create(w, id); }
    sai_status_t wred_update(int id, const WredCfg& w) override { return qos_.wred_update(id, w); }
    sai_status_t wred_destroy(int id) override { return qos_.wred_destroy(id); }
    sai_status_t queue_wred_set(int hw, int q, int id) override { return qos_.queue_wred_set(hw, q, id); }
    sai_status_t queue_buffer_set(int hw, int q, const BufferCfg* b) override { return qos_.queue_buffer_set(hw, q, b); }
    sai_status_t pg_buffer_set(int hw, int pg, const BufferCfg* b) override { return qos_.pg_buffer_set(hw, pg, b); }
    sai_status_t queue_stats_get(int hw, int q, uint32_t n, const sai_stat_id_t* ids, uint64_t* out) override
    {
        return qos_.queue_stats_get(hw, q, n, ids, out);
    }
    sai_status_t queue_stats_clear(int hw, int q, uint32_t n, const sai_stat_id_t* ids) override
    {
        return qos_.queue_stats_clear(hw, q, n, ids);
    }
    sai_status_t pg_stats_get(int hw, int pg, uint32_t n, const sai_stat_id_t* ids, uint64_t* out) override
    {
        return qos_.pg_stats_get(hw, pg, n, ids, out);
    }
    sai_status_t pg_stats_clear(int hw, int pg, uint32_t n, const sai_stat_id_t* ids) override
    {
        return qos_.pg_stats_clear(hw, pg, n, ids);
    }
    sai_status_t port_pfc_stat_get(int hw, int prio, bool tx, uint64_t& v) override { return qos_.pfc_stat_get(hw, prio, tx, v); }

    // mirror / sampling (tl_mirror.cpp)
    bool mirror_supported() const override { return true; }
    sai_status_t mirror_create(const MirrorCfg& m, int& id) override { return mirror_.create(m, id); }
    sai_status_t mirror_update(int id, const MirrorCfg& m) override { return mirror_.update(id, m); }
    sai_status_t mirror_destroy(int id) override { return mirror_.destroy(id); }
    sai_status_t port_mirror_set(int hw, bool ingress, const std::vector<int>& ids) override
    {
        return mirror_.port_set(hw, ingress, ids);
    }
    sai_status_t sample_create(uint32_t rate, int& id) override { return mirror_.sample_create(rate, id); }
    sai_status_t sample_update(int id, uint32_t rate) override { return mirror_.sample_update(id, rate); }
    sai_status_t sample_destroy(int id) override { return mirror_.sample_destroy(id); }
    sai_status_t port_sample_set(int hw, bool ingress, int id) override { return mirror_.port_sample_set(hw, ingress, id); }

private:
    Core c_;
    SwitchArea sw_;
    PortArea port_;
    L2Area l2_;
    L3Area l3_;
    HostifArea hostif_;
    LagArea lag_;
    AclArea acl_;
    QosArea qos_;
    MirrorArea mirror_;
};

Backend* make_openbcm_backend() { return new IfcsBackend(); }

} // namespace ob
