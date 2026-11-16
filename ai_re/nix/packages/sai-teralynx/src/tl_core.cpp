// SPDX-License-Identifier: Apache-2.0
// Shared core helpers (tl_core.h).
#include "tl_core.h"

#include <cstdarg>
#include <cstdio>

namespace ob {
namespace tlb {

sai_status_t map_reason(int r)
{
    switch (r) {
    case IFCS_SUCCESS: return SAI_STATUS_SUCCESS;
    case IFCS_UNINIT: return SAI_STATUS_UNINITIALIZED;
    case IFCS_BUSY: return SAI_STATUS_OBJECT_IN_USE;
    case IFCS_MEMORY: return SAI_STATUS_NO_MEMORY;
    case IFCS_PARAM: case IFCS_INVAL: case IFCS_CONFIG: case IFCS_LENGTH_MISMATCH: return SAI_STATUS_INVALID_PARAMETER;
    case IFCS_EXIST: case IFCS_MULTI_EXIST: return SAI_STATUS_ITEM_ALREADY_EXISTS;
    case IFCS_NOTFOUND: return SAI_STATUS_ITEM_NOT_FOUND;
    case IFCS_UNSUPPORTED: return SAI_STATUS_NOT_SUPPORTED;
    case IFCS_RESOURCE_FULL: return SAI_STATUS_TABLE_FULL;
    default: return SAI_STATUS_FAILURE;
    }
}

uint32_t speed_from_enum(uint32_t e)
{
    switch (e) {
    case IFCS_DEVPORT_SPEED_10G: return 10000;
    case IFCS_DEVPORT_SPEED_25G: return 25000;
    case IFCS_DEVPORT_SPEED_40G: return 40000;
    case IFCS_DEVPORT_SPEED_50G: return 50000;
    case IFCS_DEVPORT_SPEED_100G: return 100000;
    case IFCS_DEVPORT_SPEED_200G: return 200000;
    case IFCS_DEVPORT_SPEED_400G: return 400000;
    default: return 0;
    }
}

std::string Core::key(const char* k, const char* def) const
{
    auto it = cfg.profile.find(k);
    if (it != cfg.profile.end()) return it->second;
    return profile_get(k, def);   // the frontend's fallback key list lacks ours
}

const char* Core::reason(int r) const
{
    static const char* names[] = {"SUCCESS", "UNINIT", "BUSY", "CONFIG", "MEMORY", "PARAM", "EXIST", "MULTI_EXIST",
                                  "ENTRY_REPLACED", "LENGTH_MISMATCH", "NOTFOUND", "INVAL", "UNSUPPORTED",
                                  "RESOURCE_FULL", "TIMEOUT", "DEVICE_ACCESS_ERROR", "DRIVER_ERROR"};
    return r >= 0 && r <= 16 ? names[r] : "?";
}

sai_status_t Core::fail(int r, const char* what)
{
    OB_LOG(SAI_API_UNSPECIFIED, r == IFCS_UNSUPPORTED ? SAI_LOG_LEVEL_INFO : SAI_LOG_LEVEL_ERROR, "ifcs %s: %s",
           what, reason(r));
    return map_reason(r);
}

sai_status_t Core::call(ifcs_status_t rc, const char* what)
{
    int r = IFCS_STATUS_REASON(rc);
    return r == IFCS_SUCCESS ? SAI_STATUS_SUCCESS : fail(r, what);
}

void Core::log_once(const std::string& k, const char* fmt, ...)
{
    Guard g(mu);
    if (!logged.insert(k).second) return;
    char buf[512];
    va_list ap;
    va_start(ap, fmt);
    std::vsnprintf(buf, sizeof(buf), fmt, ap);
    va_end(ap);
    OB_NOTICE(SAI_API_SWITCH, "%s", buf);
}

sai_status_t Core::not_impl(const char* what)
{
    Guard g(mu);
    if (logged.insert(std::string("ni:") + what).second)
        OB_WARN(SAI_API_SWITCH, "teralynx backend: %s is NOT_IMPLEMENTED (see sai-teralynx README)", what);
    return SAI_STATUS_NOT_IMPLEMENTED;
}

uint32_t Core::usage_free(ifcs_status_t rc, const ifcs_usage_t& u, uint32_t fallback, const char* what)
{
    if (IFCS_STATUS_REASON(rc) == IFCS_SUCCESS) return u.max > u.current ? u.max - u.current : 0;
    log_once(std::string("usage:") + what, "%s usage unreadable; reporting %u free", what, fallback);
    return fallback;
}

const tl::PortSpec* Core::front(int hw) const
{
    for (auto& s : specs)
        if ((int)s.devport == hw) return &s;
    return nullptr;
}

sai_status_t Core::dp_set(int hw, ifcs_attr_t a, const char* what)
{
    return call(L.ifcs_devport_attr_set(NODE, (ifcs_devport_t)hw, 1, &a), what);
}

sai_status_t Core::dp_get_u32(int hw, uint32_t id, uint32_t& v)
{
    ifcs_attr_t a = attr_of(id);
    uint32_t n = 0;
    int r = IFCS_STATUS_REASON(L.ifcs_devport_attr_get(NODE, (ifcs_devport_t)hw, 1, &a, &n));
    if (r != IFCS_SUCCESS) return fail(r, "devport attr get");
    v = a.value.u32;
    return SAI_STATUS_SUCCESS;
}

sai_status_t Core::sp_set(int hw, ifcs_attr_t a, const char* what)
{
    auto it = sysport.find(hw);
    if (it == sysport.end()) return SAI_STATUS_INVALID_PORT_NUMBER;
    return call(L.ifcs_sysport_attr_set(NODE, it->second, 1, &a), what);
}

} // namespace tlb
} // namespace ob
