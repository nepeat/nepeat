// SPDX-License-Identifier: Apache-2.0
// Field processor (the frontend's ACL tables), policers, and the CoPP ACL:
// the ingress table HostifArea::trap_program installs trap entries in.
// SAI ACL tables (fp groups) are one IFCS node-scope table each, in the
// shape isai 6.2.0 builds them on this chip (oracle acl, sai-oracle.md).
#pragma once

#include "tl_core.h"

namespace ob {
namespace tlb {

// One match of a CoPP rule: VALUE bytes (network order) or an OBJECT handle.
struct CoppMatch {
    uint32_t type = 0;                  // IFCS_ACL_MATCH_TYPE_*
    std::vector<uint8_t> value;
    std::vector<uint8_t> mask;          // SAI ACL entries: MASK (CoPP rules: exact, none)
    ifcs_handle_t object = 0;
};
using CoppRule = std::vector<CoppMatch>;   // all must match (one ACE per rule and IB)
struct CoppActions {
    ifcs_handle_t trap = 0;             // copy to the CPU with this hostif trap (0: no copy)
    bool drop = false;                  // drop the original (SAI TRAP, DROP)
    int policer = -1;                   // backend policer: meter + its colours
    bool yellow_drop = false, red_drop = false;   // policed colours lose their CPU copy
};

class AclArea {
public:
    explicit AclArea(Core& c) : c_(c) {}
    sai_status_t group_create(const std::vector<FpQual>& qset, int priority, int& gid);
    sai_status_t group_destroy(int gid);
    sai_status_t entry_create(int gid, const FpRule& r, int& eid);
    sai_status_t entry_destroy(int eid);
    sai_status_t entry_counter(int eid, uint64_t& packets, uint64_t& bytes);
    sai_status_t policer_create(const PolicerCfg& cfg, int& pid);
    sai_status_t policer_set(int pid, const PolicerCfg& cfg);
    sai_status_t policer_destroy(int pid);

    // CoPP: install rules (one ACE per rule in every inno block's ACL) with
    // actions, at ACE priority prio (lower wins, I); id for removal.
    sai_status_t copp_install(const std::vector<CoppRule>& rules, uint32_t prio, const CoppActions& a, int& id);
    sai_status_t copp_remove(int id);
    // Packets the entries matched (all IBs): total, and per colour.
    sai_status_t copp_counters(int id, uint64_t& packets, uint64_t& green, uint64_t& yellow, uint64_t& red);
    uint32_t copp_prio(uint32_t offset);   // offset into the chip's ACL entry priority range
    int copp_ibs() const { return (int)acls_.size(); }
    void forget();                          // shutdown: SDK state is gone (ifcs_deinit)
    std::string report();                   // diag shell "acl": SAI ACL tables, entries, counters

private:
    struct Install {
        struct PerIb { int ib; ifcs_handle_t counter = 0; std::vector<ifcs_handle_t> aces, matches, actions; };
        std::vector<PerIb> ibs;
        int policer = -1;
    };
    sai_status_t copp_init();
    void copp_rollback();
    sai_status_t meter_for(int pid, int ib, ifcs_handle_t& h);
    void meter_attrs(const PolicerCfg& p, std::vector<ifcs_attr_t>& a);
    sai_status_t install_ib(const std::vector<CoppRule>& rules, uint32_t prio, const CoppActions& a, int ib,
                            Install::PerIb& out);
    void remove_ib(Install::PerIb& p);

    Core& c_;
    std::map<int, PolicerCfg> policers_;
    int next_policer_ = 1;
    // CoPP table
    bool copp_up_ = false, node_scope_ = false;
    ifcs_handle_t mprof_ = 0, aprof_ = 0, table_ = 0;
    std::vector<ifcs_handle_t> acls_;                  // index = inno block
    std::map<std::pair<int, int>, ifcs_handle_t> meters_;   // (policer, ib) -> acl_meter
    std::map<int, Install> installs_;
    int next_install_ = 1;

    // SAI ACL tables (frontend fp groups / entries)
    struct UGroup {
        std::vector<FpQual> qset;
        int priority = 0;
        bool v6 = false, meters = false;
        uint32_t size = 0;
        ifcs_handle_t mprof = 0, aprof = 0, table = 0, acl = 0;
        std::set<int> bound;                // devports the ACL is bound to (acl_port_add)
    };
    struct UEntry {
        int gid = -1;
        FpRule rule;
        bool per_port = false;              // one ACE per port (the entry covers part of the table's ports)
        ifcs_handle_t counter = 0;
        std::vector<ifcs_handle_t> aces, matches, actions;
    };
    bool ugroup_has(const UGroup& g, FpQual q) const;
    sai_status_t uentry_matches(const UGroup& g, const FpRule& r, std::vector<CoppMatch>& out);
    sai_status_t uentry_install(UGroup& g, UEntry& e);
    void uentry_uninstall(UGroup& g, UEntry& e, bool keep_counter);
    sai_status_t ugroup_bind(int gid, UGroup& g);   // bind / unbind ports to the union of the entries'
    sai_status_t umeter_for(int pid, int gid, UGroup& g, ifcs_handle_t& h);
    sai_status_t acl_trap(ifcs_handle_t& h);
    std::set<int> ugroup_ports(int gid) const;
    std::map<int, UGroup> ugroups_;
    std::map<int, UEntry> uentries_;
    std::map<std::pair<int, int>, ifcs_handle_t> umeters_;   // (policer, group) -> acl_meter
    int next_ugroup_ = 1, next_uentry_ = 1;
    ifcs_handle_t acl_trap_ = 0;            // hostif trap of ACL COPY / TRAP actions (queue 0)
};

} // namespace tlb
} // namespace ob
