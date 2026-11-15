// SPDX-License-Identifier: Apache-2.0
// Global SAI entry points (sai.h, saiobject.h): initialisation, API query,
// logging, OID queries, capability queries, debug dump.
#include "sai_internal.h"

#include <cstdio>
#include <fstream>
#include <syslog.h>

namespace ob {

// ------------------------------------------------------------------ logging
static std::atomic<int> g_level[SAI_API_MAX + 1];
static std::atomic<bool> g_stderr{false};
static std::once_flag g_level_once;

static void level_init()
{
    std::call_once(g_level_once, [] {
        for (auto& l : g_level) l.store(SAI_LOG_LEVEL_WARN);
    });
}

bool log_enabled(sai_api_t api, sai_log_level_t lvl)
{
    level_init();
    int a = (api >= 0 && api <= SAI_API_MAX) ? api : SAI_API_UNSPECIFIED;
    return lvl >= g_level[a].load();
}

void log_set_level(sai_api_t api, sai_log_level_t lvl)
{
    level_init();
    if (api >= 0 && api <= SAI_API_MAX) g_level[api].store(lvl);
}

void log_configure(bool to_stderr) { g_stderr = to_stderr; }

void log_msg(sai_api_t api, sai_log_level_t lvl, const char* fmt, ...)
{
    static const char* names[] = {"DEBUG", "INFO", "NOTICE", "WARN", "ERROR", "CRITICAL"};
    static const int prio[] = {LOG_DEBUG, LOG_INFO, LOG_NOTICE, LOG_WARNING, LOG_ERR, LOG_CRIT};
    int li = (lvl >= SAI_LOG_LEVEL_DEBUG && lvl <= SAI_LOG_LEVEL_CRITICAL) ? lvl : SAI_LOG_LEVEL_ERROR;
    char buf[1024];
    va_list ap;
    va_start(ap, fmt);
    vsnprintf(buf, sizeof(buf), fmt, ap);
    va_end(ap);
    syslog(prio[li], "sai-openbcm %s api=%d: %s", names[li], (int)api, buf);
    if (g_stderr) fprintf(stderr, "sai-openbcm %s api=%d: %s\n", names[li], (int)api, buf);
}

// ------------------------------------------------------------------ helpers
const StubDef* find_stub(const StubDef* defs, size_t n, sai_attr_id_t id)
{
    for (size_t i = 0; i < n; i++)
        if (defs[i].id == id) return &defs[i];
    return nullptr;
}

void stub_default(const StubDef& d, sai_attribute_value_t& v)
{
    std::memset(&v, 0, sizeof(v));
    switch (d.kind) {
    case Kind::Bool: v.booldata = d.def != 0; break;
    case Kind::U8: v.u8 = (uint8_t)d.def; break;
    case Kind::U16: v.u16 = (uint16_t)d.def; break;
    case Kind::U32: v.u32 = (uint32_t)d.def; break;
    case Kind::U64: v.u64 = d.def; break;
    case Kind::S32: v.s32 = (int32_t)d.def; break;
    case Kind::Oid: v.oid = d.def; break;
    case Kind::Mac: case Kind::Ip: break;
    }
}

sai_status_t attr_backend_status(sai_status_t st, uint32_t i)
{
    if (st == SAI_STATUS_INVALID_PARAMETER) return attr_invalid(i);
    if (st == SAI_STATUS_NOT_SUPPORTED) return attr_not_supported(i);
    if (st == SAI_STATUS_NOT_IMPLEMENTED) return attr_not_implemented(i);
    return st;
}

const sai_attribute_t* find_attr(uint32_t n, const sai_attribute_t* a, sai_attr_id_t id, uint32_t* idx)
{
    if (!a) return nullptr;
    for (uint32_t i = 0; i < n; i++)
        if (a[i].id == id) {
            if (idx) *idx = i;
            return &a[i];
        }
    return nullptr;
}

std::string mac_str(const uint8_t m[6])
{
    char b[18];
    snprintf(b, sizeof(b), "%02x:%02x:%02x:%02x:%02x:%02x", m[0], m[1], m[2], m[3], m[4], m[5]);
    return b;
}

IpAddr to_ip(const sai_ip_address_t& a)
{
    IpAddr r;
    r.v6 = a.addr_family == SAI_IP_ADDR_FAMILY_IPV6;
    if (r.v6) std::memcpy(r.addr, a.addr.ip6, 16);
    else std::memcpy(r.addr, &a.addr.ip4, 4);   // sai_ip4_t is already network order
    return r;
}

IpPrefix to_prefix(const sai_ip_prefix_t& p)
{
    IpPrefix r;
    r.addr.v6 = r.mask.v6 = p.addr_family == SAI_IP_ADDR_FAMILY_IPV6;
    if (r.addr.v6) {
        std::memcpy(r.addr.addr, p.addr.ip6, 16);
        std::memcpy(r.mask.addr, p.mask.ip6, 16);
    } else {
        std::memcpy(r.addr.addr, &p.addr.ip4, 4);
        std::memcpy(r.mask.addr, &p.mask.ip4, 4);
    }
    for (int i = 0; i < 16; i++) r.addr.addr[i] &= r.mask.addr[i];   // canonical key
    return r;
}

sai_ip_address_t from_ip(const IpAddr& a)
{
    sai_ip_address_t r;
    std::memset(&r, 0, sizeof(r));
    r.addr_family = a.v6 ? SAI_IP_ADDR_FAMILY_IPV6 : SAI_IP_ADDR_FAMILY_IPV4;
    if (a.v6) std::memcpy(r.addr.ip6, a.addr, 16);
    else std::memcpy(&r.addr.ip4, a.addr, 4);
    return r;
}

Ctx& ctx()
{
    static Ctx* c = new Ctx();   // never destroyed: SDK threads may outlive static destructors
    return *c;
}

std::string profile_get(const char* key, const char* def)
{
    auto& c = ctx();
    if (c.smt.profile_get_value) {
        const char* v = c.smt.profile_get_value(0, key);
        if (v) return v;
    }
    return def;
}

} // namespace ob

using namespace ob;

extern "C" {

sai_status_t sai_api_initialize(_In_ uint64_t flags, _In_ const sai_service_method_table_t* services)
{
    try {
        std::lock_guard<std::recursive_mutex> g(ctx().mu);
        auto& c = ctx();
        if (c.api_initialized) {
            OB_ERR(SAI_API_UNSPECIFIED, "sai_api_initialize: already initialized");
            return SAI_STATUS_FAILURE;
        }
        if (flags != 0) return SAI_STATUS_INVALID_PARAMETER;
        if (services == nullptr || services->profile_get_value == nullptr) {
            OB_ERR(SAI_API_UNSPECIFIED, "sai_api_initialize: no service method table / profile_get_value");
            return SAI_STATUS_INVALID_PARAMETER;
        }
        c.smt = *services;
        log_configure(profile_get("SAI_OPENBCM_LOG_STDERR", "0") == "1");
        c.api_initialized = true;
        OB_NOTICE(SAI_API_UNSPECIFIED, "sai-openbcm initialized (SAI headers %d.%d.%d)", SAI_MAJOR, SAI_MINOR,
                  SAI_REVISION);
        return SAI_STATUS_SUCCESS;
    } catch (...) {
        return SAI_STATUS_FAILURE;
    }
}

sai_status_t sai_api_uninitialize(void)
{
    try {
        {
            std::lock_guard<std::recursive_mutex> g(ctx().mu);
            if (!ctx().api_initialized) return SAI_STATUS_UNINITIALIZED;
        }
        // Tear down a switch the caller forgot to remove.
        if (ctx().switch_created) switch_api.remove_switch(ctx().switch_oid);
        std::lock_guard<std::recursive_mutex> g(ctx().mu);
        ctx().api_initialized = false;
        std::memset(&ctx().smt, 0, sizeof(ctx().smt));
        return SAI_STATUS_SUCCESS;
    } catch (...) {
        return SAI_STATUS_FAILURE;
    }
}

sai_status_t sai_api_query(_In_ sai_api_t api, _Out_ void** api_method_table)
{
    if (api_method_table == nullptr) return SAI_STATUS_INVALID_PARAMETER;
    if (!ctx().api_initialized) return SAI_STATUS_UNINITIALIZED;
    const void* t = nullptr;
    switch (api) {
    case SAI_API_SWITCH: t = &switch_api; break;
    case SAI_API_PORT: t = &port_api; break;
    case SAI_API_VLAN: t = &vlan_api; break;
    case SAI_API_BRIDGE: t = &bridge_api; break;
    case SAI_API_FDB: t = &fdb_api; break;
    case SAI_API_STP: t = &stp_api; break;
    case SAI_API_HOSTIF: t = &hostif_api; break;
    case SAI_API_QUEUE: t = &queue_api; break;
    case SAI_API_BUFFER: t = &buffer_api; break;
    case SAI_API_VIRTUAL_ROUTER: t = &vr_api; break;
    case SAI_API_ROUTER_INTERFACE: t = &rif_api; break;
    case SAI_API_NEIGHBOR: t = &neighbor_api; break;
    case SAI_API_NEXT_HOP: t = &next_hop_api; break;
    case SAI_API_ROUTE: t = &route_api; break;
    case SAI_API_LAG: t = &lag_api; break;
    case SAI_API_NEXT_HOP_GROUP: t = &nhg_api; break;
    default:
        *api_method_table = nullptr;
        return SAI_STATUS_NOT_IMPLEMENTED;
    }
    *api_method_table = const_cast<void*>(t);
    return SAI_STATUS_SUCCESS;
}

sai_status_t sai_log_set(_In_ sai_api_t api, _In_ sai_log_level_t log_level)
{
    if (api < SAI_API_UNSPECIFIED || api > SAI_API_MAX) return SAI_STATUS_INVALID_PARAMETER;
    if (log_level < SAI_LOG_LEVEL_DEBUG || log_level > SAI_LOG_LEVEL_CRITICAL) return SAI_STATUS_INVALID_PARAMETER;
    log_set_level(api, log_level);
    return SAI_STATUS_SUCCESS;
}

sai_object_type_t sai_object_type_query(_In_ sai_object_id_t object_id) { return oid_type(object_id); }

sai_object_id_t sai_switch_id_query(_In_ sai_object_id_t object_id)
{
    if (oid_type(object_id) == SAI_OBJECT_TYPE_NULL) return SAI_NULL_OBJECT_ID;
    return make_oid(SAI_OBJECT_TYPE_SWITCH, 1, oid_switch(object_id));
}

sai_status_t sai_query_api_version(_Out_ sai_api_version_t* version)
{
    if (!version) return SAI_STATUS_INVALID_PARAMETER;
    *version = SAI_API_VERSION;
    return SAI_STATUS_SUCCESS;
}

sai_status_t sai_dbg_generate_dump(_In_ const char* dump_file_name)
{
    if (!dump_file_name) return SAI_STATUS_INVALID_PARAMETER;
    return guarded(SAI_API_UNSPECIFIED, __func__, [&]() -> sai_status_t {
        auto& c = ctx();
        std::ofstream f(dump_file_name);
        if (!f) return SAI_STATUS_FAILURE;
        f << "sai-openbcm dump\nbackend " << (c.be ? c.be->name() : "none") << "\n";
        f << "switch " << std::hex << c.switch_oid << std::dec << " mac " << mac_str(c.src_mac) << "\n";
        for (auto oid : c.port_list) {
            auto& p = c.ports[oid];
            f << "port " << std::hex << oid << std::dec << " hw " << p.hw << " " << p.sdk_name << " lanes";
            for (auto l : p.lanes) f << " " << l;
            f << " admin " << p.admin << " oper " << p.oper << " pvid " << p.pvid << " mtu " << p.mtu << "\n";
        }
        for (auto& v : c.vlans) f << "vlan " << v.second.vid << " members " << v.second.members.size() << "\n";
        f << "bridge_ports " << c.bridge_ports.size() << " fdb " << c.fdbs.size() << " hostifs " << c.hostifs.size()
          << " traps " << c.traps.size() << " rifs " << c.rifs.size() << " neighbors " << c.neighbors.size()
          << " next_hops " << c.next_hops.size() << " routes " << c.routes.size() << " lags " << c.lags.size()
          << " lag_members " << c.lag_members.size() << " virtual_routers " << c.vrs.size()
          << " next_hop_groups " << c.nhgs.size() << " next_hop_group_members " << c.nhg_members.size() << "\n";
        for (auto& l : c.lags) f << "lag " << std::hex << l.first << std::dec << " trunk " << l.second.tid
                                 << " members " << l.second.members.size() << " pvid " << l.second.pvid << "\n";
        for (auto& h : c.hostifs)
            f << "hostif " << h.second.name << " netif " << h.second.netif << " filter " << h.second.filter << "\n";
        return SAI_STATUS_SUCCESS;
    }, false);
}

sai_status_t sai_object_type_get_availability(_In_ sai_object_id_t, _In_ sai_object_type_t, _In_ uint32_t,
                                              _In_ const sai_attribute_t*, _Out_ uint64_t*)
{
    return SAI_STATUS_NOT_SUPPORTED;
}

sai_status_t sai_query_attribute_capability(_In_ sai_object_id_t switch_id, _In_ sai_object_type_t object_type,
                                            _In_ sai_attr_id_t attr_id,
                                            _Out_ sai_attr_capability_t* attr_capability)
{
    (void)switch_id;
    if (!attr_capability) return SAI_STATUS_INVALID_PARAMETER;
    std::memset(attr_capability, 0, sizeof(*attr_capability));
    for (auto& c : attr_caps())
        if (c.ot == object_type && c.id == attr_id) {
            attr_capability->create_implemented = c.create;
            attr_capability->set_implemented = c.set;
            attr_capability->get_implemented = c.get;
            break;
        }
    return SAI_STATUS_SUCCESS;
}

sai_status_t sai_query_attribute_enum_values_capability(_In_ sai_object_id_t, _In_ sai_object_type_t object_type,
                                                        _In_ sai_attr_id_t attr_id,
                                                        _Inout_ sai_s32_list_t* enum_values_capability)
{
    if (!enum_values_capability) return SAI_STATUS_INVALID_PARAMETER;
    std::vector<int32_t> v;
    if (object_type == SAI_OBJECT_TYPE_PORT && attr_id == SAI_PORT_ATTR_FEC_MODE)
        v = {SAI_PORT_FEC_MODE_NONE, SAI_PORT_FEC_MODE_RS, SAI_PORT_FEC_MODE_FC};
    else if (object_type == SAI_OBJECT_TYPE_PORT && attr_id == SAI_PORT_ATTR_LOOPBACK_MODE)
        v = {SAI_PORT_LOOPBACK_MODE_NONE, SAI_PORT_LOOPBACK_MODE_MAC, SAI_PORT_LOOPBACK_MODE_PHY};
    else if (object_type == SAI_OBJECT_TYPE_HOSTIF && attr_id == SAI_HOSTIF_ATTR_TYPE)
        v = {SAI_HOSTIF_TYPE_NETDEV};
    else if (object_type == SAI_OBJECT_TYPE_ROUTER_INTERFACE && attr_id == SAI_ROUTER_INTERFACE_ATTR_TYPE)
        v = {SAI_ROUTER_INTERFACE_TYPE_PORT, SAI_ROUTER_INTERFACE_TYPE_VLAN, SAI_ROUTER_INTERFACE_TYPE_LOOPBACK};
    else
        return SAI_STATUS_NOT_SUPPORTED;
    return fill_list(*enum_values_capability, v);
}

sai_status_t sai_query_stats_capability(_In_ sai_object_id_t, _In_ sai_object_type_t object_type,
                                        _Inout_ sai_stat_capability_list_t* stats_capability)
{
    if (!stats_capability) return SAI_STATUS_INVALID_PARAMETER;
    if (object_type != SAI_OBJECT_TYPE_PORT) return SAI_STATUS_NOT_SUPPORTED;
    std::vector<sai_stat_capability_t> v;
    for (uint32_t id = 0; id < 0x100; id++)
        if (port_stat_supported(id)) {
            sai_stat_capability_t s;
            s.stat_enum = id;
            s.stat_modes = SAI_STATS_MODE_READ;
            v.push_back(s);
        }
    return fill_list(*stats_capability, v);
}

sai_status_t sai_get_maximum_attribute_count(_In_ sai_object_id_t, _In_ sai_object_type_t, _Out_ uint32_t*)
{
    return SAI_STATUS_NOT_SUPPORTED;
}

sai_status_t sai_get_object_count(_In_ sai_object_id_t, _In_ sai_object_type_t, _Out_ uint32_t*)
{
    return SAI_STATUS_NOT_SUPPORTED;
}

sai_status_t sai_get_object_key(_In_ sai_object_id_t, _In_ sai_object_type_t, _Inout_ uint32_t*,
                                _Inout_ sai_object_key_t*)
{
    return SAI_STATUS_NOT_SUPPORTED;
}

} // extern "C"
