# The slice of the Rust SAI through libsairedis -> redis -> syncd -> libsai (what
# test-syncd.nix runs). Modelled on ./sai_smoke.py (the C++ sai-openbcm's), cut
# to the objects the slice has: switch, ports, VLAN, VLAN member, port state notification.
# Prints one "key value" line per step; exits non-zero on the first failure.
import ctypes, os, sys, time
import pysairedis as P

def ok(name, st, *rest):
    print(name, st, *rest, flush=True)
    if st != P.SAI_STATUS_SUCCESS:
        print("FAILED at", name, file=sys.stderr)
        sys.exit(1)

pm = {}
ok("initialize", P.sai_api_initialize(0, pm))
sw = P.sai_switch_api_t(); P.sai_get_switch_api(sw)
a = P.sai_attribute_t()
a.id = P.SAI_REDIS_SWITCH_ATTR_SYNC_OPERATION_RESPONSE_TIMEOUT; a.value.u64 = 60000; sw.set_switch_attribute(0, a)
a.id = P.SAI_REDIS_SWITCH_ATTR_REDIS_COMMUNICATION_MODE; a.value.s32 = P.SAI_REDIS_COMMUNICATION_MODE_REDIS_SYNC
ok("comm_mode", sw.set_switch_attribute(0, a))
a.id = P.SAI_REDIS_SWITCH_ATTR_NOTIFY_SYNCD; a.value.s32 = P.SAI_REDIS_NOTIFY_SYNCD_INIT_VIEW
ok("init_view", sw.set_switch_attribute(0, a))

# Port state notifications through a ctypes callback (pysairedis' own trampoline does not
# take the GIL): see sai_smoke.py.
class PortOperStatus(ctypes.Structure):
    _fields_ = [("port_id", ctypes.c_uint64), ("port_state", ctypes.c_int32), ("port_error_status", ctypes.c_int32)]
PORT_STATE_FN = ctypes.CFUNCTYPE(None, ctypes.c_uint32, ctypes.POINTER(PortOperStatus))
port_events = []
def on_port_state(count, data):
    for n in range(count):
        port_events.append((data[n].port_id, data[n].port_state))
port_state_cb = PORT_STATE_FN(on_port_state)

poid = P.new_sai_object_id_t_p(); attrs = P.new_sai_attribute_t_arr(2)
a.id = P.SAI_SWITCH_ATTR_INIT_SWITCH; a.value.booldata = True; P.sai_attribute_t_arr_setitem(attrs, 0, a)
n = P.sai_attribute_t()
n.id = P.SAI_SWITCH_ATTR_PORT_STATE_CHANGE_NOTIFY
ctypes.c_void_p.from_address(int(n.this) + 8).value = ctypes.cast(port_state_cb, ctypes.c_void_p).value
P.sai_attribute_t_arr_setitem(attrs, 1, n)
ok("create_switch", sw.create_switch(poid, 2, attrs))
swid = P.sai_object_id_t_p_value(poid); print("swid", hex(swid))
a.id = P.SAI_REDIS_SWITCH_ATTR_NOTIFY_SYNCD; a.value.s32 = P.SAI_REDIS_NOTIFY_SYNCD_APPLY_VIEW
ok("apply_view", sw.set_switch_attribute(swid, a))

a.id = P.SAI_SWITCH_ATTR_PORT_LIST; a.value.objlist.count = 256; a.value.objlist.list = P.new_sai_object_id_t_arr(256)
ok("port_list", sw.get_switch_attribute(swid, 1, a), "ports", a.value.objlist.count)
assert a.value.objlist.count == 32, a.value.objlist.count
ports = [P.sai_object_id_t_arr_getitem(a.value.objlist.list, n) for n in range(a.value.objlist.count)]

pa = P.sai_port_api_t(); P.sai_get_port_api(pa)
q = P.sai_attribute_t(); q.id = P.SAI_PORT_ATTR_HW_LANE_LIST; q.value.u32list.count = 8; q.value.u32list.list = P.new_uint32_t_arr(8)
st = pa.get_port_attribute(ports[0], 1, q)
lanes = [P.uint32_t_arr_getitem(q.value.u32list.list, i) for i in range(q.value.u32list.count)]
ok("port0_lanes", st, lanes)
assert lanes == [1, 2, 3, 4], lanes
q2 = P.sai_attribute_t(); q2.id = P.SAI_PORT_ATTR_SPEED
ok("port0_speed", pa.get_port_attribute(ports[0], 1, q2), q2.value.u32)
assert q2.value.u32 == 100000, q2.value.u32
q3 = P.sai_attribute_t(); q3.id = P.SAI_PORT_ATTR_ADMIN_STATE
ok("port0_admin", pa.get_port_attribute(ports[0], 1, q3), q3.value.booldata)
assert q3.value.booldata is False   # ports start admin-down

va = P.sai_vlan_api_t(); P.sai_get_vlan_api(va)
p2 = P.new_sai_object_id_t_p(); at2 = P.new_sai_attribute_t_arr(1)
a.id = P.SAI_VLAN_ATTR_VLAN_ID; a.value.u16 = 100; P.sai_attribute_t_arr_setitem(at2, 0, a)
ok("create_vlan_100", va.create_vlan(p2, swid, 1, at2), hex(P.sai_object_id_t_p_value(p2)))

# Outside the slice: the first thing the full smoke test asks for next.
a.id = P.SAI_SWITCH_ATTR_DEFAULT_VIRTUAL_ROUTER_ID
print("default_vr_status", sw.get_switch_attribute(swid, 1, a), flush=True)

q3.value.booldata = True
ok("port0_admin_up", pa.set_port_attribute(ports[0], q3))
for _ in range(100):
    if (ports[0], P.SAI_PORT_OPER_STATUS_UP) in port_events:
        break
    time.sleep(0.1)
print("port_events", [(hex(p), s) for p, s in port_events], flush=True)
assert (ports[0], P.SAI_PORT_OPER_STATUS_UP) in port_events, port_events
print("port0_oper_up_notified", flush=True)

ok("uninitialize", P.sai_api_uninitialize())
print("SMOKE_OK", flush=True)
