// SPDX-License-Identifier: Apache-2.0
// The IFCS backend: sai-openbcm's backend interface (backend.h) on the
// Innovium/Marvell IFCS 0.12.41 SDK of the Teralynx 7 (CS8210-32X). The SAI
// frontend is sai-openbcm's, unmodified; this file is its "openbcm" backend
// slot (make_openbcm_backend), selected with SAI_OPENBCM_BACKEND=openbcm.
//
// libifcs.so is dlopen'ed at init (SAI_TERALYNX_LIBIFCS); every call goes
// through the IfcsLib table. Call forms follow the vendor's own code (the CLI
// python and sample scripts of the SDK, the innovium.user disassembly); where
// a form is inferred instead, the function says so and README.md lists it.
//
// Safety rules:
//  - no readable SAI_INIT_CONFIG_FILE, no port map: refuse to start;
//  - every front-panel devport is set admin-down right after node enable
//    (before the init command files run); the init command interpreter only
//    knows "ifcs set devport" serdes/eq attributes, so "port enable" lines
//    in vendor files are skipped and never enable a port;
//  - shutdown sets every port admin-down before anything else.
#include "backend.h"
#include "sai_internal.h"

#include "ifcs_api.h"
#include "ifcs_config.h"
#include "ifcs_lib.h"

#include <algorithm>
#include <cerrno>
#include <chrono>
#include <cmath>
#include <cstdarg>
#include <cstdio>
#include <cstring>
#include <fstream>
#include <map>
#include <mutex>
#include <set>
#include <sstream>

#include <fcntl.h>
#include <net/if.h>
#include <net/if_arp.h>
#include <sys/ioctl.h>
#include <sys/socket.h>
#include <sys/stat.h>
#include <unistd.h>

namespace ob {

using tl::IfcsLib;

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

static sai_status_t map_reason(int r)
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

static ifcs_attr_t attr_of(uint32_t id)
{
    ifcs_attr_t a;
    std::memset(&a, 0, sizeof(a));   // as ctypes ifcs_attr_t(): bitmap 0, value assigned directly
    a.id = id;
    return a;
}
static ifcs_attr_t attr_u32(uint32_t id, uint32_t v) { ifcs_attr_t a = attr_of(id); a.value.u32 = v; return a; }
static ifcs_attr_t attr_u16(uint32_t id, uint16_t v) { ifcs_attr_t a = attr_of(id); a.value.u16 = v; return a; }
static ifcs_attr_t attr_data(uint32_t id, bool v) { ifcs_attr_t a = attr_of(id); a.value.data = v ? IFCS_BOOL_TRUE : IFCS_BOOL_FALSE; return a; }
static ifcs_attr_t attr_handle(uint32_t id, ifcs_handle_t h) { ifcs_attr_t a = attr_of(id); a.value.handle = h; return a; }
static ifcs_attr_t attr_mac(uint32_t id, const uint8_t m[6]) { ifcs_attr_t a = attr_of(id); std::memcpy(a.value.mac, m, 6); return a; }
static ifcs_attr_t attr_fwd(uint32_t id, int action)
{
    ifcs_attr_t a = attr_of(id);
    a.value.fwd_policy.fwd_action = action;
    return a;
}
static ifcs_attr_t attr_ctc(uint32_t id, bool enable, ifcs_handle_t trap)
{
    ifcs_attr_t a = attr_of(id);
    a.value.ctc_policy.ctc_action = enable ? IFCS_COPY_TO_CPU_ENABLE : IFCS_COPY_TO_CPU_DISABLE;
    a.value.ctc_policy.trap_handle = trap;
    return a;
}

static uint32_t speed_from_enum(uint32_t e)
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

class IfcsBackend : public Backend {
public:
    ~IfcsBackend() override
    {
        if (initialized_) shutdown();
    }
    const char* name() const override { return "teralynx"; }
    bool is_fake() const override { return false; }

    // ------------------------------------------------------------ init
    sai_status_t init(const BackendConfig& cfg, LinkCallback link_cb, FdbCallback fdb_cb) override
    {
        cfg_ = cfg;
        std::string config = key("SAI_INIT_CONFIG_FILE");
        std::string append = key("SAI_TERALYNX_CONFIG_APPEND");
        std::string run_dir = key("SAI_TERALYNX_RUN_DIR", "/run/sai-teralynx");
        std::string libpath = key("SAI_TERALYNX_LIBIFCS");
        std::string portmap = key("SAI_TERALYNX_PORT_MAP");
        std::string cmds = key("SAI_TERALYNX_INIT_CMDS");
        netdev_adopt_ = key("SAI_TERALYNX_NETDEV", "create") == "adopt";
        if (key("SAI_TERALYNX_NETDEV", "create") != "create" && !netdev_adopt_) {
            OB_ERR(SAI_API_SWITCH, "SAI_TERALYNX_NETDEV must be create|adopt");
            return SAI_STATUS_INVALID_PARAMETER;
        }
        deinit_on_shutdown_ = key("SAI_TERALYNX_SHUTDOWN", "deinit") != "ports-down";

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
        specs_.clear();
        if (!tl::parse_port_map(portmap, specs_, err)) {
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
        if (!tl::ifcs_lib_load(libpath, L_, err)) {
            OB_ERR(SAI_API_SWITCH, "%s", err.c_str());
            return SAI_STATUS_FAILURE;
        }
        link_ = std::move(link_cb);
        fdb_ = std::move(fdb_cb);

        // Exactly the vendor's innovium.user (main): init, node create, enable.
        std::string logname = key("SAI_TERALYNX_IFCS_LOG_NAME", "sai-teralynx");
        uint32_t nodes = 0;
        OB_NOTICE(SAI_API_SWITCH, "ifcs_init(%s, %s, COLD) via %s", run_config_.c_str(), logname.c_str(), libpath.c_str());
        int r = IFCS_STATUS_REASON(L_.ifcs_init(run_config_.c_str(), logname.c_str(), IFCS_BOOT_COLD, &nodes));
        if (r != IFCS_SUCCESS) {
            OB_ERR(SAI_API_SWITCH, "ifcs_init failed: %s", reason(r));
            return map_reason(r);
        }
        sdk_up_ = true;
        if (nodes < 1) {
            OB_ERR(SAI_API_SWITCH, "ifcs_init found no switch node");
            deinit();
            return SAI_STATUS_FAILURE;
        }
        if (L_.ifcs_log_function_register(&IfcsBackend::on_log, this) != 0)
            OB_WARN(SAI_API_SWITCH, "ifcs_log_function_register failed: SDK messages stay in its own log file");
        r = IFCS_STATUS_REASON(L_.ifcs_node_create(NODE, 0, nullptr));
        if (r != IFCS_SUCCESS) {
            OB_ERR(SAI_API_SWITCH, "ifcs_node_create(0) failed: %s", reason(r));
            deinit();
            return map_reason(r);
        }
        ifcs_attr_t en;
        L_.ifcs_attr_t_init(&en);
        L_.ifcs_attr_t_id_set(&en, IFCS_NODE_ATTR_ENABLE);
        L_.ifcs_attr_t_value_data_set(&en, IFCS_BOOL_TRUE);
        r = IFCS_STATUS_REASON(L_.ifcs_node_attr_set(NODE, 1, &en));
        if (r != IFCS_SUCCESS) {
            OB_ERR(SAI_API_SWITCH, "node enable failed: %s", reason(r));
            deinit();
            return map_reason(r);
        }
        sai_status_t st = post_enable(ic);
        if (st != SAI_STATUS_SUCCESS) {
            all_ports_down();
            deinit();
            return st;
        }
        initialized_ = true;
        OB_NOTICE(SAI_API_SWITCH, "IFCS node 0 enabled: %zu front-panel devports, %zu init commands applied",
                  specs_.size(), ic.cmds.size());
        return SAI_STATUS_SUCCESS;
    }

    void shutdown() override
    {
        std::lock_guard<std::recursive_mutex> g(mu_);
        if (!sdk_up_) return;
        // Ports first: a dead or half-torn-down SDK must not leave links up.
        all_ports_down();
        L_.ifcs_linkscan_event_unregister_notify(NODE);
        L_.ifcs_l2_event_unregister_notify(NODE);
        for (auto& n : netifs_) netif_teardown(n.second);
        netifs_.clear();
        link_ = nullptr;
        fdb_ = nullptr;
        if (deinit_on_shutdown_) deinit();
        else OB_NOTICE(SAI_API_SWITCH, "SAI_TERALYNX_SHUTDOWN=ports-down: ports down, IFCS left initialised");
        sdk_up_ = false;
        initialized_ = false;
    }

    // ---------------------------------------------------------- diag shell
    sai_status_t run_shell(const std::string& cmd) override
    {
        std::string out;
        sai_status_t st = shell(cmd, out);
        std::istringstream is(out);
        std::string l;
        while (std::getline(is, l)) OB_NOTICE(SAI_API_SWITCH, "shell: %s", l.c_str());
        return st;
    }
    sai_status_t run_shell_to(const std::string& cmd, int fd) override
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

    int cpu_port() const override { return CPU_DEVPORT; }

    std::vector<PortInfo> ports() override
    {
        std::lock_guard<std::recursive_mutex> g(mu_);
        std::vector<PortInfo> v;
        for (auto& s : specs_) {
            PortInfo p;
            p.hw = (int)s.devport;
            p.lanes = s.lanes;
            p.speed = s.speed;
            p.name = "dp" + std::to_string(s.devport);
            uint32_t an = 0;
            if (dp_get_u32(s.devport, IFCS_DEVPORT_ATTR_AUTO_NEG, an) == SAI_STATUS_SUCCESS) p.autoneg = an != 0;
            v.push_back(p);
        }
        return v;
    }

    std::string hardware_info() override { return "teralynx7 ifcs-0.12.41"; }

    // Die temperature: IFCS_NODE_ATTR_MASTER_LAST_TEMP and REMOTE0..3 (flt,
    // degrees C), read in one call like the vendor's scripts/temp.py. A remote
    // monitor outside -40..150 C is left out. Peak = current (IFCS keeps no
    // peak we know of). Thread-safe: libifcs locks internally (I), our state
    // is not touched.
    sai_status_t asic_temperatures(std::vector<int>& cur, std::vector<int>& peak) override
    {
        if (!initialized_) return SAI_STATUS_UNINITIALIZED;
        ifcs_attr_t a[5];
        const uint32_t ids[5] = {IFCS_NODE_ATTR_MASTER_LAST_TEMP, IFCS_NODE_ATTR_REMOTE0_LAST_TEMP,
                                 IFCS_NODE_ATTR_REMOTE1_LAST_TEMP, IFCS_NODE_ATTR_REMOTE2_LAST_TEMP,
                                 IFCS_NODE_ATTR_REMOTE3_LAST_TEMP};
        for (int i = 0; i < 5; i++) a[i] = attr_of(ids[i]);
        uint32_t n = 0;
        int r = IFCS_STATUS_REASON(L_.ifcs_node_attr_get(NODE, 5, a, &n));
        if (r != IFCS_SUCCESS) return map_reason(r);
        cur.clear();
        for (int i = 0; i < 5; i++) {
            float c = a[i].value.flt;
            if (!std::isfinite(c)) continue;
            if (i > 0 && (c < -40.0f || c > 150.0f)) continue;
            cur.push_back((int)std::lround(c * 10.0f));
        }
        peak = cur;
        return SAI_STATUS_SUCCESS;
    }

    sai_status_t table_usage(TableUsage& u) override
    {
        std::lock_guard<std::recursive_mutex> g(mu_);
        uint32_t v4p = 0, v6p = 0, v4h = 0, v6h = 0;
        for (auto& r : routes_) (r.first.second.addr.v6 ? v6p : v4p)++;
        for (auto& h : hosts_) (h.first.second.v6 ? v6h : v4h)++;
        // One TCAM for prefixes, an IPv6 prefix takes two slots (I, fib-capacity.md).
        uint32_t used = v4p + 2 * v6p;
        u.v4_routes_free = used >= CAP_V4_PREFIX ? 0 : CAP_V4_PREFIX - used;
        u.v6_routes_free = std::min(CAP_V6_PREFIX, u.v4_routes_free / 2);
        uint32_t hused = v4h + 2 * v6h;
        u.hosts_free = hused >= CAP_V4_HOST ? 0 : CAP_V4_HOST - hused;
        ifcs_usage_t us;
        std::memset(&us, 0, sizeof(us));
        u.egress_free = usage_free(L_.ifcs_nexthop_usage_get(NODE, 0, nullptr, &us), us, 0, "nexthop");
        std::memset(&us, 0, sizeof(us));
        u.fdb_free = usage_free(L_.ifcs_l2_entry_usage_get(NODE, nullptr, 0, nullptr, &us), us, CAP_MAC, "l2_entry");
        std::memset(&us, 0, sizeof(us));
        u.ecmp_groups_free = usage_free(L_.ifcs_ecmp_usage_get(NODE, 0, nullptr, &us), us, 0, "ecmp");
        uint32_t members = 0;
        for (auto& e : ecmps_) members += (uint32_t)e.second.members.size();
        ChipLimits l;
        chip_limits(l);
        u.ecmp_members_free = l.ecmp_members > members ? l.ecmp_members - members : 0;
        return SAI_STATUS_SUCCESS;
    }

    sai_status_t chip_limits(ChipLimits& l) override
    {
        std::lock_guard<std::recursive_mutex> g(mu_);
        if (!limits_read_) {
            ifcs_attr_t a[3] = {attr_of(IFCS_NODE_ATTR_ACL_ENTRY_MINIMUM_PRIORITY),
                                attr_of(IFCS_NODE_ATTR_ACL_ENTRY_MAXIMUM_PRIORITY),
                                attr_of(IFCS_NODE_ATTR_NUMBER_OF_ECMP_GROUPS)};
            uint32_t n = 0;
            if (IFCS_STATUS_REASON(L_.ifcs_node_attr_get(NODE, 3, a, &n)) == IFCS_SUCCESS) {
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

    // ------------------------------------------------------------- ports
    sai_status_t port_admin_set(int hw, bool up) override
    {
        if (!front(hw)) return SAI_STATUS_INVALID_PORT_NUMBER;
        return dp_set(hw, attr_u32(IFCS_DEVPORT_ATTR_ADMIN_STATE, up ? 1 : 0), "admin_state");
    }
    sai_status_t port_link_get(int hw, bool& up) override
    {
        if (!front(hw)) return SAI_STATUS_INVALID_PORT_NUMBER;
        uint32_t v = 0;
        sai_status_t st = dp_get_u32(hw, IFCS_DEVPORT_ATTR_LINK_STATUS, v);
        if (st == SAI_STATUS_SUCCESS) up = v == IFCS_DEVPORT_LINK_STATUS_UP;
        return st;
    }
    sai_status_t port_speed_get(int hw, uint32_t& mbps) override
    {
        const tl::PortSpec* s = front(hw);
        if (!s) return SAI_STATUS_INVALID_PORT_NUMBER;
        uint32_t e = 0;
        sai_status_t st = dp_get_u32(hw, IFCS_DEVPORT_ATTR_SPEED, e);
        if (st != SAI_STATUS_SUCCESS) return st;
        mbps = speed_from_enum(e);
        if (!mbps) mbps = s->speed;
        return SAI_STATUS_SUCCESS;
    }
    // Speed is fixed by the init yaml (devport create); IFCS has no safe live
    // change we know of. The configured speed is accepted, anything else is not.
    sai_status_t port_speed_set(int hw, uint32_t mbps) override
    {
        uint32_t cur = 0;
        sai_status_t st = port_speed_get(hw, cur);
        if (st != SAI_STATUS_SUCCESS) return st;
        if (mbps == cur) return SAI_STATUS_SUCCESS;
        OB_WARN(SAI_API_PORT, "devport %d: speed %u -> %u not supported (set by the init config)", hw, cur, mbps);
        return SAI_STATUS_NOT_SUPPORTED;
    }
    sai_status_t port_supported_speeds(int hw, std::vector<uint32_t>& out) override
    {
        uint32_t cur = 0;
        sai_status_t st = port_speed_get(hw, cur);
        if (st != SAI_STATUS_SUCCESS) return st;
        out.assign(1, cur);
        return SAI_STATUS_SUCCESS;
    }
    // Frame size limit: IFCS_SYSPORT_ATTR_MTU (u16, the SDK's per-sysport L2
    // MTU; libifcs: ii_sysport_l2mtu_set). The frontend passes SAI MTU + 8;
    // we add the Ethernet header (14) so a full tagged frame passes, clamped
    // to 9290, the smallest upper bound in libifcs' range checks (I).
    sai_status_t port_frame_max_set(int hw, uint32_t bytes) override
    {
        if (!front(hw)) return SAI_STATUS_INVALID_PORT_NUMBER;
        uint32_t v = bytes + 14;
        if (v > 9290) {
            OB_WARN(SAI_API_PORT, "devport %d: frame %u B clamped to the SDK L2 MTU limit 9290", hw, v);
            v = 9290;
        }
        return sp_set(hw, attr_u16(IFCS_SYSPORT_ATTR_MTU, (uint16_t)v), "mtu");
    }
    // SAI RS: a port already on an RS mode (KR = RS528, KP = RS544, as the
    // init yaml chose per port mode) keeps it; otherwise KP for >= 200G or
    // PAM4 lanes (>= 50G per lane), KR for the rest (the vendor's
    // dynamic_config.sh FEC tables follow this split). FC -> FC.
    sai_status_t port_fec_set(int hw, Fec f) override
    {
        const tl::PortSpec* s = front(hw);
        if (!s) return SAI_STATUS_INVALID_PORT_NUMBER;
        uint32_t cur = 0xffffffffu;
        dp_get_u32(hw, IFCS_DEVPORT_ATTR_FEC_MODE, cur);
        uint32_t lane = s->speed / (uint32_t)s->lanes.size();
        uint32_t want;
        if (f == Fec::None) want = IFCS_DEVPORT_FEC_MODE_NONE;
        else if (f == Fec::Fc) want = IFCS_DEVPORT_FEC_MODE_FC;
        else if (cur == IFCS_DEVPORT_FEC_MODE_KR || cur == IFCS_DEVPORT_FEC_MODE_KP) want = cur;
        else if (s->speed >= 200000 || lane >= 50000) want = IFCS_DEVPORT_FEC_MODE_KP;
        else want = IFCS_DEVPORT_FEC_MODE_KR;
        if (cur == want) return SAI_STATUS_SUCCESS;
        return dp_set(hw, attr_u32(IFCS_DEVPORT_ATTR_FEC_MODE, want), "fec_mode");
    }
    sai_status_t port_autoneg_set(int hw, bool on) override
    {
        if (!front(hw)) return SAI_STATUS_INVALID_PORT_NUMBER;
        uint32_t cur = 2;
        if (dp_get_u32(hw, IFCS_DEVPORT_ATTR_AUTO_NEG, cur) == SAI_STATUS_SUCCESS && (cur != 0) == on)
            return SAI_STATUS_SUCCESS;
        return dp_set(hw, attr_u32(IFCS_DEVPORT_ATTR_AUTO_NEG, on ? 1 : 0), "auto_neg");
    }
    // SAI MAC loopback -> IFCS PCS loopback, PHY -> PMA (I: nearest layers).
    sai_status_t port_loopback_set(int hw, Loopback lb) override
    {
        if (!front(hw)) return SAI_STATUS_INVALID_PORT_NUMBER;
        uint32_t v = lb == Loopback::Mac ? IFCS_DEVPORT_LOOPBACK_PCS
                   : lb == Loopback::Phy ? IFCS_DEVPORT_LOOPBACK_PMA : IFCS_DEVPORT_LOOPBACK_NONE;
        return dp_set(hw, attr_u32(IFCS_DEVPORT_ATTR_LOOPBACK, v), "loopback");
    }
    sai_status_t port_pvid_set(int hw, uint16_t vid) override
    {
        if (!front(hw)) return SAI_STATUS_INVALID_PORT_NUMBER;
        return sp_set(hw, attr_u16(IFCS_SYSPORT_ATTR_DEFAULT_CVID, vid), "default_cvid");
    }
    sai_status_t port_learn_set(int hw, Learn mode) override
    {
        if (!front(hw)) return SAI_STATUS_INVALID_PORT_NUMBER;
        if (mode == Learn::Drop || mode == Learn::Cpu) return not_impl("learning mode drop/cpu");
        return sp_set(hw, attr_data(IFCS_SYSPORT_ATTR_LEARN_ENABLE, mode == Learn::Hw), "learn_enable");
    }

    // Counters: one ifcs_devport_stats_get for all ids, called like the
    // vendor CLI (count = IFCS_DEVPORT_STATS_ID_ALL, results indexed by id),
    // cached 200 ms because the frontend asks one counter at a time.
    sai_status_t port_stat_get(int hw, PortStat s, uint64_t& v) override
    {
        std::lock_guard<std::recursive_mutex> g(mu_);
        if (!front(hw)) return SAI_STATUS_INVALID_PORT_NUMBER;
        StatCache& c = stats_[hw];
        auto now = std::chrono::steady_clock::now();
        if (!c.valid || now - c.when > std::chrono::milliseconds(200)) {
            int ids[IFCS_DEVPORT_STATS_ID_ALL];
            std::memset(ids, 0, sizeof(ids));
            uint64_t vals[IFCS_DEVPORT_STATS_ID_ALL];
            std::memset(vals, 0, sizeof(vals));
            uint32_t n = 0;
            int r = IFCS_STATUS_REASON(L_.ifcs_devport_stats_get(NODE, (ifcs_devport_t)hw, IFCS_DEVPORT_STATS_ID_ALL,
                                                                 ids, vals, &n));
            if (r != IFCS_SUCCESS && r != IFCS_UNSUPPORTED) return fail(r, "devport stats get");
            std::memcpy(c.v, vals, sizeof(c.v));
            c.valid = true;
            c.when = now;
        }
        const uint64_t* x = c.v;
        switch (s) {
        case PortStat::InOctets: v = x[IFCS_DEVPORT_STATS_ID_RX_BYTES_ALL]; break;
        case PortStat::InUcastPkts: v = x[IFCS_DEVPORT_STATS_ID_RX_FRAMES_UNICAST]; break;
        case PortStat::InNonUcastPkts:
            v = x[IFCS_DEVPORT_STATS_ID_RX_FRAMES_MULTICAST] + x[IFCS_DEVPORT_STATS_ID_RX_FRAMES_BROADCAST];
            break;
        case PortStat::InDiscards: v = x[IFCS_DEVPORT_STATS_ID_RX_FRAMES_DROP]; break;
        case PortStat::InErrors: v = x[IFCS_DEVPORT_STATS_ID_RX_FRAMES_ERR_ANY]; break;
        case PortStat::InUnknownProtos: v = 0; break;   // no such MAC counter
        case PortStat::InBroadcastPkts: v = x[IFCS_DEVPORT_STATS_ID_RX_FRAMES_BROADCAST]; break;
        case PortStat::InMulticastPkts: v = x[IFCS_DEVPORT_STATS_ID_RX_FRAMES_MULTICAST]; break;
        case PortStat::OutOctets: v = x[IFCS_DEVPORT_STATS_ID_TX_BYTES_ALL]; break;
        case PortStat::OutUcastPkts: v = x[IFCS_DEVPORT_STATS_ID_TX_FRAMES_UNICAST]; break;
        case PortStat::OutNonUcastPkts:
            v = x[IFCS_DEVPORT_STATS_ID_TX_FRAMES_MULTICAST] + x[IFCS_DEVPORT_STATS_ID_TX_FRAMES_BROADCAST];
            break;
        case PortStat::OutDiscards: v = x[IFCS_DEVPORT_STATS_ID_TX_FRAMES_DROP]; break;
        case PortStat::OutErrors: v = x[IFCS_DEVPORT_STATS_ID_TX_FRAMES_ERR_ANY]; break;
        case PortStat::OutBroadcastPkts: v = x[IFCS_DEVPORT_STATS_ID_TX_FRAMES_BROADCAST]; break;
        case PortStat::OutMulticastPkts: v = x[IFCS_DEVPORT_STATS_ID_TX_FRAMES_MULTICAST]; break;
        default: return SAI_STATUS_NOT_SUPPORTED;
        }
        return SAI_STATUS_SUCCESS;
    }
    sai_status_t port_stat_clear(int hw) override
    {
        std::lock_guard<std::recursive_mutex> g(mu_);
        if (!front(hw)) return SAI_STATUS_INVALID_PORT_NUMBER;
        int ids[IFCS_DEVPORT_STATS_ID_MAX_COUNT];
        for (int i = 0; i < IFCS_DEVPORT_STATS_ID_MAX_COUNT; i++) ids[i] = i;
        int r = IFCS_STATUS_REASON(L_.ifcs_devport_stats_clear(NODE, (ifcs_devport_t)hw, IFCS_DEVPORT_STATS_ID_MAX_COUNT, ids));
        stats_[hw].valid = false;
        if (r != IFCS_SUCCESS) return fail(r, "devport stats clear");
        return SAI_STATUS_SUCCESS;
    }

    // ------------------------------------------------------------- VLANs
    // A VLAN is the knet pattern (SDK/cmds/knet.py): an L2VNI with handle
    // IFCS_HANDLE_L2VNI(vid), the shared STP instance, and its own flood group
    // (MDG) of per-port mc_dests. The CPU sysport is a member of every L2VNI and
    // its mc_dest is in every flood group (floods reach the netdevs), never
    // reported as a member. Untagged egress is per sysport
    // (EGRESS_UNTAGGED_VLAN_ID[_ENABLE]): one untagged VLAN per port.
    sai_status_t vlan_create(uint16_t vid) override
    {
        std::lock_guard<std::recursive_mutex> g(mu_);
        if (vid < 1 || vid > 4094) return SAI_STATUS_INVALID_PARAMETER;
        if (vlans_.count(vid)) return SAI_STATUS_ITEM_ALREADY_EXISTS;
        return vlan_make(vid);
    }
    sai_status_t vlan_destroy(uint16_t vid) override
    {
        std::lock_guard<std::recursive_mutex> g(mu_);
        auto it = vlans_.find(vid);
        if (it == vlans_.end()) return SAI_STATUS_ITEM_NOT_FOUND;
        if (vid == 1) return SAI_STATUS_OBJECT_IN_USE;
        for (auto m : std::vector<std::pair<int, bool>>(it->second.members.begin(), it->second.members.end()))
            vlan_port_remove(vid, m.first);
        Vlan& v = it->second;
        ifcs_handle_t cpu_sp = sysport_.at(CPU_DEVPORT), cpu_mcd = mcdest_.at(CPU_DEVPORT);
        call(L_.ifcs_mdg_mc_dest_remove(NODE, v.mdg, 1, &cpu_mcd), "mdg_mc_dest_remove(cpu)");
        call(L_.ifcs_l2vni_member_remove(NODE, v.l2vni, 1, &cpu_sp), "l2vni_member_remove(cpu)");
        sai_status_t st = call(L_.ifcs_l2vni_delete(NODE, v.l2vni), "l2vni_delete");
        if (st != SAI_STATUS_SUCCESS) return st;
        call(L_.ifcs_mdg_delete(NODE, v.mdg), "mdg_delete");
        vlans_.erase(it);
        return SAI_STATUS_SUCCESS;
    }
    sai_status_t vlan_port_add(uint16_t vid, int hw, bool untagged) override
    {
        std::lock_guard<std::recursive_mutex> g(mu_);
        auto it = vlans_.find(vid);
        if (it == vlans_.end()) return SAI_STATUS_ITEM_NOT_FOUND;
        if (is_lag_dest(hw)) return not_impl("LAG as VLAN member");
        if (!front(hw)) return SAI_STATUS_INVALID_PORT_NUMBER;
        if (untagged) {
            auto u = untagged_vid_.find(hw);
            if (u != untagged_vid_.end() && u->second != vid) {
                OB_ERR(SAI_API_VLAN, "devport %d is already untagged in VLAN %u; IFCS has one untagged VLAN per port",
                       hw, u->second);
                return SAI_STATUS_NOT_SUPPORTED;
            }
        }
        Vlan& v = it->second;
        if (!v.members.count(hw)) {
            ifcs_handle_t sp = sysport_.at(hw), mcd = mcdest_.at(hw);
            ifcs_attr_t none = attr_of(0);
            sai_status_t st = call(L_.ifcs_l2vni_member_add(NODE, v.l2vni, 1, &sp, 0, &none), "l2vni_member_add");
            if (st != SAI_STATUS_SUCCESS) return st;
            st = call(L_.ifcs_mdg_mc_dest_add(NODE, v.mdg, 1, &mcd, 0, &none), "mdg_mc_dest_add");
            if (st != SAI_STATUS_SUCCESS) {
                L_.ifcs_l2vni_member_remove(NODE, v.l2vni, 1, &sp);
                return st;
            }
        }
        if (untagged) {
            ifcs_attr_t a[2] = {attr_u16(IFCS_SYSPORT_ATTR_EGRESS_UNTAGGED_VLAN_ID, vid),
                                attr_data(IFCS_SYSPORT_ATTR_EGRESS_UNTAGGED_VLAN_ID_ENABLE, true)};
            sai_status_t st = call(L_.ifcs_sysport_attr_set(NODE, sysport_.at(hw), 2, a), "sysport egress untagged");
            if (st != SAI_STATUS_SUCCESS) return st;
            untagged_vid_[hw] = vid;
        } else if (untagged_vid_.count(hw) && untagged_vid_[hw] == vid) {
            untagged_off(hw);
        }
        v.members[hw] = untagged;
        return SAI_STATUS_SUCCESS;
    }
    sai_status_t vlan_port_remove(uint16_t vid, int hw) override
    {
        std::lock_guard<std::recursive_mutex> g(mu_);
        auto it = vlans_.find(vid);
        if (it == vlans_.end()) return SAI_STATUS_ITEM_NOT_FOUND;
        Vlan& v = it->second;
        if (!v.members.count(hw)) return SAI_STATUS_SUCCESS;
        ifcs_handle_t sp = sysport_.at(hw), mcd = mcdest_.at(hw);
        call(L_.ifcs_mdg_mc_dest_remove(NODE, v.mdg, 1, &mcd), "mdg_mc_dest_remove");
        sai_status_t st = call(L_.ifcs_l2vni_member_remove(NODE, v.l2vni, 1, &sp), "l2vni_member_remove");
        if (untagged_vid_.count(hw) && untagged_vid_[hw] == vid) untagged_off(hw);
        v.members.erase(hw);
        return st;
    }
    sai_status_t vlan_members(uint16_t vid, std::vector<std::pair<int, bool>>& out) override
    {
        std::lock_guard<std::recursive_mutex> g(mu_);
        auto it = vlans_.find(vid);
        if (it == vlans_.end()) return SAI_STATUS_ITEM_NOT_FOUND;
        out.assign(it->second.members.begin(), it->second.members.end());
        return SAI_STATUS_SUCCESS;
    }

    // -------------------------------------------------------------- LAGs
    // Not mapped: IFCS needs members detached from every L2VNI / mc_dest first
    // and the LAG handle's use as L2VNI member / mc_dest / STP port is unknown
    // (switchd/DESIGN.md U13).
    sai_status_t lag_create(int& tid) override { tid = -1; return not_impl("LAG"); }
    sai_status_t lag_destroy(int) override { return not_impl("LAG"); }
    sai_status_t lag_set_members(int, const std::vector<LagMember>&) override { return not_impl("LAG"); }

    // ------------------------------------------------------------ L2 table
    // ifcs_l2_entry_create with a MAC_L2VNI key built through the key
    // accessors, attrs ENTRY_DEST + ENTRY_TYPE (SDK/scripts/test_l2.py).
    // discard: ENTRY_FWD_POLICY = DROP (I).
    sai_status_t l2_add(const uint8_t mac[6], uint16_t vid, int hw, bool is_static, bool discard) override
    {
        std::lock_guard<std::recursive_mutex> g(mu_);
        if (!vlans_.count(vid)) return SAI_STATUS_INVALID_PARAMETER;
        if (is_lag_dest(hw)) return not_impl("FDB entry on a LAG");
        auto sp = sysport_.find(hw);
        if (sp == sysport_.end()) return SAI_STATUS_INVALID_PORT_NUMBER;
        ifcs_l2_entry_key_t k;
        l2_key(k, mac, vid);
        ifcs_attr_t a[3] = {attr_handle(IFCS_L2_ENTRY_ATTR_ENTRY_DEST, sp->second),
                            attr_u32(IFCS_L2_ENTRY_ATTR_ENTRY_TYPE,
                                     is_static ? IFCS_L2_ENTRY_TYPE_STATIC : IFCS_L2_ENTRY_TYPE_DYNAMIC),
                            attr_fwd(IFCS_L2_ENTRY_ATTR_ENTRY_FWD_POLICY, IFCS_FWD_ACTION_DROP)};
        return call(L_.ifcs_l2_entry_create(NODE, &k, discard ? 3 : 2, a), "l2_entry_create");
    }
    sai_status_t l2_delete(const uint8_t mac[6], uint16_t vid) override
    {
        std::lock_guard<std::recursive_mutex> g(mu_);
        ifcs_l2_entry_key_t k;
        l2_key(k, mac, vid);
        return call(L_.ifcs_l2_entry_delete(NODE, &k), "l2_entry_delete");
    }
    // Walk the table (ifcs_l2_entry_get_all, as SDK/ifcs_cmds/l2_entry.py),
    // read each entry's type and destination, delete the matching ones one by
    // one. No bulk delete-by-filter: we only remove what we checked.
    sai_status_t l2_flush(int hw, int vid, bool statics, bool dynamics) override
    {
        std::lock_guard<std::recursive_mutex> g(mu_);
        if (hw >= 0 && is_lag_dest(hw)) return not_impl("FDB flush on a LAG");
        std::vector<ifcs_l2_entry_key_t> keys;
        uint32_t n = 0;
        sai_status_t st = call(L_.ifcs_l2_entry_get_all(NODE, nullptr, 0, nullptr, &IfcsBackend::on_l2_entry, &keys, &n),
                               "l2_entry_get_all");
        if (st != SAI_STATUS_SUCCESS) return st;
        ifcs_handle_t want_sp = 0;
        if (hw >= 0) {
            auto sp = sysport_.find(hw);
            if (sp == sysport_.end()) return SAI_STATUS_INVALID_PORT_NUMBER;
            want_sp = sp->second;
        }
        for (auto& k : keys) {
            if (k.key_type != IFCS_L2_ENTRY_KEY_TYPE_MAC_L2VNI) continue;
            if (vid >= 0 && k.key.mac_l2vni.l2vni != IFCS_HANDLE_L2VNI((uint32_t)vid)) continue;
            ifcs_attr_t a[2] = {attr_of(IFCS_L2_ENTRY_ATTR_ENTRY_TYPE), attr_of(IFCS_L2_ENTRY_ATTR_ENTRY_DEST)};
            uint32_t got = 0;
            if (IFCS_STATUS_REASON(L_.ifcs_l2_entry_attr_get(NODE, &k, 2, a, &got)) != IFCS_SUCCESS) continue;
            bool is_static = a[0].value.u32 == IFCS_L2_ENTRY_TYPE_STATIC;
            if (is_static ? !statics : !dynamics) continue;
            if (hw >= 0 && a[1].value.handle != want_sp) continue;
            call(L_.ifcs_l2_entry_delete(NODE, &k), "l2_entry_delete (flush)");
        }
        return SAI_STATUS_SUCCESS;
    }
    // IFCS_NODE_ATTR_L2_ENTRY_AGING_INTERVAL (u32, read back as u32 by the
    // vendor's utils/ifcs_utils.py). Units taken as seconds (I).
    sai_status_t l2_age_set(uint32_t seconds) override
    {
        std::lock_guard<std::recursive_mutex> g(mu_);
        ifcs_attr_t a = attr_u32(IFCS_NODE_ATTR_L2_ENTRY_AGING_INTERVAL, seconds);
        return call(L_.ifcs_node_attr_set(NODE, 1, &a), "node l2_entry_aging_interval");
    }

    // ------------------------------------------------------------- netdevs
    // create (default): ifcs_netdev_create with INTF_NAME, DEVPORTNUM, MACADDR,
    // VLAN_TAG; ipd then makes the Linux netdev, RX is demultiplexed by source
    // sysport, TX goes straight to that sysport (PIPELINE_BYPASS).
    // adopt: the init yaml has netdev auto_create (vendor -k mode); the SDK's
    // inno_0_<sysport> netdev is renamed and given the MAC, as knet.py does.
    // Carrier: libifcs drives the netdev link state from its link scan (seen
    // on the vendor stack, I for netdevs we create); there is no IFCS call
    // for it, so netif_carrier_set only records the wish.
    sai_status_t netif_create(const std::string& ifname, int hw, uint16_t, const uint8_t mac[6], bool keep_tag,
                              int& netif_id, int& filter_id) override
    {
        std::lock_guard<std::recursive_mutex> g(mu_);
        if (!front(hw)) return SAI_STATUS_INVALID_PORT_NUMBER;
        if (ifname.empty() || ifname.size() > 15) return SAI_STATUS_INVALID_PARAMETER;
        for (auto& n : netifs_)
            if (n.second.name == ifname) return SAI_STATUS_ITEM_ALREADY_EXISTS;
        Netif n;
        n.name = ifname;
        n.hw = hw;
        n.keep_tag = keep_tag;
        if (netdev_adopt_) {
            n.adopted = true;
            n.orig = "inno_0_" + pad4(IFCS_HANDLE_VALUE(sysport_.at(hw)));
            sai_status_t st = rename_netdev(n.orig, ifname, mac);
            if (st != SAI_STATUS_SUCCESS) return st;
            if (keep_tag) OB_WARN(SAI_API_HOSTIF, "%s: adopted netdev keeps the SDK's VLAN tag action", ifname.c_str());
        } else {
            std::string nm = ifname;
            ifcs_attr_t a[4] = {attr_of(IFCS_NETDEV_ATTR_INTF_NAME), attr_of(IFCS_NETDEV_ATTR_DEVPORTNUM),
                                attr_mac(IFCS_NETDEV_ATTR_MACADDR, mac),
                                attr_u32(IFCS_NETDEV_ATTR_VLAN_TAG, tag_action(keep_tag))};
            a[0].value.string.count = (uint32_t)nm.size();
            a[0].value.string.arr = reinterpret_cast<uint8_t*>(&nm[0]);
            a[1].value.devport = (ifcs_devport_t)hw;
            ifcs_handle_t h = IFCS_NULL_HANDLE;
            sai_status_t st = call(L_.ifcs_netdev_create(NODE, &h, 4, a), "netdev_create");
            if (st != SAI_STATUS_SUCCESS) return st;
            n.handle = h;
        }
        netif_id = next_netif_++;
        filter_id = netif_id;
        netifs_[netif_id] = n;
        OB_NOTICE(SAI_API_HOSTIF, "netdev %s on devport %d (%s)", ifname.c_str(), hw, n.adopted ? "adopted" : "created");
        return SAI_STATUS_SUCCESS;
    }
    sai_status_t netif_destroy(int netif_id, int) override
    {
        std::lock_guard<std::recursive_mutex> g(mu_);
        auto it = netifs_.find(netif_id);
        if (it == netifs_.end()) return SAI_STATUS_ITEM_NOT_FOUND;
        sai_status_t st = netif_teardown(it->second);
        netifs_.erase(it);
        return st;
    }
    sai_status_t netif_carrier_set(const std::string& ifname, bool up) override
    {
        std::lock_guard<std::recursive_mutex> g(mu_);
        for (auto& n : netifs_)
            if (n.second.name == ifname) {
                n.second.carrier_wish = up;
                return SAI_STATUS_SUCCESS;
            }
        return SAI_STATUS_ITEM_NOT_FOUND;
    }
    sai_status_t netif_set_keep_tag(int netif_id, int, bool keep_tag, int& filter_id) override
    {
        std::lock_guard<std::recursive_mutex> g(mu_);
        auto it = netifs_.find(netif_id);
        if (it == netifs_.end()) return SAI_STATUS_ITEM_NOT_FOUND;
        filter_id = netif_id;
        if (it->second.keep_tag == keep_tag) return SAI_STATUS_SUCCESS;
        if (it->second.adopted) return not_impl("VLAN tag action of an adopted netdev");
        ifcs_attr_t a = attr_u32(IFCS_NETDEV_ATTR_VLAN_TAG, tag_action(keep_tag));
        sai_status_t st = call(L_.ifcs_netdev_attr_set(NODE, it->second.handle, 1, &a), "netdev vlan_tag");
        if (st == SAI_STATUS_SUCCESS) it->second.keep_tag = keep_tag;
        return st;
    }

    // IFCS hostif traps are drop/miss reasons, not protocol matches; protocol
    // traps would need ACLs (not mapped). Frames reach the CPU through the
    // flood groups (CPU in every VLAN) and through TRAP routes (l3_egress_cpu).
    sai_status_t trap_set(TrapKind kind, bool) override
    {
        static const char* names[] = {"arp-request", "arp-response", "dhcp", "dhcpv6", "igmp", "mld", "nd",
                                      "lldp", "lacp", "stp", "other"};
        std::string w = std::string("trap ") + names[(int)kind];
        log_once(w, "%s: no IFCS control; left as the SDK has it (NOT_SUPPORTED)", w.c_str());
        return SAI_STATUS_NOT_SUPPORTED;
    }

    // ------------------------------------------------------------------ L3
    // VRF n = IFCS_HANDLE_L3VNI(n + 1), created on first use with no
    // attributes (SDK/scripts/l3snake_cel.py creates its L3VNIs that way).
    // An L3 interface sits on the frontend's VLAN: intf TYPE = SVI on the
    // L2VNI (I: the vendor sample uses L3PORT on a sysport), FORWARDING_INSTANCE
    // = the VRF's L3VNI, TRANSPORT_MAC, IPv4/IPv6 unicast on, MTU.
    sai_status_t l3_intf_create(const uint8_t mac[6], uint16_t vid, uint32_t mtu, int vrf, int& intf_id) override
    {
        std::lock_guard<std::recursive_mutex> g(mu_);
        if (!vlans_.count(vid)) return SAI_STATUS_INVALID_PARAMETER;
        ifcs_handle_t vni = 0;
        sai_status_t st = l3vni(vrf, vni);
        if (st != SAI_STATUS_SUCCESS) return st;
        ifcs_attr_t a[7] = {attr_u32(IFCS_INTF_ATTR_TYPE, IFCS_INTF_TYPE_SVI),
                            attr_handle(IFCS_INTF_ATTR_FORWARDING_INSTANCE, vni),
                            attr_handle(IFCS_INTF_ATTR_TRANSPORT_INSTANCE, vlans_.at(vid).l2vni),
                            attr_mac(IFCS_INTF_ATTR_TRANSPORT_MAC, mac),
                            attr_data(IFCS_INTF_ATTR_IPV4_UNICAST_ENABLE, true),
                            attr_data(IFCS_INTF_ATTR_IPV6_UNICAST_ENABLE, true),
                            attr_u32(IFCS_INTF_ATTR_MTU, mtu)};
        ifcs_handle_t h = IFCS_NULL_HANDLE;
        st = call(L_.ifcs_intf_create(NODE, &h, 7, a), "intf_create");
        if (st != SAI_STATUS_SUCCESS) return st;
        Intf i;
        i.handle = h;
        std::memcpy(i.mac, mac, 6);
        i.vid = vid;
        i.mtu = mtu;
        i.vrf = vrf;
        intf_id = next_intf_++;
        intfs_[intf_id] = i;
        return SAI_STATUS_SUCCESS;
    }
    sai_status_t l3_intf_update(int intf_id, const uint8_t mac[6], uint16_t vid, uint32_t mtu, int vrf) override
    {
        std::lock_guard<std::recursive_mutex> g(mu_);
        auto it = intfs_.find(intf_id);
        if (it == intfs_.end()) return SAI_STATUS_ITEM_NOT_FOUND;
        Intf& i = it->second;
        if (vid != i.vid || vrf != i.vrf) return not_impl("moving an L3 interface to another VLAN/VRF");
        if (std::memcmp(mac, i.mac, 6) != 0) {
            ifcs_attr_t a = attr_mac(IFCS_INTF_ATTR_TRANSPORT_MAC, mac);
            sai_status_t st = call(L_.ifcs_intf_attr_set(NODE, i.handle, 1, &a), "intf transport_mac");
            if (st != SAI_STATUS_SUCCESS) return st;
            std::memcpy(i.mac, mac, 6);
        }
        if (mtu != i.mtu) {
            ifcs_attr_t a = attr_u32(IFCS_INTF_ATTR_MTU, mtu);
            sai_status_t st = call(L_.ifcs_intf_attr_set(NODE, i.handle, 1, &a), "intf mtu");
            if (st != SAI_STATUS_SUCCESS) return st;
            i.mtu = mtu;
        }
        return SAI_STATUS_SUCCESS;
    }
    sai_status_t l3_intf_destroy(int intf_id) override
    {
        std::lock_guard<std::recursive_mutex> g(mu_);
        auto it = intfs_.find(intf_id);
        if (it == intfs_.end()) return SAI_STATUS_ITEM_NOT_FOUND;
        sai_status_t st = call(L_.ifcs_intf_delete(NODE, it->second.handle), "intf_delete");
        if (st == SAI_STATUS_SUCCESS) intfs_.erase(it);
        return st;
    }
    // IFCS has no my-station table in the calls we use: an interface
    // terminates its own TRANSPORT_MAC (I). Stations are bookkeeping only.
    sai_status_t l3_station_add(const uint8_t mac[6], int& station_id) override
    {
        std::lock_guard<std::recursive_mutex> g(mu_);
        station_id = next_station_++;
        log_once("station", "my-station %s: no IFCS object (interfaces terminate their own MAC)", mac_str(mac).c_str());
        return SAI_STATUS_SUCCESS;
    }
    sai_status_t l3_station_delete(int) override { return SAI_STATUS_SUCCESS; }

    // Egress object = IFCS nexthop: INTF, DEST_MAC, LOCAL_DESTINATION = the
    // port's sysport (l3snake_cel.py). Shared CPU / drop egress: no nexthop,
    // they turn into route policies (see route_attrs).
    sai_status_t l3_egress_create(int intf_id, const uint8_t dmac[6], int hw, uint16_t vid, int& egress_id) override
    {
        std::lock_guard<std::recursive_mutex> g(mu_);
        auto in = intfs_.find(intf_id);
        if (in == intfs_.end()) return SAI_STATUS_INVALID_PARAMETER;
        if (is_lag_dest(hw)) return not_impl("next hop on a LAG");
        auto sp = sysport_.find(hw);
        if (sp == sysport_.end()) return SAI_STATUS_INVALID_PORT_NUMBER;
        ifcs_attr_t a[3] = {attr_handle(IFCS_NEXTHOP_ATTR_INTF, in->second.handle), attr_mac(IFCS_NEXTHOP_ATTR_DEST_MAC, dmac),
                            attr_handle(IFCS_NEXTHOP_ATTR_LOCAL_DESTINATION, sp->second)};
        ifcs_handle_t h = IFCS_NULL_HANDLE;
        sai_status_t st = call(L_.ifcs_nexthop_create(NODE, &h, 3, a), "nexthop_create");
        if (st != SAI_STATUS_SUCCESS) return st;
        Egress e;
        e.kind = Egress::Nh;
        e.handle = h;
        e.intf = intf_id;
        std::memcpy(e.dmac, dmac, 6);
        e.hw = hw;
        e.vid = vid;
        egress_id = next_egress_++;
        egress_[egress_id] = e;
        return SAI_STATUS_SUCCESS;
    }
    // In-place nexthop_attr_set of what changed (I), so routes keep their handle.
    sai_status_t l3_egress_update(int egress_id, int intf_id, const uint8_t dmac[6], int hw, uint16_t vid) override
    {
        std::lock_guard<std::recursive_mutex> g(mu_);
        auto it = egress_.find(egress_id);
        if (it == egress_.end() || it->second.kind != Egress::Nh) return SAI_STATUS_INVALID_PARAMETER;
        Egress& e = it->second;
        auto in = intfs_.find(intf_id);
        if (in == intfs_.end()) return SAI_STATUS_INVALID_PARAMETER;
        if (is_lag_dest(hw)) return not_impl("next hop on a LAG");
        auto sp = sysport_.find(hw);
        if (sp == sysport_.end()) return SAI_STATUS_INVALID_PORT_NUMBER;
        if (intf_id != e.intf) {
            ifcs_attr_t a = attr_handle(IFCS_NEXTHOP_ATTR_INTF, in->second.handle);
            sai_status_t st = call(L_.ifcs_nexthop_attr_set(NODE, e.handle, 1, &a), "nexthop intf");
            if (st != SAI_STATUS_SUCCESS) return st;
            e.intf = intf_id;
        }
        if (std::memcmp(dmac, e.dmac, 6) != 0) {
            ifcs_attr_t a = attr_mac(IFCS_NEXTHOP_ATTR_DEST_MAC, dmac);
            sai_status_t st = call(L_.ifcs_nexthop_attr_set(NODE, e.handle, 1, &a), "nexthop dest_mac");
            if (st != SAI_STATUS_SUCCESS) return st;
            std::memcpy(e.dmac, dmac, 6);
        }
        if (hw != e.hw) {
            ifcs_attr_t a = attr_handle(IFCS_NEXTHOP_ATTR_LOCAL_DESTINATION, sp->second);
            sai_status_t st = call(L_.ifcs_nexthop_attr_set(NODE, e.handle, 1, &a), "nexthop local_destination");
            if (st != SAI_STATUS_SUCCESS) return st;
            e.hw = hw;
        }
        e.vid = vid;
        return SAI_STATUS_SUCCESS;
    }
    sai_status_t l3_egress_destroy(int egress_id) override
    {
        std::lock_guard<std::recursive_mutex> g(mu_);
        auto it = egress_.find(egress_id);
        if (it == egress_.end()) return SAI_STATUS_ITEM_NOT_FOUND;
        if (it->second.kind != Egress::Nh) return SAI_STATUS_INVALID_PARAMETER;
        sai_status_t st = call(L_.ifcs_nexthop_delete(NODE, it->second.handle), "nexthop_delete");
        if (st == SAI_STATUS_SUCCESS) egress_.erase(it);
        return st;
    }
    sai_status_t l3_egress_cpu(int& egress_id) override
    {
        std::lock_guard<std::recursive_mutex> g(mu_);
        if (cpu_egress_ < 0) {
            cpu_egress_ = next_egress_++;
            Egress e;
            e.kind = Egress::Cpu;
            egress_[cpu_egress_] = e;
        }
        egress_id = cpu_egress_;
        return SAI_STATUS_SUCCESS;
    }
    sai_status_t l3_egress_drop(int& egress_id) override
    {
        std::lock_guard<std::recursive_mutex> g(mu_);
        if (drop_egress_ < 0) {
            drop_egress_ = next_egress_++;
            Egress e;
            e.kind = Egress::Drop;
            egress_[drop_egress_] = e;
        }
        egress_id = drop_egress_;
        return SAI_STATUS_SUCCESS;
    }

    // Host entries are /32 (/128) route entries: IFCS picks the host table
    // itself (route_entry has a HOST_RESOURCE_TYPE attribute; I).
    sai_status_t l3_host_add(int vrf, const IpAddr& ip, int egress_id) override
    {
        std::lock_guard<std::recursive_mutex> g(mu_);
        HostKey k{vrf, ip};
        if (hosts_.count(k)) return SAI_STATUS_ITEM_ALREADY_EXISTS;
        IpPrefix p = host_prefix(ip);
        sai_status_t st = route_program(vrf, p, egress_id, false, nullptr);
        if (st == SAI_STATUS_SUCCESS) hosts_[k] = egress_id;
        return st;
    }
    sai_status_t l3_host_delete(int vrf, const IpAddr& ip) override
    {
        std::lock_guard<std::recursive_mutex> g(mu_);
        auto it = hosts_.find(HostKey{vrf, ip});
        if (it == hosts_.end()) return SAI_STATUS_ITEM_NOT_FOUND;
        sai_status_t st = route_remove(vrf, host_prefix(ip));
        if (st == SAI_STATUS_SUCCESS) hosts_.erase(it);
        return st;
    }
    sai_status_t l3_route_add(int vrf, const IpPrefix& p, int target, bool ecmp, bool replace) override
    {
        std::lock_guard<std::recursive_mutex> g(mu_);
        RouteKey k{vrf, p};
        auto it = routes_.find(k);
        if (it != routes_.end() && !replace) return SAI_STATUS_ITEM_ALREADY_EXISTS;
        if (it == routes_.end() && replace) return SAI_STATUS_ITEM_NOT_FOUND;
        Route r{target, ecmp};
        sai_status_t st = route_program(vrf, p, target, ecmp, it == routes_.end() ? nullptr : &it->second);
        if (st == SAI_STATUS_SUCCESS) routes_[k] = r;
        return st;
    }
    sai_status_t l3_route_delete(int vrf, const IpPrefix& p) override
    {
        std::lock_guard<std::recursive_mutex> g(mu_);
        auto it = routes_.find(RouteKey{vrf, p});
        if (it == routes_.end()) return SAI_STATUS_ITEM_NOT_FOUND;
        sai_status_t st = route_remove(vrf, p);
        if (st == SAI_STATUS_SUCCESS) routes_.erase(it);
        return st;
    }

    // ECMP group: ifcs_ecmp_create without attributes (as cmds/ecmp.py with no
    // options), then ifcs_ecmp_member_add per nexthop. A repeated egress
    // object becomes IFCS_ECMP_MEMBER_ATTR_WEIGHT (u32, I).
    sai_status_t l3_ecmp_create(const std::vector<int>& egress, int& ecmp_id) override
    {
        std::lock_guard<std::recursive_mutex> g(mu_);
        std::map<ifcs_handle_t, uint32_t> want;
        sai_status_t st = ecmp_members(egress, want);
        if (st != SAI_STATUS_SUCCESS) return st;
        ifcs_handle_t h = IFCS_NULL_HANDLE;
        ifcs_attr_t none = attr_of(0);
        st = call(L_.ifcs_ecmp_create(NODE, &h, 0, &none), "ecmp_create");
        if (st != SAI_STATUS_SUCCESS) return st;
        Ecmp e;
        e.handle = h;
        st = ecmp_apply(e, want);
        if (st != SAI_STATUS_SUCCESS) {
            ecmp_apply(e, {});
            L_.ifcs_ecmp_delete(NODE, h);
            return st;
        }
        ecmp_id = next_ecmp_++;
        ecmps_[ecmp_id] = e;
        return SAI_STATUS_SUCCESS;
    }
    sai_status_t l3_ecmp_update(int ecmp_id, const std::vector<int>& egress) override
    {
        std::lock_guard<std::recursive_mutex> g(mu_);
        auto it = ecmps_.find(ecmp_id);
        if (it == ecmps_.end()) return SAI_STATUS_ITEM_NOT_FOUND;
        std::map<ifcs_handle_t, uint32_t> want;
        sai_status_t st = ecmp_members(egress, want);
        if (st != SAI_STATUS_SUCCESS) return st;
        return ecmp_apply(it->second, want);
    }
    sai_status_t l3_ecmp_destroy(int ecmp_id) override
    {
        std::lock_guard<std::recursive_mutex> g(mu_);
        auto it = ecmps_.find(ecmp_id);
        if (it == ecmps_.end()) return SAI_STATUS_ITEM_NOT_FOUND;
        ecmp_apply(it->second, {});
        sai_status_t st = call(L_.ifcs_ecmp_delete(NODE, it->second.handle), "ecmp_delete");
        if (st == SAI_STATUS_SUCCESS) ecmps_.erase(it);
        return st;
    }

    // ------------------------------------------- field processor / policers
    // ACLs are not mapped (IFCS acl tables / match profiles / actions are a
    // separate model). Policers are kept as backend objects only: nothing can
    // reference them in hardware while ACL entries are NOT_IMPLEMENTED, so the
    // frontend's CoPP falls back to "traps run unpoliced" (it logs that).
    sai_status_t fp_group_create(const std::vector<FpQual>&, int, int& gid) override { gid = -1; return not_impl("ACL / field processor"); }
    sai_status_t fp_group_destroy(int) override { return not_impl("ACL / field processor"); }
    sai_status_t fp_entry_create(int, const FpRule&, int& eid) override { eid = -1; return not_impl("ACL / field processor"); }
    sai_status_t fp_entry_destroy(int) override { return not_impl("ACL / field processor"); }
    sai_status_t fp_entry_counter(int, uint64_t&, uint64_t&) override { return not_impl("ACL / field processor"); }
    sai_status_t policer_create(const PolicerCfg& cfg, int& pid) override
    {
        std::lock_guard<std::recursive_mutex> g(mu_);
        pid = next_policer_++;
        policers_[pid] = cfg;
        log_once("policer", "policers are kept in the backend only (no IFCS meter, no ACL to attach it to)");
        return SAI_STATUS_SUCCESS;
    }
    sai_status_t policer_set(int pid, const PolicerCfg& cfg) override
    {
        std::lock_guard<std::recursive_mutex> g(mu_);
        auto it = policers_.find(pid);
        if (it == policers_.end()) return SAI_STATUS_ITEM_NOT_FOUND;
        it->second = cfg;
        return SAI_STATUS_SUCCESS;
    }
    sai_status_t policer_destroy(int pid) override
    {
        std::lock_guard<std::recursive_mutex> g(mu_);
        return policers_.erase(pid) ? SAI_STATUS_SUCCESS : SAI_STATUS_ITEM_NOT_FOUND;
    }

private:
    struct Vlan { ifcs_handle_t l2vni = 0, mdg = 0; std::map<int, bool> members; };
    struct Netif {
        std::string name, orig;
        int hw = -1;
        ifcs_handle_t handle = 0;
        bool adopted = false, keep_tag = false, carrier_wish = false;
    };
    struct Intf { ifcs_handle_t handle = 0; uint8_t mac[6] = {0}; uint16_t vid = 0; uint32_t mtu = 0; int vrf = 0; };
    struct Egress {
        enum Kind { Nh, Cpu, Drop } kind = Nh;
        ifcs_handle_t handle = 0;
        int intf = -1, hw = -1;
        uint8_t dmac[6] = {0};
        uint16_t vid = 0;
    };
    struct Ecmp { ifcs_handle_t handle = 0; std::map<ifcs_handle_t, uint32_t> members; };
    struct Route { int target = -1; bool ecmp = false; };
    struct StatCache { bool valid = false; std::chrono::steady_clock::time_point when; uint64_t v[IFCS_DEVPORT_STATS_ID_ALL] = {0}; };
    using HostKey = std::pair<int, IpAddr>;
    using RouteKey = std::pair<int, IpPrefix>;

    // ------------------------------------------------------------ helpers
    std::string key(const char* k, const char* def = "") const
    {
        auto it = cfg_.profile.find(k);
        if (it != cfg_.profile.end()) return it->second;
        return profile_get(k, def);   // the frontend's fallback key list lacks ours
    }
    const char* reason(int r) const
    {
        static const char* names[] = {"SUCCESS", "UNINIT", "BUSY", "CONFIG", "MEMORY", "PARAM", "EXIST", "MULTI_EXIST",
                                      "ENTRY_REPLACED", "LENGTH_MISMATCH", "NOTFOUND", "INVAL", "UNSUPPORTED",
                                      "RESOURCE_FULL", "TIMEOUT", "DEVICE_ACCESS_ERROR", "DRIVER_ERROR"};
        return r >= 0 && r <= 16 ? names[r] : "?";
    }
    sai_status_t fail(int r, const char* what)
    {
        OB_LOG(SAI_API_UNSPECIFIED, r == IFCS_UNSUPPORTED ? SAI_LOG_LEVEL_INFO : SAI_LOG_LEVEL_ERROR, "ifcs %s: %s",
               what, reason(r));
        return map_reason(r);
    }
    sai_status_t call(ifcs_status_t rc, const char* what)
    {
        int r = IFCS_STATUS_REASON(rc);
        return r == IFCS_SUCCESS ? SAI_STATUS_SUCCESS : fail(r, what);
    }
    void log_once(const std::string& k, const char* fmt, ...) __attribute__((format(printf, 3, 4)))
    {
        std::lock_guard<std::recursive_mutex> g(mu_);
        if (!logged_.insert(k).second) return;
        char buf[512];
        va_list ap;
        va_start(ap, fmt);
        std::vsnprintf(buf, sizeof(buf), fmt, ap);
        va_end(ap);
        OB_NOTICE(SAI_API_SWITCH, "%s", buf);
    }
    sai_status_t not_impl(const char* what)
    {
        std::lock_guard<std::recursive_mutex> g(mu_);
        if (logged_.insert(std::string("ni:") + what).second)
            OB_WARN(SAI_API_SWITCH, "teralynx backend: %s is NOT_IMPLEMENTED (see sai-teralynx README)", what);
        return SAI_STATUS_NOT_IMPLEMENTED;
    }
    uint32_t usage_free(ifcs_status_t rc, const ifcs_usage_t& u, uint32_t fallback, const char* what)
    {
        if (IFCS_STATUS_REASON(rc) == IFCS_SUCCESS) return u.max > u.current ? u.max - u.current : 0;
        log_once(std::string("usage:") + what, "%s usage unreadable; reporting %u free", what, fallback);
        return fallback;
    }
    const tl::PortSpec* front(int hw) const
    {
        for (auto& s : specs_)
            if ((int)s.devport == hw) return &s;
        return nullptr;
    }
    sai_status_t dp_set(int hw, ifcs_attr_t a, const char* what)
    {
        return call(L_.ifcs_devport_attr_set(NODE, (ifcs_devport_t)hw, 1, &a), what);
    }
    sai_status_t dp_get_u32(int hw, uint32_t id, uint32_t& v)
    {
        ifcs_attr_t a = attr_of(id);
        uint32_t n = 0;
        int r = IFCS_STATUS_REASON(L_.ifcs_devport_attr_get(NODE, (ifcs_devport_t)hw, 1, &a, &n));
        if (r != IFCS_SUCCESS) return fail(r, "devport attr get");
        v = a.value.u32;
        return SAI_STATUS_SUCCESS;
    }
    sai_status_t sp_set(int hw, ifcs_attr_t a, const char* what)
    {
        auto it = sysport_.find(hw);
        if (it == sysport_.end()) return SAI_STATUS_INVALID_PORT_NUMBER;
        return call(L_.ifcs_sysport_attr_set(NODE, it->second, 1, &a), what);
    }
    void all_ports_down()
    {
        for (auto& s : specs_) {
            ifcs_attr_t a = attr_u32(IFCS_DEVPORT_ATTR_ADMIN_STATE, 0);
            L_.ifcs_devport_attr_set(NODE, s.devport, 1, &a);
        }
    }
    void deinit()
    {
        if (!sdk_up_) return;
        int r = IFCS_STATUS_REASON(L_.ifcs_deinit(IFCS_SHUTDOWN_TYPE_COLD));
        if (r != IFCS_SUCCESS && r != IFCS_UNINIT) OB_ERR(SAI_API_SWITCH, "ifcs_deinit: %s", reason(r));
        sdk_up_ = false;
    }
    static std::string pad4(uint32_t v)
    {
        char b[16];
        std::snprintf(b, sizeof(b), "%04u", v);
        return b;
    }
    static uint32_t tag_action(bool keep_tag)
    {
        return keep_tag ? IFCS_NETDEV_VLAN_TAG_ACTION_VLAN_TAG_ORIGINAL : IFCS_NETDEV_VLAN_TAG_ACTION_VLAN_TAG_STRIP;
    }

    // After node enable: sysport handles, ports down, init commands, event
    // callbacks, the L2 base (STP instance, one mc_dest per port, VLAN 1).
    sai_status_t post_enable(const tl::InitCmds& ic)
    {
        std::lock_guard<std::recursive_mutex> g(mu_);
        sysport_.clear();
        dev_of_sysport_.clear();
        std::vector<uint32_t> dps{(uint32_t)CPU_DEVPORT};
        for (auto& s : specs_) dps.push_back(s.devport);
        for (uint32_t dp : dps) {
            ifcs_attr_t a = attr_of(IFCS_DEVPORT_ATTR_SYSPORT);
            uint32_t n = 0;
            int r = IFCS_STATUS_REASON(L_.ifcs_devport_attr_get(NODE, dp, 1, &a, &n));
            if (r != IFCS_SUCCESS) {
                OB_ERR(SAI_API_SWITCH, "devport %u: no sysport (%s); port map and init config disagree?", dp, reason(r));
                return map_reason(r);
            }
            sysport_[(int)dp] = a.value.handle;
            dev_of_sysport_[a.value.handle] = (int)dp;
        }
        all_ports_down();
        unsigned ok = 0, bad = 0;
        for (auto& c : ic.cmds) {
            if (apply_cmd(c)) ok++;
            else bad++;
        }
        for (auto& s : ic.skipped)
            OB_WARN(SAI_API_SWITCH, "init commands: skipped %u '%s' line(s) (only 'ifcs set devport' serdes/eq "
                    "attributes are applied)", s.second, s.first.c_str());
        if (bad) OB_ERR(SAI_API_SWITCH, "init commands: %u of %zu failed (see above)", bad, ic.cmds.size());
        OB_NOTICE(SAI_API_SWITCH, "init commands: %u applied", ok);

        sai_status_t st = call(L_.ifcs_linkscan_event_register_notify(NODE, this, &IfcsBackend::on_link), "linkscan register");
        if (st != SAI_STATUS_SUCCESS) return st;
        st = call(L_.ifcs_l2_event_register_notify(NODE, this, &IfcsBackend::on_l2), "l2 event register");
        if (st != SAI_STATUS_SUCCESS) return st;

        // L2 base, as SDK/cmds/knet.py builds it (without its per-port VLANs).
        ifcs_attr_t none = attr_of(0);
        stp_ = IFCS_NULL_HANDLE;
        st = call(L_.ifcs_stp_create(NODE, &stp_, 0, &none), "stp_create");
        if (st != SAI_STATUS_SUCCESS) return st;
        std::vector<ifcs_handle_t> sps;
        for (uint32_t dp : dps) sps.push_back(sysport_[(int)dp]);
        ifcs_attr_t fwd = attr_u32(IFCS_STP_PORT_ATTR_STP_STATE, IFCS_STP_PORT_STATE_FORWARDING);
        st = call(L_.ifcs_stp_port_add(NODE, stp_, (uint32_t)sps.size(), sps.data(), 1, &fwd), "stp_port_add");
        if (st != SAI_STATUS_SUCCESS) return st;
        mcdest_.clear();
        for (uint32_t dp : dps) {
            ifcs_attr_t a = attr_handle(IFCS_MC_DEST_ATTR_LOCAL_DESTINATION, sysport_[(int)dp]);
            ifcs_handle_t h = IFCS_NULL_HANDLE;
            st = call(L_.ifcs_mc_dest_create(NODE, &h, 1, &a), "mc_dest_create");
            if (st != SAI_STATUS_SUCCESS) return st;
            mcdest_[(int)dp] = h;
        }
        vlans_.clear();
        return vlan_make(1);
    }

    bool apply_cmd(const tl::DevportCmd& c)
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
        int r = IFCS_STATUS_REASON(L_.ifcs_devport_attr_set(NODE, c.devport, 1, &a));
        if (r != IFCS_SUCCESS) {
            OB_ERR(SAI_API_SWITCH, "%s: devport %u %s: %s", c.where.c_str(), c.devport, c.name.c_str(), reason(r));
            return false;
        }
        return true;
    }

    sai_status_t vlan_make(uint16_t vid)
    {
        ifcs_attr_t none = attr_of(0);
        Vlan v;
        v.mdg = IFCS_NULL_HANDLE;
        sai_status_t st = call(L_.ifcs_mdg_create(NODE, &v.mdg, 0, &none), "mdg_create");
        if (st != SAI_STATUS_SUCCESS) return st;
        v.l2vni = IFCS_HANDLE_L2VNI(vid);
        ifcs_attr_t a[2] = {attr_handle(IFCS_L2VNI_ATTR_STP_INSTANCE, stp_), attr_handle(IFCS_L2VNI_ATTR_FLOOD_GROUP, v.mdg)};
        ifcs_handle_t h = v.l2vni;
        int r = IFCS_STATUS_REASON(L_.ifcs_l2vni_create(NODE, &h, 2, a));
        if (r == IFCS_EXIST) {
            // An L2VNI the SDK made itself (VLAN 1?): take it over.
            OB_NOTICE(SAI_API_VLAN, "L2VNI %u exists already; setting its STP instance and flood group", vid);
            r = IFCS_STATUS_REASON(L_.ifcs_l2vni_attr_set(NODE, v.l2vni, 2, a));
        }
        if (r != IFCS_SUCCESS) {
            L_.ifcs_mdg_delete(NODE, v.mdg);
            return fail(r, "l2vni_create");
        }
        ifcs_handle_t cpu_sp = sysport_.at(CPU_DEVPORT), cpu_mcd = mcdest_.at(CPU_DEVPORT);
        r = IFCS_STATUS_REASON(L_.ifcs_l2vni_member_add(NODE, v.l2vni, 1, &cpu_sp, 0, &none));
        if (r != IFCS_SUCCESS && r != IFCS_EXIST) return fail(r, "l2vni_member_add(cpu)");
        st = call(L_.ifcs_mdg_mc_dest_add(NODE, v.mdg, 1, &cpu_mcd, 0, &none), "mdg_mc_dest_add(cpu)");
        if (st != SAI_STATUS_SUCCESS) return st;
        vlans_[vid] = v;
        return SAI_STATUS_SUCCESS;
    }
    void untagged_off(int hw)
    {
        ifcs_attr_t a = attr_data(IFCS_SYSPORT_ATTR_EGRESS_UNTAGGED_VLAN_ID_ENABLE, false);
        call(L_.ifcs_sysport_attr_set(NODE, sysport_.at(hw), 1, &a), "sysport egress untagged off");
        untagged_vid_.erase(hw);
    }

    // The key accessors in the vendor's order (scripts/test_l2.py): init,
    // key type, select the mac_l2vni member, then its MAC and L2VNI.
    void l2_key(ifcs_l2_entry_key_t& k, const uint8_t mac[6], uint16_t vid)
    {
        std::memset(&k, 0, sizeof(k));
        L_.ifcs_l2_entry_key_t_init(&k);
        L_.ifcs_l2_entry_key_t_key_type_set(&k, IFCS_L2_ENTRY_KEY_TYPE_MAC_L2VNI);
        L_.ifcs_l2_entry_key_t_mac_l2vni_set(&k, k.key.mac_l2vni);
        ifcs_mac_t m;
        std::memcpy(m, mac, 6);
        L_.ifcs_l2_entry_key_mac_l2vni_t_mac_addr_set(&k.key.mac_l2vni, &m);
        L_.ifcs_l2_entry_key_mac_l2vni_t_l2vni_set(&k.key.mac_l2vni, IFCS_HANDLE_L2VNI(vid));
    }
    // Route key as scripts/l3snake_cel.py builds it. IPv4 goes in as a
    // host-order value, IPv6 as 16 bytes.
    void route_key(ifcs_route_entry_key_t& k, ifcs_handle_t vni, const IpPrefix& p)
    {
        ifcs_ip_addr_t a, m;
        std::memset(&a, 0, sizeof(a));
        std::memset(&m, 0, sizeof(m));
        if (p.addr.v6) {
            std::memcpy(a.ipv6, p.addr.addr, 16);
            std::memcpy(m.ipv6, p.mask.addr, 16);
        } else {
            a.ipv4 = be32(p.addr.addr);
            m.ipv4 = be32(p.mask.addr);
        }
        ifcs_ip_prefix_t pf;
        std::memset(&pf, 0, sizeof(pf));
        L_.ifcs_ip_prefix_t_init(&pf);
        L_.ifcs_ip_prefix_t_addr_family_set(&pf, p.addr.v6 ? IFCS_IP_ADDR_FAMILY_IPV6 : IFCS_IP_ADDR_FAMILY_IPV4);
        L_.ifcs_ip_prefix_t_addr_set(&pf, &a);
        L_.ifcs_ip_prefix_t_mask_set(&pf, &m);
        std::memset(&k, 0, sizeof(k));
        L_.ifcs_route_entry_key_t_init(&k);
        L_.ifcs_route_entry_key_t_key_type_set(&k, IFCS_ROUTE_ENTRY_KEY_TYPE_IP_DEST_L3VNI);
        L_.ifcs_route_entry_key_t_ip_dest_l3vni_set(&k, k.key.ip_dest_l3vni);
        L_.ifcs_route_entry_key_ip_dest_l3vni_t_l3vni_set(&k.key.ip_dest_l3vni, vni);
        L_.ifcs_route_entry_key_ip_dest_l3vni_t_ip_dest_set(&k.key.ip_dest_l3vni, &pf);
    }
    static uint32_t be32(const uint8_t* b)
    {
        return ((uint32_t)b[0] << 24) | ((uint32_t)b[1] << 16) | ((uint32_t)b[2] << 8) | b[3];
    }
    static IpPrefix host_prefix(const IpAddr& ip)
    {
        IpPrefix p;
        p.addr = ip;
        p.mask.v6 = ip.v6;
        std::memset(p.mask.addr, 0xff, ip.v6 ? 16 : 4);
        return p;
    }
    sai_status_t l3vni(int vrf, ifcs_handle_t& h)
    {
        if (vrf < 0 || (uint32_t)vrf > VRF_MAX) return SAI_STATUS_INVALID_PARAMETER;
        auto it = l3vnis_.find(vrf);
        if (it != l3vnis_.end()) {
            h = it->second;
            return SAI_STATUS_SUCCESS;
        }
        ifcs_handle_t v = IFCS_HANDLE_L3VNI((uint32_t)vrf + 1);
        ifcs_attr_t none = attr_of(0);
        int r = IFCS_STATUS_REASON(L_.ifcs_l3vni_create(NODE, &v, 0, &none));
        if (r != IFCS_SUCCESS && r != IFCS_EXIST) return fail(r, "l3vni_create");
        l3vnis_[vrf] = v;
        h = v;
        return SAI_STATUS_SUCCESS;
    }
    // Route attributes per target kind:
    //   nexthop / ECMP: NEXTHOP + FWD_POLICY forward (l3snake_cel.py; an ECMP
    //     handle as NEXTHOP is I);
    //   drop: FWD_POLICY drop, no NEXTHOP (I);
    //   CPU (TRAP / ip2me): FWD_POLICY drop + CTC_POLICY copy-to-cpu with one
    //     shared hostif trap on CPU queue 0 (trap create as cmds/netdev_test.py;
    //     drop + copy as Marvell's SAI does for ip2me, from its strings; I).
    sai_status_t route_attrs(int target, bool ecmp, std::vector<ifcs_attr_t>& a, int& kind)
    {
        a.clear();
        if (ecmp) {
            auto it = ecmps_.find(target);
            if (it == ecmps_.end()) return SAI_STATUS_INVALID_PARAMETER;
            a.push_back(attr_handle(IFCS_ROUTE_ENTRY_ATTR_NEXTHOP, it->second.handle));
            a.push_back(attr_fwd(IFCS_ROUTE_ENTRY_ATTR_FWD_POLICY, IFCS_FWD_ACTION_FORWARD));
            kind = Egress::Nh;
            return SAI_STATUS_SUCCESS;
        }
        auto it = egress_.find(target);
        if (it == egress_.end()) return SAI_STATUS_INVALID_PARAMETER;
        kind = it->second.kind;
        if (kind == Egress::Nh) {
            a.push_back(attr_handle(IFCS_ROUTE_ENTRY_ATTR_NEXTHOP, it->second.handle));
            a.push_back(attr_fwd(IFCS_ROUTE_ENTRY_ATTR_FWD_POLICY, IFCS_FWD_ACTION_FORWARD));
        } else if (kind == Egress::Drop) {
            a.push_back(attr_fwd(IFCS_ROUTE_ENTRY_ATTR_FWD_POLICY, IFCS_FWD_ACTION_DROP));
        } else {
            if (trap_ == IFCS_NULL_HANDLE) {
                ifcs_attr_t q = attr_u32(IFCS_HOSTIF_TRAP_ATTR_QUEUE_NUM, 0);
                ifcs_handle_t h = IFCS_NULL_HANDLE;
                sai_status_t st = call(L_.ifcs_hostif_trap_create(NODE, &h, 1, &q), "hostif_trap_create");
                if (st != SAI_STATUS_SUCCESS) return st;
                trap_ = h;
            }
            a.push_back(attr_fwd(IFCS_ROUTE_ENTRY_ATTR_FWD_POLICY, IFCS_FWD_ACTION_DROP));
            a.push_back(attr_ctc(IFCS_ROUTE_ENTRY_ATTR_CTC_POLICY, true, trap_));
        }
        return SAI_STATUS_SUCCESS;
    }
    // Create, or (old != nullptr) replace: same kind (nexthop/ECMP to
    // nexthop/ECMP) is an attr_set of NEXTHOP + FWD_POLICY; a change of kind
    // deletes and re-creates the entry (no attribute "unset" in IFCS we know).
    sai_status_t route_program(int vrf, const IpPrefix& p, int target, bool ecmp, const Route* old)
    {
        ifcs_handle_t vni = 0;
        sai_status_t st = l3vni(vrf, vni);
        if (st != SAI_STATUS_SUCCESS) return st;
        std::vector<ifcs_attr_t> a;
        int kind = 0;
        st = route_attrs(target, ecmp, a, kind);
        if (st != SAI_STATUS_SUCCESS) return st;
        ifcs_route_entry_key_t k;
        route_key(k, vni, p);
        if (old) {
            int okind = old->ecmp ? (int)Egress::Nh : egress_.count(old->target) ? (int)egress_[old->target].kind : -1;
            if (okind == kind && kind == Egress::Nh)
                return call(L_.ifcs_route_entry_attr_set(NODE, &k, (uint32_t)a.size(), a.data()), "route_entry_attr_set");
            st = call(L_.ifcs_route_entry_delete(NODE, &k), "route_entry_delete (replace)");
            if (st != SAI_STATUS_SUCCESS) return st;
        }
        return call(L_.ifcs_route_entry_create(NODE, &k, (uint32_t)a.size(), a.data()), "route_entry_create");
    }
    sai_status_t route_remove(int vrf, const IpPrefix& p)
    {
        ifcs_handle_t vni = 0;
        sai_status_t st = l3vni(vrf, vni);
        if (st != SAI_STATUS_SUCCESS) return st;
        ifcs_route_entry_key_t k;
        route_key(k, vni, p);
        return call(L_.ifcs_route_entry_delete(NODE, &k), "route_entry_delete");
    }
    sai_status_t ecmp_members(const std::vector<int>& egress, std::map<ifcs_handle_t, uint32_t>& want)
    {
        if (egress.empty() || egress.size() > ECMP_PATHS) return SAI_STATUS_INVALID_PARAMETER;
        for (int e : egress) {
            auto it = egress_.find(e);
            if (it == egress_.end() || it->second.kind != Egress::Nh) return SAI_STATUS_INVALID_PARAMETER;
            want[it->second.handle]++;
        }
        return SAI_STATUS_SUCCESS;
    }
    sai_status_t ecmp_apply(Ecmp& e, const std::map<ifcs_handle_t, uint32_t>& want)
    {
        for (auto it = e.members.begin(); it != e.members.end();) {
            auto w = want.find(it->first);
            if (w != want.end() && w->second == it->second) { ++it; continue; }
            ifcs_handle_t h = it->first;
            sai_status_t st = call(L_.ifcs_ecmp_member_remove(NODE, e.handle, 1, &h), "ecmp_member_remove");
            if (st != SAI_STATUS_SUCCESS) return st;
            it = e.members.erase(it);
        }
        for (auto& w : want) {
            if (e.members.count(w.first)) continue;
            ifcs_handle_t h = w.first;
            ifcs_attr_t wt = attr_u32(IFCS_ECMP_MEMBER_ATTR_WEIGHT, w.second);
            sai_status_t st = call(L_.ifcs_ecmp_member_add(NODE, e.handle, 1, &h, w.second > 1 ? 1 : 0, &wt),
                                   "ecmp_member_add");
            if (st != SAI_STATUS_SUCCESS) return st;
            e.members[w.first] = w.second;
        }
        return SAI_STATUS_SUCCESS;
    }

    // ------------------------------------------------------------ netdevs
    sai_status_t netif_teardown(Netif& n)
    {
        if (n.adopted) return rename_netdev(n.name, n.orig, nullptr);
        return call(L_.ifcs_netdev_delete(NODE, n.handle), "netdev_delete");
    }
    sai_status_t rename_netdev(const std::string& from, const std::string& to, const uint8_t* mac)
    {
        int s = socket(AF_INET, SOCK_DGRAM | SOCK_CLOEXEC, 0);
        if (s < 0) return SAI_STATUS_FAILURE;
        struct ifreq ifr;
        std::memset(&ifr, 0, sizeof(ifr));
        std::snprintf(ifr.ifr_name, IFNAMSIZ, "%s", from.c_str());
        sai_status_t st = SAI_STATUS_SUCCESS;
        if (ioctl(s, SIOCGIFFLAGS, &ifr) != 0) {
            OB_ERR(SAI_API_HOSTIF, "netdev %s not found (%s); SAI_TERALYNX_NETDEV=adopt needs the init yaml's netdev "
                   "auto_create", from.c_str(), std::strerror(errno));
            close(s);
            return SAI_STATUS_ITEM_NOT_FOUND;
        }
        if (ifr.ifr_flags & IFF_UP) {
            ifr.ifr_flags &= ~IFF_UP;
            ioctl(s, SIOCSIFFLAGS, &ifr);
        }
        std::snprintf(ifr.ifr_newname, IFNAMSIZ, "%s", to.c_str());
        if (ioctl(s, SIOCSIFNAME, &ifr) != 0) {
            OB_ERR(SAI_API_HOSTIF, "rename %s -> %s: %s", from.c_str(), to.c_str(), std::strerror(errno));
            st = SAI_STATUS_FAILURE;
        } else if (mac) {
            std::memset(&ifr, 0, sizeof(ifr));
            std::snprintf(ifr.ifr_name, IFNAMSIZ, "%s", to.c_str());
            ifr.ifr_hwaddr.sa_family = ARPHRD_ETHER;
            std::memcpy(ifr.ifr_hwaddr.sa_data, mac, 6);
            if (ioctl(s, SIOCSIFHWADDR, &ifr) != 0)
                OB_WARN(SAI_API_HOSTIF, "%s: set MAC: %s", to.c_str(), std::strerror(errno));
        }
        close(s);
        return st;
    }

    // ---------------------------------------------------- diag shell (read-only)
    sai_status_t shell(const std::string& cmd, std::string& out)
    {
        std::istringstream is(cmd);
        std::vector<std::string> t;
        std::string w;
        while (is >> w) t.push_back(w);
        char b[256];
        if (t.empty() || t[0] == "help") {
            out = "commands: ports | devport <id> | temp | help (read-only)\n";
            return SAI_STATUS_SUCCESS;
        }
        if (!initialized_) {
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
        if (t[0] == "ports" || (t[0] == "devport" && t.size() == 2)) {
            std::lock_guard<std::recursive_mutex> g(mu_);
            for (auto& s : specs_) {
                if (t[0] == "devport" && std::to_string(s.devport) != t[1]) continue;
                uint32_t admin = 9, link = 9, speed = 0, fec = 9, an = 9;
                dp_get_u32(s.devport, IFCS_DEVPORT_ATTR_ADMIN_STATE, admin);
                dp_get_u32(s.devport, IFCS_DEVPORT_ATTR_LINK_STATUS, link);
                dp_get_u32(s.devport, IFCS_DEVPORT_ATTR_SPEED, speed);
                dp_get_u32(s.devport, IFCS_DEVPORT_ATTR_FEC_MODE, fec);
                dp_get_u32(s.devport, IFCS_DEVPORT_ATTR_AUTO_NEG, an);
                std::snprintf(b, sizeof(b), "devport %u: admin %u link %s speed %u fec %u autoneg %u sysport 0x%x\n",
                              s.devport, admin, link == 1 ? "up" : "down", speed_from_enum(speed), fec, an,
                              sysport_.count((int)s.devport) ? sysport_.at((int)s.devport) : 0);
                out += b;
            }
            if (out.empty()) { out = "no such devport\n"; return SAI_STATUS_INVALID_PARAMETER; }
            return SAI_STATUS_SUCCESS;
        }
        out = "not supported: " + cmd + "\n";
        return SAI_STATUS_NOT_SUPPORTED;
    }

    // ------------------------------------------------- SDK thread callbacks
    // Only translate and queue (the frontend's callbacks push to its queue).
    static void on_link(ifcs_node_id_t, void* user, ifcs_linkscan_event_info_t* info)
    {
        auto* b = static_cast<IfcsBackend*>(user);
        if (!b || !info || !b->link_) return;
        if (!b->front((int)info->devport_id)) return;
        b->link_((int)info->devport_id, info->port_state == IFCS_DEVPORT_LINK_STATUS_UP);
    }
    static void on_l2(ifcs_node_id_t, void* user, ifcs_l2_event_info_t* info)
    {
        auto* b = static_cast<IfcsBackend*>(user);
        if (!b || !info || !b->fdb_) return;
        if (info->l2_entry.key_type != IFCS_L2_ENTRY_KEY_TYPE_MAC_L2VNI) return;
        if (info->event_type == IFCS_L2_EVENT_TYPE_FLUSHED) return;   // our own flushes; the frontend knows
        FdbEvent e;
        e.learned = info->event_type != IFCS_L2_EVENT_TYPE_AGED;
        std::memcpy(e.mac, info->l2_entry.key.mac_l2vni.mac_addr, 6);
        ifcs_handle_t vni = info->l2_entry.key.mac_l2vni.l2vni;
        if ((vni >> IFCS_HANDLE_MODULE_SHIFT) != IFCS_MOD_L2VNI) return;
        e.vid = (uint16_t)IFCS_HANDLE_VALUE(vni);
        e.hw_port = -1;
        for (uint32_t i = 0; info->attr_p && i < info->attr_count; i++)
            if (info->attr_p[i].id == IFCS_L2_ENTRY_ATTR_ENTRY_DEST) {
                auto it = b->dev_of_sysport_.find(info->attr_p[i].value.handle);
                if (it != b->dev_of_sysport_.end()) e.hw_port = it->second;
            }
        if (e.learned && e.hw_port < 0) return;
        b->fdb_(e);
    }
    static void on_log(int level, void*, const char* msg)
    {
        if (!msg) return;
        sai_log_level_t l = level <= IFCS_LOG_LEVEL_ERROR ? SAI_LOG_LEVEL_ERROR
                          : level == IFCS_LOG_LEVEL_WARN ? SAI_LOG_LEVEL_WARN
                          : level == IFCS_LOG_LEVEL_INFO ? SAI_LOG_LEVEL_INFO : SAI_LOG_LEVEL_DEBUG;
        OB_LOG(SAI_API_SWITCH, l, "ifcs: %s", msg);
    }
    static void on_l2_entry(ifcs_node_id_t, ifcs_l2_entry_key_t* k, uint32_t, ifcs_attr_t*, void* user)
    {
        if (k && user) static_cast<std::vector<ifcs_l2_entry_key_t>*>(user)->push_back(*k);
    }

    BackendConfig cfg_;
    IfcsLib L_;
    std::recursive_mutex mu_;
    LinkCallback link_;
    FdbCallback fdb_;
    bool initialized_ = false, sdk_up_ = false, netdev_adopt_ = false, deinit_on_shutdown_ = true;
    std::string run_config_;
    std::vector<tl::PortSpec> specs_;                  // immutable after init (callbacks read it)
    std::map<int, ifcs_handle_t> sysport_, mcdest_;    // devport -> handle (immutable after init)
    std::map<ifcs_handle_t, int> dev_of_sysport_;
    ifcs_handle_t stp_ = 0, trap_ = 0;
    std::map<uint16_t, Vlan> vlans_;
    std::map<int, uint16_t> untagged_vid_;
    std::map<int, Netif> netifs_;
    std::map<int, StatCache> stats_;
    std::map<int, ifcs_handle_t> l3vnis_;
    std::map<int, Intf> intfs_;
    std::map<int, Egress> egress_;
    std::map<int, Ecmp> ecmps_;
    std::map<HostKey, int> hosts_;
    std::map<RouteKey, Route> routes_;
    std::map<int, PolicerCfg> policers_;
    std::set<std::string> logged_;
    bool limits_read_ = false;
    ChipLimits limits_;
    int next_netif_ = 1, next_intf_ = 1, next_station_ = 1, next_egress_ = 100001, next_ecmp_ = 200001, next_policer_ = 1;
    int cpu_egress_ = -1, drop_egress_ = -1;
};

Backend* make_openbcm_backend() { return new IfcsBackend(); }

} // namespace ob
