// SPDX-License-Identifier: Apache-2.0
// The real backend: Broadcom OpenBCM SDK via sdk_glue.c (libsdk.so).
// Safety rules encoded here and in the SAI layer:
//  - no SAI_INIT_CONFIG_FILE, or an unreadable one: refuse to start (the SDK
//    would otherwise come up with its compiled-in defaults);
//  - stale KNET netifs/filters are destroyed at init (they survive SDK
//    restarts and would steal packets for the wrong netdev);
//  - front-panel ports are disabled right after init by the SAI layer
//    (switch_build_objects); nothing here enables a port by itself.
#include "backend.h"
#include "sai_internal.h"
#include "sdk_glue.h"

#include <cstdio>
#include <cstdlib>
#include <unistd.h>

namespace ob {

static sai_status_t map_rv(int rv)
{
    switch (rv) {
    case OB_E_NONE: return SAI_STATUS_SUCCESS;
    case OB_E_MEMORY: return SAI_STATUS_NO_MEMORY;
    case OB_E_UNIT: case OB_E_PARAM: case OB_E_CONFIG: return SAI_STATUS_INVALID_PARAMETER;
    case OB_E_EMPTY: case OB_E_NOT_FOUND: return SAI_STATUS_ITEM_NOT_FOUND;
    case OB_E_FULL: return SAI_STATUS_TABLE_FULL;
    case OB_E_EXISTS: return SAI_STATUS_ITEM_ALREADY_EXISTS;
    case OB_E_BUSY: return SAI_STATUS_OBJECT_IN_USE;
    case OB_E_BADID: return SAI_STATUS_INVALID_OBJECT_ID;
    case OB_E_RESOURCE: return SAI_STATUS_INSUFFICIENT_RESOURCES;
    case OB_E_UNAVAIL: return SAI_STATUS_NOT_SUPPORTED;
    case OB_E_INIT: return SAI_STATUS_UNINITIALIZED;
    case OB_E_PORT: return SAI_STATUS_INVALID_PORT_NUMBER;
    default: return SAI_STATUS_FAILURE;
    }
}

#define RV(call)                                                                           \
    do {                                                                                   \
        int _rv = (call);                                                                  \
        if (_rv < 0) {                                                                     \
            OB_LOG(SAI_API_UNSPECIFIED, _rv == OB_E_UNAVAIL ? SAI_LOG_LEVEL_INFO : SAI_LOG_LEVEL_ERROR, \
                   "%s: %s", #call, ob_sdk_errmsg(_rv));                                   \
            return map_rv(_rv);                                                            \
        }                                                                                  \
    } while (0)

class OpenbcmBackend : public Backend {
public:
    const char* name() const override { return "openbcm"; }
    bool is_fake() const override { return false; }

    sai_status_t init(const BackendConfig& cfg, LinkCallback link_cb, FdbCallback fdb_cb) override
    {
        std::string config = cfg.get("SAI_INIT_CONFIG_FILE");
        std::string rc = cfg.get("SAI_OPENBCM_RC_FILE");
        unit_ = std::atoi(cfg.get("SAI_OPENBCM_UNIT", "0").c_str());
        if (config.empty()) {
            OB_ERR(SAI_API_SWITCH, "SAI_INIT_CONFIG_FILE not set in the profile; refusing to start the SDK "
                                   "with compiled-in defaults");
            return SAI_STATUS_FAILURE;
        }
        if (access(config.c_str(), R_OK) != 0) {
            OB_ERR(SAI_API_SWITCH, "SDK config file %s missing or unreadable; refusing to start", config.c_str());
            return SAI_STATUS_FAILURE;
        }
        link_ = std::move(link_cb);
        fdb_ = std::move(fdb_cb);
        char err[256] = {0};
        OB_NOTICE(SAI_API_SWITCH, "SDK init: unit %d, config %s, rc %s", unit_, config.c_str(),
                  rc.empty() ? "(none)" : rc.c_str());
        int rv = ob_sdk_init(unit_, config.c_str(), rc.c_str(), err, sizeof(err));
        if (rv < 0) {
            OB_ERR(SAI_API_SWITCH, "SDK init failed: %s (%s)", err, ob_sdk_errmsg(rv));
            return map_rv(rv);
        }
        OB_NOTICE(SAI_API_SWITCH, "SDK init: %s", err);
        RV(ob_sdk_register_callbacks(unit_, &OpenbcmBackend::on_link, &OpenbcmBackend::on_l2, this));
        int cpu = -1, drop = -1;
        RV(ob_sdk_l3_egress_special(unit_, 0, &cpu));
        RV(ob_sdk_l3_egress_special(unit_, 1, &drop));
        cpu_egress_ = cpu;
        drop_egress_ = drop;
        return SAI_STATUS_SUCCESS;
    }

    void shutdown() override
    {
        ob_sdk_shutdown(unit_);
        link_ = nullptr;
        fdb_ = nullptr;
    }

    sai_status_t run_shell(const std::string& cmd) override
    {
        RV(ob_sdk_shell(unit_, cmd.c_str()));
        return SAI_STATUS_SUCCESS;
    }

    int cpu_port() const override { return ob_sdk_cpu_port(unit_); }

    std::vector<PortInfo> ports() override
    {
        std::vector<ob_port_info_t> raw(512);
        int n = ob_sdk_ports(unit_, raw.data(), (int)raw.size());
        std::vector<PortInfo> v;
        if (n < 0) {
            OB_ERR(SAI_API_SWITCH, "port config: %s", ob_sdk_errmsg(n));
            return v;
        }
        for (int i = 0; i < n; i++) {
            PortInfo p;
            p.hw = raw[i].port;
            for (int l = 0; l < raw[i].lanes; l++) p.lanes.push_back((uint32_t)(raw[i].phys + l));
            p.speed = (uint32_t)raw[i].speed;
            p.name = raw[i].name;
            int an = 0;
            if (ob_sdk_port_autoneg_get(unit_, p.hw, &an) >= 0) p.autoneg = an != 0;
            v.push_back(p);
        }
        return v;
    }

    std::string hardware_info() override
    {
        char b[64] = {0};
        ob_sdk_hw_info(unit_, b, sizeof(b));
        return b;
    }

    sai_status_t port_admin_set(int hw, bool up) override { RV(ob_sdk_port_enable_set(unit_, hw, up)); return SAI_STATUS_SUCCESS; }
    sai_status_t port_link_get(int hw, bool& up) override
    {
        int u = 0;
        RV(ob_sdk_port_link_get(unit_, hw, &u));
        up = u != 0;
        return SAI_STATUS_SUCCESS;
    }
    sai_status_t port_speed_set(int hw, uint32_t mbps) override { RV(ob_sdk_port_speed_set(unit_, hw, (int)mbps)); return SAI_STATUS_SUCCESS; }
    sai_status_t port_speed_get(int hw, uint32_t& mbps) override
    {
        int s = 0;
        RV(ob_sdk_port_speed_get(unit_, hw, &s));
        mbps = (uint32_t)s;
        return SAI_STATUS_SUCCESS;
    }
    sai_status_t port_supported_speeds(int hw, std::vector<uint32_t>& out) override
    {
        uint32_t b[16];
        int n = ob_sdk_port_speeds(unit_, hw, b, 16);
        if (n < 0) return map_rv(n);
        out.assign(b, b + n);
        return SAI_STATUS_SUCCESS;
    }
    sai_status_t port_frame_max_set(int hw, uint32_t bytes) override { RV(ob_sdk_port_frame_max_set(unit_, hw, (int)bytes)); return SAI_STATUS_SUCCESS; }
    sai_status_t port_fec_set(int hw, Fec f) override
    {
        RV(ob_sdk_port_fec_set(unit_, hw, f == Fec::Rs ? 1 : f == Fec::Fc ? 2 : 0));
        return SAI_STATUS_SUCCESS;
    }
    sai_status_t port_autoneg_set(int hw, bool on) override { RV(ob_sdk_port_autoneg_set(unit_, hw, on)); return SAI_STATUS_SUCCESS; }
    sai_status_t port_loopback_set(int hw, Loopback lb) override
    {
        RV(ob_sdk_port_loopback_set(unit_, hw, lb == Loopback::Mac ? 1 : lb == Loopback::Phy ? 2 : 0));
        return SAI_STATUS_SUCCESS;
    }
    sai_status_t port_pvid_set(int hw, uint16_t vid) override { RV(ob_sdk_port_pvid_set(unit_, hw, vid)); return SAI_STATUS_SUCCESS; }
    sai_status_t port_learn_set(int hw, Learn m) override
    {
        RV(ob_sdk_port_learn_set(unit_, hw, m == Learn::Disable ? 1 : m == Learn::Drop ? 2 : m == Learn::Cpu ? 3 : 0));
        return SAI_STATUS_SUCCESS;
    }
    sai_status_t port_stat_get(int hw, PortStat s, uint64_t& v) override { RV(ob_sdk_port_stat_get(unit_, hw, (int)s, &v)); return SAI_STATUS_SUCCESS; }
    sai_status_t port_stat_clear(int hw) override { RV(ob_sdk_port_stat_clear(unit_, hw)); return SAI_STATUS_SUCCESS; }

    sai_status_t vlan_create(uint16_t vid) override { RV(ob_sdk_vlan_create(unit_, vid)); return SAI_STATUS_SUCCESS; }
    sai_status_t vlan_destroy(uint16_t vid) override { RV(ob_sdk_vlan_destroy(unit_, vid)); return SAI_STATUS_SUCCESS; }
    sai_status_t vlan_port_add(uint16_t vid, int hw, bool untagged) override { RV(ob_sdk_vlan_port_add(unit_, vid, hw, untagged)); return SAI_STATUS_SUCCESS; }
    sai_status_t vlan_port_remove(uint16_t vid, int hw) override { RV(ob_sdk_vlan_port_remove(unit_, vid, hw)); return SAI_STATUS_SUCCESS; }
    sai_status_t vlan_members(uint16_t vid, std::vector<std::pair<int, bool>>& out) override
    {
        int ports[512], untag[512];
        int n = ob_sdk_vlan_members(unit_, vid, ports, untag, 512);
        if (n < 0) return map_rv(n);
        out.clear();
        for (int i = 0; i < n; i++) out.push_back({ports[i], untag[i] != 0});
        return SAI_STATUS_SUCCESS;
    }

    sai_status_t l2_add(const uint8_t mac[6], uint16_t vid, int hw, bool st, bool discard) override { RV(ob_sdk_l2_add(unit_, mac, vid, hw, st, discard)); return SAI_STATUS_SUCCESS; }
    sai_status_t l2_delete(const uint8_t mac[6], uint16_t vid) override { RV(ob_sdk_l2_delete(unit_, mac, vid)); return SAI_STATUS_SUCCESS; }
    sai_status_t l2_flush(int hw, int vid, bool statics, bool dynamics) override { RV(ob_sdk_l2_flush(unit_, hw, vid, statics, dynamics)); return SAI_STATUS_SUCCESS; }
    sai_status_t l2_age_set(uint32_t seconds) override { RV(ob_sdk_l2_age_set(unit_, (int)seconds)); return SAI_STATUS_SUCCESS; }

    sai_status_t netif_create(const std::string& ifname, int hw, uint16_t vid, const uint8_t mac[6], bool keep_tag,
                              int& netif_id, int& filter_id) override
    {
        RV(ob_sdk_netif_create(unit_, ifname.c_str(), hw, vid, mac, keep_tag, &netif_id, &filter_id));
        return SAI_STATUS_SUCCESS;
    }
    sai_status_t netif_destroy(int netif_id, int filter_id) override { RV(ob_sdk_netif_destroy(unit_, netif_id, filter_id)); return SAI_STATUS_SUCCESS; }
    sai_status_t netif_carrier_set(const std::string& ifname, bool up) override
    {
        // bcm-knet's procfs: "<ifname>=up|down" sets the netdev carrier.
        FILE* f = std::fopen("/proc/bcm/knet/link", "w");
        if (!f) {
            OB_ERR(SAI_API_HOSTIF, "cannot open /proc/bcm/knet/link (is linux-bcm-knet loaded?)");
            return SAI_STATUS_FAILURE;
        }
        std::fprintf(f, "%s=%s\n", ifname.c_str(), up ? "up" : "down");
        bool ok = std::fclose(f) == 0;
        return ok ? SAI_STATUS_SUCCESS : SAI_STATUS_FAILURE;
    }
    sai_status_t netif_set_keep_tag(int netif_id, int hw, bool keep_tag, int& filter_id) override
    {
        RV(ob_sdk_netif_filter_set(unit_, netif_id, hw, keep_tag, &filter_id));
        return SAI_STATUS_SUCCESS;
    }

    sai_status_t trap_set(TrapKind kind, bool to_cpu) override { RV(ob_sdk_trap_set(unit_, (int)kind, to_cpu)); return SAI_STATUS_SUCCESS; }

    sai_status_t l3_intf_create(const uint8_t mac[6], uint16_t vid, uint32_t mtu, int& id) override { RV(ob_sdk_l3_intf_create(unit_, mac, vid, (int)mtu, &id)); return SAI_STATUS_SUCCESS; }
    sai_status_t l3_intf_update(int id, const uint8_t mac[6], uint16_t vid, uint32_t mtu) override { RV(ob_sdk_l3_intf_update(unit_, id, mac, vid, (int)mtu)); return SAI_STATUS_SUCCESS; }
    sai_status_t l3_intf_destroy(int id) override { RV(ob_sdk_l3_intf_destroy(unit_, id)); return SAI_STATUS_SUCCESS; }
    sai_status_t l3_station_add(const uint8_t mac[6], int& id) override { RV(ob_sdk_l3_station_add(unit_, mac, &id)); return SAI_STATUS_SUCCESS; }
    sai_status_t l3_station_delete(int id) override { RV(ob_sdk_l3_station_delete(unit_, id)); return SAI_STATUS_SUCCESS; }
    sai_status_t l3_egress_create(int intf, const uint8_t dmac[6], int hw, uint16_t vid, int& id) override { RV(ob_sdk_l3_egress_create(unit_, intf, dmac, hw, vid, &id)); return SAI_STATUS_SUCCESS; }
    sai_status_t l3_egress_update(int id, int intf, const uint8_t dmac[6], int hw, uint16_t vid) override { RV(ob_sdk_l3_egress_update(unit_, id, intf, dmac, hw, vid)); return SAI_STATUS_SUCCESS; }
    sai_status_t l3_egress_destroy(int id) override
    {
        if (id == cpu_egress_ || id == drop_egress_) return SAI_STATUS_INVALID_PARAMETER;
        RV(ob_sdk_l3_egress_destroy(unit_, id));
        return SAI_STATUS_SUCCESS;
    }
    sai_status_t l3_egress_cpu(int& id) override { id = cpu_egress_; return id >= 0 ? SAI_STATUS_SUCCESS : SAI_STATUS_FAILURE; }
    sai_status_t l3_egress_drop(int& id) override { id = drop_egress_; return id >= 0 ? SAI_STATUS_SUCCESS : SAI_STATUS_FAILURE; }
    sai_status_t l3_host_add(const IpAddr& ip, int egress) override { RV(ob_sdk_l3_host_add(unit_, ip.v6, ip.addr, egress)); return SAI_STATUS_SUCCESS; }
    sai_status_t l3_host_delete(const IpAddr& ip) override { RV(ob_sdk_l3_host_delete(unit_, ip.v6, ip.addr)); return SAI_STATUS_SUCCESS; }
    sai_status_t l3_route_add(const IpPrefix& p, int egress, bool replace) override
    {
        RV(ob_sdk_l3_route_add(unit_, p.addr.v6, p.addr.addr, p.mask.addr, egress, egress == drop_egress_, replace));
        return SAI_STATUS_SUCCESS;
    }
    sai_status_t l3_route_delete(const IpPrefix& p) override { RV(ob_sdk_l3_route_delete(unit_, p.addr.v6, p.addr.addr, p.mask.addr)); return SAI_STATUS_SUCCESS; }

private:
    // Called from SDK threads (linkscan, L2X): only queue.
    static void on_link(void* self, int port, int up)
    {
        auto* b = static_cast<OpenbcmBackend*>(self);
        if (b->link_) b->link_(port, up != 0);
    }
    static void on_l2(void* self, const uint8_t mac[6], int vid, int port, int learned)
    {
        auto* b = static_cast<OpenbcmBackend*>(self);
        if (!b->fdb_) return;
        FdbEvent e;
        std::memcpy(e.mac, mac, 6);
        e.vid = (uint16_t)vid;
        e.hw_port = port;
        e.learned = learned != 0;
        b->fdb_(e);
    }

    int unit_ = 0;
    LinkCallback link_;
    FdbCallback fdb_;
    int cpu_egress_ = -1, drop_egress_ = -1;
};

Backend* make_openbcm_backend() { return new OpenbcmBackend(); }

} // namespace ob
