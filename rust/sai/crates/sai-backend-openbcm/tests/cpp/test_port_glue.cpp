// SPDX-License-Identifier: Apache-2.0
// The parts of tests/test_port.h that sai-openbcm's tests/frontend_driver.cpp uses, for the glue
// differential of sai-backend-openbcm (tests/glue_differential.rs): the driver's switches run on
// the REAL C++ backend (openbcm_backend.cpp, unmodified) over the recording glue stub
// (glue_stub.c), so the hooks that tests/test_port_cpp.cpp implements on the fake backend (and
// aborts without one) are injections of the stub here, printed and replayed like the Rust
// stack's (sdk-glue's Chip::inject lines, same text). backend_id, lag_dest and events_delivered
// are tests/test_port_cpp.cpp's, over the library's context.
//
// Built without changing a C++ file: frontend_driver.cpp is compiled with
// -Dmake_fake_backend=glue_diff_backend (its RecordingBackend then wraps what glue_diff_backend
// returns) and openbcm_backend.cpp with -Dmake_openbcm_backend=glue_diff_openbcm_backend (the
// driver defines make_openbcm_backend itself); fake_backend.cpp is linked for sai_switch.cpp's
// make_fake_backend.
#include "test_port.h"

#include "backend.h"
#include "sai_internal.h"

#include <cstdio>
#include <cstring>
#include <mutex>
#include <string>

extern "C" void glue_stub_inject(const char* line);

namespace ob {
Backend* glue_diff_openbcm_backend();
// what the driver's recorder wraps
Backend* glue_diff_backend() { return glue_diff_openbcm_backend(); }
} // namespace ob

using namespace ob;

namespace tp {
namespace {

template <typename M, typename K> auto find_or_null(M& m, const K& k) -> decltype(&m.begin()->second)
{
    auto it = m.find(k);
    return it == m.end() ? nullptr : &it->second;
}

// SAI_PORT_STAT_IF_* -> the backend's PortStat (as sai_port.cpp maps them; test_port_cpp.cpp's)
bool port_stat_of(sai_stat_id_t id, PortStat& s)
{
    static const std::pair<sai_stat_id_t, PortStat> m[] = {
        {SAI_PORT_STAT_IF_IN_OCTETS, PortStat::InOctets}, {SAI_PORT_STAT_IF_IN_UCAST_PKTS, PortStat::InUcastPkts},
        {SAI_PORT_STAT_IF_IN_NON_UCAST_PKTS, PortStat::InNonUcastPkts}, {SAI_PORT_STAT_IF_IN_DISCARDS, PortStat::InDiscards},
        {SAI_PORT_STAT_IF_IN_ERRORS, PortStat::InErrors}, {SAI_PORT_STAT_IF_IN_UNKNOWN_PROTOS, PortStat::InUnknownProtos},
        {SAI_PORT_STAT_IF_IN_BROADCAST_PKTS, PortStat::InBroadcastPkts}, {SAI_PORT_STAT_IF_IN_MULTICAST_PKTS, PortStat::InMulticastPkts},
        {SAI_PORT_STAT_IF_OUT_OCTETS, PortStat::OutOctets}, {SAI_PORT_STAT_IF_OUT_UCAST_PKTS, PortStat::OutUcastPkts},
        {SAI_PORT_STAT_IF_OUT_NON_UCAST_PKTS, PortStat::OutNonUcastPkts}, {SAI_PORT_STAT_IF_OUT_DISCARDS, PortStat::OutDiscards},
        {SAI_PORT_STAT_IF_OUT_ERRORS, PortStat::OutErrors}, {SAI_PORT_STAT_IF_OUT_BROADCAST_PKTS, PortStat::OutBroadcastPkts},
        {SAI_PORT_STAT_IF_OUT_MULTICAST_PKTS, PortStat::OutMulticastPkts},
    };
    for (auto& e : m)
        if (e.first == id) { s = e.second; return true; }
    return false;
}

void inject(const std::string& line) { glue_stub_inject(line.c_str()); }

} // namespace

void inject_link(int hw, bool up)
{
    inject("inject link(port=" + std::to_string(hw) + ", up=" + std::to_string(up ? 1 : 0) + ")");
}

void linkscan() { inject("inject linkscan()"); }

void inject_fdb(bool learned, const uint8_t mac[6], uint16_t vid, int dest)
{
    char m[24];
    std::snprintf(m, sizeof(m), "%02x:%02x:%02x:%02x:%02x:%02x", mac[0], mac[1], mac[2], mac[3], mac[4], mac[5]);
    inject("inject fdb(learned=" + std::to_string(learned ? 1 : 0) + ", mac=" + m + ", vid=" + std::to_string(vid) +
           ", port=" + std::to_string(dest) + ")");
}

void set_port_stat(int hw, sai_stat_id_t id, uint64_t value)
{
    PortStat s;
    std::string which = port_stat_of(id, s) ? std::to_string((int)s) : "sai:" + std::to_string(id);
    inject("inject stat(port=" + std::to_string(hw) + ", which=" + which + ", value=" + std::to_string(value) + ")");
}

void set_feature(Feature f, bool on)
{
    const char* name = f == FEATURE_QOS ? "qos" : f == FEATURE_MIRROR ? "mirror" : "l2";
    inject(std::string("inject feature(name=\"") + name + "\", on=" + std::to_string(on ? 1 : 0) + ")");
}

void mdio_set(unsigned phy, unsigned dev, unsigned reg, unsigned value)
{
    char b[96];
    std::snprintf(b, sizeof(b), "inject mdio(phy=0x%x, dev=%u, reg=0x%x, val=0x%x)", phy, dev, reg, value);
    inject(b);
}

int backend_id(sai_object_id_t oid)
{
    auto& c = ctx();
    std::lock_guard<std::recursive_mutex> g(c.mu);
    switch (ob::oid_type(oid)) {
    case SAI_OBJECT_TYPE_PORT: { auto* p = find_or_null(c.ports, oid); return p ? p->hw : -1; }
    case SAI_OBJECT_TYPE_LAG: { auto* p = find_or_null(c.lags, oid); return p ? p->tid : -1; }
    case SAI_OBJECT_TYPE_VIRTUAL_ROUTER: { auto* p = find_or_null(c.vrs, oid); return p ? p->vrf : -1; }
    case SAI_OBJECT_TYPE_ROUTER_INTERFACE: { auto* p = find_or_null(c.rifs, oid); return p ? p->intf : -1; }
    case SAI_OBJECT_TYPE_POLICER: { auto* p = find_or_null(c.policers, oid); return p ? p->pid : -1; }
    case SAI_OBJECT_TYPE_ACL_TABLE: { auto* p = find_or_null(c.acl_tables, oid); return p ? p->gid : -1; }
    case SAI_OBJECT_TYPE_ACL_ENTRY: { auto* p = find_or_null(c.acl_entries, oid); return p ? p->eid : -1; }
    case SAI_OBJECT_TYPE_QOS_MAP: { auto* p = find_or_null(c.qos_maps, oid); return p ? p->id : -1; }
    case SAI_OBJECT_TYPE_WRED: { auto* p = find_or_null(c.wreds, oid); return p ? p->id : -1; }
    case SAI_OBJECT_TYPE_MIRROR_SESSION: { auto* p = find_or_null(c.mirrors, oid); return p ? p->id : -1; }
    case SAI_OBJECT_TYPE_SAMPLEPACKET: { auto* p = find_or_null(c.samplepackets, oid); return p ? p->id : -1; }
    case SAI_OBJECT_TYPE_HOSTIF: {
        auto* p = find_or_null(c.hostifs, oid);
        if (!p) return -1;
        return p->type == SAI_HOSTIF_TYPE_GENETLINK ? p->genl : p->netif;
    }
    default: return -1;
    }
}

int lag_dest(int tid) { return ob::lag_dest(tid); }

uint64_t events_delivered() { return ctx().events_delivered.load(); }

} // namespace tp
