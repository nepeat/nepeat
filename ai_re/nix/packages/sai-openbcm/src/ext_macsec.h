// SPDX-License-Identifier: Apache-2.0
// External MACsec table (profile key SAI_OPENBCM_EXT_MACSEC_FILE): board data
// for ports whose MACsec engine is an external PHY the SDK has no driver for.
// The SAI MACsec objects (sai_macsec.cpp) are realised by running clause-45
// sequences from this table; the key, SCI, packet number etc. are filled into
// placeholders. This code knows no chip: the board supplies the sequences
// (register recordings with the parameter-dependent words marked).
//
// Text, one item per line, numbers hex unless noted:
//
//   ext-macsec 1                                  first line
//   # comment
//   counters clear-on-read | absolute             how the PHY's counters behave (default clear-on-read:
//                                                 the SAI layer accumulates them)
//   ifg-bits <decimal>                            average inter-frame gap of the ASIC MAC, in bit times,
//                                                 while a port has a MACsec port object (room for the
//                                                 SecTAG + ICV); 0 / absent = leave the gap alone
//   port <first lane, decimal> <phy>              a port that can do MACsec and its PHY's MDIO address word
//   seq <name>                                    starts a sequence; sequences are shared by all ports
//   W p|<phy> <dev> <reg> <value>                 write ("p" = the PHY of the port the sequence runs for)
//   R p|<phy> <dev> <reg>                         read, value ignored
//   E p|<phy> <dev> <reg> <val> [<mask>]          read; (v & mask) != val stops the sequence (fault)
//   P p|<phy> <dev> <reg> <val> <mask> <tries> <interval>
//                                                 read until (v & mask) == val: <tries> reads (decimal, 1..1000),
//                                                 <interval> usec apart (decimal, 0..100000); never: fault
//   X p|<phy> <dev> <reg> <dest>                  read and keep: $<n> (n 0..15: scratch for this run) or
//                                                 <result>[<word>] (16-bit word <word> of a named result,
//                                                 least significant word first, word 0..7)
//   D <usec, decimal>                             delay (0..100000)
//
// <value> is one of
//   hhhh                                          constant
//   $<n>[&<mask>][|<bits>]                        a value kept by X in this run, masked, then OR-ed
//   <byte>.<byte>                                 high byte . low byte; a byte is hh or <param>[<index>]
// Parameters (bytes, index 0 first):
//   key      SAK, 16 or 32 bytes                  hkey   GHASH key AES-ECB(SAK, 0^128), 16 bytes
//   sci      8 bytes, as on the wire              salt   12 bytes (XPN)        ssci  4 bytes (XPN)
//   iv       salt with ssci XOR-ed into bytes 0..3 (the fixed part of the XPN nonce), 12 bytes
//   pn       packet number, 8 bytes, least significant byte first (egress: next PN; ingress: lowest acceptable)
//   window   replay window, 4 bytes, least significant byte first
//
// Sequence names the SAI layer runs (<dir> = egress | ingress, <an> = 0..3, <cipher> =
// gcm-aes-128 | gcm-aes-256 | gcm-aes-xpn-128 | gcm-aes-xpn-256):
//   enable.<dir> / disable.<dir>                  MACSEC_PORT created / removed: the engine out of / into bypass,
//                                                 control frames (EAPOL) in clear, everything else dropped
//   reset.<dir>                                   optional, before enable: empty the engine's SA tables
//   probe.<dir>                                   optional: result bypass[0] != 0 = the engine is in bypass. Read for
//                                                 every port when the switch is created; an engine a previous process
//                                                 left on is reset and disabled (its keys must not outlive it)
//   sa-add.<dir>.<an>.<cipher>                    install an SA; an egress SA becomes the transmitting one
//   sa-add-first.<dir>.<an>.<cipher>              the same when the SC has no installed SA (default: sa-add...)
//       ingress with replay protection: ...<cipher>.replay is used when the table has it
//   sa-del.<dir>.<an> / sa-del-last.<dir>.<an>    remove one; -last when it is the SC's last (default: sa-del...)
//   gate-open.<dir> / gate-close.<dir>            optional: data frames pass through the SC / are dropped
//   pn.<dir>.<an>                                 optional: result pn[0..3] (64 bit) = next / lowest PN
//   stats-sa.<dir>.<an>, stats-flow.<dir>         optional: results named after the SAI statistics, lower case
//                                                 without the SAI_MACSEC_SA_STAT_ / SAI_MACSEC_FLOW_STAT_ prefix
//                                                 (e.g. in_pkts_ok[0..3])
// One secure channel per port and direction (a point-to-point link).
//
// Sequences run under the external PHY table's sequence lock (ext_phy.h), so
// they never interleave with a link poll or an admin sequence. A fault or an
// MDIO error stops the sequence and fails the SAI call. Parameters are checked
// before the first bus access: a sequence is not started with a key that is
// too short. Key bytes are never logged.
#pragma once

#include "backend.h"

#include <cstdint>
#include <functional>
#include <map>
#include <string>
#include <vector>

namespace ob {

struct MsByte {
    bool param = false;
    uint8_t c = 0;
    std::string name;
    unsigned idx = 0;
};
struct MsVal {
    enum Kind { Const, Bytes, Kept } kind = Const;
    unsigned c = 0;                 // Const
    MsByte hi, lo;                  // Bytes
    unsigned kept = 0, mask = 0xffff, bits = 0;   // Kept
};
struct MsOp {
    char op = 0;                    // W R E P X D
    bool port_phy = true;
    unsigned phy = 0, dev = 0, reg = 0;
    MsVal val;                      // W
    unsigned want = 0, mask = 0xffff, tries = 1, interval_us = 0;   // E P
    unsigned usec = 0;              // D
    int kept = -1;                  // X -> $n
    std::string res;                // X -> result
    unsigned res_idx = 0;
    unsigned line = 0;
};
struct MsTable {
    bool clear_on_read = true;
    unsigned ifg_bits = 0;
    std::map<uint32_t, unsigned> ports;                     // first lane -> PHY
    std::map<std::string, std::vector<MsOp>> seqs;
    const std::vector<MsOp>* find(const std::string& name) const
    {
        auto it = seqs.find(name);
        return it == seqs.end() ? nullptr : &it->second;
    }
};
using MsParams = std::map<std::string, std::vector<uint8_t>>;
using MsResult = std::map<std::string, std::vector<uint16_t>>;
struct MsIo {
    std::function<sai_status_t(unsigned phy, unsigned dev, unsigned reg, unsigned& val, const MsOp& op)> rd;
    std::function<sai_status_t(unsigned phy, unsigned dev, unsigned reg, unsigned val)> wr;
    std::function<void(unsigned usec)> delay;
};

// Parse the table text; on error returns false and err names the line.
bool ext_macsec_parse(const std::string& text, MsTable& t, std::string& err);
// Run one sequence for the PHY. Parameters the sequence names must be there
// with enough bytes (checked first: INVALID_PARAMETER, nothing accessed).
sai_status_t ext_macsec_exec(const MsIo& io, const std::vector<MsOp>& ops, unsigned phy, const MsParams& par,
                             MsResult* res, std::string& why);
uint64_t ms_result_u64(const MsResult& r, const std::string& name);
void ms_wipe(MsParams& p);          // zero the bytes, then clear

// ---- the switch's table and the SAI side (sai_macsec.cpp) ----
sai_status_t ext_macsec_load(const BackendConfig& cfg);    // before backend init; unset key: nothing
sai_status_t ext_macsec_attach(Backend& be);                // after backend init: lanes -> ports
void ext_macsec_detach();
bool ext_macsec_active();
std::string ext_macsec_status();                            // diag socket "extmacsec"
// Diag socket "extmacsec run <port> <sequence> [<parameter file>]": one sequence by hand
// (file: lines "<param> <hex>"; root-only, read once). For bring-up and debugging.
std::string ext_macsec_diag_run(const std::string& port, const std::string& seq, const std::string& param_file);

} // namespace ob
