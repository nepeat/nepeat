// SPDX-License-Identifier: Apache-2.0
// Parsers for the two text files the board layer hands the IFCS backend
// (profile keys SAI_TERALYNX_PORT_MAP and SAI_TERALYNX_INIT_CMDS). Pure C++,
// no IFCS calls: unit-tested on the build host.
#pragma once

#include <cstdint>
#include <map>
#include <string>
#include <vector>

namespace tl {

// One front-panel (eth) devport: "devport lanes speed_mbps fec".
struct PortSpec {
    uint32_t devport = 0;
    std::vector<uint32_t> lanes;       // serdes_group * 8 + lane_in_group + 1
    uint32_t speed = 0;                // Mb/s
    std::string fec;                   // none | rs | fc
};
// false + err ("file:line: why") on any malformed line or duplicate devport/lane.
bool parse_port_map(const std::string& path, std::vector<PortSpec>& out, std::string& err);

// "ifcs set devport <id> <attr> <value|[v,v,...]>", value converted the way
// the vendor CLI's setter for that attribute does (int(), list of ints).
enum class ValKind { U32, I8List, U8List, U32List };
struct DevportCmd {
    uint32_t devport = 0;
    uint32_t attr = 0;                 // IFCS_DEVPORT_ATTR_*
    std::string name;                  // attribute name as written
    ValKind kind = ValKind::U32;
    std::vector<int64_t> values;       // one value for U32
    std::string where;                 // file:line
};
struct InitCmds {
    std::vector<DevportCmd> cmds;
    std::map<std::string, unsigned> skipped;   // kind ("port enable", "pci write", ...) -> lines
};
// Appends to out. false + err on an unreadable file or a malformed line of a
// known attribute (bad number, out of range, missing value).
bool parse_init_cmds(const std::string& path, InitCmds& out, std::string& err);

// The attribute table (name -> id, kind): what SDK/cel_cmds/{misc_config_32,
// dynamic_misc,dynamic_preemphasis,preemphasis_*}.cmd use.
bool devport_attr_lookup(const std::string& name, uint32_t& id, ValKind& kind);

} // namespace tl
