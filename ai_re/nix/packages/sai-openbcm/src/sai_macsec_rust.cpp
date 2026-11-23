// SPDX-License-Identifier: Apache-2.0
// SAI MACsec with the object model in Rust (make RUST_MACSEC=...: this file replaces
// sai_macsec.cpp; rust/sai/crates/sai-macsec behind sai_macsec_rs.h). It defines the same
// functions sai_macsec.cpp does, and forwards them:
//
//   kept here: the method table and guarded() (the lock, "no switch"), the *_stats_ext mode
//     checks, reading and parsing the table file (ext_macsec_load, either engine), running a
//     sequence (ext_macsec_exec under ext_phy_locked, MDIO through the backend: the trace sees
//     it as before), the inter-frame gap (port_ifg_apply), ids (ctx().alloc), logging,
//     the root-only parameter file of "extmacsec run"
//   in Rust: the MACsec objects and the MACsec-stage ACL objects, every check and status,
//     which sequence runs when with which bytes, the counters, the status text
//
// Its own differences from sai_macsec.cpp are listed in sai_macsec_rs.h's crate
// (rust/sai/crates/sai-macsec-capi) and sai-macsec's D1-D8.
#include "ext_macsec.h"
#include "ext_phy.h"
#include "sai_internal.h"

#include "sai_macsec_rs.h"

#include <chrono>
#include <cstring>
#include <fstream>
#include <sstream>
#include <sys/stat.h>
#include <thread>

namespace ob {

namespace {
struct RsState {
    MsTable tab;
    bool loaded = false;
    Backend* be = nullptr;
    std::map<int, unsigned> phy;          // hw -> PHY of the table's ports
    std::map<std::string, int> by_name;   // the table's ports by SDK name ("extmacsec run")
    std::vector<ob_rs_switch_port> handed;   // the SAI ports Rust was told about last
};
RsState& R()
{
    static RsState* s = new RsState;
    return *s;
}

uint64_t now_us()
{
    return (uint64_t)std::chrono::duration_cast<std::chrono::microseconds>(std::chrono::steady_clock::now().time_since_epoch()).count();
}
void wait_us(unsigned us)
{
    if (us == 0) return;
    if (us > 2000) {
        std::this_thread::sleep_for(std::chrono::microseconds(us));
        return;
    }
    uint64_t end = now_us() + us;   // short waits are spun: appliance kernels round a sleep up to a tick
    while (now_us() < end) {}
}

// The gap the model asks for while it is inside port_ifg_apply (set_ifg below): asking Rust then
// would be a call back into it.
thread_local int t_ifg_hw = -1;
thread_local uint32_t t_ifg_bits = 0;

// ---- callbacks (called by Rust, during a call into it; no exception may leave them) ----
extern "C" {
static void rs_log(int32_t api, int32_t level, const char* text, size_t len) noexcept
{
    try {
        OB_LOG((sai_api_t)api, (sai_log_level_t)level, "%.*s", (int)len, text);
    } catch (...) {
    }
}

static sai_status_t rs_run(uint32_t hw, const char* name, size_t name_len, const ob_rs_param* params, size_t n_params,
                           ob_rs_put_fn put, void* put_ctx, char* why, size_t why_cap) noexcept
{
    try {
        RsState& s = R();
        std::string seq(name, name_len), w;
        auto it = s.phy.find((int)hw);
        const std::vector<MsOp>* ops = s.tab.find(seq);
        sai_status_t st = SAI_STATUS_NOT_SUPPORTED;
        MsResult res;
        if (!s.be || it == s.phy.end()) {
            w = "no MACsec PHY on this port";
        } else if (!ops) {
            w = "the external MACsec table has no sequence " + seq;
        } else {
            MsParams par;
            for (size_t i = 0; i < n_params; i++) par[std::string(params[i].name, params[i].name_len)].assign(params[i].bytes, params[i].bytes + params[i].len);
            Backend& be = *s.be;
            MsIo io;
            io.rd = [&be](unsigned p, unsigned d, unsigned r, unsigned& v, const MsOp&) { return be.mdio_c45_read(p, d, r, v); };
            io.wr = [&be](unsigned p, unsigned d, unsigned r, unsigned v) { return be.mdio_c45_write(p, d, r, v); };
            io.delay = wait_us;
            unsigned phy = it->second;
            st = SAI_STATUS_FAILURE;
            ext_phy_locked([&] { st = ext_macsec_exec(io, *ops, phy, par, &res, w); });
            ms_wipe(par);
        }
        if (st == SAI_STATUS_SUCCESS) {
            for (auto& r : res)
                for (size_t k = 0; k < r.second.size(); k++) put(put_ctx, r.first.data(), r.first.size(), (uint32_t)k, r.second[k]);
        } else if (why && why_cap) {
            size_t n = std::min(w.size(), why_cap - 1);
            std::memcpy(why, w.data(), n);
            why[n] = 0;
        }
        return st;
    } catch (...) {
        return SAI_STATUS_FAILURE;
    }
}

static sai_status_t rs_set_ifg(uint32_t hw, uint32_t bits) noexcept
{
    try {
        auto it = ctx().hw2port.find((int)hw);
        PortObj* p = it == ctx().hw2port.end() ? nullptr : lookup(ctx().ports, it->second);
        if (!p) return SAI_STATUS_SUCCESS;
        t_ifg_hw = (int)hw;
        t_ifg_bits = bits;
        sai_status_t st = port_ifg_apply(*p);
        t_ifg_hw = -1;
        return st;
    } catch (...) {
        t_ifg_hw = -1;
        return SAI_STATUS_FAILURE;
    }
}

static sai_object_id_t rs_alloc(int32_t t) noexcept
{
    try {
        return ctx().alloc((sai_object_type_t)t);
    } catch (...) {
        return SAI_NULL_OBJECT_ID;
    }
}

static void rs_text(void* ctx, const char* text, size_t len) noexcept
{
    try {
        static_cast<std::string*>(ctx)->append(text, len);
    } catch (...) {
    }
}
} // extern "C"

const ob_rs_callbacks kCallbacks = {rs_log, rs_run, rs_set_ifg, rs_alloc};

// The SAI ports, handed to Rust whenever they differ from the last hand-over (the C++ model
// looked them up in ctx().ports at every call).
void sync_ports()
{
    std::vector<ob_rs_switch_port> v;
    for (auto& p : ctx().ports)
        if (!p.second.cpu) v.push_back({p.second.oid, (uint32_t)p.second.hw});
    auto& h = R().handed;
    bool same = v.size() == h.size();
    for (size_t i = 0; same && i < v.size(); i++) same = v[i].oid == h[i].oid && v[i].hw == h[i].hw;
    if (same) return;
    h = v;
    ob_rs_macsec_ports(v.data(), v.size());
}

template <typename F>
sai_status_t fwd(const char* fn, F&& f)
{
    return guarded(SAI_API_MACSEC, fn, [&]() -> sai_status_t {
        sync_ports();
        return f();
    });
}

#define OB_RS_OBJECT(name, type)                                                                                          \
    sai_status_t create_##name(sai_object_id_t* id, sai_object_id_t, uint32_t n, const sai_attribute_t* attrs)          \
    {                                                                                                                     \
        return fwd(__func__, [&] { return ob_rs_macsec_create(type, id, n, attrs); });                                   \
    }                                                                                                                     \
    sai_status_t remove_##name(sai_object_id_t id) { return fwd(__func__, [&] { return ob_rs_macsec_remove(type, id); }); } \
    sai_status_t set_##name##_attribute(sai_object_id_t id, const sai_attribute_t* attr)                                \
    {                                                                                                                     \
        return fwd(__func__, [&] { return ob_rs_macsec_set(type, id, attr); });                                          \
    }                                                                                                                     \
    sai_status_t get_##name##_attribute(sai_object_id_t id, uint32_t n, sai_attribute_t* attrs)                         \
    {                                                                                                                     \
        return fwd(__func__, [&] { return ob_rs_macsec_get(type, id, n, attrs); });                                      \
    }

OB_RS_OBJECT(macsec, SAI_OBJECT_TYPE_MACSEC)
OB_RS_OBJECT(macsec_port, SAI_OBJECT_TYPE_MACSEC_PORT)
OB_RS_OBJECT(macsec_flow, SAI_OBJECT_TYPE_MACSEC_FLOW)
OB_RS_OBJECT(macsec_sc, SAI_OBJECT_TYPE_MACSEC_SC)
OB_RS_OBJECT(macsec_sa, SAI_OBJECT_TYPE_MACSEC_SA)
#undef OB_RS_OBJECT

sai_status_t get_macsec_port_stats(sai_object_id_t id, uint32_t n, const sai_stat_id_t* ids, uint64_t* out)
{
    return fwd(__func__, [&] { return ob_rs_macsec_stats(SAI_OBJECT_TYPE_MACSEC_PORT, id, n, ids, out); });
}
sai_status_t get_macsec_port_stats_ext(sai_object_id_t id, uint32_t n, const sai_stat_id_t* ids, sai_stats_mode_t, uint64_t* out)
{
    return get_macsec_port_stats(id, n, ids, out);
}
sai_status_t clear_macsec_port_stats(sai_object_id_t, uint32_t, const sai_stat_id_t*) { return SAI_STATUS_NOT_SUPPORTED; }
sai_status_t get_macsec_flow_stats(sai_object_id_t id, uint32_t n, const sai_stat_id_t* ids, uint64_t* out)
{
    return fwd(__func__, [&] { return ob_rs_macsec_stats(SAI_OBJECT_TYPE_MACSEC_FLOW, id, n, ids, out); });
}
sai_status_t get_macsec_flow_stats_ext(sai_object_id_t id, uint32_t n, const sai_stat_id_t* ids, sai_stats_mode_t mode, uint64_t* out)
{
    if (mode != SAI_STATS_MODE_READ) return SAI_STATUS_NOT_SUPPORTED;
    return get_macsec_flow_stats(id, n, ids, out);
}
sai_status_t clear_macsec_flow_stats(sai_object_id_t id, uint32_t, const sai_stat_id_t*)
{
    return fwd(__func__, [&] { return ob_rs_macsec_clear_stats(SAI_OBJECT_TYPE_MACSEC_FLOW, id); });
}
sai_status_t get_macsec_sc_stats(sai_object_id_t id, uint32_t n, const sai_stat_id_t* ids, uint64_t* out)
{
    return fwd(__func__, [&] { return ob_rs_macsec_stats(SAI_OBJECT_TYPE_MACSEC_SC, id, n, ids, out); });
}
sai_status_t get_macsec_sc_stats_ext(sai_object_id_t id, uint32_t n, const sai_stat_id_t* ids, sai_stats_mode_t, uint64_t* out)
{
    return get_macsec_sc_stats(id, n, ids, out);
}
sai_status_t clear_macsec_sc_stats(sai_object_id_t, uint32_t, const sai_stat_id_t*) { return SAI_STATUS_NOT_SUPPORTED; }
sai_status_t get_macsec_sa_stats(sai_object_id_t id, uint32_t n, const sai_stat_id_t* ids, uint64_t* out)
{
    return fwd(__func__, [&] { return ob_rs_macsec_stats(SAI_OBJECT_TYPE_MACSEC_SA, id, n, ids, out); });
}
sai_status_t get_macsec_sa_stats_ext(sai_object_id_t id, uint32_t n, const sai_stat_id_t* ids, sai_stats_mode_t mode, uint64_t* out)
{
    if (mode != SAI_STATS_MODE_READ) return SAI_STATUS_NOT_SUPPORTED;
    return get_macsec_sa_stats(id, n, ids, out);
}
sai_status_t clear_macsec_sa_stats(sai_object_id_t id, uint32_t, const sai_stat_id_t*)
{
    return fwd(__func__, [&] { return ob_rs_macsec_clear_stats(SAI_OBJECT_TYPE_MACSEC_SA, id); });
}

sai_macsec_api_t make_macsec_api()
{
    sai_macsec_api_t a;
    std::memset(&a, 0, sizeof(a));
    a.create_macsec = create_macsec;
    a.remove_macsec = remove_macsec;
    a.set_macsec_attribute = set_macsec_attribute;
    a.get_macsec_attribute = get_macsec_attribute;
    a.create_macsec_port = create_macsec_port;
    a.remove_macsec_port = remove_macsec_port;
    a.set_macsec_port_attribute = set_macsec_port_attribute;
    a.get_macsec_port_attribute = get_macsec_port_attribute;
    a.get_macsec_port_stats = get_macsec_port_stats;
    a.get_macsec_port_stats_ext = get_macsec_port_stats_ext;
    a.clear_macsec_port_stats = clear_macsec_port_stats;
    a.create_macsec_flow = create_macsec_flow;
    a.remove_macsec_flow = remove_macsec_flow;
    a.set_macsec_flow_attribute = set_macsec_flow_attribute;
    a.get_macsec_flow_attribute = get_macsec_flow_attribute;
    a.get_macsec_flow_stats = get_macsec_flow_stats;
    a.get_macsec_flow_stats_ext = get_macsec_flow_stats_ext;
    a.clear_macsec_flow_stats = clear_macsec_flow_stats;
    a.create_macsec_sc = create_macsec_sc;
    a.remove_macsec_sc = remove_macsec_sc;
    a.set_macsec_sc_attribute = set_macsec_sc_attribute;
    a.get_macsec_sc_attribute = get_macsec_sc_attribute;
    a.get_macsec_sc_stats = get_macsec_sc_stats;
    a.get_macsec_sc_stats_ext = get_macsec_sc_stats_ext;
    a.clear_macsec_sc_stats = clear_macsec_sc_stats;
    a.create_macsec_sa = create_macsec_sa;
    a.remove_macsec_sa = remove_macsec_sa;
    a.set_macsec_sa_attribute = set_macsec_sa_attribute;
    a.get_macsec_sa_attribute = get_macsec_sa_attribute;
    a.get_macsec_sa_stats = get_macsec_sa_stats;
    a.get_macsec_sa_stats_ext = get_macsec_sa_stats_ext;
    a.clear_macsec_sa_stats = clear_macsec_sa_stats;
    return a;
}
} // namespace

const sai_macsec_api_t macsec_api = make_macsec_api();

// ---- ACL tables of the MACsec stages (called from sai_acl.cpp, under its guarded) ----
bool macsec_acl_stage(uint32_t n, const sai_attribute_t* attrs)
{
    const sai_attribute_t* a = find_attr(n, attrs, SAI_ACL_TABLE_ATTR_ACL_STAGE);
    return a && (a->value.s32 == SAI_ACL_STAGE_INGRESS_MACSEC || a->value.s32 == SAI_ACL_STAGE_EGRESS_MACSEC);
}
bool macsec_acl_table(sai_object_id_t id) { return ob_rs_macsec_owns(SAI_OBJECT_TYPE_ACL_TABLE, id) != 0; }
bool macsec_acl_entry(sai_object_id_t id) { return ob_rs_macsec_owns(SAI_OBJECT_TYPE_ACL_ENTRY, id) != 0; }
sai_status_t macsec_acl_table_create(sai_object_id_t* id, uint32_t n, const sai_attribute_t* attrs)
{
    sync_ports();
    return ob_rs_macsec_create(SAI_OBJECT_TYPE_ACL_TABLE, id, n, attrs);
}
sai_status_t macsec_acl_table_remove(sai_object_id_t id) { return ob_rs_macsec_remove(SAI_OBJECT_TYPE_ACL_TABLE, id); }
sai_status_t macsec_acl_table_get(sai_object_id_t id, uint32_t n, sai_attribute_t* attrs)
{
    return ob_rs_macsec_get(SAI_OBJECT_TYPE_ACL_TABLE, id, n, attrs);
}
sai_status_t macsec_acl_entry_create(sai_object_id_t* id, uint32_t n, const sai_attribute_t* attrs)
{
    sync_ports();
    return ob_rs_macsec_create(SAI_OBJECT_TYPE_ACL_ENTRY, id, n, attrs);
}
sai_status_t macsec_acl_entry_remove(sai_object_id_t id)
{
    sync_ports();
    return ob_rs_macsec_remove(SAI_OBJECT_TYPE_ACL_ENTRY, id);
}
sai_status_t macsec_acl_entry_set(sai_object_id_t id, const sai_attribute_t* attr)
{
    sync_ports();
    return ob_rs_macsec_set(SAI_OBJECT_TYPE_ACL_ENTRY, id, attr);
}
sai_status_t macsec_acl_entry_get(sai_object_id_t id, uint32_t n, sai_attribute_t* attrs)
{
    return ob_rs_macsec_get(SAI_OBJECT_TYPE_ACL_ENTRY, id, n, attrs);
}

// ---- PORT INGRESS_MACSEC_ACL / EGRESS_MACSEC_ACL / MACSEC_PORT_LIST (called from sai_port.cpp) ----
sai_status_t port_macsec_set(PortObj& p, const sai_attribute_t& a, uint32_t i, bool& handled)
{
    if (a.id != SAI_PORT_ATTR_INGRESS_MACSEC_ACL && a.id != SAI_PORT_ATTR_EGRESS_MACSEC_ACL) return SAI_STATUS_SUCCESS;
    handled = true;
    sync_ports();
    return ob_rs_macsec_port_acl_set(p.oid, a.id == SAI_PORT_ATTR_INGRESS_MACSEC_ACL, a.value.oid, i);
}
sai_status_t port_macsec_get(PortObj& p, sai_attribute_t& a, uint32_t i, bool& handled)
{
    if (a.id == SAI_PORT_ATTR_INGRESS_MACSEC_ACL || a.id == SAI_PORT_ATTR_EGRESS_MACSEC_ACL) {
        handled = true;
        sync_ports();
        a.value.oid = ob_rs_macsec_port_acl(p.oid, a.id == SAI_PORT_ATTR_INGRESS_MACSEC_ACL);
    } else if (a.id == SAI_PORT_ATTR_MACSEC_PORT_LIST) {
        handled = true;
        sync_ports();
        return ob_rs_macsec_port_list(p.oid, &a.value.objlist);
    }
    return SAI_STATUS_SUCCESS;
}
uint32_t macsec_port_ifg(int hw)
{
    if (hw == t_ifg_hw) return t_ifg_bits;   // asked from inside the model's own gap change
    return ob_rs_macsec_port_ifg((uint32_t)hw);
}

sai_status_t macsec_stats_capability(sai_object_type_t t, sai_stat_capability_list_t* out)
{
    std::lock_guard<std::recursive_mutex> g(ctx().mu);
    if (t != SAI_OBJECT_TYPE_MACSEC_SA && t != SAI_OBJECT_TYPE_MACSEC_FLOW) return SAI_STATUS_NOT_SUPPORTED;
    return ob_rs_macsec_stats_capability(t, out);
}

// ---- table life cycle ----
sai_status_t ext_macsec_load(const BackendConfig& cfg)
{
    RsState& s = R();
    s = RsState();
    ob_rs_macsec_detach();
    std::string path = cfg.get("SAI_OPENBCM_EXT_MACSEC_FILE");
    if (path.empty()) return SAI_STATUS_SUCCESS;
    std::ifstream f(path);
    if (!f) {
        OB_ERR(SAI_API_SWITCH, "SAI_OPENBCM_EXT_MACSEC_FILE %s: cannot open; refusing to create the switch", path.c_str());
        return SAI_STATUS_FAILURE;
    }
    std::stringstream ss;
    ss << f.rdbuf();
    std::string err;
    if (!ext_macsec_parse(ss.str(), s.tab, err)) {
        OB_ERR(SAI_API_SWITCH, "SAI_OPENBCM_EXT_MACSEC_FILE %s: %s; refusing to create the switch", path.c_str(), err.c_str());
        s.tab = MsTable();
        return SAI_STATUS_INVALID_PARAMETER;
    }
    s.loaded = true;
    return SAI_STATUS_SUCCESS;
}

sai_status_t ext_macsec_attach(Backend& be)
{
    RsState& s = R();
    if (ob_rs_macsec_abi() != OB_RS_MACSEC_ABI) {
        OB_ERR(SAI_API_SWITCH, "sai-macsec library has interface version %u, this build expects %u", ob_rs_macsec_abi(), OB_RS_MACSEC_ABI);
        return SAI_STATUS_FAILURE;
    }
    if (!s.loaded) return ob_rs_macsec_attach(&kCallbacks, 0, 1, 0, nullptr, 0, nullptr, 0, nullptr, 0);
    std::vector<PortInfo> ports = be.ports();
    std::vector<ob_rs_chip_port> cp;
    for (auto& pi : ports)
        cp.push_back({(uint32_t)pi.hw, pi.lanes.empty() ? 0u : 1u, pi.lanes.empty() ? 0u : pi.lanes[0], pi.name.data(), pi.name.size()});
    std::vector<ob_rs_engine> eng;
    for (auto& e : s.tab.ports) eng.push_back({e.first, e.second});
    std::vector<std::string> results(s.tab.seqs.size());
    std::vector<ob_rs_seq> seqs;
    size_t k = 0;
    for (auto& sq : s.tab.seqs) {
        for (auto& o : sq.second)
            if (o.op == 'X' && o.kept < 0) results[k] += o.res + "\n";
        seqs.push_back({sq.first.data(), sq.first.size(), results[k].data(), results[k].size()});
        k++;
    }
    // the table's ports as ext_macsec_attach of sai_macsec.cpp finds them (the last port whose first lane matches)
    for (auto& e : s.tab.ports)
        for (auto& pi : ports)
            if (!pi.lanes.empty() && pi.lanes[0] == e.first) {
                s.phy[pi.hw] = e.second;
                s.by_name[pi.name] = pi.hw;
            }
    s.be = &be;   // before the attach: the probes run through rs_run
    OB_NOTICE(SAI_API_SWITCH, "external MACsec table: the MACsec object model is the Rust one (sai-macsec), sequences by the %s engine",
              ext_macsec_engine());
    sai_status_t st = ob_rs_macsec_attach(&kCallbacks, 1, s.tab.clear_on_read ? 1 : 0, s.tab.ifg_bits, eng.data(), eng.size(),
                                          seqs.data(), seqs.size(), cp.data(), cp.size());
    if (st != SAI_STATUS_SUCCESS) s.be = nullptr;
    return st;
}

void ext_macsec_detach()
{
    ob_rs_macsec_detach();
    R() = RsState();
}
bool ext_macsec_active() { return R().be != nullptr; }

std::string ext_macsec_status()
{
    if (!R().be) return "extmacsec: no external MACsec table (SAI_OPENBCM_EXT_MACSEC_FILE)\n";
    std::string out;
    ob_rs_macsec_status(rs_text, &out);
    return out;
}

std::string ext_macsec_diag_run(const std::string& port, const std::string& seq, const std::string& param_file)
{
    RsState& s = R();
    if (!s.be) return "extmacsec: no external MACsec table\n";
    if (!s.by_name.count(port)) return "extmacsec: no such port in the table: " + port + "\n";
    MsParams par;
    if (!param_file.empty()) {
        struct stat sb;
        if (::stat(param_file.c_str(), &sb) != 0 || (sb.st_mode & 077) || sb.st_uid != 0)
            return "extmacsec: parameter file must exist, belong to root and be mode 0600 or 0400\n";
        std::ifstream f(param_file);
        std::string name, hex;
        while (f >> name >> hex) {
            std::vector<uint8_t> v;
            for (size_t i = 0; i + 1 < hex.size(); i += 2) v.push_back((uint8_t)std::strtoul(hex.substr(i, 2).c_str(), nullptr, 16));
            par[name] = v;
            explicit_bzero(&hex[0], hex.size());
        }
    }
    std::vector<ob_rs_param> lent;
    for (auto& p : par) lent.push_back({p.first.data(), p.first.size(), p.second.data(), p.second.size()});
    std::string out;
    ob_rs_macsec_diag_run(port.data(), port.size(), seq.data(), seq.size(), lent.data(), lent.size(), rs_text, &out);
    ms_wipe(par);
    return out;
}

} // namespace ob
