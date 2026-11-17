// SPDX-License-Identifier: Apache-2.0
// The vendor libifcs.so, dlopen'ed at backend init (no link-time dependency):
// one function table filled by dlsym from the IFCS_SYMBOLS list.
#pragma once

#include "ifcs_api.h"

#include <string>

namespace tl {

struct IfcsLib {
#define TL_FN_MEMBER(n) n##_fn n = nullptr;
    IFCS_SYMBOLS(TL_FN_MEMBER)
#undef TL_FN_MEMBER
    void* handle = nullptr;
    std::string path;
};

// The learn-ring read guard (src/ifcs_guard.c in the shim; profile keys
// SAI_TERALYNX_IFCS_GUARD, SAI_TERALYNX_LEARN_RING_SIZE, SAI_TERALYNX_IM_MALLOC_LIMIT_MIB).
struct GuardOptions {
    bool enable = true;
    std::string stats_path;              // Prometheus text file, "" = none
    uint32_t ring_size = 0x800;          // learn ring entries (the deployed init yaml: 2048)
    uint64_t malloc_limit = 1ull << 30;  // im_malloc above this is refused; 0 = no limit
};

// dlopen(path, RTLD_NOW | RTLD_GLOBAL) once per process (never dlclose'd:
// libifcs starts threads) and resolve every symbol. false + err naming the
// first missing symbol or the dlopen error.
// trace (SAI_TERALYNX_IFCS_TRACE, optional): the oracle's call tracer
// (oracle/, libifcs-trace-ours.so): dlopen'ed after libifcs, bound to it
// (ifcs_trace_bind), and every symbol it exports replaces the real one in
// the table. Missing or unbound: an error (no silent untraced run).
// guard: with enable and the shim present, libifcs' own calls of
// node_pci_reg_read32 / node_pci_reg_write32 / im_malloc go through the
// shim's guard (GOT entries, like memcpy); a libifcs without those imports
// only logs that the guard is not active.
bool ifcs_lib_load(const std::string& path, IfcsLib& lib, std::string& err, const std::string& trace = "",
                   const GuardOptions& guard = GuardOptions());

} // namespace tl
