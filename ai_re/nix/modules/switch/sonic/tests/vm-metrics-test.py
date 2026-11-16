import json, re, time

FAILS = []
def check(name, cond, detail=""):
    d = str(detail)
    print(("PASS " if cond else "FAIL ") + name + ("" if cond else " :: " + (d if len(d) < 3000 else d[:800] + " ... " + d[-2000:])))
    if not cond:
        FAILS.append(name)

def sh(cmd, m=None):
    return (m or switch).execute(cmd)

def cli(cmd):
    return sh("bash -lc " + json.dumps(cmd + " 2>&1"))

def db(n, cmd):
    return sh("redis-cli -s /var/run/redis/redis.sock -n %d %s" % (n, cmd))[1].strip()

def wait(name, cmd, timeout=120, detail_cmd=None):
    try:
        switch.wait_until_succeeds(cmd, timeout=timeout)
        check(name, True)
        return True
    except Exception:
        check(name, False, sh(detail_cmd)[1] if detail_cmd else "")
        return False

def parse(text):
    out = {}
    for line in text.splitlines():
        if not line or line.startswith("#"):
            continue
        m = re.match(r'^([a-zA-Z_:]+)(?:\{(.*)\})? (\S+)$', line)
        if not m:
            continue
        labels = frozenset(re.findall(r'([a-zA-Z_]+)="((?:[^"\\]|\\.)*)"', m.group(2) or ""))
        out[(m.group(1), labels)] = float(m.group(3))
    return out

def get(ms, metric, **labels):
    hits = [v for (n, ls), v in ms.items() if n == metric and set(labels.items()) <= ls]
    return hits[0] if len(hits) == 1 else None

def series(ms, metric):
    return [(dict(ls), v) for (n, ls), v in ms.items() if n == metric]

def produce():
    rc, out = sh("systemctl start prometheus-textfile-sonic.service")
    res = sh("systemctl show -p Result --value prometheus-textfile-sonic.service")[1].strip()
    txt = sh("cat /run/prometheus-textfile/sonic.prom")[1]
    return rc, res, txt

start_all()
switch.wait_for_unit("sonic-orchagent.service", timeout=300)
switch.wait_until_succeeds("redis-cli -s /var/run/redis/redis.sock -n 0 exists PORT_TABLE:PortInitDone | grep -q 1", timeout=240)
for u in ["sonic-bgpcfgd", "sonic-bgpmon", "sonic-bfdsyncd", "sonic-lldp-syncd", "sonic-xcvrd", "prometheus-node-exporter"]:
    switch.wait_for_unit(u + ".service", timeout=180)
check("producer timer active", sh("systemctl is-active prometheus-textfile-sonic.timer")[1].strip() == "active")

for c in ["config portchannel add PortChannel0001", "config portchannel member add PortChannel0001 Ethernet5/1",
          "config portchannel member add PortChannel0001 Ethernet6/1", "config interface startup Ethernet5/1",
          "config interface startup Ethernet6/1", "config interface ip add PortChannel0001 10.0.0.1/24",
          "config vrf add Vrf-test", "config interface vrf bind Ethernet7/1 Vrf-test", "config interface startup Ethernet7/1",
          "config interface ip add Ethernet7/1 10.1.0.1/24",
          "config loopback add Loopback0", "config interface ip add Loopback0 10.255.0.1/32"]:
    rc, out = cli(c)
    check(c, rc == 0, out)
for k, a in [("BGP_NEIGHBOR|10.0.0.2", "name peer-pc local_addr 10.0.0.1 bfd true"), ("BGP_NEIGHBOR|Vrf-test|10.1.0.2", "name peer-vrf local_addr 10.1.0.1")]:
    rc, out = sh("sonic-db-cli CONFIG_DB hset '%s' asn 65200 %s admin_status up" % (k, a))
    check("CONFIG_DB " + k, rc == 0, out)
db(4, "hset 'ACL_TABLE|DROPTEST' type L3 stage ingress policy_desc dropper ports@ Ethernet24")
db(4, "hset 'ACL_RULE|DROPTEST|R1' PRIORITY 9999 PACKET_ACTION DROP DST_IP 10.9.9.9/32")
rc, out = cli("sudo counterpoll acl enable")
print("INFO counterpoll acl enable rc=%d %s" % (rc, out[-200:]))

wait("BGP 10.0.0.2 Established (bgpmon: STATE_DB NEIGH_STATE_TABLE)",
     "redis-cli -s /var/run/redis/redis.sock -n 6 hget 'NEIGH_STATE_TABLE|10.0.0.2' state | grep -qx Established", 180,
     "vtysh -c 'show bgp summary'")
wait("BFD Up in STATE_DB", "redis-cli -s /var/run/redis/redis.sock -n 6 hget 'BFD_SESSION_TABLE|default|PortChannel0001|10.0.0.2' state | grep -qx Up", 90,
     "redis-cli -s /var/run/redis/redis.sock -n 6 keys 'BFD*'")
wait("BGP Vrf-test 10.1.0.2 Established (FRR)",
     "vtysh -c 'show bgp vrf Vrf-test neighbors 10.1.0.2 json' | grep -q '\"bgpState\":\"Established\"'", 120)
wait("LACP: both members selected", "test $(redis-cli -s /var/run/redis/redis.sock -n 6 --scan --pattern 'LAG_MEMBER_TABLE|PortChannel0001|*' | "
     "xargs -I{} redis-cli -s /var/run/redis/redis.sock -n 6 hget {} runner.aggregator.selected | grep -c true) = 2", 120)
wait("LLDP: peer seen on Ethernet16", "redis-cli -s /var/run/redis/redis.sock -n 0 exists LLDP_ENTRY_TABLE:Ethernet16 | grep -qx 1", 180)
wait("xcvrd: TRANSCEIVER_DOM_SENSOR Ethernet124",
     "redis-cli -s /var/run/redis/redis.sock -n 6 hget 'TRANSCEIVER_DOM_SENSOR|Ethernet124' temperature | grep -q 30.5", 180)
wait("ACL rule Active", "redis-cli -s /var/run/redis/redis.sock -n 6 hget 'ACL_RULE_TABLE|DROPTEST|R1' status | grep -qx Active", 60)
# a runtime change to an enforced key -> sonic-nix-watch -> conflicts.json
sh("sonic-db-cli CONFIG_DB hset 'DEVICE_METADATA|localhost' hostname not-the-nix-one")
wait("conflicts.json lists the hostname", "grep -q not-the-nix-one /run/sonic-nix/conflicts.json", 60)
# an automatic restart
sh("systemctl kill -s KILL sonic-lldp-syncd.service")
wait("lldp-syncd restarted (NRestarts 1)", "test $(systemctl show -p NRestarts --value sonic-lldp-syncd.service) -ge 1", 30)
switch.wait_for_unit("sonic-lldp-syncd.service")
time.sleep(15)   # flex counters (1 s / 10 s polls), LLDP re-sync

rc, res, txt = produce()
check("producer run succeeds", rc == 0 and res == "success", (rc, res))
ms = parse(txt)
print("INFO sonic.prom: %d samples, %d bytes" % (len(ms), len(txt)))
print("\n".join(l for l in txt.splitlines() if re.match(r"^sonic_(up|collector|collect_duration|routes|asic_routes|bgp_neighbor_state|bfd_session_state|nix_conflicts |service_restarts\{unit=\"sonic-lldp)", l)))
rc, out = sh("promtool check metrics < /run/prometheus-textfile/sonic.prom")
check("promtool check metrics (strict lint)", rc == 0, out)
check("sonic_up 1", get(ms, "sonic_up") == 1)
bad = [(l, v) for l, v in series(ms, "sonic_collector_success") if v != 1]
check("all 13 collectors succeeded", len(series(ms, "sonic_collector_success")) == 13 and not bad, bad)
nports = int(sh("redis-cli -s /var/run/redis/redis.sock -n 4 --scan --pattern 'PORT|*' | wc -l")[1].strip())
check("one sonic_port_info per CONFIG_DB port (%d)" % nports, len(series(ms, "sonic_port_info")) == nports, len(series(ms, "sonic_port_info")))
check("Ethernet16 / Ethernet5/1 admin + oper up", get(ms, "sonic_port_oper_up", port="Ethernet16", alias="Ethernet5/1") == 1
      and get(ms, "sonic_port_admin_up", port="Ethernet16") == 1)
check("an admin-down port is oper down", get(ms, "sonic_port_admin_up", port="Ethernet8") == 0 and get(ms, "sonic_port_oper_up", port="Ethernet8") == 0)
check("counters: Ethernet16 rx/tx packets > 0", (get(ms, "sonic_port_rx_packets_total", port="Ethernet16") or 0) > 0
      and (get(ms, "sonic_port_tx_packets_total", port="Ethernet16") or 0) > 0, series(ms, "sonic_port_rx_packets_total")[:4])
check("PortChannel0001 oper up, 2 members selected + enabled",
      get(ms, "sonic_portchannel_oper_up", portchannel="PortChannel0001") == 1
      and get(ms, "sonic_portchannel_members_selected", portchannel="PortChannel0001") == 2
      and get(ms, "sonic_portchannel_member_enabled", port="Ethernet20", alias="Ethernet6/1") == 1, series(ms, "sonic_portchannel_member_selected"))
check("BFD default/PortChannel0001/10.0.0.2 Up (3)",
      get(ms, "sonic_bfd_session_state", vrf="default", interface="PortChannel0001", peer="10.0.0.2") == 3, series(ms, "sonic_bfd_session_state"))
check("BGP 10.0.0.2 Established (6), name peer-pc", get(ms, "sonic_bgp_neighbor_state", neighbor="10.0.0.2", name="peer-pc") == 6
      and get(ms, "sonic_bgp_neighbor_established", neighbor="10.0.0.2") == 1, series(ms, "sonic_bgp_neighbor_state"))
check("BGP Vrf-test neighbor: configured, no state (bgpmon: default VRF only)",
      get(ms, "sonic_bgp_neighbor_info", vrf="Vrf-test", neighbor="10.1.0.2", name="peer-vrf") == 1
      and get(ms, "sonic_bgp_neighbor_state", vrf="Vrf-test") is None)
check("LLDP: Ethernet5/1 neighbor = the peer", get(ms, "sonic_lldp_neighbor_info", port="Ethernet16", alias="Ethernet5/1", remote_system="peer") == 1
      and get(ms, "sonic_lldp_neighbors", port="Ethernet16") == 1, series(ms, "sonic_lldp_neighbor_info"))
check("transceivers: Ethernet0 + Ethernet124 present, SR4 at 30.5 C",
      get(ms, "sonic_transceiver_present", port="Ethernet0") == 1 and get(ms, "sonic_transceiver_present", port="Ethernet124", alias="Ethernet32/1") == 1
      and get(ms, "sonic_transceiver_present", port="Ethernet8") == 0
      and get(ms, "sonic_transceiver_info", port="Ethernet124", model="FAKE-QSFP28-SR4") == 1
      and get(ms, "sonic_transceiver_temperature_celsius", port="Ethernet124") == 30.5, series(ms, "sonic_transceiver_info"))
check("ACL rule DROPTEST/R1: DROP, active", get(ms, "sonic_acl_rule_info", table="DROPTEST", rule="R1", action="DROP") == 1
      and get(ms, "sonic_acl_rule_active", table="DROPTEST", rule="R1") == 1)
print("INFO ACL counter (vs SAI): %s" % series(ms, "sonic_acl_rule_packets_total"))
print("INFO APPL_DB COPP_TABLE keys: %s" % db(0, "keys 'COPP_TABLE*'").replace("\n", " "))
print("INFO COPP_TABLE:queue4_group1: %s" % db(0, "hgetall COPP_TABLE:queue4_group1").replace("\n", " "))
print("INFO STATE_DB COPP: %s" % db(6, "keys 'COPP*'").replace("\n", " "))
print("INFO CONFIG_DB COPP: %s" % db(4, "keys 'COPP*'").replace("\n", " ")[:600])
print("INFO FEATURE: %s" % db(4, "keys 'FEATURE*'").replace("\n", " "))
# bgp/bgpv6 traps need CONFIG_DB FEATURE|bgp (coppmgrd), which services.sonic does not seed: reported, not checked.
check("CoPP: groups from coppmgrd, LACP trap in queue4_group1 (cir 6000)", len(series(ms, "sonic_copp_group_info")) >= 2
      and get(ms, "sonic_copp_trap_info", trap="lacp", group="queue4_group1") == 1
      and get(ms, "sonic_copp_group_cir", group="queue4_group1") == 6000, series(ms, "sonic_copp_trap_info"))
print("INFO CoPP traps mapped: %s" % sorted(l["trap"] for l, _ in series(ms, "sonic_copp_trap_info")))
appl = int(sh("redis-cli -s /var/run/redis/redis.sock -n 0 --scan --pattern 'ROUTE_TABLE:*' | grep -vc '^ROUTE_TABLE:Vrf'")[1].strip() or 0)
check("routes: default VRF = APPL_DB ROUTE_TABLE count (%d), 192.0.2.0/24 learned" % appl,
      sum(v for l, v in series(ms, "sonic_routes") if l["vrf"] == "default") == appl and appl >= 3
      and db(0, "exists ROUTE_TABLE:192.0.2.0/24") == "1", series(ms, "sonic_routes"))
check("routes: Vrf-test >= 1, ASIC ipv4 >= 3", (get(ms, "sonic_routes", vrf="Vrf-test", family="ipv4") or 0) >= 1
      and (get(ms, "sonic_asic_routes", family="ipv4") or 0) >= 3, (series(ms, "sonic_routes"), series(ms, "sonic_asic_routes")))
check("conflicts: >= 1, DEVICE_METADATA", (get(ms, "sonic_nix_conflicts") or 0) >= 1
      and (get(ms, "sonic_nix_conflicts_by_table", table="DEVICE_METADATA") or 0) >= 1, series(ms, "sonic_nix_conflicts_by_table"))
check("services: lldp-syncd restarts >= 1, orchagent active, start times",
      (get(ms, "sonic_service_restarts", unit="sonic-lldp-syncd.service") or 0) >= 1
      and get(ms, "sonic_service_active", unit="sonic-orchagent.service") == 1
      and get(ms, "sonic_service_active", unit="redis-sonic.service") == 1
      and (get(ms, "sonic_service_start_timestamp_seconds", unit="sonic-syncd.service") or 0) > 1.7e9,
      [(l, v) for l, v in series(ms, "sonic_service_restarts") if v])
dur = get(ms, "sonic_collect_duration_seconds")
check("collect duration < 1 s in the VM", dur is not None and dur < 1, dur)

# the same through the exporter
rc, out = sh("curl -s localhost:9100/metrics")
em = parse(out)
pd = get(em, "textfile_producer_duration_seconds", producer="sonic")
print("INFO exporter: %d sonic_ samples, producer run %.3f s (collect %.3f s)" % (sum(1 for (n, _) in em if n.startswith("sonic_")), pd or -1, dur or -1))
check("exporter serves sonic_* (sonic_up 1, BGP state)", get(em, "sonic_up") == 1
      and get(em, "sonic_bgp_neighbor_state", neighbor="10.0.0.2") == 6 and pd is not None and pd < 3, pd)

# restore, then redis gone
sh("sonic-db-cli CONFIG_DB hset 'DEVICE_METADATA|localhost' hostname switch")
wait("conflict cleared", "! grep -q not-the-nix-one /run/sonic-nix/conflicts.json", 60)
rc, res, txt = produce()
check("after the restore: sonic_nix_conflicts 0", get(parse(txt), "sonic_nix_conflicts") == 0, txt[-500:])
sh("systemctl stop sonic.target redis-sonic.service")
rc, res, txt = produce()
m2 = parse(txt)
check("redis stopped: run succeeds, sonic_up 0, services still reported", rc == 0 and res == "success" and get(m2, "sonic_up") == 0
      and get(m2, "sonic_service_active", unit="redis-sonic.service") == 0 and not series(m2, "sonic_port_info"), (rc, res, txt[-800:]))
print(sh("journalctl -b -u prometheus-textfile-sonic.service --no-pager | tail -5")[1])

print("SUMMARY: %d failed: %s" % (len(FAILS), FAILS))
assert not FAILS, FAILS
