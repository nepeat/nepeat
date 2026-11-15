import sys, pysairedis as P
pm = {}
st = P.sai_api_initialize(0, pm); print("initialize", st)
sw = P.sai_switch_api_t(); P.sai_get_switch_api(sw)
a = P.sai_attribute_t()
a.id = P.SAI_REDIS_SWITCH_ATTR_SYNC_OPERATION_RESPONSE_TIMEOUT; a.value.u64 = 10000; sw.set_switch_attribute(0, a)
a.id = P.SAI_REDIS_SWITCH_ATTR_REDIS_COMMUNICATION_MODE; a.value.s32 = P.SAI_REDIS_COMMUNICATION_MODE_REDIS_SYNC
print("comm mode", sw.set_switch_attribute(0, a))
a.id = P.SAI_REDIS_SWITCH_ATTR_NOTIFY_SYNCD; a.value.s32 = P.SAI_REDIS_NOTIFY_SYNCD_INIT_VIEW
print("init view", sw.set_switch_attribute(0, a))
poid = P.new_sai_object_id_t_p(); attrs = P.new_sai_attribute_t_arr(1)
a.id = P.SAI_SWITCH_ATTR_INIT_SWITCH; a.value.booldata = True; P.sai_attribute_t_arr_setitem(attrs, 0, a)
print("create_switch", sw.create_switch(poid, 1, attrs))
swid = P.sai_object_id_t_p_value(poid); print("swid", hex(swid))
a.id = P.SAI_REDIS_SWITCH_ATTR_NOTIFY_SYNCD; a.value.s32 = P.SAI_REDIS_NOTIFY_SYNCD_APPLY_VIEW
print("apply view", sw.set_switch_attribute(swid, a))
a.id = P.SAI_SWITCH_ATTR_PORT_LIST; a.value.objlist.count = 256; a.value.objlist.list = P.new_sai_object_id_t_arr(256)
print("get port list", sw.get_switch_attribute(swid, 1, a), "ports", a.value.objlist.count)
ports = [P.sai_object_id_t_arr_getitem(a.value.objlist.list, n) for n in range(a.value.objlist.count)]
pa = P.sai_port_api_t(); P.sai_get_port_api(pa)
q = P.sai_attribute_t(); q.id = P.SAI_PORT_ATTR_HW_LANE_LIST; q.value.u32list.count = 8; q.value.u32list.list = P.new_uint32_t_arr(8)
print("port0 lanes", pa.get_port_attribute(ports[0], 1, q), [P.uint32_t_arr_getitem(q.value.u32list.list, i) for i in range(q.value.u32list.count)])
q2 = P.sai_attribute_t(); q2.id = P.SAI_PORT_ATTR_SPEED
print("port0 speed", pa.get_port_attribute(ports[0], 1, q2), q2.value.u32)
a.id = P.SAI_SWITCH_ATTR_DEFAULT_VIRTUAL_ROUTER_ID
print("default vr", sw.get_switch_attribute(swid, 1, a), hex(a.value.oid)); vrid = a.value.oid
va = P.sai_vlan_api_t(); P.sai_get_vlan_api(va)
p2 = P.new_sai_object_id_t_p(); at2 = P.new_sai_attribute_t_arr(1)
a.id = P.SAI_VLAN_ATTR_VLAN_ID; a.value.u16 = 100; P.sai_attribute_t_arr_setitem(at2, 0, a)
print("create vlan 100", va.create_vlan(p2, swid, 1, at2), hex(P.sai_object_id_t_p_value(p2)))
ra = P.sai_router_interface_api_t(); P.sai_get_router_interface_api(ra)
p3 = P.new_sai_object_id_t_p(); at3 = P.new_sai_attribute_t_arr(3)
for i, (k, v) in enumerate([(P.SAI_ROUTER_INTERFACE_ATTR_VIRTUAL_ROUTER_ID, ("oid", vrid)), (P.SAI_ROUTER_INTERFACE_ATTR_TYPE, ("s32", P.SAI_ROUTER_INTERFACE_TYPE_PORT)), (P.SAI_ROUTER_INTERFACE_ATTR_PORT_ID, ("oid", ports[0]))]):
    a.id = k; setattr(a.value, v[0], v[1]); P.sai_attribute_t_arr_setitem(at3, i, a)
print("create rif", ra.create_router_interface(p3, swid, 3, at3))
print("uninitialize", P.sai_api_uninitialize())
