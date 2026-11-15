// SPDX-License-Identifier: Apache-2.0
// SWITCH object: backend selection and bring-up, default objects, event
// delivery (link scan / L2 learn -> SAI notifications). Temperatures: sai_temp.cpp.
#include "sai_internal.h"

#include <algorithm>
#include <sys/stat.h>
#include <unistd.h>

namespace ob {

static constexpr uint32_t QUEUES_PER_PORT = 8;
static constexpr uint32_t IPGS_PER_PORT = 8;
static constexpr uint32_t PORT_MAX_MTU = 9408;   // SDK frame max 9416 - FCS - VLAN tag

// Switch attributes accepted and stored without hardware effect.
static const StubDef switch_stubs[] = {
    {SAI_SWITCH_ATTR_ECMP_DEFAULT_HASH_SEED, Kind::U32, 0, false},
    {SAI_SWITCH_ATTR_LAG_DEFAULT_HASH_SEED, Kind::U32, 0, false},
    {SAI_SWITCH_ATTR_ECMP_DEFAULT_HASH_ALGORITHM, Kind::S32, SAI_HASH_ALGORITHM_CRC, false},
    {SAI_SWITCH_ATTR_LAG_DEFAULT_HASH_ALGORITHM, Kind::S32, SAI_HASH_ALGORITHM_CRC, false},
    {SAI_SWITCH_ATTR_ECMP_DEFAULT_HASH_OFFSET, Kind::U8, 0, false},
    {SAI_SWITCH_ATTR_LAG_DEFAULT_HASH_OFFSET, Kind::U8, 0, false},
    {SAI_SWITCH_ATTR_RESTART_WARM, Kind::Bool, 0, false},
    {SAI_SWITCH_ATTR_WARM_RECOVER, Kind::Bool, 0, false},
    {SAI_SWITCH_ATTR_PRE_SHUTDOWN, Kind::Bool, 0, false},
    {SAI_SWITCH_ATTR_FAST_API_ENABLE, Kind::Bool, 0, false},
    {SAI_SWITCH_ATTR_UNINIT_DATA_PLANE_ON_REMOVAL, Kind::Bool, 1, false},
    {SAI_SWITCH_ATTR_VXLAN_DEFAULT_PORT, Kind::U16, 4789, false},
    {SAI_SWITCH_ATTR_BCAST_CPU_FLOOD_ENABLE, Kind::Bool, 0, false},
    {SAI_SWITCH_ATTR_MCAST_CPU_FLOOD_ENABLE, Kind::Bool, 0, false},
    {SAI_SWITCH_ATTR_SWITCHING_MODE, Kind::S32, SAI_SWITCH_SWITCHING_MODE_STORE_AND_FORWARD, false},
    {SAI_SWITCH_ATTR_FDB_UNICAST_MISS_PACKET_ACTION, Kind::S32, SAI_PACKET_ACTION_FORWARD, false},
    {SAI_SWITCH_ATTR_FDB_BROADCAST_MISS_PACKET_ACTION, Kind::S32, SAI_PACKET_ACTION_FORWARD, false},
    {SAI_SWITCH_ATTR_FDB_MULTICAST_MISS_PACKET_ACTION, Kind::S32, SAI_PACKET_ACTION_FORWARD, false},
    {SAI_SWITCH_ATTR_COUNTER_REFRESH_INTERVAL, Kind::U32, 1, false},
    {SAI_SWITCH_ATTR_SWITCH_ID, Kind::U32, 0, true},
    {SAI_SWITCH_ATTR_SWITCH_PROFILE_ID, Kind::U32, 0, true},
    {SAI_SWITCH_ATTR_MAX_SYSTEM_CORES, Kind::U32, 0, true},
};
static constexpr size_t N_SWITCH_STUBS = sizeof(switch_stubs) / sizeof(switch_stubs[0]);

// Create-time attributes that only make sense for other platforms or carry
// pointers to the caller's environment; accepted and ignored (logged).
static bool create_ignorable(sai_attr_id_t id)
{
    switch (id) {
    case SAI_SWITCH_ATTR_HARDWARE_ACCESS_BUS:
    case SAI_SWITCH_ATTR_PLATFROM_CONTEXT:
    case SAI_SWITCH_ATTR_REGISTER_READ:
    case SAI_SWITCH_ATTR_REGISTER_WRITE:
    case SAI_SWITCH_ATTR_FIRMWARE_PATH_NAME:
    case SAI_SWITCH_ATTR_FIRMWARE_LOAD_METHOD:
    case SAI_SWITCH_ATTR_FIRMWARE_LOAD_TYPE:
    case SAI_SWITCH_ATTR_SWITCH_HARDWARE_INFO:
        return true;
    default:
        return false;
    }
}

static bool is_notify_attr(sai_attr_id_t id)
{
    switch (id) {
    case SAI_SWITCH_ATTR_BFD_SESSION_STATE_CHANGE_NOTIFY:
    case SAI_SWITCH_ATTR_BRIDGE_PORT_HW_PROTECTION_SWITCHOVER_NOTIFY:
    case SAI_SWITCH_ATTR_EXTENDED_PORT_STATE_CHANGE_NOTIFY:
    case SAI_SWITCH_ATTR_FDB_EVENT_NOTIFY:
    case SAI_SWITCH_ATTR_ICMP_ECHO_SESSION_STATE_CHANGE_NOTIFY:
    case SAI_SWITCH_ATTR_IPSEC_POST_STATUS_NOTIFY:
    case SAI_SWITCH_ATTR_IPSEC_SA_STATUS_CHANGE_NOTIFY:
    case SAI_SWITCH_ATTR_MACSEC_POST_STATUS_NOTIFY:
    case SAI_SWITCH_ATTR_NAT_EVENT_NOTIFY:
    case SAI_SWITCH_ATTR_NEXT_HOP_GROUP_HW_PROTECTION_SWITCHOVER_NOTIFY:
    case SAI_SWITCH_ATTR_PACKET_EVENT_NOTIFY:
    case SAI_SWITCH_ATTR_PORT_HOST_TX_READY_NOTIFY:
    case SAI_SWITCH_ATTR_PORT_STATE_CHANGE_NOTIFY:
    case SAI_SWITCH_ATTR_QUEUE_PFC_DEADLOCK_NOTIFY:
    case SAI_SWITCH_ATTR_SWITCH_ASIC_SDK_HEALTH_EVENT_NOTIFY:
    case SAI_SWITCH_ATTR_SWITCH_IPSEC_POST_STATUS_NOTIFY:
    case SAI_SWITCH_ATTR_SWITCH_MACSEC_POST_STATUS_NOTIFY:
    case SAI_SWITCH_ATTR_SWITCH_SHUTDOWN_REQUEST_NOTIFY:
    case SAI_SWITCH_ATTR_SWITCH_STATE_CHANGE_NOTIFY:
    case SAI_SWITCH_ATTR_TAM_EVENT_LEARN_NOTIFY:
    case SAI_SWITCH_ATTR_TAM_EVENT_NOTIFY:
    case SAI_SWITCH_ATTR_TAM_TEL_TYPE_CONFIG_CHANGE_NOTIFY:
    case SAI_SWITCH_ATTR_TWAMP_SESSION_EVENT_NOTIFY:
        return true;
    default:
        return false;
    }
}

static void set_notify(sai_attr_id_t id, sai_pointer_t p)
{
    auto& c = ctx();
    c.notify[id] = p;
    if (id == SAI_SWITCH_ATTR_PORT_STATE_CHANGE_NOTIFY) c.port_state_cb = (sai_port_state_change_notification_fn)p;
    if (id == SAI_SWITCH_ATTR_FDB_EVENT_NOTIFY) c.fdb_cb = (sai_fdb_event_notification_fn)p;
}

// ------------------------------------------------------------------ events
void push_event(const Event& e)
{
    auto& c = ctx();
    {
        std::lock_guard<std::mutex> g(c.ev_mu);
        c.events.push_back(e);
    }
    c.ev_cv.notify_one();
}

static void deliver_link(int hw, bool up)
{
    sai_port_state_change_notification_fn cb = nullptr;
    sai_port_oper_status_notification_t n;
    std::memset(&n, 0, sizeof(n));
    {
        std::lock_guard<std::recursive_mutex> g(ctx().mu);
        auto& c = ctx();
        if (!c.switch_created) return;
        auto it = c.hw2port.find(hw);
        if (it == c.hw2port.end()) return;
        PortObj& p = c.ports[it->second];
        sai_port_oper_status_t st = up ? SAI_PORT_OPER_STATUS_UP : SAI_PORT_OPER_STATUS_DOWN;
        if (p.oper == st) return;   // duplicate from link scan
        p.oper = st;
        n.port_id = p.oid;
        n.port_state = st;
        cb = c.port_state_cb;
        OB_NOTICE(SAI_API_PORT, "port %s (hw %d) oper %s", p.sdk_name.c_str(), hw, up ? "up" : "down");
    }
    if (cb) cb(1, &n);
}

static void deliver_fdb(const FdbEvent& ev)
{
    sai_fdb_event_notification_fn cb = nullptr;
    sai_fdb_event_notification_data_t d;
    sai_attribute_t attrs[3];
    std::memset(&d, 0, sizeof(d));
    std::memset(attrs, 0, sizeof(attrs));
    {
        std::lock_guard<std::recursive_mutex> g(ctx().mu);
        auto& c = ctx();
        if (!c.switch_created) return;
        auto vit = c.vid2vlan.find(ev.vid);
        sai_object_id_t bport = bridge_port_of_dest(ev.hw_port);   // port or LAG
        if (vit == c.vid2vlan.end()) return;     // internal (port RIF) VLAN: not visible to SAI
        FdbKey k;
        std::memcpy(k.mac, ev.mac, 6);
        k.bv = vit->second;
        if (ev.learned) {
            if (!bport) return;                  // learned on a port without bridge port
            auto it = c.fdbs.find(k);
            if (it != c.fdbs.end() && it->second.type == SAI_FDB_ENTRY_TYPE_STATIC) return;
            if (it != c.fdbs.end() && it->second.bport == bport) return;   // already known (e.g. created via SAI)
            bool move = it != c.fdbs.end() && it->second.bport != bport;
            FdbObj o;
            o.type = SAI_FDB_ENTRY_TYPE_DYNAMIC;
            o.bport = bport;
            c.fdbs[k] = o;
            d.event_type = move ? SAI_FDB_EVENT_MOVE : SAI_FDB_EVENT_LEARNED;
        } else {
            auto it = c.fdbs.find(k);
            if (it == c.fdbs.end() || it->second.type == SAI_FDB_ENTRY_TYPE_STATIC) return;
            if (!bport) bport = it->second.bport;
            c.fdbs.erase(it);
            d.event_type = SAI_FDB_EVENT_AGED;
        }
        d.fdb_entry.switch_id = c.switch_oid;
        std::memcpy(d.fdb_entry.mac_address, ev.mac, 6);
        d.fdb_entry.bv_id = vit->second;
        attrs[0].id = SAI_FDB_ENTRY_ATTR_TYPE;
        attrs[0].value.s32 = SAI_FDB_ENTRY_TYPE_DYNAMIC;
        attrs[1].id = SAI_FDB_ENTRY_ATTR_BRIDGE_PORT_ID;
        attrs[1].value.oid = bport;
        attrs[2].id = SAI_FDB_ENTRY_ATTR_PACKET_ACTION;
        attrs[2].value.s32 = SAI_PACKET_ACTION_FORWARD;
        d.attr_count = 3;
        d.attr = attrs;
        cb = c.fdb_cb;
    }
    if (cb) cb(1, &d);
}

static void notifier_loop()
{
    auto& c = ctx();
    for (;;) {
        Event e;
        {
            std::unique_lock<std::mutex> g(c.ev_mu);
            c.ev_cv.wait(g, [&] { return !c.events.empty(); });
            e = c.events.front();
            c.events.pop_front();
        }
        if (e.kind == Event::Stop) return;
        try {
            if (e.kind == Event::Link) deliver_link(e.hw, e.up);
            else deliver_fdb(e.fdb);
        } catch (...) {
            OB_ERR(SAI_API_SWITCH, "exception while delivering a notification");
        }
        c.events_delivered++;
    }
}

void events_start()
{
    auto& c = ctx();
    if (c.notifier.joinable()) return;
    c.notifier = std::thread(notifier_loop);
}

void events_stop()
{
    auto& c = ctx();
    if (!c.notifier.joinable()) return;
    Event e;
    e.kind = Event::Stop;
    {
        std::lock_guard<std::mutex> g(c.ev_mu);
        c.events.push_back(e);
    }
    c.ev_cv.notify_one();
    c.notifier.join();
    std::lock_guard<std::mutex> g(c.ev_mu);
    c.events.clear();
}

// --------------------------------------------------------- default objects
void switch_build_objects()
{
    auto& c = ctx();
    Backend& be = *c.be;

    c.cpu_port = make_oid(SAI_OBJECT_TYPE_PORT, (uint64_t)be.cpu_port());
    PortObj cpu;
    cpu.oid = c.cpu_port;
    cpu.hw = be.cpu_port();
    cpu.cpu = true;
    cpu.sdk_name = "cpu0";
    cpu.admin = true;
    cpu.oper = SAI_PORT_OPER_STATUS_UP;
    c.ports[cpu.oid] = cpu;
    c.hw2port[cpu.hw] = cpu.oid;

    for (const PortInfo& pi : be.ports()) {
        PortObj p;
        p.oid = make_oid(SAI_OBJECT_TYPE_PORT, (uint64_t)pi.hw);
        p.hw = pi.hw;
        p.sdk_name = pi.name;
        p.lanes = pi.lanes;
        p.autoneg = pi.autoneg;
        // Safety: every front-panel port starts admin-down (SAI default),
        // whatever the SDK init or the rc script left behind.
        sai_status_t st = be.port_admin_set(pi.hw, false);
        if (st != SAI_STATUS_SUCCESS)
            OB_ERR(SAI_API_SWITCH, "port %s: disable failed (%d)", pi.name.c_str(), st);
        p.admin = false;
        bool up = false;
        if (be.port_link_get(pi.hw, up) == SAI_STATUS_SUCCESS && up) p.oper = SAI_PORT_OPER_STATUS_UP;
        for (uint32_t q = 0; q < QUEUES_PER_PORT; q++) {
            sai_object_id_t qo = make_oid(SAI_OBJECT_TYPE_QUEUE, ((uint64_t)pi.hw << 8) | q);
            QueueObj qobj;
            qobj.index = (uint8_t)q;
            qobj.port = p.oid;
            c.queues[qo] = qobj;
            p.queues.push_back(qo);
        }
        for (uint32_t i = 0; i < IPGS_PER_PORT; i++) {
            sai_object_id_t io = make_oid(SAI_OBJECT_TYPE_INGRESS_PRIORITY_GROUP, ((uint64_t)pi.hw << 8) | i);
            IpgObj g;
            g.index = (uint8_t)i;
            g.port = p.oid;
            c.ipgs[io] = g;
            p.ipgs.push_back(io);
        }
        c.ports[p.oid] = p;
        c.hw2port[p.hw] = p.oid;
        c.port_list.push_back(p.oid);
    }

    c.default_vr = make_oid(SAI_OBJECT_TYPE_VIRTUAL_ROUTER, 1);
    c.next_index[SAI_OBJECT_TYPE_VIRTUAL_ROUTER] = 1;
    c.vrs[c.default_vr] = VrObj();   // SDK VRF 0
    c.default_stp = make_oid(SAI_OBJECT_TYPE_STP, 1);
    c.default_trap_group = make_oid(SAI_OBJECT_TYPE_HOSTIF_TRAP_GROUP, 1);
    c.next_index[SAI_OBJECT_TYPE_HOSTIF_TRAP_GROUP] = 1;
    c.trap_groups[c.default_trap_group] = TrapGroupObj();

    c.default_bridge = make_oid(SAI_OBJECT_TYPE_BRIDGE, 1);
    c.next_index[SAI_OBJECT_TYPE_BRIDGE] = 1;
    c.bridges[c.default_bridge] = BridgeObj();
    for (auto po : c.port_list) {
        sai_object_id_t bp = c.alloc(SAI_OBJECT_TYPE_BRIDGE_PORT);
        BridgePortObj b;
        b.port = po;
        b.bridge = c.default_bridge;
        b.admin = true;   // the SDK forwards on these ports
        c.bridge_ports[bp] = b;
        c.bridges[c.default_bridge].ports.insert(bp);
    }

    // Default VLAN 1 with the SDK's own membership (SONiC's orchagent
    // removes what it does not want).
    VlanObj v1;
    v1.vid = 1;
    v1.oid = make_oid(SAI_OBJECT_TYPE_VLAN, 1);
    c.vlans[v1.oid] = v1;
    c.vid2vlan[1] = v1.oid;
    std::vector<std::pair<int, bool>> members;
    if (be.vlan_members(1, members) == SAI_STATUS_SUCCESS) {
        for (auto& m : members) {
            sai_object_id_t bp = bridge_port_of_dest(m.first);
            if (!bp) continue;
            vlan_member_create_internal(v1.oid, bp,
                                        m.second ? SAI_VLAN_TAGGING_MODE_UNTAGGED : SAI_VLAN_TAGGING_MODE_TAGGED,
                                        false, nullptr);
        }
    }
}

void switch_clear_objects()
{
    auto& c = ctx();
    if (c.be) {
        for (auto& h : c.hostifs)
            if (h.second.netif >= 0) c.be->netif_destroy(h.second.netif, h.second.filter);
    }
    c.ports.clear(); c.hw2port.clear(); c.port_list.clear(); c.lags.clear(); c.lag_members.clear();
    c.vlans.clear(); c.vid2vlan.clear();
    c.vlan_members.clear(); c.bridges.clear(); c.bridge_ports.clear(); c.fdbs.clear(); c.hostifs.clear();
    c.trap_groups.clear(); c.traps.clear(); c.table_entries.clear(); c.queues.clear(); c.ipgs.clear();
    c.rifs.clear(); c.neighbors.clear(); c.next_hops.clear(); c.routes.clear(); c.internal_vlans.clear();
    c.vrs.clear(); c.nhgs.clear(); c.nhg_members.clear();
    c.next_index.clear(); c.switch_stub.clear(); c.notify.clear();
    c.port_state_cb = nullptr; c.fdb_cb = nullptr;
    c.switch_station = -1;
    c.fdb_aging = 0;
}

// ----------------------------------------------------------- backend pick
static bool hardware_present()
{
    struct stat st;
    return stat("/dev/linux-user-bde", &st) == 0 || stat("/dev/linux-kernel-bde", &st) == 0;
}

static BackendConfig collect_profile()
{
    BackendConfig cfg;
    auto& c = ctx();
    if (c.smt.profile_get_next_value) {
        const char* k = nullptr;
        const char* v = nullptr;
        c.smt.profile_get_next_value(0, nullptr, nullptr);   // restart enumeration
        for (int guard = 0; guard < 4096 && c.smt.profile_get_next_value(0, &k, &v) == 0; guard++)
            if (k && v) cfg.profile[k] = v;
    }
    static const char* keys[] = {"SAI_INIT_CONFIG_FILE", "SAI_OPENBCM_BACKEND", "SAI_OPENBCM_RC_FILE",
                                 "SAI_OPENBCM_UNIT", "SAI_OPENBCM_KNET", "SAI_OPENBCM_FAKE_PORTS",
                                 "SAI_OPENBCM_FAKE_LANES", "SAI_OPENBCM_FAKE_SPEED", "SAI_OPENBCM_FAKE_LINK",
                                 "SAI_OPENBCM_FAKE_ON_HARDWARE", "SAI_OPENBCM_RIF_VLAN_BASE",
                                 "SAI_OPENBCM_TEMP_FILE", "SAI_OPENBCM_TEMP_PROM", "SAI_OPENBCM_TEMP_INTERVAL"};
    for (const char* k : keys) {
        std::string v = profile_get(k, "\x01");
        if (v != "\x01") cfg.profile[k] = v;
    }
    return cfg;
}

static sai_status_t pick_backend(const BackendConfig& cfg)
{
    auto& c = ctx();
    std::string want = cfg.get("SAI_OPENBCM_BACKEND");
    std::unique_ptr<Backend> real(make_openbcm_backend());
    if (want.empty()) {
        if (!real) {
            OB_ERR(SAI_API_SWITCH, "this libsai has no openbcm backend (fake-only build); "
                                   "set SAI_OPENBCM_BACKEND=fake in the profile to use the in-memory model");
            return SAI_STATUS_FAILURE;
        }
        want = "openbcm";
    }
    if (want == "openbcm") {
        if (!real) {
            OB_ERR(SAI_API_SWITCH, "SAI_OPENBCM_BACKEND=openbcm but this libsai was built without the SDK");
            return SAI_STATUS_FAILURE;
        }
        c.be = std::move(real);
        return SAI_STATUS_SUCCESS;
    }
    if (want == "fake") {
        if (hardware_present() && cfg.get("SAI_OPENBCM_FAKE_ON_HARDWARE") != "1") {
            OB_ERR(SAI_API_SWITCH, "SAI_OPENBCM_BACKEND=fake but Broadcom BDE devices exist on this host; "
                                   "refusing (set SAI_OPENBCM_FAKE_ON_HARDWARE=1 to override)");
            return SAI_STATUS_FAILURE;
        }
        OB_WARN(SAI_API_SWITCH, "FAKE backend selected: nothing is programmed into any switch ASIC");
        c.be.reset(make_fake_backend());
        return SAI_STATUS_SUCCESS;
    }
    OB_ERR(SAI_API_SWITCH, "unknown SAI_OPENBCM_BACKEND '%s' (fake|openbcm)", want.c_str());
    return SAI_STATUS_INVALID_PARAMETER;
}

// ------------------------------------------------------------- switch API
static sai_status_t create_switch(_Out_ sai_object_id_t* switch_id, _In_ uint32_t attr_count,
                                  _In_ const sai_attribute_t* attr_list)
{
    return guarded(SAI_API_SWITCH, __func__, [&]() -> sai_status_t {
        auto& c = ctx();
        if (!switch_id) return SAI_STATUS_INVALID_PARAMETER;
        if (attr_count && !attr_list) return SAI_STATUS_INVALID_PARAMETER;
        if (c.switch_created) {
            OB_ERR(SAI_API_SWITCH, "only one switch is supported");
            return SAI_STATUS_INSUFFICIENT_RESOURCES;
        }
        bool init = false, have_init = false;
        bool have_mac = false;
        sai_mac_t mac;
        std::map<sai_attr_id_t, sai_pointer_t> notifies;
        StubMap stubs;
        for (uint32_t i = 0; i < attr_count; i++) {
            const sai_attribute_t& a = attr_list[i];
            if (a.id == SAI_SWITCH_ATTR_INIT_SWITCH) { init = a.value.booldata; have_init = true; }
            else if (a.id == SAI_SWITCH_ATTR_SRC_MAC_ADDRESS) { std::memcpy(mac, a.value.mac, 6); have_mac = true; }
            else if (a.id == SAI_SWITCH_ATTR_TYPE) {
                if (a.value.s32 != SAI_SWITCH_TYPE_NPU) return attr_invalid(i);
            } else if (is_notify_attr(a.id)) notifies[a.id] = a.value.ptr;
            else if (find_stub(switch_stubs, N_SWITCH_STUBS, a.id)) stubs[a.id] = a.value;
            else if (a.id == SAI_SWITCH_ATTR_FDB_AGING_TIME) { /* applied below */ }
            else if (create_ignorable(a.id)) OB_NOTICE(SAI_API_SWITCH, "create_switch: ignoring attr %d", a.id);
            else {
                OB_ERR(SAI_API_SWITCH, "create_switch: attr %d not supported", a.id);
                return attr_not_supported(i);
            }
        }
        if (!have_init) return SAI_STATUS_MANDATORY_ATTRIBUTE_MISSING;
        if (!init) {
            OB_ERR(SAI_API_SWITCH, "INIT_SWITCH=false (connect to existing switch) is not supported");
            return SAI_STATUS_NOT_SUPPORTED;
        }

        BackendConfig cfg = collect_profile();
        sai_status_t st = pick_backend(cfg);
        if (st != SAI_STATUS_SUCCESS) return st;
        OB_NOTICE(SAI_API_SWITCH, "initializing backend %s", c.be->name());
        st = c.be->init(cfg,
                        [](int hw, bool up) {
                            Event e;
                            e.kind = Event::Link;
                            e.hw = hw;
                            e.up = up;
                            push_event(e);
                        },
                        [](const FdbEvent& f) {
                            Event e;
                            e.kind = Event::Fdb;
                            e.fdb = f;
                            push_event(e);
                        });
        if (st != SAI_STATUS_SUCCESS) {
            OB_ERR(SAI_API_SWITCH, "backend %s init failed: %d", c.be->name(), st);
            c.be.reset();
            return st;
        }
        if (have_mac) std::memcpy(c.src_mac, mac, 6);
        for (auto& n : notifies) set_notify(n.first, n.second);
        c.switch_stub = stubs;
        c.switch_oid = make_oid(SAI_OBJECT_TYPE_SWITCH, 1);
        switch_build_objects();
        if (const sai_attribute_t* a = find_attr(attr_count, attr_list, SAI_SWITCH_ATTR_FDB_AGING_TIME)) {
            if (c.be->l2_age_set(a->value.u32) == SAI_STATUS_SUCCESS) c.fdb_aging = a->value.u32;
        }
        c.switch_created = true;
        events_start();
        // Temperature files for fan control. Never from the fake on a host
        // with a switch ASIC: fand would trust made-up readings.
        if (!c.be->is_fake() || !hardware_present()) temp_publisher_start(cfg, c.be.get());
        else if (!cfg.get("SAI_OPENBCM_TEMP_FILE").empty() || !cfg.get("SAI_OPENBCM_TEMP_PROM").empty())
            OB_WARN(SAI_API_SWITCH, "fake backend on hardware: not publishing ASIC temperature files");
        *switch_id = c.switch_oid;
        OB_NOTICE(SAI_API_SWITCH, "switch created: %zu front-panel ports, backend %s", c.port_list.size(),
                  c.be->name());
        return SAI_STATUS_SUCCESS;
    }, false);
}

static sai_status_t remove_switch(_In_ sai_object_id_t switch_id)
{
    sai_status_t st = guarded(SAI_API_SWITCH, __func__, [&]() -> sai_status_t {
        if (switch_id != ctx().switch_oid) return SAI_STATUS_INVALID_OBJECT_ID;
        ctx().switch_created = false;   // stop event delivery from touching objects
        return SAI_STATUS_SUCCESS;
    });
    if (st != SAI_STATUS_SUCCESS) return st;
    temp_publisher_stop();   // removes the files; before the backend goes away
    events_stop();   // without the API lock: the notifier may be waiting for it
    try {
        std::lock_guard<std::recursive_mutex> g(ctx().mu);
        auto& c = ctx();
        switch_clear_objects();
        if (c.be) {
            c.be->shutdown();
            c.be.reset();
        }
        c.switch_oid = 0;
        OB_NOTICE(SAI_API_SWITCH, "switch removed");
    } catch (...) {
        return SAI_STATUS_FAILURE;
    }
    return SAI_STATUS_SUCCESS;
}

// Capacity and capability attributes orchagent reads at init and CRM polls
// (see the README's orchagent table). Real values from the backend where
// the SAI implements the feature; for features this SAI does not implement,
// an honest zero / empty list where orchagent then takes its "unsupported"
// path, otherwise ATTR_NOT_SUPPORTED (get_one's default).
static sai_status_t get_capacity(uint32_t i, sai_attribute_t& a, bool& handled)
{
    auto& c = ctx();
    handled = true;
    switch (a.id) {
    case SAI_SWITCH_ATTR_ACL_ENTRY_MINIMUM_PRIORITY:
    case SAI_SWITCH_ATTR_ACL_ENTRY_MAXIMUM_PRIORITY: {
        // The SDK's field entry priority range. AclOrch needs it to start;
        // ACL tables/entries themselves are not implemented (ACL API).
        ChipLimits l;
        sai_status_t st = c.be->chip_limits(l);
        if (st != SAI_STATUS_SUCCESS) return attr_backend_status(st, i);
        a.value.u32 = a.id == SAI_SWITCH_ATTR_ACL_ENTRY_MINIMUM_PRIORITY ? l.acl_prio_min : l.acl_prio_max;
        return SAI_STATUS_SUCCESS;
    }
    case SAI_SWITCH_ATTR_MAX_ACL_ACTION_COUNT:
        a.value.u32 = 0;   // no ACL actions: AclOrch records none as supported
        return SAI_STATUS_SUCCESS;
    case SAI_SWITCH_ATTR_ACL_STAGE_INGRESS:
    case SAI_SWITCH_ATTR_ACL_STAGE_EGRESS: {
        a.value.aclcapability.is_action_list_mandatory = false;
        std::vector<int32_t> none;
        return fill_list(a.value.aclcapability.action_list, none);
    }
    case SAI_SWITCH_ATTR_NUMBER_OF_ECMP_GROUPS:
    case SAI_SWITCH_ATTR_ECMP_MEMBERS:
    case SAI_SWITCH_ATTR_MAX_ECMP_MEMBER_COUNT: {
        // SDK ECMP limits: groups, members per group, member entries in all.
        ChipLimits l;
        sai_status_t st = c.be->chip_limits(l);
        if (st != SAI_STATUS_SUCCESS) return attr_backend_status(st, i);
        a.value.u32 = a.id == SAI_SWITCH_ATTR_NUMBER_OF_ECMP_GROUPS ? l.ecmp_groups
                    : a.id == SAI_SWITCH_ATTR_ECMP_MEMBERS ? l.ecmp_paths : l.ecmp_members;
        return SAI_STATUS_SUCCESS;
    }
    case SAI_SWITCH_ATTR_AVAILABLE_NEXT_HOP_GROUP_ENTRY:
    case SAI_SWITCH_ATTR_AVAILABLE_NEXT_HOP_GROUP_MEMBER_ENTRY: {
        // Free SDK ECMP groups / member entries. An empty (or unresolved)
        // next-hop group holds no SDK group yet.
        TableUsage u;
        sai_status_t st = c.be->table_usage(u);
        if (st != SAI_STATUS_SUCCESS) return attr_backend_status(st, i);
        a.value.u32 = a.id == SAI_SWITCH_ATTR_AVAILABLE_NEXT_HOP_GROUP_ENTRY ? u.ecmp_groups_free : u.ecmp_members_free;
        return SAI_STATUS_SUCCESS;
    }
    case SAI_SWITCH_ATTR_TOTAL_BUFFER_SIZE: {
        ChipLimits l;
        sai_status_t st = c.be->chip_limits(l);
        if (st != SAI_STATUS_SUCCESS) return attr_backend_status(st, i);
        a.value.u64 = l.buffer_bytes / 1024;   // KB
        return SAI_STATUS_SUCCESS;
    }
    case SAI_SWITCH_ATTR_AVAILABLE_IPV4_ROUTE_ENTRY:
    case SAI_SWITCH_ATTR_AVAILABLE_IPV6_ROUTE_ENTRY:
    case SAI_SWITCH_ATTR_AVAILABLE_IPV4_NEXTHOP_ENTRY:
    case SAI_SWITCH_ATTR_AVAILABLE_IPV6_NEXTHOP_ENTRY:
    case SAI_SWITCH_ATTR_AVAILABLE_IPV4_NEIGHBOR_ENTRY:
    case SAI_SWITCH_ATTR_AVAILABLE_IPV6_NEIGHBOR_ENTRY:
    case SAI_SWITCH_ATTR_AVAILABLE_FDB_ENTRY: {
        TableUsage u;
        sai_status_t st = c.be->table_usage(u);
        if (st != SAI_STATUS_SUCCESS) return attr_backend_status(st, i);
        switch (a.id) {
        case SAI_SWITCH_ATTR_AVAILABLE_IPV4_ROUTE_ENTRY: a.value.u32 = u.v4_routes_free; break;
        case SAI_SWITCH_ATTR_AVAILABLE_IPV6_ROUTE_ENTRY: a.value.u32 = u.v6_routes_free; break;
        // A next hop uses its neighbor's egress object.
        case SAI_SWITCH_ATTR_AVAILABLE_IPV4_NEXTHOP_ENTRY:
        case SAI_SWITCH_ATTR_AVAILABLE_IPV6_NEXTHOP_ENTRY: a.value.u32 = u.egress_free; break;
        // A neighbor takes a host entry (IPv6: 2) and an egress object.
        case SAI_SWITCH_ATTR_AVAILABLE_IPV4_NEIGHBOR_ENTRY: a.value.u32 = std::min(u.hosts_free, u.egress_free); break;
        case SAI_SWITCH_ATTR_AVAILABLE_IPV6_NEIGHBOR_ENTRY: a.value.u32 = std::min(u.hosts_free / 2, u.egress_free); break;
        default: a.value.u32 = u.fdb_free; break;
        }
        return SAI_STATUS_SUCCESS;
    }
    default:
        handled = false;
        return SAI_STATUS_SUCCESS;
    }
}

static sai_status_t get_one(uint32_t i, sai_attribute_t& a)
{
    auto& c = ctx();
    bool handled = false;
    sai_status_t cst = get_capacity(i, a, handled);
    if (handled) return cst;
    switch (a.id) {
    case SAI_SWITCH_ATTR_TYPE: a.value.s32 = SAI_SWITCH_TYPE_NPU; return SAI_STATUS_SUCCESS;
    case SAI_SWITCH_ATTR_INIT_SWITCH: a.value.booldata = true; return SAI_STATUS_SUCCESS;
    case SAI_SWITCH_ATTR_OPER_STATUS: a.value.s32 = SAI_SWITCH_OPER_STATUS_UP; return SAI_STATUS_SUCCESS;
    case SAI_SWITCH_ATTR_PORT_NUMBER:   // == NUMBER_OF_ACTIVE_PORTS
    case SAI_SWITCH_ATTR_MAX_NUMBER_OF_SUPPORTED_PORTS:
        a.value.u32 = (uint32_t)c.port_list.size();
        return SAI_STATUS_SUCCESS;
    case SAI_SWITCH_ATTR_PORT_LIST: return fill_list(a.value.objlist, c.port_list);
    case SAI_SWITCH_ATTR_CPU_PORT: a.value.oid = c.cpu_port; return SAI_STATUS_SUCCESS;
    case SAI_SWITCH_ATTR_DEFAULT_VIRTUAL_ROUTER_ID: a.value.oid = c.default_vr; return SAI_STATUS_SUCCESS;
    case SAI_SWITCH_ATTR_DEFAULT_VLAN_ID: a.value.oid = c.vid2vlan[1]; return SAI_STATUS_SUCCESS;
    case SAI_SWITCH_ATTR_DEFAULT_1Q_BRIDGE_ID: a.value.oid = c.default_bridge; return SAI_STATUS_SUCCESS;
    case SAI_SWITCH_ATTR_DEFAULT_STP_INST_ID: a.value.oid = c.default_stp; return SAI_STATUS_SUCCESS;
    case SAI_SWITCH_ATTR_DEFAULT_TRAP_GROUP: a.value.oid = c.default_trap_group; return SAI_STATUS_SUCCESS;
    case SAI_SWITCH_ATTR_SRC_MAC_ADDRESS: std::memcpy(a.value.mac, c.src_mac, 6); return SAI_STATUS_SUCCESS;
    case SAI_SWITCH_ATTR_PORT_MAX_MTU: a.value.u32 = PORT_MAX_MTU; return SAI_STATUS_SUCCESS;
    case SAI_SWITCH_ATTR_FDB_AGING_TIME: a.value.u32 = c.fdb_aging; return SAI_STATUS_SUCCESS;
    case SAI_SWITCH_ATTR_QOS_MAX_NUMBER_OF_TRAFFIC_CLASSES: a.value.u8 = 8; return SAI_STATUS_SUCCESS;
    case SAI_SWITCH_ATTR_NUMBER_OF_UNICAST_QUEUES: a.value.u32 = QUEUES_PER_PORT; return SAI_STATUS_SUCCESS;
    case SAI_SWITCH_ATTR_NUMBER_OF_MULTICAST_QUEUES: a.value.u32 = 0; return SAI_STATUS_SUCCESS;
    case SAI_SWITCH_ATTR_NUMBER_OF_QUEUES: a.value.u32 = QUEUES_PER_PORT; return SAI_STATUS_SUCCESS;
    case SAI_SWITCH_ATTR_SWITCH_HARDWARE_INFO: {
        std::vector<int8_t> none;
        return fill_list(a.value.s8list, none);
    }
    default:
        break;
    }
    if (is_temp_attr(a.id)) return switch_temp_get(i, a);
    if (is_notify_attr(a.id)) {
        auto it = c.notify.find(a.id);
        a.value.ptr = it == c.notify.end() ? nullptr : it->second;
        return SAI_STATUS_SUCCESS;
    }
    if (const StubDef* d = find_stub(switch_stubs, N_SWITCH_STUBS, a.id)) {
        auto it = c.switch_stub.find(a.id);
        if (it != c.switch_stub.end()) a.value = it->second;
        else stub_default(*d, a.value);
        return SAI_STATUS_SUCCESS;
    }
    return attr_not_supported(i);
}

static sai_status_t get_switch_attribute(_In_ sai_object_id_t switch_id, _In_ uint32_t attr_count,
                                         _Inout_ sai_attribute_t* attr_list)
{
    return guarded(SAI_API_SWITCH, __func__, [&]() -> sai_status_t {
        if (switch_id != ctx().switch_oid) return SAI_STATUS_INVALID_OBJECT_ID;
        if (attr_count && !attr_list) return SAI_STATUS_INVALID_PARAMETER;
        for (uint32_t i = 0; i < attr_count; i++) {
            sai_status_t st = get_one(i, attr_list[i]);
            if (st != SAI_STATUS_SUCCESS) return st;
        }
        return SAI_STATUS_SUCCESS;
    });
}

static sai_status_t set_switch_attribute(_In_ sai_object_id_t switch_id, _In_ const sai_attribute_t* attr)
{
    return guarded(SAI_API_SWITCH, __func__, [&]() -> sai_status_t {
        auto& c = ctx();
        if (switch_id != c.switch_oid) return SAI_STATUS_INVALID_OBJECT_ID;
        if (!attr) return SAI_STATUS_INVALID_PARAMETER;
        if (attr->id == SAI_SWITCH_ATTR_SRC_MAC_ADDRESS) {
            uint8_t old[6];
            std::memcpy(old, c.src_mac, 6);
            std::memcpy(c.src_mac, attr->value.mac, 6);
            sai_status_t st = l3_switch_mac_changed(old);
            if (st != SAI_STATUS_SUCCESS) std::memcpy(c.src_mac, old, 6);
            return st;
        }
        if (attr->id == SAI_SWITCH_ATTR_FDB_AGING_TIME) {
            sai_status_t st = c.be->l2_age_set(attr->value.u32);
            if (st == SAI_STATUS_SUCCESS) c.fdb_aging = attr->value.u32;
            return attr_backend_status(st, 0);
        }
        if (is_notify_attr(attr->id)) {
            set_notify(attr->id, attr->value.ptr);
            return SAI_STATUS_SUCCESS;
        }
        if (const StubDef* d = find_stub(switch_stubs, N_SWITCH_STUBS, attr->id)) {
            if (d->create_only) return attr_invalid(0);
            c.switch_stub[attr->id] = attr->value;
            OB_INFO(SAI_API_SWITCH, "switch attr %d stored (not programmed)", attr->id);
            return SAI_STATUS_SUCCESS;
        }
        OB_WARN(SAI_API_SWITCH, "set switch attr %d not supported", attr->id);
        return attr_not_supported(0);
    });
}

static sai_status_t get_switch_stats(_In_ sai_object_id_t, _In_ uint32_t, _In_ const sai_stat_id_t*, _Out_ uint64_t*)
{
    return SAI_STATUS_NOT_SUPPORTED;
}
static sai_status_t get_switch_stats_ext(_In_ sai_object_id_t, _In_ uint32_t, _In_ const sai_stat_id_t*,
                                         _In_ sai_stats_mode_t, _Out_ uint64_t*)
{
    return SAI_STATUS_NOT_SUPPORTED;
}
static sai_status_t clear_switch_stats(_In_ sai_object_id_t, _In_ uint32_t, _In_ const sai_stat_id_t*)
{
    return SAI_STATUS_NOT_SUPPORTED;
}

static sai_switch_api_t make_switch_api()
{
    sai_switch_api_t a;
    std::memset(&a, 0, sizeof(a));
    a.create_switch = create_switch;
    a.remove_switch = remove_switch;
    a.set_switch_attribute = set_switch_attribute;
    a.get_switch_attribute = get_switch_attribute;
    a.get_switch_stats = get_switch_stats;
    a.get_switch_stats_ext = get_switch_stats_ext;
    a.clear_switch_stats = clear_switch_stats;
    return a;
}
const sai_switch_api_t switch_api = make_switch_api();

} // namespace ob
