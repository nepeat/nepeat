// SPDX-License-Identifier: Apache-2.0
// External PHY table (profile key SAI_OPENBCM_EXT_PHY_FILE): board data for
// front-panel ports whose line side is another chip that the SDK has no
// driver for (a retimer or MACsec PHY between the ASIC and the cage). The
// ASIC port's own link then ends at that chip and says nothing about the
// cable. Per port the table gives up to three short clause-45 sequences:
//
//   link         run from the backend's link scan while the port is admin up;
//                its C lines are the conditions for "line side up". The
//                port's link is then (ASIC link) AND (those conditions).
//   admin-down   run when the port goes admin down (also at switch creation,
//                where every port starts admin down): typically powers the
//                PHY's line side down so the far end sees the link drop.
//   admin-up     run before the ASIC port is enabled.
//
// This code knows no chip. Text, one item per line, numbers hex unless noted
// (the operations are those of src/mdio_script.h plus C):
//
//   ext-phy 1                                      first line
//   # comment
//   poll-ms <decimal>                              line-side poll period, 0..60000 (default 500; 0 = every link scan pass)
//   up-polls <decimal>                             consecutive good polls before "up", 1..100 (default 2)
//   holdoff-ms <decimal>                           pause after a "phy raw" diag-socket command, 0..600000 (default 3000)
//   port <first lane, decimal>                     starts a port (lane = SDK physical port = SONiC lane)
//   link | admin-down | admin-up                   starts a sequence of that port
//   W <phy> <dev> <reg> <val>                      write
//   R <phy> <dev> <reg>                            read, value ignored (clear-on-read)
//   E <phy> <dev> <reg> <val> [<mask>]             read; (v & mask) != val is a FAULT: the sequence stops
//   P <phy> <dev> <reg> <val> <mask> <tries> <interval>
//                                                  read until (v & mask) == val, <tries> reads (decimal, 1..1000)
//                                                  <interval> usec (decimal, 0..100000) apart; never: FAULT
//   C <phy> <dev> <reg> <val> <mask>               link only: condition (v & mask) == val; all must hold
//
// <dev> is 1..31: clause 22 cannot be expressed. A fault or an MDIO error in
// a link sequence counts as "line side down" (logged when it starts and when
// it ends); in an admin sequence it is logged as an error and the port
// carries on (its line side then never reads up, or the far end keeps link).
// Debounce: one failed poll takes the port down at once; coming up needs
// <up-polls> good polls in a row, <poll-ms> apart.
#pragma once

#include "backend.h"

#include <cstdint>
#include <functional>
#include <string>
#include <vector>

namespace ob {

struct ExtOp {
    char op = 0;                    // W R E P C
    unsigned phy = 0, dev = 0, reg = 0, val = 0, mask = 0xffff;
    unsigned tries = 1, interval_us = 0;
    unsigned line = 0;              // table line, for messages
};

struct ExtPort {
    uint32_t lane = 0;
    unsigned line = 0;
    std::vector<ExtOp> link, down, up;
    // ---- runtime ----
    int hw = -1;
    std::string name;
    bool admin = false;             // as the SAI layer last set it
    bool ext_up = false;            // debounced line-side state
    bool fault = false;             // the last link poll ended in a fault
    unsigned good = 0;              // consecutive good polls
    unsigned last_val = 0;          // value of the last C read
    uint64_t next_poll_ms = 0;
    uint64_t polls = 0, faults = 0, downs = 0, ups = 0, hooks = 0, hook_fail = 0;
    uint64_t last_us = 0, max_us = 0, total_us = 0, hook_max_us = 0;
};

struct ExtPhyTable {
    unsigned poll_ms = 500, up_polls = 2, holdoff_ms = 3000;
    std::vector<ExtPort> ports;
};

// Parse the table text; on error returns false and err names the line.
bool ext_phy_parse(const std::string& text, ExtPhyTable& t, std::string& err);

// ---- the switch's table (one per process, like the switch) ----
// Load SAI_OPENBCM_EXT_PHY_FILE (unset: nothing, SUCCESS). Before backend init.
sai_status_t ext_phy_load(const BackendConfig& cfg);
// Resolve lanes to ports and register the link gates. After backend init,
// before the ports are disabled. Unknown lane, or a backend without MDIO /
// link gates: failure (the switch is not created).
sai_status_t ext_phy_attach(Backend& be);
void ext_phy_detach();
bool ext_phy_active();
// The port's admin state changes: run its admin sequence. Call with up=true
// before enabling the ASIC port, with up=false after disabling it.
void ext_phy_admin(int hw, bool up);
// The backend's link scan asks, per pass and port: -1 = no opinion (use the
// ASIC link), 0 = line side down.
int ext_phy_gate(int hw);
// A diag-socket command that does raw MDIO runs under this (it must not land
// inside one of our sequences, nor ours inside a run of the caller's): takes
// the sequence lock for fn and starts the hold-off.
void ext_phy_foreign_mdio(const std::function<void()>& fn);
// Another user of the same PHYs' register interfaces inside libsai (the external
// MACsec table's sequences): runs fn under the sequence lock, without a hold-off.
void ext_phy_locked(const std::function<void()>& fn);
std::string ext_phy_status();        // text table for the diag socket ("extphy")
ExtPhyTable& ext_phy_table();        // tests

} // namespace ob
