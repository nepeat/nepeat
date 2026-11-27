//! t_hostif.cpp: host interface: netdevs.
use crate::support::*;

pub fn test_hostif(e: &mut Env) {
    e.ck.section("hostif");
    let h = [
        a_s32(sys::SAI_HOSTIF_ATTR_TYPE, sys::SAI_HOSTIF_TYPE_NETDEV as i32),
        a_oid(sys::SAI_HOSTIF_ATTR_OBJ_ID, e.p1),
        a_text(sys::SAI_HOSTIF_ATTR_NAME, "Ethernet0"),
    ];
    let (st, hif) = e.create(sys::SAI_OBJECT_TYPE_HOSTIF, &h);
    e.hif = hif;
    ok!(e, "t_hostif.cpp:19", st);
    check!(
        e,
        "t_hostif.cpp:20",
        has(&e.m().log(), "ifcs_netdev_create h=") && has(&e.m().log(), "name=Ethernet0 devport=1")
    );
    check!(e, "t_hostif.cpp:21", has(&e.m().log(), " tag=1")); // SAI default STRIP
    // system traps as isai sets them (once, before the first netdev): MTU and
    // L2 source miss off, TTL and IPv4 options on
    check!(
        e,
        "t_hostif.cpp:24",
        has(&e.m().log(), "ifcs_hostif_trap_attr_set h=0x18200017 id=1 u32=0")
    );
    check!(
        e,
        "t_hostif.cpp:25",
        has(&e.m().log(), "ifcs_hostif_trap_attr_set h=0x18200010 id=1 u32=0")
    );
    check!(
        e,
        "t_hostif.cpp:26",
        has(&e.m().log(), "ifcs_hostif_trap_attr_set h=0x18200011 id=1 u32=1")
    );
    check!(
        e,
        "t_hostif.cpp:27",
        has(&e.m().log(), "ifcs_hostif_trap_attr_set h=0x18200014 id=1 u32=1")
    );
    let st = e.set(
        sys::SAI_OBJECT_TYPE_HOSTIF,
        e.hif,
        a_bool(sys::SAI_HOSTIF_ATTR_OPER_STATUS, true),
    );
    ok!(e, "t_hostif.cpp:30", st);
}
