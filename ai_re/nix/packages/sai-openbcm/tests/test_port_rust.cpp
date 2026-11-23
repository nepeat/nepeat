// SPDX-License-Identifier: Apache-2.0
// tests/test_port.h over the Rust libsai (rust/sai): every question goes through the library's one
// test-only C function, sai_rs_test_query (cargo feature "test-port" of sai-ffi; wire contract:
// rust/sai/crates/sai-test-port/include/sai_rs_test.h). Link: test_sai.o + this file + the Rust
// libsai.so (rust/sai/tests/test-port/run.sh does it). Needs no library-internal header.
//
// Differences from tests/test_port_cpp.cpp (stated, on purpose):
//  - Never aborts. A request the library cannot answer (UNSUPPORTED, BAD_REQUEST, FAILURE, a
//    malformed reply) prints "test_port_rust: <request>: <reason>" to stderr once per request name
//    and the function returns its absent answer (-1, 0, false, empty, a default struct), so the
//    suite runs to its end and its sections show progress. tp::port(hw) of a missing port returns
//    the default Port (hw -1). The C++ port aborts there (no fake backend, object_count of another
//    type, set_port_stat without a counter, a missing port). At exit one line per request name
//    that went unanswered: "test_port_rust: summary <request> <calls>".
//  - diag_start(""): nothing is sent (the header's meaning: empty path, do nothing).
//  - PROFILE SHIM (to be removed when the library reads SAI_OPENBCM_BACKEND itself): the suite's
//    profile uses the C++ keys (SAI_OPENBCM_BACKEND=fake, SAI_OPENBCM_FAKE_*) and port numbers that
//    assume the C++ fake's numbering hw = 1 + i*lanes. This file defines sai_api_initialize, calls
//    the library's through dlsym(RTLD_NEXT) and hands it a wrapped service method table whose
//    profile_get_value answers, when the suite's table has no such key itself:
//      SAI_RS_BACKEND         "virtual" for SAI_OPENBCM_BACKEND=fake, else the SAI_OPENBCM_BACKEND
//                             value unchanged (absent: NULL)
//      SAI_RS_VIRTUAL_COMPAT  "cpp" when SAI_OPENBCM_BACKEND=fake
//    Every other key, and profile_get_next_value, pass through. Each distinct mapping is printed
//    once ("test_port_rust: profile shim: ..."). TP_RUST_PROFILE_SHIM=0 turns the shim off (the
//    suite's table is passed unchanged) to measure the library's own behaviour.
#include "test_port.h"

#if __has_include("sai_rs_test.h")
#include "sai_rs_test.h"
#else
// Until rust/sai/crates/sai-test-port/include/sai_rs_test.h exists: the contract's declaration.
extern "C" int32_t sai_rs_test_query(const char* request, size_t request_len, char* reply, size_t reply_cap,
                                     size_t* reply_len);
#endif

#include <dlfcn.h>

#include <charconv>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <limits>
#include <map>
#include <mutex>
#include <set>
#include <string>
#include <string_view>
#include <type_traits>
#include <utility>
#include <vector>

namespace {

// ============================================================ unanswered requests (stderr)

class Unanswered {
public:
    void note(const std::string& name, const std::string& why)
    {
        std::lock_guard<std::mutex> g(mu_);
        auto& n = calls_[name];
        if (n++ == 0) std::fprintf(stderr, "test_port_rust: %s: %s\n", name.c_str(), why.c_str());
    }
    ~Unanswered()
    {
        for (auto& kv : calls_) std::fprintf(stderr, "test_port_rust: summary %s %llu\n", kv.first.c_str(), (unsigned long long)kv.second);
    }

private:
    std::mutex mu_;
    std::map<std::string, unsigned long long> calls_;
};

Unanswered& unanswered()
{
    static Unanswered u;
    return u;
}

std::string one_line(std::string s)
{
    for (char& c : s)
        if (c == '\n' || c == '\r') c = ' ';
    return s;
}

// ====================================================================== values: encoding

bool unreserved(unsigned char c)
{
    return (c >= 'A' && c <= 'Z') || (c >= 'a' && c <= 'z') || (c >= '0' && c <= '9') || c == '_' || c == '.' || c == ':' ||
           c == '/' || c == '-';
}

std::string pct_encode(std::string_view s)
{
    static const char hex[] = "0123456789ABCDEF";
    std::string o;
    for (unsigned char c : s) {
        if (unreserved(c)) {
            o += (char)c;
        } else {
            o += '%';
            o += hex[c >> 4];
            o += hex[c & 15];
        }
    }
    return o;
}

int hexval(char c)
{
    if (c >= '0' && c <= '9') return c - '0';
    if (c >= 'a' && c <= 'f') return c - 'a' + 10;
    if (c >= 'A' && c <= 'F') return c - 'A' + 10;
    return -1;
}

bool pct_decode(std::string_view s, std::string& out)
{
    out.clear();
    for (size_t i = 0; i < s.size(); i++) {
        if (s[i] != '%') {
            out += s[i];
            continue;
        }
        if (i + 2 >= s.size()) return false;
        int hi = hexval(s[i + 1]), lo = hexval(s[i + 2]);
        if (hi < 0 || lo < 0) return false;
        out += (char)(hi * 16 + lo);
        i += 2;
    }
    return true;
}

template <typename T> bool parse_int(std::string_view s, T& out)
{
    static_assert(std::is_integral_v<T> && !std::is_same_v<T, bool>);
    if (s.empty()) return false;
    const char* end = s.data() + s.size();
    if constexpr (std::is_signed_v<T>) {
        long long v = 0;
        auto r = std::from_chars(s.data(), end, v);
        if (r.ec != std::errc() || r.ptr != end) return false;
        if (v < (long long)std::numeric_limits<T>::min() || v > (long long)std::numeric_limits<T>::max()) return false;
        out = (T)v;
    } else {
        unsigned long long v = 0;
        auto r = std::from_chars(s.data(), end, v);
        if (r.ec != std::errc() || r.ptr != end) return false;
        if (v > (unsigned long long)std::numeric_limits<T>::max()) return false;
        out = (T)v;
    }
    return true;
}

bool parse_bool(std::string_view s, bool& out)
{
    if (s == "0") out = false;
    else if (s == "1") out = true;
    else return false;
    return true;
}

bool parse_mac(std::string_view s, uint8_t out[6])
{
    if (s.size() != 17) return false;
    for (int i = 0; i < 6; i++) {
        int hi = hexval(s[(size_t)i * 3]), lo = hexval(s[(size_t)i * 3 + 1]);
        if (hi < 0 || lo < 0 || (i < 5 && s[(size_t)i * 3 + 2] != ':')) return false;
        out[i] = (uint8_t)(hi * 16 + lo);
    }
    return true;
}

bool parse_ip(std::string_view s, tp::Ip& out)
{
    size_t n;
    if (s.rfind("4/", 0) == 0) n = 4;
    else if (s.rfind("6/", 0) == 0) n = 16;
    else return false;
    if (s.size() != 2 + 2 * n) return false;
    tp::Ip ip;
    ip.v6 = n == 16;
    for (size_t i = 0; i < n; i++) {
        int hi = hexval(s[2 + 2 * i]), lo = hexval(s[3 + 2 * i]);
        if (hi < 0 || lo < 0) return false;
        ip.addr[i] = (uint8_t)(hi * 16 + lo);
    }
    out = ip;
    return true;
}

std::vector<std::string_view> split(std::string_view s, char sep)
{
    std::vector<std::string_view> v;
    size_t start = 0;
    for (;;) {
        size_t p = s.find(sep, start);
        v.push_back(s.substr(start, p == std::string_view::npos ? std::string_view::npos : p - start));
        if (p == std::string_view::npos) return v;
        start = p + 1;
    }
}

// ============================================================ request words

struct Mac { const uint8_t* b; };
struct Str { std::string_view s; };
struct IpArg { const uint8_t* b; size_t n; };

IpArg ip_arg(const sai_ip_address_t& a)
{
    if (a.addr_family == SAI_IP_ADDR_FAMILY_IPV6) return {a.addr.ip6, 16};
    return {reinterpret_cast<const uint8_t*>(&a.addr.ip4), 4};   // sai_ip4_t: network order in memory
}

void put(std::string& o, bool v) { o += v ? '1' : '0'; }
template <typename T, typename = std::enable_if_t<std::is_integral_v<T> || std::is_enum_v<T>>> void put(std::string& o, T v)
{
    if constexpr (std::is_enum_v<T>) o += std::to_string((long long)v);
    else o += std::to_string(v);
}
void put(std::string& o, const char* w) { o += w; }
void put(std::string& o, Str s) { o += pct_encode(s.s); }
void put(std::string& o, Mac m)
{
    char b[18];
    std::snprintf(b, sizeof b, "%02x:%02x:%02x:%02x:%02x:%02x", m.b[0], m.b[1], m.b[2], m.b[3], m.b[4], m.b[5]);
    o += b;
}
void put(std::string& o, IpArg ip)
{
    o += ip.n == 16 ? "6/" : "4/";
    char b[3];
    for (size_t i = 0; i < ip.n; i++) {
        std::snprintf(b, sizeof b, "%02x", ip.b[i]);
        o += b;
    }
}

template <typename... A> std::string req(const char* name, const A&... a)
{
    std::string o = name;
    ((o += ' ', put(o, a)), ...);
    return o;
}

// ============================================================ reply records

class Rec {
public:
    // A "key=value key=value" line; on a malformed line error() says why.
    explicit Rec(std::string_view line)
    {
        for (std::string_view f : split(line, ' ')) {
            size_t eq = f.find('=');
            if (eq == std::string_view::npos || eq == 0) {
                fail("malformed field '" + std::string(f) + "'");
                return;
            }
            std::string k(f.substr(0, eq));
            for (auto& kv : fields_)
                if (kv.first == k) {
                    fail("duplicate field " + k);
                    return;
                }
            fields_.emplace_back(std::move(k), std::string(f.substr(eq + 1)));
        }
    }
    bool ok() const { return err_.empty(); }
    const std::string& error() const { return err_; }

    template <typename T> T num(const char* k)
    {
        T v{};
        if (const std::string* s = raw(k))
            if (!parse_int(*s, v)) bad(k, *s);
        return v;
    }
    bool flag(const char* k)
    {
        bool v = false;
        if (const std::string* s = raw(k))
            if (!parse_bool(*s, v)) bad(k, *s);
        return v;
    }
    char op(const char* k)
    {
        const std::string* s = raw(k);
        if (s && (*s == "R" || *s == "W")) return (*s)[0];
        if (s) bad(k, *s);
        return '?';
    }
    std::string str(const char* k)
    {
        std::string v;
        if (const std::string* s = raw(k))
            if (!pct_decode(*s, v)) bad(k, *s);
        return v;
    }
    void mac(const char* k, uint8_t out[6])
    {
        if (const std::string* s = raw(k))
            if (!parse_mac(*s, out)) bad(k, *s);
    }
    tp::Ip ip(const char* k)
    {
        tp::Ip v;
        if (const std::string* s = raw(k))
            if (!parse_ip(*s, v)) bad(k, *s);
        return v;
    }
    template <typename T> std::vector<T> nums(const char* k)
    {
        std::vector<T> v;
        const std::string* s = raw(k);
        if (!s || s->empty()) return v;
        for (std::string_view e : split(*s, ',')) {
            T x{};
            if (!parse_int(e, x)) {
                bad(k, *s);
                return {};
            }
            v.push_back(x);
        }
        return v;
    }
    // A list of exactly n elements, each parsed by f(std::string_view, T&).
    template <typename T, size_t N, typename F> void fixed(const char* k, T (&out)[N], F f)
    {
        const std::string* s = raw(k);
        if (!s) return;
        auto parts = split(*s, ',');
        if (parts.size() != N) return bad(k, *s);
        for (size_t i = 0; i < N; i++)
            if (!f(parts[i], out[i])) return bad(k, *s);
    }
    std::vector<tp::QosMapEntry> qos_entries(const char* k)
    {
        std::vector<tp::QosMapEntry> v;
        const std::string* s = raw(k);
        if (!s || s->empty()) return v;
        for (std::string_view e : split(*s, ',')) {
            auto p = split(e, '.');
            tp::QosMapEntry q{};
            if (p.size() != 4 || !parse_int(p[0], q.key) || !parse_int(p[1], q.key_color) || !parse_int(p[2], q.value) ||
                !parse_int(p[3], q.value_color)) {
                bad(k, *s);
                return {};
            }
            v.push_back(q);
        }
        return v;
    }

private:
    const std::string* raw(const char* k)
    {
        for (auto& kv : fields_)
            if (kv.first == k) return &kv.second;
        fail(std::string("missing field ") + k);
        return nullptr;
    }
    void bad(const char* k, const std::string& v) { fail(std::string("bad ") + k + "=" + v); }
    void fail(std::string why)
    {
        if (err_.empty()) err_ = std::move(why);
    }
    std::vector<std::pair<std::string, std::string>> fields_;
    std::string err_;
};

// ============================================================ the one call

enum class Got { Ok, Absent, Error };

std::string name_of(const std::string& request) { return request.substr(0, request.find(' ')); }

// Runs a request. Ok: *recs holds the reply's records (parsed, not yet read). Absent: the thing
// does not exist. Error: the library could not answer (already reported on stderr).
Got ask(const std::string& request, std::vector<Rec>* recs)
{
    const std::string name = name_of(request);
    std::string buf(4096, '\0');
    int32_t rc = -1;
    size_t len = 0;
    for (int attempt = 0; attempt < 8; attempt++) {
        len = 0;
        rc = sai_rs_test_query(request.data(), request.size(), &buf[0], buf.size(), &len);
        if (rc != 2) break;
        if (len <= buf.size()) {
            unanswered().note(name, "TOO_SMALL with a length that fits (" + std::to_string(len) + ")");
            return Got::Error;
        }
        buf.assign(len, '\0');
    }
    std::string reply = len <= buf.size() ? buf.substr(0, len) : std::string();
    switch (rc) {
    case 0: break;
    case 1: return Got::Absent;
    case 2: unanswered().note(name, "TOO_SMALL after 8 attempts"); return Got::Error;
    case 3: unanswered().note(name, "UNSUPPORTED: " + one_line(reply)); return Got::Error;
    case 4: unanswered().note(name, "BAD_REQUEST: " + one_line(reply) + " [" + request + "]"); return Got::Error;
    case -1: unanswered().note(name, "FAILURE"); return Got::Error;
    default: unanswered().note(name, "unknown return code " + std::to_string(rc)); return Got::Error;
    }
    if (len > buf.size()) {
        unanswered().note(name, "OK with reply_len > reply_cap");
        return Got::Error;
    }
    if (!reply.empty() && reply.back() != '\n') {
        unanswered().note(name, "reply does not end in a newline");
        return Got::Error;
    }
    std::vector<Rec> out;
    size_t start = 0;
    while (start < reply.size()) {
        size_t nl = reply.find('\n', start);
        out.emplace_back(std::string_view(reply).substr(start, nl - start));
        if (!out.back().ok()) {
            unanswered().note(name, "malformed reply: " + out.back().error());
            return Got::Error;
        }
        start = nl + 1;
    }
    if (recs) *recs = std::move(out);
    return Got::Ok;
}

// A record read completely? (else reported, and the caller returns its absent answer)
bool done(const Rec& r, const std::string& request)
{
    if (r.ok()) return true;
    unanswered().note(name_of(request), "malformed reply: " + r.error());
    return false;
}

// Exactly one record. false: absent or not answered.
bool one(const std::string& request, std::vector<Rec>& recs)
{
    if (ask(request, &recs) != Got::Ok) return false;
    if (recs.size() == 1) return true;
    unanswered().note(name_of(request), std::to_string(recs.size()) + " records, expected 1");
    return false;
}

// A hook (empty reply expected).
void act(const std::string& request) { (void)ask(request, nullptr); }

// A "value=<n>" answer, or absent.
template <typename T> T value(const std::string& request, T absent)
{
    std::vector<Rec> r;
    if (!one(request, r)) return absent;
    T v = r[0].num<T>("value");
    return done(r[0], request) ? v : absent;
}
bool flag(const std::string& request)
{
    std::vector<Rec> r;
    if (!one(request, r)) return false;
    bool v = r[0].flag("value");
    return done(r[0], request) && v;
}
std::string text(const std::string& request)
{
    std::vector<Rec> r;
    if (!one(request, r)) return std::string();
    std::string v = r[0].str("text");
    return done(r[0], request) ? v : std::string();
}
// Every record through f(Rec&) -> T; empty if any record is malformed.
template <typename F> auto records(const std::string& request, F f) -> std::vector<decltype(f(std::declval<Rec&>()))>
{
    std::vector<decltype(f(std::declval<Rec&>()))> v;
    std::vector<Rec> recs;
    if (ask(request, &recs) != Got::Ok) return v;
    for (Rec& r : recs) {
        v.push_back(f(r));
        if (!done(r, request)) return {};
    }
    return v;
}
// One record through f(Rec&, T&) into out; false: absent, unanswered or malformed.
template <typename T, typename F> bool record(const std::string& request, T& out, F f)
{
    std::vector<Rec> r;
    if (!one(request, r)) return false;
    T v{};
    f(r[0], v);
    if (!done(r[0], request)) return false;
    out = v;
    return true;
}

const char* table_name(tp::Table t)
{
    switch (t) {
    case tp::T_STATIONS: return "T_STATIONS";
    case tp::T_QOS_MAPS: return "T_QOS_MAPS";
    case tp::T_PORT_QOS_MAPS: return "T_PORT_QOS_MAPS";
    case tp::T_QUEUE_SCHEDULERS: return "T_QUEUE_SCHEDULERS";
    case tp::T_PORT_SCHEDULERS: return "T_PORT_SCHEDULERS";
    case tp::T_QUEUE_BUFFERS: return "T_QUEUE_BUFFERS";
    case tp::T_PG_BUFFERS: return "T_PG_BUFFERS";
    case tp::T_MIRRORS: return "T_MIRRORS";
    case tp::T_PORT_MIRRORS: return "T_PORT_MIRRORS";
    case tp::T_SAMPLES: return "T_SAMPLES";
    case tp::T_PORT_SAMPLES: return "T_PORT_SAMPLES";
    case tp::T_STORM: return "T_STORM";
    case tp::T_FLOOD: return "T_FLOOD";
    case tp::T_FP_GROUPS: return "T_FP_GROUPS";
    case tp::T_FP_ENTRIES: return "T_FP_ENTRIES";
    case tp::T_POLICERS: return "T_POLICERS";
    }
    return "T_UNKNOWN";
}

const char* feature_name(tp::Feature f)
{
    switch (f) {
    case tp::FEATURE_QOS: return "qos";
    case tp::FEATURE_MIRROR: return "mirror";
    case tp::FEATURE_L2_EXTRAS: return "l2";
    }
    return "unknown";
}

void read_port(Rec& r, tp::Port& p)
{
    p.hw = r.num<int>("hw");
    p.admin = r.flag("admin");
    p.link = r.flag("link");
    p.fec = r.num<int32_t>("fec");
    p.autoneg = r.flag("autoneg");
    p.iface = r.str("iface");
    p.ifg = r.num<uint32_t>("ifg");
    p.quirk_calls = r.num<int>("quirk_calls");
    p.loopback = r.num<int32_t>("loopback");
    p.pvid = r.num<uint16_t>("pvid");
    p.frame_max = r.num<uint32_t>("frame_max");
    p.learn = r.num<int>("learn");
}

void read_egress(Rec& r, tp::Egress& e)
{
    e.id = r.num<int>("id");
    e.intf = r.num<int>("intf");
    r.mac("mac", e.mac);
    e.dest = r.num<int>("dest");
    e.vid = r.num<uint16_t>("vid");
    e.cpu = r.flag("cpu");
    e.drop = r.flag("drop");
}

void read_fp_entry(Rec& r, tp::FpEntry& e)
{
    e.eid = r.num<int>("eid");
    e.gid = r.num<int>("gid");
    e.priority = r.num<uint32_t>("priority");
    e.present = r.num<uint32_t>("present");
    e.in_ports = r.nums<int>("in_ports");
    e.ether = r.num<uint16_t>("ether");
    e.ip_type = r.num<int32_t>("ip_type");
    e.l4_src = r.num<uint16_t>("l4_src");
    e.l4_dst = r.num<uint16_t>("l4_dst");
    e.dst = r.ip("dst");
    e.dst_mask = r.ip("dst_mask");
    e.drop = r.flag("drop");
    e.drop_cancel = r.flag("drop_cancel");
    e.copy_cpu = r.flag("copy_cpu");
    e.counter = r.flag("counter");
    e.policer = r.num<int>("policer");
    e.red_drop = r.flag("red_drop");
    e.cpu_copy_only = r.flag("cpu_copy_only");
    e.mirror_ingress = r.nums<int>("mirror_ingress");
    e.mirror_egress = r.nums<int>("mirror_egress");
}

void read_sched(Rec& r, tp::Sched& s)
{
    s.strict = r.flag("strict");
    s.weight = r.num<uint32_t>("weight");
    s.packets = r.flag("packets");
    s.min_rate = r.num<uint64_t>("min_rate");
    s.min_burst = r.num<uint64_t>("min_burst");
    s.max_rate = r.num<uint64_t>("max_rate");
    s.max_burst = r.num<uint64_t>("max_burst");
}

void read_buffer(Rec& r, tp::Buffer& b)
{
    b.ingress = r.flag("ingress");
    b.dynamic = r.flag("dynamic");
    b.dynamic_th = r.num<int8_t>("dynamic_th");
    b.static_th = r.num<uint64_t>("static_th");
    b.reserved = r.num<uint64_t>("reserved");
    b.xoff = r.num<uint64_t>("xoff");
    b.xon = r.num<uint64_t>("xon");
    b.xon_offset = r.num<uint64_t>("xon_offset");
    b.pool_size = r.num<uint64_t>("pool_size");
    b.pool_xoff = r.num<uint64_t>("pool_xoff");
}

// ============================================================ profile shim

using InitFn = sai_status_t (*)(uint64_t, const sai_service_method_table_t*);
const sai_service_method_table_t* g_user_smt = nullptr;
std::mutex g_shim_mu;
std::set<std::string> g_shim_said;

const char* user_value(sai_switch_profile_id_t id, const char* k)
{
    const sai_service_method_table_t* t = g_user_smt;
    return t && t->profile_get_value ? t->profile_get_value(id, k) : nullptr;
}

const char* shim_said(const char* key, const char* value, const char* why)
{
    std::lock_guard<std::mutex> g(g_shim_mu);
    std::string line = std::string(key) + " -> " + value + " (" + why + ")";
    if (g_shim_said.insert(line).second) std::fprintf(stderr, "test_port_rust: profile shim: %s\n", line.c_str());
    return value;
}

const char* shim_get(sai_switch_profile_id_t id, const char* variable)
{
    if (!variable) return nullptr;
    const char* own = user_value(id, variable);
    if (own) return own;
    bool rs_backend = std::strcmp(variable, "SAI_RS_BACKEND") == 0;
    bool rs_compat = std::strcmp(variable, "SAI_RS_VIRTUAL_COMPAT") == 0;
    if (!rs_backend && !rs_compat) return nullptr;
    const char* be = user_value(id, "SAI_OPENBCM_BACKEND");
    if (!be) return nullptr;
    bool fake = std::strcmp(be, "fake") == 0;
    if (rs_backend) return fake ? shim_said(variable, "virtual", "SAI_OPENBCM_BACKEND=fake") : shim_said(variable, be, "SAI_OPENBCM_BACKEND, unchanged");
    return fake ? shim_said(variable, "cpp", "SAI_OPENBCM_BACKEND=fake") : nullptr;
}

int shim_next(sai_switch_profile_id_t id, const char** variable, const char** value)
{
    const sai_service_method_table_t* t = g_user_smt;
    return t && t->profile_get_next_value ? t->profile_get_next_value(id, variable, value) : -1;
}

const sai_service_method_table_t g_shim_smt = {shim_get, shim_next};

} // namespace

extern "C" sai_status_t sai_api_initialize(uint64_t flags, const sai_service_method_table_t* services)
{
    static InitFn next = nullptr;
    if (!next) {
        void* sym = dlsym(RTLD_NEXT, "sai_api_initialize");
        if (!sym) {
            std::fprintf(stderr, "test_port_rust: profile shim: no sai_api_initialize after this one: %s\n", dlerror());
            return SAI_STATUS_FAILURE;
        }
        next = reinterpret_cast<InitFn>(sym);
    }
    const char* off = std::getenv("TP_RUST_PROFILE_SHIM");
    if (!services || (off && std::strcmp(off, "0") == 0)) return next(flags, services);
    g_user_smt = services;
    return next(flags, &g_shim_smt);
}

namespace tp {

// ================================================================== library and OID layout
bool has_hardware_backend() { return flag("has_hardware_backend"); }
bool switch_created() { return flag("switch_created"); }
sai_object_id_t make_oid(sai_object_type_t type, uint64_t index, uint32_t sw)
{
    return value<sai_object_id_t>(req("make_oid", type, index, sw), SAI_NULL_OBJECT_ID);
}
sai_object_type_t oid_type(sai_object_id_t oid)
{
    return (sai_object_type_t)value<int32_t>(req("oid_type", oid), SAI_OBJECT_TYPE_NULL);
}
uint64_t oid_index(sai_object_id_t oid) { return value<uint64_t>(req("oid_index", oid), 0); }
uint32_t oid_switch(sai_object_id_t oid) { return value<uint32_t>(req("oid_switch", oid), 0); }
uint64_t oid_index_mask() { return value<uint64_t>("oid_index_mask", 0); }

std::vector<AttrCap> attr_caps()
{
    return records("attr_caps", [](Rec& r) {
        return AttrCap{(sai_object_type_t)r.num<int32_t>("ot"), r.num<sai_attr_id_t>("id"), r.flag("create"), r.flag("set"),
                       r.flag("get")};
    });
}

// ======================================================= the library's view of its objects
int backend_id(sai_object_id_t oid) { return value<int>(req("backend_id", oid), -1); }
Rif rif(sai_object_id_t oid)
{
    Rif out;
    record(req("rif", oid), out, [](Rec& r, Rif& v) {
        v.intf = r.num<int>("intf");
        v.vrf = r.num<int>("vrf");
        v.vid = r.num<uint16_t>("vid");
    });
    return out;
}
sai_object_id_t port_lag(sai_object_id_t port) { return value<sai_object_id_t>(req("port_lag", port), SAI_NULL_OBJECT_ID); }
bool trap_programmed(sai_object_id_t trap) { return flag(req("trap_programmed", trap)); }
size_t object_count(sai_object_type_t type) { return value<size_t>(req("object_count", type), 0); }
size_t acl_binding_count() { return value<size_t>("acl_binding_count", 0); }
int copp_group() { return value<int>("copp_group", -1); }
size_t copp_rule_count() { return value<size_t>("copp_rule_count", 0); }
int neighbor_egress(sai_object_id_t rif, const sai_ip_address_t& ip)
{
    return value<int>(req("neighbor_egress", rif, ip_arg(ip)), -1);
}
uint64_t events_delivered() { return value<uint64_t>("events_delivered", 0); }

// ======================================================== library services (diag, logging)
void diag_start(const std::string& socket_path)
{
    if (!socket_path.empty()) act(req("diag_start", Str{socket_path}));
}
void diag_stop() { act("diag_stop"); }
void log_to_stderr(bool on) { act(req("log_to_stderr", on)); }
bool ext_phy_active() { return flag("ext_phy_active"); }
std::string ext_phy_status() { return text("ext_phy_status"); }
bool ext_macsec_active() { return flag("ext_macsec_active"); }
std::string ext_macsec_status() { return text("ext_macsec_status"); }

// ================================================================ virtual backend: hooks
void set_feature(Feature f, bool on) { act(req("set_feature", feature_name(f), on)); }
void inject_link(int hw, bool up) { act(req("inject_link", hw, up)); }
void linkscan() { act("linkscan"); }
void inject_fdb(bool learned, const uint8_t mac[6], uint16_t vid, int dest) { act(req("inject_fdb", learned, Mac{mac}, vid, dest)); }
void set_port_stat(int hw, sai_stat_id_t id, uint64_t v) { act(req("set_port_stat", hw, id, v)); }
void set_queue_stat(int hw, int queue, sai_stat_id_t id, uint64_t v) { act(req("set_queue_stat", hw, queue, id, v)); }
void set_pg_stat(int hw, int pg, sai_stat_id_t id, uint64_t v) { act(req("set_pg_stat", hw, pg, id, v)); }
void fp_hit(int eid, uint64_t packets, uint64_t bytes) { act(req("fp_hit", eid, packets, bytes)); }
void set_temperatures(const std::vector<int>& tenths)
{
    std::string list;
    for (size_t i = 0; i < tenths.size(); i++) list += (i ? "," : "") + std::to_string(tenths[i]);
    act(req("set_temperatures", list.c_str()));   // empty list: "set_temperatures " (an empty word)
}
void set_temperature_fail(bool fail) { act(req("set_temperature_fail", fail)); }
void mdio_set(unsigned phy, unsigned dev, unsigned reg, unsigned v) { act(req("mdio_set", phy, dev, reg, v)); }
void mdio_set_dead(unsigned phy, bool dead) { act(req("mdio_set_dead", phy, dead)); }
std::vector<MdioOp> mdio_log(bool clear)
{
    return records(req("mdio_log", clear), [](Rec& r) {
        return MdioOp{r.op("op"), r.num<unsigned>("phy"), r.num<unsigned>("dev"), r.num<unsigned>("reg"), r.num<unsigned>("val")};
    });
}
std::vector<std::string> shell_log()
{
    return records("shell_log", [](Rec& r) { return r.str("cmd"); });
}
void backend_host_insert(int vrf, const sai_ip_address_t& ip, int egress) { act(req("backend_host_insert", vrf, ip_arg(ip), egress)); }
void backend_host_erase(int vrf, const sai_ip_address_t& ip) { act(req("backend_host_erase", vrf, ip_arg(ip))); }
void backend_l2_insert(const uint8_t mac[6], uint16_t vid, int dest) { act(req("backend_l2_insert", Mac{mac}, vid, dest)); }
void backend_l2_erase(const uint8_t mac[6], uint16_t vid) { act(req("backend_l2_erase", Mac{mac}, vid)); }
int drop_egress() { return value<int>("drop_egress", -1); }
sai_status_t backend_egress_destroy(int egress)
{
    std::vector<Rec> r;
    if (!one(req("backend_egress_destroy", egress), r)) return SAI_STATUS_FAILURE;
    sai_status_t st = r[0].num<int32_t>("status");
    return done(r[0], "backend_egress_destroy") ? st : SAI_STATUS_FAILURE;
}
int lag_dest(int tid) { return value<int>(req("lag_dest", tid), -1); }

// ================================================================ virtual backend: state
size_t count(Table t) { return value<size_t>(req("count", table_name(t)), 0); }

std::vector<Port> ports()
{
    return records("ports", [](Rec& r) {
        Port p;
        read_port(r, p);
        return p;
    });
}
Port port(int hw)
{
    Port p;
    std::string q = req("port", hw);
    std::vector<Rec> r;
    Got g = ask(q, &r);
    if (g == Got::Absent) unanswered().note("port", "no such backend port " + std::to_string(hw));
    if (g != Got::Ok) return p;
    if (r.size() != 1) {
        unanswered().note("port", std::to_string(r.size()) + " records, expected 1");
        return p;
    }
    Port v;
    read_port(r[0], v);
    return done(r[0], q) ? v : p;
}
void port_forget_quirks(int hw) { act(req("port_forget_quirks", hw)); }
uint32_t l2_age_seconds() { return value<uint32_t>("l2_age_seconds", 0); }
bool vlan_exists(uint16_t vid) { return flag(req("vlan_exists", vid)); }
std::vector<VlanMember> vlan_members(uint16_t vid)
{
    return records(req("vlan_members", vid), [](Rec& r) { return VlanMember{r.num<int>("hw"), r.flag("untagged")}; });
}
std::vector<L2Entry> l2_entries()
{
    return records("l2_entries", [](Rec& r) {
        L2Entry e{};
        r.mac("mac", e.mac);
        e.vid = r.num<uint16_t>("vid");
        e.dest = r.num<int>("dest");
        e.is_static = r.flag("static");
        e.discard = r.flag("discard");
        return e;
    });
}
std::vector<Netif> netifs()
{
    return records("netifs", [](Rec& r) {
        Netif n{};
        n.id = r.num<int>("id");
        n.name = r.str("name");
        n.hw = r.num<int>("hw");
        n.vid = r.num<uint16_t>("vid");
        r.mac("mac", n.mac);
        n.keep_tag = r.flag("keep_tag");
        n.carrier = r.flag("carrier");
        n.filter = r.num<int>("filter");
        return n;
    });
}
bool trap_to_cpu(int32_t sai_trap_type) { return flag(req("trap_to_cpu", sai_trap_type)); }
std::vector<L3Intf> l3_intfs()
{
    return records("l3_intfs", [](Rec& r) {
        L3Intf i{};
        i.id = r.num<int>("id");
        r.mac("mac", i.mac);
        i.vid = r.num<uint16_t>("vid");
        i.mtu = r.num<uint32_t>("mtu");
        i.vrf = r.num<int>("vrf");
        return i;
    });
}
std::vector<Egress> egress_objects()
{
    return records("egress_objects", [](Rec& r) {
        Egress e;
        read_egress(r, e);
        return e;
    });
}
bool egress(int id, Egress& out) { return record(req("egress", id), out, read_egress); }
std::vector<Host> hosts()
{
    return records("hosts", [](Rec& r) { return Host{r.num<int>("vrf"), r.ip("ip"), r.num<int>("egress")}; });
}
int host_egress(int vrf, const sai_ip_address_t& ip) { return value<int>(req("host_egress", vrf, ip_arg(ip)), -1); }
std::vector<Route> routes()
{
    return records("routes", [](Rec& r) { return Route{r.num<int>("vrf"), r.ip("addr"), r.ip("mask"), r.num<int>("target")}; });
}
int route_target(int vrf, const sai_ip_prefix_t& prefix)
{
    sai_ip_address_t addr{}, mask{};
    addr.addr_family = mask.addr_family = prefix.addr_family;
    if (prefix.addr_family == SAI_IP_ADDR_FAMILY_IPV6) {
        std::memcpy(addr.addr.ip6, prefix.addr.ip6, 16);
        std::memcpy(mask.addr.ip6, prefix.mask.ip6, 16);
    } else {
        addr.addr.ip4 = prefix.addr.ip4;
        mask.addr.ip4 = prefix.mask.ip4;
    }
    return value<int>(req("route_target", vrf, ip_arg(addr), ip_arg(mask)), -1);
}
std::vector<Ecmp> ecmp_groups()
{
    return records("ecmp_groups", [](Rec& r) { return Ecmp{r.num<int>("id"), r.nums<int>("egress")}; });
}
bool lag_members(int tid, std::vector<LagMember>& out)
{
    std::string q = req("lag_members", tid);
    std::vector<Rec> recs;
    if (ask(q, &recs) != Got::Ok) return false;
    std::vector<LagMember> v;
    for (Rec& r : recs) {
        v.push_back({r.num<int>("hw"), r.flag("egress_disable"), r.flag("ingress_disable")});
        if (!done(r, q)) return false;
    }
    out = std::move(v);
    return true;
}
bool fp_group(int gid, FpGroup& out)
{
    return record(req("fp_group", gid), out, [](Rec& r, FpGroup& g) {
        g.qset = r.num<uint32_t>("qset");
        g.priority = r.num<int>("priority");
    });
}
std::vector<FpEntry> fp_entries()
{
    return records("fp_entries", [](Rec& r) {
        FpEntry e;
        read_fp_entry(r, e);
        return e;
    });
}
bool fp_entry(int eid, FpEntry& out) { return eid >= 0 && record(req("fp_entry", eid), out, read_fp_entry); }
bool policer(int pid, Policer& out)
{
    return record(req("policer", pid), out, [](Rec& r, Policer& p) {
        p.tr_tcm = r.flag("tr_tcm");
        p.packets = r.flag("packets");
        p.color_blind = r.flag("color_blind");
        p.cir = r.num<uint64_t>("cir");
        p.cbs = r.num<uint64_t>("cbs");
        p.pir = r.num<uint64_t>("pir");
        p.pbs = r.num<uint64_t>("pbs");
    });
}
bool qos_map(int id, QosMap& out)
{
    return record(req("qos_map", id), out, [](Rec& r, QosMap& m) {
        m.type = r.num<int32_t>("type");
        m.entries = r.qos_entries("entries");
    });
}
int port_qos_map(int hw, int32_t sai_qos_map_type) { return value<int>(req("port_qos_map", hw, sai_qos_map_type), -1); }
int port_default_tc(int hw) { return value<int>(req("port_default_tc", hw), -1); }
bool port_pfc(int hw, Pfc& out)
{
    return record(req("port_pfc", hw), out, [](Rec& r, Pfc& p) {
        p.separate = r.flag("separate");
        p.tx = r.num<uint8_t>("tx");
        p.rx = r.num<uint8_t>("rx");
    });
}
bool queue_scheduler(int hw, int queue, Sched& out) { return record(req("queue_scheduler", hw, queue), out, read_sched); }
bool port_scheduler(int hw, Sched& out) { return record(req("port_scheduler", hw), out, read_sched); }
bool wred(int id, Wred& out)
{
    return record(req("wred", id), out, [](Rec& r, Wred& w) {
        r.fixed("enable", w.enable, [](std::string_view s, bool& b) { return parse_bool(s, b); });
        r.fixed("min", w.min, [](std::string_view s, uint32_t& x) { return parse_int(s, x); });
        r.fixed("max", w.max, [](std::string_view s, uint32_t& x) { return parse_int(s, x); });
        r.fixed("prob", w.prob, [](std::string_view s, uint32_t& x) { return parse_int(s, x); });
        w.weight = r.num<uint8_t>("weight");
        w.ecn = r.num<int32_t>("ecn");
    });
}
int queue_wred(int hw, int queue) { return value<int>(req("queue_wred", hw, queue), -1); }
bool queue_buffer(int hw, int queue, Buffer& out) { return record(req("queue_buffer", hw, queue), out, read_buffer); }
bool pg_buffer(int hw, int pg, Buffer& out) { return record(req("pg_buffer", hw, pg), out, read_buffer); }
bool mirror(int id, Mirror& out)
{
    return record(req("mirror", id), out, [](Rec& r, Mirror& m) {
        m.erspan = r.flag("erspan");
        m.monitor = r.num<int>("monitor");
        m.truncate = r.num<uint16_t>("truncate");
        m.sample_rate = r.num<uint32_t>("sample_rate");
        m.tos = r.num<uint8_t>("tos");
        m.ttl = r.num<uint8_t>("ttl");
        m.src = r.ip("src");
        m.dst = r.ip("dst");
        r.mac("smac", m.smac);
        r.mac("dmac", m.dmac);
        m.gre_type = r.num<uint16_t>("gre_type");
        m.vlan = r.flag("vlan");
        m.vlan_tpid = r.num<uint16_t>("vlan_tpid");
        m.vlan_id = r.num<uint16_t>("vlan_id");
        m.vlan_pri = r.num<uint8_t>("vlan_pri");
        m.vlan_cfi = r.num<uint8_t>("vlan_cfi");
    });
}
std::vector<int> port_mirrors(int hw, bool ingress)
{
    std::string q = req("port_mirrors", hw, ingress);
    std::vector<Rec> r;
    if (!one(q, r)) return {};
    std::vector<int> v = r[0].nums<int>("value");
    return done(r[0], q) ? v : std::vector<int>();
}
uint32_t sample_rate(int id) { return value<uint32_t>(req("sample_rate", id), 0); }
int port_sample(int hw, bool ingress) { return value<int>(req("port_sample", hw, ingress), -1); }
bool genetlink(int id, Genl& out)
{
    return record(req("genetlink", id), out, [](Rec& r, Genl& g) {
        g.name = r.str("name");
        g.mcgrp = r.str("mcgrp");
        g.traps = r.nums<int32_t>("traps");
    });
}
bool hash(bool lag, Hash& out)
{
    return record(req("hash", lag), out, [](Rec& r, Hash& h) {
        h.fields = r.nums<int32_t>("fields");
        h.seed = r.num<uint32_t>("seed");
    });
}
int port_storm(int hw, sai_attr_id_t kind) { return value<int>(req("port_storm", hw, kind), -1); }
int vlan_flood(uint16_t vid, sai_attr_id_t kind) { return value<int>(req("vlan_flood", vid, kind), -1); }

} // namespace tp
