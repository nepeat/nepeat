// SPDX-License-Identifier: Apache-2.0
// RecordingBackend (recording_backend.h): one text line per backend call, for the differential
// oracle. The line grammar below is the reference; the Rust virtual backend's call log prints
// the same bytes for the same call (agent K; copy of this block: call-log-format.md in the
// migration scratchpad).
//
//   line    = call | event
//   call    = op "(" [ arg *( ", " arg ) ] ")" " -> " result *( " " out )
//   arg     = name "=" value          inputs, in declaration order (names below)
//   out     = name "=" value          outputs, only when result is SUCCESS
//   result  = status | value | "void"  status for sai_status_t ops; the value for bool / int /
//                                      string / vector returning ops; "void" for shutdown
//   event   = "event link(hw=" int ", up=" bool ")"
//           | "event fdb(learned=" bool ", mac=" mac ", vid=" int ", hw_port=" int ")"
//   status  = SAI status name without "SAI_STATUS_" (SUCCESS, FAILURE, NOT_SUPPORTED, ...,
//             INVALID_ATTRIBUTE_<i>, INVALID_ATTR_VALUE_<i>, ATTR_NOT_IMPLEMENTED_<i>,
//             UNKNOWN_ATTRIBUTE_<i>, ATTR_NOT_SUPPORTED_<i>), else the decimal number
//   value   = int (decimal; uint8_t too) | "true" | "false" | string (double quotes, \" \\ and
//             \xHH for bytes < 0x20 or >= 0x7f) | mac (aa:bb:cc:dd:ee:ff, lower case)
//             | ip (v4 dotted; v6 eight lower-case hex groups without leading zeros, no "::")
//             | prefix (ip "/" len for a contiguous mask, else ip "/" mask-ip)
//             | enum (the C++ enumerator name: Rs, InOctets, ArpRequest, DscpToTc, ...)
//             | "[" [ value *( "," value ) ] "]" | "(" value "," value ")" (pairs)
//             | "{" [ field "=" value *( "," field "=" value ) ] "}" (structs, all fields in
//               declaration order) | "none" (a null struct pointer) | "set" / "empty" (gate)
//   Special forms: FpRule.m (FpMatch) lists only present qualifiers in FpQual order as
//   <FpQual>=<data>: InPorts=[..], IpType=<int>, DstCpu=true, SrcIp/DstIp/SrcIp6/DstIp6=ip/mask-ip,
//   SrcMac/DstMac=mac/mask-mac, the others 0x<hex>/0x<hex>; then L4SrcRange=<min>-<max>,
//   L4DstRange=<min>-<max> when set and OutPorts=[..] when not empty. QosMapEntry vectors:
//   [<key>.<key_color>=<value>.<value_color>,...]. BackendConfig: profile={KEY="value",...}
//   (sorted keys). run_shell_to does not print its fd. stat id arrays print as ids=[..] (the
//   count is the length), their results as values=[..].
//   Inputs that are also outputs (netif_set_keep_tag's filter_id) appear on both sides.
//
// Mode::Changes leaves out the queries (name, is_fake, cpu_port, hardware_info, chip_limits,
// qos_supported, mirror_supported, hash_supported, fp_supported, port_stat_by_id,
// l3_intf_stat_supported); Mode::Full logs them too.
#include "recording_backend.h"

#include <cstdio>
#include <utility>

namespace ob {
namespace {

std::string s_status(sai_status_t st);

} // namespace

std::string sai_status_name(sai_status_t st) { return s_status(st); }

namespace {

std::string s_status(sai_status_t st)
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
    for (const R& r : ranges)
        if (st <= r.base && st >= r.max) return r.name + std::to_string(r.base - st);
    return std::to_string(st);
}

// ---- value formatting
std::string f(bool v) { return v ? "true" : "false"; }
std::string f(int v) { return std::to_string(v); }
std::string f(unsigned v) { return std::to_string(v); }
std::string f(unsigned long v) { return std::to_string(v); }
std::string f(const std::string& s)
{
    std::string o = "\"";
    char b[8];
    for (unsigned char c : s) {
        if (c == '"' || c == '\\') { o += '\\'; o += (char)c; }
        else if (c < 0x20 || c >= 0x7f) { std::snprintf(b, sizeof(b), "\\x%02x", c); o += b; }
        else o += (char)c;
    }
    return o + "\"";
}
std::string f(const char* s) { return f(std::string(s ? s : "")); }
std::string mac(const uint8_t* m)
{
    char b[24];
    std::snprintf(b, sizeof(b), "%02x:%02x:%02x:%02x:%02x:%02x", m[0], m[1], m[2], m[3], m[4], m[5]);
    return b;
}
std::string hex(unsigned long long v)
{
    char b[24];
    std::snprintf(b, sizeof(b), "0x%llx", v);
    return b;
}
// every formatter, declared before the templates that call them
std::string f(const IpAddr& a);
std::string f(const IpPrefix& p);
std::string f(const PortInfo& p);
std::string f(const LagMember& m);
std::string f(const QosMapEntry& e);
template <typename T> std::string f(const std::vector<T>& v);
template <typename A, typename B> std::string f(const std::pair<A, B>& p);

std::string f(const IpAddr& a)
{
    char b[64];
    if (!a.v6) {
        std::snprintf(b, sizeof(b), "%u.%u.%u.%u", a.addr[0], a.addr[1], a.addr[2], a.addr[3]);
        return b;
    }
    std::string s;
    for (int i = 0; i < 8; i++) {
        std::snprintf(b, sizeof(b), "%s%x", i ? ":" : "", (unsigned)(a.addr[2 * i] << 8 | a.addr[2 * i + 1]));
        s += b;
    }
    return s;
}
// Prefix length of a contiguous mask, -1 if it is not one.
int mask_len(const IpAddr& m)
{
    int n = m.v6 ? 16 : 4, len = 0;
    bool zero = false;
    for (int i = 0; i < n; i++)
        for (int b = 7; b >= 0; b--) {
            bool one = (m.addr[i] >> b) & 1;
            if (one && zero) return -1;
            if (one) len++;
            else zero = true;
        }
    return len;
}
std::string f(const IpPrefix& p)
{
    IpAddr m = p.mask;
    m.v6 = p.addr.v6;
    int len = mask_len(m);
    return f(p.addr) + "/" + (len >= 0 ? std::to_string(len) : f(m));
}
std::string f(Fec v) { static const char* n[] = {"None", "Rs", "Fc"}; return n[(int)v]; }
std::string f(Loopback v) { static const char* n[] = {"None", "Mac", "Phy"}; return n[(int)v]; }
std::string f(Learn v) { static const char* n[] = {"Hw", "Disable", "Drop", "Cpu"}; return n[(int)v]; }
std::string f(PortStat v)
{
    static const char* n[] = {"InOctets", "InUcastPkts", "InNonUcastPkts", "InDiscards", "InErrors", "InUnknownProtos",
                              "InBroadcastPkts", "InMulticastPkts", "OutOctets", "OutUcastPkts", "OutNonUcastPkts",
                              "OutDiscards", "OutErrors", "OutBroadcastPkts", "OutMulticastPkts"};
    return (int)v >= 0 && v < PortStat::Count ? n[(int)v] : std::to_string((int)v);
}
std::string f(TrapKind v)
{
    static const char* n[] = {"ArpRequest", "ArpResponse", "Dhcp", "Dhcpv6", "Igmp", "Mld", "Nd", "Lldp", "Lacp", "Stp", "Other"};
    return n[(int)v];
}
const char* const kQual[] = {"InPorts", "EtherType", "IpType", "IpProtocol", "SrcIp", "DstIp", "SrcIp6", "DstIp6", "L4SrcPort",
                             "L4DstPort", "TcpFlags", "IcmpTypeCode", "Tos", "DstMac", "SrcMac", "OuterVlan", "Ttl", "DstCpu"};
std::string f(FpQual v) { return v < FpQual::Count ? kQual[(int)v] : std::to_string((int)v); }
std::string f(QosMapKind v)
{
    static const char* n[] = {"DscpToTc", "Dot1pToTc", "TcToQueue", "TcToPg", "PfcPrioToQueue", "PfcPrioToPg",
                              "TcColorToDscp", "TcColorToDot1p", "DscpToColor", "Dot1pToColor"};
    return v < QosMapKind::Count ? n[(int)v] : std::to_string((int)v);
}
std::string f(StormKind v) { static const char* n[] = {"Broadcast", "Multicast", "UnknownUnicast"}; return n[(int)v]; }
std::string f(FloodKind v) { static const char* n[] = {"UnknownUnicast", "UnknownMulticast", "Broadcast"}; return n[(int)v]; }
std::string f(FpCap v)
{
    static const char* n[] = {"Ranges", "Redirect", "SetTc", "SetDscp", "Egress", "OutPorts", "EntryEnable", "EntryUpdate"};
    return n[(int)v];
}
template <typename T> std::string f(const std::vector<T>& v)
{
    std::string s = "[";
    for (size_t i = 0; i < v.size(); i++) s += (i ? "," : "") + f(v[i]);
    return s + "]";
}
template <typename A, typename B> std::string f(const std::pair<A, B>& p) { return "(" + f(p.first) + "," + f(p.second) + ")"; }
std::string ids(uint32_t n, const sai_stat_id_t* p)
{
    std::string s = "[";
    for (uint32_t i = 0; i < n; i++) s += (i ? "," : "") + std::to_string(p[i]);
    return s + "]";
}
std::string vals(uint32_t n, const uint64_t* p)
{
    std::string s = "[";
    for (uint32_t i = 0; i < n; i++) s += (i ? "," : "") + std::to_string(p[i]);
    return s + "]";
}

// struct builder: {a=1,b=2}
struct St {
    std::string s = "{";
    template <typename V> St& a(const char* n, const V& v) { return raw(n, f(v)); }
    St& raw(const char* n, const std::string& v)
    {
        if (s.size() > 1) s += ',';
        s += n;
        s += '=';
        s += v;
        return *this;
    }
    std::string str() const { return s + "}"; }
};
std::string f(const PortInfo& p) { return St().a("hw", p.hw).a("lanes", p.lanes).a("speed", p.speed).a("name", p.name).a("autoneg", p.autoneg).str(); }
std::string f(const TableUsage& u)
{
    return St().a("v4_routes_free", u.v4_routes_free).a("v6_routes_free", u.v6_routes_free).a("hosts_free", u.hosts_free)
        .a("egress_free", u.egress_free).a("fdb_free", u.fdb_free).a("ecmp_groups_free", u.ecmp_groups_free)
        .a("ecmp_members_free", u.ecmp_members_free).str();
}
std::string f(const ChipLimits& l)
{
    return St().a("buffer_bytes", l.buffer_bytes).a("acl_prio_min", l.acl_prio_min).a("acl_prio_max", l.acl_prio_max)
        .a("vrf_max", l.vrf_max).a("ecmp_groups", l.ecmp_groups).a("ecmp_paths", l.ecmp_paths).a("ecmp_members", l.ecmp_members).str();
}
std::string f(const LagMember& m) { return St().a("hw", m.hw).a("egress_disable", m.egress_disable).a("ingress_disable", m.ingress_disable).str(); }
std::string f(const BackendConfig& c)
{
    std::string s = "{";
    for (auto& kv : c.profile) s += (s.size() > 1 ? "," : "") + kv.first + "=" + f(kv.second);
    return s + "}";
}
std::string f(const FpMatch& m)
{
    St s;
    auto ip = [](const IpAddr& a, const IpAddr& k) { return f(a) + "/" + f(k); };
    for (int q = 0; q < (int)FpQual::Count; q++) {
        if (!m.has((FpQual)q)) continue;
        const char* n = kQual[q];
        switch ((FpQual)q) {
        case FpQual::InPorts: s.a(n, m.ports); break;
        case FpQual::EtherType: s.raw(n, hex(m.ether) + "/" + hex(m.ether_mask)); break;
        case FpQual::IpType: s.a(n, m.ip_type); break;
        case FpQual::IpProtocol: s.raw(n, hex(m.proto) + "/" + hex(m.proto_mask)); break;
        case FpQual::SrcIp: case FpQual::SrcIp6: s.raw(n, ip(m.src, m.src_mask)); break;
        case FpQual::DstIp: case FpQual::DstIp6: s.raw(n, ip(m.dst, m.dst_mask)); break;
        case FpQual::L4SrcPort: s.raw(n, hex(m.l4_src) + "/" + hex(m.l4_src_mask)); break;
        case FpQual::L4DstPort: s.raw(n, hex(m.l4_dst) + "/" + hex(m.l4_dst_mask)); break;
        case FpQual::TcpFlags: s.raw(n, hex(m.tcp) + "/" + hex(m.tcp_mask)); break;
        case FpQual::IcmpTypeCode: s.raw(n, hex(m.icmp) + "/" + hex(m.icmp_mask)); break;
        case FpQual::Tos: s.raw(n, hex(m.tos) + "/" + hex(m.tos_mask)); break;
        case FpQual::DstMac: s.raw(n, mac(m.dmac) + "/" + mac(m.dmac_mask)); break;
        case FpQual::SrcMac: s.raw(n, mac(m.smac) + "/" + mac(m.smac_mask)); break;
        case FpQual::OuterVlan: s.raw(n, hex(m.vlan) + "/" + hex(m.vlan_mask)); break;
        case FpQual::Ttl: s.raw(n, hex(m.ttl) + "/" + hex(m.ttl_mask)); break;
        case FpQual::DstCpu: s.raw(n, "true"); break;
        default: break;
        }
    }
    if (m.l4_src_range) s.raw("L4SrcRange", std::to_string(m.l4_src_min) + "-" + std::to_string(m.l4_src_max));
    if (m.l4_dst_range) s.raw("L4DstRange", std::to_string(m.l4_dst_min) + "-" + std::to_string(m.l4_dst_max));
    if (!m.out_ports.empty()) s.a("OutPorts", m.out_ports);
    return s.str();
}
std::string f(const FpRule& r)
{
    return St().a("priority", r.priority).raw("m", f(r.m)).a("drop", r.drop).a("drop_cancel", r.drop_cancel)
        .a("copy_cpu", r.copy_cpu).a("counter", r.counter).a("policer", r.policer).a("green_drop", r.green_drop)
        .a("yellow_drop", r.yellow_drop).a("red_drop", r.red_drop).a("cpu_copy_only", r.cpu_copy_only)
        .a("mirror_ingress", r.mirror_ingress).a("mirror_egress", r.mirror_egress).a("redirect", r.redirect)
        .a("redirect_egress", r.redirect_egress).a("set_tc", r.set_tc).a("set_dscp", r.set_dscp).str();
}
std::string f(const FpGroupExtra& x)
{
    return St().a("egress", x.egress).a("src_range", x.src_range).a("dst_range", x.dst_range).a("out_ports", x.out_ports)
        .a("redirect", x.redirect).a("set_tc", x.set_tc).a("set_dscp", x.set_dscp).a("mirror", x.mirror).str();
}
std::string f(const PolicerCfg& c)
{
    return St().a("tr_tcm", c.tr_tcm).a("packets", c.packets).a("color_blind", c.color_blind).a("cir", c.cir).a("cbs", c.cbs)
        .a("pir", c.pir).a("pbs", c.pbs).str();
}
std::string f(const TrapSpec& t)
{
    return St().a("type", t.type).a("action", t.action).a("priority", t.priority).a("queue", t.queue).a("group_up", t.group_up)
        .a("policer", t.policer).a("yellow_drop", t.yellow_drop).a("red_drop", t.red_drop).str();
}
std::string f(const QosMapEntry& e)
{
    return std::to_string(e.key) + "." + std::to_string(e.key_color) + "=" + std::to_string(e.value) + "." + std::to_string(e.value_color);
}
std::string f(const SchedCfg* c)
{
    if (!c) return "none";
    return St().a("strict", c->strict).a("weight", c->weight).a("packets", c->packets).a("min_rate", c->min_rate)
        .a("min_burst", c->min_burst).a("max_rate", c->max_rate).a("max_burst", c->max_burst).str();
}
template <typename T> std::vector<T> arr3(const T* p) { return std::vector<T>(p, p + 3); }
std::string f(const WredCfg& w)
{
    std::string e = "[";
    for (int i = 0; i < 3; i++) e += (i ? "," : "") + f(w.enable[i]);
    e += "]";
    return St().raw("enable", e).a("min", arr3(w.min)).a("max", arr3(w.max)).a("prob", arr3(w.prob)).a("weight", w.weight)
        .a("ecn", w.ecn).str();
}
std::string f(const BufferCfg* b)
{
    if (!b) return "none";
    return St().a("ingress", b->ingress).a("dynamic", b->dynamic).a("dynamic_th", b->dynamic_th).a("static_th", b->static_th)
        .a("reserved", b->reserved).a("xoff", b->xoff).a("xon", b->xon).a("xon_offset", b->xon_offset)
        .a("pool_size", b->pool_size).a("pool_xoff", b->pool_xoff).str();
}
std::string f(const MirrorCfg& m)
{
    return St().a("erspan", m.erspan).a("monitor", m.monitor).a("truncate", m.truncate).a("sample_rate", m.sample_rate)
        .a("tos", m.tos).a("ttl", m.ttl).a("src", m.src).a("dst", m.dst).raw("smac", mac(m.smac)).raw("dmac", mac(m.dmac))
        .a("gre_type", m.gre_type).a("vlan", m.vlan).a("vlan_tpid", m.vlan_tpid).a("vlan_id", m.vlan_id)
        .a("vlan_pri", m.vlan_pri).a("vlan_cfi", m.vlan_cfi).str();
}

// call line builder: op(a=1, b=2) -> STATUS out=..
struct Call {
    std::string s;
    explicit Call(const char* op) : s(op) { s += '('; }
    bool any = false;
    template <typename V> Call& a(const char* n, const V& v) { return raw(n, f(v)); }
    Call& raw(const char* n, const std::string& v)
    {
        if (any) s += ", ";
        any = true;
        s += n;
        s += '=';
        s += v;
        return *this;
    }
    Call& ret(sai_status_t st) { s += ") -> " + s_status(st); ok = st == SAI_STATUS_SUCCESS; return *this; }
    Call& ret_value(const std::string& v) { s += ") -> " + v; ok = false; return *this; }
    bool ok = false;
    template <typename V> Call& out(const char* n, const V& v) { return out_raw(n, f(v)); }
    Call& out_raw(const char* n, const std::string& v)
    {
        if (ok) { s += ' '; s += n; s += '='; s += v; }
        return *this;
    }
};

} // namespace

void CallLog::add(std::string line)
{
    std::lock_guard<std::mutex> g(mu_);
    lines_.push_back(std::move(line));
}
void CallLog::event(std::string line)
{
    std::lock_guard<std::mutex> g(mu_);
    events_++;
    lines_.push_back(std::move(line));
}
std::vector<std::string> CallLog::take()
{
    std::lock_guard<std::mutex> g(mu_);
    std::vector<std::string> v;
    v.swap(lines_);
    return v;
}
uint64_t CallLog::events_raised() const
{
    std::lock_guard<std::mutex> g(mu_);
    return events_;
}

void RecordingBackend::add(std::string line, bool query) const
{
    if (query && mode_ != Mode::Full) return;
    log_->add(std::move(line));
}

// ---- lifecycle / misc
const char* RecordingBackend::name() const
{
    const char* n = inner_->name();
    add(Call("name").ret_value(f(n)).s, true);
    return n;
}
bool RecordingBackend::is_fake() const
{
    bool v = inner_->is_fake();
    add(Call("is_fake").ret_value(f(v)).s, true);
    return v;
}
sai_status_t RecordingBackend::init(const BackendConfig& cfg, LinkCallback link_cb, FdbCallback fdb_cb)
{
    std::shared_ptr<CallLog> log = log_;
    LinkCallback l = [log, link_cb](int hw, bool up) {
        log->event("event link(hw=" + f(hw) + ", up=" + f(up) + ")");
        if (link_cb) link_cb(hw, up);
    };
    FdbCallback d = [log, fdb_cb](const FdbEvent& e) {
        log->event("event fdb(learned=" + f(e.learned) + ", mac=" + mac(e.mac) + ", vid=" + f(e.vid) + ", hw_port=" +
                   f(e.hw_port) + ")");
        if (fdb_cb) fdb_cb(e);
    };
    sai_status_t st = inner_->init(cfg, l, d);
    add(Call("init").raw("profile", f(cfg)).ret(st).s);
    return st;
}
void RecordingBackend::shutdown()
{
    inner_->shutdown();
    add("shutdown() -> void");
}
sai_status_t RecordingBackend::run_shell(const std::string& cmd)
{
    sai_status_t st = inner_->run_shell(cmd);
    add(Call("run_shell").a("cmd", cmd).ret(st).s);
    return st;
}
sai_status_t RecordingBackend::run_shell_to(const std::string& cmd, int fd)
{
    sai_status_t st = inner_->run_shell_to(cmd, fd);
    add(Call("run_shell_to").a("cmd", cmd).ret(st).s);
    return st;
}
int RecordingBackend::cpu_port() const
{
    int v = inner_->cpu_port();
    add(Call("cpu_port").ret_value(f(v)).s, true);
    return v;
}
std::vector<PortInfo> RecordingBackend::ports()
{
    std::vector<PortInfo> v = inner_->ports();
    add(Call("ports").ret_value(f(v)).s);
    return v;
}
std::string RecordingBackend::hardware_info()
{
    std::string v = inner_->hardware_info();
    add(Call("hardware_info").ret_value(f(v)).s, true);
    return v;
}
sai_status_t RecordingBackend::asic_temperatures(std::vector<int>& cur, std::vector<int>& peak)
{
    sai_status_t st = inner_->asic_temperatures(cur, peak);
    add(Call("asic_temperatures").ret(st).out("cur", cur).out("peak", peak).s);
    return st;
}
sai_status_t RecordingBackend::table_usage(TableUsage& u)
{
    sai_status_t st = inner_->table_usage(u);
    add(Call("table_usage").ret(st).out("u", u).s);
    return st;
}
sai_status_t RecordingBackend::chip_limits(ChipLimits& l)
{
    sai_status_t st = inner_->chip_limits(l);
    add(Call("chip_limits").ret(st).out("l", l).s, true);
    return st;
}

// ---- ports
#define REC(op, args, call) \
    sai_status_t st = inner_->call; \
    add(Call(#op) args .ret(st).s); \
    return st
sai_status_t RecordingBackend::port_admin_set(int hw, bool up) { REC(port_admin_set, .a("hw", hw).a("up", up), port_admin_set(hw, up)); }
sai_status_t RecordingBackend::port_link_get(int hw, bool& up)
{
    sai_status_t st = inner_->port_link_get(hw, up);
    add(Call("port_link_get").a("hw", hw).ret(st).out("up", up).s);
    return st;
}
sai_status_t RecordingBackend::mdio_c45_read(unsigned phy, unsigned dev, unsigned reg, unsigned& val)
{
    sai_status_t st = inner_->mdio_c45_read(phy, dev, reg, val);
    add(Call("mdio_c45_read").a("phy", phy).a("dev", dev).a("reg", reg).ret(st).out("val", val).s);
    return st;
}
sai_status_t RecordingBackend::mdio_c45_write(unsigned phy, unsigned dev, unsigned reg, unsigned val)
{
    REC(mdio_c45_write, .a("phy", phy).a("dev", dev).a("reg", reg).a("val", val), mdio_c45_write(phy, dev, reg, val));
}
sai_status_t RecordingBackend::port_link_gate(int hw, std::function<int()> gate)
{
    std::string g = gate ? "set" : "empty";
    REC(port_link_gate, .a("hw", hw).raw("gate", g), port_link_gate(hw, std::move(gate)));
}
sai_status_t RecordingBackend::port_speed_set(int hw, uint32_t mbps) { REC(port_speed_set, .a("hw", hw).a("mbps", mbps), port_speed_set(hw, mbps)); }
sai_status_t RecordingBackend::port_speed_get(int hw, uint32_t& mbps)
{
    sai_status_t st = inner_->port_speed_get(hw, mbps);
    add(Call("port_speed_get").a("hw", hw).ret(st).out("mbps", mbps).s);
    return st;
}
sai_status_t RecordingBackend::port_supported_speeds(int hw, std::vector<uint32_t>& out)
{
    sai_status_t st = inner_->port_supported_speeds(hw, out);
    add(Call("port_supported_speeds").a("hw", hw).ret(st).out("out", out).s);
    return st;
}
sai_status_t RecordingBackend::port_frame_max_set(int hw, uint32_t bytes) { REC(port_frame_max_set, .a("hw", hw).a("bytes", bytes), port_frame_max_set(hw, bytes)); }
sai_status_t RecordingBackend::port_fec_set(int hw, Fec fec) { REC(port_fec_set, .a("hw", hw).a("fec", fec), port_fec_set(hw, fec)); }
sai_status_t RecordingBackend::port_autoneg_set(int hw, bool on) { REC(port_autoneg_set, .a("hw", hw).a("on", on), port_autoneg_set(hw, on)); }
sai_status_t RecordingBackend::port_interface_set(int hw, const std::string& name) { REC(port_interface_set, .a("hw", hw).a("name", name), port_interface_set(hw, name)); }
sai_status_t RecordingBackend::port_ifg_set(int hw, uint32_t bits) { REC(port_ifg_set, .a("hw", hw).a("bits", bits), port_ifg_set(hw, bits)); }
sai_status_t RecordingBackend::port_loopback_set(int hw, Loopback lb) { REC(port_loopback_set, .a("hw", hw).a("lb", lb), port_loopback_set(hw, lb)); }
sai_status_t RecordingBackend::port_pvid_set(int hw, uint16_t vid) { REC(port_pvid_set, .a("hw", hw).a("vid", vid), port_pvid_set(hw, vid)); }
sai_status_t RecordingBackend::port_learn_set(int hw, Learn mode) { REC(port_learn_set, .a("hw", hw).a("mode", mode), port_learn_set(hw, mode)); }
sai_status_t RecordingBackend::port_stat_get(int hw, PortStat s, uint64_t& v)
{
    sai_status_t st = inner_->port_stat_get(hw, s, v);
    add(Call("port_stat_get").a("hw", hw).a("stat", s).ret(st).out("v", v).s);
    return st;
}
sai_status_t RecordingBackend::port_stat_clear(int hw) { REC(port_stat_clear, .a("hw", hw), port_stat_clear(hw)); }

// ---- VLAN, LAG, L2
sai_status_t RecordingBackend::vlan_create(uint16_t vid) { REC(vlan_create, .a("vid", vid), vlan_create(vid)); }
sai_status_t RecordingBackend::vlan_destroy(uint16_t vid) { REC(vlan_destroy, .a("vid", vid), vlan_destroy(vid)); }
sai_status_t RecordingBackend::vlan_port_add(uint16_t vid, int hw, bool untagged) { REC(vlan_port_add, .a("vid", vid).a("hw", hw).a("untagged", untagged), vlan_port_add(vid, hw, untagged)); }
sai_status_t RecordingBackend::vlan_port_remove(uint16_t vid, int hw) { REC(vlan_port_remove, .a("vid", vid).a("hw", hw), vlan_port_remove(vid, hw)); }
sai_status_t RecordingBackend::vlan_members(uint16_t vid, std::vector<std::pair<int, bool>>& out)
{
    sai_status_t st = inner_->vlan_members(vid, out);
    add(Call("vlan_members").a("vid", vid).ret(st).out("out", out).s);
    return st;
}
sai_status_t RecordingBackend::lag_create(int& tid)
{
    sai_status_t st = inner_->lag_create(tid);
    add(Call("lag_create").ret(st).out("tid", tid).s);
    return st;
}
sai_status_t RecordingBackend::lag_destroy(int tid) { REC(lag_destroy, .a("tid", tid), lag_destroy(tid)); }
sai_status_t RecordingBackend::lag_set_members(int tid, const std::vector<LagMember>& members) { REC(lag_set_members, .a("tid", tid).a("members", members), lag_set_members(tid, members)); }
sai_status_t RecordingBackend::l2_add(const uint8_t m[6], uint16_t vid, int hw, bool is_static, bool discard)
{
    REC(l2_add, .raw("mac", mac(m)).a("vid", vid).a("hw", hw).a("is_static", is_static).a("discard", discard), l2_add(m, vid, hw, is_static, discard));
}
sai_status_t RecordingBackend::l2_delete(const uint8_t m[6], uint16_t vid) { REC(l2_delete, .raw("mac", mac(m)).a("vid", vid), l2_delete(m, vid)); }
sai_status_t RecordingBackend::l2_flush(int hw, int vid, bool statics, bool dynamics) { REC(l2_flush, .a("hw", hw).a("vid", vid).a("statics", statics).a("dynamics", dynamics), l2_flush(hw, vid, statics, dynamics)); }
sai_status_t RecordingBackend::l2_age_set(uint32_t seconds) { REC(l2_age_set, .a("seconds", seconds), l2_age_set(seconds)); }

// ---- host interfaces, traps
sai_status_t RecordingBackend::netif_create(const std::string& ifname, int hw, uint16_t vid, const uint8_t m[6], bool keep_tag,
                                            int& netif_id, int& filter_id)
{
    sai_status_t st = inner_->netif_create(ifname, hw, vid, m, keep_tag, netif_id, filter_id);
    add(Call("netif_create").a("ifname", ifname).a("hw", hw).a("vid", vid).raw("mac", mac(m)).a("keep_tag", keep_tag).ret(st)
            .out("netif_id", netif_id).out("filter_id", filter_id).s);
    return st;
}
sai_status_t RecordingBackend::netif_destroy(int netif_id, int filter_id) { REC(netif_destroy, .a("netif_id", netif_id).a("filter_id", filter_id), netif_destroy(netif_id, filter_id)); }
sai_status_t RecordingBackend::netif_carrier_set(const std::string& ifname, bool up) { REC(netif_carrier_set, .a("ifname", ifname).a("up", up), netif_carrier_set(ifname, up)); }
sai_status_t RecordingBackend::netif_set_keep_tag(int netif_id, int hw, bool keep_tag, int& filter_id)
{
    int in = filter_id;
    sai_status_t st = inner_->netif_set_keep_tag(netif_id, hw, keep_tag, filter_id);
    add(Call("netif_set_keep_tag").a("netif_id", netif_id).a("hw", hw).a("keep_tag", keep_tag).a("filter_id", in).ret(st)
            .out("filter_id", filter_id).s);
    return st;
}
sai_status_t RecordingBackend::trap_set(TrapKind kind, bool to_cpu) { REC(trap_set, .a("kind", kind).a("to_cpu", to_cpu), trap_set(kind, to_cpu)); }
sai_status_t RecordingBackend::trap_program(const TrapSpec& t, bool remove) { REC(trap_program, .a("t", t).a("remove", remove), trap_program(t, remove)); }

// ---- L3
sai_status_t RecordingBackend::l3_intf_create(const uint8_t m[6], uint16_t vid, uint32_t mtu, int vrf, int& intf_id)
{
    sai_status_t st = inner_->l3_intf_create(m, vid, mtu, vrf, intf_id);
    add(Call("l3_intf_create").raw("mac", mac(m)).a("vid", vid).a("mtu", mtu).a("vrf", vrf).ret(st).out("intf_id", intf_id).s);
    return st;
}
sai_status_t RecordingBackend::l3_intf_update(int intf_id, const uint8_t m[6], uint16_t vid, uint32_t mtu, int vrf)
{
    REC(l3_intf_update, .a("intf_id", intf_id).raw("mac", mac(m)).a("vid", vid).a("mtu", mtu).a("vrf", vrf), l3_intf_update(intf_id, m, vid, mtu, vrf));
}
sai_status_t RecordingBackend::l3_intf_destroy(int intf_id) { REC(l3_intf_destroy, .a("intf_id", intf_id), l3_intf_destroy(intf_id)); }
sai_status_t RecordingBackend::l3_station_add(const uint8_t m[6], int& station_id)
{
    sai_status_t st = inner_->l3_station_add(m, station_id);
    add(Call("l3_station_add").raw("mac", mac(m)).ret(st).out("station_id", station_id).s);
    return st;
}
sai_status_t RecordingBackend::l3_station_delete(int station_id) { REC(l3_station_delete, .a("station_id", station_id), l3_station_delete(station_id)); }
sai_status_t RecordingBackend::l3_egress_create(int intf_id, const uint8_t dmac[6], int hw, uint16_t vid, int& egress_id)
{
    sai_status_t st = inner_->l3_egress_create(intf_id, dmac, hw, vid, egress_id);
    add(Call("l3_egress_create").a("intf_id", intf_id).raw("dmac", mac(dmac)).a("hw", hw).a("vid", vid).ret(st).out("egress_id", egress_id).s);
    return st;
}
sai_status_t RecordingBackend::l3_egress_update(int egress_id, int intf_id, const uint8_t dmac[6], int hw, uint16_t vid)
{
    REC(l3_egress_update, .a("egress_id", egress_id).a("intf_id", intf_id).raw("dmac", mac(dmac)).a("hw", hw).a("vid", vid),
        l3_egress_update(egress_id, intf_id, dmac, hw, vid));
}
sai_status_t RecordingBackend::l3_egress_destroy(int egress_id) { REC(l3_egress_destroy, .a("egress_id", egress_id), l3_egress_destroy(egress_id)); }
sai_status_t RecordingBackend::l3_egress_cpu(int& egress_id)
{
    sai_status_t st = inner_->l3_egress_cpu(egress_id);
    add(Call("l3_egress_cpu").ret(st).out("egress_id", egress_id).s);
    return st;
}
sai_status_t RecordingBackend::l3_egress_drop(int& egress_id)
{
    sai_status_t st = inner_->l3_egress_drop(egress_id);
    add(Call("l3_egress_drop").ret(st).out("egress_id", egress_id).s);
    return st;
}
sai_status_t RecordingBackend::l3_host_add(int vrf, const IpAddr& ip, int egress_id) { REC(l3_host_add, .a("vrf", vrf).a("ip", ip).a("egress_id", egress_id), l3_host_add(vrf, ip, egress_id)); }
sai_status_t RecordingBackend::l3_host_delete(int vrf, const IpAddr& ip) { REC(l3_host_delete, .a("vrf", vrf).a("ip", ip), l3_host_delete(vrf, ip)); }
sai_status_t RecordingBackend::l3_route_add(int vrf, const IpPrefix& p, int target, bool ecmp, bool replace)
{
    REC(l3_route_add, .a("vrf", vrf).a("p", p).a("target", target).a("ecmp", ecmp).a("replace", replace), l3_route_add(vrf, p, target, ecmp, replace));
}
sai_status_t RecordingBackend::l3_route_delete(int vrf, const IpPrefix& p) { REC(l3_route_delete, .a("vrf", vrf).a("p", p), l3_route_delete(vrf, p)); }
sai_status_t RecordingBackend::l3_ecmp_create(const std::vector<int>& egress, int& ecmp_id)
{
    sai_status_t st = inner_->l3_ecmp_create(egress, ecmp_id);
    add(Call("l3_ecmp_create").a("egress", egress).ret(st).out("ecmp_id", ecmp_id).s);
    return st;
}
sai_status_t RecordingBackend::l3_ecmp_update(int ecmp_id, const std::vector<int>& egress) { REC(l3_ecmp_update, .a("ecmp_id", ecmp_id).a("egress", egress), l3_ecmp_update(ecmp_id, egress)); }
sai_status_t RecordingBackend::l3_ecmp_destroy(int ecmp_id) { REC(l3_ecmp_destroy, .a("ecmp_id", ecmp_id), l3_ecmp_destroy(ecmp_id)); }

// ---- field processor, policers
sai_status_t RecordingBackend::fp_group_create(const std::vector<FpQual>& qset, int priority, int& gid)
{
    sai_status_t st = inner_->fp_group_create(qset, priority, gid);
    add(Call("fp_group_create").a("qset", qset).a("priority", priority).ret(st).out("gid", gid).s);
    return st;
}
sai_status_t RecordingBackend::fp_group_destroy(int gid) { REC(fp_group_destroy, .a("gid", gid), fp_group_destroy(gid)); }
sai_status_t RecordingBackend::fp_entry_create(int gid, const FpRule& r, int& eid)
{
    sai_status_t st = inner_->fp_entry_create(gid, r, eid);
    add(Call("fp_entry_create").a("gid", gid).a("r", r).ret(st).out("eid", eid).s);
    return st;
}
sai_status_t RecordingBackend::fp_entry_destroy(int eid) { REC(fp_entry_destroy, .a("eid", eid), fp_entry_destroy(eid)); }
sai_status_t RecordingBackend::fp_entry_counter(int eid, uint64_t& packets, uint64_t& bytes)
{
    sai_status_t st = inner_->fp_entry_counter(eid, packets, bytes);
    add(Call("fp_entry_counter").a("eid", eid).ret(st).out("packets", packets).out("bytes", bytes).s);
    return st;
}
sai_status_t RecordingBackend::policer_create(const PolicerCfg& cfg, int& pid)
{
    sai_status_t st = inner_->policer_create(cfg, pid);
    add(Call("policer_create").a("cfg", cfg).ret(st).out("pid", pid).s);
    return st;
}
sai_status_t RecordingBackend::policer_set(int pid, const PolicerCfg& cfg) { REC(policer_set, .a("pid", pid).a("cfg", cfg), policer_set(pid, cfg)); }
sai_status_t RecordingBackend::policer_destroy(int pid) { REC(policer_destroy, .a("pid", pid), policer_destroy(pid)); }

// ---- QoS
bool RecordingBackend::qos_supported() const
{
    bool v = inner_->qos_supported();
    add(Call("qos_supported").ret_value(f(v)).s, true);
    return v;
}
sai_status_t RecordingBackend::qos_map_create(QosMapKind kind, const std::vector<QosMapEntry>& entries, int& id)
{
    sai_status_t st = inner_->qos_map_create(kind, entries, id);
    add(Call("qos_map_create").a("kind", kind).a("entries", entries).ret(st).out("id", id).s);
    return st;
}
sai_status_t RecordingBackend::qos_map_update(int id, const std::vector<QosMapEntry>& entries) { REC(qos_map_update, .a("id", id).a("entries", entries), qos_map_update(id, entries)); }
sai_status_t RecordingBackend::qos_map_destroy(int id) { REC(qos_map_destroy, .a("id", id), qos_map_destroy(id)); }
sai_status_t RecordingBackend::port_qos_map_set(int hw, QosMapKind kind, int id) { REC(port_qos_map_set, .a("hw", hw).a("kind", kind).a("id", id), port_qos_map_set(hw, kind, id)); }
sai_status_t RecordingBackend::port_default_tc_set(int hw, uint8_t tc) { REC(port_default_tc_set, .a("hw", hw).a("tc", tc), port_default_tc_set(hw, tc)); }
sai_status_t RecordingBackend::port_pfc_set(int hw, bool separate, uint8_t tx, uint8_t rx) { REC(port_pfc_set, .a("hw", hw).a("separate", separate).a("tx", tx).a("rx", rx), port_pfc_set(hw, separate, tx, rx)); }
sai_status_t RecordingBackend::queue_scheduler_set(int hw, int queue, const SchedCfg* cfg) { REC(queue_scheduler_set, .a("hw", hw).a("queue", queue).a("cfg", cfg), queue_scheduler_set(hw, queue, cfg)); }
sai_status_t RecordingBackend::port_scheduler_set(int hw, const SchedCfg* cfg) { REC(port_scheduler_set, .a("hw", hw).a("cfg", cfg), port_scheduler_set(hw, cfg)); }
sai_status_t RecordingBackend::wred_create(const WredCfg& cfg, int& id)
{
    sai_status_t st = inner_->wred_create(cfg, id);
    add(Call("wred_create").a("cfg", cfg).ret(st).out("id", id).s);
    return st;
}
sai_status_t RecordingBackend::wred_update(int id, const WredCfg& cfg) { REC(wred_update, .a("id", id).a("cfg", cfg), wred_update(id, cfg)); }
sai_status_t RecordingBackend::wred_destroy(int id) { REC(wred_destroy, .a("id", id), wred_destroy(id)); }
sai_status_t RecordingBackend::queue_wred_set(int hw, int queue, int id) { REC(queue_wred_set, .a("hw", hw).a("queue", queue).a("id", id), queue_wred_set(hw, queue, id)); }
sai_status_t RecordingBackend::queue_buffer_set(int hw, int queue, const BufferCfg* cfg) { REC(queue_buffer_set, .a("hw", hw).a("queue", queue).a("cfg", cfg), queue_buffer_set(hw, queue, cfg)); }
sai_status_t RecordingBackend::pg_buffer_set(int hw, int pg, const BufferCfg* cfg) { REC(pg_buffer_set, .a("hw", hw).a("pg", pg).a("cfg", cfg), pg_buffer_set(hw, pg, cfg)); }
sai_status_t RecordingBackend::queue_stats_get(int hw, int queue, uint32_t n, const sai_stat_id_t* id, uint64_t* out)
{
    sai_status_t st = inner_->queue_stats_get(hw, queue, n, id, out);
    add(Call("queue_stats_get").a("hw", hw).a("queue", queue).raw("ids", ids(n, id)).ret(st).out_raw("values", st == SAI_STATUS_SUCCESS ? vals(n, out) : "").s);
    return st;
}
sai_status_t RecordingBackend::queue_stats_clear(int hw, int queue, uint32_t n, const sai_stat_id_t* id)
{
    REC(queue_stats_clear, .a("hw", hw).a("queue", queue).raw("ids", ids(n, id)), queue_stats_clear(hw, queue, n, id));
}
sai_status_t RecordingBackend::pg_stats_get(int hw, int pg, uint32_t n, const sai_stat_id_t* id, uint64_t* out)
{
    sai_status_t st = inner_->pg_stats_get(hw, pg, n, id, out);
    add(Call("pg_stats_get").a("hw", hw).a("pg", pg).raw("ids", ids(n, id)).ret(st).out_raw("values", st == SAI_STATUS_SUCCESS ? vals(n, out) : "").s);
    return st;
}
sai_status_t RecordingBackend::pg_stats_clear(int hw, int pg, uint32_t n, const sai_stat_id_t* id)
{
    REC(pg_stats_clear, .a("hw", hw).a("pg", pg).raw("ids", ids(n, id)), pg_stats_clear(hw, pg, n, id));
}
sai_status_t RecordingBackend::pool_stats_get(bool ingress, uint32_t n, const sai_stat_id_t* id, uint64_t* out)
{
    sai_status_t st = inner_->pool_stats_get(ingress, n, id, out);
    add(Call("pool_stats_get").a("ingress", ingress).raw("ids", ids(n, id)).ret(st).out_raw("values", st == SAI_STATUS_SUCCESS ? vals(n, out) : "").s);
    return st;
}
sai_status_t RecordingBackend::port_pfc_stat_get(int hw, int prio, bool tx, uint64_t& v)
{
    sai_status_t st = inner_->port_pfc_stat_get(hw, prio, tx, v);
    add(Call("port_pfc_stat_get").a("hw", hw).a("prio", prio).a("tx", tx).ret(st).out("v", v).s);
    return st;
}

// ---- mirror, sampling, genetlink
bool RecordingBackend::mirror_supported() const
{
    bool v = inner_->mirror_supported();
    add(Call("mirror_supported").ret_value(f(v)).s, true);
    return v;
}
sai_status_t RecordingBackend::mirror_create(const MirrorCfg& cfg, int& id)
{
    sai_status_t st = inner_->mirror_create(cfg, id);
    add(Call("mirror_create").a("cfg", cfg).ret(st).out("id", id).s);
    return st;
}
sai_status_t RecordingBackend::mirror_update(int id, const MirrorCfg& cfg) { REC(mirror_update, .a("id", id).a("cfg", cfg), mirror_update(id, cfg)); }
sai_status_t RecordingBackend::mirror_destroy(int id) { REC(mirror_destroy, .a("id", id), mirror_destroy(id)); }
sai_status_t RecordingBackend::port_mirror_set(int hw, bool ingress, const std::vector<int>& id) { REC(port_mirror_set, .a("hw", hw).a("ingress", ingress).a("ids", id), port_mirror_set(hw, ingress, id)); }
sai_status_t RecordingBackend::sample_create(uint32_t rate, int& id)
{
    sai_status_t st = inner_->sample_create(rate, id);
    add(Call("sample_create").a("rate", rate).ret(st).out("id", id).s);
    return st;
}
sai_status_t RecordingBackend::sample_update(int id, uint32_t rate) { REC(sample_update, .a("id", id).a("rate", rate), sample_update(id, rate)); }
sai_status_t RecordingBackend::sample_destroy(int id) { REC(sample_destroy, .a("id", id), sample_destroy(id)); }
sai_status_t RecordingBackend::port_sample_set(int hw, bool ingress, int id) { REC(port_sample_set, .a("hw", hw).a("ingress", ingress).a("id", id), port_sample_set(hw, ingress, id)); }
sai_status_t RecordingBackend::genetlink_create(const std::string& name, const std::string& mcgrp, int& id)
{
    sai_status_t st = inner_->genetlink_create(name, mcgrp, id);
    add(Call("genetlink_create").a("name", name).a("mcgrp", mcgrp).ret(st).out("id", id).s);
    return st;
}
sai_status_t RecordingBackend::genetlink_destroy(int id) { REC(genetlink_destroy, .a("id", id), genetlink_destroy(id)); }
sai_status_t RecordingBackend::genetlink_trap_set(int id, int32_t trap_type, bool add_) { REC(genetlink_trap_set, .a("id", id).a("trap_type", trap_type).a("add", add_), genetlink_trap_set(id, trap_type, add_)); }

// ---- hash, storm, flood
bool RecordingBackend::hash_supported() const
{
    bool v = inner_->hash_supported();
    add(Call("hash_supported").ret_value(f(v)).s, true);
    return v;
}
sai_status_t RecordingBackend::hash_set(bool lag, const std::vector<int32_t>& fields, uint32_t seed) { REC(hash_set, .a("lag", lag).a("fields", fields).a("seed", seed), hash_set(lag, fields, seed)); }
sai_status_t RecordingBackend::port_storm_set(int hw, StormKind kind, int policer) { REC(port_storm_set, .a("hw", hw).a("kind", kind).a("policer", policer), port_storm_set(hw, kind, policer)); }
sai_status_t RecordingBackend::vlan_flood_set(uint16_t vid, FloodKind kind, bool on) { REC(vlan_flood_set, .a("vid", vid).a("kind", kind).a("on", on), vlan_flood_set(vid, kind, on)); }

// ---- optional L3 / counter / ACL extras
bool RecordingBackend::l3_intf_stat_supported(sai_stat_id_t id) const
{
    bool v = inner_->l3_intf_stat_supported(id);
    add(Call("l3_intf_stat_supported").a("id", id).ret_value(f(v)).s, true);
    return v;
}
sai_status_t RecordingBackend::l3_intf_stats(int intf_id, uint32_t n, const sai_stat_id_t* id, uint64_t* out, bool clear)
{
    sai_status_t st = inner_->l3_intf_stats(intf_id, n, id, out, clear);
    add(Call("l3_intf_stats").a("intf_id", intf_id).raw("ids", ids(n, id)).a("clear", clear).ret(st)
            .out_raw("values", st == SAI_STATUS_SUCCESS ? vals(n, out) : "").s);
    return st;
}
sai_status_t RecordingBackend::l3_intf_admin_set(int intf_id, bool v4, bool v6) { REC(l3_intf_admin_set, .a("intf_id", intf_id).a("v4", v4).a("v6", v6), l3_intf_admin_set(intf_id, v4, v6)); }
sai_status_t RecordingBackend::l3_egress_glean(int& egress_id)
{
    sai_status_t st = inner_->l3_egress_glean(egress_id);
    add(Call("l3_egress_glean").ret(st).out("egress_id", egress_id).s);
    return st;
}
bool RecordingBackend::port_stat_by_id(sai_stat_id_t id) const
{
    bool v = inner_->port_stat_by_id(id);
    add(Call("port_stat_by_id").a("id", id).ret_value(f(v)).s, true);
    return v;
}
sai_status_t RecordingBackend::port_stats_by_id(int hw, uint32_t n, const sai_stat_id_t* id, uint64_t* out)
{
    sai_status_t st = inner_->port_stats_by_id(hw, n, id, out);
    add(Call("port_stats_by_id").a("hw", hw).raw("ids", ids(n, id)).ret(st).out_raw("values", st == SAI_STATUS_SUCCESS ? vals(n, out) : "").s);
    return st;
}
bool RecordingBackend::fp_supported(FpCap cap) const
{
    bool v = inner_->fp_supported(cap);
    add(Call("fp_supported").a("cap", cap).ret_value(f(v)).s, true);
    return v;
}
sai_status_t RecordingBackend::fp_group_create_ex(const std::vector<FpQual>& qset, int priority, const FpGroupExtra& x, int& gid)
{
    sai_status_t st = inner_->fp_group_create_ex(qset, priority, x, gid);
    add(Call("fp_group_create_ex").a("qset", qset).a("priority", priority).a("x", x).ret(st).out("gid", gid).s);
    return st;
}
sai_status_t RecordingBackend::fp_entry_update(int eid, const FpRule& r) { REC(fp_entry_update, .a("eid", eid).a("r", r), fp_entry_update(eid, r)); }
sai_status_t RecordingBackend::fp_entry_enable(int eid, bool on) { REC(fp_entry_enable, .a("eid", eid).a("on", on), fp_entry_enable(eid, on)); }
sai_status_t RecordingBackend::fp_group_free(int gid, uint32_t& entries)
{
    sai_status_t st = inner_->fp_group_free(gid, entries);
    add(Call("fp_group_free").a("gid", gid).ret(st).out("entries", entries).s);
    return st;
}
#undef REC

} // namespace ob
