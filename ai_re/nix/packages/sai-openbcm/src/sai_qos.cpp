// SPDX-License-Identifier: Apache-2.0
// QoS: QOS_MAP, SCHEDULER, SCHEDULER_GROUP, WRED, BUFFER_POOL, BUFFER_PROFILE,
// QUEUE and INGRESS_PRIORITY_GROUP attributes and counters, and the port /
// switch QoS attributes. With a backend that has qos_supported() every
// change goes through the QoS calls of backend.h; otherwise the objects are
// kept here only, nothing is programmed (as before these objects existed).
//
// Hierarchy: one scheduler group per queue (level 0, parent = the port,
// one child = that queue), as SONiC's qosorch looks a queue's group up and
// sets the group's SCHEDULER_PROFILE_ID; a port's own QOS_SCHEDULER_PROFILE_ID
// is the port shaper. A queue's scheduler: its group's, else the queue's
// own SCHEDULER_PROFILE_ID.
#include "sai_internal.h"

namespace ob {

static bool qos_be() { return ctx().be && ctx().be->qos_supported(); }

// ------------------------------------------------------------- QoS maps
static bool map_kind(int32_t type, QosMapKind& k)
{
    switch (type) {
    case SAI_QOS_MAP_TYPE_DOT1P_TO_TC: k = QosMapKind::Dot1pToTc; return true;
    case SAI_QOS_MAP_TYPE_DOT1P_TO_COLOR: k = QosMapKind::Dot1pToColor; return true;
    case SAI_QOS_MAP_TYPE_DSCP_TO_TC: k = QosMapKind::DscpToTc; return true;
    case SAI_QOS_MAP_TYPE_DSCP_TO_COLOR: k = QosMapKind::DscpToColor; return true;
    case SAI_QOS_MAP_TYPE_TC_TO_QUEUE: k = QosMapKind::TcToQueue; return true;
    case SAI_QOS_MAP_TYPE_TC_AND_COLOR_TO_DSCP: k = QosMapKind::TcColorToDscp; return true;
    case SAI_QOS_MAP_TYPE_TC_AND_COLOR_TO_DOT1P: k = QosMapKind::TcColorToDot1p; return true;
    case SAI_QOS_MAP_TYPE_TC_TO_PRIORITY_GROUP: k = QosMapKind::TcToPg; return true;
    case SAI_QOS_MAP_TYPE_PFC_PRIORITY_TO_PRIORITY_GROUP: k = QosMapKind::PfcPrioToPg; return true;
    case SAI_QOS_MAP_TYPE_PFC_PRIORITY_TO_QUEUE: k = QosMapKind::PfcPrioToQueue; return true;
    default: return false;
    }
}

static bool port_map_attr(sai_attr_id_t id, QosMapKind& k)
{
    switch (id) {
    case SAI_PORT_ATTR_QOS_DOT1P_TO_TC_MAP: k = QosMapKind::Dot1pToTc; return true;
    case SAI_PORT_ATTR_QOS_DOT1P_TO_COLOR_MAP: k = QosMapKind::Dot1pToColor; return true;
    case SAI_PORT_ATTR_QOS_DSCP_TO_TC_MAP: k = QosMapKind::DscpToTc; return true;
    case SAI_PORT_ATTR_QOS_DSCP_TO_COLOR_MAP: k = QosMapKind::DscpToColor; return true;
    case SAI_PORT_ATTR_QOS_TC_TO_QUEUE_MAP: k = QosMapKind::TcToQueue; return true;
    case SAI_PORT_ATTR_QOS_TC_AND_COLOR_TO_DOT1P_MAP: k = QosMapKind::TcColorToDot1p; return true;
    case SAI_PORT_ATTR_QOS_TC_AND_COLOR_TO_DSCP_MAP: k = QosMapKind::TcColorToDscp; return true;
    case SAI_PORT_ATTR_QOS_TC_TO_PRIORITY_GROUP_MAP: k = QosMapKind::TcToPg; return true;
    case SAI_PORT_ATTR_QOS_PFC_PRIORITY_TO_PRIORITY_GROUP_MAP: k = QosMapKind::PfcPrioToPg; return true;
    case SAI_PORT_ATTR_QOS_PFC_PRIORITY_TO_QUEUE_MAP: k = QosMapKind::PfcPrioToQueue; return true;
    default: return false;
    }
}

static bool switch_map_attr(sai_attr_id_t id, QosMapKind& k)
{
    switch (id) {
    case SAI_SWITCH_ATTR_QOS_DOT1P_TO_TC_MAP: k = QosMapKind::Dot1pToTc; return true;
    case SAI_SWITCH_ATTR_QOS_DOT1P_TO_COLOR_MAP: k = QosMapKind::Dot1pToColor; return true;
    case SAI_SWITCH_ATTR_QOS_DSCP_TO_TC_MAP: k = QosMapKind::DscpToTc; return true;
    case SAI_SWITCH_ATTR_QOS_DSCP_TO_COLOR_MAP: k = QosMapKind::DscpToColor; return true;
    case SAI_SWITCH_ATTR_QOS_TC_TO_QUEUE_MAP: k = QosMapKind::TcToQueue; return true;
    case SAI_SWITCH_ATTR_QOS_TC_AND_COLOR_TO_DOT1P_MAP: k = QosMapKind::TcColorToDot1p; return true;
    case SAI_SWITCH_ATTR_QOS_TC_AND_COLOR_TO_DSCP_MAP: k = QosMapKind::TcColorToDscp; return true;
    default: return false;
    }
}

// Key / value of one SAI entry for a map kind; false: out of range.
static bool entry_kv(QosMapKind k, const sai_qos_map_t& m, uint32_t& key, uint32_t& kc, uint32_t& val, uint32_t& vc)
{
    kc = vc = 0;
    switch (k) {
    case QosMapKind::DscpToTc: key = m.key.dscp; val = m.value.tc; return key < 64 && val < 16;
    case QosMapKind::DscpToColor: key = m.key.dscp; vc = m.value.color; val = 0; return key < 64 && vc <= 2;
    case QosMapKind::Dot1pToTc: key = m.key.dot1p; val = m.value.tc; return key < 8 && val < 16;
    case QosMapKind::Dot1pToColor: key = m.key.dot1p; vc = m.value.color; val = 0; return key < 8 && vc <= 2;
    case QosMapKind::TcToQueue: key = m.key.tc; val = m.value.queue_index; return key < 16 && val < 8;
    case QosMapKind::TcToPg: key = m.key.tc; val = m.value.pg; return key < 16 && val < 8;
    case QosMapKind::PfcPrioToQueue: key = m.key.prio; val = m.value.queue_index; return key < 8 && val < 8;
    case QosMapKind::PfcPrioToPg: key = m.key.prio; val = m.value.pg; return key < 8 && val < 8;
    case QosMapKind::TcColorToDscp: key = m.key.tc; kc = m.key.color; val = m.value.dscp; return key < 16 && kc <= 2 && val < 64;
    case QosMapKind::TcColorToDot1p: key = m.key.tc; kc = m.key.color; val = m.value.dot1p; return key < 16 && kc <= 2 && val < 8;
    default: return false;
    }
}

static uint32_t key_count(QosMapKind k)
{
    switch (k) {
    case QosMapKind::DscpToTc: case QosMapKind::DscpToColor: return 64;
    default: return 8;
    }
}
static bool colour_keyed(QosMapKind k) { return k == QosMapKind::TcColorToDscp || k == QosMapKind::TcColorToDot1p; }

// The complete table for the backend: every key (x colour), unlisted = 0 / green.
static bool full_entries(QosMapKind k, const std::vector<sai_qos_map_t>& l, std::vector<QosMapEntry>& out)
{
    uint32_t n = key_count(k), colours = colour_keyed(k) ? 3 : 1;
    out.assign(n * colours, QosMapEntry());
    for (uint32_t i = 0; i < n * colours; i++) {
        out[i].key = i / colours;
        out[i].key_color = i % colours;
    }
    for (auto& m : l) {
        uint32_t key, kc, val, vc;
        if (!entry_kv(k, m, key, kc, val, vc)) return false;
        if (key >= n) continue;                     // TC 8..15: no such queue / priority here
        QosMapEntry& e = out[key * colours + kc];
        e.value = val;
        e.value_color = vc;
    }
    return true;
}

static QosMapObj* qos_map_of(sai_object_id_t oid) { return lookup(ctx().qos_maps, oid); }
static int map_id(sai_object_id_t oid)
{
    QosMapObj* m = qos_map_of(oid);
    return m ? m->id : -1;
}
static bool map_used(sai_object_id_t oid)
{
    auto& c = ctx();
    for (int k = 0; k < (int)QosMapKind::Count; k++)
        if (c.switch_qos_map[k] == oid) return true;
    for (auto& p : c.ports)
        for (int k = 0; k < (int)QosMapKind::Count; k++)
            if (p.second.qos_map[k] == oid) return true;
    return false;
}

// The map a port uses for a kind: its own, else the switch's.
static sai_status_t apply_port_map(PortObj& p, QosMapKind k)
{
    if (!qos_be() || p.cpu) return SAI_STATUS_SUCCESS;
    sai_object_id_t m = p.qos_map[(int)k] ? p.qos_map[(int)k] : ctx().switch_qos_map[(int)k];
    return ctx().be->port_qos_map_set(p.hw, k, map_id(m));
}

static sai_status_t create_qos_map(_Out_ sai_object_id_t* id, _In_ sai_object_id_t sw, _In_ uint32_t n,
                                   _In_ const sai_attribute_t* attrs)
{
    return guarded(SAI_API_QOS_MAP, __func__, [&]() -> sai_status_t {
        auto& c = ctx();
        if (!id || (n && !attrs)) return SAI_STATUS_INVALID_PARAMETER;
        if (sw != c.switch_oid) return SAI_STATUS_INVALID_OBJECT_ID;
        QosMapObj m;
        bool have_type = false, have_list = false;
        for (uint32_t i = 0; i < n; i++) {
            const sai_attribute_t& a = attrs[i];
            if (a.id == SAI_QOS_MAP_ATTR_TYPE) { m.type = a.value.s32; have_type = true; }
            else if (a.id == SAI_QOS_MAP_ATTR_MAP_TO_VALUE_LIST) {
                if (a.value.qosmap.count && !a.value.qosmap.list) return attr_invalid(i);
                m.list.assign(a.value.qosmap.list, a.value.qosmap.list + a.value.qosmap.count);
                have_list = true;
            } else return attr_unknown(i);
        }
        if (!have_type || !have_list) return SAI_STATUS_MANDATORY_ATTRIBUTE_MISSING;
        QosMapKind k;
        if (!map_kind(m.type, k)) return SAI_STATUS_NOT_SUPPORTED;
        std::vector<QosMapEntry> full;
        if (!full_entries(k, m.list, full)) return SAI_STATUS_INVALID_ATTR_VALUE_0;
        if (qos_be()) {
            sai_status_t st = c.be->qos_map_create(k, full, m.id);
            if (st != SAI_STATUS_SUCCESS) return st;
        }
        *id = c.alloc(SAI_OBJECT_TYPE_QOS_MAP);
        c.qos_maps[*id] = m;
        OB_INFO(SAI_API_QOS_MAP, "qos map 0x%lx type %d, %zu entries, backend id %d", (unsigned long)*id, m.type,
                m.list.size(), m.id);
        return SAI_STATUS_SUCCESS;
    });
}

static sai_status_t remove_qos_map(_In_ sai_object_id_t id)
{
    return guarded(SAI_API_QOS_MAP, __func__, [&]() -> sai_status_t {
        auto& c = ctx();
        QosMapObj* m = qos_map_of(id);
        if (!m) return SAI_STATUS_INVALID_OBJECT_ID;
        if (map_used(id)) return SAI_STATUS_OBJECT_IN_USE;
        if (m->id >= 0) {
            sai_status_t st = c.be->qos_map_destroy(m->id);
            if (st != SAI_STATUS_SUCCESS) return st;
        }
        c.qos_maps.erase(id);
        return SAI_STATUS_SUCCESS;
    });
}

static sai_status_t set_qos_map_attribute(_In_ sai_object_id_t id, _In_ const sai_attribute_t* a)
{
    return guarded(SAI_API_QOS_MAP, __func__, [&]() -> sai_status_t {
        QosMapObj* m = qos_map_of(id);
        if (!m) return SAI_STATUS_INVALID_OBJECT_ID;
        if (!a) return SAI_STATUS_INVALID_PARAMETER;
        if (a->id == SAI_QOS_MAP_ATTR_TYPE) return attr_invalid(0);   // create only
        if (a->id != SAI_QOS_MAP_ATTR_MAP_TO_VALUE_LIST) return attr_unknown(0);
        if (a->value.qosmap.count && !a->value.qosmap.list) return attr_invalid(0);
        std::vector<sai_qos_map_t> l(a->value.qosmap.list, a->value.qosmap.list + a->value.qosmap.count);
        QosMapKind k;
        map_kind(m->type, k);
        std::vector<QosMapEntry> full;
        if (!full_entries(k, l, full)) return attr_invalid(0);
        if (m->id >= 0) {
            sai_status_t st = ctx().be->qos_map_update(m->id, full);
            if (st != SAI_STATUS_SUCCESS) return attr_backend_status(st, 0);
        }
        m->list = l;
        return SAI_STATUS_SUCCESS;
    });
}

static sai_status_t get_qos_map_attribute(_In_ sai_object_id_t id, _In_ uint32_t n, _Inout_ sai_attribute_t* attrs)
{
    return guarded(SAI_API_QOS_MAP, __func__, [&]() -> sai_status_t {
        QosMapObj* m = qos_map_of(id);
        if (!m) return SAI_STATUS_INVALID_OBJECT_ID;
        if (n && !attrs) return SAI_STATUS_INVALID_PARAMETER;
        for (uint32_t i = 0; i < n; i++) {
            sai_attribute_t& a = attrs[i];
            if (a.id == SAI_QOS_MAP_ATTR_TYPE) a.value.s32 = m->type;
            else if (a.id == SAI_QOS_MAP_ATTR_MAP_TO_VALUE_LIST) {
                sai_status_t st = fill_list(a.value.qosmap, m->list);
                if (st != SAI_STATUS_SUCCESS) return st;
            } else return attr_unknown(i);
        }
        return SAI_STATUS_SUCCESS;
    });
}

static sai_qos_map_api_t make_qos_map_api()
{
    sai_qos_map_api_t a;
    std::memset(&a, 0, sizeof(a));
    a.create_qos_map = create_qos_map;
    a.remove_qos_map = remove_qos_map;
    a.set_qos_map_attribute = set_qos_map_attribute;
    a.get_qos_map_attribute = get_qos_map_attribute;
    return a;
}
const sai_qos_map_api_t qos_map_api = make_qos_map_api();

// ------------------------------------------------------------ schedulers
static SchedCfg sched_cfg(const SchedulerObj& s)
{
    SchedCfg c;
    c.strict = s.type == SAI_SCHEDULING_TYPE_STRICT;
    c.weight = s.weight;
    c.packets = s.meter == SAI_METER_TYPE_PACKETS;
    c.min_rate = s.min_rate;
    c.min_burst = s.min_burst;
    c.max_rate = s.max_rate;
    c.max_burst = s.max_burst;
    return c;
}

static sai_object_id_t queue_sched(const QueueObj& q)
{
    auto* g = lookup(ctx().sched_groups, q.group);
    return g && g->sched ? g->sched : q.sched;
}

static sai_status_t apply_queue_sched(const QueueObj& q)
{
    if (!qos_be()) return SAI_STATUS_SUCCESS;
    PortObj* p = lookup(ctx().ports, q.port);
    if (!p) return SAI_STATUS_INVALID_OBJECT_ID;
    SchedulerObj* s = lookup(ctx().schedulers, queue_sched(q));
    SchedCfg cfg;
    if (s) cfg = sched_cfg(*s);
    return ctx().be->queue_scheduler_set(p->hw, q.index, s ? &cfg : nullptr);
}

static sai_status_t apply_port_sched(const PortObj& p)
{
    if (!qos_be() || p.cpu) return SAI_STATUS_SUCCESS;
    SchedulerObj* s = lookup(ctx().schedulers, p.scheduler);
    SchedCfg cfg;
    if (s) cfg = sched_cfg(*s);
    return ctx().be->port_scheduler_set(p.hw, s ? &cfg : nullptr);
}

static bool sched_used(sai_object_id_t s)
{
    auto& c = ctx();
    for (auto& g : c.sched_groups) if (g.second.sched == s) return true;
    for (auto& q : c.queues) if (q.second.sched == s) return true;
    for (auto& p : c.ports) if (p.second.scheduler == s) return true;
    return false;
}

static sai_status_t sched_attr(SchedulerObj& s, const sai_attribute_t& a, uint32_t i)
{
    switch (a.id) {
    case SAI_SCHEDULER_ATTR_SCHEDULING_TYPE:
        if (a.value.s32 != SAI_SCHEDULING_TYPE_STRICT && a.value.s32 != SAI_SCHEDULING_TYPE_WRR &&
            a.value.s32 != SAI_SCHEDULING_TYPE_DWRR)
            return attr_invalid(i);
        s.type = a.value.s32;
        return SAI_STATUS_SUCCESS;
    case SAI_SCHEDULER_ATTR_SCHEDULING_WEIGHT:
        if (a.value.u8 < 1 || a.value.u8 > 100) return attr_invalid(i);
        s.weight = a.value.u8;
        return SAI_STATUS_SUCCESS;
    case SAI_SCHEDULER_ATTR_METER_TYPE:
        if (a.value.s32 != SAI_METER_TYPE_PACKETS && a.value.s32 != SAI_METER_TYPE_BYTES) return attr_invalid(i);
        s.meter = a.value.s32;
        return SAI_STATUS_SUCCESS;
    case SAI_SCHEDULER_ATTR_MIN_BANDWIDTH_RATE: s.min_rate = a.value.u64; return SAI_STATUS_SUCCESS;
    case SAI_SCHEDULER_ATTR_MIN_BANDWIDTH_BURST_RATE: s.min_burst = a.value.u64; return SAI_STATUS_SUCCESS;
    case SAI_SCHEDULER_ATTR_MAX_BANDWIDTH_RATE: s.max_rate = a.value.u64; return SAI_STATUS_SUCCESS;
    case SAI_SCHEDULER_ATTR_MAX_BANDWIDTH_BURST_RATE: s.max_burst = a.value.u64; return SAI_STATUS_SUCCESS;
    default: return attr_unknown(i);
    }
}

static sai_status_t create_scheduler(_Out_ sai_object_id_t* id, _In_ sai_object_id_t sw, _In_ uint32_t n,
                                     _In_ const sai_attribute_t* attrs)
{
    return guarded(SAI_API_SCHEDULER, __func__, [&]() -> sai_status_t {
        auto& c = ctx();
        if (!id || (n && !attrs)) return SAI_STATUS_INVALID_PARAMETER;
        if (sw != c.switch_oid) return SAI_STATUS_INVALID_OBJECT_ID;
        SchedulerObj s;
        for (uint32_t i = 0; i < n; i++) {
            sai_status_t st = sched_attr(s, attrs[i], i);
            if (st != SAI_STATUS_SUCCESS) return st;
        }
        *id = c.alloc(SAI_OBJECT_TYPE_SCHEDULER);
        c.schedulers[*id] = s;
        return SAI_STATUS_SUCCESS;
    });
}

static sai_status_t remove_scheduler(_In_ sai_object_id_t id)
{
    return guarded(SAI_API_SCHEDULER, __func__, [&]() -> sai_status_t {
        if (!lookup(ctx().schedulers, id)) return SAI_STATUS_INVALID_OBJECT_ID;
        if (sched_used(id)) return SAI_STATUS_OBJECT_IN_USE;
        ctx().schedulers.erase(id);
        return SAI_STATUS_SUCCESS;
    });
}

static sai_status_t set_scheduler_attribute(_In_ sai_object_id_t id, _In_ const sai_attribute_t* a)
{
    return guarded(SAI_API_SCHEDULER, __func__, [&]() -> sai_status_t {
        auto& c = ctx();
        SchedulerObj* s = lookup(c.schedulers, id);
        if (!s) return SAI_STATUS_INVALID_OBJECT_ID;
        if (!a) return SAI_STATUS_INVALID_PARAMETER;
        SchedulerObj old = *s;
        sai_status_t st = sched_attr(*s, *a, 0);
        if (st != SAI_STATUS_SUCCESS) return st;
        // Every user follows; on a failure the old profile goes back everywhere.
        for (auto& q : c.queues)
            if (queue_sched(q.second) == id && (st = apply_queue_sched(q.second)) != SAI_STATUS_SUCCESS) break;
        for (auto& p : c.ports)
            if (st == SAI_STATUS_SUCCESS && p.second.scheduler == id) st = apply_port_sched(p.second);
        if (st != SAI_STATUS_SUCCESS) {
            *s = old;
            for (auto& q : c.queues) if (queue_sched(q.second) == id) apply_queue_sched(q.second);
            for (auto& p : c.ports) if (p.second.scheduler == id) apply_port_sched(p.second);
            return attr_backend_status(st, 0);
        }
        return SAI_STATUS_SUCCESS;
    });
}

static sai_status_t get_scheduler_attribute(_In_ sai_object_id_t id, _In_ uint32_t n, _Inout_ sai_attribute_t* attrs)
{
    return guarded(SAI_API_SCHEDULER, __func__, [&]() -> sai_status_t {
        SchedulerObj* s = lookup(ctx().schedulers, id);
        if (!s) return SAI_STATUS_INVALID_OBJECT_ID;
        if (n && !attrs) return SAI_STATUS_INVALID_PARAMETER;
        for (uint32_t i = 0; i < n; i++) {
            sai_attribute_t& a = attrs[i];
            switch (a.id) {
            case SAI_SCHEDULER_ATTR_SCHEDULING_TYPE: a.value.s32 = s->type; break;
            case SAI_SCHEDULER_ATTR_SCHEDULING_WEIGHT: a.value.u8 = s->weight; break;
            case SAI_SCHEDULER_ATTR_METER_TYPE: a.value.s32 = s->meter; break;
            case SAI_SCHEDULER_ATTR_MIN_BANDWIDTH_RATE: a.value.u64 = s->min_rate; break;
            case SAI_SCHEDULER_ATTR_MIN_BANDWIDTH_BURST_RATE: a.value.u64 = s->min_burst; break;
            case SAI_SCHEDULER_ATTR_MAX_BANDWIDTH_RATE: a.value.u64 = s->max_rate; break;
            case SAI_SCHEDULER_ATTR_MAX_BANDWIDTH_BURST_RATE: a.value.u64 = s->max_burst; break;
            default: return attr_unknown(i);
            }
        }
        return SAI_STATUS_SUCCESS;
    });
}

static sai_scheduler_api_t make_scheduler_api()
{
    sai_scheduler_api_t a;
    std::memset(&a, 0, sizeof(a));
    a.create_scheduler = create_scheduler;
    a.remove_scheduler = remove_scheduler;
    a.set_scheduler_attribute = set_scheduler_attribute;
    a.get_scheduler_attribute = get_scheduler_attribute;
    return a;
}
const sai_scheduler_api_t scheduler_api = make_scheduler_api();

// ----------------------------------------------------- scheduler groups
void qos_build_port(PortObj& p)
{
    auto& c = ctx();
    for (size_t i = 0; i < p.queues.size(); i++) {
        sai_object_id_t g = make_oid(SAI_OBJECT_TYPE_SCHEDULER_GROUP, ((uint64_t)p.hw << 8) | i);
        SchedGroupObj go;
        go.port = p.oid;
        go.parent = p.oid;
        go.children.push_back(p.queues[i]);
        c.sched_groups[g] = go;
        p.sched_groups.push_back(g);
        c.queues[p.queues[i]].group = g;
    }
}

static sai_status_t create_scheduler_group(_Out_ sai_object_id_t*, _In_ sai_object_id_t, _In_ uint32_t,
                                           _In_ const sai_attribute_t*)
{
    OB_WARN(SAI_API_SCHEDULER_GROUP, "scheduler groups are fixed (one per queue); create refused");
    return SAI_STATUS_NOT_SUPPORTED;
}

static sai_status_t remove_scheduler_group(_In_ sai_object_id_t)
{
    return SAI_STATUS_NOT_SUPPORTED;
}

static sai_status_t set_scheduler_group_attribute(_In_ sai_object_id_t id, _In_ const sai_attribute_t* a)
{
    return guarded(SAI_API_SCHEDULER_GROUP, __func__, [&]() -> sai_status_t {
        auto& c = ctx();
        SchedGroupObj* g = lookup(c.sched_groups, id);
        if (!g) return SAI_STATUS_INVALID_OBJECT_ID;
        if (!a) return SAI_STATUS_INVALID_PARAMETER;
        if (a->id == SAI_SCHEDULER_GROUP_ATTR_PARENT_NODE)
            return a->value.oid == g->parent ? SAI_STATUS_SUCCESS : attr_not_supported(0);
        if (a->id != SAI_SCHEDULER_GROUP_ATTR_SCHEDULER_PROFILE_ID) return attr_not_supported(0);
        if (a->value.oid && !lookup(c.schedulers, a->value.oid)) return attr_invalid(0);
        sai_object_id_t old = g->sched;
        g->sched = a->value.oid;
        for (auto q : g->children) {
            QueueObj* qo = lookup(c.queues, q);
            if (!qo) continue;
            sai_status_t st = apply_queue_sched(*qo);
            if (st != SAI_STATUS_SUCCESS) {
                g->sched = old;
                apply_queue_sched(*qo);
                return attr_backend_status(st, 0);
            }
        }
        return SAI_STATUS_SUCCESS;
    });
}

static sai_status_t get_scheduler_group_attribute(_In_ sai_object_id_t id, _In_ uint32_t n,
                                                  _Inout_ sai_attribute_t* attrs)
{
    return guarded(SAI_API_SCHEDULER_GROUP, __func__, [&]() -> sai_status_t {
        SchedGroupObj* g = lookup(ctx().sched_groups, id);
        if (!g) return SAI_STATUS_INVALID_OBJECT_ID;
        if (n && !attrs) return SAI_STATUS_INVALID_PARAMETER;
        for (uint32_t i = 0; i < n; i++) {
            sai_attribute_t& a = attrs[i];
            switch (a.id) {
            case SAI_SCHEDULER_GROUP_ATTR_CHILD_COUNT: a.value.u32 = (uint32_t)g->children.size(); break;
            case SAI_SCHEDULER_GROUP_ATTR_CHILD_LIST: {
                sai_status_t st = fill_list(a.value.objlist, g->children);
                if (st != SAI_STATUS_SUCCESS) return st;
                break;
            }
            case SAI_SCHEDULER_GROUP_ATTR_PORT_ID: a.value.oid = g->port; break;
            case SAI_SCHEDULER_GROUP_ATTR_LEVEL: a.value.u8 = g->level; break;
            case SAI_SCHEDULER_GROUP_ATTR_MAX_CHILDS: a.value.u8 = g->max_childs; break;
            case SAI_SCHEDULER_GROUP_ATTR_SCHEDULER_PROFILE_ID: a.value.oid = g->sched; break;
            case SAI_SCHEDULER_GROUP_ATTR_PARENT_NODE: a.value.oid = g->parent; break;
            default: return attr_unknown(i);
            }
        }
        return SAI_STATUS_SUCCESS;
    });
}

static sai_scheduler_group_api_t make_scheduler_group_api()
{
    sai_scheduler_group_api_t a;
    std::memset(&a, 0, sizeof(a));
    a.create_scheduler_group = create_scheduler_group;
    a.remove_scheduler_group = remove_scheduler_group;
    a.set_scheduler_group_attribute = set_scheduler_group_attribute;
    a.get_scheduler_group_attribute = get_scheduler_group_attribute;
    return a;
}
const sai_scheduler_group_api_t scheduler_group_api = make_scheduler_group_api();

// ------------------------------------------------------------------ WRED
static sai_status_t wred_attr(WredCfg& w, const sai_attribute_t& a, uint32_t i)
{
    switch (a.id) {
    case SAI_WRED_ATTR_GREEN_ENABLE: w.enable[0] = a.value.booldata; break;
    case SAI_WRED_ATTR_GREEN_MIN_THRESHOLD: w.min[0] = a.value.u32; break;
    case SAI_WRED_ATTR_GREEN_MAX_THRESHOLD: w.max[0] = a.value.u32; break;
    case SAI_WRED_ATTR_GREEN_DROP_PROBABILITY: w.prob[0] = a.value.u32; break;
    case SAI_WRED_ATTR_YELLOW_ENABLE: w.enable[1] = a.value.booldata; break;
    case SAI_WRED_ATTR_YELLOW_MIN_THRESHOLD: w.min[1] = a.value.u32; break;
    case SAI_WRED_ATTR_YELLOW_MAX_THRESHOLD: w.max[1] = a.value.u32; break;
    case SAI_WRED_ATTR_YELLOW_DROP_PROBABILITY: w.prob[1] = a.value.u32; break;
    case SAI_WRED_ATTR_RED_ENABLE: w.enable[2] = a.value.booldata; break;
    case SAI_WRED_ATTR_RED_MIN_THRESHOLD: w.min[2] = a.value.u32; break;
    case SAI_WRED_ATTR_RED_MAX_THRESHOLD: w.max[2] = a.value.u32; break;
    case SAI_WRED_ATTR_RED_DROP_PROBABILITY: w.prob[2] = a.value.u32; break;
    case SAI_WRED_ATTR_WEIGHT:
        if (a.value.u8 > 15) return attr_invalid(i);
        w.weight = a.value.u8;
        break;
    case SAI_WRED_ATTR_ECN_MARK_MODE:
        if (a.value.s32 < SAI_ECN_MARK_MODE_NONE || a.value.s32 > SAI_ECN_MARK_MODE_ALL) return attr_invalid(i);
        w.ecn = a.value.s32;
        break;
    default: return attr_not_supported(i);
    }
    for (int c = 0; c < 3; c++)
        if (w.prob[c] > 100) return attr_invalid(i);
    return SAI_STATUS_SUCCESS;
}
static bool wred_valid(const WredCfg& w)
{
    for (int c = 0; c < 3; c++)
        if (w.enable[c] && w.min[c] > w.max[c]) return false;
    return true;
}

static int wred_id(sai_object_id_t oid)
{
    WredObj* w = lookup(ctx().wreds, oid);
    return w ? w->id : -1;
}

static sai_status_t apply_queue_wred(const QueueObj& q)
{
    if (!qos_be()) return SAI_STATUS_SUCCESS;
    PortObj* p = lookup(ctx().ports, q.port);
    if (!p) return SAI_STATUS_INVALID_OBJECT_ID;
    return ctx().be->queue_wred_set(p->hw, q.index, wred_id(q.wred));
}

static sai_status_t create_wred(_Out_ sai_object_id_t* id, _In_ sai_object_id_t sw, _In_ uint32_t n,
                                _In_ const sai_attribute_t* attrs)
{
    return guarded(SAI_API_WRED, __func__, [&]() -> sai_status_t {
        auto& c = ctx();
        if (!id || (n && !attrs)) return SAI_STATUS_INVALID_PARAMETER;
        if (sw != c.switch_oid) return SAI_STATUS_INVALID_OBJECT_ID;
        WredObj w;
        for (uint32_t i = 0; i < n; i++) {
            sai_status_t st = wred_attr(w.cfg, attrs[i], i);
            if (st != SAI_STATUS_SUCCESS) return st;
        }
        if (!wred_valid(w.cfg)) return SAI_STATUS_INVALID_PARAMETER;
        if (qos_be()) {
            sai_status_t st = c.be->wred_create(w.cfg, w.id);
            if (st != SAI_STATUS_SUCCESS) return st;
        }
        *id = c.alloc(SAI_OBJECT_TYPE_WRED);
        c.wreds[*id] = w;
        return SAI_STATUS_SUCCESS;
    });
}

static sai_status_t remove_wred(_In_ sai_object_id_t id)
{
    return guarded(SAI_API_WRED, __func__, [&]() -> sai_status_t {
        auto& c = ctx();
        WredObj* w = lookup(c.wreds, id);
        if (!w) return SAI_STATUS_INVALID_OBJECT_ID;
        for (auto& q : c.queues) if (q.second.wred == id) return SAI_STATUS_OBJECT_IN_USE;
        if (w->id >= 0) {
            sai_status_t st = c.be->wred_destroy(w->id);
            if (st != SAI_STATUS_SUCCESS) return st;
        }
        c.wreds.erase(id);
        return SAI_STATUS_SUCCESS;
    });
}

static sai_status_t set_wred_attribute(_In_ sai_object_id_t id, _In_ const sai_attribute_t* a)
{
    return guarded(SAI_API_WRED, __func__, [&]() -> sai_status_t {
        WredObj* w = lookup(ctx().wreds, id);
        if (!w) return SAI_STATUS_INVALID_OBJECT_ID;
        if (!a) return SAI_STATUS_INVALID_PARAMETER;
        WredCfg cfg = w->cfg;
        sai_status_t st = wred_attr(cfg, *a, 0);
        if (st != SAI_STATUS_SUCCESS) return st;
        if (!wred_valid(cfg)) return attr_invalid(0);
        if (w->id >= 0 && (st = ctx().be->wred_update(w->id, cfg)) != SAI_STATUS_SUCCESS) return attr_backend_status(st, 0);
        w->cfg = cfg;
        return SAI_STATUS_SUCCESS;
    });
}

static sai_status_t get_wred_attribute(_In_ sai_object_id_t id, _In_ uint32_t n, _Inout_ sai_attribute_t* attrs)
{
    return guarded(SAI_API_WRED, __func__, [&]() -> sai_status_t {
        WredObj* w = lookup(ctx().wreds, id);
        if (!w) return SAI_STATUS_INVALID_OBJECT_ID;
        if (n && !attrs) return SAI_STATUS_INVALID_PARAMETER;
        const WredCfg& c = w->cfg;
        for (uint32_t i = 0; i < n; i++) {
            sai_attribute_t& a = attrs[i];
            switch (a.id) {
            case SAI_WRED_ATTR_GREEN_ENABLE: a.value.booldata = c.enable[0]; break;
            case SAI_WRED_ATTR_GREEN_MIN_THRESHOLD: a.value.u32 = c.min[0]; break;
            case SAI_WRED_ATTR_GREEN_MAX_THRESHOLD: a.value.u32 = c.max[0]; break;
            case SAI_WRED_ATTR_GREEN_DROP_PROBABILITY: a.value.u32 = c.prob[0]; break;
            case SAI_WRED_ATTR_YELLOW_ENABLE: a.value.booldata = c.enable[1]; break;
            case SAI_WRED_ATTR_YELLOW_MIN_THRESHOLD: a.value.u32 = c.min[1]; break;
            case SAI_WRED_ATTR_YELLOW_MAX_THRESHOLD: a.value.u32 = c.max[1]; break;
            case SAI_WRED_ATTR_YELLOW_DROP_PROBABILITY: a.value.u32 = c.prob[1]; break;
            case SAI_WRED_ATTR_RED_ENABLE: a.value.booldata = c.enable[2]; break;
            case SAI_WRED_ATTR_RED_MIN_THRESHOLD: a.value.u32 = c.min[2]; break;
            case SAI_WRED_ATTR_RED_MAX_THRESHOLD: a.value.u32 = c.max[2]; break;
            case SAI_WRED_ATTR_RED_DROP_PROBABILITY: a.value.u32 = c.prob[2]; break;
            case SAI_WRED_ATTR_WEIGHT: a.value.u8 = c.weight; break;
            case SAI_WRED_ATTR_ECN_MARK_MODE: a.value.s32 = c.ecn; break;
            default: return attr_not_supported(i);
            }
        }
        return SAI_STATUS_SUCCESS;
    });
}

static sai_wred_api_t make_wred_api()
{
    sai_wred_api_t a;
    std::memset(&a, 0, sizeof(a));
    a.create_wred = create_wred;
    a.remove_wred = remove_wred;
    a.set_wred_attribute = set_wred_attribute;
    a.get_wred_attribute = get_wred_attribute;
    return a;
}
const sai_wred_api_t wred_api = make_wred_api();

// ------------------------------------------------------ buffer pools / profiles
static bool buffer_cfg(sai_object_id_t profile, BufferCfg& b)
{
    auto& c = ctx();
    BufferProfileObj* p = lookup(c.buffer_profiles, profile);
    if (!p) return false;
    BufferPoolObj* pool = lookup(c.buffer_pools, p->pool);
    if (!pool) return false;
    b.ingress = pool->type == SAI_BUFFER_POOL_TYPE_INGRESS;
    int32_t mode = p->mode >= 0 ? p->mode : pool->mode;
    b.dynamic = mode == SAI_BUFFER_PROFILE_THRESHOLD_MODE_DYNAMIC;
    b.dynamic_th = p->dynamic_th;
    b.static_th = p->static_th;
    b.reserved = p->reserved;
    b.xoff = p->xoff;
    b.xon = p->xon;
    b.xon_offset = p->xon_offset;
    b.pool_size = pool->size;
    b.pool_xoff = pool->xoff;
    return true;
}

static sai_status_t apply_queue_buffer(const QueueObj& q)
{
    if (!qos_be()) return SAI_STATUS_SUCCESS;
    PortObj* p = lookup(ctx().ports, q.port);
    if (!p) return SAI_STATUS_INVALID_OBJECT_ID;
    BufferCfg b;
    bool have = q.buffer && buffer_cfg(q.buffer, b);
    return ctx().be->queue_buffer_set(p->hw, q.index, have ? &b : nullptr);
}

static sai_status_t apply_pg_buffer(const IpgObj& g)
{
    if (!qos_be()) return SAI_STATUS_SUCCESS;
    PortObj* p = lookup(ctx().ports, g.port);
    if (!p) return SAI_STATUS_INVALID_OBJECT_ID;
    BufferCfg b;
    bool have = g.buffer && buffer_cfg(g.buffer, b);
    return ctx().be->pg_buffer_set(p->hw, g.index, have ? &b : nullptr);
}

// Re-apply every queue / PG using a profile (or any profile of a pool).
static sai_status_t reapply_buffers(sai_object_id_t profile, sai_object_id_t pool)
{
    auto& c = ctx();
    auto uses = [&](sai_object_id_t b) {
        if (!b) return false;
        if (profile) return b == profile;
        BufferProfileObj* p = lookup(c.buffer_profiles, b);
        return p && p->pool == pool;
    };
    sai_status_t first = SAI_STATUS_SUCCESS;
    for (auto& q : c.queues)
        if (uses(q.second.buffer)) {
            sai_status_t st = apply_queue_buffer(q.second);
            if (st != SAI_STATUS_SUCCESS && first == SAI_STATUS_SUCCESS) first = st;
        }
    for (auto& g : c.ipgs)
        if (uses(g.second.buffer)) {
            sai_status_t st = apply_pg_buffer(g.second);
            if (st != SAI_STATUS_SUCCESS && first == SAI_STATUS_SUCCESS) first = st;
        }
    return first;
}

static bool profile_used(sai_object_id_t b)
{
    auto& c = ctx();
    for (auto& q : c.queues) if (q.second.buffer == b) return true;
    for (auto& g : c.ipgs) if (g.second.buffer == b) return true;
    for (auto& p : c.ports) {
        for (auto x : p.second.in_buffers) if (x == b) return true;
        for (auto x : p.second.eg_buffers) if (x == b) return true;
    }
    return false;
}

static sai_status_t pool_attr(BufferPoolObj& p, const sai_attribute_t& a, uint32_t i, bool create)
{
    switch (a.id) {
    case SAI_BUFFER_POOL_ATTR_TYPE:
        if (!create) return attr_invalid(i);
        if (a.value.s32 != SAI_BUFFER_POOL_TYPE_INGRESS && a.value.s32 != SAI_BUFFER_POOL_TYPE_EGRESS) return attr_invalid(i);
        p.type = a.value.s32;
        return SAI_STATUS_SUCCESS;
    case SAI_BUFFER_POOL_ATTR_SIZE: p.size = a.value.u64; return SAI_STATUS_SUCCESS;
    case SAI_BUFFER_POOL_ATTR_THRESHOLD_MODE:
        if (!create) return attr_invalid(i);
        p.mode = a.value.s32;
        return SAI_STATUS_SUCCESS;
    case SAI_BUFFER_POOL_ATTR_XOFF_SIZE: p.xoff = a.value.u64; return SAI_STATUS_SUCCESS;
    default: return attr_not_supported(i);
    }
}

static sai_status_t profile_attr(BufferProfileObj& p, const sai_attribute_t& a, uint32_t i, bool create)
{
    switch (a.id) {
    case SAI_BUFFER_PROFILE_ATTR_POOL_ID:
        if (!create) return attr_invalid(i);
        if (!lookup(ctx().buffer_pools, a.value.oid)) return attr_invalid(i);
        p.pool = a.value.oid;
        return SAI_STATUS_SUCCESS;
    case SAI_BUFFER_PROFILE_ATTR_RESERVED_BUFFER_SIZE: p.reserved = a.value.u64; return SAI_STATUS_SUCCESS;
    case SAI_BUFFER_PROFILE_ATTR_THRESHOLD_MODE:
        if (!create) return attr_invalid(i);
        p.mode = a.value.s32;
        return SAI_STATUS_SUCCESS;
    case SAI_BUFFER_PROFILE_ATTR_SHARED_DYNAMIC_TH: p.dynamic_th = a.value.s8; return SAI_STATUS_SUCCESS;
    case SAI_BUFFER_PROFILE_ATTR_SHARED_STATIC_TH: p.static_th = a.value.u64; return SAI_STATUS_SUCCESS;
    case SAI_BUFFER_PROFILE_ATTR_XOFF_TH: p.xoff = a.value.u64; return SAI_STATUS_SUCCESS;
    case SAI_BUFFER_PROFILE_ATTR_XON_TH: p.xon = a.value.u64; return SAI_STATUS_SUCCESS;
    case SAI_BUFFER_PROFILE_ATTR_XON_OFFSET_TH: p.xon_offset = a.value.u64; return SAI_STATUS_SUCCESS;
    default: return attr_not_supported(i);
    }
}

static sai_status_t create_buffer_pool(_Out_ sai_object_id_t* id, _In_ sai_object_id_t sw, _In_ uint32_t n,
                                       _In_ const sai_attribute_t* attrs)
{
    return guarded(SAI_API_BUFFER, __func__, [&]() -> sai_status_t {
        auto& c = ctx();
        if (!id || (n && !attrs)) return SAI_STATUS_INVALID_PARAMETER;
        if (sw != c.switch_oid) return SAI_STATUS_INVALID_OBJECT_ID;
        BufferPoolObj p;
        bool type = false, size = false;
        for (uint32_t i = 0; i < n; i++) {
            sai_status_t st = pool_attr(p, attrs[i], i, true);
            if (st != SAI_STATUS_SUCCESS) return st;
            type |= attrs[i].id == SAI_BUFFER_POOL_ATTR_TYPE;
            size |= attrs[i].id == SAI_BUFFER_POOL_ATTR_SIZE;
        }
        if (!type || !size) return SAI_STATUS_MANDATORY_ATTRIBUTE_MISSING;
        *id = c.alloc(SAI_OBJECT_TYPE_BUFFER_POOL);
        c.buffer_pools[*id] = p;
        return SAI_STATUS_SUCCESS;
    });
}

static sai_status_t remove_buffer_pool(_In_ sai_object_id_t id)
{
    return guarded(SAI_API_BUFFER, __func__, [&]() -> sai_status_t {
        auto& c = ctx();
        if (!lookup(c.buffer_pools, id)) return SAI_STATUS_INVALID_OBJECT_ID;
        for (auto& b : c.buffer_profiles) if (b.second.pool == id) return SAI_STATUS_OBJECT_IN_USE;
        c.buffer_pools.erase(id);
        return SAI_STATUS_SUCCESS;
    });
}

static sai_status_t set_buffer_pool_attribute(_In_ sai_object_id_t id, _In_ const sai_attribute_t* a)
{
    return guarded(SAI_API_BUFFER, __func__, [&]() -> sai_status_t {
        BufferPoolObj* p = lookup(ctx().buffer_pools, id);
        if (!p) return SAI_STATUS_INVALID_OBJECT_ID;
        if (!a) return SAI_STATUS_INVALID_PARAMETER;
        BufferPoolObj old = *p;
        sai_status_t st = pool_attr(*p, *a, 0, false);
        if (st != SAI_STATUS_SUCCESS) return st;
        if ((st = reapply_buffers(0, id)) != SAI_STATUS_SUCCESS) {
            *p = old;
            reapply_buffers(0, id);
            return attr_backend_status(st, 0);
        }
        return SAI_STATUS_SUCCESS;
    });
}

static sai_status_t get_buffer_pool_attribute(_In_ sai_object_id_t id, _In_ uint32_t n, _Inout_ sai_attribute_t* attrs)
{
    return guarded(SAI_API_BUFFER, __func__, [&]() -> sai_status_t {
        BufferPoolObj* p = lookup(ctx().buffer_pools, id);
        if (!p) return SAI_STATUS_INVALID_OBJECT_ID;
        if (n && !attrs) return SAI_STATUS_INVALID_PARAMETER;
        for (uint32_t i = 0; i < n; i++) {
            sai_attribute_t& a = attrs[i];
            switch (a.id) {
            case SAI_BUFFER_POOL_ATTR_SHARED_SIZE: a.value.u64 = p->size; break;   // reservations not subtracted
            case SAI_BUFFER_POOL_ATTR_TYPE: a.value.s32 = p->type; break;
            case SAI_BUFFER_POOL_ATTR_SIZE: a.value.u64 = p->size; break;
            case SAI_BUFFER_POOL_ATTR_THRESHOLD_MODE: a.value.s32 = p->mode; break;
            case SAI_BUFFER_POOL_ATTR_XOFF_SIZE: a.value.u64 = p->xoff; break;
            default: return attr_not_supported(i);
            }
        }
        return SAI_STATUS_SUCCESS;
    });
}

static sai_status_t get_buffer_pool_stats(_In_ sai_object_id_t id, _In_ uint32_t n, _In_ const sai_stat_id_t* ids,
                                          _Out_ uint64_t* out)
{
    return guarded(SAI_API_BUFFER, __func__, [&]() -> sai_status_t {
        BufferPoolObj* p = lookup(ctx().buffer_pools, id);
        if (!p) return SAI_STATUS_INVALID_OBJECT_ID;
        if (n && (!ids || !out)) return SAI_STATUS_INVALID_PARAMETER;
        if (!qos_be()) return SAI_STATUS_NOT_SUPPORTED;
        return ctx().be->pool_stats_get(p->type == SAI_BUFFER_POOL_TYPE_INGRESS, n, ids, out);
    });
}
static sai_status_t get_buffer_pool_stats_ext(_In_ sai_object_id_t id, _In_ uint32_t n, _In_ const sai_stat_id_t* ids,
                                              _In_ sai_stats_mode_t mode, _Out_ uint64_t* out)
{
    if (mode != SAI_STATS_MODE_READ) return SAI_STATUS_NOT_SUPPORTED;
    return get_buffer_pool_stats(id, n, ids, out);
}
static sai_status_t clear_buffer_pool_stats(_In_ sai_object_id_t, _In_ uint32_t, _In_ const sai_stat_id_t*)
{
    return SAI_STATUS_NOT_SUPPORTED;
}

static sai_status_t create_buffer_profile(_Out_ sai_object_id_t* id, _In_ sai_object_id_t sw, _In_ uint32_t n,
                                          _In_ const sai_attribute_t* attrs)
{
    return guarded(SAI_API_BUFFER, __func__, [&]() -> sai_status_t {
        auto& c = ctx();
        if (!id || (n && !attrs)) return SAI_STATUS_INVALID_PARAMETER;
        if (sw != c.switch_oid) return SAI_STATUS_INVALID_OBJECT_ID;
        BufferProfileObj p;
        bool pool = false, size = false;
        for (uint32_t i = 0; i < n; i++) {
            sai_status_t st = profile_attr(p, attrs[i], i, true);
            if (st != SAI_STATUS_SUCCESS) return st;
            pool |= attrs[i].id == SAI_BUFFER_PROFILE_ATTR_POOL_ID;
            size |= attrs[i].id == SAI_BUFFER_PROFILE_ATTR_RESERVED_BUFFER_SIZE;
        }
        if (!pool || !size) return SAI_STATUS_MANDATORY_ATTRIBUTE_MISSING;
        *id = c.alloc(SAI_OBJECT_TYPE_BUFFER_PROFILE);
        c.buffer_profiles[*id] = p;
        return SAI_STATUS_SUCCESS;
    });
}

static sai_status_t remove_buffer_profile(_In_ sai_object_id_t id)
{
    return guarded(SAI_API_BUFFER, __func__, [&]() -> sai_status_t {
        if (!lookup(ctx().buffer_profiles, id)) return SAI_STATUS_INVALID_OBJECT_ID;
        if (profile_used(id)) return SAI_STATUS_OBJECT_IN_USE;
        ctx().buffer_profiles.erase(id);
        return SAI_STATUS_SUCCESS;
    });
}

static sai_status_t set_buffer_profile_attribute(_In_ sai_object_id_t id, _In_ const sai_attribute_t* a)
{
    return guarded(SAI_API_BUFFER, __func__, [&]() -> sai_status_t {
        BufferProfileObj* p = lookup(ctx().buffer_profiles, id);
        if (!p) return SAI_STATUS_INVALID_OBJECT_ID;
        if (!a) return SAI_STATUS_INVALID_PARAMETER;
        BufferProfileObj old = *p;
        sai_status_t st = profile_attr(*p, *a, 0, false);
        if (st != SAI_STATUS_SUCCESS) return st;
        if ((st = reapply_buffers(id, 0)) != SAI_STATUS_SUCCESS) {
            *p = old;
            reapply_buffers(id, 0);
            return attr_backend_status(st, 0);
        }
        return SAI_STATUS_SUCCESS;
    });
}

static sai_status_t get_buffer_profile_attribute(_In_ sai_object_id_t id, _In_ uint32_t n,
                                                 _Inout_ sai_attribute_t* attrs)
{
    return guarded(SAI_API_BUFFER, __func__, [&]() -> sai_status_t {
        auto& c = ctx();
        BufferProfileObj* p = lookup(c.buffer_profiles, id);
        if (!p) return SAI_STATUS_INVALID_OBJECT_ID;
        if (n && !attrs) return SAI_STATUS_INVALID_PARAMETER;
        for (uint32_t i = 0; i < n; i++) {
            sai_attribute_t& a = attrs[i];
            switch (a.id) {
            case SAI_BUFFER_PROFILE_ATTR_POOL_ID: a.value.oid = p->pool; break;
            case SAI_BUFFER_PROFILE_ATTR_RESERVED_BUFFER_SIZE: a.value.u64 = p->reserved; break;
            case SAI_BUFFER_PROFILE_ATTR_THRESHOLD_MODE: {
                BufferPoolObj* pool = lookup(c.buffer_pools, p->pool);
                a.value.s32 = p->mode >= 0 ? p->mode : pool ? pool->mode : SAI_BUFFER_PROFILE_THRESHOLD_MODE_DYNAMIC;
                break;
            }
            case SAI_BUFFER_PROFILE_ATTR_SHARED_DYNAMIC_TH: a.value.s8 = p->dynamic_th; break;
            case SAI_BUFFER_PROFILE_ATTR_SHARED_STATIC_TH: a.value.u64 = p->static_th; break;
            case SAI_BUFFER_PROFILE_ATTR_XOFF_TH: a.value.u64 = p->xoff; break;
            case SAI_BUFFER_PROFILE_ATTR_XON_TH: a.value.u64 = p->xon; break;
            case SAI_BUFFER_PROFILE_ATTR_XON_OFFSET_TH: a.value.u64 = p->xon_offset; break;
            default: return attr_not_supported(i);
            }
        }
        return SAI_STATUS_SUCCESS;
    });
}

// ----------------------------------------------- ingress priority groups
static sai_status_t get_ipg_attribute(_In_ sai_object_id_t id, _In_ uint32_t n, _Inout_ sai_attribute_t* attrs)
{
    return guarded(SAI_API_BUFFER, __func__, [&]() -> sai_status_t {
        IpgObj* g = lookup(ctx().ipgs, id);
        if (!g) return SAI_STATUS_INVALID_OBJECT_ID;
        if (n && !attrs) return SAI_STATUS_INVALID_PARAMETER;
        for (uint32_t i = 0; i < n; i++) {
            sai_attribute_t& a = attrs[i];
            switch (a.id) {
            case SAI_INGRESS_PRIORITY_GROUP_ATTR_PORT: a.value.oid = g->port; break;
            case SAI_INGRESS_PRIORITY_GROUP_ATTR_INDEX: a.value.u8 = g->index; break;
            case SAI_INGRESS_PRIORITY_GROUP_ATTR_BUFFER_PROFILE: a.value.oid = g->buffer; break;
            default: return attr_not_supported(i);
            }
        }
        return SAI_STATUS_SUCCESS;
    });
}

static sai_status_t set_ipg_attribute(_In_ sai_object_id_t id, _In_ const sai_attribute_t* a)
{
    return guarded(SAI_API_BUFFER, __func__, [&]() -> sai_status_t {
        auto& c = ctx();
        IpgObj* g = lookup(c.ipgs, id);
        if (!g) return SAI_STATUS_INVALID_OBJECT_ID;
        if (!a) return SAI_STATUS_INVALID_PARAMETER;
        if (a->id != SAI_INGRESS_PRIORITY_GROUP_ATTR_BUFFER_PROFILE) return attr_not_supported(0);
        BufferProfileObj* p = a->value.oid ? lookup(c.buffer_profiles, a->value.oid) : nullptr;
        if (a->value.oid && !p) return attr_invalid(0);
        if (p) {
            BufferPoolObj* pool = lookup(c.buffer_pools, p->pool);
            if (!pool || pool->type != SAI_BUFFER_POOL_TYPE_INGRESS) return attr_invalid(0);
        }
        sai_object_id_t old = g->buffer;
        g->buffer = a->value.oid;
        sai_status_t st = apply_pg_buffer(*g);
        if (st != SAI_STATUS_SUCCESS) {
            g->buffer = old;
            return attr_backend_status(st, 0);
        }
        return SAI_STATUS_SUCCESS;
    });
}

static sai_status_t get_ipg_stats(_In_ sai_object_id_t id, _In_ uint32_t n, _In_ const sai_stat_id_t* ids,
                                  _Out_ uint64_t* out)
{
    return guarded(SAI_API_BUFFER, __func__, [&]() -> sai_status_t {
        IpgObj* g = lookup(ctx().ipgs, id);
        if (!g) return SAI_STATUS_INVALID_OBJECT_ID;
        if (n && (!ids || !out)) return SAI_STATUS_INVALID_PARAMETER;
        PortObj* p = lookup(ctx().ports, g->port);
        if (!qos_be() || !p) return SAI_STATUS_NOT_SUPPORTED;
        return ctx().be->pg_stats_get(p->hw, g->index, n, ids, out);
    });
}

static sai_status_t clear_ipg_stats(_In_ sai_object_id_t id, _In_ uint32_t n, _In_ const sai_stat_id_t* ids)
{
    return guarded(SAI_API_BUFFER, __func__, [&]() -> sai_status_t {
        IpgObj* g = lookup(ctx().ipgs, id);
        if (!g) return SAI_STATUS_INVALID_OBJECT_ID;
        if (n && !ids) return SAI_STATUS_INVALID_PARAMETER;
        PortObj* p = lookup(ctx().ports, g->port);
        if (!qos_be() || !p) return SAI_STATUS_NOT_SUPPORTED;
        return ctx().be->pg_stats_clear(p->hw, g->index, n, ids);
    });
}

static sai_status_t get_ipg_stats_ext(_In_ sai_object_id_t id, _In_ uint32_t n, _In_ const sai_stat_id_t* ids,
                                      _In_ sai_stats_mode_t mode, _Out_ uint64_t* out)
{
    sai_status_t st = get_ipg_stats(id, n, ids, out);
    if (st == SAI_STATUS_SUCCESS && mode == SAI_STATS_MODE_READ_AND_CLEAR) st = clear_ipg_stats(id, n, ids);
    return st;
}

static sai_buffer_api_t make_buffer_api()
{
    sai_buffer_api_t a;
    std::memset(&a, 0, sizeof(a));
    a.create_buffer_pool = create_buffer_pool;
    a.remove_buffer_pool = remove_buffer_pool;
    a.set_buffer_pool_attribute = set_buffer_pool_attribute;
    a.get_buffer_pool_attribute = get_buffer_pool_attribute;
    a.get_buffer_pool_stats = get_buffer_pool_stats;
    a.get_buffer_pool_stats_ext = get_buffer_pool_stats_ext;
    a.clear_buffer_pool_stats = clear_buffer_pool_stats;
    a.get_ingress_priority_group_attribute = get_ipg_attribute;
    a.set_ingress_priority_group_attribute = set_ipg_attribute;
    a.get_ingress_priority_group_stats = get_ipg_stats;
    a.get_ingress_priority_group_stats_ext = get_ipg_stats_ext;
    a.clear_ingress_priority_group_stats = clear_ipg_stats;
    a.create_buffer_profile = create_buffer_profile;
    a.remove_buffer_profile = remove_buffer_profile;
    a.set_buffer_profile_attribute = set_buffer_profile_attribute;
    a.get_buffer_profile_attribute = get_buffer_profile_attribute;
    return a;
}
const sai_buffer_api_t buffer_api = make_buffer_api();

// ------------------------------------------------------------------ queues
static sai_status_t get_queue_attribute(_In_ sai_object_id_t qid, _In_ uint32_t n, _Inout_ sai_attribute_t* attrs)
{
    return guarded(SAI_API_QUEUE, __func__, [&]() -> sai_status_t {
        QueueObj* q = lookup(ctx().queues, qid);
        if (!q) return SAI_STATUS_INVALID_OBJECT_ID;
        if (n && !attrs) return SAI_STATUS_INVALID_PARAMETER;
        for (uint32_t i = 0; i < n; i++) {
            sai_attribute_t& a = attrs[i];
            switch (a.id) {
            case SAI_QUEUE_ATTR_TYPE: a.value.s32 = q->type; break;
            case SAI_QUEUE_ATTR_INDEX: a.value.u8 = q->index; break;
            case SAI_QUEUE_ATTR_PORT: a.value.oid = q->port; break;
            case SAI_QUEUE_ATTR_PARENT_SCHEDULER_NODE: a.value.oid = q->group && qos_be() ? q->group : q->port; break;
            case SAI_QUEUE_ATTR_SCHEDULER_PROFILE_ID: a.value.oid = q->sched; break;
            case SAI_QUEUE_ATTR_WRED_PROFILE_ID: a.value.oid = q->wred; break;
            case SAI_QUEUE_ATTR_BUFFER_PROFILE_ID: a.value.oid = q->buffer; break;
            case SAI_QUEUE_ATTR_PFC_DLR_INIT: a.value.booldata = q->pfc_dlr; break;
            default: return attr_not_supported(i);
            }
        }
        return SAI_STATUS_SUCCESS;
    });
}

static sai_status_t set_queue_attribute(_In_ sai_object_id_t qid, _In_ const sai_attribute_t* a)
{
    return guarded(SAI_API_QUEUE, __func__, [&]() -> sai_status_t {
        auto& c = ctx();
        QueueObj* q = lookup(c.queues, qid);
        if (!q) return SAI_STATUS_INVALID_OBJECT_ID;
        if (!a) return SAI_STATUS_INVALID_PARAMETER;
        sai_status_t st;
        switch (a->id) {
        case SAI_QUEUE_ATTR_SCHEDULER_PROFILE_ID: {
            if (a->value.oid && !lookup(c.schedulers, a->value.oid)) return attr_invalid(0);
            sai_object_id_t old = q->sched;
            q->sched = a->value.oid;
            if ((st = apply_queue_sched(*q)) != SAI_STATUS_SUCCESS) { q->sched = old; return attr_backend_status(st, 0); }
            return SAI_STATUS_SUCCESS;
        }
        case SAI_QUEUE_ATTR_WRED_PROFILE_ID: {
            if (a->value.oid && !lookup(c.wreds, a->value.oid)) return attr_invalid(0);
            sai_object_id_t old = q->wred;
            q->wred = a->value.oid;
            if ((st = apply_queue_wred(*q)) != SAI_STATUS_SUCCESS) { q->wred = old; return attr_backend_status(st, 0); }
            return SAI_STATUS_SUCCESS;
        }
        case SAI_QUEUE_ATTR_BUFFER_PROFILE_ID: {
            BufferProfileObj* p = a->value.oid ? lookup(c.buffer_profiles, a->value.oid) : nullptr;
            if (a->value.oid && !p) return attr_invalid(0);
            if (p) {
                BufferPoolObj* pool = lookup(c.buffer_pools, p->pool);
                if (!pool || pool->type != SAI_BUFFER_POOL_TYPE_EGRESS) return attr_invalid(0);
            }
            sai_object_id_t old = q->buffer;
            q->buffer = a->value.oid;
            if ((st = apply_queue_buffer(*q)) != SAI_STATUS_SUCCESS) { q->buffer = old; return attr_backend_status(st, 0); }
            return SAI_STATUS_SUCCESS;
        }
        case SAI_QUEUE_ATTR_PFC_DLR_INIT:
            q->pfc_dlr = a->value.booldata;   // kept: no deadlock-recovery mode in the backends
            return SAI_STATUS_SUCCESS;
        default:
            return attr_not_supported(0);
        }
    });
}

static sai_status_t get_queue_stats(_In_ sai_object_id_t qid, _In_ uint32_t n, _In_ const sai_stat_id_t* ids,
                                    _Out_ uint64_t* out)
{
    return guarded(SAI_API_QUEUE, __func__, [&]() -> sai_status_t {
        QueueObj* q = lookup(ctx().queues, qid);
        if (!q) return SAI_STATUS_INVALID_OBJECT_ID;
        if (n && (!ids || !out)) return SAI_STATUS_INVALID_PARAMETER;
        PortObj* p = lookup(ctx().ports, q->port);
        if (!qos_be() || !p || p->cpu) return SAI_STATUS_NOT_SUPPORTED;
        return ctx().be->queue_stats_get(p->hw, q->index, n, ids, out);
    });
}

static sai_status_t clear_queue_stats(_In_ sai_object_id_t qid, _In_ uint32_t n, _In_ const sai_stat_id_t* ids)
{
    return guarded(SAI_API_QUEUE, __func__, [&]() -> sai_status_t {
        QueueObj* q = lookup(ctx().queues, qid);
        if (!q) return SAI_STATUS_INVALID_OBJECT_ID;
        if (n && !ids) return SAI_STATUS_INVALID_PARAMETER;
        PortObj* p = lookup(ctx().ports, q->port);
        if (!qos_be() || !p || p->cpu) return SAI_STATUS_NOT_SUPPORTED;
        return ctx().be->queue_stats_clear(p->hw, q->index, n, ids);
    });
}

static sai_status_t get_queue_stats_ext(_In_ sai_object_id_t qid, _In_ uint32_t n, _In_ const sai_stat_id_t* ids,
                                        _In_ sai_stats_mode_t mode, _Out_ uint64_t* out)
{
    sai_status_t st = get_queue_stats(qid, n, ids, out);
    if (st == SAI_STATUS_SUCCESS && mode == SAI_STATS_MODE_READ_AND_CLEAR) st = clear_queue_stats(qid, n, ids);
    return st;
}

static sai_queue_api_t make_queue_api()
{
    sai_queue_api_t a;
    std::memset(&a, 0, sizeof(a));
    a.set_queue_attribute = set_queue_attribute;
    a.get_queue_attribute = get_queue_attribute;
    a.get_queue_stats = get_queue_stats;
    a.get_queue_stats_ext = get_queue_stats_ext;
    a.clear_queue_stats = clear_queue_stats;
    return a;
}
const sai_queue_api_t queue_api = make_queue_api();

// ------------------------------------------------- port / switch attributes
static sai_status_t set_pfc(PortObj& p)
{
    if (!qos_be() || p.cpu) return SAI_STATUS_SUCCESS;
    bool sep = p.pfc_mode == SAI_PORT_PRIORITY_FLOW_CONTROL_MODE_SEPARATE;
    return ctx().be->port_pfc_set(p.hw, sep, sep ? p.pfc_tx : p.pfc, sep ? p.pfc_rx : p.pfc);
}

static bool buffer_list_valid(const sai_object_list_t& l, int32_t pool_type)
{
    auto& c = ctx();
    if (l.count && !l.list) return false;
    for (uint32_t k = 0; k < l.count; k++) {
        BufferProfileObj* b = lookup(c.buffer_profiles, l.list[k]);
        BufferPoolObj* pool = b ? lookup(c.buffer_pools, b->pool) : nullptr;
        if (!pool || pool->type != pool_type) return false;
    }
    return true;
}

sai_status_t port_qos_set(PortObj& p, const sai_attribute_t& a, uint32_t i, bool& handled)
{
    auto& c = ctx();
    handled = true;
    QosMapKind k;
    sai_status_t st;
    if (port_map_attr(a.id, k)) {
        if (a.value.oid) {
            QosMapObj* m = qos_map_of(a.value.oid);
            QosMapKind mk;
            if (!m || !map_kind(m->type, mk) || mk != k) return attr_invalid(i);
        }
        sai_object_id_t old = p.qos_map[(int)k];
        p.qos_map[(int)k] = a.value.oid;
        if ((st = apply_port_map(p, k)) != SAI_STATUS_SUCCESS) {
            p.qos_map[(int)k] = old;
            return attr_backend_status(st, i);
        }
        return SAI_STATUS_SUCCESS;
    }
    switch (a.id) {
    case SAI_PORT_ATTR_QOS_DEFAULT_TC:
        if (a.value.u8 > 15) return attr_invalid(i);
        if (qos_be() && !p.cpu && (st = c.be->port_default_tc_set(p.hw, a.value.u8)) != SAI_STATUS_SUCCESS)
            return attr_backend_status(st, i);
        p.default_tc = a.value.u8;
        return SAI_STATUS_SUCCESS;
    case SAI_PORT_ATTR_PRIORITY_FLOW_CONTROL:
    case SAI_PORT_ATTR_PRIORITY_FLOW_CONTROL_TX:
    case SAI_PORT_ATTR_PRIORITY_FLOW_CONTROL_RX:
    case SAI_PORT_ATTR_PRIORITY_FLOW_CONTROL_MODE: {
        PortObj old = p;
        if (a.id == SAI_PORT_ATTR_PRIORITY_FLOW_CONTROL) p.pfc = a.value.u8;
        else if (a.id == SAI_PORT_ATTR_PRIORITY_FLOW_CONTROL_TX) p.pfc_tx = a.value.u8;
        else if (a.id == SAI_PORT_ATTR_PRIORITY_FLOW_CONTROL_RX) p.pfc_rx = a.value.u8;
        else {
            if (a.value.s32 != SAI_PORT_PRIORITY_FLOW_CONTROL_MODE_COMBINED &&
                a.value.s32 != SAI_PORT_PRIORITY_FLOW_CONTROL_MODE_SEPARATE)
                return attr_invalid(i);
            p.pfc_mode = a.value.s32;
        }
        if ((st = set_pfc(p)) != SAI_STATUS_SUCCESS) {
            p.pfc = old.pfc; p.pfc_tx = old.pfc_tx; p.pfc_rx = old.pfc_rx; p.pfc_mode = old.pfc_mode;
            return attr_backend_status(st, i);
        }
        return SAI_STATUS_SUCCESS;
    }
    case SAI_PORT_ATTR_QOS_SCHEDULER_PROFILE_ID: {
        if (a.value.oid && !lookup(c.schedulers, a.value.oid)) return attr_invalid(i);
        sai_object_id_t old = p.scheduler;
        p.scheduler = a.value.oid;
        if ((st = apply_port_sched(p)) != SAI_STATUS_SUCCESS) {
            p.scheduler = old;
            return attr_backend_status(st, i);
        }
        return SAI_STATUS_SUCCESS;
    }
    case SAI_PORT_ATTR_QOS_INGRESS_BUFFER_PROFILE_LIST:
    case SAI_PORT_ATTR_QOS_EGRESS_BUFFER_PROFILE_LIST: {
        bool in = a.id == SAI_PORT_ATTR_QOS_INGRESS_BUFFER_PROFILE_LIST;
        if (!buffer_list_valid(a.value.objlist, in ? SAI_BUFFER_POOL_TYPE_INGRESS : SAI_BUFFER_POOL_TYPE_EGRESS))
            return attr_invalid(i);
        // Kept: per-port pool limits are not programmed (the queue / PG profiles are).
        (in ? p.in_buffers : p.eg_buffers).assign(a.value.objlist.list, a.value.objlist.list + a.value.objlist.count);
        return SAI_STATUS_SUCCESS;
    }
    default:
        handled = false;
        return SAI_STATUS_SUCCESS;
    }
}

sai_status_t port_qos_get(PortObj& p, sai_attribute_t& a, uint32_t i, bool& handled)
{
    handled = true;
    QosMapKind k;
    if (port_map_attr(a.id, k)) { a.value.oid = p.qos_map[(int)k]; return SAI_STATUS_SUCCESS; }
    switch (a.id) {
    case SAI_PORT_ATTR_QOS_DEFAULT_TC: a.value.u8 = p.default_tc; return SAI_STATUS_SUCCESS;
    case SAI_PORT_ATTR_PRIORITY_FLOW_CONTROL: a.value.u8 = p.pfc; return SAI_STATUS_SUCCESS;
    case SAI_PORT_ATTR_PRIORITY_FLOW_CONTROL_TX: a.value.u8 = p.pfc_tx; return SAI_STATUS_SUCCESS;
    case SAI_PORT_ATTR_PRIORITY_FLOW_CONTROL_RX: a.value.u8 = p.pfc_rx; return SAI_STATUS_SUCCESS;
    case SAI_PORT_ATTR_PRIORITY_FLOW_CONTROL_MODE: a.value.s32 = p.pfc_mode; return SAI_STATUS_SUCCESS;
    case SAI_PORT_ATTR_QOS_SCHEDULER_PROFILE_ID: a.value.oid = p.scheduler; return SAI_STATUS_SUCCESS;
    // The groups exist for every backend; a backend without QoS shows none (as before them).
    case SAI_PORT_ATTR_QOS_NUMBER_OF_SCHEDULER_GROUPS: a.value.u32 = qos_be() ? (uint32_t)p.sched_groups.size() : 0; return SAI_STATUS_SUCCESS;
    case SAI_PORT_ATTR_QOS_SCHEDULER_GROUP_LIST: return fill_list(a.value.objlist, qos_be() ? p.sched_groups : std::vector<sai_object_id_t>());
    case SAI_PORT_ATTR_QOS_INGRESS_BUFFER_PROFILE_LIST: return fill_list(a.value.objlist, p.in_buffers);
    case SAI_PORT_ATTR_QOS_EGRESS_BUFFER_PROFILE_LIST: return fill_list(a.value.objlist, p.eg_buffers);
    default:
        (void)i;
        handled = false;
        return SAI_STATUS_SUCCESS;
    }
}

sai_status_t switch_qos_set(const sai_attribute_t& a, bool& handled)
{
    auto& c = ctx();
    QosMapKind k;
    handled = switch_map_attr(a.id, k);
    if (!handled) return SAI_STATUS_SUCCESS;
    if (a.value.oid) {
        QosMapObj* m = qos_map_of(a.value.oid);
        QosMapKind mk;
        if (!m || !map_kind(m->type, mk) || mk != k) return attr_invalid(0);
    }
    sai_object_id_t old = c.switch_qos_map[(int)k];
    c.switch_qos_map[(int)k] = a.value.oid;
    for (auto& p : c.ports) {
        if (p.second.cpu || p.second.qos_map[(int)k]) continue;
        sai_status_t st = apply_port_map(p.second, k);
        if (st != SAI_STATUS_SUCCESS) {
            c.switch_qos_map[(int)k] = old;
            for (auto& q : c.ports)
                if (!q.second.cpu && !q.second.qos_map[(int)k]) apply_port_map(q.second, k);
            return attr_backend_status(st, 0);
        }
    }
    return SAI_STATUS_SUCCESS;
}

sai_status_t switch_qos_get(sai_attribute_t& a, bool& handled)
{
    QosMapKind k;
    handled = switch_map_attr(a.id, k);
    if (handled) a.value.oid = ctx().switch_qos_map[(int)k];
    return SAI_STATUS_SUCCESS;
}

bool port_pfc_stat(sai_stat_id_t id, int& prio, bool& tx)
{
    static const sai_stat_id_t rx[8] = {
        SAI_PORT_STAT_PFC_0_RX_PKTS, SAI_PORT_STAT_PFC_1_RX_PKTS, SAI_PORT_STAT_PFC_2_RX_PKTS, SAI_PORT_STAT_PFC_3_RX_PKTS,
        SAI_PORT_STAT_PFC_4_RX_PKTS, SAI_PORT_STAT_PFC_5_RX_PKTS, SAI_PORT_STAT_PFC_6_RX_PKTS, SAI_PORT_STAT_PFC_7_RX_PKTS};
    static const sai_stat_id_t txs[8] = {
        SAI_PORT_STAT_PFC_0_TX_PKTS, SAI_PORT_STAT_PFC_1_TX_PKTS, SAI_PORT_STAT_PFC_2_TX_PKTS, SAI_PORT_STAT_PFC_3_TX_PKTS,
        SAI_PORT_STAT_PFC_4_TX_PKTS, SAI_PORT_STAT_PFC_5_TX_PKTS, SAI_PORT_STAT_PFC_6_TX_PKTS, SAI_PORT_STAT_PFC_7_TX_PKTS};
    for (int i = 0; i < 8; i++) {
        if (id == rx[i]) { prio = i; tx = false; return true; }
        if (id == txs[i]) { prio = i; tx = true; return true; }
    }
    return false;
}

// Probe the backend with each counter on the first front-panel port's queue /
// PG 0 (the backends answer per id: NOT_SUPPORTED for one they lack).
sai_status_t qos_stats_capability(sai_object_type_t t, sai_stat_capability_list_t* out)
{
    return guarded(SAI_API_QUEUE, __func__, [&]() -> sai_status_t {
        auto& c = ctx();
        if (!qos_be() || c.port_list.empty()) return SAI_STATUS_NOT_SUPPORTED;
        PortObj* p = lookup(c.ports, c.port_list[0]);
        if (!p) return SAI_STATUS_NOT_SUPPORTED;
        bool q = t == SAI_OBJECT_TYPE_QUEUE;
        uint32_t last = q ? 0x40 : 0x20;   // past the last SAI_QUEUE_STAT_* / _PRIORITY_GROUP_STAT_* the backends know
        std::vector<sai_stat_capability_t> v;
        for (uint32_t id = 0; id <= last; id++) {
            sai_stat_id_t s = id;
            uint64_t val = 0;
            sai_status_t st = q ? c.be->queue_stats_get(p->hw, 0, 1, &s, &val) : c.be->pg_stats_get(p->hw, 0, 1, &s, &val);
            if (st != SAI_STATUS_SUCCESS) continue;
            sai_stat_capability_t cap;
            cap.stat_enum = id;
            cap.stat_modes = SAI_STATS_MODE_READ | SAI_STATS_MODE_READ_AND_CLEAR;
            v.push_back(cap);
        }
        return fill_list(*out, v);
    }, false);
}

void qos_clear_objects()
{
    auto& c = ctx();   // the backend's objects went with its shutdown
    c.qos_maps.clear(); c.schedulers.clear(); c.sched_groups.clear(); c.wreds.clear();
    c.buffer_pools.clear(); c.buffer_profiles.clear();
    for (auto& m : c.switch_qos_map) m = 0;
}

} // namespace ob
