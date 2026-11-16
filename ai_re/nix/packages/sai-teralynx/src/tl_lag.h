// SPDX-License-Identifier: Apache-2.0
// LAGs (IFCS lag + members) and the switch hash (IFCS entropy). Call forms
// as isai 6.2.0 makes them on this chip (oracle lag-l2, sai-oracle.md):
//   create      ifcs_lag_create(0, &h, 0, NULL)
//   member      ifcs_lag_member_add(0, lag, 1, [sysport], 0, NULL) /
//               ifcs_lag_member_remove; a member that is both egress- and
//               ingress-disabled is not in the IFCS LAG (isai), its learning
//               is off; egress-only disable keeps it (no IFCS member state)
//   dest        L2 entries, L2VNI members, mc_dests and next hops take the
//               LAG handle where a port takes its sysport
//   hash        ifcs_extended_field_list(ELEMENTS) per packet type on the node
//               ENTROPY_SELECT_* attributes, ENTROPY_SEED (one entropy for LAG
//               and ECMP)
// libifcs refuses a member that is in an L2VNI, an mc_dest or an L2 entry:
// tl_l2 keeps ports out of all three while they are in no VLAN.
#pragma once

#include "tl_core.h"

#include <mutex>

namespace ob {
namespace tlb {

class LagArea {
public:
    explicit LagArea(Core& c) : c_(c) {}
    sai_status_t create(int& tid);
    sai_status_t destroy(int tid);
    sai_status_t set_members(int tid, const std::vector<LagMember>& members);

    // Called with Core::mu held (from L2Area::base_init): forget all LAGs
    // (a new switch), then the profile's hash settings.
    void reset();
    sai_status_t apply_profile();

    // For the other areas (Core::mu held):
    // the LAG (tid) a front devport is a member of in the frontend's view, -1 if none;
    int lag_of_port(int hw) const;
    // the IFCS destination of a backend dest: sysport of a port, LAG handle of lag_dest(tid).
    bool dest_handle(int dest, ifcs_handle_t& h) const;
    // From SDK callbacks (any thread, no Core::mu): the backend dest of an
    // ENTRY_DEST handle (sysport -> devport, or its LAG when the port is an
    // IFCS member; LAG handle -> lag_dest), -1 if unknown.
    int dest_of_handle(ifcs_handle_t h) const;

    // Hash (IFCS_EXTENDED_FIELD_* lists, or names: see parse_fields). The
    // frontend has no hash object yet (sai-features.md, frontend requests):
    // reached through SAI_TERALYNX_HASH_FIELDS / _HASH_SEED at init.
    sai_status_t hash_fields_set(const std::vector<uint32_t>& fields);
    sai_status_t hash_seed_set(uint32_t seed);
    static bool parse_fields(const std::string& s, std::vector<uint32_t>& out);

private:
    struct Lag {
        ifcs_handle_t h = 0;
        std::vector<LagMember> members;   // the frontend's list
        std::set<int> in_ifcs;            // devports that are IFCS members now
    };
    sai_status_t member_add(Lag& l, int hw);
    sai_status_t member_remove(Lag& l, int hw);
    void publish();                        // rebuild the callback map (map_mu_)

    Core& c_;
    std::map<int, Lag> lags_;
    int next_tid_ = 1;
    std::vector<ifcs_handle_t> hash_lists_;   // extended field lists we created
    mutable std::mutex map_mu_;               // guards dest_map_ only; never held across IFCS calls
    std::map<ifcs_handle_t, int> dest_map_;   // LAG handle / member sysport -> lag_dest(tid)
};

} // namespace tlb
} // namespace ob
