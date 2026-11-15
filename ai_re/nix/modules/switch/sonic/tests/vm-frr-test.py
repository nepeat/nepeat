import json, time

FAILS = []
def check(name, cond, detail=""):
    d = str(detail)
    print(("PASS " if cond else "FAIL ") + name + ("" if cond else " :: " + (d if len(d) < 3000 else d[:800] + " ... " + d[-2000:])))
    if not cond:
        FAILS.append(name)

def sh(cmd, m=None):
    return (m or switch).execute(cmd)

def cli(cmd):
    # login shell: SONIC_CLI_IFACE_MODE=alias
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

FRR_UNITS = ["sonic-frr-config", "sonic-frr-mgmtd", "sonic-frr-zebra", "sonic-frr-staticd", "sonic-frr-bgpd",
             "sonic-fpmsyncd", "sonic-bgpcfgd", "sonic-bgpmon"]

def units_report():
    out = {}
    for u in FRR_UNITS + ["sonic-orchagent", "sonic-syncd"]:
        out[u] = sh("systemctl show -p ActiveState -p SubState -p NRestarts --value %s.service | tr '\\n' ' '" % u)[1].strip()
    return out

def diag(tag):
    print("==== diag " + tag)
    print(json.dumps(units_report(), indent=1))
    print(sh("journalctl -b --no-pager -o short-monotonic -u 'sonic-frr-*' -u sonic-fpmsyncd -u sonic-bgpcfgd | tail -120")[1])

def route_in_asic(prefix):
    return "SAI_OBJECT_TYPE_ROUTE_ENTRY:" in db(1, "--scan --pattern '*ROUTE_ENTRY*\"dest\":\"%s\"*'" % prefix)

start_all()
switch.wait_for_unit("sonic.target", timeout=300)
switch.wait_for_unit("sonic-orchagent.service", timeout=300)
switch.wait_until_succeeds("redis-cli -s /var/run/redis/redis.sock -n 0 exists PORT_TABLE:PortInitDone | grep -q 1", timeout=240)

# ---------------------------------------------------------------- 1. units, rendered config, vtysh
try:
    for u in FRR_UNITS:
        switch.wait_for_unit(u + ".service", timeout=120)
except Exception:
    diag("frr units")
    raise
time.sleep(10)
rep = units_report()
print(json.dumps(rep, indent=1))
bad = {u: s for u, s in rep.items() if not s.startswith("active") or not s.endswith(" 0")}
check("FRR/fpmsyncd/bgpcfgd units active, no restarts", not bad, bad)
rc, out = sh("command -v vtysh && sudo vtysh -c 'show daemons'")
check("vtysh on PATH (also via sudo), daemons mgmtd zebra bgpd staticd", rc == 0 and all(d in out for d in ["mgmtd", "zebra", "bgpd", "staticd"]), out)
conf = sh("cat /etc/frr/bgpd.conf /etc/frr/zebra.conf /etc/frr/staticd.conf")[1]
check("bgpd.conf rendered from CONFIG_DB: router bgp 65100", "router bgp 65100" in conf, conf[:3000])
check("zebra.conf: FPM to 127.0.0.1, no next-hop groups", "fpm address 127.0.0.1" in conf and "no fpm use-next-hop-groups" in conf, conf)
check("config files owned by frr", sh("stat -c %U /etc/frr/bgpd.conf")[1].strip() == "frr")
out = vtysh("show running-config")
check("running bgpd: router bgp 65100, no ebgp-requires-policy", "router bgp 65100" in out and "no bgp ebgp-requires-policy" in out, out)
out = vtysh("show fpm status")
print(out)
wait("zebra's FPM connected to fpmsyncd", "vtysh -c 'show fpm status' | grep -qi 'Connected'", timeout=60,
     detail_cmd="vtysh -c 'show fpm status'; ss -tnp | grep 2620")

# ---------------------------------------------------------------- 2. PortChannel to the peer, IP, eBGP via CONFIG_DB
for c in ["config portchannel add PortChannel0001", "config portchannel member add PortChannel0001 Ethernet5/1",
          "config portchannel member add PortChannel0001 Ethernet6/1", "config interface startup Ethernet5/1",
          "config interface startup Ethernet6/1", "config interface ip add PortChannel0001 10.0.0.1/24"]:
    rc, out = cli(c)
    check(c, rc == 0, out)
peer.wait_for_unit("frr.service")
wait("ping across the PortChannel (peer -> 10.0.0.1)", "ping -c1 -W2 10.0.0.1", timeout=180, m=peer,
     detail_cmd="ip addr; cat /proc/net/bonding/bond0")
check("seeded: DEVICE_METADATA type LeafRouter, BGP_DEVICE_GLOBAL|STATE", db(4, "hget 'DEVICE_METADATA|localhost' type") == "LeafRouter" and db(4, "hget 'BGP_DEVICE_GLOBAL|STATE' tsa_enabled") == "false")
# Loopback0: a bgpcfgd dependency for BGP_NEIGHBOR peers, as on SONiC.
for c in ["config loopback add Loopback0", "config interface ip add Loopback0 10.255.0.1/32"]:
    rc, out = cli(c)
    check(c, rc == 0, out)
wait("Loopback0 in the kernel", "ip addr show Loopback0 | grep -q 10.255.0.1/32", timeout=60)
# Unmanaged CONFIG_DB (what "config load"/"apply-patch" would write): bgpcfgd -> bgpd.
rc, out = sh("sonic-db-cli CONFIG_DB hset 'BGP_NEIGHBOR|10.0.0.2' asn 65200 name peer local_addr 10.0.0.1 holdtime 9 keepalive 3 admin_status up")
check("BGP_NEIGHBOR|10.0.0.2 written to CONFIG_DB", rc == 0, out)
wait("bgpcfgd configured the neighbor in bgpd", "vtysh -c 'show running-config' | grep -q 'neighbor 10.0.0.2 remote-as 65200'", timeout=60,
     detail_cmd="journalctl -b -u sonic-bgpcfgd --no-pager | tail -40")
ok = wait("BGP session Established", "vtysh -c 'show bgp neighbors 10.0.0.2 json' | grep -q '\"bgpState\":\"Established\"'", timeout=120,
          detail_cmd="vtysh -c 'show bgp summary'; vtysh -c 'show bgp neighbors 10.0.0.2' | head -40")
if not ok:
    print(vtysh("show bgp summary", peer))
print(vtysh("show bgp summary"))
rc, out = cli("show ip bgp summary")
check("show ip bgp summary (CLI)", rc == 0 and "10.0.0.2" in out and "65200" in out, out)
wait("bgpmon: STATE_DB NEIGH_STATE_TABLE|10.0.0.2 Established", "redis-cli -s /var/run/redis/redis.sock -n 6 hget 'NEIGH_STATE_TABLE|10.0.0.2' state | grep -qx Established", timeout=60)

# ---------------------------------------------------------------- 3. learned prefix -> zebra -> FPM -> APPL_DB -> ASIC_DB
P = "192.0.2.0/24"
wait("zebra RIB: %s via BGP" % P, "vtysh -c 'show ip route %s json' | grep -q '\"protocol\":\"bgp\"'" % P, timeout=60,
     detail_cmd="vtysh -c 'show ip route'")
wait("APPL_DB ROUTE_TABLE:%s" % P, "redis-cli -s /var/run/redis/redis.sock -n 0 exists ROUTE_TABLE:%s | grep -qx 1" % P, timeout=60,
     detail_cmd="redis-cli -s /var/run/redis/redis.sock -n 0 keys 'ROUTE_TABLE*'; journalctl -b -u sonic-fpmsyncd --no-pager | tail -30")
r = db(0, "hgetall ROUTE_TABLE:%s" % P).split()
rt = dict(zip(r[0::2], r[1::2]))
check("APPL_DB route: nexthop 10.0.0.2 ifname PortChannel0001 protocol bgp", rt.get("nexthop") == "10.0.0.2" and rt.get("ifname") == "PortChannel0001" and rt.get("protocol") == "bgp", rt)
wait("ASIC_DB ROUTE_ENTRY %s" % P, "redis-cli -s /var/run/redis/redis.sock -n 1 --scan --pattern '*ROUTE_ENTRY*\"dest\":\"%s\"*' | grep -q ." % P, timeout=60,
     detail_cmd="journalctl -b -u sonic-orchagent --no-pager | tail -30")
key = db(1, "--scan --pattern '*ROUTE_ENTRY*\"dest\":\"%s\"*'" % P)
nh = db(1, "hget '%s' SAI_ROUTE_ENTRY_ATTR_NEXT_HOP_ID" % key)
nhip = db(1, "hget ASIC_STATE:SAI_OBJECT_TYPE_NEXT_HOP:%s SAI_NEXT_HOP_ATTR_IP" % nh)
check("ASIC_DB: route next hop is 10.0.0.2", nhip == "10.0.0.2", (key, nh, nhip))
check("ASIC_DB ROUTE_ENTRY 198.51.100.0/24 (second prefix)", route_in_asic("198.51.100.0/24"))
check("kernel: %s via 10.0.0.2 proto bgp" % P, "via 10.0.0.2" in sh("ip route show %s" % P)[1] and "bgp" in sh("ip route show %s" % P)[1], sh("ip route show %s" % P)[1])
wait("zebra marks the route offloaded (fpmsyncd ack)", "vtysh -c 'show ip route %s json' | grep -q '\"offloaded\":true'" % P, timeout=60,
     detail_cmd="vtysh -c 'show ip route %s json'" % P)
check("connected 10.0.0.0/24 not duplicated as a /32 local route in APPL_DB", db(0, "exists ROUTE_TABLE:10.0.0.1/32") == "0")
rc, out = cli("show ip route")
print(out)
check("show ip route (CLI, alias mode) lists %s via 10.0.0.2" % P, rc == 0 and P in out and "10.0.0.2" in out, out)
rc, out = cli("show ip route %s" % P)
check("show ip route %s (CLI)" % P, rc == 0 and "10.0.0.2" in out, out)

# ---------------------------------------------------------------- 4. static route from CONFIG_DB (bgpcfgd -> staticd)
S = "203.0.113.0/24"
rc, out = cli("config route add prefix %s nexthop 10.0.0.2" % S)
check("config route add prefix %s nexthop 10.0.0.2" % S, rc == 0, out)
check("CONFIG_DB STATIC_ROUTE|default|%s" % S, db(4, "hget 'STATIC_ROUTE|default|%s' nexthop" % S) == "10.0.0.2", db(4, "keys 'STATIC_ROUTE*'"))
wait("staticd route %s in zebra" % S, "vtysh -c 'show ip route static' | grep -q '%s'" % S, timeout=60,
     detail_cmd="journalctl -b -u sonic-bgpcfgd --no-pager | tail -30; vtysh -c 'show running-config'")
# protocol is zebra's 196 (iproute2's rt_protos has no name for it, as on SONiC).
wait("APPL_DB ROUTE_TABLE:%s (static, via 10.0.0.2)" % S, "redis-cli -s /var/run/redis/redis.sock -n 0 hget ROUTE_TABLE:%s nexthop | grep -qx 10.0.0.2" % S, timeout=60)
print("static route in APPL_DB:", db(0, "hgetall ROUTE_TABLE:%s" % S))
wait("ASIC_DB ROUTE_ENTRY %s" % S, "redis-cli -s /var/run/redis/redis.sock -n 1 --scan --pattern '*ROUTE_ENTRY*\"dest\":\"%s\"*' | grep -q ." % S, timeout=60)

# ---------------------------------------------------------------- 5. config interface ip remove
rc, out = cli("config interface ip remove PortChannel0001 10.0.0.1/24")
check("ip remove refused while a static route uses the RIF (SONiC's vtysh check)", rc != 0 and "static" in out.lower(), out)
check("... and the address stays", db(4, "exists 'PORTCHANNEL_INTERFACE|PortChannel0001|10.0.0.1/24'") == "1")
rc, out = cli("config route del prefix %s nexthop 10.0.0.2" % S)
check("config route del prefix %s" % S, rc == 0, out)
wait("static route gone from APPL_DB", "redis-cli -s /var/run/redis/redis.sock -n 0 exists ROUTE_TABLE:%s | grep -qx 0" % S, timeout=60)
wait("static route gone from ASIC_DB", "! redis-cli -s /var/run/redis/redis.sock -n 1 --scan --pattern '*ROUTE_ENTRY*\"dest\":\"%s\"*' | grep -q ." % S, timeout=60)
rc, out = cli("config interface ip add Ethernet7/1 10.1.0.1/24")
check("config interface ip add Ethernet7/1 10.1.0.1/24", rc == 0, out)
switch.wait_until_succeeds("ip addr show Ethernet24 | grep -q 10.1.0.1/24", timeout=60)
rc, out = cli("config interface ip remove Ethernet7/1 10.1.0.1/24")
check("config interface ip remove Ethernet7/1 10.1.0.1/24", rc == 0 and "command not found" not in out, out)
check("... gone from CONFIG_DB", db(4, "exists 'INTERFACE|Ethernet24|10.1.0.1/24'") == "0" and db(4, "exists 'INTERFACE|Ethernet24'") == "0", db(4, "keys 'INTERFACE*'"))
wait("... gone from the kernel", "! ip addr show Ethernet24 | grep -q 10.1.0.1", timeout=60)

# ---------------------------------------------------------------- 6. zebra restart: stack comes back, bgpcfgd replays CONFIG_DB
sh("systemctl restart sonic-frr-zebra.service")
ok = wait("after zebra restart: BGP Established again", "vtysh -c 'show bgp neighbors 10.0.0.2 json' | grep -q '\"bgpState\":\"Established\"'", timeout=180,
          detail_cmd="systemctl list-units 'sonic-*' --no-pager; journalctl -b -u sonic-bgpcfgd --no-pager | tail -20")
if not ok:
    diag("zebra restart")
wait("after zebra restart: route back in APPL_DB and ASIC_DB",
     "redis-cli -s /var/run/redis/redis.sock -n 0 exists ROUTE_TABLE:%s | grep -qx 1 && redis-cli -s /var/run/redis/redis.sock -n 1 --scan --pattern '*ROUTE_ENTRY*\"dest\":\"%s\"*' | grep -q ." % (P, P), timeout=120)
rep = units_report()
check("after zebra restart: all FRR units active", all(s.startswith("active") for s in rep.values()), rep)
check("after zebra restart: orchagent not restarted", rep["sonic-orchagent"].endswith(" 0"), rep)

# ---------------------------------------------------------------- 7. crash recovery (recovery.nix)
def inv(u):
    return sh("systemctl show -p InvocationID --value %s" % u)[1].strip()
def teamd_pid(lag):
    return sh("cat /run/teamd/%s.pid" % lag)[1].strip()
def all_active():
    rep = units_report()
    return all(s.startswith("active") for s in rep.values()), rep
# A second PortChannel (no members) must not flap when PortChannel0001's teamd dies.
rc, out = cli("config portchannel add PortChannel0002")
check("config portchannel add PortChannel0002", rc == 0, out)
switch.wait_until_succeeds("test -s /run/teamd/PortChannel0002.pid && kill -0 $(cat /run/teamd/PortChannel0002.pid)", timeout=60)
time.sleep(6)   # the watcher records the command lines
pid1, pid2, tm = teamd_pid("PortChannel0001"), teamd_pid("PortChannel0002"), inv("sonic-teammgrd")
sh("kill -9 %s" % pid1)
wait("teamd killed: respawned for PortChannel0001", "p=$(cat /run/teamd/PortChannel0001.pid) && [ \"$p\" != %s ] && kill -0 $p" % pid1, timeout=60,
     detail_cmd="journalctl -b -u sonic-teamd-watch --no-pager | tail -20")
wait("teamd respawn: both members selected again", "teamdctl PortChannel0001 state item get ports.Ethernet16.runner.selected | grep -qx true && teamdctl PortChannel0001 state item get ports.Ethernet20.runner.selected | grep -qx true", timeout=120,
     detail_cmd="teamdctl PortChannel0001 state; journalctl -b -u sonic-teamd-watch --no-pager | tail -20")
check("teamd respawn: PortChannel0001 kept its IP (netdev taken over)", "10.0.0.1/24" in sh("ip addr show PortChannel0001")[1], sh("ip addr show PortChannel0001")[1])
wait("teamd respawn: ping peer", "ping -c1 -W2 10.0.0.1", timeout=90, m=peer)
check("teamd respawn: PortChannel0002's teamd untouched", teamd_pid("PortChannel0002") == pid2, (pid2, teamd_pid("PortChannel0002")))
check("teamd respawn: teammgrd not restarted", inv("sonic-teammgrd") == tm)
check("teamd respawn: new teamd in teammgrd's cgroup", "sonic-teammgrd.service" in sh("cat /proc/%s/cgroup" % teamd_pid("PortChannel0001"))[1], sh("cat /proc/%s/cgroup" % teamd_pid("PortChannel0001"))[1])
wait("teamd respawn: BGP Established", "vtysh -c 'show bgp neighbors 10.0.0.2 json' | grep -q '\"bgpState\":\"Established\"'", timeout=120)
check("teamd respawn: APPL_DB LAG members kept", db(0, "exists LAG_MEMBER_TABLE:PortChannel0001:Ethernet16") == "1" and db(0, "exists LAG_MEMBER_TABLE:PortChannel0001:Ethernet20") == "1")
wait("teamd respawn: show interfaces portchannel shows members", "bash -lc 'show interfaces portchannel' | grep PortChannel0001 | grep -q 'Ethernet5/1(S)'", timeout=60,
     detail_cmd="bash -lc 'show interfaces portchannel'")

# orchagent dies alone -> syncd (and everything below) restarts.
syncd_inv, orch_pid = inv("sonic-syncd"), sh("systemctl show -p MainPID --value sonic-orchagent")[1].strip()
sh("kill -9 %s" % orch_pid)
wait("orchagent killed: syncd restarted", "[ \"$(systemctl show -p InvocationID --value sonic-syncd)\" != %s ] && systemctl is-active sonic-syncd" % syncd_inv, timeout=60,
     detail_cmd="journalctl -b -u sonic-orchagent -u sonic-syncd --no-pager | tail -30")
switch.wait_until_succeeds("systemctl is-active sonic-orchagent sonic-portsyncd sonic-teammgrd sonic-frr-bgpd sonic-bgpcfgd", timeout=180)
switch.wait_until_succeeds("redis-cli -s /var/run/redis/redis.sock -n 0 exists PORT_TABLE:PortInitDone | grep -q 1", timeout=240)
wait("after orchagent kill: Ethernet netdevs back (35)", "[ $(ls /sys/class/net | grep -c '^Ethernet') -eq 35 ]", timeout=120,
     detail_cmd="ls /sys/class/net")
wait("after orchagent kill: LACP + ping peer", "ping -c1 -W2 10.0.0.1", timeout=180, m=peer)
wait("after orchagent kill: BGP route back in ASIC_DB", "redis-cli -s /var/run/redis/redis.sock -n 1 --scan --pattern '*ROUTE_ENTRY*\"dest\":\"%s\"*' | grep -q ." % P, timeout=180,
     detail_cmd="vtysh -c 'show bgp summary'; journalctl -b -u sonic-bgpcfgd --no-pager | tail -20")
time.sleep(15)   # syncd's restartOnStop grace (10 s): the ASIC temperature reader stays stopped
check("after orchagent kill: fake-asic-temp stays stopped (syncd is back)", sh("systemctl is-active fake-asic-temp")[1].strip() != "active", sh("systemctl status fake-asic-temp --no-pager")[1])
ok, rep = all_active()
check("after orchagent kill: all units active", ok, rep)

# syncd dies -> orchagent restarts with it.
orch_inv = inv("sonic-orchagent")
sh("kill -9 $(systemctl show -p MainPID --value sonic-syncd)")
wait("syncd killed: orchagent restarted", "[ \"$(systemctl show -p InvocationID --value sonic-orchagent)\" != %s ] && systemctl is-active sonic-orchagent" % orch_inv, timeout=90)
switch.wait_until_succeeds("redis-cli -s /var/run/redis/redis.sock -n 0 exists PORT_TABLE:PortInitDone | grep -q 1", timeout=240)
wait("after syncd kill: Ethernet netdevs back (35)", "[ $(ls /sys/class/net | grep -c '^Ethernet') -eq 35 ]", timeout=120)
wait("after syncd kill: ping peer", "ping -c1 -W2 10.0.0.1", timeout=180, m=peer)
time.sleep(15)
check("after syncd kill: fake-asic-temp stays stopped", sh("systemctl is-active fake-asic-temp")[1].strip() != "active")
# Stopping the stack on purpose is not a failure: no syncd restart from the hook.
sh("systemctl stop sonic.target")
wait("stop sonic.target: fake-asic-temp started again (syncd's 10 s grace)", "systemctl is-active fake-asic-temp", timeout=40)
check("stop sonic.target: syncd stays down (no restart from the orchagent hook)", sh("systemctl is-active sonic-syncd")[1].strip() != "active", sh("systemctl is-active sonic-syncd")[1])
sh("systemctl start sonic.target")
switch.wait_until_succeeds("systemctl is-active sonic-orchagent sonic-frr-bgpd sonic-bgpcfgd", timeout=180)
switch.wait_until_succeeds("redis-cli -s /var/run/redis/redis.sock -n 0 exists PORT_TABLE:PortInitDone | grep -q 1", timeout=240)
wait("after sonic.target restart: BGP Established", "vtysh -c 'show bgp neighbors 10.0.0.2 json' | grep -q '\"bgpState\":\"Established\"'", timeout=240)
wait("after sonic.target restart: route back in ASIC_DB", "redis-cli -s /var/run/redis/redis.sock -n 1 --scan --pattern '*ROUTE_ENTRY*\"dest\":\"%s\"*' | grep -q ." % P, timeout=120)

# ---------------------------------------------------------------- 8. withdraw: peer stops advertising
sh("vtysh -c 'conf t' -c 'router bgp 65200' -c 'address-family ipv4 unicast' -c 'no network 192.0.2.0/24'", peer)
wait("withdrawn prefix leaves APPL_DB", "redis-cli -s /var/run/redis/redis.sock -n 0 exists ROUTE_TABLE:%s | grep -qx 0" % P, timeout=60)
wait("withdrawn prefix leaves ASIC_DB", "! redis-cli -s /var/run/redis/redis.sock -n 1 --scan --pattern '*ROUTE_ENTRY*\"dest\":\"%s\"*' | grep -q ." % P, timeout=60)
check("other prefix still in ASIC_DB", route_in_asic("198.51.100.0/24"))
# Last IP of the PortChannel, no static route left: removal works, session drops, routes go.
rc, out = cli("config interface ip remove PortChannel0001 10.0.0.1/24")
check("config interface ip remove PortChannel0001 10.0.0.1/24 (no static route left)", rc == 0, out)
wait("routes via the removed RIF leave ASIC_DB", "! redis-cli -s /var/run/redis/redis.sock -n 1 --scan --pattern '*ROUTE_ENTRY*\"dest\":\"198.51.100.0/24\"*' | grep -q .", timeout=120)
print(sh("journalctl -b --no-pager -o cat -u sonic-orchagent -p warning | tail -20")[1])

# ---------------------------------------------------------------- 9. config reload without a saved file: Nix base only
sh("rm -f /var/lib/sonic/config_db.json")
rc, out = cli("config reload -y")
print(out)
check("config reload -y with no saved file: handed to sonic-nix reload, not aborted", rc == 0 and "Aborted" not in out and "sonic-nix reload" in out, out)
check("reload: runtime config gone (PortChannel0001, BGP_NEIGHBOR)", db(4, "exists 'PORTCHANNEL|PortChannel0001'") == "0" and db(4, "exists 'BGP_NEIGHBOR|10.0.0.2'") == "0")
check("reload: Nix base loaded (bgp_asn 65100, ports)", db(4, "hget 'DEVICE_METADATA|localhost' bgp_asn") == "65100" and db(4, "exists 'PORT|Ethernet0'") == "1")
switch.wait_until_succeeds("systemctl is-active sonic-orchagent sonic-frr-bgpd sonic-bgpcfgd sonic-fpmsyncd", timeout=180)
switch.wait_until_succeeds("redis-cli -s /var/run/redis/redis.sock -n 0 exists PORT_TABLE:PortInitDone | grep -q 1", timeout=240)
check("reload: bgpd running with router bgp 65100 and no neighbor", "router bgp 65100" in vtysh("show running-config") and "10.0.0.2" not in vtysh("show running-config"), vtysh("show running-config"))

print("FAILED CHECKS: %d %s" % (len(FAILS), FAILS))
assert not FAILS, FAILS
