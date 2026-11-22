// SPDX-License-Identifier: Apache-2.0
// External MACsec table (SAI_OPENBCM_EXT_MACSEC_FILE, src/ext_macsec.h) and
// the SAI MACsec objects on top of it (src/sai_macsec.cpp): parser, sequence
// rendering against a golden list, the object model as SONiC's MACsecOrch
// drives it (SAs before the flow is bound, gate, rekey, counters, PN), the
// inter-frame gap, clean-up of what a previous process left, and that key
// bytes reach the bus and nothing else. Included by test_sai.cpp; leaves a
// default switch. Uses the helpers of test_extphy.inc.

static sai_macsec_api_t* ma;   // (aa: the ACL API, test_acl.inc)

// A made-up PHY: one register per thing, so the bus log reads like a story.
//   1.100 / 1.101   engine bypass (egress / ingress), 1.110 / 1.111 "reset"
//   1.120 / 1.121   gate (1 = open)
//   1.2d0..         SA install, d = direction (0 egress, 1 ingress): 2d0 = marker (first<<8 | cipher<<4 | an),
//                   2d1.. key / hash key / sci / pn / window / iv bytes
//   1.300 + an      SA removal (bit 15: "last"), ingress: read-modify-write of 1.310
//   1.4xx           counters, 1.500.. PN
static std::string ms_table(bool gates = true, bool probe = true)
{
    std::string t = "ext-macsec 1\n# test table\ncounters clear-on-read\nifg-bits 384\nport 1 10\nport 5 11\n";
    const char* dirs[2] = {"egress", "ingress"};
    const char* ciphers[4] = {"gcm-aes-128", "gcm-aes-256", "gcm-aes-xpn-128", "gcm-aes-xpn-256"};
    char b[256];
    for (int d = 0; d < 2; d++) {
        std::string dn = dirs[d];
        if (probe) t += "seq probe." + dn + "\nX p 1 10" + std::to_string(d) + " bypass[0]\n";
        t += "seq reset." + dn + "\nW p 1 11" + std::to_string(d) + " 1\n";
        t += "seq enable." + dn + "\nW p 1 10" + std::to_string(d) + " 0\nD 10\n";
        t += "seq disable." + dn + "\nW p 1 10" + std::to_string(d) + " 1\n";
        if (gates) {
            t += "seq gate-open." + dn + "\nW p 1 12" + std::to_string(d) + " 1\n";
            t += "seq gate-close." + dn + "\nW p 1 12" + std::to_string(d) + " 0\n";
        }
        t += "seq stats-flow." + dn + "\nX p 1 4f0 control_pkts[0]\nX p 1 4f1 control_pkts[1]\nX p 1 4f2 x_unknown[0]\n";
        for (int an = 0; an < 4; an++) {
            for (int c = 0; c < 4; c++) {
                bool xpn = c >= 2, k256 = c & 1;
                for (int first = 0; first < 2; first++) {
                    std::snprintf(b, sizeof(b), "seq sa-add%s.%s.%d.%s\nW p 1 2%d0 %x\n", first ? "-first" : "", dirs[d], an, ciphers[c], d,
                                  (first << 8) | (c << 4) | an);
                    t += b;
                    std::snprintf(b, sizeof(b), "W p 1 2%d1 key[1].key[0]\nW p 1 2%d2 key[%d].key[%d]\nW p 1 2%d3 hkey[15].hkey[0]\n", d, d,
                                  k256 ? 31 : 15, k256 ? 30 : 14, d);
                    t += b;
                    std::snprintf(b, sizeof(b), "W p 1 2%d4 sci[0].sci[7]\nW p 1 2%d5 pn[1].pn[0]\nW p 1 2%d6 %s\nW p 1 2%d7 window[1].window[0]\n", d, d, d,
                                  xpn ? "pn[5].pn[4]" : "0", d);
                    t += b;
                    if (xpn) {
                        std::snprintf(b, sizeof(b), "W p 1 2%d8 iv[0].iv[3]\nW p 1 2%d9 iv[4].iv[11]\nW p 1 2%da ssci[0].salt[0]\n", d, d, d);
                        t += b;
                    }
                    if (d == 1) t += "X p 1 310 $0\nW p 1 310 $0&ff00|" + std::string(1, (char)('1' + an)) + "\n";
                    std::snprintf(b, sizeof(b), "P p 1 2%df 1 1 3 0\n", d);
                    t += b;
                }
            }
            if (d == 1 && an == 0) t += "seq sa-add-first.ingress.0.gcm-aes-128.replay\nW p 1 210 f00\nW p 1 217 window[1].window[0]\nP p 1 21f 1 1 3 0\n";
            std::snprintf(b, sizeof(b), "seq sa-del.%s.%d\nW p 1 3%d%d 1\nseq sa-del-last.%s.%d\nW p 1 3%d%d 8001\n", dirs[d], an, d, an, dirs[d], an, d, an);
            t += b;
            std::snprintf(b, sizeof(b), "seq pn.%s.%d\nX p 1 5%d%d pn[0]\nX p 1 5%d%d pn[1]\nX p 1 5%d%d pn[2]\nX p 1 5%d%d pn[3]\n", dirs[d], an, d, an * 4,
                          d, an * 4 + 1, d, an * 4 + 2, d, an * 4 + 3);
            t += b;
            std::snprintf(b, sizeof(b), "seq stats-sa.%s.%d\nX p 1 4%d%d %s[0]\nX p 1 4%d%d %s[1]\n", dirs[d], an, d, an * 2,
                          d ? "in_pkts_ok" : "out_pkts_encrypted", d, an * 2 + 1, d ? "in_pkts_ok" : "out_pkts_encrypted");
            t += b;
        }
    }
    return t;
}

struct MsBus { std::vector<std::string> ops; };
static std::vector<std::string> ms_log_take()
{
    std::lock_guard<std::mutex> g(fake().mdio_mu);
    std::vector<std::string> v;
    char b[64];
    for (auto& o : fake().mdio_log) {
        std::snprintf(b, sizeof(b), "%c %x %x.%03x %x", o.op, o.phy, o.dev, o.reg, o.op == 'W' ? o.val : 0);
        v.push_back(b);
    }
    fake().mdio_log.clear();
    return v;
}
static std::vector<std::string> ms_writes()   // "reg=val" of the writes since the last call
{
    std::vector<std::string> v;
    for (auto& s : ms_log_take())
        if (s[0] == 'W') v.push_back(s.substr(s.find('.') + 1).replace(3, 1, "="));
    return v;
}
static std::string ms_join(const std::vector<std::string>& v)
{
    std::string s;
    for (auto& x : v) s += (s.empty() ? "" : " ") + x;
    return s;
}
#define MS_BUS(expect)                                                                 \
    do {                                                                               \
        std::string got = ms_join(ms_writes());                                        \
        g_checks++;                                                                    \
        if (got != (expect)) {                                                         \
            g_fail++;                                                                  \
            std::fprintf(stderr, "FAIL %s:%d: bus writes\n   got  %s\n   want %s\n", __FILE__, __LINE__, got.c_str(), std::string(expect).c_str()); \
        }                                                                              \
    } while (0)

static sai_object_id_t ms_create(sai_status_t (*fn)(sai_object_id_t*, sai_object_id_t, uint32_t, const sai_attribute_t*),
                                 const std::vector<sai_attribute_t>& attrs, sai_status_t want = SAI_STATUS_SUCCESS)
{
    sai_object_id_t id = 0;
    CHECK_EQ(fn(&id, g_sw, (uint32_t)attrs.size(), attrs.data()), want);
    return id;
}
static sai_attribute_t ms_s32(sai_attr_id_t id, int32_t v) { sai_attribute_t a; std::memset(&a, 0, sizeof(a)); a.id = id; a.value.s32 = v; return a; }
static sai_attribute_t ms_oid(sai_attr_id_t id, sai_object_id_t v) { sai_attribute_t a; std::memset(&a, 0, sizeof(a)); a.id = id; a.value.oid = v; return a; }
static sai_attribute_t ms_bool(sai_attr_id_t id, bool v) { sai_attribute_t a; std::memset(&a, 0, sizeof(a)); a.id = id; a.value.booldata = v; return a; }
static sai_attribute_t ms_u64(sai_attr_id_t id, uint64_t v) { sai_attribute_t a; std::memset(&a, 0, sizeof(a)); a.id = id; a.value.u64 = v; return a; }
static sai_attribute_t ms_u32(sai_attr_id_t id, uint32_t v) { sai_attribute_t a; std::memset(&a, 0, sizeof(a)); a.id = id; a.value.u32 = v; return a; }
static sai_attribute_t ms_u8(sai_attr_id_t id, uint8_t v) { sai_attribute_t a; std::memset(&a, 0, sizeof(a)); a.id = id; a.value.u8 = v; return a; }

// The test key: bytes 0xc0.. so they are easy to find where they must not be.
static std::vector<sai_attribute_t> ms_sa_attrs(int dir, sai_object_id_t sc, uint8_t an, bool k256, bool xpn, uint64_t pn)
{
    std::vector<sai_attribute_t> v = {ms_s32(SAI_MACSEC_SA_ATTR_MACSEC_DIRECTION, dir), ms_oid(SAI_MACSEC_SA_ATTR_SC_ID, sc), ms_u8(SAI_MACSEC_SA_ATTR_AN, an),
                                      ms_u64(dir == SAI_MACSEC_DIRECTION_EGRESS ? SAI_MACSEC_SA_ATTR_CONFIGURED_EGRESS_XPN : SAI_MACSEC_SA_ATTR_MINIMUM_INGRESS_XPN, pn)};
    sai_attribute_t a;
    std::memset(&a, 0, sizeof(a));
    a.id = SAI_MACSEC_SA_ATTR_SAK;
    for (int i = 0; i < 32; i++) a.value.macsecsak[i] = (k256 || i >= 16) ? (uint8_t)(0xc0 + (k256 ? i : i - 16) + an) : 0;
    v.push_back(a);
    std::memset(&a, 0, sizeof(a));
    a.id = SAI_MACSEC_SA_ATTR_AUTH_KEY;
    for (int i = 0; i < 16; i++) a.value.macsecauthkey[i] = (uint8_t)(0x80 + i);
    v.push_back(a);
    if (xpn) {
        std::memset(&a, 0, sizeof(a));
        a.id = SAI_MACSEC_SA_ATTR_SALT;
        for (int i = 0; i < 12; i++) a.value.macsecsalt[i] = (uint8_t)(0x50 + i);
        v.push_back(a);
        v.push_back(ms_u32(SAI_MACSEC_SA_ATTR_MACSEC_SSCI, 0x01020304));
    }
    return v;
}
static sai_attribute_t ms_action(sai_attr_id_t id, bool enable, int32_t s32, sai_object_id_t oid = 0)
{
    sai_attribute_t a;
    std::memset(&a, 0, sizeof(a));
    a.id = id;
    a.value.aclaction.enable = enable;
    if (oid) a.value.aclaction.parameter.oid = oid;
    else a.value.aclaction.parameter.s32 = s32;
    return a;
}
static uint64_t ms_stat(sai_object_type_t t, sai_object_id_t id, sai_stat_id_t s)
{
    uint64_t v = ~0ull;
    if (t == SAI_OBJECT_TYPE_MACSEC_SA) OK(ma->get_macsec_sa_stats(id, 1, &s, &v));
    else OK(ma->get_macsec_flow_stats(id, 1, &s, &v));
    return v;
}

static void test_macsec()
{
    // ---- parser
    MsTable t;
    std::string err;
    CHECK(ext_macsec_parse(ms_table(), t, err));
    CHECK_EQ(t.ports.size(), 2u);
    CHECK_EQ(t.ports[5], 0x11u);
    CHECK_EQ(t.ifg_bits, 384u);
    CHECK(t.clear_on_read);
    CHECK(t.find("sa-add-first.ingress.3.gcm-aes-xpn-256") != nullptr);
    CHECK(t.find("sa-add.ingress.3.gcm-aes-xpn-512") == nullptr);
    CHECK(ext_macsec_parse("ext-macsec 1\n", t, err));   // empty: valid
    CHECK(ext_macsec_parse("ext-macsec 1\ncounters absolute\nseq a\nW 10 1 a000 $0|1\n", t, err) == false);   // $0 never read
    struct { const char* text; const char* in_err; } bad[] = {
        {"", "line 1"},
        {"ext-phy 1\n", "line 1"},
        {"ext-macsec 2\n", "line 1"},
        {"ext-macsec 1\nW p 1 a000 6\n", "outside a sequence"},
        {"ext-macsec 1\nseq a\nW p 0 a000 6\n", "clause 22"},
        {"ext-macsec 1\nseq a\nW p 20 a000 6\n", "line 3"},
        {"ext-macsec 1\nseq a\nW q 1 a000 6\n", "phy"},
        {"ext-macsec 1\nseq a\nW p 1 a000 10000\n", "value"},
        {"ext-macsec 1\nseq a\nW p 1 a000 key[32].00\n", "value"},        // beyond the longest key
        {"ext-macsec 1\nseq a\nW p 1 a000 hkey[16].00\n", "value"},
        {"ext-macsec 1\nseq a\nW p 1 a000 secret[0].00\n", "value"},      // unknown parameter
        {"ext-macsec 1\nseq a\nW p 1 a000 0.key[0]\n", "value"},          // a byte is two hex digits
        {"ext-macsec 1\nseq a\nW p 1 a000 $16\n", "value"},
        {"ext-macsec 1\nseq a\nW p 1 a000 $0\n", "before an X"},
        {"ext-macsec 1\nseq a\nX p 1 a000 $16\n", "destination"},
        {"ext-macsec 1\nseq a\nX p 1 a000 pn[8]\n", "destination"},
        {"ext-macsec 1\nseq a\nX p 1 a000 pn\n", "destination"},
        {"ext-macsec 1\nseq a\nE p 1 a000 3 1\n", "outside the mask"},
        {"ext-macsec 1\nseq a\nP p 1 a000 1 1 0 0\n", "tries"},
        {"ext-macsec 1\nseq a\nD 200000\n", "D <usec"},
        {"ext-macsec 1\nseq a\nC p 1 a000 1 1\n", "unknown keyword"},
        {"ext-macsec 1\nseq a\nseq a\n", "twice"},
        {"ext-macsec 1\nseq A\n", "seq <name>"},
        {"ext-macsec 1\nport 1 10\nport 1 11\n", "twice"},
        {"ext-macsec 1\nport 1\n", "port <first lane"},
        {"ext-macsec 1\ncounters sometimes\n", "counters"},
        {"ext-macsec 1\nifg-bits 9999\n", "ifg-bits"},
    };
    for (auto& b : bad) {
        err.clear();
        CHECK(!ext_macsec_parse(b.text, t, err));
        if (err.find(b.in_err) == std::string::npos) {
            g_fail++;
            std::fprintf(stderr, "FAIL ext-macsec parse: '%s' gave '%s', wanted '%s' in it\n", b.text, err.c_str(), b.in_err);
        }
        g_checks++;
    }

    // ---- executor against a golden rendering (no switch needed)
    {
        CHECK(ext_macsec_parse("ext-macsec 1\nseq s\nR p 1 900a\nW p 1 a000 key[1].key[0]\nW 22 1 a001 00.sci[7]\nX p 1 9005 $3\nX p 1 9005 $4\n"
                               "W p 1 9005 $3&7fff\nW p 1 9005 $4|8002\nD 7\nE p 1 a100 2 f\nP p 1 8221 12e3 ffff 3 5\nX p 1 a101 pn[0]\nX p 1 a102 pn[2]\n", t, err));
        std::vector<std::string> log;
        std::map<unsigned, std::vector<unsigned>> answers = {{0x9005, {0x8001, 0x0010}}, {0xa100, {0x12}}, {0x8221, {0, 0x12e3}}, {0xa101, {0xbeef}}, {0xa102, {0x1}}};
        MsIo io;
        char b[64];
        io.rd = [&](unsigned p, unsigned d, unsigned r, unsigned& v, const MsOp&) {
            auto& q = answers[r];
            v = q.empty() ? 0 : q.front();
            if (!q.empty()) q.erase(q.begin());
            std::snprintf(b, sizeof(b), "R %x %x.%04x", p, d, r);
            log.push_back(b);
            return SAI_STATUS_SUCCESS;
        };
        io.wr = [&](unsigned p, unsigned d, unsigned r, unsigned v) {
            std::snprintf(b, sizeof(b), "W %x %x.%04x %04x", p, d, r, v);
            log.push_back(b);
            return SAI_STATUS_SUCCESS;
        };
        io.delay = [&](unsigned us) { log.push_back("D " + std::to_string(us)); };
        MsParams par = {{"key", {0x11, 0x22}}, {"sci", {1, 2, 3, 4, 5, 6, 7, 8}}};
        MsResult res;
        OK(ext_macsec_exec(io, *t.find("s"), 0x30, par, &res, err));
        const char* golden = "R 30 1.900a|W 30 1.a000 2211|W 22 1.a001 0008|R 30 1.9005|R 30 1.9005|W 30 1.9005 0001|W 30 1.9005 8012|D 7|R 30 1.a100|"
                             "R 30 1.8221|D 5|R 30 1.8221|R 30 1.a101|R 30 1.a102";
        std::string got;
        for (auto& l : log) got += (got.empty() ? "" : "|") + l;
        CHECK(got == golden);
        CHECK_EQ(ms_result_u64(res, "pn"), 0x10000beefull);
        // a missing parameter byte: nothing is accessed
        log.clear();
        par["key"] = {0x11};
        CHECK_EQ(ext_macsec_exec(io, *t.find("s"), 0x30, par, &res, err), SAI_STATUS_INVALID_PARAMETER);
        CHECK(log.empty());
        CHECK(err.find("key[1]") != std::string::npos);
        // a compare that fails stops the sequence there and names the line, not a value of ours
        par["key"] = {0x11, 0x22};
        answers = {{0xa100, {0x13}}};
        log.clear();
        CHECK_EQ(ext_macsec_exec(io, *t.find("s"), 0x30, par, &res, err), SAI_STATUS_FAILURE);
        CHECK(log.back() == "R 30 1.a100");
        CHECK(err.find("line 11") != std::string::npos);
        ms_wipe(par);
        CHECK(par.empty());
    }

    // ---- no table: the API is there and says so
    OK(sai_api_query(SAI_API_MACSEC, (void**)&ma));
    ms_create(ma->create_macsec, {ms_s32(SAI_MACSEC_ATTR_DIRECTION, SAI_MACSEC_DIRECTION_EGRESS)}, SAI_STATUS_NOT_SUPPORTED);

    // ---- switch creation: missing file, bad table, unknown lane
    OK(sw->remove_switch(g_sw));
    g_profile["SAI_OPENBCM_FAKE_PORTS"] = "4";
    g_profile["SAI_OPENBCM_FAKE_LANES"] = "4";
    g_profile["SAI_OPENBCM_FAKE_SPEED"] = "100000";
    g_profile["SAI_OPENBCM_PORT_IFG"] = "128";
    g_profile["SAI_OPENBCM_EXT_MACSEC_FILE"] = "/nonexistent/ext-macsec.tbl";
    CHECK_EQ(create_switch_default(), SAI_STATUS_FAILURE);
    std::string f_bad = xp_write("ext-macsec 1\nseq a\nW p 0 1 1\n");
    g_profile["SAI_OPENBCM_EXT_MACSEC_FILE"] = f_bad;
    CHECK_EQ(create_switch_default(), SAI_STATUS_INVALID_PARAMETER);
    std::string f_lane = xp_write("ext-macsec 1\nport 3 10\n");
    g_profile["SAI_OPENBCM_EXT_MACSEC_FILE"] = f_lane;
    CHECK_EQ(create_switch_default(), SAI_STATUS_INVALID_PARAMETER);
    CHECK(!ctx().switch_created);
    CHECK(!ext_macsec_active());

    // ---- the good table. Port 0 = lane 1 = PHY 0x10, port 1 = lane 5 = PHY 0x11.
    std::string f_good = xp_write(ms_table());
    g_profile["SAI_OPENBCM_EXT_MACSEC_FILE"] = f_good;
    OK(create_switch_default());
    CHECK(ext_macsec_active());
    // every engine read 0 = "not in bypass" (the fake's registers start at 0): all four were reset and disabled
    MS_BUS("110=1 100=1 111=1 101=1 110=1 100=1 111=1 101=1");
    sai_object_id_t ports[64];
    sai_attribute_t a;
    a.id = SAI_SWITCH_ATTR_PORT_LIST;
    a.value.objlist.count = 64;
    a.value.objlist.list = ports;
    OK(sw->get_switch_attribute(g_sw, 1, &a));
    sai_object_id_t p0 = ports[0], p2 = ports[2];
    int hw0 = ctx().ports[p0].hw;
    CHECK_EQ(fake().port_state[hw0].ifg, 128u);
    for (unsigned phy : {0x10u, 0x11u})   // the SA sequences poll 1.2df for 1
        for (unsigned r : {0x20fu, 0x21fu}) xp_reg(phy, r, 1);

    // MACSEC objects and what MACsecOrch asks them
    sai_object_id_t m_e = ms_create(ma->create_macsec, {ms_s32(SAI_MACSEC_ATTR_DIRECTION, SAI_MACSEC_DIRECTION_EGRESS), ms_bool(SAI_MACSEC_ATTR_PHYSICAL_BYPASS_ENABLE, true)});
    sai_object_id_t m_i = ms_create(ma->create_macsec, {ms_s32(SAI_MACSEC_ATTR_DIRECTION, SAI_MACSEC_DIRECTION_INGRESS), ms_bool(SAI_MACSEC_ATTR_PHYSICAL_BYPASS_ENABLE, true)});
    ms_create(ma->create_macsec, {ms_s32(SAI_MACSEC_ATTR_DIRECTION, SAI_MACSEC_DIRECTION_INGRESS)}, SAI_STATUS_ITEM_ALREADY_EXISTS);
    ms_create(ma->create_macsec, {ms_s32(SAI_MACSEC_ATTR_DIRECTION, SAI_MACSEC_DIRECTION_INGRESS), ms_bool(SAI_MACSEC_ATTR_ENABLE_POST, true)}, SAI_STATUS_ATTR_NOT_SUPPORTED_0 - 1);
    a.id = SAI_MACSEC_ATTR_SCI_IN_INGRESS_MACSEC_ACL;
    OK(ma->get_macsec_attribute(m_i, 1, &a));
    CHECK(!a.value.booldata);
    a.id = SAI_MACSEC_ATTR_MAX_SECURE_ASSOCIATIONS_PER_SC;
    OK(ma->get_macsec_attribute(m_e, 1, &a));
    CHECK_EQ(a.value.s32, SAI_MACSEC_MAX_SECURE_ASSOCIATIONS_PER_SC_FOUR);
    a.id = SAI_MACSEC_ATTR_XPN_64BIT_SUPPORTED;
    OK(ma->get_macsec_attribute(m_e, 1, &a));
    CHECK(a.value.booldata);

    // MACSEC_PORT: the engine comes out of bypass (after a reset), the inter-frame gap widens
    const int32_t EGR = SAI_MACSEC_DIRECTION_EGRESS, ING = SAI_MACSEC_DIRECTION_INGRESS;
    ms_create(ma->create_macsec_port, {ms_s32(SAI_MACSEC_PORT_ATTR_MACSEC_DIRECTION, EGR), ms_oid(SAI_MACSEC_PORT_ATTR_PORT_ID, p2)}, SAI_STATUS_NOT_SUPPORTED);   // not in the table
    int qc0 = fake().port_state[hw0].quirk_calls;
    sai_object_id_t mp_e = ms_create(ma->create_macsec_port, {ms_s32(SAI_MACSEC_PORT_ATTR_MACSEC_DIRECTION, EGR), ms_oid(SAI_MACSEC_PORT_ATTR_PORT_ID, p0)});
    MS_BUS("110=1 100=0");
    CHECK_EQ(fake().port_state[hw0].ifg, 384u);
    CHECK_EQ(fake().port_state[hw0].quirk_calls, qc0 + 1);   // the gap alone: no interface / autoneg re-set (a link flap on a live port)
    sai_object_id_t mp_i = ms_create(ma->create_macsec_port, {ms_s32(SAI_MACSEC_PORT_ATTR_MACSEC_DIRECTION, ING), ms_oid(SAI_MACSEC_PORT_ATTR_PORT_ID, p0)});
    MS_BUS("111=1 101=0");
    ms_create(ma->create_macsec_port, {ms_s32(SAI_MACSEC_PORT_ATTR_MACSEC_DIRECTION, ING), ms_oid(SAI_MACSEC_PORT_ATTR_PORT_ID, p0)}, SAI_STATUS_ITEM_ALREADY_EXISTS);
    // the gap survives what re-applies the port quirks (a speed set)
    a.id = SAI_PORT_ATTR_SPEED;
    a.value.u32 = 100000;
    OK(pa->set_port_attribute(p0, &a));
    CHECK_EQ(fake().port_state[hw0].ifg, 384u);
    ms_log_take();

    // flows, the MACsec ACL tables with MACsecOrch's entries, bound to the port: nothing reaches the ASIC or the PHY
    sai_object_id_t f_e = ms_create(ma->create_macsec_flow, {ms_s32(SAI_MACSEC_FLOW_ATTR_MACSEC_DIRECTION, EGR)});
    sai_object_id_t f_i = ms_create(ma->create_macsec_flow, {ms_s32(SAI_MACSEC_FLOW_ATTR_MACSEC_DIRECTION, ING)});
    size_t fp_before = fake().fp_groups.size();
    int32_t bind = SAI_ACL_BIND_POINT_TYPE_PORT;
    sai_object_id_t tbl[2], eapol[2], data[2];
    for (int d = 0; d < 2; d++) {
        sai_attribute_t bl;
        std::memset(&bl, 0, sizeof(bl));
        bl.id = SAI_ACL_TABLE_ATTR_ACL_BIND_POINT_TYPE_LIST;
        bl.value.s32list.count = 1;
        bl.value.s32list.list = &bind;
        tbl[d] = ms_create(aa->create_acl_table, {ms_s32(SAI_ACL_TABLE_ATTR_ACL_STAGE, d ? SAI_ACL_STAGE_INGRESS_MACSEC : SAI_ACL_STAGE_EGRESS_MACSEC), bl,
                                                  ms_bool(SAI_ACL_TABLE_ATTR_FIELD_DST_MAC, true), ms_bool(SAI_ACL_TABLE_ATTR_FIELD_ETHER_TYPE, true)});
        sai_attribute_t et;
        std::memset(&et, 0, sizeof(et));
        et.id = SAI_ACL_ENTRY_ATTR_FIELD_ETHER_TYPE;
        et.value.aclfield.enable = true;
        et.value.aclfield.data.u16 = 0x888e;
        et.value.aclfield.mask.u16 = 0xffff;
        eapol[d] = ms_create(aa->create_acl_entry, {ms_oid(SAI_ACL_ENTRY_ATTR_TABLE_ID, tbl[d]), ms_u32(SAI_ACL_ENTRY_ATTR_PRIORITY, 1000), ms_bool(SAI_ACL_ENTRY_ATTR_ADMIN_STATE, true),
                                                    et, ms_action(SAI_ACL_ENTRY_ATTR_ACTION_PACKET_ACTION, true, SAI_PACKET_ACTION_FORWARD)});
        a = ms_oid(d ? SAI_PORT_ATTR_INGRESS_MACSEC_ACL : SAI_PORT_ATTR_EGRESS_MACSEC_ACL, tbl[d]);
        OK(pa->set_port_attribute(p0, &a));
        a.id = d ? SAI_PORT_ATTR_INGRESS_MACSEC_ACL : SAI_PORT_ATTR_EGRESS_MACSEC_ACL;
        a.value.oid = 0;
        OK(pa->get_port_attribute(p0, 1, &a));
        CHECK_EQ(a.value.oid, tbl[d]);
    }
    CHECK_EQ(fake().fp_groups.size(), fp_before);
    CHECK(ms_log_take().empty());
    a = ms_oid(SAI_PORT_ATTR_EGRESS_MACSEC_ACL, tbl[1]);
    CHECK_EQ(pa->set_port_attribute(p0, &a), SAI_STATUS_INVALID_ATTR_VALUE_0);   // an ingress table on the egress side

    // SCs (the SCI as SONiC passes it: bytes in memory order), each with MACsecOrch's data entry (drop)
    uint8_t sci_b[8] = {0x1c, 0xea, 0x0b, 0x38, 0x2b, 0xd5, 0x00, 0x01};
    uint64_t sci;
    std::memcpy(&sci, sci_b, 8);
    auto sc_attrs = [&](int32_t dir, sai_object_id_t flow, int32_t cipher) {
        return std::vector<sai_attribute_t>{ms_s32(SAI_MACSEC_SC_ATTR_MACSEC_DIRECTION, dir), ms_oid(SAI_MACSEC_SC_ATTR_FLOW_ID, flow), ms_u64(SAI_MACSEC_SC_ATTR_MACSEC_SCI, sci),
                                            ms_bool(SAI_MACSEC_SC_ATTR_ENCRYPTION_ENABLE, true), ms_bool(SAI_MACSEC_SC_ATTR_MACSEC_EXPLICIT_SCI_ENABLE, true),
                                            ms_s32(SAI_MACSEC_SC_ATTR_MACSEC_CIPHER_SUITE, cipher)};
    };
    {
        auto v = sc_attrs(EGR, f_e, SAI_MACSEC_CIPHER_SUITE_GCM_AES_128);
        v[3].value.booldata = false;   // integrity only: not what the sequences do
        ms_create(ma->create_macsec_sc, v, SAI_STATUS_ATTR_NOT_SUPPORTED_0 - 3);
        ms_create(ma->create_macsec_sc, sc_attrs(EGR, f_i, SAI_MACSEC_CIPHER_SUITE_GCM_AES_128), SAI_STATUS_INVALID_PARAMETER);   // an ingress flow
    }
    sai_object_id_t sc_e = ms_create(ma->create_macsec_sc, sc_attrs(EGR, f_e, SAI_MACSEC_CIPHER_SUITE_GCM_AES_128));
    ms_create(ma->create_macsec_sc, sc_attrs(EGR, f_e, SAI_MACSEC_CIPHER_SUITE_GCM_AES_128), SAI_STATUS_INSUFFICIENT_RESOURCES);   // one SC per flow
    auto vi = sc_attrs(ING, f_i, SAI_MACSEC_CIPHER_SUITE_GCM_AES_XPN_256);
    vi.push_back(ms_bool(SAI_MACSEC_SC_ATTR_MACSEC_REPLAY_PROTECTION_ENABLE, true));
    vi.push_back(ms_u32(SAI_MACSEC_SC_ATTR_MACSEC_REPLAY_PROTECTION_WINDOW, 0x1234));
    sai_object_id_t sc_i = ms_create(ma->create_macsec_sc, vi);
    for (int d = 0; d < 2; d++)
        data[d] = ms_create(aa->create_acl_entry, {ms_oid(SAI_ACL_ENTRY_ATTR_TABLE_ID, tbl[d]), ms_u32(SAI_ACL_ENTRY_ATTR_PRIORITY, 1), ms_bool(SAI_ACL_ENTRY_ATTR_ADMIN_STATE, true),
                                                   ms_action(SAI_ACL_ENTRY_ATTR_ACTION_PACKET_ACTION, true, SAI_PACKET_ACTION_DROP)});
    CHECK(ms_log_take().empty());

    // An SA before its flow is bound to the port (MACsecOrch with the port not enabled yet): kept, nothing on the bus.
    // Everything logged from here to the end of the key tests goes to a file that is searched for key bytes.
    char logf[] = "/tmp/sai-macsec-log-XXXXXX";
    int lfd = ::mkstemp(logf), saved = ::dup(2);
    CHECK(lfd >= 0 && saved >= 0);
    std::fflush(stderr);
    ::dup2(lfd, 2);
    log_configure(true);
    sai_log_set(SAI_API_MACSEC, SAI_LOG_LEVEL_DEBUG);
    sai_log_set(SAI_API_SWITCH, SAI_LOG_LEVEL_DEBUG);
    sai_object_id_t sa_e0 = ms_create(ma->create_macsec_sa, ms_sa_attrs(EGR, sc_e, 0, false, false, 7));
    CHECK(ms_log_take().empty());
    CHECK(ext_macsec_status().find("waiting for their flow") != std::string::npos);
    a.id = SAI_MACSEC_SA_ATTR_CURRENT_XPN;
    OK(ma->get_macsec_sa_attribute(sa_e0, 1, &a));
    CHECK_EQ(a.value.u64, 7u);   // as configured: not in the PHY yet
    // The flow becomes active (setMACsecFlowActive): the SA goes in as the SC's first, then the gate opens.
    a = ms_action(SAI_ACL_ENTRY_ATTR_ACTION_MACSEC_FLOW, true, 0, f_e);
    OK(aa->set_acl_entry_attribute(data[0], &a));
    a = ms_action(SAI_ACL_ENTRY_ATTR_ACTION_PACKET_ACTION, false, SAI_PACKET_ACTION_DROP);
    OK(aa->set_acl_entry_attribute(data[0], &a));
    //         marker: first, cipher 0, AN 0 | key[1].key[0] | key[15].key[14] | hkey[15].hkey[0] | sci[0].sci[7] | pn | pn hi | window
    MS_BUS("200=100 201=c1c0 202=cfce 203=8f80 204=1c01 205=7 206=0 207=0 120=1");
    CHECK(ext_macsec_status().find("waiting") == std::string::npos);
    // rekey: AN 1 created when it becomes the encoding AN; it transmits; then the old one goes (no bus access needed to switch back)
    sai_object_id_t sa_e1 = ms_create(ma->create_macsec_sa, ms_sa_attrs(EGR, sc_e, 1, false, false, 1));
    MS_BUS("200=1 201=c2c1 202=d0cf 203=8f80 204=1c01 205=1 206=0 207=0");
    a.id = SAI_MACSEC_SC_ATTR_ACTIVE_EGRESS_SA_ID;
    OK(ma->get_macsec_sc_attribute(sc_e, 1, &a));
    CHECK_EQ(a.value.oid, sa_e1);
    ms_create(ma->create_macsec_sa, ms_sa_attrs(EGR, sc_e, 1, false, false, 1), SAI_STATUS_ITEM_ALREADY_EXISTS);
    OK(ma->remove_macsec_sa(sa_e0));
    MS_BUS("300=1");
    // counters: the PHY clears on read, the SAI value accumulates; PN from the PHY (32 bit for a non-XPN suite)
    xp_reg(0x10, 0x402, 5);
    CHECK_EQ(ms_stat(SAI_OBJECT_TYPE_MACSEC_SA, sa_e1, SAI_MACSEC_SA_STAT_OUT_PKTS_ENCRYPTED), 5u);
    xp_reg(0x10, 0x402, 3);
    xp_reg(0x10, 0x403, 1);
    CHECK_EQ(ms_stat(SAI_OBJECT_TYPE_MACSEC_SA, sa_e1, SAI_MACSEC_SA_STAT_OUT_PKTS_ENCRYPTED), 5u + 3u + 0x10000u);
    CHECK_EQ(ms_stat(SAI_OBJECT_TYPE_MACSEC_SA, sa_e1, SAI_MACSEC_SA_STAT_OCTETS_ENCRYPTED), 0u);   // not in this table
    xp_reg(0x10, 0x4f0, 9);
    CHECK_EQ(ms_stat(SAI_OBJECT_TYPE_MACSEC_FLOW, f_e, SAI_MACSEC_FLOW_STAT_CONTROL_PKTS), 9u);
    CHECK_EQ(ms_stat(SAI_OBJECT_TYPE_MACSEC_FLOW, f_e, SAI_MACSEC_FLOW_STAT_CONTROL_PKTS), 18u);
    xp_reg(0x10, 0x504, 0x2222);
    xp_reg(0x10, 0x505, 0x1111);
    xp_reg(0x10, 0x506, 0xffff);   // beyond 32 bit: ignored for gcm-aes-128
    a.id = SAI_MACSEC_SA_ATTR_CURRENT_XPN;
    OK(ma->get_macsec_sa_attribute(sa_e1, 1, &a));
    CHECK_EQ(a.value.u64, 0x11112222u);
    a.id = SAI_MACSEC_SA_ATTR_SAK;
    CHECK_EQ(ma->get_macsec_sa_attribute(sa_e1, 1, &a), SAI_STATUS_ATTR_NOT_SUPPORTED_0);   // write-only
    sai_stat_capability_t caps[64];
    sai_stat_capability_list_t cl = {64, caps};
    OK(sai_query_stats_capability(g_sw, SAI_OBJECT_TYPE_MACSEC_SA, &cl));
    CHECK_EQ(cl.count, 2u);   // out_pkts_encrypted, in_pkts_ok: what this table reads
    ms_log_take();

    // ingress, flow active first (the port enabled): XPN-256 with replay protection, the SA goes in at once
    a = ms_action(SAI_ACL_ENTRY_ATTR_ACTION_MACSEC_FLOW, true, 0, f_i);
    OK(aa->set_acl_entry_attribute(data[1], &a));
    CHECK(ms_log_take().empty());   // no SA yet: the gate stays closed
    xp_reg(0x10, 0x310, 0xab00);
    sai_object_id_t sa_i2 = ms_create(ma->create_macsec_sa, ms_sa_attrs(ING, sc_i, 2, true, true, 0x100000002ull));
    //      first, cipher 3, AN 2 | key | key[31].key[30] | hkey | sci | pn lo | pn hi | window | iv[0].iv[3] (salt ^ ssci) | iv[4].iv[11] | ssci[0].salt[0] | kept & ff00 | 3
    MS_BUS("210=132 211=c3c2 212=e1e0 213=8f80 214=1c01 215=2 216=1 217=1234 218=5157 219=545b 21a=150 310=ab03 121=1");
    sai_object_id_t sa_i3 = ms_create(ma->create_macsec_sa, ms_sa_attrs(ING, sc_i, 3, true, true, 1));
    MS_BUS("210=33 211=c4c3 212=e2e1 213=8f80 214=1c01 215=1 216=0 217=1234 218=5157 219=545b 21a=150 310=ab04");
    a = ms_s32(SAI_MACSEC_SC_ATTR_MACSEC_CIPHER_SUITE, SAI_MACSEC_CIPHER_SUITE_GCM_AES_128);
    CHECK_EQ(ma->set_macsec_sc_attribute(sc_i, &a), SAI_STATUS_OBJECT_IN_USE);   // not under installed SAs
    a = ms_u64(SAI_MACSEC_SA_ATTR_MINIMUM_INGRESS_XPN, 99);
    OK(ma->set_macsec_sa_attribute(sa_i3, &a));   // accepted, not written
    CHECK(ms_log_take().empty());
    // a sequence that fails (the PHY does not acknowledge): the SA is not created, nothing of it stays
    xp_reg(0x10, 0x21f, 0);
    ms_create(ma->create_macsec_sa, ms_sa_attrs(ING, sc_i, 0, true, true, 1), SAI_STATUS_FAILURE);
    CHECK(ext_macsec_status().find("sa-add.ingress.0.gcm-aes-xpn-256") != std::string::npos);   // the last failure, by name
    xp_reg(0x10, 0x21f, 1);
    ms_log_take();

    // the key reached the bus (above) and nothing else: not the log, not the status text
    std::fflush(stderr);
    ::dup2(saved, 2);
    ::close(saved);
    log_configure(false);
    sai_log_set(SAI_API_MACSEC, SAI_LOG_LEVEL_NOTICE);
    sai_log_set(SAI_API_SWITCH, SAI_LOG_LEVEL_NOTICE);
    {
        std::string text, st = ext_macsec_status();
        char buf[4096];
        ::lseek(lfd, 0, SEEK_SET);
        for (ssize_t n; (n = ::read(lfd, buf, sizeof(buf))) > 0;) text.append(buf, (size_t)n);
        ::close(lfd);
        ::unlink(logf);
        CHECK(text.find("SA AN 2 installed") != std::string::npos);   // the log was captured
        for (const char* k : {"c0c1", "c1c0", "c2c3", "c3c2", "8081", "8f80", "5051", "5157", "c0 c1", "c0:c1", "0xc0"}) {
            CHECK(text.find(k) == std::string::npos);
            CHECK(st.find(k) == std::string::npos);
        }
    }

    // removal: the transmitting egress SA is also the last: gate closed first, then the "last" sequence
    OK(ma->remove_macsec_sa(sa_e1));
    MS_BUS("120=0 301=8001");
    OK(ma->remove_macsec_sa(sa_i2));
    MS_BUS("312=1");
    OK(ma->remove_macsec_sa(sa_i3));
    MS_BUS("121=0 313=8001");
    // a new first SA after that: installed as first again, gate open again (the flow is still active)
    sai_object_id_t sa_e2 = ms_create(ma->create_macsec_sa, ms_sa_attrs(EGR, sc_e, 2, false, false, 1));
    MS_BUS("200=102 201=c3c2 202=d1d0 203=8f80 204=1c01 205=1 206=0 207=0 120=1");
    // the flow goes inactive (MACsecOrch: port disabled): the gate closes, the SA stays
    a = ms_action(SAI_ACL_ENTRY_ATTR_ACTION_MACSEC_FLOW, false, 0, 0);
    OK(aa->set_acl_entry_attribute(data[0], &a));
    MS_BUS("120=0");
    CHECK_EQ(ma->remove_macsec_sc(sc_e), SAI_STATUS_OBJECT_IN_USE);
    CHECK_EQ(ma->remove_macsec_flow(f_e), SAI_STATUS_OBJECT_IN_USE);
    CHECK_EQ(aa->remove_acl_table(tbl[0]), SAI_STATUS_OBJECT_IN_USE);
    OK(ma->remove_macsec_sa(sa_e2));
    MS_BUS("302=8001");

    // teardown in MACsecOrch's order; the port's engines go back to bypass and the gap to the quirk's value
    for (int d = 0; d < 2; d++) {
        OK(aa->remove_acl_entry(data[d]));
        OK(aa->remove_acl_entry(eapol[d]));
        a = ms_oid(d ? SAI_PORT_ATTR_INGRESS_MACSEC_ACL : SAI_PORT_ATTR_EGRESS_MACSEC_ACL, SAI_NULL_OBJECT_ID);
        OK(pa->set_port_attribute(p0, &a));
        OK(aa->remove_acl_table(tbl[d]));
    }
    OK(ma->remove_macsec_sc(sc_e));
    OK(ma->remove_macsec_sc(sc_i));
    OK(ma->remove_macsec_flow(f_e));
    OK(ma->remove_macsec_flow(f_i));
    CHECK_EQ(ma->remove_macsec(m_e), SAI_STATUS_OBJECT_IN_USE);
    OK(ma->remove_macsec_port(mp_e));
    MS_BUS("100=1");
    CHECK_EQ(fake().port_state[hw0].ifg, 384u);   // the ingress MACsec port is still there
    OK(ma->remove_macsec_port(mp_i));
    MS_BUS("101=1");
    CHECK_EQ(fake().port_state[hw0].ifg, 128u);
    OK(ma->remove_macsec(m_e));
    OK(ma->remove_macsec(m_i));
    CHECK(ext_macsec_status().find("0 macsec, 0 ports, 0 flows, 0 SCs, 0 SAs, 0 ACL tables, 0 ACL entries") != std::string::npos);

    // ---- a table without gate sequences and without probe: nothing at creation, SAs still work
    OK(sw->remove_switch(g_sw));
    std::string f_plain = xp_write(ms_table(false, false));
    g_profile["SAI_OPENBCM_EXT_MACSEC_FILE"] = f_plain;
    g_profile.erase("SAI_OPENBCM_PORT_IFG");
    OK(create_switch_default());
    CHECK(ms_log_take().empty());
    a.id = SAI_SWITCH_ATTR_PORT_LIST;
    a.value.objlist.count = 64;
    a.value.objlist.list = ports;
    OK(sw->get_switch_attribute(g_sw, 1, &a));
    p0 = ports[0];
    hw0 = ctx().ports[p0].hw;
    xp_reg(0x10, 0x20f, 1);
    m_e = ms_create(ma->create_macsec, {ms_s32(SAI_MACSEC_ATTR_DIRECTION, EGR)});
    mp_e = ms_create(ma->create_macsec_port, {ms_s32(SAI_MACSEC_PORT_ATTR_MACSEC_DIRECTION, EGR), ms_oid(SAI_MACSEC_PORT_ATTR_PORT_ID, p0)});
    CHECK_EQ(fake().port_state[hw0].ifg, 384u);
    OK(ma->remove_macsec_port(mp_e));
    CHECK_EQ(fake().port_state[hw0].ifg, 96u);   // no quirk: the Ethernet default
    OK(ma->remove_macsec(m_e));

    // ---- back to a default switch
    OK(sw->remove_switch(g_sw));
    for (const char* k : {"SAI_OPENBCM_FAKE_PORTS", "SAI_OPENBCM_FAKE_LANES", "SAI_OPENBCM_FAKE_SPEED", "SAI_OPENBCM_EXT_MACSEC_FILE"}) g_profile.erase(k);
    for (auto& f : {f_bad, f_lane, f_good, f_plain}) ::unlink(f.c_str());
    OK(create_switch_default());
}
