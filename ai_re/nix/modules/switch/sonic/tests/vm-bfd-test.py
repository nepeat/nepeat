import json, time

FAILS = []
RESULTS = {}
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

def vtysh(cmd, m=None):
    return sh("vtysh -c " + json.dumps(cmd), m)[1]

def wait(name, cmd, timeout=90, m=None, detail_cmd=None):
    try:
        (m or switch).wait_until_succeeds(cmd, timeout=timeout)
        check(name, True)
        return True
    except Exception:
        check(name, False, sh(detail_cmd, m)[1] if detail_cmd else "")
        return False

def bfd_peers(m=None):
    try:
        return json.loads(vtysh("show bfd peers json", m))
    except ValueError:
        return []

def bfd_peer(peer, vrf="default", m=None):
    for s in bfd_peers(m):
        if s.get("peer") == peer and s.get("vrf", "default") == vrf:
            return s
    return None

def wait_bfd(name, peer, vrf="default", status="up", timeout=60, m=None):
    """bfdd session (peer, vrf) reaches status ('up', 'down', None = gone)."""
    t0 = time.monotonic()
    while time.monotonic() - t0 < timeout:
        if (bfd_peer(peer, vrf, m) or {}).get("status") == status:
            check(name, True)
            return True
        time.sleep(0.5)
    check(name, False, vtysh("show bfd peers", m) + vtysh("show bfd peers counters", m) + journal(["sonic-bgpcfgd"], 20, grep="bfd")
          + "\n==== peer:\n" + vtysh("show bfd peers", peer) + sh("journalctl -b -u frr --no-pager | grep -i bfd | tail -20", peer)[1])
    return False

def hgetall(n, key):
    r = sh("redis-cli -s /var/run/redis/redis.sock -n %d --raw hgetall %s" % (n, json.dumps(key)))[1].split("\n")
    r = [x for x in r if x != ""]
    return dict(zip(r[0::2], r[1::2]))

def established(nbr, vrf=None):
    v = "vrf %s " % vrf if vrf else ""
    return "vtysh -c 'show bgp %sneighbors %s json' | grep -q '\"bgpState\":\"Established\"'" % (v, nbr)

def time_until(cmd, timeout, m=None, step=0.05):
    """Seconds (host clock) until cmd succeeds, from the moment of the call."""
    t0 = time.monotonic()
    while time.monotonic() - t0 < timeout:
        if sh(cmd, m)[0] == 0:
            return time.monotonic() - t0
        time.sleep(step)
    return None

UNITS = ["sonic-frr-config", "sonic-frr-mgmtd", "sonic-frr-zebra", "sonic-frr-staticd", "sonic-frr-bgpd",
         "sonic-frr-bfdd", "sonic-fpmsyncd", "sonic-bgpcfgd", "sonic-bgpmon", "sonic-bfdsyncd", "sonic-staticroutebfd"]

def units_report():
    out = {}
    for u in UNITS + ["sonic-orchagent", "sonic-syncd"]:
        out[u] = sh("systemctl show -p ActiveState -p SubState -p NRestarts --value %s.service | tr '\\n' ' '" % u)[1].strip()
    return out

def journal(units, n=60, grep=None):
    g = (" | grep -i -E %s" % json.dumps(grep)) if grep else ""
    return sh("journalctl -b --no-pager -o short-precise %s%s | tail -%d" % (" ".join("-u " + u for u in units), g, n))[1]

start_all()
switch.wait_for_unit("sonic.target", timeout=300)
switch.wait_for_unit("sonic-orchagent.service", timeout=300)
switch.wait_until_succeeds("redis-cli -s /var/run/redis/redis.sock -n 0 exists PORT_TABLE:PortInitDone | grep -q 1", timeout=240)

# ---------------------------------------------------------------- 1. units, seeds, SAI profile, firewall
try:
    for u in UNITS:
        switch.wait_for_unit(u + ".service", timeout=120)
except Exception:
    print(json.dumps(units_report(), indent=1))
    print(journal(["sonic-frr-bfdd", "sonic-bfdsyncd", "sonic-staticroutebfd", "sonic-bgpcfgd"], 120))
    raise
time.sleep(10)
rep = units_report()
print(json.dumps(rep, indent=1))
bad = {u: s for u, s in rep.items() if not s.startswith("active") or not s.endswith(" 0")}
check("FRR (incl. bfdd), bgpcfgd, bfdsyncd, staticroutebfd active, no restarts", not bad, bad)
check("vtysh show daemons lists bfdd", "bfdd" in vtysh("show daemons"), vtysh("show daemons"))
check("bfdd.conf is empty (no file-owned BFD config)", sh("test -f /etc/frr/bfdd.conf && ! test -s /etc/frr/bfdd.conf")[0] == 0)
check("seeded: SYSTEM_DEFAULTS|software_bfd status enabled", db(4, "hget 'SYSTEM_DEFAULTS|software_bfd' status") == "enabled")
rc, out = sh("cat /usr/share/sonic/device/*/*/sai.profile")
check("vs sai.profile: SAI_VS_BFD_OFFLOAD_SUPPORTED=false (no offload, as sai-openbcm)", "SAI_VS_BFD_OFFLOAD_SUPPORTED=false" in out, out)
wait("bgpcfgd started BfdMgr (software_bfd enabled)", "journalctl -b -u sonic-bgpcfgd --no-pager | grep -q 'software_bfd feature is enabled'", timeout=30,
     detail_cmd="journalctl -b -u sonic-bgpcfgd --no-pager | tail -30")
check("seeded: FEATURE|bgp state enabled", db(4, "hget 'FEATURE|bgp' state") == "enabled")
wait("coppmgrd: APPL_DB COPP_TABLE has the bgp,bgpv6 traps",
     "redis-cli -s /var/run/redis/redis.sock -n 0 --scan --pattern 'COPP_TABLE:*' | while read k; do redis-cli -s /var/run/redis/redis.sock -n 0 hget \"$k\" trap_ids; done | tr ',' '\\n' | grep -qx bgpv6",
     timeout=60, detail_cmd="redis-cli -s /var/run/redis/redis.sock -n 0 --scan --pattern 'COPP_TABLE:*'; journalctl -b -u sonic-coppmgrd --no-pager | tail -20")
wait("CoppOrch: ASIC_DB hostif traps BGP and BGPV6",
     "for t in SAI_HOSTIF_TRAP_TYPE_BGP SAI_HOSTIF_TRAP_TYPE_BGPV6; do redis-cli -s /var/run/redis/redis.sock -n 1 --scan --pattern 'ASIC_STATE:SAI_OBJECT_TYPE_HOSTIF_TRAP:*' | while read k; do redis-cli -s /var/run/redis/redis.sock -n 1 hget \"$k\" SAI_HOSTIF_TRAP_ATTR_TRAP_TYPE; done | grep -qx $t || exit 1; done",
     timeout=60, detail_cmd="journalctl -b -u sonic-orchagent --no-pager | grep -i copp | tail -20")
rc, out = sh("nft list ruleset")
check("firewall: BFD ports (3784, 3785, 4784) open except on eth0", "udp dport { 3784, 3785, 4784 } accept" in out and 'iifname != "eth0"' in out, out[-3000:])

# ---------------------------------------------------------------- 2. PortChannel (default VRF) + port in Vrf-test
for c in ["config portchannel add PortChannel0001", "config portchannel member add PortChannel0001 Ethernet5/1",
          "config portchannel member add PortChannel0001 Ethernet6/1", "config interface startup Ethernet5/1",
          "config interface startup Ethernet6/1", "config interface ip add PortChannel0001 10.0.0.1/24",
          "config vrf add Vrf-test", "config interface vrf bind Ethernet7/1 Vrf-test", "config interface startup Ethernet7/1",
          "config interface ip add Ethernet7/1 10.1.0.1/24",
          "config interface startup Ethernet8/1", "config interface ip add Ethernet8/1 10.2.0.1/24",
          "config interface vrf bind Ethernet9/1 Vrf-test", "config interface startup Ethernet9/1",
          "config interface ip add Ethernet9/1 10.3.0.1/24",
          "config loopback add Loopback0", "config interface ip add Loopback0 10.255.0.1/32"]:
    rc, out = cli(c)
    check(c, rc == 0, out)
peer.wait_for_unit("frr.service")
wait("ping peer over the PortChannel", "ping -c1 -W2 10.0.0.2", timeout=180, detail_cmd="ip addr; teamdctl PortChannel0001 state")
wait("ping peer over Ethernet7/1 in Vrf-test", "ip vrf exec Vrf-test ping -c1 -W2 10.1.0.2", timeout=120, detail_cmd="ip -br addr; ip vrf")
wait("ping peer over Ethernet8/1", "ping -c1 -W2 10.2.0.2", timeout=120, detail_cmd="ip -br addr")
wait("ping peer over Ethernet9/1 in Vrf-test", "ip vrf exec Vrf-test ping -c1 -W2 10.3.0.2", timeout=120, detail_cmd="ip -br addr; ip vrf")

# ---------------------------------------------------------------- 3. BGP neighbors with BFD from CONFIG_DB
for k, a in [("BGP_NEIGHBOR|10.0.0.2", "name peer-pc local_addr 10.0.0.1"), ("BGP_NEIGHBOR|Vrf-test|10.1.0.2", "name peer-port local_addr 10.1.0.1")]:
    rc, out = sh("sonic-db-cli CONFIG_DB hset '%s' asn 65200 %s holdtime 90 keepalive 30 admin_status up bfd true" % (k, a))
    check("CONFIG_DB %s bfd=true" % k, rc == 0, out)
wait("bgpcfgd: neighbor 10.0.0.2 bfd in bgpd", "vtysh -c 'show running-config' | grep -q '^ neighbor 10.0.0.2 bfd$'", timeout=60,
     detail_cmd="vtysh -c 'show running-config'; journalctl -b -u sonic-bgpcfgd --no-pager | tail -30")
wait("bgpcfgd: router bgp 65100 vrf Vrf-test with neighbor 10.1.0.2 bfd",
     "vtysh -c 'show running-config' | sed -n '/^router bgp 65100 vrf Vrf-test/,/^exit/p' | grep -q 'neighbor 10.1.0.2 bfd'", timeout=60,
     detail_cmd="vtysh -c 'show running-config'")
wait("BGP 10.0.0.2 (PortChannel) Established", established("10.0.0.2"), timeout=120, detail_cmd="vtysh -c 'show bgp summary'")
wait("BGP Vrf-test 10.1.0.2 (Ethernet7/1) Established", established("10.1.0.2", "Vrf-test"), timeout=120, detail_cmd="vtysh -c 'show bgp vrf Vrf-test summary'")
wait_bfd("bfdd: session to 10.0.0.2 up", "10.0.0.2")
wait_bfd("bfdd: session to 10.1.0.2 in Vrf-test up", "10.1.0.2", "Vrf-test")
print(vtysh("show bfd peers"))
s = bfd_peer("10.0.0.2") or {}
check("BGP BFD session (PortChannel): dynamic, single hop, PortChannel0001, 300/300 ms x3",
      s.get("type") == "dynamic" and not s.get("multihop") and s.get("interface") == "PortChannel0001"
      and s.get("transmit-interval") == 300 and s.get("receive-interval") == 300 and s.get("detect-multiplier") == 3, s)
s = bfd_peer("10.1.0.2", "Vrf-test") or {}
check("BGP BFD session (Vrf-test): vrf Vrf-test, interface Ethernet24", s.get("vrf") == "Vrf-test" and s.get("interface") == "Ethernet24", s)
nb = json.loads(vtysh("show bgp neighbors 10.0.0.2 json")).get("10.0.0.2", {})
check("bgpd: peerBfdInfo status Up", nb.get("peerBfdInfo", {}).get("status") == "Up", nb.get("peerBfdInfo"))
wait("bfdsyncd: STATE_DB BFD_SESSION_TABLE|default|PortChannel0001|10.0.0.2 Up",
     "redis-cli -s /var/run/redis/redis.sock -n 6 hget 'BFD_SESSION_TABLE|default|PortChannel0001|10.0.0.2' state | grep -qx Up", timeout=30,
     detail_cmd="redis-cli -s /var/run/redis/redis.sock -n 6 keys 'BFD*'; journalctl -b -u sonic-bfdsyncd --no-pager | tail")
wait("bfdsyncd: STATE_DB BFD_SESSION_TABLE|Vrf-test|Ethernet24|10.1.0.2 Up",
     "redis-cli -s /var/run/redis/redis.sock -n 6 hget 'BFD_SESSION_TABLE|Vrf-test|Ethernet24|10.1.0.2' state | grep -qx Up", timeout=30)
st = hgetall(6, "BFD_SESSION_TABLE|default|PortChannel0001|10.0.0.2")
check("STATE_DB session fields as BfdOrch's (type, local_addr, intervals, multihop)",
      st.get("type") == "async_active" and st.get("local_addr") == "10.0.0.1" and st.get("tx_interval") == "300"
      and st.get("multiplier") == "3" and st.get("multihop") == "false" and st.get("local_discriminator", "") != "", st)
rc, out = cli("show bfd summary")
print(out)
check("show bfd summary (sonic-utilities): both BGP sessions Up", rc == 0 and "10.0.0.2" in out and "10.1.0.2" in out and "Vrf-test" in out and out.count("Up") >= 2, out)
rc, out = cli("show bfd peer 10.1.0.2")
check("show bfd peer 10.1.0.2", rc == 0 and "Ethernet24" in out and "Up" in out, out)

# ---------------------------------------------------------------- 4. static routes with BFD (staticroutebfd)
S, SV = "203.0.113.0/24", "198.18.0.0/24"
rc, out = sh("sonic-db-cli CONFIG_DB hset 'STATIC_ROUTE|default|%s' nexthop 10.2.0.2 ifname Ethernet28 bfd true" % S)
check("CONFIG_DB STATIC_ROUTE|default|%s bfd=true" % S, rc == 0, out)
rc, out = sh("sonic-db-cli CONFIG_DB hset 'STATIC_ROUTE|Vrf-test|%s' nexthop 10.3.0.2 ifname Ethernet32 bfd true" % SV)
check("CONFIG_DB STATIC_ROUTE|Vrf-test|%s bfd=true" % SV, rc == 0, out)
wait("staticroutebfd: APPL_DB BFD_SESSION_TABLE:default:default:10.2.0.2", "redis-cli -s /var/run/redis/redis.sock -n 0 exists BFD_SESSION_TABLE:default:default:10.2.0.2 | grep -qx 1", timeout=60,
     detail_cmd="redis-cli -s /var/run/redis/redis.sock -n 0 keys 'BFD*'; journalctl -b -u sonic-staticroutebfd --no-pager | tail -30")
a = hgetall(0, "BFD_SESSION_TABLE:default:default:10.2.0.2")
check("APPL_DB session: local_addr 10.2.0.1, 300/300 ms x3 (services.sonic.bfd.staticRoute)",
      a.get("local_addr") == "10.2.0.1" and a.get("tx_interval") == "300" and a.get("rx_interval") == "300" and a.get("multiplier") == "3", a)
wait("BfdOrch: no offload -> STATE_DB BFD_SOFTWARE_SESSION_TABLE|default|default|10.2.0.2",
     "redis-cli -s /var/run/redis/redis.sock -n 6 exists 'BFD_SOFTWARE_SESSION_TABLE|default|default|10.2.0.2' | grep -qx 1", timeout=60,
     detail_cmd="redis-cli -s /var/run/redis/redis.sock -n 6 keys 'BFD*'; journalctl -b -u sonic-orchagent --no-pager | grep -i bfd | tail")
check("ASIC_DB: no SAI BFD session object", db(1, "--scan --pattern 'ASIC_STATE:SAI_OBJECT_TYPE_BFD_SESSION*'") == "")
wait_bfd("BfdMgr -> bfdd: configured session 10.2.0.2 up (active, not passive)", "10.2.0.2")
s = bfd_peer("10.2.0.2") or {}
check("static BFD session: configured, no interface, passive-mode false", s.get("type") == "configured" and "interface" not in s and s.get("passive-mode") is False, s)
wait_bfd("Vrf-test: bfdd session 10.3.0.2 up", "10.3.0.2", "Vrf-test")
wait("staticroutebfd saw Up: APPL_DB STATIC_ROUTE:default:%s" % S, "redis-cli -s /var/run/redis/redis.sock -n 0 exists STATIC_ROUTE:default:%s | grep -qx 1" % S, timeout=60,
     detail_cmd="redis-cli -s /var/run/redis/redis.sock -n 6 keys 'BFD*'; journalctl -b -u sonic-staticroutebfd --no-pager | tail -20")
wait("static route %s via 10.2.0.2 in APPL_DB ROUTE_TABLE" % S, "redis-cli -s /var/run/redis/redis.sock -n 0 hget ROUTE_TABLE:%s nexthop | grep -qx 10.2.0.2" % S, timeout=60,
     detail_cmd="vtysh -c 'show ip route static'; vtysh -c 'show running-config'")
wait("static route %s in ASIC_DB" % S, "redis-cli -s /var/run/redis/redis.sock -n 1 --scan --pattern '*ROUTE_ENTRY*\"dest\":\"%s\"*' | grep -q ." % S, timeout=60)
wait("Vrf-test static route %s via 10.3.0.2 in APPL_DB ROUTE_TABLE:Vrf-test:" % SV,
     "redis-cli -s /var/run/redis/redis.sock -n 0 hget ROUTE_TABLE:Vrf-test:%s nexthop | grep -qx 10.3.0.2" % SV, timeout=60,
     detail_cmd="vtysh -c 'show ip route vrf Vrf-test'; redis-cli -s /var/run/redis/redis.sock -n 0 keys 'ROUTE_TABLE:Vrf*'")
rc, out = cli("show bfd summary")
print(out)
check("show bfd summary: 4 sessions", "Total number of BFD sessions: 4" in out, out)

# ---------------------------------------------------------------- 5. static route follows its session (peer: admin shutdown)
pk = "vtysh -c 'conf t' -c 'bfd' -c 'peer 10.2.0.1 local-address 10.2.0.2 interface eth4' -c '%s'"
sh(pk % "shutdown", peer)
t = time_until("redis-cli -s /var/run/redis/redis.sock -n 0 exists ROUTE_TABLE:%s | grep -qx 0" % S, 20)
RESULTS["static route removed after peer BFD admin-down (s)"] = t
check("peer BFD admin-down: static route leaves APPL_DB ROUTE_TABLE (%s s)" % (None if t is None else round(t, 2)), t is not None and t < 3, t)
check("... and APPL_DB STATIC_ROUTE:default:%s" % S, db(0, "exists STATIC_ROUTE:default:%s" % S) == "0")
check("... STATE_DB session Down", db(6, "hget 'BFD_SESSION_TABLE|default|default|10.2.0.2' state") == "Down", hgetall(6, "BFD_SESSION_TABLE|default|default|10.2.0.2"))
wait("... and ASIC_DB", "! redis-cli -s /var/run/redis/redis.sock -n 1 --scan --pattern '*ROUTE_ENTRY*\"dest\":\"%s\"*' | grep -q ." % S, timeout=30)
check("... BGP (PortChannel) unaffected", sh(established("10.0.0.2"))[0] == 0)
sh(pk % "no shutdown", peer)
t = time_until("redis-cli -s /var/run/redis/redis.sock -n 0 hget ROUTE_TABLE:%s nexthop | grep -qx 10.2.0.2" % S, 30)
RESULTS["static route restored after BFD up (s)"] = t
check("peer BFD no shutdown: static route back in APPL_DB (%s s)" % (None if t is None else round(t, 2)), t is not None, t)
wait("... and in ASIC_DB", "redis-cli -s /var/run/redis/redis.sock -n 1 --scan --pattern '*ROUTE_ENTRY*\"dest\":\"%s\"*' | grep -q ." % S, timeout=30)

# ---------------------------------------------------------------- 6. BGP over the PortChannel: 100% loss at the peer
sh("tc qdisc add dev bond0 root netem loss 100%", peer)
t = time_until("! " + established("10.0.0.2"), 30)
RESULTS["BGP (PortChannel) down after 100% loss (s)"] = t
check("100%% loss on the peer's bond0: BGP 10.0.0.2 down in %s s (< 1.5 s; hold time 90 s)" % (None if t is None else round(t, 2)), t is not None and t < 1.5, t)
nb = json.loads(vtysh("show bgp neighbors 10.0.0.2 json")).get("10.0.0.2", {})
print("lastResetDueTo:", nb.get("lastResetDueTo"))
check("bgpd: reset due to BFD", "BFD" in str(nb.get("lastResetDueTo", "")), nb.get("lastResetDueTo"))
check("... static route via Ethernet8/1 unaffected", db(0, "hget ROUTE_TABLE:%s nexthop" % S) == "10.2.0.2")
wait("... BGP prefix 192.0.2.0/24 gone from APPL_DB (default VRF)", "redis-cli -s /var/run/redis/redis.sock -n 0 exists ROUTE_TABLE:192.0.2.0/24 | grep -qx 0", timeout=10)
check("... LACP still up (only IP is lost: BFD, not LACP, detected it)", sh("teamdctl PortChannel0001 state item get ports.Ethernet16.runner.selected | grep -qx true")[0] == 0)
print(journal(["sonic-frr-bgpd", "sonic-frr-bfdd", "sonic-bfdsyncd", "sonic-staticroutebfd"], 25, grep="bfd|ADJCHANGE|10.0.0"))
sh("tc qdisc del dev bond0 root", peer)
wait_bfd("loss removed: BFD 10.0.0.2 up again", "10.0.0.2")
wait("... BGP 10.0.0.2 Established again", established("10.0.0.2"), timeout=120)
wait("... BGP prefix 192.0.2.0/24 back in APPL_DB", "redis-cli -s /var/run/redis/redis.sock -n 0 exists ROUTE_TABLE:192.0.2.0/24 | grep -qx 1", timeout=60)

# ---------------------------------------------------------------- 7. BGP over a port in Vrf-test: link down at the peer
sh("ip link set eth3 down", peer)
t = time_until("! " + established("10.1.0.2", "Vrf-test"), 30)
RESULTS["BGP (Vrf-test port) down after peer link down (s)"] = t
check("peer eth3 down: BGP Vrf-test 10.1.0.2 down in %s s (< 1.5 s)" % (None if t is None else round(t, 2)), t is not None and t < 1.5, t)
check("... switch's Ethernet24 still oper up (no carrier loss seen: BFD detected it)", db(0, "hget PORT_TABLE:Ethernet24 oper_status") == "up", hgetall(0, "PORT_TABLE:Ethernet24"))
check("... Vrf-test static route (Ethernet9/1) and default VRF BGP unaffected",
      db(0, "hget ROUTE_TABLE:Vrf-test:%s nexthop" % SV) == "10.3.0.2" and sh(established("10.0.0.2"))[0] == 0)
sh("ip link set eth3 up", peer)
wait("peer eth3 up: BGP Vrf-test Established again", established("10.1.0.2", "Vrf-test"), timeout=120)
# static route in Vrf-test: link down at the peer
sh("ip link set eth5 down", peer)
t = time_until("redis-cli -s /var/run/redis/redis.sock -n 0 exists ROUTE_TABLE:Vrf-test:%s | grep -qx 0" % SV, 30)
RESULTS["Vrf-test static route removed after peer link down (s)"] = t
check("peer eth5 down: Vrf-test static route out of APPL_DB in %s s (< 1.5 s)" % (None if t is None else round(t, 2)), t is not None and t < 1.5, t)
wait("... and out of ASIC_DB", "! redis-cli -s /var/run/redis/redis.sock -n 1 --scan --pattern '*ROUTE_ENTRY*\"dest\":\"%s\"*' | grep -q ." % SV, timeout=30)
sh("ip link set eth5 up", peer)
wait("peer eth5 up: Vrf-test static route back", "redis-cli -s /var/run/redis/redis.sock -n 0 hget ROUTE_TABLE:Vrf-test:%s nexthop | grep -qx 10.3.0.2" % SV, timeout=60)

# ---------------------------------------------------------------- 8. live toggle: without BFD the hold timer (90 s) rules
sh("vtysh -c 'conf t' -c 'router bgp 65200' -c 'no neighbor 10.1.0.1 bfd'", peer)
sh("sonic-db-cli CONFIG_DB hset 'BGP_NEIGHBOR|Vrf-test|10.1.0.2' bfd false")
wait("bfd=false: bgpcfgd removed 'neighbor 10.1.0.2 bfd' live", "! vtysh -c 'show running-config' | grep -q 'neighbor 10.1.0.2 bfd'", timeout=30,
     detail_cmd="journalctl -b -u sonic-bgpcfgd --no-pager | tail -20")
wait_bfd("... bfdd session 10.1.0.2 gone", "10.1.0.2", "Vrf-test", None, timeout=30)
wait("... and from STATE_DB (bfdsyncd)", "redis-cli -s /var/run/redis/redis.sock -n 6 exists 'BFD_SESSION_TABLE|Vrf-test|Ethernet24|10.1.0.2' | grep -qx 0", timeout=30)
check("... no 'Can't update the peer' error from bgpcfgd", "Can't update the peer" not in journal(["sonic-bgpcfgd"], 400), journal(["sonic-bgpcfgd"], 20))
wait("... BGP Vrf-test Established", established("10.1.0.2", "Vrf-test"), timeout=120)
sh("ip link set eth3 down", peer)
time.sleep(5)
check("no BFD: BGP Vrf-test still Established 5 s after the peer link went down (hold time 90 s)", sh(established("10.1.0.2", "Vrf-test"))[0] == 0)
sh("ip link set eth3 up", peer)
sh("vtysh -c 'conf t' -c 'router bgp 65200' -c 'neighbor 10.1.0.1 bfd'", peer)
sh("sonic-db-cli CONFIG_DB hset 'BGP_NEIGHBOR|Vrf-test|10.1.0.2' bfd true")
wait("bfd=true again: neighbor 10.1.0.2 bfd back in bgpd", "vtysh -c 'show running-config' | grep -q 'neighbor 10.1.0.2 bfd'", timeout=30)
wait_bfd("... session up", "10.1.0.2", "Vrf-test")

# ---------------------------------------------------------------- 9. restarts: bfdsyncd, bfdd
sh("systemctl kill -s KILL sonic-bfdsyncd")
wait("bfdsyncd killed: restarted, STATE_DB still Up", "systemctl is-active sonic-bfdsyncd && redis-cli -s /var/run/redis/redis.sock -n 6 hget 'BFD_SESSION_TABLE|default|default|10.2.0.2' state | grep -qx Up", timeout=30)
sh("systemctl restart sonic-frr-bfdd")
wait("bfdd restart: bgpcfgd restarted with it, all 4 sessions up again",
     "[ $(vtysh -c 'show bfd peers json' | grep -o '\"status\":\"up\"' | wc -l) -eq 4 ]", timeout=120,
     detail_cmd="vtysh -c 'show bfd peers'; systemctl list-units 'sonic-*' --no-pager")
wait("... static routes back in APPL_DB", "redis-cli -s /var/run/redis/redis.sock -n 0 hget ROUTE_TABLE:%s nexthop | grep -qx 10.2.0.2 && redis-cli -s /var/run/redis/redis.sock -n 0 hget ROUTE_TABLE:Vrf-test:%s nexthop | grep -qx 10.3.0.2" % (S, SV), timeout=60)
wait("... BGP both Established", established("10.0.0.2") + " && " + established("10.1.0.2", "Vrf-test"), timeout=120)
check("orchagent not restarted by any of this", units_report()["sonic-orchagent"].endswith(" 0"), units_report())

# ---------------------------------------------------------------- 10. CPU (4 sessions at 300 ms) and logs
def ticks(pid):
    return int(sh("awk '{print $14+$15}' /proc/%s/stat" % pid)[1].strip() or 0)
pids = {"bfdd": sh("pgrep -x bfdd")[1].strip(), "bfdsyncd": sh("systemctl show -p MainPID --value sonic-bfdsyncd")[1].strip()}
hz = int(sh("getconf CLK_TCK")[1].strip())
t0 = {k: ticks(p) for k, p in pids.items()}
time.sleep(30)
cpu = {k: round(100.0 * (ticks(p) - t0[k]) / hz / 30, 2) for k, p in pids.items()}
RESULTS["CPU % over 30 s, 4 sessions (x86 VM)"] = cpu
print("CPU %% over 30 s: %s" % cpu)
check("bfdd + bfdsyncd CPU below 10%% of a core in the VM (%s)" % cpu, sum(cpu.values()) < 10, cpu)
errs = sh("journalctl -b --no-pager -o cat -u sonic-orchagent -u sonic-syncd -p err | grep -i bfd")[1]
check("no BFD errors from orchagent/syncd", errs.strip() == "", errs)
errs = sh("journalctl -b --no-pager -o cat -u sonic-orchagent | grep -i 'Exception caught'")[1]
check("orchagent: no exceptions (BfdMonitorOrch parses STATE_DB BFD_SESSION_TABLE)", errs.strip() == "", errs)
print(journal(["sonic-orchagent"], 10, grep="bfd"))
print(journal(["sonic-bfdsyncd"], 40))
print("RESULTS " + json.dumps(RESULTS))
print("FAILED CHECKS: %d %s" % (len(FAILS), FAILS))
assert not FAILS, FAILS
