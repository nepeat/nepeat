// SPDX-License-Identifier: Apache-2.0
// Switch (tl_switch.h).
//
// Safety rules:
//  - no readable SAI_INIT_CONFIG_FILE, no port map: refuse to start;
//  - every front-panel devport is set admin-down right after node enable
//    (before the init command files run); the init command interpreter only
//    knows "ifcs set devport" serdes/eq attributes, so "port enable" lines
//    in vendor files are skipped and never enable a port;
//  - shutdown sets every port admin-down before anything else.
#include "tl_switch.h"
#include "tl_sflow.h"

#include "tl_acl.h"
#include "tl_hostif.h"
#include "tl_counters.h"
#include "tl_l2.h"
#include "tl_l3.h"
#include "tl_port.h"

#include <cerrno>
#include <chrono>
#include <cmath>
#include <cstdlib>
#include <cstdio>
#include <fstream>
#include <sstream>

#include <sys/stat.h>
#include <unistd.h>

namespace ob {
namespace tlb {

static bool read_file(const std::string& path, std::string& out)
{
    std::ifstream f(path, std::ios::binary);
    if (!f) return false;
    std::ostringstream s;
    s << f.rdbuf();
    out = s.str();
    return true;
}

static bool write_atomic(const std::string& path, const std::string& data, std::string& err)
{
    std::string tmp = path + ".tmp";
    {
        std::ofstream f(tmp, std::ios::binary | std::ios::trunc);
        if (!f) { err = "cannot write " + tmp + ": " + std::strerror(errno); return false; }
        f << data;
        f.flush();
        if (!f) { err = "write " + tmp + " failed"; return false; }
    }
    if (std::rename(tmp.c_str(), path.c_str()) != 0) {
        err = "rename " + tmp + " -> " + path + ": " + std::strerror(errno);
        return false;
    }
    return true;
}

sai_status_t SwitchArea::init(const BackendConfig& cfg, LinkCallback link_cb, FdbCallback fdb_cb)
{
    c_.cfg = cfg;
    std::string config = c_.key("SAI_INIT_CONFIG_FILE");
    std::string append = c_.key("SAI_TERALYNX_CONFIG_APPEND");
    std::string run_dir = c_.key("SAI_TERALYNX_RUN_DIR", "/run/sai-teralynx");
    std::string libpath = c_.key("SAI_TERALYNX_LIBIFCS");
    std::string portmap = c_.key("SAI_TERALYNX_PORT_MAP");
    std::string cmds = c_.key("SAI_TERALYNX_INIT_CMDS");
    // Default adopt: with the deployed init yaml (netdev auto_create) create
    // mode makes no netdev (chip 2026-11-16, hostif-create).
    bool adopt = c_.key("SAI_TERALYNX_NETDEV", "adopt") == "adopt";
    c_.hostif->configure(adopt);
    if (c_.key("SAI_TERALYNX_NETDEV", "adopt") != "create" && !adopt) {
        OB_ERR(SAI_API_SWITCH, "SAI_TERALYNX_NETDEV must be create|adopt");
        return SAI_STATUS_INVALID_PARAMETER;
    }
    deinit_on_shutdown_ = c_.key("SAI_TERALYNX_SHUTDOWN", "deinit") != "ports-down";

    if (config.empty() || access(config.c_str(), R_OK) != 0) {
        OB_ERR(SAI_API_SWITCH, "SAI_INIT_CONFIG_FILE '%s' not set or unreadable; refusing to start the SDK",
               config.c_str());
        return SAI_STATUS_FAILURE;
    }
    std::string err;
    if (portmap.empty()) {
        OB_ERR(SAI_API_SWITCH, "SAI_TERALYNX_PORT_MAP not set; refusing to start");
        return SAI_STATUS_FAILURE;
    }
    c_.specs.clear();
    if (!tl::parse_port_map(portmap, c_.specs, err)) {
        OB_ERR(SAI_API_SWITCH, "port map: %s", err.c_str());
        return SAI_STATUS_FAILURE;
    }
    tl::InitCmds ic;
    {
        std::stringstream ss(cmds);
        std::string f;
        while (std::getline(ss, f, ':')) {
            if (f.empty()) continue;
            if (!tl::parse_init_cmds(f, ic, err)) {
                OB_ERR(SAI_API_SWITCH, "init commands: %s", err.c_str());
                return SAI_STATUS_FAILURE;
            }
        }
    }
    // Runtime config: the generated init yaml, then the board's isg yaml.
    std::string yaml, extra;
    if (!read_file(config, yaml)) {
        OB_ERR(SAI_API_SWITCH, "cannot read %s", config.c_str());
        return SAI_STATUS_FAILURE;
    }
    if (!append.empty()) {
        if (!read_file(append, extra)) {
            OB_ERR(SAI_API_SWITCH, "SAI_TERALYNX_CONFIG_APPEND %s unreadable; refusing to start", append.c_str());
            return SAI_STATUS_FAILURE;
        }
        if (!yaml.empty() && yaml.back() != '\n') yaml += '\n';
        yaml += extra;
    }
    mkdir(run_dir.c_str(), 0755);   // EEXIST is fine; the write below reports real errors
    run_config_ = run_dir + "/config.yaml";
    if (!write_atomic(run_config_, yaml, err)) {
        OB_ERR(SAI_API_SWITCH, "%s", err.c_str());
        return SAI_STATUS_FAILURE;
    }
    // learn-ring read guard (src/ifcs_guard.c, learn-wedge.md): on unless SAI_TERALYNX_IFCS_GUARD=off
    tl::GuardOptions guard;
    guard.enable = c_.key("SAI_TERALYNX_IFCS_GUARD", "on") != "off";
    guard.stats_path = run_dir + "/ifcs-guard.prom";
    guard.ring_size = static_cast<uint32_t>(std::strtoul(c_.key("SAI_TERALYNX_LEARN_RING_SIZE", "2048").c_str(), nullptr, 0));
    guard.malloc_limit = std::strtoull(c_.key("SAI_TERALYNX_IM_MALLOC_LIMIT_MIB", "1024").c_str(), nullptr, 0) << 20;
    if (!tl::ifcs_lib_load(libpath, c_.L, err, c_.key("SAI_TERALYNX_IFCS_TRACE"), guard)) {
        OB_ERR(SAI_API_SWITCH, "%s", err.c_str());
        return SAI_STATUS_FAILURE;
    }
    c_.link = std::move(link_cb);
    c_.fdb = std::move(fdb_cb);

    // Exactly the vendor's innovium.user (main): init, node create, enable.
    std::string logname = c_.key("SAI_TERALYNX_IFCS_LOG_NAME", "sai-teralynx");
    uint32_t nodes = 0;
    OB_NOTICE(SAI_API_SWITCH, "ifcs_init(%s, %s, COLD) via %s", run_config_.c_str(), logname.c_str(), libpath.c_str());
    int r = IFCS_STATUS_REASON(c_.L.ifcs_init(run_config_.c_str(), logname.c_str(), IFCS_BOOT_COLD, &nodes));
    if (r != IFCS_SUCCESS) {
        OB_ERR(SAI_API_SWITCH, "ifcs_init failed: %s", c_.reason(r));
        return map_reason(r);
    }
    c_.sdk_up = true;
    if (nodes < 1) {
        OB_ERR(SAI_API_SWITCH, "ifcs_init found no switch node");
        deinit();
        return SAI_STATUS_FAILURE;
    }
    if (c_.L.ifcs_log_function_register(&SwitchArea::on_log, this) != 0)
        OB_WARN(SAI_API_SWITCH, "ifcs_log_function_register failed: SDK messages stay in its own log file");
    r = IFCS_STATUS_REASON(c_.L.ifcs_node_create(NODE, 0, nullptr));
    if (r != IFCS_SUCCESS) {
        OB_ERR(SAI_API_SWITCH, "ifcs_node_create(0) failed: %s", c_.reason(r));
        deinit();
        return map_reason(r);
    }
    c_.l2->pre_enable();   // node attributes that must precede ENABLE (tl_l2.h)
    c_.l3->pre_enable();   // L3 interface counter modes (tl_l3.h)
    if (c_.sflow) c_.sflow->pre_enable();   // egress sampling, a boot-time node attribute (tl_sflow.h)
    ifcs_attr_t en;
    c_.L.ifcs_attr_t_init(&en);
    c_.L.ifcs_attr_t_id_set(&en, IFCS_NODE_ATTR_ENABLE);
    c_.L.ifcs_attr_t_value_data_set(&en, IFCS_BOOL_TRUE);
    r = IFCS_STATUS_REASON(c_.L.ifcs_node_attr_set(NODE, 1, &en));
    if (r != IFCS_SUCCESS) {
        OB_ERR(SAI_API_SWITCH, "node enable failed: %s", c_.reason(r));
        deinit();
        return map_reason(r);
    }
    sai_status_t st = post_enable(ic);
    if (st != SAI_STATUS_SUCCESS) {
        c_.port->all_down();
        deinit();
        return st;
    }
    c_.initialized = true;
    OB_NOTICE(SAI_API_SWITCH, "IFCS node 0 enabled: %zu front-panel devports, %zu init commands applied",
              c_.specs.size(), ic.cmds.size());
    return SAI_STATUS_SUCCESS;
}

void SwitchArea::shutdown()
{
    Guard g(c_.mu);
    if (!c_.sdk_up) return;
    // Ports first: a dead or half-torn-down SDK must not leave links up.
    c_.port->all_down();
    c_.L.ifcs_linkscan_event_unregister_notify(NODE);
    c_.L.ifcs_l2_event_unregister_notify(NODE);
    c_.hostif->teardown_all();
    c_.acl->forget();
    c_.link = nullptr;
    c_.fdb = nullptr;
    if (deinit_on_shutdown_) deinit();
    else OB_NOTICE(SAI_API_SWITCH, "SAI_TERALYNX_SHUTDOWN=ports-down: ports down, IFCS left initialised");
    c_.sdk_up = false;
    c_.initialized = false;
}

// After node enable: sysport handles, ports down, init commands, event
// callbacks, the L2 base (STP instance, one mc_dest per port, VLAN 1).
sai_status_t SwitchArea::post_enable(const tl::InitCmds& ic)
{
    Guard g(c_.mu);
    c_.sysport.clear();
    c_.dev_of_sysport.clear();
    std::vector<uint32_t> dps{(uint32_t)CPU_DEVPORT};
    for (auto& s : c_.specs) dps.push_back(s.devport);
    for (uint32_t dp : dps) {
        ifcs_attr_t a = attr_of(IFCS_DEVPORT_ATTR_SYSPORT);
        uint32_t n = 0;
        int r = IFCS_STATUS_REASON(c_.L.ifcs_devport_attr_get(NODE, dp, 1, &a, &n));
        if (r != IFCS_SUCCESS) {
            OB_ERR(SAI_API_SWITCH, "devport %u: no sysport (%s); port map and init config disagree?", dp, c_.reason(r));
            return map_reason(r);
        }
        c_.sysport[(int)dp] = a.value.handle;
        c_.dev_of_sysport[a.value.handle] = (int)dp;
    }
    c_.port->all_down();
    c_.port->configure();   // link cache: every port down from here on
    unsigned ok = 0, bad = 0;
    for (auto& cmd : ic.cmds) {
        if (apply_cmd(cmd)) ok++;
        else bad++;
    }
    for (auto& s : ic.skipped)
        OB_WARN(SAI_API_SWITCH, "init commands: skipped %u '%s' line(s) (only 'ifcs set devport' serdes/eq "
                "attributes are applied)", s.second, s.first.c_str());
    if (bad) OB_ERR(SAI_API_SWITCH, "init commands: %u of %zu failed (see above)", bad, ic.cmds.size());
    OB_NOTICE(SAI_API_SWITCH, "init commands: %u applied", ok);

    sai_status_t st = c_.call(c_.L.ifcs_linkscan_event_register_notify(NODE, &c_, &PortArea::on_link), "linkscan register");
    if (st != SAI_STATUS_SUCCESS) return st;
    st = c_.call(c_.L.ifcs_l2_event_register_notify(NODE, &c_, &L2Area::on_l2), "l2 event register");
    if (st != SAI_STATUS_SUCCESS) return st;
    return c_.l2->base_init(dps);
}

bool SwitchArea::apply_cmd(const tl::DevportCmd& c)
{
    ifcs_attr_t a = attr_of(c.attr);
    std::vector<int8_t> i8;
    std::vector<uint8_t> u8;
    std::vector<uint32_t> u32;
    switch (c.kind) {
    case tl::ValKind::U32: a.value.u32 = (uint32_t)c.values[0]; break;
    case tl::ValKind::I8List:
        for (auto v : c.values) i8.push_back((int8_t)v);
        a.value.i8_list.count = (uint32_t)i8.size();
        a.value.i8_list.arr = i8.data();
        break;
    case tl::ValKind::U8List:
        for (auto v : c.values) u8.push_back((uint8_t)v);
        a.value.u8_list.count = (uint32_t)u8.size();
        a.value.u8_list.arr = u8.data();
        break;
    case tl::ValKind::U32List:
        for (auto v : c.values) u32.push_back((uint32_t)v);
        a.value.u32_list.count = (uint32_t)u32.size();
        a.value.u32_list.arr = u32.data();
        break;
    }
    int r = IFCS_STATUS_REASON(c_.L.ifcs_devport_attr_set(NODE, c.devport, 1, &a));
    if (r != IFCS_SUCCESS) {
        OB_ERR(SAI_API_SWITCH, "%s: devport %u %s: %s", c.where.c_str(), c.devport, c.name.c_str(), c_.reason(r));
        return false;
    }
    return true;
}

void SwitchArea::deinit()
{
    if (!c_.sdk_up) return;
    int r = IFCS_STATUS_REASON(c_.L.ifcs_deinit(IFCS_SHUTDOWN_TYPE_COLD));
    if (r != IFCS_SUCCESS && r != IFCS_UNINIT) OB_ERR(SAI_API_SWITCH, "ifcs_deinit: %s", c_.reason(r));
    c_.sdk_up = false;
}

// Die temperature: IFCS_NODE_ATTR_MASTER_LAST_TEMP and REMOTE0..3 (flt,
// degrees C, as the vendor's scripts/temp.py reads them). Each monitor read
// takes ~400 ms inside libifcs and holds its API lock: any other IFCS call
// waits (oracle trace 2026-11-16: the frontend's 5 s temperature publisher
// made one SAI call in every 5 s wait 2 s, the "62 ms per OPER_STATUS get"
// of the first benchmark). So we read one monitor per refresh, round-robin,
// at most every SAI_TERALYNX_TEMP_PERIOD seconds (default 15; 0 = every
// call), and answer from the cache in between: one 400 ms stall per period,
// each monitor fresh within 5 periods. SAI_TERALYNX_TEMP_MONITORS=master
// reads only the master monitor. A remote monitor outside -40..150 C is
// left out. Peak = current. Thread-safe (own mutex; the publisher calls
// without the SAI lock).
sai_status_t SwitchArea::asic_temperatures(std::vector<int>& cur, std::vector<int>& peak)
{
    if (!c_.initialized) return SAI_STATUS_UNINITIALIZED;
    static const uint32_t ids[5] = {IFCS_NODE_ATTR_MASTER_LAST_TEMP, IFCS_NODE_ATTR_REMOTE0_LAST_TEMP,
                                    IFCS_NODE_ATTR_REMOTE1_LAST_TEMP, IFCS_NODE_ATTR_REMOTE2_LAST_TEMP,
                                    IFCS_NODE_ATTR_REMOTE3_LAST_TEMP};
    std::lock_guard<std::mutex> g(temp_mu_);
    if (temp_period_ < 0) {
        int p = std::atoi(c_.key("SAI_TERALYNX_TEMP_PERIOD", "15").c_str());
        temp_period_ = p < 0 || p > 3600 ? 15 : p;
        temp_n_ = c_.key("SAI_TERALYNX_TEMP_MONITORS", "all") == "master" ? 1 : 5;
    }
    auto now = std::chrono::steady_clock::now();
    bool due = temp_reads_ == 0 || temp_period_ == 0 || now - temp_when_ >= std::chrono::seconds(temp_period_);
    // Until every monitor was read once, read the next one on each call.
    if (temp_reads_ < (unsigned)temp_n_) due = true;
    if (due) {
        int i = temp_next_;
        ifcs_attr_t a = attr_of(ids[i]);
        uint32_t n = 0;
        int r = IFCS_STATUS_REASON(c_.L.ifcs_node_attr_get(NODE, 1, &a, &n));
        if (r != IFCS_SUCCESS) return map_reason(r);
        float t = a.value.flt;
        temp_valid_[i] = std::isfinite(t) && (i == 0 || (t >= -40.0f && t <= 150.0f));
        temp_tenths_[i] = temp_valid_[i] ? (int)std::lround(t * 10.0f) : 0;
        temp_next_ = (temp_next_ + 1) % temp_n_;
        temp_when_ = now;
        temp_reads_++;
    }
    cur.clear();
    for (int i = 0; i < temp_n_; i++)
        if (temp_valid_[i]) cur.push_back(temp_tenths_[i]);
    peak = cur;
    return SAI_STATUS_SUCCESS;
}

sai_status_t SwitchArea::table_usage(TableUsage& u)
{
    Guard g(c_.mu);
    L3Area::Counts n = c_.l3->counts();
    // One TCAM for prefixes, an IPv6 prefix takes two slots (I, fib-capacity.md).
    uint32_t used = n.v4_prefixes + 2 * n.v6_prefixes;
    u.v4_routes_free = used >= CAP_V4_PREFIX ? 0 : CAP_V4_PREFIX - used;
    u.v6_routes_free = std::min(CAP_V6_PREFIX, u.v4_routes_free / 2);
    uint32_t hused = n.v4_hosts + 2 * n.v6_hosts;
    u.hosts_free = hused >= CAP_V4_HOST ? 0 : CAP_V4_HOST - hused;
    ifcs_usage_t us;
    std::memset(&us, 0, sizeof(us));
    u.egress_free = c_.usage_free(c_.L.ifcs_nexthop_usage_get(NODE, 0, nullptr, &us), us, 0, "nexthop");
    std::memset(&us, 0, sizeof(us));
    u.fdb_free = c_.usage_free(c_.L.ifcs_l2_entry_usage_get(NODE, nullptr, 0, nullptr, &us), us, CAP_MAC, "l2_entry");
    std::memset(&us, 0, sizeof(us));
    u.ecmp_groups_free = c_.usage_free(c_.L.ifcs_ecmp_usage_get(NODE, 0, nullptr, &us), us, 0, "ecmp");
    ChipLimits l;
    chip_limits(l);
    u.ecmp_members_free = l.ecmp_members > n.ecmp_members ? l.ecmp_members - n.ecmp_members : 0;
    return SAI_STATUS_SUCCESS;
}

sai_status_t SwitchArea::chip_limits(ChipLimits& l)
{
    Guard g(c_.mu);
    if (!limits_read_) {
        ifcs_attr_t a[3] = {attr_of(IFCS_NODE_ATTR_ACL_ENTRY_MINIMUM_PRIORITY),
                            attr_of(IFCS_NODE_ATTR_ACL_ENTRY_MAXIMUM_PRIORITY),
                            attr_of(IFCS_NODE_ATTR_NUMBER_OF_ECMP_GROUPS)};
        uint32_t n = 0;
        if (IFCS_STATUS_REASON(c_.L.ifcs_node_attr_get(NODE, 3, a, &n)) == IFCS_SUCCESS) {
            limits_.acl_prio_min = a[0].value.u32;
            limits_.acl_prio_max = a[1].value.u32;
            limits_.ecmp_groups = a[2].value.u32;
        } else {
            OB_WARN(SAI_API_SWITCH, "node limits (ACL priority, ECMP groups) unreadable; reporting 0");
        }
        limits_.ecmp_paths = ECMP_PATHS;
        limits_.ecmp_members = limits_.ecmp_groups * ECMP_PATHS;
        limits_.vrf_max = VRF_MAX;
        limits_.buffer_bytes = 0;   // not known from this SDK: SWITCH TOTAL_BUFFER_SIZE reads 0
        limits_read_ = true;
    }
    l = limits_;
    return SAI_STATUS_SUCCESS;
}

// ---------------------------------------------------- diag shell (read-only)
sai_status_t SwitchArea::run_shell(const std::string& cmd)
{
    std::string out;
    sai_status_t st = shell(cmd, out);
    std::istringstream is(out);
    std::string l;
    while (std::getline(is, l)) OB_NOTICE(SAI_API_SWITCH, "shell: %s", l.c_str());
    return st;
}

sai_status_t SwitchArea::run_shell_to(const std::string& cmd, int fd)
{
    std::string out;
    sai_status_t st = shell(cmd, out);
    size_t off = 0;
    while (off < out.size()) {
        ssize_t n = ::write(fd, out.data() + off, out.size() - off);
        if (n <= 0) break;
        off += (size_t)n;
    }
    return st;
}

sai_status_t SwitchArea::shell(const std::string& cmd, std::string& out)
{
    std::istringstream is(cmd);
    std::vector<std::string> t;
    std::string w;
    while (is >> w) t.push_back(w);
    char b[256];
    if (t.empty() || t[0] == "help") {
        out = "commands: ports | devport <id> | temp | copp | acl | counters <devport> | help (read-only)\n";
        return SAI_STATUS_SUCCESS;
    }
    if (!c_.initialized) {
        out = "SDK not initialised\n";
        return SAI_STATUS_UNINITIALIZED;
    }
    if (t[0] == "temp") {
        std::vector<int> c, p;
        sai_status_t st = asic_temperatures(c, p);
        if (st != SAI_STATUS_SUCCESS) { out = "temperature read failed\n"; return st; }
        for (size_t i = 0; i < c.size(); i++) {
            std::snprintf(b, sizeof(b), "monitor %zu: %.1f C\n", i, c[i] / 10.0);
            out += b;
        }
        return SAI_STATUS_SUCCESS;
    }
    if (t[0] == "copp") {
        out = c_.hostif->trap_report();
        return SAI_STATUS_SUCCESS;
    }
    if (t[0] == "acl") {   // SAI ACL tables (tl_acl.cpp)
        out = c_.acl->report();
        return SAI_STATUS_SUCCESS;
    }
    if (t[0] == "counters" && t.size() == 2) {   // every mapped SAI port counter + CPU queues (tl_counters.cpp)
        out = counters_report(c_, std::atoi(t[1].c_str()));
        return SAI_STATUS_SUCCESS;
    }
    if (t[0] == "ports" || (t[0] == "devport" && t.size() == 2)) {
        Guard g(c_.mu);
        for (auto& s : c_.specs) {
            if (t[0] == "devport" && std::to_string(s.devport) != t[1]) continue;
            uint32_t admin = 9, link = 9, speed = 0, fec = 9, an = 9;
            c_.dp_get_u32(s.devport, IFCS_DEVPORT_ATTR_ADMIN_STATE, admin);
            c_.dp_get_u32(s.devport, IFCS_DEVPORT_ATTR_LINK_STATUS, link);
            c_.dp_get_u32(s.devport, IFCS_DEVPORT_ATTR_SPEED, speed);
            c_.dp_get_u32(s.devport, IFCS_DEVPORT_ATTR_FEC_MODE, fec);
            c_.dp_get_u32(s.devport, IFCS_DEVPORT_ATTR_AUTO_NEG, an);
            std::snprintf(b, sizeof(b), "devport %u: admin %u link %s speed %u fec %u autoneg %u sysport 0x%x\n",
                          s.devport, admin, link == 1 ? "up" : "down", speed_from_enum(speed), fec, an,
                          c_.sysport.count((int)s.devport) ? c_.sysport.at((int)s.devport) : 0);
            out += b;
        }
        if (out.empty()) { out = "no such devport\n"; return SAI_STATUS_INVALID_PARAMETER; }
        return SAI_STATUS_SUCCESS;
    }
    out = "not supported: " + cmd + "\n";
    return SAI_STATUS_NOT_SUPPORTED;
}

void Core::sw_limits(ChipLimits& l) { sw->chip_limits(l); }

void SwitchArea::on_log(int level, void*, const char* msg)
{
    if (!msg) return;
    sai_log_level_t l = level <= IFCS_LOG_LEVEL_ERROR ? SAI_LOG_LEVEL_ERROR
                      : level == IFCS_LOG_LEVEL_WARN ? SAI_LOG_LEVEL_WARN
                      : level == IFCS_LOG_LEVEL_INFO ? SAI_LOG_LEVEL_INFO : SAI_LOG_LEVEL_DEBUG;
    OB_LOG(SAI_API_SWITCH, l, "ifcs: %s", msg);
}

} // namespace tlb
} // namespace ob
