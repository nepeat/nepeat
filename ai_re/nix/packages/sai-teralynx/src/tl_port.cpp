// SPDX-License-Identifier: Apache-2.0
// Ports (tl_port.h).
#include "tl_port.h"

namespace ob {
namespace tlb {

std::vector<PortInfo> PortArea::infos()
{
    Guard g(c_.mu);
    std::vector<PortInfo> v;
    for (auto& s : c_.specs) {
        PortInfo p;
        p.hw = (int)s.devport;
        p.lanes = s.lanes;
        p.speed = s.speed;
        p.name = "dp" + std::to_string(s.devport);
        uint32_t an = 0;
        if (c_.dp_get_u32(s.devport, IFCS_DEVPORT_ATTR_AUTO_NEG, an) == SAI_STATUS_SUCCESS) p.autoneg = an != 0;
        v.push_back(p);
    }
    return v;
}

// Link state. SAI_TERALYNX_LINK_STATUS=read (default): one
// IFCS_DEVPORT_ATTR_LINK_STATUS read per get, ~5 us on the chip. (The
// "62 ms per OPER_STATUS get" of the first benchmark was the temperature
// read holding the libifcs lock, see SwitchArea::asic_temperatures; the
// oracle traces showed isai reading LINK_STATUS per get too.) "cached": the
// state from what we know instead, no IFCS call: every port is down after
// init (all admin-down before the link scan is registered), admin-down
// makes it down, the SDK's link scan events (on_link) carry every change
// after that; after admin-up and for any other unknown state the next get
// reads IFCS once. A transition the link scan does not report would stay
// wrong in "cached", hence not the default.
void PortArea::configure()
{
    link_cached_ = c_.key("SAI_TERALYNX_LINK_STATUS", "read") == "cached";
    stats_by_id_ = c_.key("SAI_TERALYNX_STATS", "ids") != "all";
    for (auto& l : link_) l = LINK_UNKNOWN;
    for (auto& s : c_.specs)
        if (s.devport < (uint32_t)MAX_DEVPORT) link_[s.devport] = LINK_DOWN;
    stats_.clear();
}

void PortArea::link_event(int hw, bool up)
{
    if (hw >= 0 && hw < MAX_DEVPORT) link_[hw] = up ? LINK_UP : LINK_DOWN;
}

sai_status_t PortArea::admin_set(int hw, bool up)
{
    if (!c_.front(hw)) return SAI_STATUS_INVALID_PORT_NUMBER;
    sai_status_t st = c_.dp_set(hw, attr_u32(IFCS_DEVPORT_ATTR_ADMIN_STATE, up ? 1 : 0), "admin_state");
    // Down: known. Up: read once on the next get (a down/up quicker than the
    // link scan interval would otherwise leave no event to correct us).
    if (st == SAI_STATUS_SUCCESS && hw < MAX_DEVPORT) link_[hw] = up ? LINK_UNKNOWN : LINK_DOWN;
    return st;
}

sai_status_t PortArea::link_get(int hw, bool& up)
{
    if (!c_.front(hw)) return SAI_STATUS_INVALID_PORT_NUMBER;
    if (link_cached_ && hw < MAX_DEVPORT) {
        int8_t l = link_[hw];
        if (l != LINK_UNKNOWN) {
            up = l == LINK_UP;
            return SAI_STATUS_SUCCESS;
        }
    }
    uint32_t v = 0;
    sai_status_t st = c_.dp_get_u32(hw, IFCS_DEVPORT_ATTR_LINK_STATUS, v);
    if (st == SAI_STATUS_SUCCESS) {
        up = v == IFCS_DEVPORT_LINK_STATUS_UP;
        if (hw < MAX_DEVPORT) {
            int8_t unknown = LINK_UNKNOWN;   // an event in between wins
            link_[hw].compare_exchange_strong(unknown, up ? LINK_UP : LINK_DOWN);
        }
    }
    return st;
}

sai_status_t PortArea::speed_get(int hw, uint32_t& mbps)
{
    const tl::PortSpec* s = c_.front(hw);
    if (!s) return SAI_STATUS_INVALID_PORT_NUMBER;
    uint32_t e = 0;
    sai_status_t st = c_.dp_get_u32(hw, IFCS_DEVPORT_ATTR_SPEED, e);
    if (st != SAI_STATUS_SUCCESS) return st;
    mbps = speed_from_enum(e);
    if (!mbps) mbps = s->speed;
    return SAI_STATUS_SUCCESS;
}

// Speed is fixed by the init yaml (devport create); IFCS has no safe live
// change we know of. The configured speed is accepted, anything else is not.
sai_status_t PortArea::speed_set(int hw, uint32_t mbps)
{
    uint32_t cur = 0;
    sai_status_t st = speed_get(hw, cur);
    if (st != SAI_STATUS_SUCCESS) return st;
    if (mbps == cur) return SAI_STATUS_SUCCESS;
    OB_WARN(SAI_API_PORT, "devport %d: speed %u -> %u not supported (set by the init config)", hw, cur, mbps);
    return SAI_STATUS_NOT_SUPPORTED;
}

sai_status_t PortArea::supported_speeds(int hw, std::vector<uint32_t>& out)
{
    uint32_t cur = 0;
    sai_status_t st = speed_get(hw, cur);
    if (st != SAI_STATUS_SUCCESS) return st;
    out.assign(1, cur);
    return SAI_STATUS_SUCCESS;
}

// Frame size limit: IFCS_SYSPORT_ATTR_MTU (u16, the SDK's per-sysport L2
// MTU; libifcs: ii_sysport_l2mtu_set). The frontend passes SAI MTU + 8;
// we add the Ethernet header (14) so a full tagged frame passes, clamped
// to 9290, the smallest upper bound in libifcs' range checks (I).
sai_status_t PortArea::frame_max_set(int hw, uint32_t bytes)
{
    if (!c_.front(hw)) return SAI_STATUS_INVALID_PORT_NUMBER;
    uint32_t v = bytes + 14;
    if (v > 9290) {
        OB_WARN(SAI_API_PORT, "devport %d: frame %u B clamped to the SDK L2 MTU limit 9290", hw, v);
        v = 9290;
    }
    return c_.sp_set(hw, attr_u16(IFCS_SYSPORT_ATTR_MTU, (uint16_t)v), "mtu");
}

// SAI RS: a port already on an RS mode (KR = RS528, KP = RS544, as the
// init yaml chose per port mode) keeps it; otherwise KP for >= 200G or
// PAM4 lanes (>= 50G per lane), KR for the rest (the vendor's
// dynamic_config.sh FEC tables follow this split). FC -> FC.
sai_status_t PortArea::fec_set(int hw, Fec f)
{
    const tl::PortSpec* s = c_.front(hw);
    if (!s) return SAI_STATUS_INVALID_PORT_NUMBER;
    uint32_t cur = 0xffffffffu;
    c_.dp_get_u32(hw, IFCS_DEVPORT_ATTR_FEC_MODE, cur);
    uint32_t lane = s->speed / (uint32_t)s->lanes.size();
    uint32_t want;
    if (f == Fec::None) want = IFCS_DEVPORT_FEC_MODE_NONE;
    else if (f == Fec::Fc) want = IFCS_DEVPORT_FEC_MODE_FC;
    else if (cur == IFCS_DEVPORT_FEC_MODE_KR || cur == IFCS_DEVPORT_FEC_MODE_KP) want = cur;
    else if (s->speed >= 200000 || lane >= 50000) want = IFCS_DEVPORT_FEC_MODE_KP;
    else want = IFCS_DEVPORT_FEC_MODE_KR;
    if (cur == want) return SAI_STATUS_SUCCESS;
    return c_.dp_set(hw, attr_u32(IFCS_DEVPORT_ATTR_FEC_MODE, want), "fec_mode");
}

sai_status_t PortArea::autoneg_set(int hw, bool on)
{
    if (!c_.front(hw)) return SAI_STATUS_INVALID_PORT_NUMBER;
    uint32_t cur = 2;
    if (c_.dp_get_u32(hw, IFCS_DEVPORT_ATTR_AUTO_NEG, cur) == SAI_STATUS_SUCCESS && (cur != 0) == on)
        return SAI_STATUS_SUCCESS;
    return c_.dp_set(hw, attr_u32(IFCS_DEVPORT_ATTR_AUTO_NEG, on ? 1 : 0), "auto_neg");
}

// SAI MAC loopback -> IFCS PCS loopback, PHY -> PMA (I: nearest layers).
sai_status_t PortArea::loopback_set(int hw, Loopback lb)
{
    if (!c_.front(hw)) return SAI_STATUS_INVALID_PORT_NUMBER;
    uint32_t v = lb == Loopback::Mac ? IFCS_DEVPORT_LOOPBACK_PCS
               : lb == Loopback::Phy ? IFCS_DEVPORT_LOOPBACK_PMA : IFCS_DEVPORT_LOOPBACK_NONE;
    return c_.dp_set(hw, attr_u32(IFCS_DEVPORT_ATTR_LOOPBACK, v), "loopback");
}

sai_status_t PortArea::pvid_set(int hw, uint16_t vid)
{
    if (!c_.front(hw)) return SAI_STATUS_INVALID_PORT_NUMBER;
    return c_.sp_set(hw, attr_u16(IFCS_SYSPORT_ATTR_DEFAULT_CVID, vid), "default_cvid");
}

sai_status_t PortArea::learn_set(int hw, Learn mode)
{
    if (!c_.front(hw)) return SAI_STATUS_INVALID_PORT_NUMBER;
    if (mode == Learn::Drop || mode == Learn::Cpu) return c_.not_impl("learning mode drop/cpu");
    return c_.sp_set(hw, attr_data(IFCS_SYSPORT_ATTR_LEARN_ENABLE, mode == Learn::Hw), "learn_enable");
}

// The IFCS counters behind the frontend's PortStats.
static const int k_stat_ids[] = {
    IFCS_DEVPORT_STATS_ID_RX_BYTES_ALL, IFCS_DEVPORT_STATS_ID_RX_FRAMES_UNICAST, IFCS_DEVPORT_STATS_ID_RX_FRAMES_MULTICAST,
    IFCS_DEVPORT_STATS_ID_RX_FRAMES_BROADCAST, IFCS_DEVPORT_STATS_ID_RX_FRAMES_DROP, IFCS_DEVPORT_STATS_ID_RX_FRAMES_ERR_ANY,
    IFCS_DEVPORT_STATS_ID_TX_BYTES_OK, IFCS_DEVPORT_STATS_ID_TX_FRAMES_UNICAST, IFCS_DEVPORT_STATS_ID_TX_FRAMES_MULTICAST,
    IFCS_DEVPORT_STATS_ID_TX_FRAMES_BROADCAST, IFCS_DEVPORT_STATS_ID_TX_FRAMES_DROP, IFCS_DEVPORT_STATS_ID_TX_FRAMES_ERR_ANY,
};
static constexpr uint32_t N_STAT_IDS = sizeof(k_stat_ids) / sizeof(k_stat_ids[0]);

// Counters: one ifcs_devport_stats_get per port, cached 200 ms because the
// frontend asks one counter at a time. SAI_TERALYNX_STATS=ids (default):
// only the 12 counters we map, as an id list (values in list order; the
// libifcs API form, I); "all": the vendor CLI's form (count =
// IFCS_DEVPORT_STATS_ID_ALL, results indexed by id), every MIB counter read.
sai_status_t PortArea::stat_get(int hw, PortStat s, uint64_t& v)
{
    Guard g(c_.mu);
    if (!c_.front(hw)) return SAI_STATUS_INVALID_PORT_NUMBER;
    StatCache& sc = stats_[hw];
    auto now = std::chrono::steady_clock::now();
    if (!sc.valid || now - sc.when > std::chrono::milliseconds(200)) {
        int ids[IFCS_DEVPORT_STATS_ID_ALL];
        std::memset(ids, 0, sizeof(ids));
        uint64_t vals[IFCS_DEVPORT_STATS_ID_ALL];
        std::memset(vals, 0, sizeof(vals));
        uint32_t n = 0;
        uint32_t count = IFCS_DEVPORT_STATS_ID_ALL;
        if (stats_by_id_) {
            count = N_STAT_IDS;
            std::memcpy(ids, k_stat_ids, sizeof(k_stat_ids));
        }
        int r = IFCS_STATUS_REASON(c_.L.ifcs_devport_stats_get(NODE, (ifcs_devport_t)hw, count, ids, vals, &n));
        if (r != IFCS_SUCCESS && r != IFCS_UNSUPPORTED) return c_.fail(r, "devport stats get");
        if (stats_by_id_) {
            std::memset(sc.v, 0, sizeof(sc.v));
            for (uint32_t i = 0; i < N_STAT_IDS; i++) sc.v[k_stat_ids[i]] = vals[i];
        } else {
            std::memcpy(sc.v, vals, sizeof(sc.v));
        }
        sc.valid = true;
        sc.when = now;
    }
    const uint64_t* x = sc.v;
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
    case PortStat::OutOctets: v = x[IFCS_DEVPORT_STATS_ID_TX_BYTES_OK]; break;   // as isai (RX side: BYTES_ALL)
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

sai_status_t PortArea::stat_clear(int hw)
{
    Guard g(c_.mu);
    if (!c_.front(hw)) return SAI_STATUS_INVALID_PORT_NUMBER;
    int ids[IFCS_DEVPORT_STATS_ID_MAX_COUNT];
    for (int i = 0; i < IFCS_DEVPORT_STATS_ID_MAX_COUNT; i++) ids[i] = i;
    int r = IFCS_STATUS_REASON(c_.L.ifcs_devport_stats_clear(NODE, (ifcs_devport_t)hw, IFCS_DEVPORT_STATS_ID_MAX_COUNT, ids));
    stats_[hw].valid = false;
    if (r != IFCS_SUCCESS) return c_.fail(r, "devport stats clear");
    return SAI_STATUS_SUCCESS;
}

void PortArea::all_down()
{
    for (auto& s : c_.specs) {
        ifcs_attr_t a = attr_u32(IFCS_DEVPORT_ATTR_ADMIN_STATE, 0);
        c_.L.ifcs_devport_attr_set(NODE, s.devport, 1, &a);
    }
}

// Only translate and queue (the frontend's callback pushes to its queue).
void PortArea::on_link(ifcs_node_id_t, void* user, ifcs_linkscan_event_info_t* info)
{
    auto* c = static_cast<Core*>(user);
    if (!c || !info || !c->link) return;
    if (!c->front((int)info->devport_id)) return;
    bool up = info->port_state == IFCS_DEVPORT_LINK_STATUS_UP;
    c->port->link_event((int)info->devport_id, up);
    c->link((int)info->devport_id, up);
}

} // namespace tlb
} // namespace ob
