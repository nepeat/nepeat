// SPDX-License-Identifier: Apache-2.0
// Shared core of the IFCS backend (sai-teralynx): the loaded libifcs table,
// the lock, the port map and sysport handles, status mapping, attribute
// builders and the small helpers every area uses. One area per file
// (tl_<area>.{h,cpp}); ifcs_backend.cpp owns the Core and the areas and maps
// the frontend's Backend interface onto them. An area reaches another
// through the Core's pointers (c.l2->..., c.hostif->...).
//
// Locking: Core::mu (recursive) guards all backend state; SDK callbacks
// (link scan, L2 events) run on libifcs threads and only read immutable
// state (specs, sysport maps) or atomics before queueing to the frontend.
#pragma once

#include "backend.h"
#include "sai_internal.h"

#include "ifcs_api.h"
#include "ifcs_config.h"
#include "ifcs_lib.h"

#include <cstdint>
#include <cstring>
#include <map>
#include <mutex>
#include <set>
#include <string>
#include <vector>

namespace ob {
namespace tlb {

static constexpr ifcs_node_id_t NODE = 0;
static constexpr int CPU_DEVPORT = 0;

// Documented capacities (devices/cs8210-32x/fib-capacity.md): this SDK
// build has no ILPM, so prefixes live in the 24,576-entry TCAM; profile A
// host and MAC hash sizes. Used where IFCS has no usage call we trust.
static constexpr uint32_t CAP_V4_PREFIX = 24576;
static constexpr uint32_t CAP_V6_PREFIX = 12288;
static constexpr uint32_t CAP_V4_HOST = 98304;
static constexpr uint32_t CAP_MAC = 45056;
static constexpr uint32_t ECMP_PATHS = 64;       // our cap per group (no IFCS limit found)
static constexpr uint32_t VRF_MAX = 255;         // L3VNI values 1..256 (vrf + 1)

class SwitchArea;
class PortArea;
class L2Area;
class L3Area;
class HostifArea;
class LagArea;
class AclArea;
class QosArea;
class MirrorArea;
class SflowArea;

sai_status_t map_reason(int r);
uint32_t speed_from_enum(uint32_t e);

// Attribute builders (zeroed like ctypes ifcs_attr_t(): bitmap 0, value
// assigned directly, as the vendor CLI does).
inline ifcs_attr_t attr_of(uint32_t id)
{
    ifcs_attr_t a;
    std::memset(&a, 0, sizeof(a));
    a.id = id;
    return a;
}
inline ifcs_attr_t attr_u32(uint32_t id, uint32_t v) { ifcs_attr_t a = attr_of(id); a.value.u32 = v; return a; }
inline ifcs_attr_t attr_u16(uint32_t id, uint16_t v) { ifcs_attr_t a = attr_of(id); a.value.u16 = v; return a; }
inline ifcs_attr_t attr_data(uint32_t id, bool v) { ifcs_attr_t a = attr_of(id); a.value.data = v ? IFCS_BOOL_TRUE : IFCS_BOOL_FALSE; return a; }
inline ifcs_attr_t attr_handle(uint32_t id, ifcs_handle_t h) { ifcs_attr_t a = attr_of(id); a.value.handle = h; return a; }
inline ifcs_attr_t attr_mac(uint32_t id, const uint8_t m[6]) { ifcs_attr_t a = attr_of(id); std::memcpy(a.value.mac, m, 6); return a; }
inline ifcs_attr_t attr_fwd(uint32_t id, int action)
{
    ifcs_attr_t a = attr_of(id);
    a.value.fwd_policy.fwd_action = action;
    return a;
}
inline ifcs_attr_t attr_ctc(uint32_t id, bool enable, ifcs_handle_t trap)
{
    ifcs_attr_t a = attr_of(id);
    a.value.ctc_policy.ctc_action = enable ? IFCS_COPY_TO_CPU_ENABLE : IFCS_COPY_TO_CPU_DISABLE;
    a.value.ctc_policy.trap_handle = trap;
    return a;
}

struct Core {
    BackendConfig cfg;
    tl::IfcsLib L;
    std::recursive_mutex mu;
    LinkCallback link;
    FdbCallback fdb;
    bool initialized = false, sdk_up = false;
    std::vector<tl::PortSpec> specs;                  // immutable after init (callbacks read it)
    std::map<int, ifcs_handle_t> sysport;             // devport -> sysport (immutable after init)
    std::map<ifcs_handle_t, int> dev_of_sysport;
    std::set<std::string> logged;

    // The areas (owned by IfcsBackend, which sets these).
    SwitchArea* sw = nullptr;
    PortArea* port = nullptr;
    L2Area* l2 = nullptr;
    L3Area* l3 = nullptr;
    HostifArea* hostif = nullptr;
    LagArea* lag = nullptr;
    AclArea* acl = nullptr;
    QosArea* qos = nullptr;
    MirrorArea* mirror = nullptr;
    SflowArea* sflow = nullptr;      // tl_sflow: samples to psample (genetlink hostifs)

    // Profile value: the frontend's collected keys, else profile_get.
    std::string key(const char* k, const char* def = "") const;
    const char* reason(int r) const;
    sai_status_t fail(int r, const char* what);              // log + map
    sai_status_t call(ifcs_status_t rc, const char* what);   // SUCCESS or fail()
    void log_once(const std::string& k, const char* fmt, ...) __attribute__((format(printf, 3, 4)));
    sai_status_t not_impl(const char* what);                 // NOT_IMPLEMENTED, warned once
    uint32_t usage_free(ifcs_status_t rc, const ifcs_usage_t& u, uint32_t fallback, const char* what);
    const tl::PortSpec* front(int hw) const;                 // front-panel devport of the port map
    sai_status_t dp_set(int hw, ifcs_attr_t a, const char* what);
    sai_status_t dp_get_u32(int hw, uint32_t id, uint32_t& v);
    sai_status_t sp_set(int hw, ifcs_attr_t a, const char* what);
    void sw_limits(ChipLimits& l);                           // SwitchArea::chip_limits (cached node limits)
};

using Guard = std::lock_guard<std::recursive_mutex>;

} // namespace tlb
} // namespace ob
