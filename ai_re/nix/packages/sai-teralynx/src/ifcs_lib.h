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

// dlopen(path, RTLD_NOW | RTLD_GLOBAL) once per process (never dlclose'd:
// libifcs starts threads) and resolve every symbol. false + err naming the
// first missing symbol or the dlopen error.
// trace (SAI_TERALYNX_IFCS_TRACE, optional): the oracle's call tracer
// (oracle/, libifcs-trace-ours.so): dlopen'ed after libifcs, bound to it
// (ifcs_trace_bind), and every symbol it exports replaces the real one in
// the table. Missing or unbound: an error (no silent untraced run).
bool ifcs_lib_load(const std::string& path, IfcsLib& lib, std::string& err, const std::string& trace = "");

} // namespace tl
