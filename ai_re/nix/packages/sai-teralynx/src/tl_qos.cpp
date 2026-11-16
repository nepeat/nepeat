// SPDX-License-Identifier: Apache-2.0
// QoS and buffers (tl_qos.h): what each SAI QoS object becomes in IFCS.
#include "tl_qos.h"

#include <cstdio>

namespace ob {
namespace tlb {

static constexpr uint32_t WRED_OFF_CELLS = 0xb2a0;   // an unused colour: min = max = this, 100 % (isai)

uint32_t QosArea::cell_bytes()
{
    if (cell_) return cell_;
    ifcs_attr_t a = attr_of(IFCS_NODE_ATTR_DTM_CELL_SIZE);
    uint32_t n = 0;
    if (IFCS_STATUS_REASON(c_.L.ifcs_node_attr_get(NODE, 1, &a, &n)) == IFCS_SUCCESS && a.value.u16)
        cell_ = a.value.u16;
    else {
        cell_ = 224;   // what isai reads on this chip
        c_.log_once("qos:cell", "DTM cell size unreadable; using 224 bytes");
    }
    return cell_;
}

ifcs_queue_t QosArea::queue_key(int hw, int q)
{
    ifcs_queue_t k;
    std::memset(&k, 0, sizeof(k));
    c_.L.ifcs_queue_t_init(&k);
    c_.L.ifcs_queue_t_devport_set(&k, (ifcs_devport_t)hw);
    c_.L.ifcs_queue_t_queue_id_set(&k, (uint32_t)q);
    return k;
}

sai_status_t QosArea::queue_set(int hw, int q, std::vector<ifcs_attr_t> a, const char* what)
{
    if (!c_.front(hw) || q < 0 || q > 7) return SAI_STATUS_INVALID_PARAMETER;
    ifcs_queue_t k = queue_key(hw, q);
    return c_.call(c_.L.ifcs_queue_attr_set(NODE, &k, (uint32_t)a.size(), a.data()), what);
}

// ------------------------------------------------------------- QoS maps
static bool programmed(QosMapKind k) { return k == QosMapKind::DscpToTc || k == QosMapKind::Dot1pToTc; }

sai_status_t QosArea::map_info(const Map& m, ifcs_attr_t& a, std::vector<ifcs_qos_map_t>& store)
{
    store.assign(m.e.size(), ifcs_qos_map_t());
    for (size_t i = 0; i < m.e.size(); i++) {
        ifcs_qos_map_t& x = store[i];
        std::memset(&x, 0, sizeof(x));
        if (m.kind == QosMapKind::DscpToTc) x.map_src.dscp = m.e[i].key;
        else x.map_src.dot1p_cfi.dot1p = m.e[i].key;   // CFI 0 (isai: 8 entries)
        x.map_value.tc_dp.tc = m.e[i].value;
        x.map_value.tc_dp.dp = 0;                        // green: the colour maps are separate SAI maps
    }
    ifcs_qos_map_info_t info;
    std::memset(&info, 0, sizeof(info));
    c_.L.ifcs_qos_map_info_t_init(&info);
    info.type = m.kind == QosMapKind::DscpToTc ? IFCS_QOS_MAP_TYPE_DSCP_TO_TC_DP : IFCS_QOS_MAP_TYPE_DOT1P_CFI_TO_TC_DP;
    info.list.count = (uint32_t)store.size();
    info.list.arr = store.data();
    a = attr_of(IFCS_QOS_MAP_ATTR_QOS_MAP_INFO);
    std::memcpy(&a.value, &info, sizeof(info));
    return SAI_STATUS_SUCCESS;
}

static bool identity(const std::vector<QosMapEntry>& e)
{
    for (auto& x : e)
        if (x.value != x.key) return false;
    return true;
}
static bool all_green(const std::vector<QosMapEntry>& e)
{
    for (auto& x : e)
        if (x.value_color) return false;
    return true;
}

// Kinds that program nothing: TC -> PG and the PFC priority maps (as isai),
// TC -> queue when it is the chip's identity, the colour maps when every
// colour is green.
static sai_status_t check_kind(Core& c, QosMapKind k, const std::vector<QosMapEntry>& e)
{
    switch (k) {
    case QosMapKind::DscpToTc: case QosMapKind::Dot1pToTc:
    case QosMapKind::TcToPg: case QosMapKind::PfcPrioToQueue: case QosMapKind::PfcPrioToPg:
        return SAI_STATUS_SUCCESS;
    case QosMapKind::TcToQueue:
        if (identity(e)) return SAI_STATUS_SUCCESS;
        c.log_once("qos:tcq", "TC_TO_QUEUE: only the identity map (the chip's fixed TC = queue) is supported");
        return SAI_STATUS_NOT_SUPPORTED;
    case QosMapKind::DscpToColor: case QosMapKind::Dot1pToColor:
        if (all_green(e)) return SAI_STATUS_SUCCESS;
        c.log_once("qos:color", "DSCP/DOT1P_TO_COLOR: only all-green maps are supported");
        return SAI_STATUS_NOT_SUPPORTED;
    default:
        c.log_once("qos:remark", "TC_AND_COLOR_TO_DSCP/DOT1P (remarking) is not implemented");
        return SAI_STATUS_NOT_SUPPORTED;
    }
}

sai_status_t QosArea::map_create(QosMapKind k, const std::vector<QosMapEntry>& e, int& id)
{
    Guard g(c_.mu);
    sai_status_t st = check_kind(c_, k, e);
    if (st != SAI_STATUS_SUCCESS) return st;
    Map m{k, e, IFCS_NULL_HANDLE};
    if (programmed(k)) {
        ifcs_attr_t a;
        std::vector<ifcs_qos_map_t> store;
        map_info(m, a, store);
        st = c_.call(c_.L.ifcs_qos_map_create(NODE, &m.h, 1, &a), "qos map create");
        if (st != SAI_STATUS_SUCCESS) return st;
    }
    id = next_++;
    maps_[id] = m;
    return SAI_STATUS_SUCCESS;
}

sai_status_t QosArea::map_update(int id, const std::vector<QosMapEntry>& e)
{
    Guard g(c_.mu);
    auto it = maps_.find(id);
    if (it == maps_.end()) return SAI_STATUS_ITEM_NOT_FOUND;
    sai_status_t st = check_kind(c_, it->second.kind, e);
    if (st != SAI_STATUS_SUCCESS) return st;
    Map m = it->second;
    m.e = e;
    if (m.h != IFCS_NULL_HANDLE) {
        ifcs_attr_t a;
        std::vector<ifcs_qos_map_t> store;
        map_info(m, a, store);
        st = c_.call(c_.L.ifcs_qos_map_attr_set(NODE, m.h, 1, &a), "qos map set");
        if (st != SAI_STATUS_SUCCESS) return st;
    }
    it->second = m;
    return SAI_STATUS_SUCCESS;
}

sai_status_t QosArea::map_destroy(int id)
{
    Guard g(c_.mu);
    auto it = maps_.find(id);
    if (it == maps_.end()) return SAI_STATUS_ITEM_NOT_FOUND;
    for (auto& p : port_key_)
        if (p.second.first == id || p.second.second == id) return SAI_STATUS_OBJECT_IN_USE;
    if (it->second.h != IFCS_NULL_HANDLE) {
        sai_status_t st = c_.call(c_.L.ifcs_qos_map_delete(NODE, it->second.h), "qos map delete");
        if (st != SAI_STATUS_SUCCESS) return st;
    }
    maps_.erase(it);
    return SAI_STATUS_SUCCESS;
}

sai_status_t QosArea::group_get(const Key& k, ifcs_handle_t& h)
{
    auto it = groups_.find(k);
    if (it != groups_.end()) {
        it->second.refs++;
        h = it->second.h;
        return SAI_STATUS_SUCCESS;
    }
    std::vector<ifcs_attr_t> a = {attr_u32(IFCS_QOS_MAP_GROUP_ATTR_ACTION_TYPE, IFCS_QOS_ACTION_TYPE_MAP)};
    if (k.first >= 0) a.push_back(attr_handle(IFCS_QOS_MAP_GROUP_ATTR_DSCP_TO_TC_DP, maps_[k.first].h));
    if (k.second >= 0) a.push_back(attr_handle(IFCS_QOS_MAP_GROUP_ATTR_DOT1P_CFI_TO_TC_DP, maps_[k.second].h));
    Group gr;
    sai_status_t st = c_.call(c_.L.ifcs_qos_map_group_create(NODE, &gr.h, (uint32_t)a.size(), a.data()), "qos map group create");
    if (st != SAI_STATUS_SUCCESS) return st;
    gr.refs = 1;
    groups_[k] = gr;
    h = gr.h;
    return SAI_STATUS_SUCCESS;
}

void QosArea::group_put(const Key& k)
{
    auto it = groups_.find(k);
    if (it == groups_.end() || --it->second.refs > 0) return;
    c_.call(c_.L.ifcs_qos_map_group_delete(NODE, it->second.h), "qos map group delete");
    groups_.erase(it);
}

sai_status_t QosArea::port_map_set(int hw, QosMapKind k, int id)
{
    Guard g(c_.mu);
    if (!c_.front(hw)) return SAI_STATUS_INVALID_PORT_NUMBER;
    if (id >= 0 && !maps_.count(id)) return SAI_STATUS_ITEM_NOT_FOUND;
    if (id >= 0 && maps_[id].kind != k) return SAI_STATUS_INVALID_PARAMETER;
    if (!programmed(k)) return SAI_STATUS_SUCCESS;   // nothing per port (check_kind)
    Key old = port_key_.count(hw) ? port_key_[hw] : Key(-1, -1);
    Key now = old;
    (k == QosMapKind::DscpToTc ? now.first : now.second) = id;
    if (now == old) return SAI_STATUS_SUCCESS;
    ifcs_handle_t h = IFCS_NULL_HANDLE;
    bool any = now.first >= 0 || now.second >= 0;
    if (any) {
        sai_status_t st = group_get(now, h);
        if (st != SAI_STATUS_SUCCESS) return st;
    }
    ifcs_attr_t a[2] = {attr_handle(IFCS_SYSPORT_ATTR_QOS_DSCP_TO_TC_DP_MAP, now.first >= 0 ? h : IFCS_NULL_HANDLE),
                        attr_handle(IFCS_SYSPORT_ATTR_QOS_DOT1P_CFI_TO_TC_DP_MAP, now.second >= 0 ? h : IFCS_NULL_HANDLE)};
    sai_status_t st = c_.sp_set(hw, a[0], "sysport dscp map");
    if (st == SAI_STATUS_SUCCESS) st = c_.sp_set(hw, a[1], "sysport dot1p map");
    if (st != SAI_STATUS_SUCCESS) {
        if (any) group_put(now);
        return st;
    }
    if (any) port_key_[hw] = now;
    else port_key_.erase(hw);
    if (old.first >= 0 || old.second >= 0) group_put(old);
    return SAI_STATUS_SUCCESS;
}

sai_status_t QosArea::default_tc_set(int hw, uint8_t tc)
{
    Guard g(c_.mu);
    if (tc > 7) return SAI_STATUS_INVALID_PARAMETER;
    ifcs_attr_t a = attr_of(IFCS_SYSPORT_ATTR_DEFAULT_TC);
    a.value.u8 = tc;
    return c_.sp_set(hw, a, "sysport default tc");
}

// ------------------------------------------------------------------ PFC
sai_status_t QosArea::pfc_set(int hw, bool separate, uint8_t tx, uint8_t rx)
{
    Guard g(c_.mu);
    if (!c_.front(hw)) return SAI_STATUS_INVALID_PORT_NUMBER;
    auto pm = pfc_mode_.find(hw);
    if (pm == pfc_mode_.end()) {   // the SDK's mode, read once (isai reads it every time)
        uint32_t mode = 0;
        if (c_.dp_get_u32(hw, IFCS_DEVPORT_ATTR_PFC_MODE, mode) != SAI_STATUS_SUCCESS) mode = 0;
        pm = pfc_mode_.emplace(hw, mode).first;
    }
    uint32_t want = separate ? IFCS_DEVPORT_PFC_MODE_SEPARATE : IFCS_DEVPORT_PFC_MODE_COMBINED;
    if (pm->second != want) {
        sai_status_t st = c_.dp_set(hw, attr_u32(IFCS_DEVPORT_ATTR_PFC_MODE, want), "devport pfc mode");
        if (st != SAI_STATUS_SUCCESS) return st;
        pm->second = want;
    }
    std::vector<ifcs_attr_t> a;
    for (int p = 0; p < 8; p++) {
        if (separate) {
            a.push_back(attr_u32(IFCS_DEVPORT_ATTR_PFC0_TX_ENABLE + p, (tx >> p) & 1));
            a.push_back(attr_u32(IFCS_DEVPORT_ATTR_PFC0_RX_ENABLE + p, (rx >> p) & 1));
        } else
            a.push_back(attr_u32(IFCS_DEVPORT_ATTR_PFC0_ENABLE + p, (tx >> p) & 1));
    }
    return c_.call(c_.L.ifcs_devport_attr_set(NODE, (ifcs_devport_t)hw, (uint32_t)a.size(), a.data()), "devport pfc");
}

// ------------------------------------------------------------ schedulers
// isai: strict = mode 0; DWRR / WRR = mode 1 + weight; both rate enables
// written every time (0 when the profile has none); unset = WDRR weight 1.
// Rates: IFCS takes bits/s: a devport MAX_RATE of 12500000 drained a full
// queue at 12.5 Mbit/s on the wire (chip 2026-11-16, sai-hw-qos-20261116.txt),
// so SAI's bytes/s go in x 8 (isai passes them unscaled: 8x too slow).
// Bursts: bytes as SAI gives them (unverified).
static uint64_t bits(uint64_t bytes_per_s) { return bytes_per_s * 8; }
sai_status_t QosArea::queue_sched_set(int hw, int q, const SchedCfg* s)
{
    Guard g(c_.mu);
    if (s && s->packets && (s->min_rate || s->max_rate)) {
        c_.log_once("qos:pps", "scheduler: packet-rate shaping is not supported (byte rates only)");
        return SAI_STATUS_NOT_SUPPORTED;
    }
    std::vector<ifcs_attr_t> a;
    if (s && s->strict) a.push_back(attr_u32(IFCS_QUEUE_ATTR_SCHEDULING_MODE, IFCS_QUEUE_SCHEDULING_MODE_STRICT));
    else {
        a.push_back(attr_u32(IFCS_QUEUE_ATTR_SCHEDULING_MODE, IFCS_QUEUE_SCHEDULING_MODE_WDRR));
        a.push_back(attr_u32(IFCS_QUEUE_ATTR_WDRR_WEIGHT, s ? s->weight : 1));
    }
    a.push_back(attr_u32(IFCS_QUEUE_ATTR_GUARANTEED_RATE_ENABLE, s && s->min_rate ? 1 : 0));
    if (s && s->min_rate) {
        ifcs_attr_t r = attr_of(IFCS_QUEUE_ATTR_GUARANTEED_RATE);
        r.value.u64 = bits(s->min_rate);
        a.push_back(r);
        if (s->min_burst) a.push_back(attr_u32(IFCS_QUEUE_ATTR_GUARANTEED_RATE_BURST_SIZE, (uint32_t)s->min_burst));
    }
    a.push_back(attr_u32(IFCS_QUEUE_ATTR_MAX_RATE_ENABLE, s && s->max_rate ? 1 : 0));
    if (s && s->max_rate) {
        ifcs_attr_t r = attr_of(IFCS_QUEUE_ATTR_MAX_RATE);
        r.value.u64 = bits(s->max_rate);
        a.push_back(r);
        if (s->max_burst) a.push_back(attr_u32(IFCS_QUEUE_ATTR_MAX_RATE_BURST_SIZE, (uint32_t)s->max_burst));
    }
    return queue_set(hw, q, a, "queue scheduler");
}

sai_status_t QosArea::port_sched_set(int hw, const SchedCfg* s)
{
    Guard g(c_.mu);
    if (!c_.front(hw)) return SAI_STATUS_INVALID_PORT_NUMBER;
    if (s && s->packets && s->max_rate) {
        c_.log_once("qos:pps", "scheduler: packet-rate shaping is not supported (byte rates only)");
        return SAI_STATUS_NOT_SUPPORTED;
    }
    std::vector<ifcs_attr_t> a;
    a.push_back(attr_u32(IFCS_DEVPORT_ATTR_MAX_RATE_ENABLE, s && s->max_rate ? 1 : 0));
    if (s && s->max_rate) {
        ifcs_attr_t r = attr_of(IFCS_DEVPORT_ATTR_MAX_RATE);
        r.value.u64 = bits(s->max_rate);
        a.push_back(r);
        if (s->max_burst) a.push_back(attr_u32(IFCS_DEVPORT_ATTR_MAX_BURST_SIZE, (uint32_t)s->max_burst));
    }
    return c_.call(c_.L.ifcs_devport_attr_set(NODE, (ifcs_devport_t)hw, (uint32_t)a.size(), a.data()), "devport shaper");
}

// ------------------------------------------------------------------ WRED
std::vector<ifcs_attr_t> QosArea::curve_attrs(const WredCfg& w)
{
    uint32_t cell = cell_bytes();
    static const uint32_t ids[3][3] = {
        {IFCS_AQM_CURVE_GROUP_ATTR_WRED_GREEN_MIN_THRESHOLD, IFCS_AQM_CURVE_GROUP_ATTR_WRED_GREEN_MAX_THRESHOLD,
         IFCS_AQM_CURVE_GROUP_ATTR_WRED_GREEN_MAXP_PERCENTAGE},
        {IFCS_AQM_CURVE_GROUP_ATTR_WRED_YELLOW_MIN_THRESHOLD, IFCS_AQM_CURVE_GROUP_ATTR_WRED_YELLOW_MAX_THRESHOLD,
         IFCS_AQM_CURVE_GROUP_ATTR_WRED_YELLOW_MAXP_PERCENTAGE},
        {IFCS_AQM_CURVE_GROUP_ATTR_WRED_RED_MIN_THRESHOLD, IFCS_AQM_CURVE_GROUP_ATTR_WRED_RED_MAX_THRESHOLD,
         IFCS_AQM_CURVE_GROUP_ATTR_WRED_RED_MAXP_PERCENTAGE}};
    std::vector<ifcs_attr_t> a;
    for (int c = 0; c < 3; c++) {
        // bytes -> cells, down to a multiple of 8 (isai: 100000 B -> 440, 2000000 B -> 8928)
        uint32_t mn = w.enable[c] ? (w.min[c] / cell) & ~7u : WRED_OFF_CELLS;
        uint32_t mx = w.enable[c] ? (w.max[c] / cell) & ~7u : WRED_OFF_CELLS;
        uint32_t p = w.enable[c] ? w.prob[c] : 100;
        a.push_back(attr_u32(ids[c][0], mn));
        a.push_back(attr_u32(ids[c][1], mx));
        a.push_back(attr_u32(ids[c][2], p));
    }
    return a;
}

// The queue's WRED / ECN switches, one attribute per call (a refusal then
// names the attribute). The averaging weight only when the profile has one.
// ECN_EXTENSION_ENABLE only when the profile marks (or to undo that):
// 0.12.41 refused it on a plain lossy queue (chip 2026-11-16, "Failed to Set
// PD State", PARAM), so ECN marking is NOT_SUPPORTED where it is refused.
sai_status_t QosArea::wred_flags(int hw, int q, const WredCfg* w)
{
    Key k(hw, q);
    bool any = w && (w->enable[0] || w->enable[1] || w->enable[2]);
    bool ecn = w && w->ecn != SAI_ECN_MARK_MODE_NONE;
    sai_status_t st = SAI_STATUS_SUCCESS;
    if (w && w->weight)
        st = queue_set(hw, q, {attr_u32(IFCS_QUEUE_ATTR_AQM_QSIZE_AVERAGING_WEIGHT, w->weight)}, "queue aqm averaging weight");
    if (st == SAI_STATUS_SUCCESS) {
        // 0.12.41 refuses WRED_ENABLE 1 on these queues ("Failed to Set PD State",
        // PARAM, also with the port disabled; chip 2026-11-16): NOT_SUPPORTED then.
        st = queue_set(hw, q, {attr_u32(IFCS_QUEUE_ATTR_WRED_ENABLE, any ? 1 : 0)}, "queue wred enable");
        if (st != SAI_STATUS_SUCCESS && any) {
            c_.log_once("qos:wred", "WRED: the SDK refuses WRED_ENABLE on this queue (IFCS 0.12.41); not supported");
            st = SAI_STATUS_NOT_SUPPORTED;
        }
    }
    if (st == SAI_STATUS_SUCCESS && (ecn || queue_ecn_.count(k))) {
        st = queue_set(hw, q, {attr_u32(IFCS_QUEUE_ATTR_ECN_EXTENSION_ENABLE, ecn ? 1 : 0)}, "queue ecn enable");
        if (st == SAI_STATUS_SUCCESS) {
            if (ecn) queue_ecn_.insert(k);
            else queue_ecn_.erase(k);
        } else if (ecn) {
            c_.log_once("qos:ecn", "WRED ECN marking: the SDK refuses ECN_EXTENSION_ENABLE on this queue");
            st = SAI_STATUS_NOT_SUPPORTED;
        }
    }
    return st;
}

sai_status_t QosArea::wred_create(const WredCfg& w, int& id)
{
    Guard g(c_.mu);
    Wred x{w, IFCS_NULL_HANDLE};
    std::vector<ifcs_attr_t> a = curve_attrs(w);
    sai_status_t st = c_.call(c_.L.ifcs_aqm_curve_group_create(NODE, &x.h, (uint32_t)a.size(), a.data()), "aqm curve group create");
    if (st != SAI_STATUS_SUCCESS) return st;
    id = next_++;
    wreds_[id] = x;
    return SAI_STATUS_SUCCESS;
}

sai_status_t QosArea::wred_update(int id, const WredCfg& w)
{
    Guard g(c_.mu);
    auto it = wreds_.find(id);
    if (it == wreds_.end()) return SAI_STATUS_ITEM_NOT_FOUND;
    std::vector<ifcs_attr_t> a = curve_attrs(w);
    sai_status_t st = c_.call(c_.L.ifcs_aqm_curve_group_attr_set(NODE, it->second.h, (uint32_t)a.size(), a.data()),
                              "aqm curve group set");
    if (st != SAI_STATUS_SUCCESS) return st;
    it->second.cfg = w;
    for (auto& q : queue_wred_)   // ECN / enable flags live on the queues
        if (q.second == id && (st = wred_flags(q.first.first, q.first.second, &w)) != SAI_STATUS_SUCCESS) return st;
    return SAI_STATUS_SUCCESS;
}

sai_status_t QosArea::wred_destroy(int id)
{
    Guard g(c_.mu);
    auto it = wreds_.find(id);
    if (it == wreds_.end()) return SAI_STATUS_ITEM_NOT_FOUND;
    for (auto& q : queue_wred_)
        if (q.second == id) return SAI_STATUS_OBJECT_IN_USE;
    sai_status_t st = c_.call(c_.L.ifcs_aqm_curve_group_delete(NODE, it->second.h), "aqm curve group delete");
    if (st != SAI_STATUS_SUCCESS) return st;
    wreds_.erase(it);
    return SAI_STATUS_SUCCESS;
}

sai_status_t QosArea::queue_wred_set(int hw, int q, int id)
{
    Guard g(c_.mu);
    Key k(hw, q);
    if (id >= 0) {
        auto it = wreds_.find(id);
        if (it == wreds_.end()) return SAI_STATUS_ITEM_NOT_FOUND;
        // isai: the curve group first, then averaging weight / ECN / enable
        sai_status_t st = queue_set(hw, q, {attr_handle(IFCS_QUEUE_ATTR_AQM_CURVE_GROUP, it->second.h)}, "queue aqm curve");
        if (st != SAI_STATUS_SUCCESS) return st;
        if ((st = wred_flags(hw, q, &it->second.cfg)) != SAI_STATUS_SUCCESS) {   // back to no curve
            wred_flags(hw, q, nullptr);
            queue_set(hw, q, {attr_handle(IFCS_QUEUE_ATTR_AQM_CURVE_GROUP, IFCS_NULL_HANDLE)}, "queue aqm curve");
            return st;
        }
        queue_wred_[k] = id;
        return SAI_STATUS_SUCCESS;
    }
    if (!queue_wred_.count(k)) return SAI_STATUS_SUCCESS;
    sai_status_t st = wred_flags(hw, q, nullptr);
    if (st == SAI_STATUS_SUCCESS)
        st = queue_set(hw, q, {attr_handle(IFCS_QUEUE_ATTR_AQM_CURVE_GROUP, IFCS_NULL_HANDLE)}, "queue aqm curve");
    if (st != SAI_STATUS_SUCCESS) return st;
    queue_wred_.erase(k);
    return SAI_STATUS_SUCCESS;
}

// --------------------------------------------------------------- buffers
sai_status_t QosArea::queue_defaults(int hw, int q, QueueDefaults& d)
{
    Key k(hw, q);
    auto it = queue_defaults_.find(k);
    if (it != queue_defaults_.end()) { d = it->second; return SAI_STATUS_SUCCESS; }
    ifcs_queue_t key = queue_key(hw, q);
    ifcs_attr_t a[6] = {attr_of(IFCS_QUEUE_ATTR_RESERVED_SIZE), attr_of(IFCS_QUEUE_ATTR_THRESHOLD_MODE),
                        attr_of(IFCS_QUEUE_ATTR_UC_DYNAMIC_THRESHOLD_FACTOR), attr_of(IFCS_QUEUE_ATTR_NONUC_DYNAMIC_THRESHOLD_FACTOR),
                        attr_of(IFCS_QUEUE_ATTR_UC_STATIC_THRESHOLD_LIMIT), attr_of(IFCS_QUEUE_ATTR_NONUC_STATIC_THRESHOLD_LIMIT)};
    uint32_t n = 0;
    sai_status_t st = c_.call(c_.L.ifcs_queue_attr_get(NODE, &key, 6, a, &n), "queue attr get");
    if (st != SAI_STATUS_SUCCESS) return st;
    d.have = true;
    d.reserved = a[0].value.u32;
    d.mode = a[1].value.u32;
    d.uc = a[2].value.u32;
    d.nonuc = a[3].value.u32;
    d.uc_static = a[4].value.u32;
    d.nonuc_static = a[5].value.u32;
    queue_defaults_[k] = d;
    queue_reserved_[k] = d.reserved;
    return SAI_STATUS_SUCCESS;
}

// isai: RESERVED_SIZE changes with the port disabled (admin down, set, up),
// then FORCE_DO_NOT_ADMIT / FORCE_BYPASS_FLOW_CONTROL 0; threshold changes
// alone are written on the running port.
sai_status_t QosArea::queue_buffer_set(int hw, int q, const BufferCfg* b)
{
    Guard g(c_.mu);
    if (!c_.front(hw) || q < 0 || q > 7) return SAI_STATUS_INVALID_PARAMETER;
    if (b && b->ingress) return SAI_STATUS_INVALID_PARAMETER;
    QueueDefaults d;
    sai_status_t st = queue_defaults(hw, q, d);
    if (st != SAI_STATUS_SUCCESS) return st;
    uint32_t cell = cell_bytes();
    uint32_t reserved = b ? (uint32_t)((b->reserved + cell - 1) / cell) : d.reserved;
    std::vector<ifcs_attr_t> th;
    if (!b) {
        th = {attr_u32(IFCS_QUEUE_ATTR_THRESHOLD_MODE, d.mode), attr_u32(IFCS_QUEUE_ATTR_UC_DYNAMIC_THRESHOLD_FACTOR, d.uc),
              attr_u32(IFCS_QUEUE_ATTR_NONUC_DYNAMIC_THRESHOLD_FACTOR, d.nonuc),
              attr_u32(IFCS_QUEUE_ATTR_UC_STATIC_THRESHOLD_LIMIT, d.uc_static),
              attr_u32(IFCS_QUEUE_ATTR_NONUC_STATIC_THRESHOLD_LIMIT, d.nonuc_static)};
    } else if (b->dynamic) {
        int uc = b->dynamic_th + 5, nonuc = b->dynamic_th + 1;   // isai: th 3 -> 8 / 4, th 1 -> 6 / 2
        if (uc < 0 || nonuc < 0) {   // below isai's range: the smallest factor
            c_.log_once("qos:th", "buffer profile: SHARED_DYNAMIC_TH %d below the factor range; using factor 0", b->dynamic_th);
            uc = uc < 0 ? 0 : uc;
            nonuc = nonuc < 0 ? 0 : nonuc;
        }
        th = {attr_u32(IFCS_QUEUE_ATTR_THRESHOLD_MODE, IFCS_QUEUE_THRESHOLD_MODE_DYNAMIC),
              attr_u32(IFCS_QUEUE_ATTR_UC_DYNAMIC_THRESHOLD_FACTOR, (uint32_t)uc),
              attr_u32(IFCS_QUEUE_ATTR_NONUC_DYNAMIC_THRESHOLD_FACTOR, (uint32_t)nonuc)};
    } else {
        uint32_t lim = (uint32_t)((b->static_th + cell - 1) / cell);
        th = {attr_u32(IFCS_QUEUE_ATTR_THRESHOLD_MODE, IFCS_QUEUE_THRESHOLD_MODE_STATIC),
              attr_u32(IFCS_QUEUE_ATTR_UC_STATIC_THRESHOLD_LIMIT, lim), attr_u32(IFCS_QUEUE_ATTR_NONUC_STATIC_THRESHOLD_LIMIT, lim)};
    }
    Key k(hw, q);
    if (reserved == queue_reserved_[k] || reserve_refused_) return queue_set(hw, q, th, "queue thresholds");
    uint32_t admin = 0;
    c_.dp_get_u32(hw, IFCS_DEVPORT_ATTR_ADMIN_STATE, admin);
    if (admin && (st = c_.dp_set(hw, attr_u32(IFCS_DEVPORT_ATTR_ADMIN_STATE, 0), "devport disable (queue reserve)")) != SAI_STATUS_SUCCESS)
        return st;
    std::vector<ifcs_attr_t> all = {attr_u32(IFCS_QUEUE_ATTR_RESERVED_SIZE, reserved)};
    all.insert(all.end(), th.begin(), th.end());
    st = queue_set(hw, q, all, "queue reserve + thresholds");
    if (st != SAI_STATUS_SUCCESS) {
        // IFCS 0.12.41: "Attr id(2) is Not valid for this API(ifcs_queue_attr_set)" (SONiC's
        // BUFFER_QUEUE on the chip, 2026-11-16): the reserve stays the SDK's, thresholds only
        reserve_refused_ = true;
        OB_WARN(SAI_API_BUFFER, "teralynx: queue RESERVED_SIZE refused by this SDK; buffer profile size kept at the "
                "SDK's reserve, thresholds programmed");
        st = queue_set(hw, q, th, "queue thresholds");
    }
    sai_status_t up = admin ? c_.dp_set(hw, attr_u32(IFCS_DEVPORT_ATTR_ADMIN_STATE, 1), "devport enable (queue reserve)")
                            : SAI_STATUS_SUCCESS;
    if (st != SAI_STATUS_SUCCESS) return st;
    if (up != SAI_STATUS_SUCCESS) return up;
    queue_reserved_[k] = reserved;
    return queue_set(hw, q, {attr_u32(IFCS_QUEUE_ATTR_FORCE_DO_NOT_ADMIT, 0), attr_u32(IFCS_QUEUE_ATTR_FORCE_BYPASS_FLOW_CONTROL, 0)},
                     "queue admit");
}

sai_status_t QosArea::pg_buffer_set(int hw, int pg, const BufferCfg* b)
{
    Guard g(c_.mu);
    if (!c_.front(hw) || pg < 0 || pg > 7) return SAI_STATUS_INVALID_PARAMETER;
    if (b && !pg_logged_.count(0)) {
        pg_logged_.insert(0);
        OB_WARN(SAI_API_BUFFER, "teralynx: ingress priority-group buffer profiles are kept, not programmed "
                "(the SDK init config has no lossless buffer partition; xoff/xon headroom has no effect)");
    }
    return SAI_STATUS_SUCCESS;
}

// -------------------------------------------------------------- counters
// Queue: TX packets / bytes, drops, occupancy and watermark (cells x the
// DTM cell size: SHARED_WATERMARK is the total use, reserved included),
// ECN marks. Unmapped ids: NOT_SUPPORTED.
static bool queue_stat_id(sai_stat_id_t s, int& id, bool& cells)
{
    cells = false;
    switch (s) {
    case SAI_QUEUE_STAT_PACKETS: id = IFCS_QUEUE_STATS_ID_PACKET_TX_COUNT; return true;
    case SAI_QUEUE_STAT_BYTES: id = IFCS_QUEUE_STATS_ID_BYTE_TX_COUNT; return true;
    case SAI_QUEUE_STAT_DROPPED_PACKETS: id = IFCS_QUEUE_STATS_ID_PACKET_DROP_COUNT; return true;
    case SAI_QUEUE_STAT_CURR_OCCUPANCY_BYTES: id = IFCS_QUEUE_STATS_ID_TOTAL_USE_COUNT; cells = true; return true;
    case SAI_QUEUE_STAT_WATERMARK_BYTES:
    case SAI_QUEUE_STAT_SHARED_WATERMARK_BYTES: id = IFCS_QUEUE_STATS_ID_TOTAL_USE_WATERMARK; cells = true; return true;
    case SAI_QUEUE_STAT_WRED_ECN_MARKED_PACKETS: id = IFCS_QUEUE_STATS_ID_PACKET_ECN_MARKED_COUNT; return true;
    case SAI_QUEUE_STAT_WRED_ECN_MARKED_BYTES: id = IFCS_QUEUE_STATS_ID_BYTE_ECN_MARKED_COUNT; return true;
    default: return false;
    }
}

sai_status_t QosArea::queue_stats_get(int hw, int q, uint32_t n, const sai_stat_id_t* ids, uint64_t* out)
{
    Guard g(c_.mu);
    if (!c_.front(hw) || q < 0 || q > 7) return SAI_STATUS_INVALID_PARAMETER;
    std::vector<ifcs_queue_stats_id_t> v(n);
    std::vector<bool> cells(n);
    for (uint32_t i = 0; i < n; i++) {
        int id;
        bool cl;
        if (!queue_stat_id(ids[i], id, cl)) return SAI_STATUS_NOT_SUPPORTED;
        v[i] = id;
        cells[i] = cl;
    }
    if (!n) return SAI_STATUS_SUCCESS;
    ifcs_queue_t k = queue_key(hw, q);
    std::vector<uint64_t> vals(n);
    uint32_t got = 0;
    sai_status_t st = c_.call(c_.L.ifcs_queue_stats_get(NODE, &k, n, v.data(), vals.data(), &got), "queue stats");
    if (st != SAI_STATUS_SUCCESS) return st;
    uint32_t cell = cell_bytes();
    for (uint32_t i = 0; i < n; i++) out[i] = cells[i] ? vals[i] * cell : vals[i];
    return SAI_STATUS_SUCCESS;
}

sai_status_t QosArea::queue_stats_clear(int hw, int q, uint32_t n, const sai_stat_id_t* ids)
{
    Guard g(c_.mu);
    if (!c_.front(hw) || q < 0 || q > 7) return SAI_STATUS_INVALID_PARAMETER;
    std::vector<ifcs_queue_stats_id_t> v;
    for (uint32_t i = 0; i < n; i++) {
        int id;
        bool cl;
        if (!queue_stat_id(ids[i], id, cl)) return SAI_STATUS_NOT_SUPPORTED;
        if (ids[i] == SAI_QUEUE_STAT_CURR_OCCUPANCY_BYTES) continue;   // a level, nothing to clear
        v.push_back(id);
    }
    if (v.empty()) return SAI_STATUS_SUCCESS;
    ifcs_queue_t k = queue_key(hw, q);
    return c_.call(c_.L.ifcs_queue_stats_clear(NODE, &k, (uint32_t)v.size(), v.data()), "queue stats clear");
}

// Ingress priority group = the BAM (ingress admission) queue of that index:
// drops, occupancy, watermark (cells x the DTM cell size: unverified for BAM).
static bool pg_stat_id(sai_stat_id_t s, int& id, bool& cells)
{
    cells = false;
    switch (s) {
    case SAI_INGRESS_PRIORITY_GROUP_STAT_DROPPED_PACKETS: id = IFCS_BAM_QUEUE_STATS_ID_PACKET_DROP_COUNT; return true;
    case SAI_INGRESS_PRIORITY_GROUP_STAT_CURR_OCCUPANCY_BYTES: id = IFCS_BAM_QUEUE_STATS_ID_TOTAL_USE_COUNT; cells = true; return true;
    case SAI_INGRESS_PRIORITY_GROUP_STAT_WATERMARK_BYTES:
    case SAI_INGRESS_PRIORITY_GROUP_STAT_SHARED_WATERMARK_BYTES:
        id = IFCS_BAM_QUEUE_STATS_ID_TOTAL_USE_WATERMARK; cells = true; return true;
    default: return false;
    }
}

sai_status_t QosArea::pg_stats_get(int hw, int pg, uint32_t n, const sai_stat_id_t* ids, uint64_t* out)
{
    Guard g(c_.mu);
    if (!c_.front(hw) || pg < 0 || pg > 7) return SAI_STATUS_INVALID_PARAMETER;
    std::vector<ifcs_bam_queue_stats_id_t> v(n);
    std::vector<bool> cells(n);
    for (uint32_t i = 0; i < n; i++) {
        int id;
        bool cl;
        if (!pg_stat_id(ids[i], id, cl)) return SAI_STATUS_NOT_SUPPORTED;
        v[i] = id;
        cells[i] = cl;
    }
    if (!n) return SAI_STATUS_SUCCESS;
    ifcs_bam_queue_t k;
    std::memset(&k, 0, sizeof(k));
    c_.L.ifcs_bam_queue_t_init(&k);
    k.devport = (ifcs_devport_t)hw;
    k.queue_id = (uint32_t)pg;
    k.dirty = 3;
    std::vector<uint64_t> vals(n);
    uint32_t got = 0;
    sai_status_t st = c_.call(c_.L.ifcs_bam_queue_stats_get(NODE, &k, n, v.data(), vals.data(), &got), "bam queue stats");
    if (st != SAI_STATUS_SUCCESS) return st;
    uint32_t cell = cell_bytes();
    for (uint32_t i = 0; i < n; i++) out[i] = cells[i] ? vals[i] * cell : vals[i];
    return SAI_STATUS_SUCCESS;
}

sai_status_t QosArea::pg_stats_clear(int hw, int pg, uint32_t n, const sai_stat_id_t* ids)
{
    Guard g(c_.mu);
    if (!c_.front(hw) || pg < 0 || pg > 7) return SAI_STATUS_INVALID_PARAMETER;
    std::vector<ifcs_bam_queue_stats_id_t> v;
    for (uint32_t i = 0; i < n; i++) {
        int id;
        bool cl;
        if (!pg_stat_id(ids[i], id, cl)) return SAI_STATUS_NOT_SUPPORTED;
        if (ids[i] != SAI_INGRESS_PRIORITY_GROUP_STAT_CURR_OCCUPANCY_BYTES) v.push_back(id);
    }
    if (v.empty()) return SAI_STATUS_SUCCESS;
    ifcs_bam_queue_t k;
    std::memset(&k, 0, sizeof(k));
    c_.L.ifcs_bam_queue_t_init(&k);
    k.devport = (ifcs_devport_t)hw;
    k.queue_id = (uint32_t)pg;
    k.dirty = 3;
    return c_.call(c_.L.ifcs_bam_queue_stats_clear(NODE, &k, (uint32_t)v.size(), v.data()), "bam queue stats clear");
}

sai_status_t QosArea::pfc_stat_get(int hw, int prio, bool tx, uint64_t& v)
{
    Guard g(c_.mu);
    if (!c_.front(hw) || prio < 0 || prio > 7) return SAI_STATUS_INVALID_PARAMETER;
    int id = (tx ? IFCS_DEVPORT_STATS_ID_TX_FRAMES_PFC0 : IFCS_DEVPORT_STATS_ID_RX_FRAMES_PFC0) + prio;
    uint32_t got = 0;
    return c_.call(c_.L.ifcs_devport_stats_get(NODE, (ifcs_devport_t)hw, 1, &id, &v, &got), "devport pfc stats");
}

std::string QosArea::show()
{
    Guard g(c_.mu);
    std::string s;
    char b[200];
    std::snprintf(b, sizeof(b), "qos: %zu maps, %zu map groups, %zu ports mapped, %zu wred, cell %u B\n", maps_.size(),
                  groups_.size(), port_key_.size(), wreds_.size(), cell_);
    s += b;
    for (auto& p : port_key_) {
        std::snprintf(b, sizeof(b), "  devport %d: dscp map %d dot1p map %d\n", p.first, p.second.first, p.second.second);
        s += b;
    }
    for (auto& q : queue_wred_) {
        std::snprintf(b, sizeof(b), "  queue %d.%d: wred %d\n", q.first.first, q.first.second, q.second);
        s += b;
    }
    return s;
}

} // namespace tlb
} // namespace ob
