// SPDX-License-Identifier: Apache-2.0
// Differential driver for the sai-macsec crate (tests/differential.rs): runs a script of
// MACsec object operations against the C++ sai-openbcm library on its fake backend and
// prints, per operation, the SAI status, the values read and every access of the fake MDIO
// bus. The Rust test runs the same script on the Rust model and compares the text.
//
// Built from the unmodified C++ frontend sources (frozen since 2026-11-23 in
// ai_re/nix/packages/sai-teralynx/frontend/src; this file is the only addition and lives in the
// Rust crate): c++ -std=c++17 -I<SAI/inc> -I<frontend/src> driver.cpp <CORE .cpp> temp_files.o
// -lpthread. It reaches into the library the way the C++ unit test does (ctx(), FakeBackend's
// MDIO log and registers).
//
//   driver <table file> <script file>
//
// Built with -DSAI_MACSEC_DRIVER_CHIP: the "chip" variant with
// read-only overlay registers ("rdreg"), for sai-macsec-phy's tests/e2e.rs on a real board table.
//
// Environment (both optional, for sai-macsec-phy's tests/e2e.rs):
//   SAI_MACSEC_DRIVER_PORTS=<n>   fake ports (default 4; the AS7772 table names 32)
//   SAI_MACSEC_DRIVER_DELAYS=1    the bus text also carries the delays, in order ("D<usec>"), and
//                                 an access the bus refused ("!R..." / "!W..."): taken from the
//                                 sequence trace (ext_macsec.h), values from the fake's MDIO log
//
// Script: one operation per line, objects by variable name (see differential.rs, which is
// the reference for the format: both sides implement the same few lines per operation).
#include "ext_macsec.h"
#include "fake_backend.h"
#include "sai_internal.h"

#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <fstream>
#include <map>
#include <sstream>
#include <string>
#include <vector>

using namespace ob;

static std::map<std::string, std::string> g_profile;
static std::map<std::string, std::string>::iterator g_iter;
static const char* prof_get(sai_switch_profile_id_t, const char* k)
{
    auto it = g_profile.find(k);
    return it == g_profile.end() ? nullptr : it->second.c_str();
}
static int prof_next(sai_switch_profile_id_t, const char** k, const char** v)
{
    if (!k || !v) { g_iter = g_profile.begin(); return 0; }
    if (g_iter == g_profile.end()) return -1;
    *k = g_iter->first.c_str();
    *v = g_iter->second.c_str();
    ++g_iter;
    return 0;
}
static sai_service_method_table_t g_smt = {prof_get, prof_next};

static sai_switch_api_t* sw;
static sai_port_api_t* pa;
static sai_acl_api_t* aa;
static sai_macsec_api_t* ma;
static sai_object_id_t g_sw;
static sai_object_id_t g_ports[64];

static FakeBackend& fake() { return *dynamic_cast<FakeBackend*>(ctx().be.get()); }

#ifdef SAI_MACSEC_DRIVER_CHIP
// The "chip" variant (built with -DSAI_MACSEC_DRIVER_CHIP, profile
// SAI_OPENBCM_BACKEND=openbcm): the fake backend with registers whose reads do not see the
// writes ("rdreg": a command / data FIFO such as the AS7772 PHYs' 1.9005 reads what the
// engine answers, not the last word written). Everything else is the fake's.
namespace {
struct ChipFake : FakeBackend {
    std::map<std::tuple<unsigned, unsigned, unsigned>, unsigned> rd_only;   // under mdio_mu
    sai_status_t mdio_c45_read(unsigned phy, unsigned dev, unsigned reg, unsigned& val) override
    {
        {
            std::lock_guard<std::mutex> g(mdio_mu);
            auto it = rd_only.find(std::make_tuple(phy, dev, reg));
            if (it != rd_only.end() && !mdio_dead.count(phy)) {
                val = it->second;
                mdio_log.push_back({'R', phy, dev, reg, val});
                return SAI_STATUS_SUCCESS;
            }
        }
        return FakeBackend::mdio_c45_read(phy, dev, reg, val);
    }
};
} // namespace
namespace ob {
Backend* make_openbcm_backend() { return new ChipFake; }
} // namespace ob
static ChipFake& chip() { return *dynamic_cast<ChipFake*>(ctx().be.get()); }
#else
// No SDK backend in this build (what sai-openbcm's backend_none.cpp was: the fake backend only).
namespace ob {
Backend* make_openbcm_backend() { return nullptr; }
} // namespace ob
#endif

static std::map<std::string, sai_object_id_t> vars;
static bool var(const std::string& v, sai_object_id_t& o)
{
    auto it = vars.find(v);
    if (it == vars.end()) return false;
    o = it->second;
    return true;
}
static std::string name_of(sai_object_id_t o)
{
    if (!o) return "-";
    for (auto& kv : vars)
        if (kv.second == o) return kv.first;
    return "?";
}
static bool g_delays = false;
static std::string bus_take()
{
    std::lock_guard<std::mutex> g(fake().mdio_mu);
    std::string s;
    char b[64];
    auto& log = fake().mdio_log;
    if (!g_delays) {
        for (auto& o : log) {
            std::snprintf(b, sizeof(b), " %c%x.%x.%x=%x", o.op, o.phy, o.dev, o.reg, o.val);
            s += b;
        }
        log.clear();
        return s;
    }
    // The trace has every access and delay in order (no values); the fake's log has the
    // values of the accesses that reached it.
    std::istringstream ts(ext_macsec_trace("dump"));
    ext_macsec_trace("clear");
    size_t k = 0;
    std::string l;
    std::getline(ts, l);   // the trace's header line
    while (std::getline(ts, l)) {
        unsigned phy = 0, dev = 0, reg = 0;
        if (l.size() > 2 && l[0] == 'D') {
            s += " D" + l.substr(2);
        } else if ((l[0] == 'R' || l[0] == 'W') && std::sscanf(l.c_str() + 1, "%x %x %x", &phy, &dev, &reg) == 3) {
            if (k < log.size() && log[k].op == l[0] && log[k].phy == phy && log[k].dev == dev && log[k].reg == reg) {
                std::snprintf(b, sizeof(b), " %c%x.%x.%x=%x", log[k].op, phy, dev, reg, log[k].val);
                k++;
            } else {
                std::snprintf(b, sizeof(b), " !%c%x.%x.%x", l[0], phy, dev, reg);
            }
            s += b;
        }
    }
    for (; k < log.size(); k++) {   // outside a sequence (none expected)
        std::snprintf(b, sizeof(b), " ?%c%x.%x.%x=%x", log[k].op, log[k].phy, log[k].dev, log[k].reg, log[k].val);
        s += b;
    }
    log.clear();
    return s;
}
static sai_attribute_t at(sai_attr_id_t id)
{
    sai_attribute_t a;
    std::memset(&a, 0, sizeof(a));
    a.id = id;
    return a;
}
static sai_attribute_t a_s32(sai_attr_id_t id, int32_t v) { auto a = at(id); a.value.s32 = v; return a; }
static sai_attribute_t a_oid(sai_attr_id_t id, sai_object_id_t v) { auto a = at(id); a.value.oid = v; return a; }
static sai_attribute_t a_bool(sai_attr_id_t id, bool v) { auto a = at(id); a.value.booldata = v; return a; }
static sai_attribute_t a_u64(sai_attr_id_t id, uint64_t v) { auto a = at(id); a.value.u64 = v; return a; }
static sai_attribute_t a_u32(sai_attr_id_t id, uint32_t v) { auto a = at(id); a.value.u32 = v; return a; }
static sai_attribute_t a_u8(sai_attr_id_t id, uint8_t v) { auto a = at(id); a.value.u8 = v; return a; }
static int32_t dir_of(const std::string& d) { return d == "1" ? SAI_MACSEC_DIRECTION_INGRESS : SAI_MACSEC_DIRECTION_EGRESS; }
static unsigned long long num(const std::string& s) { return std::strtoull(s.c_str(), nullptr, 16); }
static std::string names(const sai_object_list_t& l)
{
    std::string s;
    for (uint32_t i = 0; i < l.count; i++) s += (i ? "," : "") + name_of(l.list[i]);
    return s.empty() ? "none" : s;
}

int main(int argc, char** argv)
{
    if (argc != 3) {
        std::fprintf(stderr, "usage: %s <table file> <script file>\n", argv[0]);
        return 2;
    }
#ifdef SAI_MACSEC_DRIVER_CHIP
    g_profile["SAI_OPENBCM_BACKEND"] = "openbcm";   // ChipFake, above
#else
    g_profile["SAI_OPENBCM_BACKEND"] = "fake";
#endif
    const char* ports = std::getenv("SAI_MACSEC_DRIVER_PORTS");
    g_profile["SAI_OPENBCM_FAKE_PORTS"] = ports && *ports ? ports : "4";
    const char* delays = std::getenv("SAI_MACSEC_DRIVER_DELAYS");
    g_delays = delays && std::strcmp(delays, "1") == 0;
    g_profile["SAI_OPENBCM_FAKE_LANES"] = "4";
    g_profile["SAI_OPENBCM_FAKE_SPEED"] = "100000";
    g_profile["SAI_OPENBCM_EXT_MACSEC_FILE"] = argv[1];
    // On a switch (the test bundle run on the AS7772) the BDE device nodes exist and the library
    // refuses its fake backend unless told; told only when the bundle says so (its run.sh
    // exports SAI_RS_VIRTUAL_ON_HARDWARE=1, the Rust virtual backend's convention). The fake
    // opens no device either way.
    const char* on_hw = std::getenv("SAI_RS_VIRTUAL_ON_HARDWARE");
    if (on_hw && std::strcmp(on_hw, "1") == 0) g_profile["SAI_OPENBCM_FAKE_ON_HARDWARE"] = "1";
    if (sai_api_initialize(0, &g_smt) != SAI_STATUS_SUCCESS) return 3;
    if (g_delays) ext_macsec_trace("on all");
    if (sai_api_query(SAI_API_SWITCH, (void**)&sw) || sai_api_query(SAI_API_PORT, (void**)&pa) ||
        sai_api_query(SAI_API_ACL, (void**)&aa) || sai_api_query(SAI_API_MACSEC, (void**)&ma))
        return 3;
    sai_attribute_t init = a_bool(SAI_SWITCH_ATTR_INIT_SWITCH, true);
    sai_status_t cst = sw->create_switch(&g_sw, 1, &init);
    std::printf("attach => %d", (int)cst);
    if (cst != SAI_STATUS_SUCCESS) {
        std::printf("\n");
        return 0;
    }
    std::printf("%s\n", bus_take().c_str());
    sai_attribute_t pl = at(SAI_SWITCH_ATTR_PORT_LIST);
    pl.value.objlist.count = 64;
    pl.value.objlist.list = g_ports;
    if (sw->get_switch_attribute(g_sw, 1, &pl) != SAI_STATUS_SUCCESS) return 3;

    std::ifstream f(argv[2]);
    std::string line;
    while (std::getline(f, line)) {
        std::istringstream is(line);
        std::vector<std::string> w;
        for (std::string t; is >> t;) w.push_back(t);
        if (w.empty()) continue;
        const std::string& op = w[0];
        sai_status_t st = SAI_STATUS_SUCCESS;
        std::string extra;
        char b[256];
        sai_object_id_t id = 0, o1 = 0, o2 = 0;
        bool skip = false;
        auto create = [&](sai_status_t s) {
            st = s;
            if (st == SAI_STATUS_SUCCESS) vars[w[1]] = id;
        };
        if (op == "macsec" || op == "mport" || op == "flow" || op == "table" || op == "entry" || op == "sc" || op == "sa") {
            if (vars.count(w[1])) skip = true;
        }
        if (skip) {
        } else if (op == "macsec") {
            sai_attribute_t a[] = {a_s32(SAI_MACSEC_ATTR_DIRECTION, dir_of(w[2]))};
            create(ma->create_macsec(&id, g_sw, 1, a));
        } else if (op == "mport") {
            sai_attribute_t a[] = {a_s32(SAI_MACSEC_PORT_ATTR_MACSEC_DIRECTION, dir_of(w[2])),
                                   a_oid(SAI_MACSEC_PORT_ATTR_PORT_ID, g_ports[num(w[3])])};
            create(ma->create_macsec_port(&id, g_sw, 2, a));
        } else if (op == "flow") {
            sai_attribute_t a[] = {a_s32(SAI_MACSEC_FLOW_ATTR_MACSEC_DIRECTION, dir_of(w[2]))};
            create(ma->create_macsec_flow(&id, g_sw, 1, a));
        } else if (op == "table") {
            int32_t bind = SAI_ACL_BIND_POINT_TYPE_PORT;
            sai_attribute_t bl = at(SAI_ACL_TABLE_ATTR_ACL_BIND_POINT_TYPE_LIST);
            bl.value.s32list.count = 1;
            bl.value.s32list.list = &bind;
            sai_attribute_t a[] = {a_s32(SAI_ACL_TABLE_ATTR_ACL_STAGE, w[2] == "1" ? SAI_ACL_STAGE_INGRESS_MACSEC : SAI_ACL_STAGE_EGRESS_MACSEC), bl,
                                   a_bool(SAI_ACL_TABLE_ATTR_FIELD_ETHER_TYPE, true)};
            create(aa->create_acl_table(&id, g_sw, 3, a));
        } else if (op == "bind") {   // bind <port index> <dir> <table|->
            if (w[3] != "-" && !var(w[3], o1)) skip = true;
            else {
                sai_attribute_t a = a_oid(w[2] == "1" ? SAI_PORT_ATTR_INGRESS_MACSEC_ACL : SAI_PORT_ATTR_EGRESS_MACSEC_ACL, o1);
                st = pa->set_port_attribute(g_ports[num(w[1])], &a);
            }
        } else if (op == "entry") {   // entry <var> <table> <admin> <flow|-> <action 0 none 1 drop 2 forward> <eapol>
            if (!var(w[2], o1) || (w[4] != "-" && !var(w[4], o2))) skip = true;
            else {
                std::vector<sai_attribute_t> a = {a_oid(SAI_ACL_ENTRY_ATTR_TABLE_ID, o1), a_u32(SAI_ACL_ENTRY_ATTR_PRIORITY, 1),
                                                  a_bool(SAI_ACL_ENTRY_ATTR_ADMIN_STATE, w[3] == "1")};
                if (w[6] == "1") {
                    sai_attribute_t et = at(SAI_ACL_ENTRY_ATTR_FIELD_ETHER_TYPE);
                    et.value.aclfield.enable = true;
                    et.value.aclfield.data.u16 = 0x888e;
                    et.value.aclfield.mask.u16 = 0xffff;
                    a.push_back(et);
                }
                if (w[5] != "0") {
                    sai_attribute_t x = at(SAI_ACL_ENTRY_ATTR_ACTION_PACKET_ACTION);
                    x.value.aclaction.enable = true;
                    x.value.aclaction.parameter.s32 = w[5] == "1" ? SAI_PACKET_ACTION_DROP : SAI_PACKET_ACTION_FORWARD;
                    a.push_back(x);
                }
                if (w[4] != "-") {
                    sai_attribute_t x = at(SAI_ACL_ENTRY_ATTR_ACTION_MACSEC_FLOW);
                    x.value.aclaction.enable = true;
                    x.value.aclaction.parameter.oid = o2;
                    a.push_back(x);
                }
                create(aa->create_acl_entry(&id, g_sw, (uint32_t)a.size(), a.data()));
            }
        } else if (op == "eflow") {   // eflow <entry> <flow|->
            if (!var(w[1], o1) || (w[2] != "-" && !var(w[2], o2))) skip = true;
            else {
                sai_attribute_t x = at(SAI_ACL_ENTRY_ATTR_ACTION_MACSEC_FLOW);
                x.value.aclaction.enable = w[2] != "-";
                x.value.aclaction.parameter.oid = o2;
                st = aa->set_acl_entry_attribute(o1, &x);
            }
        } else if (op == "eadmin") {
            if (!var(w[1], o1)) skip = true;
            else {
                sai_attribute_t x = a_bool(SAI_ACL_ENTRY_ATTR_ADMIN_STATE, w[2] == "1");
                st = aa->set_acl_entry_attribute(o1, &x);
            }
        } else if (op == "sc") {   // sc <var> <dir> <flow> <cipher 0..3> <replay> <window hex> <sci last byte hex>
            if (!var(w[3], o1)) skip = true;
            else {
                uint8_t sci_b[8] = {0x1c, 0xea, 0x0b, 0x38, 0x2b, 0xd5, 0x00, (uint8_t)num(w[7])};
                uint64_t sci;
                std::memcpy(&sci, sci_b, 8);
                sai_attribute_t a[] = {a_s32(SAI_MACSEC_SC_ATTR_MACSEC_DIRECTION, dir_of(w[2])), a_oid(SAI_MACSEC_SC_ATTR_FLOW_ID, o1),
                                       a_u64(SAI_MACSEC_SC_ATTR_MACSEC_SCI, sci),
                                       a_s32(SAI_MACSEC_SC_ATTR_MACSEC_CIPHER_SUITE, (int32_t)num(w[4])),
                                       a_bool(SAI_MACSEC_SC_ATTR_MACSEC_REPLAY_PROTECTION_ENABLE, w[5] == "1"),
                                       a_u32(SAI_MACSEC_SC_ATTR_MACSEC_REPLAY_PROTECTION_WINDOW, (uint32_t)num(w[6]))};
                create(ma->create_macsec_sc(&id, g_sw, 6, a));
            }
        } else if (op == "scset") {   // scset <sc> cipher|replay|window <hex>
            if (!var(w[1], o1)) skip = true;
            else {
                sai_attribute_t x = w[2] == "cipher"   ? a_s32(SAI_MACSEC_SC_ATTR_MACSEC_CIPHER_SUITE, (int32_t)num(w[3]))
                                    : w[2] == "replay" ? a_bool(SAI_MACSEC_SC_ATTR_MACSEC_REPLAY_PROTECTION_ENABLE, w[3] == "1")
                                                       : a_u32(SAI_MACSEC_SC_ATTR_MACSEC_REPLAY_PROTECTION_WINDOW, (uint32_t)num(w[3]));
                st = ma->set_macsec_sc_attribute(o1, &x);
            }
        } else if (op == "sa") {   // sa <var> <dir> <sc> <an> <pn hex> <key seed hex>
            if (!var(w[3], o1)) skip = true;
            else {
                int32_t dir = dir_of(w[2]);
                uint8_t seed = (uint8_t)num(w[6]);
                std::vector<sai_attribute_t> a = {
                    a_s32(SAI_MACSEC_SA_ATTR_MACSEC_DIRECTION, dir), a_oid(SAI_MACSEC_SA_ATTR_SC_ID, o1), a_u8(SAI_MACSEC_SA_ATTR_AN, (uint8_t)num(w[4])),
                    a_u64(dir == SAI_MACSEC_DIRECTION_EGRESS ? SAI_MACSEC_SA_ATTR_CONFIGURED_EGRESS_XPN : SAI_MACSEC_SA_ATTR_MINIMUM_INGRESS_XPN, num(w[5]))};
                sai_attribute_t k = at(SAI_MACSEC_SA_ATTR_SAK);
                for (int i = 0; i < 32; i++) k.value.macsecsak[i] = (uint8_t)(seed * 37 + i);
                a.push_back(k);
                k = at(SAI_MACSEC_SA_ATTR_AUTH_KEY);
                for (int i = 0; i < 16; i++) k.value.macsecauthkey[i] = (uint8_t)(0x80 + seed + i);
                a.push_back(k);
                k = at(SAI_MACSEC_SA_ATTR_SALT);
                for (int i = 0; i < 12; i++) k.value.macsecsalt[i] = (uint8_t)(0x50 + seed + i);
                a.push_back(k);
                a.push_back(a_u32(SAI_MACSEC_SA_ATTR_MACSEC_SSCI, 0x01020304u + seed));
                create(ma->create_macsec_sa(&id, g_sw, (uint32_t)a.size(), a.data()));
            }
        } else if (op == "saminpn") {
            if (!var(w[1], o1)) skip = true;
            else {
                sai_attribute_t x = a_u64(SAI_MACSEC_SA_ATTR_MINIMUM_INGRESS_XPN, num(w[2]));
                st = ma->set_macsec_sa_attribute(o1, &x);
            }
        } else if (op == "sapn") {
            if (!var(w[1], o1)) skip = true;
            else {
                sai_attribute_t x = at(SAI_MACSEC_SA_ATTR_CURRENT_XPN);
                st = ma->get_macsec_sa_attribute(o1, 1, &x);
                if (st == SAI_STATUS_SUCCESS) {
                    std::snprintf(b, sizeof(b), " pn=%llx", (unsigned long long)x.value.u64);
                    extra = b;
                }
            }
        } else if (op == "sastat" || op == "flowstat") {
            if (!var(w[1], o1)) skip = true;
            else {
                uint64_t v[2] = {0, 0};
                if (op == "sastat") {
                    sai_stat_id_t ids[2] = {SAI_MACSEC_SA_STAT_OUT_PKTS_ENCRYPTED, SAI_MACSEC_SA_STAT_IN_PKTS_OK};
                    st = ma->get_macsec_sa_stats(o1, 2, ids, v);
                } else {
                    sai_stat_id_t ids[2] = {SAI_MACSEC_FLOW_STAT_CONTROL_PKTS, SAI_MACSEC_FLOW_STAT_OTHER_ERR};
                    st = ma->get_macsec_flow_stats(o1, 2, ids, v);
                }
                if (st == SAI_STATUS_SUCCESS) {
                    std::snprintf(b, sizeof(b), " stats=%llx,%llx", (unsigned long long)v[0], (unsigned long long)v[1]);
                    extra = b;
                }
            }
        } else if (op == "saclr" || op == "flowclr") {
            if (!var(w[1], o1)) skip = true;
            else st = op == "saclr" ? ma->clear_macsec_sa_stats(o1, 0, nullptr) : ma->clear_macsec_flow_stats(o1, 0, nullptr);
        } else if (op == "active") {
            if (!var(w[1], o1)) skip = true;
            else {
                sai_attribute_t x = at(SAI_MACSEC_SC_ATTR_ACTIVE_EGRESS_SA_ID);
                st = ma->get_macsec_sc_attribute(o1, 1, &x);
                if (st == SAI_STATUS_SUCCESS) extra = " active=" + name_of(x.value.oid);
            }
        } else if (op == "lists") {
            if (!var(w[1], o1)) skip = true;
            else {
                sai_object_id_t l1[64], l2[64];
                sai_attribute_t x[2] = {at(SAI_MACSEC_FLOW_ATTR_SC_LIST), at(SAI_MACSEC_FLOW_ATTR_ACL_ENTRY_LIST)};
                x[0].value.objlist.count = 64;
                x[0].value.objlist.list = l1;
                x[1].value.objlist.count = 64;
                x[1].value.objlist.list = l2;
                st = ma->get_macsec_flow_attribute(o1, 2, x);
                if (st == SAI_STATUS_SUCCESS) extra = " scs=" + names(x[0].value.objlist) + " entries=" + names(x[1].value.objlist);
            }
        } else if (op == "caps") {
            if (!var(w[1], o1)) skip = true;
            else {
                sai_attribute_t x[5] = {at(SAI_MACSEC_ATTR_PN_32BIT_SUPPORTED), at(SAI_MACSEC_ATTR_XPN_64BIT_SUPPORTED), at(SAI_MACSEC_ATTR_GCM_AES128_SUPPORTED),
                                        at(SAI_MACSEC_ATTR_GCM_AES256_SUPPORTED), at(SAI_MACSEC_ATTR_AVAILABLE_MACSEC_SA)};
                st = ma->get_macsec_attribute(o1, 5, x);
                sai_stat_capability_t caps[64];
                sai_stat_capability_list_t cl = {64, caps};
                sai_status_t s1 = sai_query_stats_capability(g_sw, SAI_OBJECT_TYPE_MACSEC_SA, &cl);
                uint32_t n_sa = cl.count;
                cl.count = 64;
                sai_status_t s2 = sai_query_stats_capability(g_sw, SAI_OBJECT_TYPE_MACSEC_FLOW, &cl);
                if (st == SAI_STATUS_SUCCESS && s1 == SAI_STATUS_SUCCESS && s2 == SAI_STATUS_SUCCESS) {
                    std::snprintf(b, sizeof(b), " caps=%d%d%d%d sas=%u stats=%u,%u", x[0].value.booldata, x[1].value.booldata, x[2].value.booldata,
                                  x[3].value.booldata, x[4].value.u32, n_sa, cl.count);
                    extra = b;
                }
            }
        } else if (op == "rm") {   // rm <kind> <var>
            if (!var(w[2], o1)) skip = true;
            else {
                const std::string& k = w[1];
                st = k == "macsec" ? ma->remove_macsec(o1) : k == "mport" ? ma->remove_macsec_port(o1) : k == "flow" ? ma->remove_macsec_flow(o1)
                     : k == "sc" ? ma->remove_macsec_sc(o1) : k == "sa" ? ma->remove_macsec_sa(o1) : k == "table" ? aa->remove_acl_table(o1)
                     : aa->remove_acl_entry(o1);
                if (st != SAI_STATUS_OBJECT_IN_USE) vars.erase(w[2]);
            }
        } else if (op == "reg") {   // reg <phy hex> <reg hex> <value hex>
            std::lock_guard<std::mutex> g(fake().mdio_mu);
            fake().mdio_regs[std::make_tuple((unsigned)num(w[1]), 1u, (unsigned)num(w[2]))] = (unsigned)num(w[3]);
#ifdef SAI_MACSEC_DRIVER_CHIP
        } else if (op == "rdreg") {   // rdreg <phy hex> <reg hex> <value hex>|-: device 1 register whose reads see this, not the writes
            std::lock_guard<std::mutex> g(fake().mdio_mu);
            auto key = std::make_tuple((unsigned)num(w[1]), 1u, (unsigned)num(w[2]));
            if (w[3] == "-") chip().rd_only.erase(key);
            else chip().rd_only[key] = (unsigned)num(w[3]);
#endif
        } else if (op == "dead") {   // dead <phy hex> 1|0: every access of that PHY fails (or works again)
            std::lock_guard<std::mutex> g(fake().mdio_mu);
            if (w[2] == "1") fake().mdio_dead.insert((unsigned)num(w[1]));
            else fake().mdio_dead.erase((unsigned)num(w[1]));
        } else if (op == "ifg") {   // ifg <port index>
            std::snprintf(b, sizeof(b), " ifg=%u", fake().port_state[ctx().ports[g_ports[num(w[1])]].hw].ifg);
            extra = b;
        } else if (op == "status") {
            std::string s = ext_macsec_status();
            std::string out;
            std::istringstream ls(s);
            for (std::string l; std::getline(ls, l);) out += "\n| " + l;
            extra = out;
        } else {
            std::fprintf(stderr, "unknown operation: %s\n", line.c_str());
            return 2;
        }
        if (skip) std::printf("%s => skip\n", line.c_str());
        else std::printf("%s => %d%s%s\n", line.c_str(), (int)st, extra.c_str(), bus_take().c_str());
    }
    std::fflush(stdout);
    return 0;
}
