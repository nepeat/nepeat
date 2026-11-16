#!/usr/bin/env python3
"""Offline test of sonic-metrics against a real redis-server loaded with a
fake SONiC dump (the AS7760's lab state: cage 1 split 4x25G, a PortChannel,
BGP + BFD across two VRFs, LLDP, optics, an ACL rule, CoPP, routes), plus
redis missing, a broken table and the services/conflicts sources.

  test.py OUTDIR [--wide]   (redis-server on PATH; writes OUTDIR/metrics.prom;
                             --wide: every cage 4x25G, 128 ports)
"""
import json
import os
import re
import subprocess
import sys
import tempfile
import time

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import sonic_metrics as sm  # noqa: E402

FAILS = []
WIDE = "--wide" in sys.argv


def check(name, cond, detail=""):
    print(("PASS " if cond else "FAIL ") + name + ("" if cond else " :: " + str(detail)[-2000:]))
    if not cond:
        FAILS.append(name)


# ------------------------------------------------------------------ the dump
APPL, ASIC, COUNTERS, CONFIG, STATE = 0, 1, 2, 4, 6
H = {}   # (db, key) -> {field: value}
S = {}   # (db, key) -> string


def port_list():
    """AS7760 lab shape: cage 1 4x25G (Ethernet0..3), cages 2..32 1x100G."""
    ports = []
    for cage in range(1, 33):
        sub = 4 if cage == 1 or WIDE else 1
        for i in range(sub):
            idx = 4 * (cage - 1) + i
            lanes = [48 + 4 * (cage - 1) + i + 1] if sub == 4 else [48 + 4 * (cage - 1) + j + 1 for j in range(4)]
            ports.append(("Ethernet%d" % idx, "Ethernet%d/%d" % (cage, i + 1), cage, lanes, 25000 if sub == 4 else 100000))
    return ports


def build():
    S[(CONFIG, "CONFIG_DB_INITIALIZED")] = "1"
    H[(CONFIG, "DEVICE_METADATA|localhost")] = {"platform": "powerpc64-accton_as7760_32x-r0", "hwsku": "Accton-AS7760-32X",
                                                "type": "LeafRouter", "bgp_asn": "65100", "hostname": "lab"}
    H[(APPL, "PORT_TABLE:PortInitDone")] = {"lanes": "0"}
    H[(APPL, "PORT_TABLE:PortConfigDone")] = {"count": "35"}
    names = {}
    for n, (name, alias, cage, lanes, speed) in enumerate(port_list()):
        up = name in ("Ethernet0", "Ethernet124", "Ethernet20", "Ethernet24")
        H[(CONFIG, "PORT|" + name)] = {"alias": alias, "index": str(cage), "lanes": ",".join(map(str, lanes)),
                                       "speed": str(speed), "admin_status": "up" if up else "down", "mtu": "9100"}
        H[(APPL, "PORT_TABLE:" + name)] = {"alias": alias, "speed": str(speed), "mtu": "9100",
                                           "admin_status": "up" if up else "down",
                                           "oper_status": "up" if up and name != "Ethernet24" else "down"}
        oid = "oid:0x1000000000%03x" % n
        names[name] = oid
        c = {f: "0" for _, _, fs in sm.PORT_COUNTERS for f in fs}
        c.update({"SAI_PORT_STAT_IF_IN_OCTETS": str(1000 * (n + 1)), "SAI_PORT_STAT_IF_IN_UCAST_PKTS": "10",
                  "SAI_PORT_STAT_IF_IN_NON_UCAST_PKTS": "5"})
        for k in range(60):   # syncd writes ~60-90 stats per port; only 10 are read
            c["SAI_PORT_STAT_FILLER_%d" % k] = str(k)
        H[(COUNTERS, "COUNTERS:" + oid)] = c
    H[(COUNTERS, "COUNTERS_PORT_NAME_MAP")] = names
    H[(COUNTERS, "COUNTERS:oid:0x1000000000000")]["SAI_PORT_STAT_IF_IN_ERRORS"] = "7"
    H[(COUNTERS, "COUNTERS:oid:0x1000000000000")]["SAI_PORT_STAT_IF_OUT_OCTETS"] = "123456789012"
    # PortChannel0001 = Ethernet20 + Ethernet24 (Ethernet24 down, not selected)
    H[(CONFIG, "PORTCHANNEL|PortChannel0001")] = {"admin_status": "up", "mtu": "9100", "min_links": "1"}
    for p, sel in (("Ethernet20", "true"), ("Ethernet24", "false")):
        H[(CONFIG, "PORTCHANNEL_MEMBER|PortChannel0001|" + p)] = {"NULL": "NULL"}
        H[(APPL, "LAG_MEMBER_TABLE:PortChannel0001:" + p)] = {"status": "enabled" if sel == "true" else "disabled"}
        H[(STATE, "LAG_MEMBER_TABLE|PortChannel0001|" + p)] = {"runner.aggregator.selected": sel, "runner.selected": sel,
                                                               "runner.actor_lacpdu_info.state": "61"}
    H[(APPL, "LAG_TABLE:PortChannel0001")] = {"admin_status": "up", "oper_status": "up", "mtu": "9100"}
    H[(STATE, "LAG_TABLE|PortChannel0001")] = {"runner.active": "true"}
    # BGP: default VRF neighbor (bgpmon reports it), Vrf-test neighbor (config only), an admin-down v6 peer
    H[(CONFIG, "BGP_NEIGHBOR|10.0.0.1")] = {"asn": "65100", "name": "to-vrf-test", "local_addr": "10.0.0.0",
                                            "admin_status": "up", "bfd": "true"}
    H[(CONFIG, "BGP_NEIGHBOR|Vrf-test|10.0.0.0")] = {"asn": "65100", "name": "to-default", "local_addr": "10.0.0.1",
                                                     "admin_status": "up"}
    H[(CONFIG, "BGP_NEIGHBOR|fc00::2")] = {"asn": "65200", "name": "v6-peer", "admin_status": "down"}
    H[(STATE, "NEIGH_STATE_TABLE|10.0.0.1")] = {"state": "Established", "peerType": "i-BGP"}
    H[(STATE, "NEIGH_STATE_TABLE|fc00::2")] = {"state": "Idle (Admin)", "peerType": "e-BGP"}
    # BFD (bfdsyncd): one Up, one Down, one Admin_Down in Vrf-test
    bfd = {"type": "async_active", "local_addr": "10.0.0.0", "tx_interval": "300", "rx_interval": "300",
           "multiplier": "3", "multihop": "false", "local_discriminator": "1"}
    H[(STATE, "BFD_SESSION_TABLE|default|default|10.0.0.1")] = dict(bfd, state="Up")
    H[(STATE, "BFD_SESSION_TABLE|default|Ethernet0|10.2.0.2")] = dict(bfd, state="Down", local_addr="10.2.0.1")
    H[(STATE, "BFD_SESSION_TABLE|Vrf-test|Ethernet124|10.0.0.0")] = dict(bfd, state="Admin_Down", local_addr="10.0.0.1")
    # LLDP: the lab cable Ethernet1/1 <-> Ethernet32/1 (+ the management port)
    for a, b in (("Ethernet0", "Ethernet32/1"), ("Ethernet124", "Ethernet1/1"), ("eth0", "port 7")):
        H[(APPL, "LLDP_ENTRY_TABLE:" + a)] = {"lldp_rem_sys_name": "lab-switch" if a != "eth0" else "agg",
                                              "lldp_rem_port_id": b, "lldp_rem_port_desc": "",
                                              "lldp_rem_chassis_id": "52:54:00:aa:bb:01",
                                              "lldp_rem_port_id_subtype": "5", "lldp_rem_time_mark": "123"}
    H[(APPL, "LLDP_LOC_CHASSIS")] = {"lldp_loc_chassis_id": "52:54:00:aa:bb:01"}
    # Transceivers: a DAC (no DOM) in Ethernet0, an SR4 with DOM in Ethernet124, one with only DOM_TEMPERATURE
    H[(STATE, "TRANSCEIVER_INFO|Ethernet0")] = {"type": "SFP/SFP+/SFP28", "manufacturer": "FAKE-VENDOR", "model": "FAKE-SFP28-DAC"}
    H[(STATE, "TRANSCEIVER_DOM_SENSOR|Ethernet0")] = {"temperature": "N/A", "voltage": "N/A"}
    H[(STATE, "TRANSCEIVER_INFO|Ethernet124")] = {"type": "QSFP28 or later", "manufacturer": "FAKE-VENDOR     ",
                                                  "model": "FAKE-QSFP28-SR4 "}
    H[(STATE, "TRANSCEIVER_DOM_SENSOR|Ethernet124")] = {"temperature": "30.5", "voltage": "3.29", "rx1power": "-2.1"}
    H[(STATE, "TRANSCEIVER_INFO|Ethernet8")] = {"type": "QSFP28 or later", "manufacturer": "X", "model": "Y"}
    H[(STATE, "TRANSCEIVER_DOM_TEMPERATURE|Ethernet8")] = {"temperature": "41.25"}
    # ACL: a rule with counter (counterpoll acl on), one without
    H[(CONFIG, "ACL_TABLE|DROPTEST")] = {"type": "L3", "stage": "ingress", "ports@": "Ethernet124"}
    H[(CONFIG, "ACL_RULE|DROPTEST|R1")] = {"PRIORITY": "9000", "PACKET_ACTION": "DROP", "DST_IP": "10.9.9.9/32"}
    H[(CONFIG, "ACL_RULE|DROPTEST|R2")] = {"PRIORITY": "8000", "PACKET_ACTION": "FORWARD", "DST_IP": "10.9.9.8/32"}
    H[(STATE, "ACL_RULE_TABLE|DROPTEST|R1")] = {"status": "Active"}
    H[(STATE, "ACL_RULE_TABLE|DROPTEST|R2")] = {"status": "Inactive"}
    H[(COUNTERS, "ACL_COUNTER_RULE_MAP")] = {"DROPTEST:R1": "oid:0x9000000000a01"}
    H[(COUNTERS, "COUNTERS:oid:0x9000000000a01")] = {"SAI_ACL_COUNTER_ATTR_PACKETS": "40", "SAI_ACL_COUNTER_ATTR_BYTES": "4080"}
    # CoPP (coppmgrd) and one trap counter
    H[(APPL, "COPP_TABLE:queue4_group1")] = {"trap_action": "trap", "trap_priority": "4", "queue": "4",
                                             "trap_ids": "bgp,bgpv6,lacp", "meter_type": "packets", "mode": "sr_tcm",
                                             "cir": "6000", "cbs": "6000"}
    H[(APPL, "COPP_TABLE:queue4_group2")] = {"trap_action": "copy", "queue": "4", "trap_ids": "arp_req,arp_resp,neigh_discovery",
                                             "meter_type": "packets", "cir": "600", "cbs": "600"}
    H[(COUNTERS, "COUNTERS_TRAP_NAME_MAP")] = {"bgp": "oid:0x7000000000b01"}
    H[(COUNTERS, "COUNTERS:oid:0x7000000000b01")] = {"SAI_COUNTER_STAT_PACKETS": "99", "SAI_COUNTER_STAT_BYTES": "9900"}
    # Routes: APPL (fpmsyncd) and ASIC
    for p in ("10.0.0.0/31", "10.255.0.1/32", "192.0.2.0/24", "fc00::/64"):
        H[(APPL, "ROUTE_TABLE:" + p)] = {"nexthop": "", "ifname": "Ethernet0"}
    for p in ("10.0.0.0/31", "198.51.100.0/24", "fc00:1::/64"):
        H[(APPL, "ROUTE_TABLE:Vrf-test:" + p)] = {"nexthop": "", "ifname": "Ethernet124"}
    for p in ("10.0.0.0/31", "192.0.2.0/24", "fc00::/64"):
        H[(ASIC, 'ASIC_STATE:SAI_OBJECT_TYPE_ROUTE_ENTRY:{"dest":"%s","switch_id":"oid:0x21000000000000","vr":"oid:0x3000000000022"}' % p)] = \
            {"SAI_ROUTE_ENTRY_ATTR_PACKET_ACTION": "SAI_PACKET_ACTION_FORWARD"}
    # A non-hash key inside a table's key space must not break the table.
    S[(STATE, "TRANSCEIVER_INFO|garbage")] = "x"


# ------------------------------------------------------------------ helpers

def load(r):
    by_db = {}
    for (db, key), f in H.items():
        by_db.setdefault(db, []).append(["HSET", key] + [x for kv in f.items() for x in kv])
    for (db, key), v in S.items():
        by_db.setdefault(db, []).append(["SET", key, v])
    for db, cmds in by_db.items():
        rep = r.pipeline([["SELECT", db]] + cmds)
        bad = [x for x in rep if isinstance(x, sm.RedisError)]
        assert not bad, bad


def parse(text):
    """{(name, frozenset(labels)): value}."""
    out = {}
    for line in text.splitlines():
        if not line or line.startswith("#"):
            continue
        m = re.match(r'^([a-z_:]+)(?:\{(.*)\})? (\S+)$', line)
        assert m, line
        labels = frozenset(re.findall(r'([a-z_]+)="((?:[^"\\]|\\.)*)"', m.group(2) or ""))
        out[(m.group(1), labels)] = float(m.group(3))
    return out


def get(ms, metric, **labels):
    """Value of the one sample of metric whose labels include the given ones."""
    hits = [v for (n, ls), v in ms.items() if n == metric and set(labels.items()) <= ls]
    return hits[0] if len(hits) == 1 else ("%d matches" % len(hits))


def count(ms, name):
    return sum(1 for (n, _) in ms if n == name)


def run(*args):
    t0 = time.monotonic()
    p = subprocess.run([sys.executable, "-I", "-S", os.path.join(HERE, "sonic_metrics.py")] + list(args),
                       stdout=subprocess.PIPE, stderr=subprocess.PIPE, universal_newlines=True, timeout=30)
    return p, time.monotonic() - t0


def main():
    out = sys.argv[1]
    os.makedirs(out, exist_ok=True)
    tmp = tempfile.mkdtemp()
    sock = os.path.join(tmp, "redis.sock")
    srv = subprocess.Popen(["redis-server", "--port", "0", "--unixsocket", sock, "--save", "", "--appendonly", "no",
                            "--databases", "20"], stdout=subprocess.DEVNULL)
    try:
        for _ in range(100):
            if os.path.exists(sock):
                break
            time.sleep(0.05)
        build()
        r = sm.Redis(path=sock)
        load(r)
        dbcfg = os.path.join(tmp, "database_config.json")
        with open(dbcfg, "w") as f:
            json.dump({"INSTANCES": {"redis": {"hostname": "127.0.0.1", "port": 1, "unix_socket_path": sock}},
                       "DATABASES": {n: {"id": i, "separator": s, "instance": "redis"} for n, (i, s) in sm.DEFAULT_DBS.items()}}, f)
        conflicts = os.path.join(tmp, "conflicts.json")
        with open(conflicts, "w") as f:
            json.dump({"time": 1794790000, "notes": ["PORT|Ethernet8 speed: seeded 100000, user 40000"],
                       "conflicts": [{"table": "DEVICE_METADATA", "key": "localhost", "field": "hostname", "nix": "as7760",
                                      "current": "x", "source": "runtime"},
                                     {"table": "PORT", "key": "Ethernet0", "field": "alias", "nix": "Ethernet1/1",
                                      "current": "y", "source": "runtime"}]}, f)
        systemctl = os.path.join(tmp, "systemctl")
        mono = time.monotonic()
        with open(systemctl, "w") as f:
            f.write("#!/bin/sh\n[ \"$1 $2\" = 'show --property=Id,ActiveState,NRestarts,ActiveEnterTimestampMonotonic' ] || exit 3\n"
                    "cat <<EOF\nId=sonic-syncd.service\nActiveState=active\nNRestarts=0\nActiveEnterTimestampMonotonic=%d\n\n"
                    "Id=sonic-orchagent.service\nActiveState=active\nNRestarts=2\nActiveEnterTimestampMonotonic=%d\n\n"
                    "Id=sonic-bgpmon.service\nActiveState=failed\nNRestarts=5\nActiveEnterTimestampMonotonic=0\n\n"
                    "Id=redis-sonic.service\nActiveState=active\nNRestarts=0\nActiveEnterTimestampMonotonic=%d\nEOF\n"
                    % (int((mono - 100) * 1e6), int((mono - 10) * 1e6), int((mono - 200) * 1e6)))
        os.chmod(systemctl, 0o755)

        # ------------------------------------------------ 1. full run
        p, dt = run("--db-config", dbcfg, "--conflicts", conflicts, "--systemctl", systemctl)
        with open(os.path.join(out, "metrics.prom"), "w") as f:
            f.write(p.stdout)
        check("exit 0, nothing on stderr", p.returncode == 0 and p.stderr == "", p.stderr)
        ms = parse(p.stdout)
        nports = len(port_list())
        print("INFO %d samples, %d ports, %.3f s wall (Python start-up included), collect %s s"
              % (len(ms), nports, dt, get(ms, "sonic_collect_duration_seconds")))
        check("sonic_up 1", get(ms, "sonic_up") == 1)
        succ = [(n, sorted(l), v) for (n, l), v in ms.items() if n == "sonic_collector_success"]
        check("all 13 collectors succeeded", len(succ) == 13 and all(v == 1 for _, _, v in succ), succ)
        check("device info", get(ms, "sonic_device_info", hwsku="Accton-AS7760-32X", type="LeafRouter", bgp_asn="65100") == 1)
        check("config initialized, ports init done", get(ms, "sonic_config_db_initialized") == 1 and get(ms, "sonic_port_init_done") == 1)
        # ports
        check("one sonic_port_info per CONFIG_DB port", count(ms, "sonic_port_info") == nports, count(ms, "sonic_port_info"))
        check("port labels: SONiC name + alias", get(ms, "sonic_port_info", port="Ethernet1", alias="Ethernet1/2", index="1", lanes="50") == 1)
        check("Ethernet0 admin up, oper up, 25G", get(ms, "sonic_port_admin_up", port="Ethernet0") == 1
              and get(ms, "sonic_port_oper_up", port="Ethernet0", alias="Ethernet1/1") == 1
              and get(ms, "sonic_port_speed_bytes", port="Ethernet0") == 25e9 / 8)
        check("Ethernet24 admin up, oper down", get(ms, "sonic_port_admin_up", port="Ethernet24") == 1
              and get(ms, "sonic_port_oper_up", port="Ethernet24") == 0)
        check("Ethernet8 admin down, 100G, mtu 9100", get(ms, "sonic_port_admin_up", port="Ethernet8") == 0
              and get(ms, "sonic_port_speed_bytes", port="Ethernet8") == (25e9 if WIDE else 100e9) / 8
              and get(ms, "sonic_port_mtu_bytes", port="Ethernet8") == 9100)
        check("PortInitDone/PortConfigDone are no ports", get(ms, "sonic_port_info", port="PortInitDone") == "0 matches")
        # counters
        check("counters for every port", count(ms, "sonic_port_rx_bytes_total") == nports)
        check("rx packets = ucast + non-ucast", get(ms, "sonic_port_rx_packets_total", port="Ethernet0") == 15)
        check("rx errors / tx bytes (large integer exact)", get(ms, "sonic_port_rx_errors_total", port="Ethernet0", alias="Ethernet1/1") == 7
              and 'sonic_port_tx_bytes_total{port="Ethernet0",alias="Ethernet1/1"} 123456789012' in p.stdout)
        check("counters are TYPE counter", "# TYPE sonic_port_rx_bytes_total counter" in p.stdout)
        # portchannels
        check("PortChannel0001 oper up, 2 members, 1 selected", get(ms, "sonic_portchannel_oper_up", portchannel="PortChannel0001") == 1
              and get(ms, "sonic_portchannel_members", portchannel="PortChannel0001") == 2
              and get(ms, "sonic_portchannel_members_selected", portchannel="PortChannel0001") == 1)
        check("member selected/enabled with alias", get(ms, "sonic_portchannel_member_selected", port="Ethernet20",
                                                        alias="Ethernet6/1") == 1
              and get(ms, "sonic_portchannel_member_selected", port="Ethernet24") == 0
              and get(ms, "sonic_portchannel_member_enabled", port="Ethernet24") == 0)
        # bfd
        check("BFD default/default/10.0.0.1 Up (3)", get(ms, "sonic_bfd_session_state", vrf="default", interface="default", peer="10.0.0.1") == 3
              and get(ms, "sonic_bfd_session_up", peer="10.0.0.1") == 1)
        check("BFD Ethernet0 session Down (1), alias label", get(ms, "sonic_bfd_session_state", interface="Ethernet0", alias="Ethernet1/1") == 1)
        check("BFD Vrf-test Admin_Down (0)", get(ms, "sonic_bfd_session_state", vrf="Vrf-test", peer="10.0.0.0") == 0)
        check("BFD timers in seconds", get(ms, "sonic_bfd_session_tx_interval_seconds", peer="10.0.0.1") == 0.3
              and get(ms, "sonic_bfd_session_detect_multiplier", peer="10.0.0.1") == 3)
        # bgp
        check("BGP neighbors from CONFIG_DB (3, both VRFs)", count(ms, "sonic_bgp_neighbor_info") == 3
              and get(ms, "sonic_bgp_neighbor_info", vrf="Vrf-test", neighbor="10.0.0.0", name="to-default", asn="65100") == 1)
        check("BGP 10.0.0.1 Established (6) with its name", get(ms, "sonic_bgp_neighbor_state", neighbor="10.0.0.1", name="to-vrf-test") == 6
              and get(ms, "sonic_bgp_neighbor_established", neighbor="10.0.0.1") == 1)
        check("BGP 'Idle (Admin)' -> 1, admin down", get(ms, "sonic_bgp_neighbor_state", neighbor="fc00::2") == 1
              and get(ms, "sonic_bgp_neighbor_admin_up", neighbor="fc00::2") == 0)
        check("no state for the Vrf-test neighbor (bgpmon: default VRF only)", get(ms, "sonic_bgp_neighbor_state", vrf="Vrf-test") == "0 matches")
        # lldp
        check("LLDP neighbors per port", count(ms, "sonic_lldp_neighbors") == nports
              and get(ms, "sonic_lldp_neighbors", port="Ethernet0") == 1 and get(ms, "sonic_lldp_neighbors", port="Ethernet4") == 0)
        check("LLDP remote of Ethernet1/1 = Ethernet32/1", get(ms, "sonic_lldp_neighbor_info", alias="Ethernet1/1", remote_port="Ethernet32/1",
                                                                remote_system="lab-switch") == 1)
        check("LLDP mgmt port listed, LLDP_LOC_CHASSIS not", get(ms, "sonic_lldp_neighbor_info", port="eth0") == 1
              and count(ms, "sonic_lldp_neighbor_info") == 3)
        # transceivers
        check("transceiver presence for every port, 3 present", count(ms, "sonic_transceiver_present") == nports
              and sum(v for (n, _), v in ms.items() if n == "sonic_transceiver_present") == 3)
        check("SR4 info + DOM", get(ms, "sonic_transceiver_info", port="Ethernet124", model="FAKE-QSFP28-SR4", vendor="FAKE-VENDOR") == 1
              and get(ms, "sonic_transceiver_temperature_celsius", port="Ethernet124") == 30.5
              and get(ms, "sonic_transceiver_voltage_volts", port="Ethernet124") == 3.29)
        check("DAC: N/A DOM gives no sample", get(ms, "sonic_transceiver_temperature_celsius", port="Ethernet0") == "0 matches")
        check("TRANSCEIVER_DOM_TEMPERATURE fallback", get(ms, "sonic_transceiver_temperature_celsius", port="Ethernet8") == 41.25)
        # acl / copp
        check("ACL rules: info, active", get(ms, "sonic_acl_rule_info", table="DROPTEST", rule="R1", action="DROP") == 1
              and get(ms, "sonic_acl_rule_active", rule="R1") == 1 and get(ms, "sonic_acl_rule_active", rule="R2") == 0)
        check("ACL counter R1: 40 packets, 4080 bytes", get(ms, "sonic_acl_rule_packets_total", table="DROPTEST", rule="R1") == 40
              and get(ms, "sonic_acl_rule_bytes_total", rule="R1") == 4080 and count(ms, "sonic_acl_rule_packets_total") == 1)
        check("CoPP groups: cir, traps", get(ms, "sonic_copp_group_cir", group="queue4_group2") == 600
              and get(ms, "sonic_copp_group_traps", group="queue4_group1") == 3
              and get(ms, "sonic_copp_trap_info", trap="arp_req", group="queue4_group2") == 1)
        check("CoPP trap counter", get(ms, "sonic_copp_trap_packets_total", trap="bgp") == 99)
        # routes
        check("APPL routes by VRF/family", get(ms, "sonic_routes", vrf="default", family="ipv4") == 3
              and get(ms, "sonic_routes", vrf="default", family="ipv6") == 1
              and get(ms, "sonic_routes", vrf="Vrf-test", family="ipv4") == 2 and get(ms, "sonic_routes", vrf="Vrf-test", family="ipv6") == 1)
        check("ASIC routes by family", get(ms, "sonic_asic_routes", family="ipv4") == 2 and get(ms, "sonic_asic_routes", family="ipv6") == 1)
        # conflicts / services
        check("conflicts 2 (by table), notes 1", get(ms, "sonic_nix_conflicts") == 2 and get(ms, "sonic_nix_conflicts_by_table", table="PORT") == 1
              and get(ms, "sonic_nix_notes") == 1 and get(ms, "sonic_nix_conflicts_updated_timestamp_seconds") == 1794790000)
        now = time.time()
        st = get(ms, "sonic_service_start_timestamp_seconds", unit="sonic-orchagent.service")
        check("services: active, restarts, start time", get(ms, "sonic_service_active", unit="sonic-bgpmon.service") == 0
              and get(ms, "sonic_service_restarts", unit="sonic-orchagent.service") == 2
              and isinstance(st, float) and abs(st - (now - 10)) < 5
              and get(ms, "sonic_service_start_timestamp_seconds", unit="sonic-bgpmon.service") == "0 matches", st)

        # ------------------------------------------------ 2. redis gone
        p2, _ = run("--db-config", os.path.join(tmp, "missing.json"), "--socket", os.path.join(tmp, "nope.sock"),
                    "--host", "127.0.0.1", "--port", "1", "--conflicts", conflicts, "--systemctl", systemctl)
        ms2 = parse(p2.stdout)
        check("redis missing: exit 0, sonic_up 0, no table families", p2.returncode == 0 and get(ms2, "sonic_up") == 0
              and count(ms2, "sonic_port_info") == 0, p2.stdout + p2.stderr)
        check("redis missing: conflicts + services still there", get(ms2, "sonic_nix_conflicts") == 2
              and get(ms2, "sonic_service_active", unit="sonic-syncd.service") == 1)
        check("redis missing: says why on stderr", "redis" in p2.stderr, p2.stderr)
        with open(os.path.join(out, "metrics-no-redis.prom"), "w") as f:
            f.write(p2.stdout)

        # ------------------------------------------------ 3. one broken table, missing sources
        r.pipeline([["SELECT", STATE], ["DEL", "TRANSCEIVER_INFO|garbage"]])
        r.pipeline([["SELECT", COUNTERS], ["DEL", "COUNTERS_PORT_NAME_MAP"], ["SET", "COUNTERS_PORT_NAME_MAP", "oops"]])
        p3, _ = run("--db-config", dbcfg, "--conflicts", os.path.join(tmp, "none.json"), "--systemctl", os.path.join(tmp, "no-systemctl"))
        ms3 = parse(p3.stdout)
        check("broken COUNTERS_PORT_NAME_MAP: counters collector 0, others fine", p3.returncode == 0
              and get(ms3, "sonic_collector_success", collector="counters") == 0
              and get(ms3, "sonic_collector_success", collector="ports") == 1 and count(ms3, "sonic_port_info") == nports
              and count(ms3, "sonic_port_rx_bytes_total") == 0, p3.stderr)
        check("no conflicts file / systemctl: their collectors 0", get(ms3, "sonic_collector_success", collector="conflicts") == 0
              and get(ms3, "sonic_collector_success", collector="services") == 0)
        with open(os.path.join(out, "metrics-degraded.prom"), "w") as f:
            f.write(p3.stdout)

        # ------------------------------------------------ 4. speed
        times = []
        for _ in range(5):
            p4, dt = run("--db-config", dbcfg, "--conflicts", conflicts, "--systemctl", "")
            times.append((dt, get(parse(p4.stdout), "sonic_collect_duration_seconds")))
        best = min(times)
        print("INFO best of 5: %.3f s wall, %.4f s collect (%d ports)" % (best[0], best[1], nports))
        check("collect < 0.25 s on the build host (the e6500 is ~10x slower; budget 2 s)", best[1] < 0.25, times)
        with open(os.path.join(out, "result.txt"), "w") as f:
            f.write("%d ports, %d samples, best wall %.3f s, collect %.4f s, failed: %s\n" % (nports, len(ms), best[0], best[1], FAILS))
        r.close()
    finally:
        srv.terminate()
        srv.wait()
    print("SUMMARY: %d failed: %s" % (len(FAILS), FAILS))
    return 1 if FAILS else 0


if __name__ == "__main__":
    sys.exit(main())
