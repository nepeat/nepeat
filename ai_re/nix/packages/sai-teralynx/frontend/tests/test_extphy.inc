// SPDX-License-Identifier: Apache-2.0
// External PHY table (SAI_OPENBCM_EXT_PHY_FILE, src/ext_phy.h): parsing, the
// link = ASIC link AND line-side conditions rule, debounce, faults, the admin
// sequences, hold-off. Included by test_sai.cpp; leaves a default switch. The table texts:
// tests/test_tables.h; the parser's own unit tests: tests/test_units.cpp.

static std::string xp_write(const std::string& text)
{
    char tmpl[] = "/tmp/sai-extphy-XXXXXX";
    int fd = ::mkstemp(tmpl);
    CHECK(fd >= 0);
    (void)!::write(fd, text.data(), text.size());
    ::close(fd);
    return tmpl;
}

static int xp_events(sai_object_id_t port, sai_port_oper_status_t st)
{
    std::lock_guard<std::mutex> g(g_nmu);
    int n = 0;
    for (auto& e : g_port_events)
        if (e.port_id == port && e.port_state == st) n++;
    return n;
}
static void xp_clear() { std::lock_guard<std::mutex> g(g_nmu); g_port_events.clear(); }
static bool xp_quiet()
{
    std::this_thread::sleep_for(std::chrono::milliseconds(40));
    std::lock_guard<std::mutex> g(g_nmu);
    return g_port_events.empty();
}
static bool xp_oper(sai_object_id_t port)
{
    sai_attribute_t a;
    a.id = SAI_PORT_ATTR_OPER_STATUS;
    OK(pa->get_port_attribute(port, 1, &a));
    return a.value.s32 == SAI_PORT_OPER_STATUS_UP;
}
static void xp_admin(sai_object_id_t port, bool up)
{
    sai_attribute_t a;
    a.id = SAI_PORT_ATTR_ADMIN_STATE;
    a.value.booldata = up;
    OK(pa->set_port_attribute(port, &a));
}
static void xp_reg(unsigned phy, unsigned reg, unsigned v) { tp::mdio_set(phy, 1u, reg, v); }
static int xp_writes(unsigned phy, unsigned reg, unsigned val)
{
    int n = 0;
    for (auto& o : tp::mdio_log())
        if (o.op == 'W' && o.phy == phy && o.reg == reg && o.val == val) n++;
    return n;
}

static void test_ext_phy()
{
    // ---- parser: tests/test_units.cpp (a pure C++ unit; the Rust side tests its own in phy-seq)

    // ---- switch creation: missing file, bad table, unknown lane
    sai_object_id_t ports[64];
    sai_attribute_t a;
    OK(sw->remove_switch(g_sw));
    g_profile["SAI_OPENBCM_FAKE_PORTS"] = "4";
    g_profile["SAI_OPENBCM_FAKE_LANES"] = "4";
    g_profile["SAI_OPENBCM_FAKE_SPEED"] = "100000";
    g_profile["SAI_OPENBCM_EXT_PHY_FILE"] = "/nonexistent/ext-phy.tbl";
    CHECK_EQ(create_switch_default(), SAI_STATUS_FAILURE);
    CHECK(!tp::switch_created());
    std::string f_bad = xp_write("ext-phy 1\nport 1\n");
    g_profile["SAI_OPENBCM_EXT_PHY_FILE"] = f_bad;
    CHECK_EQ(create_switch_default(), SAI_STATUS_INVALID_PARAMETER);
    CHECK(!tp::switch_created());
    std::string f_lane = xp_write("ext-phy 1\nport 3\nlink\nC 10 1 a101 3 3\n");   // lane 3 is inside port 0, not its first
    g_profile["SAI_OPENBCM_EXT_PHY_FILE"] = f_lane;
    CHECK_EQ(create_switch_default(), SAI_STATUS_INVALID_PARAMETER);
    CHECK(!tp::switch_created());
    CHECK(!tp::ext_phy_active());

    // ---- the good table: fake ports hw 1 (lane 1), 5 (lane 5), 9, 13
    std::string f_good = xp_write(kExtPhyGood);
    g_profile["SAI_OPENBCM_EXT_PHY_FILE"] = f_good;
    std::string ddir = "/tmp/sai-extphy-diag-" + std::to_string(::getpid());
    std::string dsock = ddir + "/diag.sock";
    g_profile["SAI_OPENBCM_DIAG_SOCKET"] = dsock;
    OK(create_switch_default());
    CHECK(tp::ext_phy_active());
    a.id = SAI_SWITCH_ATTR_PORT_LIST;
    a.value.objlist.count = 64;
    a.value.objlist.list = ports;
    OK(sw->get_switch_attribute(g_sw, 1, &a));
    CHECK_EQ(a.value.objlist.count, 4u);
    sai_object_id_t p0 = ports[0], p1 = ports[1], p2 = ports[2];
    // Creation disables every port: the admin-down sequence of port 0 ran once.
    CHECK_EQ(xp_writes(0x10, 0xd139, 0x351), 1);
    xp_clear();

    // A port the table does not list behaves as before: link follows the ASIC.
    xp_admin(p2, true);
    CHECK(wait_for([&] { return !g_port_events.empty(); }));
    CHECK_EQ(xp_events(p2, SAI_PORT_OPER_STATUS_UP), 1);
    CHECK(xp_oper(p2));
    xp_clear();

    // Port 0, line side down (a101 = 0): admin up runs admin-up, the ASIC link
    // comes up in the fake, the port stays down.
    xp_reg(0x10, 0x8221, 0x11e3);
    xp_reg(0x10, 0xa100, 2);
    xp_reg(0x10, 0xa101, 0);
    xp_reg(0x10, 0xd0e1, 0xa088);   // E masks bit 7: passes
    xp_admin(p0, true);
    CHECK_EQ(xp_writes(0x10, 0xd139, 0x350), 1);
    CHECK(xp_quiet());
    CHECK(!xp_oper(p0));
    CHECK(tp::port(1).admin);
    for (int i = 0; i < 5; i++) tp::linkscan();
    CHECK(xp_quiet());
    CHECK(!xp_oper(p0));
    // Line side up: one good poll is not enough (up-polls 2), the second is.
    xp_reg(0x10, 0xa101, 3);
    tp::linkscan();
    CHECK(xp_quiet());
    CHECK(!xp_oper(p0));
    tp::linkscan();
    CHECK(wait_for([&] { return !g_port_events.empty(); }));
    CHECK_EQ(xp_events(p0, SAI_PORT_OPER_STATUS_UP), 1);
    CHECK(xp_oper(p0));
    xp_clear();
    tp::linkscan();
    CHECK(xp_quiet());
    // One bad poll takes it down at once; one bit of the condition is enough.
    xp_reg(0x10, 0xa101, 1);
    tp::linkscan();
    CHECK(wait_for([&] { return !g_port_events.empty(); }));
    CHECK_EQ(xp_events(p0, SAI_PORT_OPER_STATUS_DOWN), 1);
    CHECK(!xp_oper(p0));
    xp_clear();
    // A flapping line never reaches two good polls in a row.
    for (int i = 0; i < 4; i++) {
        xp_reg(0x10, 0xa101, i % 2 ? 0 : 3);
        tp::linkscan();
    }
    CHECK(xp_quiet());
    xp_reg(0x10, 0xa101, 3);
    tp::linkscan();
    tp::linkscan();
    CHECK(wait_for([&] { return !g_port_events.empty(); }));
    CHECK(xp_oper(p0));
    xp_clear();
    // AND: line side up, ASIC link down -> down.
    tp::inject_link(1, false);
    CHECK(wait_for([&] { return !g_port_events.empty(); }));
    CHECK_EQ(xp_events(p0, SAI_PORT_OPER_STATUS_DOWN), 1);
    CHECK(!xp_oper(p0));
    xp_clear();
    tp::linkscan();   // ASIC link back (the fake follows admin), line side still good
    CHECK(wait_for([&] { return !g_port_events.empty(); }));
    CHECK(xp_oper(p0));
    xp_clear();
    // Fault: the PHY stops answering (MDIO error) -> down at once, back after two good polls.
    tp::mdio_set_dead(0x10, true);
    tp::linkscan();
    CHECK(wait_for([&] { return !g_port_events.empty(); }));
    CHECK(!xp_oper(p0));
    CHECK(tp::ext_phy_status().find("FAULT") != std::string::npos);
    xp_clear();
    tp::mdio_set_dead(0x10, false);
    tp::linkscan();
    tp::linkscan();
    CHECK(wait_for([&] { return !g_port_events.empty(); }));
    CHECK(xp_oper(p0));
    CHECK(tp::ext_phy_status().find("FAULT") == std::string::npos);
    xp_clear();
    // Fault: the poll (P) never sees its value, or the E check fails -> down.
    xp_reg(0x10, 0x8221, 0);
    tp::linkscan();
    CHECK(wait_for([&] { return !g_port_events.empty(); }));
    CHECK(!xp_oper(p0));
    xp_clear();
    xp_reg(0x10, 0x8221, 0x11e3);
    xp_reg(0x10, 0xa100, 7);
    tp::linkscan();
    tp::linkscan();
    CHECK(xp_quiet());
    CHECK(!xp_oper(p0));
    xp_reg(0x10, 0xa100, 2);
    tp::linkscan();
    tp::linkscan();
    CHECK(wait_for([&] { return !g_port_events.empty(); }));
    CHECK(xp_oper(p0));
    xp_clear();

    // Hold-off: raw MDIO on the diag socket runs under the sequence lock and
    // pauses the polls (state frozen) for holdoff-ms after the last one.
    size_t shells = tp::shell_log().size();
    CHECK(diag_cmd(dsock, "phy raw c45 0x10 1 0x8b00\n") == "fake: phy raw c45 0x10 1 0x8b00\n");
    CHECK_EQ(tp::shell_log().size(), shells + 1);
    xp_reg(0x10, 0xa101, 0);
    tp::linkscan();
    CHECK(xp_oper(p0));                                  // not polled: still up
    CHECK(tp::ext_phy_status().find("PAUSED") != std::string::npos);
    std::this_thread::sleep_for(std::chrono::milliseconds(80));
    tp::linkscan();
    CHECK(wait_for([&] { return !g_port_events.empty(); }));
    CHECK(!xp_oper(p0));
    xp_clear();
    // "extphy" on the diag socket is answered here, not by the SDK shell.
    std::string stx = diag_cmd(dsock, "extphy\n");
    CHECK(stx.rfind("extphy: 2 ports", 0) == 0);
    CHECK(stx.find("ce0") != std::string::npos && stx.find("ce1") != std::string::npos);
    CHECK_EQ(tp::shell_log().size(), shells + 1);

    // Admin down: the ASIC port first, then the admin-down sequence; no poll while down.
    xp_reg(0x10, 0xa101, 3);
    tp::linkscan();
    tp::linkscan();
    CHECK(wait_for([&] { return !g_port_events.empty(); }));
    xp_clear();
    xp_admin(p0, false);
    CHECK_EQ(xp_writes(0x10, 0xd139, 0x351), 2);
    CHECK(wait_for([&] { return !g_port_events.empty(); }));
    CHECK_EQ(xp_events(p0, SAI_PORT_OPER_STATUS_DOWN), 1);
    xp_clear();
    size_t ops;
    ops = tp::mdio_log().size();
    tp::linkscan();
    CHECK_EQ(tp::mdio_log().size(), ops);
    // Admin up again starts from "down": two good polls (the first is made by the enable itself).
    xp_admin(p0, true);
    CHECK(xp_quiet());
    tp::linkscan();
    CHECK(wait_for([&] { return !g_port_events.empty(); }));
    CHECK(xp_oper(p0));
    xp_clear();
    // An admin sequence that stops (E mismatch) is logged and counted; the SAI call still succeeds.
    xp_admin(p0, false);
    CHECK(wait_for([&] { return !g_port_events.empty(); }));
    xp_reg(0x10, 0xd0e1, 0x1234);
    xp_admin(p0, true);
    CHECK_EQ(xp_writes(0x10, 0xd139, 0x350), 2);         // the write after the failed E did not run
    CHECK(tp::port(1).admin);
    xp_clear();

    // Port 1: a table entry with only a link condition (no admin sequences).
    xp_reg(0x11, 0xa101, 3);
    xp_admin(p1, true);
    CHECK(xp_quiet());
    tp::linkscan();
    CHECK(wait_for([&] { return !g_port_events.empty(); }));
    CHECK_EQ(xp_events(p1, SAI_PORT_OPER_STATUS_UP), 1);
    xp_clear();
    OK(sw->remove_switch(g_sw));
    CHECK(!tp::ext_phy_active());

    // poll-ms: between polls the cached state answers (here: one poll, then none for a minute).
    std::string f_slow = xp_write("ext-phy 1\npoll-ms 60000\nup-polls 1\nport 1\nlink\nC 10 1 a101 3 3\n");
    g_profile["SAI_OPENBCM_EXT_PHY_FILE"] = f_slow;
    g_profile.erase("SAI_OPENBCM_DIAG_SOCKET");
    OK(create_switch_default());
    a.id = SAI_SWITCH_ATTR_PORT_LIST;
    a.value.objlist.count = 64;
    a.value.objlist.list = ports;
    OK(sw->get_switch_attribute(g_sw, 1, &a));
    xp_clear();
    xp_reg(0x10, 0xa101, 3);
    xp_admin(ports[0], true);                            // up-polls 1: the enable's own poll is enough
    CHECK(wait_for([&] { return !g_port_events.empty(); }));
    CHECK(xp_oper(ports[0]));
    xp_clear();
    xp_reg(0x10, 0xa101, 0);
    tp::linkscan();
    CHECK(xp_quiet());
    CHECK(xp_oper(ports[0]));
    OK(sw->remove_switch(g_sw));

    // No key: nothing registered, no MDIO, ports as ever.
    g_profile.erase("SAI_OPENBCM_EXT_PHY_FILE");
    OK(create_switch_default());
    CHECK(!tp::ext_phy_active());
    CHECK_EQ(tp::mdio_log().size(), 0u);
    CHECK(tp::ext_phy_status().find("no external PHY table") != std::string::npos);
    for (auto& f : {f_bad, f_lane, f_good, f_slow}) std::remove(f.c_str());
    ::rmdir(ddir.c_str());
}
