import json, re, time

FAILS = []
def check(name, cond, detail=""):
    d = str(detail)
    print(("PASS " if cond else "FAIL ") + name + ("" if cond else " :: " + (d if len(d) < 2500 else d[:500] + " ... " + d[-2000:])))
    if not cond:
        FAILS.append(name)

def sh(cmd):
    return switch.execute(cmd)

def cli(cmd):
    return sh("bash -lc " + json.dumps(cmd + " 2>&1"))   # alias naming mode

def db(n, cmd):
    return sh("redis-cli -s /var/run/redis/redis.sock -n %d %s" % (n, cmd))[1].strip()

def asic_keys(t):
    return [k for k in db(1, "--scan --pattern 'ASIC_STATE:SAI_OBJECT_TYPE_%s:*'" % t).split("\n") if k]

def asic(key, field):
    return sh("redis-cli -s /var/run/redis/redis.sock -n 1 hget %s %s" % (json.dumps(key), field))[1].strip()

def routes():
    out = {}
    for k in asic_keys("ROUTE_ENTRY"):
        e = json.loads(k.split("ROUTE_ENTRY:", 1)[1])
        out[(e["vr"], e["dest"])] = k
    return out

def wait(desc, f, timeout=60):
    for _ in range(timeout * 2):
        v = f()
        if v:
            return v
        time.sleep(0.5)
    check(desc + " (timed out)", False)
    return None

def swssconfig(entries):
    switch.succeed("cat > /tmp/c.json << 'EOF'\n" + json.dumps(entries) + "\nEOF")
    rc, out = sh("swssconfig /tmp/c.json 2>&1")
    check("swssconfig " + ",".join(next(iter(e)) for e in entries), rc == 0, out)

UNITS = ["sonic-syncd", "sonic-orchagent", "sonic-intfmgrd", "sonic-vrfmgrd", "sonic-neighsyncd", "sonic-portmgrd"]
def restarts():
    return {u: sh("systemctl show -p ActiveState -p NRestarts --value %s.service | tr '\\n' ' '" % u)[1].strip() for u in UNITS}

start_all()
switch.wait_for_unit("sonic.target", timeout=300)
switch.wait_for_unit("sonic-orchagent.service", timeout=300)
switch.wait_until_succeeds("redis-cli -s /var/run/redis/redis.sock -n 0 exists PORT_TABLE:PortInitDone | grep -q 1", timeout=240)
switch.wait_until_succeeds("ip link show Ethernet124", timeout=120)
time.sleep(10)
rep = restarts()
check("units active, no restarts (vrfmgrd included)", all(s == "active 0" for s in rep.values()), rep)

# ---------------------------------------------------------------- capacities
log = sh("journalctl -b --no-pager -o cat -u sonic-orchagent")[1]
check("RouteOrch: ECMP groups from the SAI", "Maximum number of ECMP groups supported is 4096" in log,
      [l for l in log.split("\n") if "ECMP" in l])
check("RouteOrch: ECMP group size from the SAI", "Max ECMP Group Size supported: 16384" in log,
      [l for l in log.split("\n") if "ECMP" in l])

# ---------------------------------------------------------------- flex counters
check("CONFIG_DB FLEX_COUNTER_TABLE|PORT seeded", db(4, "hget 'FLEX_COUNTER_TABLE|PORT' FLEX_COUNTER_STATUS") == "enable")
oid8 = wait("COUNTERS_PORT_NAME_MAP Ethernet8", lambda: db(2, "hget COUNTERS_PORT_NAME_MAP Ethernet8"), 120)
if oid8:
    v = wait("COUNTERS port stats polled", lambda: db(2, "hget COUNTERS:%s SAI_PORT_STAT_IF_IN_OCTETS" % oid8), 60)
    check("syncd polls SAI_PORT_STAT_IF_IN_OCTETS for Ethernet8", v is not None and v.isdigit(), v)
rc, out = cli("show interfaces counters -i Ethernet3/1,Ethernet4/1")
print(out)
check("show interfaces counters -i <alias>,<alias>", rc == 0 and "cannot find" not in out and re.search(r"Ethernet(3/1|8)\b", out) and re.search(r"Ethernet(4/1|12)\b", out), out)
rc, out = cli("show interfaces counters -i Ethernet3/1-4/1")
check("show interfaces counters -i <alias range>", rc == 0 and "cannot find" not in out and re.search(r"Ethernet(4/1|12)\b", out), out)
rc, out = cli("show interfaces counters -i Ethernet3/1")
check("show interfaces counters -i <alias>", rc == 0 and re.search(r"Ethernet(3/1|8)\b", out), out)
rc, out = cli("show interfaces counters -i Ethernet99/1,Ethernet3/1")
check("unknown alias in a list still fails", rc != 0 and "cannot find interface name for alias Ethernet99/1" in out, out)

# ---------------------------------------------------------------- VRF
default_vr = asic_keys("VIRTUAL_ROUTER")
check("one virtual router before", len(default_vr) == 1, default_vr)
default_vr = default_vr[0].split(":", 2)[2] if default_vr else ""
rc, out = cli("config vrf add Vrf-test")
check("config vrf add Vrf-test", rc == 0, out)
wait("kernel VRF device", lambda: sh("ip -d link show Vrf-test | grep -q 'vrf table'")[0] == 0)
wait("APPL_DB VRF_TABLE:Vrf-test", lambda: db(0, "exists VRF_TABLE:Vrf-test") == "1")
wait("STATE_DB VRF_OBJECT_TABLE ok", lambda: db(6, "hget 'VRF_OBJECT_TABLE|Vrf-test' state") == "ok")
vrs = asic_keys("VIRTUAL_ROUTER")
check("ASIC_DB: second virtual router", len(vrs) == 2, vrs)
vr = [k.split(":", 2)[2] for k in vrs if k.split(":", 2)[2] != default_vr]
vr = vr[0] if vr else ""
rc, out = cli("config interface vrf bind Ethernet32/1 Vrf-test")
check("config interface vrf bind Ethernet32/1 Vrf-test", rc == 0, out)
for alias, ip in [("Ethernet32/1", "10.0.0.1/31"), ("Ethernet1/1", "10.0.0.0/31")]:
    rc, out = cli("config interface ip add %s %s" % (alias, ip))
    check("config interface ip add %s %s" % (alias, ip), rc == 0, out)
wait("Ethernet124 enslaved to Vrf-test", lambda: sh("ip link show Ethernet124 | grep -q 'master Vrf-test'")[0] == 0)
wait("Ethernet124 has 10.0.0.1/31", lambda: sh("ip -4 addr show Ethernet124 | grep -q 10.0.0.1/31")[0] == 0)
def rif_vr(vrid):
    return [k for k in asic_keys("ROUTER_INTERFACE")
            if asic(k, "SAI_ROUTER_INTERFACE_ATTR_VIRTUAL_ROUTER_ID") == vrid
            and asic(k, "SAI_ROUTER_INTERFACE_ATTR_TYPE") == "SAI_ROUTER_INTERFACE_TYPE_PORT"]
r = wait("ASIC_DB: port RIF in Vrf-test", lambda: rif_vr(vr))
check("exactly one port RIF in Vrf-test", r and len(r) == 1, r)
rt = wait("ASIC_DB: IP2ME 10.0.0.1/32 in Vrf-test", lambda: (vr, "10.0.0.1/32") in routes() and routes())
# (no subnet routes: those come from zebra via fpmsyncd, not intfsorch; FRR is off here)
check("IP2ME 10.0.0.0/32 in the default VR", rt and (default_vr, "10.0.0.0/32") in rt, rt and list(rt))
check("no 10.0.0.1/32 in the default VR", rt and (default_vr, "10.0.0.1/32") not in rt)
# A static route in the VRF without FRR: neighbor (kernel, as ARP would) + APPL_DB route.
switch.succeed("ip neigh replace 10.0.0.0 lladdr 52:54:00:aa:bb:01 dev Ethernet124 nud permanent")
wait("ASIC_DB: neighbor 10.0.0.0 on the Vrf-test RIF", lambda: any('"ip":"10.0.0.0"' in k for k in asic_keys("NEIGHBOR_ENTRY")))
swssconfig([{"ROUTE_TABLE:Vrf-test:198.51.100.0/24": {"nexthop": "10.0.0.0", "ifname": "Ethernet124"}, "OP": "SET"}])
k = wait("ASIC_DB: 198.51.100.0/24 in Vrf-test", lambda: routes().get((vr, "198.51.100.0/24")))
if k:
    nh = asic(k, "SAI_ROUTE_ENTRY_ATTR_NEXT_HOP_ID")
    check("its next hop is 10.0.0.0", asic("ASIC_STATE:SAI_OBJECT_TYPE_NEXT_HOP:" + nh, "SAI_NEXT_HOP_ATTR_IP") == "10.0.0.0", nh)
check("198.51.100.0/24 not in the default VR", (default_vr, "198.51.100.0/24") not in routes())

# ---------------------------------------------------------------- ECMP
for alias, ip in [("Ethernet3/1", "10.1.1.0/31"), ("Ethernet4/1", "10.1.2.0/31")]:
    rc, out = cli("config interface ip add %s %s" % (alias, ip))
    check("config interface ip add %s %s" % (alias, ip), rc == 0, out)
wait("Ethernet8/12 addresses", lambda: sh("ip -4 addr show Ethernet8 | grep -q 10.1.1.0 && ip -4 addr show Ethernet12 | grep -q 10.1.2.0")[0] == 0)
switch.succeed("ip neigh replace 10.1.1.1 lladdr 02:00:00:00:01:01 dev Ethernet8 nud permanent")
switch.succeed("ip neigh replace 10.1.2.1 lladdr 02:00:00:00:02:01 dev Ethernet12 nud permanent")
wait("ASIC_DB: both neighbors", lambda: sum(1 for k in asic_keys("NEIGHBOR_ENTRY") if '"ip":"10.1.1.1"' in k or '"ip":"10.1.2.1"' in k) == 2)
swssconfig([{"ROUTE_TABLE:192.0.2.0/24": {"nexthop": "10.1.1.1,10.1.2.1", "ifname": "Ethernet8,Ethernet12"}, "OP": "SET"}])
k = wait("ASIC_DB: 192.0.2.0/24", lambda: routes().get((default_vr, "192.0.2.0/24")))
groups = asic_keys("NEXT_HOP_GROUP")
members = asic_keys("NEXT_HOP_GROUP_MEMBER")
check("RouteOrch created a next-hop group", len(groups) == 1, groups)
check("with two members", len(members) == 2, members)
if k and groups:
    gid = groups[0].split(":", 2)[2]
    check("192.0.2.0/24 points at the group", asic(k, "SAI_ROUTE_ENTRY_ATTR_NEXT_HOP_ID") == gid)
    check("members belong to it", all(asic(m, "SAI_NEXT_HOP_GROUP_MEMBER_ATTR_NEXT_HOP_GROUP_ID") == gid for m in members))
# down to one next hop: the route moves to it, the group goes
swssconfig([{"ROUTE_TABLE:192.0.2.0/24": {"nexthop": "10.1.1.1", "ifname": "Ethernet8"}, "OP": "SET"}])
wait("group removed", lambda: not asic_keys("NEXT_HOP_GROUP") and not asic_keys("NEXT_HOP_GROUP_MEMBER"))
k = routes().get((default_vr, "192.0.2.0/24"))
check("route on a single next hop", k and asic("ASIC_STATE:SAI_OBJECT_TYPE_NEXT_HOP:" + asic(k, "SAI_ROUTE_ENTRY_ATTR_NEXT_HOP_ID"), "SAI_NEXT_HOP_ATTR_IP") == "10.1.1.1")
swssconfig([{"ROUTE_TABLE:192.0.2.0/24": {"nexthop": "10.1.1.1,10.1.2.1", "ifname": "Ethernet8,Ethernet12"}, "OP": "SET"}])
wait("group back", lambda: len(asic_keys("NEXT_HOP_GROUP_MEMBER")) == 2)
swssconfig([{"ROUTE_TABLE:192.0.2.0/24": {}, "OP": "DEL"}])
wait("route and group removed", lambda: (default_vr, "192.0.2.0/24") not in routes() and not asic_keys("NEXT_HOP_GROUP"))

# ---------------------------------------------------------------- VRF teardown
swssconfig([{"ROUTE_TABLE:Vrf-test:198.51.100.0/24": {}, "OP": "DEL"}])
wait("VRF route removed", lambda: (vr, "198.51.100.0/24") not in routes())
switch.succeed("ip neigh del 10.0.0.0 dev Ethernet124")
rc, out = cli("config interface vrf unbind Ethernet32/1")
check("config interface vrf unbind", rc == 0, out)
wait("Ethernet124 released", lambda: sh("ip link show Ethernet124 | grep -q master")[0] != 0)
rc, out = cli("config vrf del Vrf-test")
check("config vrf del Vrf-test", rc == 0, out)
wait("virtual router removed", lambda: len(asic_keys("VIRTUAL_ROUTER")) == 1, 90)
wait("kernel VRF device removed", lambda: sh("ip link show Vrf-test")[0] != 0)

err = sh("journalctl -b --no-pager -o cat -u sonic-orchagent -u sonic-syncd | grep -iE 'Failed to create (next hop group|virtual router)|NOT_IMPLEMENTED.*(NEXT_HOP_GROUP|VIRTUAL_ROUTER)' || true")[1].strip()
check("no VR/NHG create failures in the logs", err == "", err)
rep = restarts()
check("units active, no restarts at the end", all(s == "active 0" for s in rep.values()), rep)
print("SUMMARY: %d failures: %s" % (len(FAILS), FAILS))
assert not FAILS, FAILS
