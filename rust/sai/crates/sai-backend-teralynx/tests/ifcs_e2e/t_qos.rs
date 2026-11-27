//! t_qos.cpp: QoS and buffers on the mock libifcs: DSCP / DOT1P -> TC maps (IFCS map + shared map
//! group + sysport attrs), map update, the TC -> queue identity rule, default TC, PFC, queue
//! schedulers through scheduler groups, the port shaper, WRED (AQM curve groups in cells, queue
//! flags), queue buffer profiles (reserve with the port flapped, thresholds), queue / PG / PFC
//! counters.
use crate::support::*;
use sai_client::{QosMapEntry, QosParams};

// IFCS ids the checks name (values: the generated ifcs_api_qos.h / ifcs_api.h)
const Q_WDRR_WEIGHT: u32 = 14;
const QS_TOTAL_USE_WATERMARK: i32 = 1;
const QS_PACKET_DROP: i32 = 3;
const QS_PACKET_TX: i32 = 5;
const QS_BYTE_TX: i32 = 6;
const BAM_PACKET_DROP: i32 = 2;
const DP_RX_FRAMES_PFC0: u32 = 21;

/// 'qmap': creates a QoS map of "type" with the list; with "check" the create is an OK(), else the
/// status is the caller's.
fn qmap(e: &mut Env, ty: i32, l: Vec<QosMapEntry>, check: bool) -> (Status, u64) {
    let (r, m) = e.create(
        sys::SAI_OBJECT_TYPE_QOS_MAP,
        &[
            a_s32(sys::SAI_QOS_MAP_ATTR_TYPE, ty),
            Attr::new(sys::SAI_QOS_MAP_ATTR_MAP_TO_VALUE_LIST, Value::QosMap(l)),
        ],
    );
    if check {
        ok!(e, "t_qos.cpp:41", r);
    }
    (r, m)
}

fn entries(n: i32, f: fn(i32) -> i32, dscp: bool, tcq: bool) -> Vec<QosMapEntry> {
    let mut v = Vec::new();
    for i in 0..n {
        let mut m = QosMapEntry::default();
        if tcq {
            m.key.tc = i as u8;
            m.value.queue_index = f(i) as u8;
        } else if dscp {
            m.key.dscp = i as u8;
            m.value.tc = f(i) as u8;
        } else {
            m.key.dot1p = i as u8;
            m.value.tc = f(i) as u8;
        }
        v.push(m);
    }
    v
}

fn objlist(e: &mut Env, p: u64, id: u32) -> Vec<u64> {
    let (st, a) = e.get1(sys::SAI_OBJECT_TYPE_PORT, p, id, Value::OidList(vec![0; 32]));
    ok!(e, "t_qos.cpp:64", st);
    a.oids()
}

pub fn test_qos(e: &mut Env) {
    e.ck.section("qos");
    for api in [
        Api::QosMap,
        Api::Scheduler,
        Api::SchedulerGroup,
        Api::Wred,
        Api::Buffer,
        Api::Queue,
    ] {
        let st = e.sai.query(api);
        match api {
            Api::QosMap => ok!(e, "t_qos.cpp:83", st),
            Api::Scheduler => ok!(e, "t_qos.cpp:84", st),
            Api::SchedulerGroup => ok!(e, "t_qos.cpp:85", st),
            Api::Wred => ok!(e, "t_qos.cpp:86", st),
            Api::Buffer => ok!(e, "t_qos.cpp:87", st),
            _ => ok!(e, "t_qos.cpp:88", st),
        }
    }
    let (p1, p2) = (e.p1, e.p2);
    let port = sys::SAI_OBJECT_TYPE_PORT;
    let queue = sys::SAI_OBJECT_TYPE_QUEUE;

    // ---- DSCP -> TC: an IFCS map at create (64 entries, dp 0), a group on first use, both sysport attrs
    e.m().log_clear();
    let (_, md) = qmap(
        e,
        sys::SAI_QOS_MAP_TYPE_DSCP_TO_TC as i32,
        entries(64, |d| d >> 3, true, false),
        true,
    );
    check!(
        e,
        "t_qos.cpp:94",
        has(&e.m().log(), "ifcs_qos_map_create h=")
            && has(&e.m().log(), "type=1 count=64 0:0/0")
            && has(&e.m().log(), " 46:5/0 ")
    );
    check_eq!(e, "t_qos.cpp:95", e.m().qos_maps(), 1);
    e.m().log_clear();
    let st = e.set(port, p1, a_oid(sys::SAI_PORT_ATTR_QOS_DSCP_TO_TC_MAP, md));
    ok!(e, "t_qos.cpp:99", st);
    let mut log = e.m().log();
    check!(
        e,
        "t_qos.cpp:101",
        has(&log, "ifcs_qos_map_group_create") && has(&log, " 0=0x0 2=0x")
    ); // ACTION_TYPE MAP, DSCP_TO_TC_DP
    check!(
        e,
        "t_qos.cpp:102",
        has(&log, "ifcs_sysport_attr_set sp=0x3e200001 id=1 u32=")
            && has(&log, "ifcs_sysport_attr_set sp=0x3e200001 id=0 u32=0")
    );
    check_eq!(e, "t_qos.cpp:103", e.m().qos_groups(), 1);
    let st = e.set(port, p2, a_oid(sys::SAI_PORT_ATTR_QOS_DSCP_TO_TC_MAP, md)); // same map: the group is shared
    ok!(e, "t_qos.cpp:104", st);
    check_eq!(e, "t_qos.cpp:105", e.m().qos_groups(), 1);
    // DOT1P -> TC on p1: a second group (DSCP + DOT1P), the old one stays for p2
    let (_, m1p) = qmap(
        e,
        sys::SAI_QOS_MAP_TYPE_DOT1P_TO_TC as i32,
        entries(8, |d| 7 - d, false, false),
        true,
    );
    check!(e, "t_qos.cpp:108", has(&e.m().log(), "type=0 count=8 0:7/0"));
    e.m().log_clear();
    let st = e.set(port, p1, a_oid(sys::SAI_PORT_ATTR_QOS_DOT1P_TO_TC_MAP, m1p));
    ok!(e, "t_qos.cpp:112", st);
    check!(
        e,
        "t_qos.cpp:113",
        has(&e.m().log(), " 2=0x") && has(&e.m().log(), " 1=0x")
    );
    check_eq!(e, "t_qos.cpp:114", e.m().qos_groups(), 2);
    check!(
        e,
        "t_qos.cpp:115",
        has(&e.m().log(), "ifcs_sysport_attr_set sp=0x3e200001 id=0 u32=") && !has(&e.m().log(), "id=0 u32=0\n")
    );
    // map update: one attr_set with the new table
    e.m().log_clear();
    let upd = entries(64, |d| if d == 46 { 6 } else { d >> 3 }, true, false);
    let st = e.set(
        sys::SAI_OBJECT_TYPE_QOS_MAP,
        md,
        Attr::new(sys::SAI_QOS_MAP_ATTR_MAP_TO_VALUE_LIST, Value::QosMap(upd)),
    );
    ok!(e, "t_qos.cpp:122", st);
    check!(
        e,
        "t_qos.cpp:123",
        has(&e.m().log(), "ifcs_qos_map_attr_set") && has(&e.m().log(), " 46:6/0 ")
    );
    // unbinding: p2 off -> its group goes once nobody uses it
    let st = e.set(port, p2, a_oid(sys::SAI_PORT_ATTR_QOS_DSCP_TO_TC_MAP, 0));
    ok!(e, "t_qos.cpp:127", st);
    check_eq!(e, "t_qos.cpp:128", e.m().qos_groups(), 1);
    check_eq!(
        e,
        "t_qos.cpp:129",
        e.remove(sys::SAI_OBJECT_TYPE_QOS_MAP, md),
        sys::SAI_STATUS_OBJECT_IN_USE
    );
    // TC -> queue: identity accepted (nothing programmed), anything else NOT_SUPPORTED; TC -> PG kept
    e.m().log_clear();
    let (_, mq) = qmap(
        e,
        sys::SAI_QOS_MAP_TYPE_TC_TO_QUEUE as i32,
        entries(8, |d| d, false, true),
        true,
    );
    let st = e.set(port, p1, a_oid(sys::SAI_PORT_ATTR_QOS_TC_TO_QUEUE_MAP, mq));
    ok!(e, "t_qos.cpp:135", st);
    check!(e, "t_qos.cpp:136", !has(&e.m().log(), "ifcs_"));
    let (st, _) = qmap(
        e,
        sys::SAI_QOS_MAP_TYPE_TC_TO_QUEUE as i32,
        entries(
            8,
            |d| {
                if d == 1 {
                    2
                } else if d == 2 {
                    1
                } else {
                    d
                }
            },
            false,
            true,
        ),
        false,
    );
    check_eq!(e, "t_qos.cpp:139", st, sys::SAI_STATUS_NOT_SUPPORTED);
    let mut tcpg = vec![QosMapEntry::default(); 8];
    for (i, t) in tcpg.iter_mut().enumerate() {
        t.key.tc = i as u8;
        t.value.pg = if i == 3 || i == 4 { i as u8 } else { 0 };
    }
    let (_, mpg) = qmap(e, sys::SAI_QOS_MAP_TYPE_TC_TO_PRIORITY_GROUP as i32, tcpg, true);
    let st = e.set(port, p1, a_oid(sys::SAI_PORT_ATTR_QOS_TC_TO_PRIORITY_GROUP_MAP, mpg));
    ok!(e, "t_qos.cpp:146", st);
    check!(e, "t_qos.cpp:147", !has(&e.m().log(), "ifcs_"));
    let rem = vec![QosMapEntry {
        key: QosParams::default(),
        value: QosParams::default(),
    }];
    let (st, _) = qmap(e, sys::SAI_QOS_MAP_TYPE_TC_AND_COLOR_TO_DSCP as i32, rem, false);
    check_eq!(e, "t_qos.cpp:151", st, sys::SAI_STATUS_NOT_SUPPORTED);

    // ---- default TC, PFC (combined then separate)
    e.m().log_clear();
    let st = e.set(port, p1, a_u8(sys::SAI_PORT_ATTR_QOS_DEFAULT_TC, 1));
    ok!(e, "t_qos.cpp:157", st);
    check!(
        e,
        "t_qos.cpp:158",
        has(&e.m().log(), "ifcs_sysport_attr_set sp=0x3e200001 id=49 u32=1")
    );
    e.m().log_clear();
    let st = e.set(port, p1, a_u8(sys::SAI_PORT_ATTR_PRIORITY_FLOW_CONTROL, 0x18));
    ok!(e, "t_qos.cpp:162", st);
    log = e.m().log();
    check!(
        e,
        "t_qos.cpp:164",
        has(&log, "dp=1 id=82 u32=1")
            && has(&log, "dp=1 id=83 u32=1")
            && has(&log, "dp=1 id=79 u32=0")
            && has(&log, "dp=1 id=86 u32=0")
    );
    e.m().log_clear();
    let st = e.set(
        port,
        p1,
        a_s32(
            sys::SAI_PORT_ATTR_PRIORITY_FLOW_CONTROL_MODE,
            sys::SAI_PORT_PRIORITY_FLOW_CONTROL_MODE_SEPARATE as i32,
        ),
    );
    ok!(e, "t_qos.cpp:168", st);
    log = e.m().log();
    check!(e, "t_qos.cpp:170", has(&log, "dp=1 id=78 u32=1")); // PFC_MODE separate
    e.m().log_clear();
    let st = e.set(port, p1, a_u8(sys::SAI_PORT_ATTR_PRIORITY_FLOW_CONTROL_TX, 0x08));
    ok!(e, "t_qos.cpp:174", st);
    log = e.m().log();
    check!(
        e,
        "t_qos.cpp:176",
        has(&log, "dp=1 id=90 u32=1") && has(&log, "dp=1 id=98 u32=0") && !has(&log, "id=78")
    ); // PFC3 TX on, RX off
    let st = e.set(port, p1, a_u8(sys::SAI_PORT_ATTR_PRIORITY_FLOW_CONTROL_TX, 0));
    ok!(e, "t_qos.cpp:179", st);
    let st = e.set(
        port,
        p1,
        a_s32(
            sys::SAI_PORT_ATTR_PRIORITY_FLOW_CONTROL_MODE,
            sys::SAI_PORT_PRIORITY_FLOW_CONTROL_MODE_COMBINED as i32,
        ),
    );
    ok!(e, "t_qos.cpp:182", st);
    let st = e.set(port, p1, a_u8(sys::SAI_PORT_ATTR_PRIORITY_FLOW_CONTROL, 0));
    ok!(e, "t_qos.cpp:185", st);

    // ---- schedulers via the queue's scheduler group; weight change follows; strict; port shaper
    let qs = objlist(e, p1, sys::SAI_PORT_ATTR_QOS_QUEUE_LIST);
    let sgs = objlist(e, p1, sys::SAI_PORT_ATTR_QOS_SCHEDULER_GROUP_LIST);
    check!(e, "t_qos.cpp:190", qs.len() == 8 && sgs.len() == 8);
    let sched = sys::SAI_OBJECT_TYPE_SCHEDULER;
    let sgrp = sys::SAI_OBJECT_TYPE_SCHEDULER_GROUP;
    let mut sc = [
        a_s32(
            sys::SAI_SCHEDULER_ATTR_SCHEDULING_TYPE,
            sys::SAI_SCHEDULING_TYPE_DWRR as i32,
        ),
        a_u8(sys::SAI_SCHEDULER_ATTR_SCHEDULING_WEIGHT, 20),
    ];
    let (st, sd) = e.create(sched, &sc);
    ok!(e, "t_qos.cpp:197", st);
    sc[0] = a_s32(
        sys::SAI_SCHEDULER_ATTR_SCHEDULING_TYPE,
        sys::SAI_SCHEDULING_TYPE_STRICT as i32,
    );
    let (st, ssp) = e.create(sched, &sc[..1]);
    ok!(e, "t_qos.cpp:199", st);
    e.m().log_clear();
    let st = e.set(
        sgrp,
        sgs[3],
        a_oid(sys::SAI_SCHEDULER_GROUP_ATTR_SCHEDULER_PROFILE_ID, sd),
    );
    ok!(e, "t_qos.cpp:203", st);
    check!(
        e,
        "t_qos.cpp:204",
        has(&e.m().log(), "ifcs_queue_attr_set dp=1 q=3 n=4 13=1 14=20 16=0 21=0")
    );
    check_eq!(e, "t_qos.cpp:205", e.m().queue_attr(1, 3, Q_WDRR_WEIGHT), 20u32);
    e.m().log_clear();
    let st = e.set(sched, sd, a_u8(sys::SAI_SCHEDULER_ATTR_SCHEDULING_WEIGHT, 50));
    ok!(e, "t_qos.cpp:209", st);
    check!(
        e,
        "t_qos.cpp:210",
        has(&e.m().log(), "ifcs_queue_attr_set dp=1 q=3 n=4 13=1 14=50 16=0 21=0")
    );
    e.m().log_clear();
    let st = e.set(
        sgrp,
        sgs[5],
        a_oid(sys::SAI_SCHEDULER_GROUP_ATTR_SCHEDULER_PROFILE_ID, ssp),
    );
    ok!(e, "t_qos.cpp:214", st);
    check!(
        e,
        "t_qos.cpp:215",
        has(&e.m().log(), "ifcs_queue_attr_set dp=1 q=5 n=3 13=0 16=0 21=0")
    );
    e.m().log_clear();
    let st = e.set(
        sgrp,
        sgs[5],
        a_oid(sys::SAI_SCHEDULER_GROUP_ATTR_SCHEDULER_PROFILE_ID, 0),
    );
    ok!(e, "t_qos.cpp:218", st);
    check!(
        e,
        "t_qos.cpp:219",
        has(&e.m().log(), "ifcs_queue_attr_set dp=1 q=5 n=4 13=1 14=1 16=0 21=0")
    ); // default: WDRR 1
    let shp = [
        a_s32(sys::SAI_SCHEDULER_ATTR_METER_TYPE, sys::SAI_METER_TYPE_BYTES as i32),
        a_u64(sys::SAI_SCHEDULER_ATTR_MAX_BANDWIDTH_RATE, 125_000_000),
        a_u64(sys::SAI_SCHEDULER_ATTR_MAX_BANDWIDTH_BURST_RATE, 65536),
    ];
    let (st, sh) = e.create(sched, &shp);
    ok!(e, "t_qos.cpp:224", st);
    e.m().log_clear();
    let st = e.set(port, p1, a_oid(sys::SAI_PORT_ATTR_QOS_SCHEDULER_PROFILE_ID, sh));
    ok!(e, "t_qos.cpp:228", st);
    log = e.m().log();
    check!(
        e,
        "t_qos.cpp:230",
        has(&log, "ifcs_devport_attr_set dp=1 id=104 u32=1")
            && has(&log, "dp=1 id=105")
            && has(&log, "dp=1 id=106 u32=65536")
    );
    check!(e, "t_qos.cpp:231", has(&log, "dp=1 id=105 u32=1000000000")); // 125000000 B/s as bits/s
    e.m().log_clear();
    let st = e.set(port, p1, a_oid(sys::SAI_PORT_ATTR_QOS_SCHEDULER_PROFILE_ID, 0));
    ok!(e, "t_qos.cpp:234", st);
    check!(
        e,
        "t_qos.cpp:235",
        has(&e.m().log(), "ifcs_devport_attr_set dp=1 id=104 u32=0")
    );
    // queue shaper: a scheduler with a max rate on a queue
    e.m().log_clear();
    let st = e.set(
        sgrp,
        sgs[1],
        a_oid(sys::SAI_SCHEDULER_GROUP_ATTR_SCHEDULER_PROFILE_ID, sh),
    );
    ok!(e, "t_qos.cpp:240", st);
    check!(
        e,
        "t_qos.cpp:241",
        has(
            &e.m().log(),
            "ifcs_queue_attr_set dp=1 q=1 n=6 13=1 14=1 16=0 21=1 22=1000000000 23=65536"
        )
    ); // bits/s
    let st = e.set(
        sgrp,
        sgs[1],
        a_oid(sys::SAI_SCHEDULER_GROUP_ATTR_SCHEDULER_PROFILE_ID, 0),
    );
    ok!(e, "t_qos.cpp:243", st);
    let st = e.set(
        sgrp,
        sgs[3],
        a_oid(sys::SAI_SCHEDULER_GROUP_ATTR_SCHEDULER_PROFILE_ID, 0),
    );
    ok!(e, "t_qos.cpp:244", st);
    let st = e.remove(sched, sd);
    ok!(e, "t_qos.cpp:245", st);
    let st = e.remove(sched, ssp);
    ok!(e, "t_qos.cpp:246", st);
    let st = e.remove(sched, sh);
    ok!(e, "t_qos.cpp:247", st);

    // ---- WRED: curve group in cells (isai: 100000 B -> 440, 2000000 B -> 8928), queue flags
    let w = [
        a_bool(sys::SAI_WRED_ATTR_GREEN_ENABLE, true),
        a_u32(sys::SAI_WRED_ATTR_GREEN_MIN_THRESHOLD, 100_000),
        a_u32(sys::SAI_WRED_ATTR_GREEN_MAX_THRESHOLD, 2_000_000),
        a_u32(sys::SAI_WRED_ATTR_GREEN_DROP_PROBABILITY, 5),
        a_s32(sys::SAI_WRED_ATTR_ECN_MARK_MODE, sys::SAI_ECN_MARK_MODE_GREEN as i32),
    ];
    let wred = sys::SAI_OBJECT_TYPE_WRED;
    e.m().log_clear();
    let (st, wr) = e.create(wred, &w);
    ok!(e, "t_qos.cpp:258", st);
    check!(
        e,
        "t_qos.cpp:259",
        has(&e.m().log(), "ifcs_aqm_curve_group_create")
            && has(
                &e.m().log(),
                " 0=440 1=8928 2=5 6=45728 7=45728 8=100 12=45728 13=45728 14=100"
            )
    );
    check_eq!(e, "t_qos.cpp:260", e.m().qos_curves(), 1);
    e.m().log_clear();
    let st = e.set(queue, qs[3], a_oid(sys::SAI_QUEUE_ATTR_WRED_PROFILE_ID, wr));
    ok!(e, "t_qos.cpp:264", st);
    log = e.m().log();
    check!(
        e,
        "t_qos.cpp:266",
        has(&log, "ifcs_queue_attr_set dp=1 q=3 n=1 26=")
            && has(&log, "ifcs_queue_attr_set dp=1 q=3 n=1 28=1")
            && has(&log, "ifcs_queue_attr_set dp=1 q=3 n=1 27=1")
            && !has(&log, " 32=")
    ); // no averaging weight: the default
    e.m().log_clear();
    let st = e.set(wred, wr, a_u32(sys::SAI_WRED_ATTR_GREEN_MAX_THRESHOLD, 3_000_000));
    ok!(e, "t_qos.cpp:271", st);
    check!(
        e,
        "t_qos.cpp:272",
        has(&e.m().log(), "ifcs_aqm_curve_group_attr_set") && has(&e.m().log(), " 1=13392 ")
    );
    check_eq!(e, "t_qos.cpp:273", e.remove(wred, wr), sys::SAI_STATUS_OBJECT_IN_USE);
    e.m().log_clear();
    let st = e.set(queue, qs[3], a_oid(sys::SAI_QUEUE_ATTR_WRED_PROFILE_ID, 0));
    ok!(e, "t_qos.cpp:277", st);
    check!(
        e,
        "t_qos.cpp:278",
        has(&e.m().log(), "ifcs_queue_attr_set dp=1 q=3 n=1 28=0")
            && has(&e.m().log(), "ifcs_queue_attr_set dp=1 q=3 n=1 27=0")
            && has(&e.m().log(), "ifcs_queue_attr_set dp=1 q=3 n=1 26=0")
    );
    // a profile without ECN never writes ECN_EXTENSION_ENABLE (0.12.41 refuses it on this queue type)
    let (st, wr2) = e.create(wred, &w[..3]);
    ok!(e, "t_qos.cpp:283", st);
    e.m().log_clear();
    let st = e.set(queue, qs[3], a_oid(sys::SAI_QUEUE_ATTR_WRED_PROFILE_ID, wr2));
    ok!(e, "t_qos.cpp:286", st);
    check!(
        e,
        "t_qos.cpp:287",
        has(&e.m().log(), "n=1 27=1") && !has(&e.m().log(), " 28=")
    );
    let st = e.set(queue, qs[3], a_oid(sys::SAI_QUEUE_ATTR_WRED_PROFILE_ID, 0));
    ok!(e, "t_qos.cpp:289", st);
    check!(e, "t_qos.cpp:290", !has(&e.m().log(), " 28="));
    let st = e.remove(wred, wr2);
    ok!(e, "t_qos.cpp:291", st);
    let st = e.remove(wred, wr);
    ok!(e, "t_qos.cpp:292", st);
    check_eq!(e, "t_qos.cpp:293", e.m().qos_curves(), 0);

    // ---- buffer profile on a queue: reserve 1518 B = 7 cells with the port flapped, dynamic th 3 -> 8 / 4
    let bpool = sys::SAI_OBJECT_TYPE_BUFFER_POOL;
    let bprof = sys::SAI_OBJECT_TYPE_BUFFER_PROFILE;
    let mut pl = [
        a_s32(sys::SAI_BUFFER_POOL_ATTR_TYPE, sys::SAI_BUFFER_POOL_TYPE_EGRESS as i32),
        a_u64(sys::SAI_BUFFER_POOL_ATTR_SIZE, 10_000_000),
        a_s32(
            sys::SAI_BUFFER_POOL_ATTR_THRESHOLD_MODE,
            sys::SAI_BUFFER_POOL_THRESHOLD_MODE_DYNAMIC as i32,
        ),
    ];
    let (st, pool) = e.create(bpool, &pl);
    ok!(e, "t_qos.cpp:301", st);
    pl[0] = a_s32(sys::SAI_BUFFER_POOL_ATTR_TYPE, sys::SAI_BUFFER_POOL_TYPE_INGRESS as i32);
    let (st, pin) = e.create(bpool, &pl);
    ok!(e, "t_qos.cpp:303", st);
    let mut bp = [
        a_oid(sys::SAI_BUFFER_PROFILE_ATTR_POOL_ID, pool),
        a_u64(sys::SAI_BUFFER_PROFILE_ATTR_RESERVED_BUFFER_SIZE, 1518),
        Attr::new(sys::SAI_BUFFER_PROFILE_ATTR_SHARED_DYNAMIC_TH, Value::S8(3)),
    ];
    let (st, prof) = e.create(bprof, &bp);
    ok!(e, "t_qos.cpp:308", st);
    bp[0] = a_oid(sys::SAI_BUFFER_PROFILE_ATTR_POOL_ID, pin);
    let (st, ppg) = e.create(bprof, &bp);
    ok!(e, "t_qos.cpp:310", st);
    let st = e.set(port, p1, a_bool(sys::SAI_PORT_ATTR_ADMIN_STATE, true));
    ok!(e, "t_qos.cpp:313", st);
    e.m().log_clear();
    let st = e.set(queue, qs[0], a_oid(sys::SAI_QUEUE_ATTR_BUFFER_PROFILE_ID, prof));
    ok!(e, "t_qos.cpp:317", st);
    log = e.m().log();
    let npos = usize::MAX;
    let down = log.find("ifcs_devport_attr_set dp=1 id=23 u32=0").unwrap_or(npos);
    let set_ = log
        .find("ifcs_queue_attr_set dp=1 q=0 n=4 2=7 3=1 6=8 9=4")
        .unwrap_or(npos);
    let up = log.find("ifcs_devport_attr_set dp=1 id=23 u32=1").unwrap_or(npos);
    let adm = log.find("ifcs_queue_attr_set dp=1 q=0 n=2 35=0 34=0").unwrap_or(npos);
    check!(
        e,
        "t_qos.cpp:321",
        down != npos && set_ != npos && up != npos && adm != npos
    );
    check!(e, "t_qos.cpp:322", down < set_ && set_ < up && up < adm);
    e.m().log_clear(); // threshold-only change: no flap
    let st = e.set(
        bprof,
        prof,
        Attr::new(sys::SAI_BUFFER_PROFILE_ATTR_SHARED_DYNAMIC_TH, Value::S8(1)),
    );
    ok!(e, "t_qos.cpp:326", st);
    check!(
        e,
        "t_qos.cpp:327",
        has(&e.m().log(), "ifcs_queue_attr_set dp=1 q=0 n=3 3=1 6=6 9=2") && !has(&e.m().log(), "id=23")
    );
    e.m().log_clear(); // unset: the SDK's values back (reserve 14 cells read at first use)
    let st = e.set(queue, qs[0], a_oid(sys::SAI_QUEUE_ATTR_BUFFER_PROFILE_ID, 0));
    ok!(e, "t_qos.cpp:331", st);
    check!(
        e,
        "t_qos.cpp:332",
        has(&e.m().log(), "ifcs_queue_attr_set dp=1 q=0 n=6 2=14 ")
    );
    // PG profile: kept, nothing programmed
    let (st, a) = e.get1(
        port,
        p1,
        sys::SAI_PORT_ATTR_INGRESS_PRIORITY_GROUP_LIST,
        Value::OidList(vec![0; 8]),
    );
    ok!(e, "t_qos.cpp:338", st);
    let pgs = a.oids();
    e.m().log_clear();
    let ipg = sys::SAI_OBJECT_TYPE_INGRESS_PRIORITY_GROUP;
    let st = e.set(
        ipg,
        pgs[3],
        a_oid(sys::SAI_INGRESS_PRIORITY_GROUP_ATTR_BUFFER_PROFILE, ppg),
    );
    ok!(e, "t_qos.cpp:342", st);
    check!(e, "t_qos.cpp:343", !has(&e.m().log(), "ifcs_"));
    let st = e.set(
        ipg,
        pgs[3],
        a_oid(sys::SAI_INGRESS_PRIORITY_GROUP_ATTR_BUFFER_PROFILE, 0),
    );
    ok!(e, "t_qos.cpp:345", st);
    let st = e.remove(bprof, prof);
    ok!(e, "t_qos.cpp:346", st);
    let st = e.remove(bprof, ppg);
    ok!(e, "t_qos.cpp:347", st);
    let st = e.remove(bpool, pool);
    ok!(e, "t_qos.cpp:348", st);
    let st = e.remove(bpool, pin);
    ok!(e, "t_qos.cpp:349", st);
    let st = e.set(port, p1, a_bool(sys::SAI_PORT_ATTR_ADMIN_STATE, false));
    ok!(e, "t_qos.cpp:352", st);

    // ---- counters: queue (one batched call; watermarks in cells x 224), PG (BAM), PFC frames
    e.m().queue_stat(1, 3, QS_PACKET_TX, 50);
    e.m().queue_stat(1, 3, QS_BYTE_TX, 5000);
    e.m().queue_stat(1, 3, QS_PACKET_DROP, 2);
    e.m().queue_stat(1, 3, QS_TOTAL_USE_WATERMARK, 10);
    let ids = [
        sys::SAI_QUEUE_STAT_PACKETS,
        sys::SAI_QUEUE_STAT_BYTES,
        sys::SAI_QUEUE_STAT_DROPPED_PACKETS,
        sys::SAI_QUEUE_STAT_SHARED_WATERMARK_BYTES,
    ];
    e.m().log_clear();
    let (st, v) = e.sai.stats(queue, qs[3], &ids);
    ok!(e, "t_qos.cpp:363", st);
    check!(
        e,
        "t_qos.cpp:364",
        v[0] == 50 && v[1] == 5000 && v[2] == 2 && v[3] == 2240
    );
    check_eq!(
        e,
        "t_qos.cpp:365",
        count(&e.m().log(), "ifcs_queue_stats_get dp=1 q=3 ids=5,6,3,1,"),
        1usize
    );
    let (st, _) = e.sai.stats_ext(queue, qs[3], &ids, sys::SAI_STATS_MODE_READ_AND_CLEAR);
    ok!(e, "t_qos.cpp:366", st);
    let (st, v) = e.sai.stats(queue, qs[3], &ids);
    ok!(e, "t_qos.cpp:367", st);
    check!(e, "t_qos.cpp:368", v[0] == 0 && v[1] == 0 && v[3] == 0);
    let db = [sys::SAI_QUEUE_STAT_DROPPED_BYTES];
    check_eq!(
        e,
        "t_qos.cpp:370",
        e.sai.stats(queue, qs[3], &db).0,
        sys::SAI_STATUS_NOT_SUPPORTED
    );
    e.m().bam_stat(1, 3, BAM_PACKET_DROP, 7);
    let pid = [sys::SAI_INGRESS_PRIORITY_GROUP_STAT_DROPPED_PACKETS];
    let (st, v) = e.sai.stats(ipg, pgs[3], &pid);
    ok!(e, "t_qos.cpp:373", st);
    check_eq!(e, "t_qos.cpp:374", v[0], 7u64);
    let pp = [sys::SAI_INGRESS_PRIORITY_GROUP_STAT_PACKETS];
    check_eq!(
        e,
        "t_qos.cpp:376",
        e.sai.stats(ipg, pgs[3], &pp).0,
        sys::SAI_STATUS_NOT_SUPPORTED
    );
    e.m().set_stat(1, DP_RX_FRAMES_PFC0 + 3, 11);
    let pf = [sys::SAI_PORT_STAT_PFC_3_RX_PKTS];
    let (st, v) = e.sai.stats(port, p1, &pf);
    ok!(e, "t_qos.cpp:379", st);
    check_eq!(e, "t_qos.cpp:380", v[0], 11u64);
    let (st, cl) = e.sai.stats_capability(e.s, queue, 64);
    ok!(e, "t_qos.cpp:385", st);
    check_eq!(e, "t_qos.cpp:386", cl.len() as u32, 8u32); // the 8 mapped queue counters

    // ---- teardown of the maps
    let st = e.set(port, p1, a_oid(sys::SAI_PORT_ATTR_QOS_DSCP_TO_TC_MAP, 0));
    ok!(e, "t_qos.cpp:391", st);
    let st = e.set(port, p1, a_oid(sys::SAI_PORT_ATTR_QOS_DOT1P_TO_TC_MAP, 0));
    ok!(e, "t_qos.cpp:393", st);
    let st = e.set(port, p1, a_oid(sys::SAI_PORT_ATTR_QOS_TC_TO_QUEUE_MAP, 0));
    ok!(e, "t_qos.cpp:395", st);
    let st = e.set(port, p1, a_oid(sys::SAI_PORT_ATTR_QOS_TC_TO_PRIORITY_GROUP_MAP, 0));
    ok!(e, "t_qos.cpp:397", st);
    check_eq!(e, "t_qos.cpp:398", e.m().qos_groups(), 0);
    let qm = sys::SAI_OBJECT_TYPE_QOS_MAP;
    let st = e.remove(qm, md);
    ok!(e, "t_qos.cpp:399", st);
    let st = e.remove(qm, m1p);
    ok!(e, "t_qos.cpp:400", st);
    let st = e.remove(qm, mq);
    ok!(e, "t_qos.cpp:401", st);
    let st = e.remove(qm, mpg);
    ok!(e, "t_qos.cpp:402", st);
    check_eq!(e, "t_qos.cpp:403", e.m().qos_maps(), 0);
}
