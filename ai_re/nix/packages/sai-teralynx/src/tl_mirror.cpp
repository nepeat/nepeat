// SPDX-License-Identifier: Apache-2.0
// Mirror sessions and sampling (tl_mirror.h).
#include "tl_mirror.h"
#include "tl_sflow.h"

#include <cstdio>

namespace ob {
namespace tlb {

static int dir_ix(bool ingress) { return ingress ? 0 : 1; }
static uint32_t dir_val(bool ingress) { return ingress ? IFCS_TRAFFIC_MONITOR_DIRECTION_INGRESS : IFCS_TRAFFIC_MONITOR_DIRECTION_EGRESS; }

// One DCDP per sysport: the SDK refuses a second one ("Sysport in use by
// DCDP", chip 2026-11-16), so sessions on the same monitor port share it
// (isai: the same). hw -1 = the CPU (sFlow).
sai_status_t MirrorArea::dcdp_get(int hw, ifcs_handle_t& h)
{
    auto it = dcdps_.find(hw);
    if (it != dcdps_.end()) {
        it->second.second++;
        h = it->second.first;
        return SAI_STATUS_SUCCESS;
    }
    sai_status_t st = dcdp_create(hw, h);
    if (st == SAI_STATUS_SUCCESS) dcdps_[hw] = std::make_pair(h, 1);
    return st;
}

void MirrorArea::dcdp_put(int hw)
{
    auto it = dcdps_.find(hw);
    if (it == dcdps_.end() || --it->second.second > 0) return;
    c_.call(c_.L.ifcs_dup_copy_dest_port_delete(NODE, it->second.first), "dcdp delete");
    dcdps_.erase(it);
}

sai_status_t MirrorArea::dcdp_create(int hw, ifcs_handle_t& h)
{
    ifcs_handle_t sp;
    if (hw < 0) {   // the CPU (sFlow)
        ifcs_attr_t a = attr_of(IFCS_NODE_ATTR_CPU_SYSPORT);
        uint32_t n = 0;
        sp = IFCS_STATUS_REASON(c_.L.ifcs_node_attr_get(NODE, 1, &a, &n)) == IFCS_SUCCESS && a.value.handle
                 ? a.value.handle : IFCS_HANDLE_SYSPORT(1000);
    } else {
        auto it = c_.sysport.find(hw);
        if (it == c_.sysport.end()) return SAI_STATUS_INVALID_PORT_NUMBER;
        sp = it->second;
    }
    ifcs_attr_t a = attr_handle(IFCS_DUP_COPY_DEST_PORT_ATTR_SYSPORT, sp);
    h = IFCS_NULL_HANDLE;
    return c_.call(c_.L.ifcs_dup_copy_dest_port_create(NODE, &h, 1, &a), "dcdp create");
}

static ifcs_attr_t ip_attr(uint32_t id, const IpAddr& ip)
{
    ifcs_attr_t a = attr_of(id);
    if (ip.v6) {
        a.value.ip_addr.addr_family = IFCS_IP_ADDR_FAMILY_IPV6;
        std::memcpy(a.value.ip_addr.addr.ipv6, ip.addr, 16);
    } else {
        a.value.ip_addr.addr_family = IFCS_IP_ADDR_FAMILY_IPV4;
        a.value.ip_addr.addr.ipv4 = (uint32_t)ip.addr[0] << 24 | (uint32_t)ip.addr[1] << 16 | (uint32_t)ip.addr[2] << 8 | ip.addr[3];
    }
    return a;
}

// isai: SPAN ingress {type, direction, dcdp, truncate}, egress without
// truncate; ERSPAN {type II, direction, dcdp, dest/src MAC, src/dest IP,
// DSCP, TTL, session 0, truncate}.
sai_status_t MirrorArea::collector_make(const Session& s, bool ingress, ifcs_handle_t& h)
{
    const MirrorCfg& m = s.cfg;
    std::vector<ifcs_attr_t> a;
    a.push_back(attr_u32(IFCS_COLLECTOR_ATTR_COLLECTOR_TYPE, m.erspan ? IFCS_COLLECTOR_TYPE_ERSPAN_TYPE_II : IFCS_COLLECTOR_TYPE_SPAN));
    a.push_back(attr_u32(IFCS_COLLECTOR_ATTR_DIRECTION, dir_val(ingress)));
    a.push_back(attr_handle(IFCS_COLLECTOR_ATTR_DCDP, s.dcdp));
    if (m.erspan) {
        a.push_back(attr_mac(IFCS_COLLECTOR_ATTR_DEST_MAC, m.dmac));
        a.push_back(attr_mac(IFCS_COLLECTOR_ATTR_SRC_MAC, m.smac));
        a.push_back(ip_attr(IFCS_COLLECTOR_ATTR_SRC_IP, m.src));
        a.push_back(ip_attr(IFCS_COLLECTOR_ATTR_DEST_IP, m.dst));
        ifcs_attr_t d = attr_of(IFCS_COLLECTOR_ATTR_DSCP);
        d.value.u8 = (uint8_t)(m.tos >> 2);
        a.push_back(d);
        ifcs_attr_t t = attr_of(IFCS_COLLECTOR_ATTR_TTL);
        t.value.u8 = m.ttl;
        a.push_back(t);
        if (m.vlan) a.push_back(attr_u16(IFCS_COLLECTOR_ATTR_DOT1Q_VLAN, m.vlan_id));
        a.push_back(attr_u16(IFCS_COLLECTOR_ATTR_SESSION_ID, 0));
    }
    // TRUNCATE: on SPAN ingress collectors only (isai). 0.12.41 refuses it on
    // egress and on ERSPAN collectors ("Invalid attr check failed", chip
    // 2026-11-16); egress copies come out truncated anyway (180 B on the wire
    // for 260 B frames), ERSPAN ones there only when asked.
    if (m.erspan ? m.truncate != 0 : ingress)
        a.push_back(attr_u32(IFCS_COLLECTOR_ATTR_TRUNCATE, m.truncate ? IFCS_TRAFFIC_MONITOR_TRUNCATE_YES : IFCS_TRAFFIC_MONITOR_TRUNCATE_NO));
    h = IFCS_NULL_HANDLE;
    return c_.call(c_.L.ifcs_collector_create(NODE, &h, (uint32_t)a.size(), a.data()), "collector create");
}

sai_status_t MirrorArea::dir_get(Session& s, bool ingress)
{
    Dir& d = s.dir[dir_ix(ingress)];
    if (d.refs++ > 0) return SAI_STATUS_SUCCESS;
    sai_status_t st = collector_make(s, ingress, d.collector);
    if (st != SAI_STATUS_SUCCESS) { d.refs = 0; return st; }
    ifcs_attr_t a[2] = {attr_u32(IFCS_COLLECTOR_SET_ATTR_COLLECTOR_SET_TYPE,
                                 s.cfg.erspan ? IFCS_COLLECTOR_TYPE_ERSPAN_TYPE_II : IFCS_COLLECTOR_TYPE_SPAN),
                        attr_u32(IFCS_COLLECTOR_SET_ATTR_DIRECTION, dir_val(ingress))};
    st = c_.call(c_.L.ifcs_collector_set_create(NODE, &d.set, 2, a), "collector set create");
    if (st == SAI_STATUS_SUCCESS)
        st = c_.call(c_.L.ifcs_collector_set_member_add(NODE, d.set, 1, &d.collector, 0, nullptr), "collector set add");
    if (st != SAI_STATUS_SUCCESS) {
        if (d.set) c_.L.ifcs_collector_set_delete(NODE, d.set);
        c_.L.ifcs_collector_delete(NODE, d.collector);
        d = Dir();
    }
    return st;
}

void MirrorArea::dir_put(Session& s, bool ingress)
{
    Dir& d = s.dir[dir_ix(ingress)];
    if (d.refs == 0 || --d.refs > 0) return;
    c_.call(c_.L.ifcs_collector_set_member_remove(NODE, d.set, 1, &d.collector), "collector set remove");
    c_.call(c_.L.ifcs_collector_set_delete(NODE, d.set), "collector set delete");
    c_.call(c_.L.ifcs_collector_delete(NODE, d.collector), "collector delete");
    d = Dir();
}

sai_status_t MirrorArea::create(const MirrorCfg& m, int& id)
{
    Guard g(c_.mu);
    if (!c_.front(m.monitor)) return SAI_STATUS_INVALID_PORT_NUMBER;
    if (m.erspan && m.gre_type != 0x88be)
        c_.log_once("mirror:gre", "ERSPAN: GRE protocol type 0x%04x requested; the chip sends type II (0x88be)", m.gre_type);
    if (m.truncate && m.truncate != 192)
        c_.log_once("mirror:trunc", "mirror: TRUNCATE_SIZE %u requested; the chip truncates at its own size (isai reads 192)",
                    m.truncate);
    if (m.sample_rate > 1) {
        c_.log_once("mirror:rate", "mirror: sampled mirroring (SAMPLE_RATE %u) is not supported", m.sample_rate);
        return SAI_STATUS_NOT_SUPPORTED;
    }
    Session s;
    s.cfg = m;
    sai_status_t st = dcdp_get(m.monitor, s.dcdp);
    if (st != SAI_STATUS_SUCCESS) return st;
    id = next_++;
    sessions_[id] = s;
    return SAI_STATUS_SUCCESS;
}

sai_status_t MirrorArea::update(int id, const MirrorCfg& m)
{
    Guard g(c_.mu);
    auto it = sessions_.find(id);
    if (it == sessions_.end()) return SAI_STATUS_ITEM_NOT_FOUND;
    if (!c_.front(m.monitor)) return SAI_STATUS_INVALID_PORT_NUMBER;
    if (m.erspan != it->second.cfg.erspan) return SAI_STATUS_INVALID_PARAMETER;
    if (m.sample_rate > 1) return SAI_STATUS_NOT_SUPPORTED;
    Session& s = it->second;
    // isai: take the collectors out of their sets, a new DCDP when the
    // monitor port moved, new collectors into the same sets.
    for (int d = 0; d < 2; d++)
        if (s.dir[d].refs) {
            c_.call(c_.L.ifcs_collector_set_member_remove(NODE, s.dir[d].set, 1, &s.dir[d].collector), "collector set remove");
            c_.call(c_.L.ifcs_collector_delete(NODE, s.dir[d].collector), "collector delete");
            s.dir[d].collector = IFCS_NULL_HANDLE;
        }
    sai_status_t st = SAI_STATUS_SUCCESS;
    if (m.monitor != s.cfg.monitor) {
        ifcs_handle_t nd;
        st = dcdp_get(m.monitor, nd);
        if (st == SAI_STATUS_SUCCESS) {
            dcdp_put(s.cfg.monitor);
            s.dcdp = nd;
        }
    }
    MirrorCfg old = s.cfg;
    if (st == SAI_STATUS_SUCCESS) s.cfg = m;
    for (int d = 0; d < 2; d++) {
        if (!s.dir[d].refs) continue;
        sai_status_t r = collector_make(s, d == 0, s.dir[d].collector);
        if (r == SAI_STATUS_SUCCESS)
            r = c_.call(c_.L.ifcs_collector_set_member_add(NODE, s.dir[d].set, 1, &s.dir[d].collector, 0, nullptr),
                        "collector set add");
        if (r != SAI_STATUS_SUCCESS && st == SAI_STATUS_SUCCESS) st = r;
    }
    if (st != SAI_STATUS_SUCCESS) s.cfg = old;
    return st;
}

sai_status_t MirrorArea::destroy(int id)
{
    Guard g(c_.mu);
    auto it = sessions_.find(id);
    if (it == sessions_.end()) return SAI_STATUS_ITEM_NOT_FOUND;
    if (it->second.dir[0].refs || it->second.dir[1].refs) return SAI_STATUS_OBJECT_IN_USE;
    dcdp_put(it->second.cfg.monitor);
    sessions_.erase(it);
    return SAI_STATUS_SUCCESS;
}

sai_status_t MirrorArea::policy_set(int hw, uint32_t attr, bool enable, ifcs_handle_t set, int container, uint32_t rate)
{
    ifcs_traffic_monitor_policy_t p;
    std::memset(&p, 0, sizeof(p));
    c_.L.ifcs_traffic_monitor_policy_t_init(&p);
    p.traffic_monitor_action = enable ? IFCS_TRAFFIC_MONITOR_ACTION_ENABLE : IFCS_TRAFFIC_MONITOR_ACTION_DISABLE;
    p.collector_set = set;
    p.traffic_monitor_container = container;
    p.sampling_rate = rate;
    ifcs_attr_t a = attr_of(attr);
    std::memcpy(&a.value, &p, sizeof(p));
    return c_.sp_set(hw, a, "sysport traffic monitor");
}

sai_status_t MirrorArea::port_set(int hw, bool ingress, const std::vector<int>& ids)
{
    Guard g(c_.mu);
    if (!c_.front(hw)) return SAI_STATUS_INVALID_PORT_NUMBER;
    if (ids.size() > 1) {
        c_.log_once("mirror:multi", "port mirroring: one session per port and direction");
        return SAI_STATUS_NOT_SUPPORTED;
    }
    for (int x : ids)
        if (!sessions_.count(x)) return SAI_STATUS_ITEM_NOT_FOUND;
    auto key = std::make_pair(hw, ingress);
    int old = port_session_.count(key) ? port_session_[key] : -1;
    int now = ids.empty() ? -1 : ids[0];
    if (old == now) return SAI_STATUS_SUCCESS;
    uint32_t attr = ingress ? IFCS_SYSPORT_ATTR_MIRROR_INGRESS : IFCS_SYSPORT_ATTR_MIRROR_EGRESS;
    sai_status_t st;
    if (now >= 0) {
        Session& s = sessions_[now];
        if ((st = dir_get(s, ingress)) != SAI_STATUS_SUCCESS) return st;
        st = policy_set(hw, attr, true, s.dir[dir_ix(ingress)].set, IFCS_TRAFFIC_MONITOR_CONTAINER_0, 1);
        if (st != SAI_STATUS_SUCCESS) { dir_put(s, ingress); return st; }
        port_session_[key] = now;
    } else {
        Session& s = sessions_[old];
        st = policy_set(hw, attr, false, s.dir[dir_ix(ingress)].set, IFCS_TRAFFIC_MONITOR_CONTAINER_0, 1);
        if (st != SAI_STATUS_SUCCESS) return st;
        port_session_.erase(key);
    }
    if (old >= 0) dir_put(sessions_[old], ingress);
    return SAI_STATUS_SUCCESS;
}

sai_status_t MirrorArea::collector_set(int id, bool ingress, ifcs_handle_t& set)
{
    Guard g(c_.mu);
    auto it = sessions_.find(id);
    if (it == sessions_.end()) return SAI_STATUS_ITEM_NOT_FOUND;
    sai_status_t st = dir_get(it->second, ingress);
    if (st == SAI_STATUS_SUCCESS) set = it->second.dir[dir_ix(ingress)].set;
    return st;
}

void MirrorArea::collector_set_release(int id, bool ingress)
{
    Guard g(c_.mu);
    auto it = sessions_.find(id);
    if (it != sessions_.end()) dir_put(it->second, ingress);
}

// ------------------------------------------------------------- sampling
// isai's sFlow collector toward the CPU: these MACs / DSCP / TTL / session
// id are the values it uses (tags in the sample's header).
sai_status_t MirrorArea::sflow_get(bool ingress, ifcs_handle_t& set)
{
    Dir& d = sflow_.dir[dir_ix(ingress)];
    if (d.refs++ > 0) { set = d.set; return SAI_STATUS_SUCCESS; }
    sai_status_t st = SAI_STATUS_SUCCESS;
    if (!sflow_.dcdp && (st = dcdp_get(-1, sflow_.dcdp)) != SAI_STATUS_SUCCESS) { d.refs = 0; return st; }
    static const uint8_t smac[6] = {0x00, 0x02, 0x04, 0x06, 0x08, 0x0a}, dmac[6] = {0x00, 0x01, 0x03, 0x05, 0x07, 0x09};
    IpAddr zero;
    std::vector<ifcs_attr_t> a = {attr_u32(IFCS_COLLECTOR_ATTR_COLLECTOR_TYPE, IFCS_COLLECTOR_TYPE_SFLOW),
                                  attr_u32(IFCS_COLLECTOR_ATTR_DIRECTION, dir_val(ingress)),
                                  attr_handle(IFCS_COLLECTOR_ATTR_DCDP, sflow_.dcdp),
                                  attr_mac(IFCS_COLLECTOR_ATTR_SRC_MAC, smac),
                                  ip_attr(IFCS_COLLECTOR_ATTR_SRC_IP, zero),
                                  attr_mac(IFCS_COLLECTOR_ATTR_DEST_MAC, dmac),
                                  ip_attr(IFCS_COLLECTOR_ATTR_DEST_IP, zero)};
    ifcs_attr_t x = attr_of(IFCS_COLLECTOR_ATTR_DSCP);
    x.value.u8 = 2;
    a.push_back(x);
    x = attr_of(IFCS_COLLECTOR_ATTR_TTL);
    x.value.u8 = 100;
    a.push_back(x);
    a.push_back(attr_u16(IFCS_COLLECTOR_ATTR_SESSION_ID, 111));
    st = c_.call(c_.L.ifcs_collector_create(NODE, &d.collector, (uint32_t)a.size(), a.data()), "sflow collector create");
    if (st == SAI_STATUS_SUCCESS) {
        ifcs_attr_t s2[2] = {attr_u32(IFCS_COLLECTOR_SET_ATTR_COLLECTOR_SET_TYPE, IFCS_COLLECTOR_TYPE_SFLOW),
                             attr_u32(IFCS_COLLECTOR_SET_ATTR_DIRECTION, dir_val(ingress))};
        st = c_.call(c_.L.ifcs_collector_set_create(NODE, &d.set, 2, s2), "sflow collector set create");
        if (st == SAI_STATUS_SUCCESS)
            st = c_.call(c_.L.ifcs_collector_set_member_add(NODE, d.set, 1, &d.collector, 0, nullptr), "sflow collector set add");
    }
    if (st != SAI_STATUS_SUCCESS) {
        if (d.set) c_.L.ifcs_collector_set_delete(NODE, d.set);
        if (d.collector) c_.L.ifcs_collector_delete(NODE, d.collector);
        d = Dir();
        return st;
    }
    set = d.set;
    return SAI_STATUS_SUCCESS;
}

void MirrorArea::sflow_put(bool ingress)
{
    Dir& d = sflow_.dir[dir_ix(ingress)];
    if (d.refs == 0 || --d.refs > 0) return;
    c_.call(c_.L.ifcs_collector_set_member_remove(NODE, d.set, 1, &d.collector), "sflow collector set remove");
    c_.call(c_.L.ifcs_collector_set_delete(NODE, d.set), "sflow collector set delete");
    c_.call(c_.L.ifcs_collector_delete(NODE, d.collector), "sflow collector delete");
    d = Dir();
    if (!sflow_.dir[0].refs && !sflow_.dir[1].refs && sflow_.dcdp) {
        dcdp_put(-1);
        sflow_.dcdp = IFCS_NULL_HANDLE;
    }
}

sai_status_t MirrorArea::sample_create(uint32_t rate, int& id)
{
    Guard g(c_.mu);
    if (!rate) return SAI_STATUS_INVALID_PARAMETER;
    id = next_++;
    samples_[id] = rate;
    return SAI_STATUS_SUCCESS;
}

sai_status_t MirrorArea::sample_update(int id, uint32_t rate)
{
    Guard g(c_.mu);
    auto it = samples_.find(id);
    if (it == samples_.end()) return SAI_STATUS_ITEM_NOT_FOUND;
    uint32_t old = it->second;
    it->second = rate;
    for (auto& p : port_sample_) {
        if (p.second != id) continue;
        Dir& d = sflow_.dir[dir_ix(p.first.second)];
        sai_status_t st = policy_set(p.first.first, p.first.second ? IFCS_SYSPORT_ATTR_SFLOW_INGRESS : IFCS_SYSPORT_ATTR_SFLOW_EGRESS,
                                     true, d.set, IFCS_TRAFFIC_MONITOR_CONTAINER_1, rate);
        if (st != SAI_STATUS_SUCCESS) { it->second = old; return st; }
        if (c_.sflow) c_.sflow->port_rate(p.first.first, p.first.second, rate);   // the samples' metadata
    }
    return SAI_STATUS_SUCCESS;
}

sai_status_t MirrorArea::sample_destroy(int id)
{
    Guard g(c_.mu);
    for (auto& p : port_sample_)
        if (p.second == id) return SAI_STATUS_OBJECT_IN_USE;
    return samples_.erase(id) ? SAI_STATUS_SUCCESS : SAI_STATUS_ITEM_NOT_FOUND;
}

sai_status_t MirrorArea::port_sample_set(int hw, bool ingress, int id)
{
    Guard g(c_.mu);
    if (!c_.front(hw)) return SAI_STATUS_INVALID_PORT_NUMBER;
    if (id >= 0 && !samples_.count(id)) return SAI_STATUS_ITEM_NOT_FOUND;
    if (!ingress && id >= 0 && !(c_.sflow && c_.sflow->egress_enabled())) {
        c_.log_once("sflow:egress", "egress sampling: not supported (node EGRESS_SFLOW_ENABLE is off in this init config)");
        return SAI_STATUS_NOT_SUPPORTED;
    }
    auto key = std::make_pair(hw, ingress);
    bool had = port_sample_.count(key) != 0;
    uint32_t attr = ingress ? IFCS_SYSPORT_ATTR_SFLOW_INGRESS : IFCS_SYSPORT_ATTR_SFLOW_EGRESS;
    sai_status_t st;
    if (id >= 0) {
        ifcs_handle_t set = IFCS_NULL_HANDLE;
        if (!had && (st = sflow_get(ingress, set)) != SAI_STATUS_SUCCESS) return st;
        set = sflow_.dir[dir_ix(ingress)].set;
        st = policy_set(hw, attr, true, set, IFCS_TRAFFIC_MONITOR_CONTAINER_1, samples_[id]);
        if (st != SAI_STATUS_SUCCESS) {
            if (!had) sflow_put(ingress);
            return st;
        }
        port_sample_[key] = id;
        if (c_.sflow) c_.sflow->port_rate(hw, ingress, samples_[id]);   // the samples' metadata
        return SAI_STATUS_SUCCESS;
    }
    if (!had) return SAI_STATUS_SUCCESS;
    st = policy_set(hw, attr, false, sflow_.dir[dir_ix(ingress)].set, IFCS_TRAFFIC_MONITOR_CONTAINER_1, 0);
    if (st != SAI_STATUS_SUCCESS) return st;
    port_sample_.erase(key);
    sflow_put(ingress);
    if (c_.sflow) c_.sflow->port_rate(hw, ingress, 0);
    return SAI_STATUS_SUCCESS;
}

std::string MirrorArea::show()
{
    Guard g(c_.mu);
    std::string s;
    char b[200];
    std::snprintf(b, sizeof(b), "mirror: %zu sessions, %zu port bindings, %zu samplers on %zu ports\n", sessions_.size(),
                  port_session_.size(), samples_.size(), port_sample_.size());
    s += b;
    for (auto& x : sessions_) {
        std::snprintf(b, sizeof(b), "  session %d: %s monitor devport %d dcdp 0x%x in set 0x%x (%d) eg set 0x%x (%d)\n", x.first,
                      x.second.cfg.erspan ? "ERSPAN" : "SPAN", x.second.cfg.monitor, x.second.dcdp, x.second.dir[0].set,
                      x.second.dir[0].refs, x.second.dir[1].set, x.second.dir[1].refs);
        s += b;
    }
    for (auto& p : port_sample_) {
        std::snprintf(b, sizeof(b), "  devport %d %s: sample %d rate %u\n", p.first.first, p.first.second ? "in" : "eg",
                      p.second, samples_[p.second]);
        s += b;
    }
    if (c_.sflow) s += c_.sflow->show();
    return s;
}

} // namespace tlb
} // namespace ob
