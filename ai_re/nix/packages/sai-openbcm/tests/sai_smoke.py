# SAI smoke test through libsairedis -> redis -> syncd -> libsai, adapted
# from devices/sonic-nixos/sai_smoke.py (same calls, plus assertions).
# Expects syncd running with a profile that selects the fake backend
# (SAI_OPENBCM_BACKEND=fake), or the openbcm backend on the AS7760.
# Prints one "key value" line per step; exits non-zero on the first failure.
import ctypes, os, sys, time
import pysairedis as P

expect_ports = int(os.environ.get("EXPECT_PORTS", "32"))
expect_lanes = [int(x) for x in os.environ.get("EXPECT_LANES0", "1,2,3,4").split(",") if x]
expect_speed = os.environ.get("EXPECT_SPEED0", "100000")   # "" = do not check
admin_up_port0 = os.environ.get("ADMIN_UP_PORT0", "1") == "1"

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
# Port state notifications: pysairedis' own callback trampoline builds
# Python objects without taking the GIL and crashes when libsairedis calls
# it from its notification thread, so register a ctypes callback (which
# does take the GIL) and poke its address into the attribute's pointer.
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
# sai_attribute_t: 4-byte id, value union 8-byte aligned at offset 8.
ctypes.c_void_p.from_address(int(n.this) + 8).value = ctypes.cast(port_state_cb, ctypes.c_void_p).value
P.sai_attribute_t_arr_setitem(attrs, 1, n)
ok("create_switch", sw.create_switch(poid, 2, attrs))
swid = P.sai_object_id_t_p_value(poid); print("swid", hex(swid))
a.id = P.SAI_REDIS_SWITCH_ATTR_NOTIFY_SYNCD; a.value.s32 = P.SAI_REDIS_NOTIFY_SYNCD_APPLY_VIEW
ok("apply_view", sw.set_switch_attribute(swid, a))

a.id = P.SAI_SWITCH_ATTR_PORT_LIST; a.value.objlist.count = 256; a.value.objlist.list = P.new_sai_object_id_t_arr(256)
ok("port_list", sw.get_switch_attribute(swid, 1, a), "ports", a.value.objlist.count)
assert a.value.objlist.count == expect_ports, a.value.objlist.count
ports = [P.sai_object_id_t_arr_getitem(a.value.objlist.list, n) for n in range(a.value.objlist.count)]

pa = P.sai_port_api_t(); P.sai_get_port_api(pa)
q = P.sai_attribute_t(); q.id = P.SAI_PORT_ATTR_HW_LANE_LIST; q.value.u32list.count = 8; q.value.u32list.list = P.new_uint32_t_arr(8)
st = pa.get_port_attribute(ports[0], 1, q)
lanes = [P.uint32_t_arr_getitem(q.value.u32list.list, i) for i in range(q.value.u32list.count)]
ok("port0_lanes", st, lanes)
assert not expect_lanes or lanes == expect_lanes, lanes
q2 = P.sai_attribute_t(); q2.id = P.SAI_PORT_ATTR_SPEED
ok("port0_speed", pa.get_port_attribute(ports[0], 1, q2), q2.value.u32)
assert not expect_speed or q2.value.u32 == int(expect_speed), q2.value.u32
q3 = P.sai_attribute_t(); q3.id = P.SAI_PORT_ATTR_ADMIN_STATE
ok("port0_admin", pa.get_port_attribute(ports[0], 1, q3), q3.value.booldata)
assert q3.value.booldata is False   # safety rule: ports start admin-down

a.id = P.SAI_SWITCH_ATTR_DEFAULT_VIRTUAL_ROUTER_ID
ok("default_vr", sw.get_switch_attribute(swid, 1, a), hex(a.value.oid)); vrid = a.value.oid
assert vrid != 0

va = P.sai_vlan_api_t(); P.sai_get_vlan_api(va)
p2 = P.new_sai_object_id_t_p(); at2 = P.new_sai_attribute_t_arr(1)
a.id = P.SAI_VLAN_ATTR_VLAN_ID; a.value.u16 = 100; P.sai_attribute_t_arr_setitem(at2, 0, a)
ok("create_vlan_100", va.create_vlan(p2, swid, 1, at2), hex(P.sai_object_id_t_p_value(p2)))

ra = P.sai_router_interface_api_t(); P.sai_get_router_interface_api(ra)
p3 = P.new_sai_object_id_t_p(); at3 = P.new_sai_attribute_t_arr(3)
for i, (k, v) in enumerate([(P.SAI_ROUTER_INTERFACE_ATTR_VIRTUAL_ROUTER_ID, ("oid", vrid)), (P.SAI_ROUTER_INTERFACE_ATTR_TYPE, ("s32", P.SAI_ROUTER_INTERFACE_TYPE_PORT)), (P.SAI_ROUTER_INTERFACE_ATTR_PORT_ID, ("oid", ports[0]))]):
    a.id = k; setattr(a.value, v[0], v[1]); P.sai_attribute_t_arr_setitem(at3, i, a)
ok("create_rif", ra.create_router_interface(p3, swid, 3, at3))

if admin_up_port0:
    # Fake backend: link follows admin state, so this raises a port
    # oper-status notification through syncd (checked by the caller).
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
