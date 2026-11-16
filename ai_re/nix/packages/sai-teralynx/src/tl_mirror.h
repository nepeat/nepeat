// SPDX-License-Identifier: Apache-2.0
// Mirror sessions and sampling on IFCS (Backend mirror calls, backend.h),
// the way isai 6.2.0 programs this chip (oracle, sai-oracle.md "Mirror"):
//  - a session = a dup_copy_dest_port (DCDP) on the monitor port's
//    sysport, plus per direction in use a collector (SPAN: type SPAN;
//    ERSPAN: ERSPAN_TYPE_II with MACs, IPs, DSCP = TOS >> 2, TTL) in a
//    collector set of the same type; a session change re-creates the
//    collector inside the same set (sysport policies and ACL actions keep
//    pointing at the set);
//  - port mirroring: sysport MIRROR_INGRESS / MIRROR_EGRESS =
//    traffic-monitor policy {ENABLE, the set, container 0, rate 1}, off =
//    {DISABLE, the same set}; one session per port and direction;
//  - SAMPLEPACKET: one sFlow collector (type SFLOW, DCDP = the CPU
//    sysport, isai's tag values) and set per direction for all ports;
//    sysport SFLOW_INGRESS = {ENABLE, the set, container 1, rate}; egress
//    sampling needs the node's EGRESS_SFLOW_ENABLE (off in this init config:
//    isai's set fails the same way), so it is refused;
//  - genetlink (psample) host interfaces: 0.12.41 has no ifcs_netlink_*,
//    so NOT_SUPPORTED.
// collector_set() is for the ACL area (ACL mirror actions): the set of a
// session and direction, created on first use and counted.
#pragma once

#include "tl_core.h"

namespace ob {
namespace tlb {

class MirrorArea {
public:
    explicit MirrorArea(Core& c) : c_(c) {}

    sai_status_t create(const MirrorCfg& m, int& id);
    sai_status_t update(int id, const MirrorCfg& m);
    sai_status_t destroy(int id);
    sai_status_t port_set(int hw, bool ingress, const std::vector<int>& ids);
    sai_status_t sample_create(uint32_t rate, int& id);
    sai_status_t sample_update(int id, uint32_t rate);
    sai_status_t sample_destroy(int id);
    sai_status_t port_sample_set(int hw, bool ingress, int id);
    // ACL mirror actions (tl_acl): the collector set of session id in a
    // direction (created on first use); release when the action goes.
    sai_status_t collector_set(int id, bool ingress, ifcs_handle_t& set);
    void collector_set_release(int id, bool ingress);
    std::string show();                                  // diag "mirror"

private:
    struct Dir { ifcs_handle_t collector = IFCS_NULL_HANDLE, set = IFCS_NULL_HANDLE; int refs = 0; };
    struct Session { MirrorCfg cfg; ifcs_handle_t dcdp = IFCS_NULL_HANDLE; Dir dir[2]; };   // [0] ingress, [1] egress
    struct Sampler { Dir dir[2]; ifcs_handle_t dcdp = IFCS_NULL_HANDLE; };

    sai_status_t dcdp_create(int hw, ifcs_handle_t& h);
    sai_status_t dcdp_get(int hw, ifcs_handle_t& h);   // shared per monitor port (refcounted)
    void dcdp_put(int hw);
    sai_status_t collector_make(const Session& s, bool ingress, ifcs_handle_t& h);
    sai_status_t dir_get(Session& s, bool ingress);
    void dir_put(Session& s, bool ingress);
    sai_status_t policy_set(int hw, uint32_t attr, bool enable, ifcs_handle_t set, int container, uint32_t rate);
    sai_status_t sflow_get(bool ingress, ifcs_handle_t& set);
    void sflow_put(bool ingress);

    Core& c_;
    int next_ = 1;
    std::map<int, Session> sessions_;
    std::map<std::pair<int, bool>, int> port_session_;   // (hw, ingress) -> session
    std::map<int, uint32_t> samples_;                     // id -> rate
    std::map<std::pair<int, bool>, int> port_sample_;    // (hw, ingress) -> sample id
    Sampler sflow_;
    std::map<int, std::pair<ifcs_handle_t, int>> dcdps_;  // monitor hw (-1 CPU) -> DCDP, users
};

} // namespace tlb
} // namespace ob
