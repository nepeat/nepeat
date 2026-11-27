//! t_switch.cpp: init call sequence and commands, temperatures, switch removal.
use crate::support::*;

pub fn test_switch_init(e: &mut Env) {
    e.ck.section("switch_init");
    let (run, init, isg) = (e.run.clone(), e.init.clone(), e.isg.clone());
    check!(
        e,
        "t_switch.cpp:9",
        get(&format!("{run}/config.yaml")) == format!("{}{}", get(&init), get(&isg))
    );
    let log = e.m().log();
    let l = lines(&log);
    // The vendor innovium.user sequence, then our log hook in between.
    let want: Vec<String> = vec![
        format!("ifcs_init cfg={run}/config.yaml log=sai-teralynx boot=0 first=# generated init config"),
        "ifcs_log_function_register".to_owned(),
        "ifcs_node_create node=0 count=0 attrs=null".to_owned(),
        "ifcs_node_attr_set node=0 count=1 id=82 u32=1".to_owned(), // multicast LAG resolution: only before ENABLE
        "ifcs_node_attr_set node=0 count=1 id=46 u32=0".to_owned(), // L3 interface counters: flex block, then mode (ingress)
        "ifcs_node_attr_set node=0 count=1 id=45 u32=4".to_owned(),
        "ifcs_node_attr_set node=0 count=1 id=48 u32=0".to_owned(), // (egress)
        "ifcs_node_attr_set node=0 count=1 id=47 u32=4".to_owned(),
        "ifcs_attr_t_init".to_owned(),
        "ifcs_attr_t_id_set id=0".to_owned(),
        "ifcs_attr_t_value_data_set 1".to_owned(),
        "ifcs_node_attr_set node=0 count=1 id=0 u32=1".to_owned(),
    ];
    check!(e, "t_switch.cpp:27", l.len() > want.len());
    for i in 0..want.len().min(l.len()) {
        if l[i] != want[i] {
            eprintln!("init call {i}: got '{}' want '{}'", l[i], want[i]);
        }
        check!(e, "t_switch.cpp:30", l[i] == want[i]);
    }
    // ports down before any init command, then the commands in file order
    let down3 = log.find("ifcs_devport_attr_set dp=3 id=23 u32=0");
    let c1 = log.find("ifcs_devport_attr_set dp=1 id=49 u32=1");
    check!(e, "t_switch.cpp:35", matches!((down3, c1), (Some(a), Some(b)) if a < b));
    let cmds: Vec<&String> = l
        .iter()
        .filter(|x| x.starts_with("ifcs_devport_attr_set") && !x.contains("id=23 "))
        .collect();
    let want_cmds = [
        "ifcs_devport_attr_set dp=1 id=49 u32=1",
        "ifcs_devport_attr_set dp=1 id=43 i8=[2,2,2,2,2,2,2,2]",
        "ifcs_devport_attr_set dp=2 id=50 u8=[1,2,3,4]",
        "ifcs_devport_attr_set dp=3 id=53 u32l=[5]",
        "ifcs_devport_attr_set dp=1 id=21 u32=1",
    ];
    check_eq!(e, "t_switch.cpp:46", cmds.len(), want_cmds.len());
    for i in 0..want_cmds.len().min(cmds.len()) {
        if cmds[i] != want_cmds[i] {
            eprintln!("cmd {i}: got '{}' want '{}'", cmds[i], want_cmds[i]);
        }
        check!(e, "t_switch.cpp:49", cmds[i] == want_cmds[i]);
    }
    check!(e, "t_switch.cpp:51", !has(&log, "id=23 u32=1")); // "port enable" in a vendor file never enables a port
    check!(
        e,
        "t_switch.cpp:52",
        has(&log, "ifcs_linkscan_event_register_notify node=0")
    );
    check!(e, "t_switch.cpp:53", has(&log, "ifcs_l2_event_register_notify node=0"));
    check!(
        e,
        "t_switch.cpp:54",
        has(&log, "ifcs_stp_port_add stp=0x100 members=4 attrs=1 state=3")
    );
    check!(
        e,
        "t_switch.cpp:55",
        has(&log, "ifcs_l2vni_create vni=1 count=4 stp=0x100")
    );
    // the CPU is in no VLAN (traps bring what it needs)
    check!(
        e,
        "t_switch.cpp:56",
        !has(&log, "ifcs_l2vni_member_add vni=1 n=1 first=0x3e2003e8")
    );
}

pub fn test_switch_misc(e: &mut Env) {
    e.ck.section("switch_misc");
    // ---- temperature: master 55.5, remotes 54.0 53.5 (-300 left out) 52.0.
    // One monitor per IFCS read (each is ~400 ms on the chip), round-robin,
    // at most one read per SAI_TERALYNX_TEMP_PERIOD (15 s) once all were read.
    let temp_list = || Value::S32List(vec![0; 8]);
    let mut a = Attr::default();
    for _ in 0..5 {
        let (st, got) = e.get1(
            sys::SAI_OBJECT_TYPE_SWITCH,
            e.s,
            sys::SAI_SWITCH_ATTR_TEMP_LIST,
            temp_list(),
        );
        a = got;
        ok!(e, "t_switch.cpp:74", st);
    }
    check_eq!(e, "t_switch.cpp:76", e.m().temp_reads(), 5);
    check_eq!(e, "t_switch.cpp:77", a.count(), 4u32);
    // within the period: cached
    let (st, a) = e.get1(
        sys::SAI_OBJECT_TYPE_SWITCH,
        e.s,
        sys::SAI_SWITCH_ATTR_TEMP_LIST,
        temp_list(),
    );
    ok!(e, "t_switch.cpp:79", st);
    check_eq!(e, "t_switch.cpp:80", e.m().temp_reads(), 5);
    check_eq!(e, "t_switch.cpp:81", a.count(), 4u32);
    let (st, a) = e.get1(
        sys::SAI_OBJECT_TYPE_SWITCH,
        e.s,
        sys::SAI_SWITCH_ATTR_MAX_TEMP,
        Value::S32(0),
    );
    ok!(e, "t_switch.cpp:83", st);
    check!(e, "t_switch.cpp:84", a.s32() == 55 || a.s32() == 56);
    let (st, a) = e.get1(
        sys::SAI_OBJECT_TYPE_SWITCH,
        e.s,
        sys::SAI_SWITCH_ATTR_AVAILABLE_IPV4_ROUTE_ENTRY,
        Value::U32(0),
    );
    ok!(e, "t_switch.cpp:86", st);
    check!(e, "t_switch.cpp:87", a.u32() > 0 && a.u32() < 24576);
}

pub fn test_switch_remove(e: &mut Env) {
    e.ck.section("switch_remove");
    // ---- remove: ports down, callbacks gone, deinit
    e.m().log_clear();
    let st = e.sai.remove_switch(e.s);
    ok!(e, "t_switch.cpp:96", st);
    let log = e.m().log();
    let d1 = log.find("ifcs_devport_attr_set dp=1 id=23 u32=0");
    let un = log.find("ifcs_linkscan_event_unregister_notify");
    let de = log.find("ifcs_deinit type=0");
    check!(
        e,
        "t_switch.cpp:101",
        matches!((d1, un, de), (Some(d), Some(u), Some(x)) if d < u && u < x)
    );
    check!(e, "t_switch.cpp:102", has(&log, "ifcs_netdev_delete"));
    check_eq!(e, "t_switch.cpp:103", e.m().inited(), 0);
}
