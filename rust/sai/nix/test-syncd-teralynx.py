# vm-sai-teralynx-syncd (./test-syncd-teralynx.nix): SONiC on the Rust Teralynx libsai over the mock
# libifcs. CALLS (the mock's call log), SDK (the stand-in SDK tree), LIBDIR are set by the nix file.
import json, re, time

FAILS = []
def check(name, cond, detail=""):
    d = str(detail)
    print(("PASS " if cond else "FAIL ") + name + ("" if cond else " :: " + (d if len(d) < 2500 else d[:500] + " ... " + d[-2000:])))
    if not cond:
        FAILS.append(name)

def sh(cmd):
    return switch.execute(cmd)

def db(n, cmd):
    return sh("redis-cli -s /var/run/redis/redis.sock -n %d %s" % (n, cmd))[1].strip()

def asic_keys(t):
    return [k for k in db(1, "--scan --pattern 'ASIC_STATE:SAI_OBJECT_TYPE_%s:*'" % t).split("\n") if k]

def asic(key, field):
    return db(1, "hget %s %s" % (json.dumps(key), field))

def calls():
    return sh("cat " + CALLS)[1]

mark = [0]
def since():
    """The call-log lines since the last since() (and moves the mark)."""
    c = calls().split("\n")
    new, mark[0] = c[mark[0]:], len(c)
    return new

def dps(lines):
    """The devport numbers of ifcs_devport_attr_set lines."""
    return {m.group(1) for m in (re.search(r"dp=(\d+)", x) for x in lines) if m}

def has(lines, rx):
    return [l for l in lines if re.search(rx, l)]

def wait(desc, f, timeout=60):
    for _ in range(timeout * 2):
        v = f()
        if v:
            return v
        time.sleep(0.5)
    check(desc + " (timed out)", False)
    return None

def journal(unit):
    return sh("journalctl -b --no-pager -o cat -u %s" % unit)[1]

UNITS = ["sonic-syncd", "sonic-orchagent", "sonic-portmgrd", "sonic-vlanmgrd", "sonic-intfmgrd", "sonic-coppmgrd", "sonic-buffermgrd"]
def restarts():
    return {u: sh("systemctl show -p ActiveState -p NRestarts --value %s.service | tr '\\n' ' '" % u)[1].strip() for u in UNITS}

start_all()
switch.wait_for_unit("sonic.target", timeout=300)
switch.wait_for_unit("sonic-orchagent.service", timeout=300)
switch.wait_until_succeeds("redis-cli -s /var/run/redis/redis.sock -n 0 exists PORT_TABLE:PortInitDone | grep -q 1", timeout=240)
time.sleep(15)
slog = journal("sonic-syncd")
print("SYNCD (head)")
print("\n".join(slog.split("\n")[:80]))
rep = restarts()
check("units active, no restarts", all(s == "active 0" for s in rep.values()), rep)

# ------------------------------------------------------------ create_switch on the mock, the board's profile
profile = sh("cat $(systemctl show -p ExecStart --value sonic-syncd.service | grep -o '/nix/store/[^ ]*sai.profile' | head -1)")[1]
print(profile)
check("profile: the board's keys, libifcs = the stand-in SDK's (the mock)",
      "SAI_TERALYNX_LIBIFCS=%s/libifcs.so" % SDK in profile and "SAI_TERALYNX_PORT_MAP=" in profile
      and "SAI_INIT_CONFIG_FILE=" in profile and "SAI_TERALYNX_INIT_CMDS=" in profile and "SAI_OPENBCM_BACKEND=openbcm" in profile, profile)
env = sh("tr '\\0' '\\n' < /proc/$(systemctl show -p MainPID --value sonic-syncd.service)/environ")[1]
maps = sh("cat /proc/$(systemctl show -p MainPID --value sonic-syncd.service)/maps")[1]
check("syncd runs the Rust libsai with the Rust shim next to it",
      LIBDIR + "/libsai.so.1.0.0" in maps and LIBDIR + "/libsai-teralynx-shim.so" in maps and SDK + "/libifcs.so" in maps,
      [l for l in maps.split("\n") if "libsai" in l or "libifcs" in l])
c = calls().split("\n")
print("CALL LOG (head)")
print("\n".join(c[:60]))
init = has(c, r"^ifcs_init cfg=")
check("ifcs_init with the merged run-time config", init and "cfg=/run/sai-teralynx/config.yaml" in init[0] and "boot=" in init[0], init)
check("node create + enable (the vendor's sequence)", has(c, r"^ifcs_node_create node=0 count=0 attrs=null$")
      and has(c, r"^ifcs_node_attr_set node=0 count=1 id=0 u32=1$"), has(c, r"^ifcs_node"))
merged = sh("cat /run/sai-teralynx/config.yaml")[1]
check("merged config: the generated yaml, then the stand-in isg.yaml", "stand-in isg.yaml" in merged and "devport" in merged, merged[-400:])
down = has(c, r"^ifcs_devport_attr_set dp=(\d+) id=23 u32=0$")
check("every port-map devport admin-down at init (32)", len(dps(down)) >= 32, down)
pre = has(c, r"^ifcs_devport_attr_set dp=\d+ id=\d+ .*\[1,2,3,4,5,6,7,8\]$")
check("cs8210-sai-files' preemphasis.cmd applied: 32 devports", len(pre) == 32, pre)
check("syncd: switch created, 32 front-panel ports", re.search(r"switch created: 32 front-panel ports, backend \S+", slog),
      [l for l in slog.split("\n") if "switch created" in l or "init" in l][-10:])
ports = asic_keys("PORT")
check("ASIC_DB: 33 ports (32 + CPU)", len(ports) == 33, len(ports))
pmap_file = re.search(r"^SAI_TERALYNX_PORT_MAP=(\S+)$", profile, re.M)
pmap = sh("cat " + pmap_file.group(1))[1] if pmap_file else ""
check("port map: devport 1 = cage 1's lanes 209..216 at 400G, 32 devports", "1 209,210,211,212,213,214,215,216 400000 rs" in pmap
      and len([x for x in pmap.split("\n") if x and not x.startswith("#")]) == 32, pmap)
check("APPL_DB: Ethernet0 on lanes 209..216", db(0, "hget PORT_TABLE:Ethernet0 lanes") == "209,210,211,212,213,214,215,216")

# ------------------------------------------------------------ netdevs: the SDK's adopted (default mode)
links = sh("ip -o link show")[1]
check("kernel: Ethernet0..Ethernet248, no inno_0_* left", all(("Ethernet%d:" % (8 * i)) in links for i in range(32)) and "inno_0_" not in links,
      links)
adopt = re.findall(r"netdev (Ethernet\d+) on devport (\d+) \(adopted\)", slog)
check("libsai: 32 netdevs adopted", len(adopt) == 32, adopt)
check("no netdev made through IFCS in adopt mode", not has(c, r"^ifcs_netdev_create"), has(c, r"^ifcs_netdev_create"))
mac = sh("cat /sys/class/net/Ethernet0/address")[1].strip()
check("adopted netdev carries the switch MAC", mac == "52:54:00:aa:bb:01", mac)

# ------------------------------------------------------------ ports admin up (CONFIG_DB via services.sonic.settings)
up = wait("devports 1-3 admin up in the call log", lambda: len(dps(has(calls().split("\n"), r"^ifcs_devport_attr_set dp=[123] id=23 u32=1$"))) == 3, 120)
check("ports admin up -> IFCS devport ADMIN_STATE 1 on devports 1, 2, 3", up)
check("ASIC_DB: Ethernet0 admin up", any(asic(k, "SAI_PORT_ATTR_ADMIN_STATE") == "true" for k in ports))
# link up on devports 1-3: the mock's link scan callback -> the SAI's port state notification -> orchagent
switch.succeed("printf '1 1\\n2 1\\n3 1\\n' > " + LINK)
oper = wait("Ethernet0/8/16 oper up in APPL_DB", lambda: all(db(0, "hget PORT_TABLE:Ethernet%d oper_status" % p) == "up" for p in (0, 8, 16)), 60)
check("link events -> ports oper up (Ethernet0, 8, 16)", oper, [db(0, "hget PORT_TABLE:Ethernet%d oper_status" % p) for p in (0, 8, 16)])
check("the link hook fired the SAI's callback", re.findall(r"teralynx-mock-link: devport [123] up: 0", journal("sonic-syncd")), journal("sonic-syncd")[-1500:])
since()

# ------------------------------------------------------------ VLAN 100 + member
db(4, "hset 'VLAN|Vlan100' vlanid 100")
db(4, "hset 'VLAN_MEMBER|Vlan100|Ethernet8' tagging_mode untagged")
v = wait("ASIC_DB VLAN 100", lambda: [k for k in asic_keys("VLAN") if asic(k, "SAI_VLAN_ATTR_VLAN_ID") == "100"], 60)
wait("ASIC_DB VLAN member", lambda: asic_keys("VLAN_MEMBER"), 60)
time.sleep(2)
l = since()
check("VLAN 100 -> ifcs_l2vni_create vni=100", has(l, r"^ifcs_l2vni_create vni=100 "), l[:80])
check("member Ethernet8 -> l2vni member add (sysport 2) + its flood mc_dest", has(l, r"^ifcs_l2vni_member_add vni=100 n=1 first=0x3e200002")
      and has(l, r"^ifcs_mc_dest_create .*dest=0x3e200002"), has(l, r"vni=100|mc_dest"))
check("untagged member: sysport EGRESS_UNTAGGED_VLAN_ID 100", has(l, r"^ifcs_sysport_attr_set sp=0x3e200002 id=\d+ u32=100$"), has(l, r"sysport_attr_set"))

# ------------------------------------------------------------ a RIF, a neighbor, a route
db(4, "hset 'INTERFACE|Ethernet0' NULL NULL")
db(4, "hset 'INTERFACE|Ethernet0|10.0.0.0/31' NULL NULL")
wait("Ethernet0 has 10.0.0.0/31", lambda: sh("ip -4 addr show Ethernet0 | grep -q 10.0.0.0/31")[0] == 0, 60)
wait("ASIC_DB port RIF", lambda: [k for k in asic_keys("ROUTER_INTERFACE") if asic(k, "SAI_ROUTER_INTERFACE_ATTR_TYPE") == "SAI_ROUTER_INTERFACE_TYPE_PORT"], 60)
switch.succeed("ip neigh replace 10.0.0.1 lladdr 52:54:00:00:00:02 dev Ethernet0 nud permanent")
n = wait("ASIC_DB neighbor 10.0.0.1", lambda: any('"ip":"10.0.0.1"' in k for k in asic_keys("NEIGHBOR_ENTRY")), 60)
if not n:
    print("DIAG neighbor:", sh("ip -d link show Ethernet0; ip neigh show dev Ethernet0; ip -4 addr show Ethernet0")[1])
    print("DIAG APPL_DB:", db(0, "keys 'NEIGH_TABLE*'"), db(0, "hgetall PORT_TABLE:Ethernet0"), db(6, "hgetall 'PORT_TABLE|Ethernet0'"))
    print("DIAG neighsyncd / orchagent:", "\n".join([x for x in (journal("sonic-neighsyncd") + journal("sonic-orchagent")).split("\n") if re.search(r"(?i)neigh|10\.0\.0|Ethernet0\b", x)][-30:]))
switch.succeed("cat > /tmp/r.json << 'EOF'\n" + json.dumps([{"ROUTE_TABLE:198.51.100.0/24": {"nexthop": "10.0.0.1", "ifname": "Ethernet0"}, "OP": "SET"}]) + "\nEOF")
rc, out = sh("swssconfig /tmp/r.json 2>&1")
check("swssconfig ROUTE_TABLE 198.51.100.0/24 via 10.0.0.1", rc == 0, out)
wait("ASIC_DB route 198.51.100.0/24", lambda: any('"dest":"198.51.100.0/24"' in k for k in asic_keys("ROUTE_ENTRY")), 60)
time.sleep(2)
l = since()
check("RIF -> ifcs_intf_create", has(l, r"^ifcs_intf_create h="), l[:80])
check("IP2ME 10.0.0.0/32 -> route entry", has(l, r"^ifcs_route_entry_create key=[0-9a-f]+/4/0a000000/ffffffff "), has(l, r"route_entry"))
check("neighbor -> nexthop on devport 1's sysport + host route", has(l, r"^ifcs_nexthop_create .*dest=0x3e200001")
      and has(l, r"^ifcs_route_entry_create key=[0-9a-f]+/4/0a000001/ffffffff "), has(l, r"nexthop|route_entry"))
check("route 198.51.100.0/24 -> route entry", has(l, r"^ifcs_route_entry_create key=[0-9a-f]+/4/c6336400/ffffff00 "), has(l, r"route_entry"))

# ------------------------------------------------------------ ACL table + rule
db(4, "hset 'ACL_TABLE|DROPTEST' type L3 stage ingress policy_desc dropper ports@ Ethernet16")
wait("ASIC_DB ACL table", lambda: asic_keys("ACL_TABLE"), 60)
# priority inside the SAI's ACL_ENTRY_MINIMUM / MAXIMUM_PRIORITY (orchagent refuses 9999 here)
print("ACL entry priority range:", [l for l in journal("sonic-orchagent").split("\n") if "priority" in l.lower()][:5])
db(4, "hset 'ACL_RULE|DROPTEST|R1' PRIORITY 100 PACKET_ACTION DROP DST_IP 10.9.9.9/32")
wait("ASIC_DB ACL entry", lambda: asic_keys("ACL_ENTRY"), 60)
st = wait("STATE_DB rule Active", lambda: db(6, "hget 'ACL_RULE_TABLE|DROPTEST|R1' status") == "Active", 30)
time.sleep(2)
l = since()
check("ACL table -> an IFCS ACL table", has(l, r"^ifcs_acl_table_create h="), l[:80])
check("bound to Ethernet16 -> acl port add (sysport 3)", has(l, r"^ifcs_acl_port_add acl=0x[0-9a-f]+ sysport=0x3e200003$"), has(l, r"acl_port"))
check("rule -> drop action + ACE + DST_IP match 10.9.9.9", has(l, r"^ifcs_acl_action_create h=0x[0-9a-f]+ type=drop")
      and has(l, r"^ifcs_ace_create ") and has(l, r"^ifcs_acl_match_create .*0a090909"), has(l, r"acl_action|ace_create|acl_match"))
db(4, "del 'ACL_RULE|DROPTEST|R1'")
wait("ACL entry removed", lambda: not asic_keys("ACL_ENTRY"), 60)
db(4, "del 'ACL_TABLE|DROPTEST'")
wait("ACL table removed", lambda: not asic_keys("ACL_TABLE"), 60)
time.sleep(2)
l = since()
check("rule and table removed -> ace delete, acl table delete", has(l, r"^ifcs_ace_delete") and has(l, r"^ifcs_acl_table_delete"), has(l, r"delete|remove"))

# ------------------------------------------------------------ CoPP (coppmgrd / CoppOrch at start)
c = calls().split("\n")
traps = asic_keys("HOSTIF_TRAP")
pols = asic_keys("POLICER")
check("ASIC_DB: CoPP traps (>= 5) and policers (>= 4)", len(traps) >= 5 and len(pols) >= 4, (len(traps), len(pols)))
check("CoPP -> IFCS hostif traps", len(has(c, r"^ifcs_hostif_trap_create h=")) >= 3, has(c, r"hostif_trap"))
check("CoPP -> its ACL table, meters and rules", has(c, r"^ifcs_acl_table_create .*scope=1 ") and len(has(c, r"^ifcs_acl_meter_create")) >= 4
      and len(has(c, r"^ifcs_ace_create")) >= 3, (has(c, r"acl_table_create"), len(has(c, r"acl_meter_create")), len(has(c, r"ace_create"))))
olog = journal("sonic-orchagent")
# not failures of this SAI's areas (as the C++): orchagent's optional PORT_SERDES_ID probe (no port serdes
# objects: ATTR_NOT_SUPPORTED) and VLAN_TAG keep on a VLAN member's netdev (NOT_IMPLEMENTED for an
# adopted netdev, README Profile keys SAI_TERALYNX_NETDEV)
expected = r"getPortSerdesIdFromPortId: Failed to get port serdes ID|setHostIntfsStripTag: Failed to set SAI_HOSTIF_VLAN_TAG_KEEP"
bad = [x for x in olog.split("\n") if re.search(r"(?i)(policer|trap|acl|route|vlan|port|neigh)", x) and re.search(r"(Failed|NOT_IMPLEMENTED|rc=-| ERR )", x)
       and not re.search(expected, x)]
check("orchagent: the expected refusals seen", re.search(expected, olog))
print("orchagent errors on these objects:", bad[:40])
check("orchagent: no SAI failures for ports, VLAN, routes, ACL, CoPP", not bad, bad)

# ------------------------------------------------------------ steady state
rep = restarts()
check("still no restarts", all(s == "active 0" for s in rep.values()), rep)
print("CALL LOG: %d lines; IFCS functions called: %s" % (len(c), sorted({x.split(" ")[0] for x in c if x.startswith("ifcs_")})))
print("SYNCD (tail)")
print("\n".join(journal("sonic-syncd").split("\n")[-60:]))
print("%d failures: %s" % (len(FAILS), FAILS))
assert not FAILS, FAILS
