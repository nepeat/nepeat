"""SAI smoke test for sai-openbcm temperature and LAG (run against syncd).

Same setup as sai_smoke.py (redis, syncd -SUu -z redis_sync -p <profile>).
Checks: TEMP_LIST / MAX_TEMP / AVERAGE_TEMP, a LAG with two members,
LAG PORT_LIST, member EGRESS_DISABLE, then removes everything.
"""
import sys, pysairedis as P

ok = True
def check(what, st, cond=True):
    global ok
    good = st == P.SAI_STATUS_SUCCESS and cond
    ok &= good
    print(("PASS " if good else "FAIL ") + what, st)

P.sai_api_initialize(0, {})
sw = P.sai_switch_api_t(); P.sai_get_switch_api(sw)
a = P.sai_attribute_t()
a.id = P.SAI_REDIS_SWITCH_ATTR_SYNC_OPERATION_RESPONSE_TIMEOUT; a.value.u64 = 20000; sw.set_switch_attribute(0, a)
a.id = P.SAI_REDIS_SWITCH_ATTR_REDIS_COMMUNICATION_MODE; a.value.s32 = P.SAI_REDIS_COMMUNICATION_MODE_REDIS_SYNC; sw.set_switch_attribute(0, a)
a.id = P.SAI_REDIS_SWITCH_ATTR_NOTIFY_SYNCD; a.value.s32 = P.SAI_REDIS_NOTIFY_SYNCD_INIT_VIEW; sw.set_switch_attribute(0, a)
poid = P.new_sai_object_id_t_p(); attrs = P.new_sai_attribute_t_arr(1)
a.id = P.SAI_SWITCH_ATTR_INIT_SWITCH; a.value.booldata = True; P.sai_attribute_t_arr_setitem(attrs, 0, a)
check("create_switch", sw.create_switch(poid, 1, attrs))
swid = P.sai_object_id_t_p_value(poid)
a.id = P.SAI_REDIS_SWITCH_ATTR_NOTIFY_SYNCD; a.value.s32 = P.SAI_REDIS_NOTIFY_SYNCD_APPLY_VIEW; sw.set_switch_attribute(swid, a)

t = P.sai_attribute_t(); t.id = P.SAI_SWITCH_ATTR_MAX_NUMBER_OF_TEMP_SENSORS
check("max temp sensors", sw.get_switch_attribute(swid, 1, t)); n = t.value.u8; print("  sensors", n)
t.id = P.SAI_SWITCH_ATTR_TEMP_LIST; t.value.s32list.count = 16
try:   # the SWIG binding has no int32 array helper; same width as uint32
    t.value.s32list.list = P.new_uint32_t_arr(16)
    st = sw.get_switch_attribute(swid, 1, t)
    temps = [P.uint32_t_arr_getitem(t.value.s32list.list, i) for i in range(t.value.s32list.count)]
    check("temp list", st, len(temps) >= 1 and all(0 < x < 130 for x in temps)); print("  temps C", temps)
except TypeError:
    t.value.s32list.count = 0
    st = sw.get_switch_attribute(swid, 1, t)
    check("temp list (count probe)", P.SAI_STATUS_SUCCESS if st == P.SAI_STATUS_BUFFER_OVERFLOW else st, t.value.s32list.count == n)
    print("  needs", t.value.s32list.count)
t2 = P.sai_attribute_t(); t2.id = P.SAI_SWITCH_ATTR_MAX_TEMP
check("max temp", sw.get_switch_attribute(swid, 1, t2), 0 < t2.value.s32 < 130); print("  max", t2.value.s32)
t2.id = P.SAI_SWITCH_ATTR_AVERAGE_TEMP
check("average temp", sw.get_switch_attribute(swid, 1, t2), 0 < t2.value.s32 < 130); print("  avg", t2.value.s32)

a.id = P.SAI_SWITCH_ATTR_PORT_LIST; a.value.objlist.count = 256; a.value.objlist.list = P.new_sai_object_id_t_arr(256)
sw.get_switch_attribute(swid, 1, a)
ports = [P.sai_object_id_t_arr_getitem(a.value.objlist.list, i) for i in range(a.value.objlist.count)]
# As orchagent does before a port joins a LAG: drop its default VLAN member
# and its bridge port on the default .1Q bridge.
ba = P.sai_bridge_api_t(); P.sai_get_bridge_api(ba)
va = P.sai_vlan_api_t(); P.sai_get_vlan_api(va)
def objlist(get, oid, attr_id, n=512):
    x = P.sai_attribute_t(); x.id = attr_id; x.value.objlist.count = n; x.value.objlist.list = P.new_sai_object_id_t_arr(n)
    st = get(oid, 1, x)
    return st, [P.sai_object_id_t_arr_getitem(x.value.objlist.list, i) for i in range(x.value.objlist.count)]
def oid_attr(get, oid, attr_id):
    x = P.sai_attribute_t(); x.id = attr_id; st = get(oid, 1, x); return x.value.oid
q = P.sai_attribute_t(); q.id = P.SAI_SWITCH_ATTR_DEFAULT_1Q_BRIDGE_ID; sw.get_switch_attribute(swid, 1, q); br = q.value.oid
q.id = P.SAI_SWITCH_ATTR_DEFAULT_VLAN_ID; sw.get_switch_attribute(swid, 1, q); vlan1 = q.value.oid
_, bps = objlist(ba.get_bridge_attribute, br, P.SAI_BRIDGE_ATTR_PORT_LIST)
_, vms = objlist(va.get_vlan_attribute, vlan1, P.SAI_VLAN_ATTR_MEMBER_LIST)
for port in ports[2:4]:
    for bp in bps:
        if oid_attr(ba.get_bridge_port_attribute, bp, P.SAI_BRIDGE_PORT_ATTR_PORT_ID) != port:
            continue
        for vm in vms:
            if oid_attr(va.get_vlan_member_attribute, vm, P.SAI_VLAN_MEMBER_ATTR_BRIDGE_PORT_ID) == bp:
                check("remove vlan1 member", va.remove_vlan_member(vm))
        check("remove bridge port", ba.remove_bridge_port(bp))
la = P.sai_lag_api_t(); P.sai_get_lag_api(la)
lp_ = P.new_sai_object_id_t_p()
check("create lag", la.create_lag(lp_, swid, 0, P.new_sai_attribute_t_arr(1)))
lag = P.sai_object_id_t_p_value(lp_); print("  lag", hex(lag))
members = []
for port in ports[2:4]:
    mp = P.new_sai_object_id_t_p(); at = P.new_sai_attribute_t_arr(2)
    a.id = P.SAI_LAG_MEMBER_ATTR_LAG_ID; a.value.oid = lag; P.sai_attribute_t_arr_setitem(at, 0, a)
    a.id = P.SAI_LAG_MEMBER_ATTR_PORT_ID; a.value.oid = port; P.sai_attribute_t_arr_setitem(at, 1, a)
    check("create lag member " + hex(port), la.create_lag_member(mp, swid, 2, at))
    members.append(P.sai_object_id_t_p_value(mp))
g = P.sai_attribute_t(); g.id = P.SAI_LAG_ATTR_PORT_LIST; g.value.objlist.count = 32; g.value.objlist.list = P.new_sai_object_id_t_arr(32)
st = la.get_lag_attribute(lag, 1, g)
got = sorted(P.sai_object_id_t_arr_getitem(g.value.objlist.list, i) for i in range(g.value.objlist.count))
check("lag port list", st, got == sorted(members)); print("  members", [hex(x) for x in got])
e = P.sai_attribute_t(); e.id = P.SAI_LAG_MEMBER_ATTR_EGRESS_DISABLE; e.value.booldata = True
check("member egress disable", la.set_lag_member_attribute(members[0], e))
e2 = P.sai_attribute_t(); e2.id = P.SAI_LAG_MEMBER_ATTR_EGRESS_DISABLE
check("member egress disable readback", la.get_lag_member_attribute(members[0], 1, e2), e2.value.booldata is True)
for m in members:
    check("remove lag member", la.remove_lag_member(m))
check("remove lag", la.remove_lag(lag))
P.sai_api_uninitialize()
print("SMOKE_OK" if ok else "SMOKE_FAIL")
sys.exit(0 if ok else 1)
