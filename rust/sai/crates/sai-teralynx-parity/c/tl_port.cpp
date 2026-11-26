// SPDX-License-Identifier: Apache-2.0
// The frozen frontend's op-script driver (ai_re/nix/packages/sai-teralynx/frontend/tests/frontend_driver.cpp,
// compiled UNMODIFIED) as a plain SAI C API client of a Teralynx libsai.so (the C++ sai-teralynx or the
// Rust one) on the mock libifcs (crate sai-teralynx-parity, README there). frontend_driver.cpp reaches
// past the SAI C API through three things, all supplied here instead of the frontend's internals:
//
//   - ob::CallLog (recording_backend.h): the "be" lines of an operation. Here they are the mock's
//     IFCS call lines the operation caused: the tap (tap.c, PARITY_TAP_FILE) appends every mock line
//     to a file; take() returns what was appended since the last take(). events_raised(): the events
//     this port injected into the mock (link / L2 callbacks it fired).
//   - tp:: (test_port.h), the functions the driver calls: events_delivered() counts the port-state and
//     FDB notifications the library delivered (the switch API is wrapped: -Wl,--wrap=sai_api_query,
//     the notification pointers of create_switch / set_switch_attribute go through counters, then to
//     the driver's own callbacks); the injections go to the mock's test hooks (mock_fire_link,
//     mock_fire_l2, mock_fire_l2_dest, mock_set_stat); backend_id of a PORT is its OID index (the
//     frontend's: the backend's hw number = the devport), of a LAG its OID index (lag_dest marks it;
//     the mock handle is the n-th "ifcs_lag_create h=" line of the stream). linkscan, mdio_set and
//     set_feature have no Teralynx counterpart (no link-scan pass, no MDIO through the backend, every
//     area always on): they do nothing.
//   - ob::sai_status_name: the status names of recording_backend.cpp (same table).
//
// The driver's factory (make_openbcm_backend, over the fake backend) is never called (the library
// has its own backend): it and the recorder it names are dropped by --gc-sections.
//   PARITY_TAP_FILE   the tap's stream (the same file the tap writes)
//   PARITY_MOCK       the mock's absolute path (its hooks: dlopen RTLD_NOLOAD once the library loaded it)
#include "recording_backend.h"
#include "test_port.h"

extern "C" {
#include "ifcs_api.h"
}

#include <dlfcn.h>
#include <fcntl.h>
#include <unistd.h>

#include <atomic>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <mutex>
#include <string>
#include <vector>

namespace {

std::atomic<uint64_t> g_raised{0}, g_delivered{0};

// ------------------------------------------------------------------ the tap's stream
std::mutex g_mu;
off_t g_off = 0;
std::string g_partial;
std::vector<std::string> g_pending;
std::vector<uint32_t> g_lags;   // mock LAG handles in order of creation

void read_new()   // g_mu held
{
    const char* f = std::getenv("PARITY_TAP_FILE");
    if (!f || !*f) return;
    int fd = open(f, O_RDONLY | O_CLOEXEC);
    if (fd < 0) return;
    char b[65536];
    ssize_t n;
    while ((n = pread(fd, b, sizeof(b), g_off)) > 0) {
        g_off += n;
        g_partial.append(b, (size_t)n);
    }
    close(fd);
    size_t start = 0, nl;
    while ((nl = g_partial.find('\n', start)) != std::string::npos) {
        std::string l = g_partial.substr(start, nl - start);
        start = nl + 1;
        static const char kLag[] = "ifcs_lag_create h=0x";
        if (l.compare(0, sizeof(kLag) - 1, kLag) == 0) g_lags.push_back((uint32_t)std::strtoul(l.c_str() + sizeof(kLag) - 1, nullptr, 16));
        g_pending.push_back(std::move(l));
    }
    g_partial.erase(0, start);
}

// ------------------------------------------------------------------ the mock's hooks
template <typename T> T hook(const char* name)
{
    const char* m = std::getenv("PARITY_MOCK");
    if (!m || !*m) return nullptr;
    void* h = dlopen(m, RTLD_NOW | RTLD_NOLOAD);
    if (!h) return nullptr;
    T fn = reinterpret_cast<T>(dlsym(h, name));
    dlclose(h);
    return fn;
}

void note(const std::string& what) { std::fprintf(stderr, "tl_port: %s\n", what.c_str()); }

// ------------------------------------------------------------------ notification counters
sai_port_state_change_notification_fn g_port_cb;
sai_fdb_event_notification_fn g_fdb_cb;
void port_cb(uint32_t n, const sai_port_oper_status_notification_t* d)
{
    if (g_port_cb) g_port_cb(n, d);
    g_delivered += n;
}
void fdb_cb(uint32_t n, const sai_fdb_event_notification_data_t* d)
{
    if (g_fdb_cb) g_fdb_cb(n, d);
    g_delivered += n;
}
void route(sai_attribute_t& a)
{
    if (a.id == SAI_SWITCH_ATTR_PORT_STATE_CHANGE_NOTIFY) {
        g_port_cb = (sai_port_state_change_notification_fn)a.value.ptr;
        if (a.value.ptr) a.value.ptr = (void*)&port_cb;
    } else if (a.id == SAI_SWITCH_ATTR_FDB_EVENT_NOTIFY) {
        g_fdb_cb = (sai_fdb_event_notification_fn)a.value.ptr;
        if (a.value.ptr) a.value.ptr = (void*)&fdb_cb;
    }
}
sai_switch_api_t g_real_sw, g_sw;
sai_status_t create_switch(sai_object_id_t* s, uint32_t n, const sai_attribute_t* attrs)
{
    std::vector<sai_attribute_t> v(attrs, attrs + (attrs ? n : 0));
    for (auto& a : v) route(a);
    return g_real_sw.create_switch(s, n, attrs ? v.data() : nullptr);
}
sai_status_t set_switch_attribute(sai_object_id_t s, const sai_attribute_t* attr)
{
    if (!attr) return g_real_sw.set_switch_attribute(s, attr);
    sai_attribute_t a = *attr;
    route(a);
    return g_real_sw.set_switch_attribute(s, &a);
}

int devport_stat(sai_stat_id_t id)
{
#define P(x) SAI_PORT_STAT_##x
#define D(x) IFCS_DEVPORT_STATS_ID_##x
    // the one-counter entries of sai-teralynx's tl_counters.cpp stat_map (the scripts' inject stat)
    static const struct { sai_stat_id_t sai; int ifcs; } m[] = {
        {P(IF_IN_OCTETS), D(RX_BYTES_ALL)},        {P(IF_IN_UCAST_PKTS), D(RX_FRAMES_UNICAST)},
        {P(IF_IN_DISCARDS), D(RX_FRAMES_DROP)},    {P(IF_IN_ERRORS), D(RX_FRAMES_ERR_ANY)},
        {P(IF_IN_BROADCAST_PKTS), D(RX_FRAMES_BROADCAST)}, {P(IF_IN_MULTICAST_PKTS), D(RX_FRAMES_MULTICAST)},
        {P(IF_OUT_OCTETS), D(TX_BYTES_OK)},        {P(IF_OUT_UCAST_PKTS), D(TX_FRAMES_UNICAST)},
        {P(IF_OUT_DISCARDS), D(TX_FRAMES_DROP)},   {P(IF_OUT_ERRORS), D(TX_FRAMES_ERR_ANY)},
        {P(IF_OUT_BROADCAST_PKTS), D(TX_FRAMES_BROADCAST)}, {P(IF_OUT_MULTICAST_PKTS), D(TX_FRAMES_MULTICAST)},
    };
#undef P
#undef D
    for (auto& e : m)
        if (e.sai == id) return e.ifcs;
    return -1;
}

} // namespace

extern "C" sai_status_t __real_sai_api_query(sai_api_t api, void** table);
extern "C" sai_status_t __wrap_sai_api_query(sai_api_t api, void** table)
{
    sai_status_t st = __real_sai_api_query(api, table);
    if (api == SAI_API_SWITCH && st == SAI_STATUS_SUCCESS && table && *table) {
        g_real_sw = *static_cast<sai_switch_api_t*>(*table);
        g_sw = g_real_sw;
        if (g_sw.create_switch) g_sw.create_switch = create_switch;
        if (g_sw.set_switch_attribute) g_sw.set_switch_attribute = set_switch_attribute;
        *table = &g_sw;
    }
    return st;
}

// ====================================================================== ob::
namespace ob {

void CallLog::add(std::string line)
{
    std::lock_guard<std::mutex> g(g_mu);
    g_pending.push_back(std::move(line));
}
void CallLog::event(std::string line)
{
    std::lock_guard<std::mutex> g(g_mu);
    g_pending.push_back(std::move(line));
    g_raised++;
}
std::vector<std::string> CallLog::take()
{
    std::lock_guard<std::mutex> g(g_mu);
    read_new();
    std::vector<std::string> v;
    v.swap(g_pending);
    return v;
}
uint64_t CallLog::events_raised() const { return g_raised; }

// recording_backend.cpp's names
std::string sai_status_name(sai_status_t st)
{
    switch (st) {
    case SAI_STATUS_SUCCESS: return "SUCCESS";
    case SAI_STATUS_FAILURE: return "FAILURE";
    case SAI_STATUS_NOT_SUPPORTED: return "NOT_SUPPORTED";
    case SAI_STATUS_NO_MEMORY: return "NO_MEMORY";
    case SAI_STATUS_INSUFFICIENT_RESOURCES: return "INSUFFICIENT_RESOURCES";
    case SAI_STATUS_INVALID_PARAMETER: return "INVALID_PARAMETER";
    case SAI_STATUS_ITEM_ALREADY_EXISTS: return "ITEM_ALREADY_EXISTS";
    case SAI_STATUS_ITEM_NOT_FOUND: return "ITEM_NOT_FOUND";
    case SAI_STATUS_BUFFER_OVERFLOW: return "BUFFER_OVERFLOW";
    case SAI_STATUS_INVALID_PORT_NUMBER: return "INVALID_PORT_NUMBER";
    case SAI_STATUS_INVALID_PORT_MEMBER: return "INVALID_PORT_MEMBER";
    case SAI_STATUS_INVALID_VLAN_ID: return "INVALID_VLAN_ID";
    case SAI_STATUS_UNINITIALIZED: return "UNINITIALIZED";
    case SAI_STATUS_TABLE_FULL: return "TABLE_FULL";
    case SAI_STATUS_MANDATORY_ATTRIBUTE_MISSING: return "MANDATORY_ATTRIBUTE_MISSING";
    case SAI_STATUS_NOT_IMPLEMENTED: return "NOT_IMPLEMENTED";
    case SAI_STATUS_ADDR_NOT_FOUND: return "ADDR_NOT_FOUND";
    case SAI_STATUS_OBJECT_IN_USE: return "OBJECT_IN_USE";
    case SAI_STATUS_INVALID_OBJECT_TYPE: return "INVALID_OBJECT_TYPE";
    case SAI_STATUS_INVALID_OBJECT_ID: return "INVALID_OBJECT_ID";
    case SAI_STATUS_INVALID_NV_STORAGE: return "INVALID_NV_STORAGE";
    case SAI_STATUS_NV_STORAGE_FULL: return "NV_STORAGE_FULL";
    case SAI_STATUS_SW_UPGRADE_VERSION_MISMATCH: return "SW_UPGRADE_VERSION_MISMATCH";
    case SAI_STATUS_NOT_EXECUTED: return "NOT_EXECUTED";
    case SAI_STATUS_STAGE_MISMATCH: return "STAGE_MISMATCH";
    default: break;
    }
    struct R { sai_status_t base, max; const char* name; };
    static const R ranges[] = {
        {SAI_STATUS_INVALID_ATTRIBUTE_0, SAI_STATUS_INVALID_ATTRIBUTE_MAX, "INVALID_ATTRIBUTE_"},
        {SAI_STATUS_INVALID_ATTR_VALUE_0, SAI_STATUS_INVALID_ATTR_VALUE_MAX, "INVALID_ATTR_VALUE_"},
        {SAI_STATUS_ATTR_NOT_IMPLEMENTED_0, SAI_STATUS_ATTR_NOT_IMPLEMENTED_MAX, "ATTR_NOT_IMPLEMENTED_"},
        {SAI_STATUS_UNKNOWN_ATTRIBUTE_0, SAI_STATUS_UNKNOWN_ATTRIBUTE_MAX, "UNKNOWN_ATTRIBUTE_"},
        {SAI_STATUS_ATTR_NOT_SUPPORTED_0, SAI_STATUS_ATTR_NOT_SUPPORTED_MAX, "ATTR_NOT_SUPPORTED_"},
    };
    for (auto& r : ranges)
        if (st <= r.base && st >= r.max) return std::string(r.name) + std::to_string(r.base - st);
    return "STATUS_" + std::to_string(st);
}

} // namespace ob

// ====================================================================== tp::
namespace tp {

uint64_t events_delivered() { return g_delivered; }

int backend_id(sai_object_id_t oid)
{
    sai_object_type_t t = sai_object_type_query(oid);
    if (t == SAI_OBJECT_TYPE_PORT || t == SAI_OBJECT_TYPE_LAG) return (int)(oid & 0xffffffffffffull);
    return -1;
}

int lag_dest(int tid) { return 0x40000000 | tid; }

void inject_link(int hw, bool up)
{
    auto fire = hook<int (*)(uint32_t, int)>("mock_fire_link");
    if (!fire) return note("inject link: no switch (mock not loaded)");
    if (fire((uint32_t)hw, up ? 1 : 0) == 0) g_raised++;
}

void inject_fdb(bool learned, const uint8_t mac[6], uint16_t vid, int dest)
{
    int type = learned ? IFCS_L2_EVENT_TYPE_LEARNED : IFCS_L2_EVENT_TYPE_AGED;
    if (dest & 0x40000000) {
        uint32_t h = 0;
        {
            std::lock_guard<std::mutex> g(g_mu);
            read_new();
            size_t k = (size_t)(dest & 0xffff);
            if (k >= 1 && k <= g_lags.size()) h = g_lags[k - 1];
        }
        if (!h) return note("inject fdb: the LAG has no mock handle (no ifcs_lag_create line)");
        auto fire = hook<int (*)(int, const uint8_t*, uint16_t, uint32_t)>("mock_fire_l2_dest");
        if (fire && fire(type, mac, vid, h) == 0) g_raised++;
        return;
    }
    auto fire = hook<int (*)(int, const uint8_t*, uint16_t, uint32_t)>("mock_fire_l2");
    if (!fire) return note("inject fdb: no switch (mock not loaded)");
    if (fire(type, mac, vid, (uint32_t)dest) == 0) g_raised++;
}

void set_port_stat(int hw, sai_stat_id_t id, uint64_t value)
{
    int d = devport_stat(id);
    auto set = hook<void (*)(uint32_t, uint32_t, uint64_t)>("mock_set_stat");
    if (d < 0 || !set) return note("inject stat: no devport counter for SAI port stat " + std::to_string(id));
    set((uint32_t)hw, (uint32_t)d, value);
}

void linkscan() {}
void mdio_set(unsigned, unsigned, unsigned, unsigned) {}
void set_feature(Feature, bool) {}

} // namespace tp
