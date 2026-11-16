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

def asic_all(key):
    out = db(1, "hgetall %s" % json.dumps(key)).split("\n")
    return dict(zip(out[0::2], out[1::2]))

def wait(desc, f, timeout=60):
    for _ in range(timeout * 2):
        v = f()
        if v:
            return v
        time.sleep(0.5)
    check(desc + " (timed out)", False)
    return None

UNITS = ["sonic-syncd", "sonic-orchagent", "sonic-coppmgrd", "sonic-portmgrd"]
def restarts():
    return {u: sh("systemctl show -p ActiveState -p NRestarts --value %s.service | tr '\\n' ' '" % u)[1].strip() for u in UNITS}

def journal(unit):
    return sh("journalctl -b --no-pager -o cat -u %s" % unit)[1]

start_all()
switch.wait_for_unit("sonic.target", timeout=300)
switch.wait_for_unit("sonic-orchagent.service", timeout=300)
switch.wait_until_succeeds("redis-cli -s /var/run/redis/redis.sock -n 0 exists PORT_TABLE:PortInitDone | grep -q 1", timeout=240)
time.sleep(15)
rep = restarts()
check("units active, no restarts", all(s == "active 0" for s in rep.values()), rep)

# ------------------------------------------------------------------- CoPP
pols = wait("CoPP policers in ASIC_DB", lambda: asic_keys("POLICER"), 60) or []
print("policers:", len(pols))
check("CoPP: one policer per COPP_GROUP with a meter (>= 6)", len(pols) >= 6, pols)
olog = journal("sonic-orchagent")
bad = [l for l in olog.split("\n") if re.search(r"(?i)policer", l) and re.search(r"(Failed|NOT_IMPLEMENTED|rc=-)", l)]
check("CoppOrch: no policer errors", not bad, bad)
groups = asic_keys("HOSTIF_TRAP_GROUP")
policed = [g for g in groups if asic_all(g).get("SAI_HOSTIF_TRAP_GROUP_ATTR_POLICER", "oid:0x0") != "oid:0x0"]
check("trap groups carry their policer", len(policed) >= 6, {g: asic_all(g) for g in groups})
slog = journal("sonic-syncd")
copp = re.findall(r"CoPP: (\d+) field entries, (\d+) policers", slog)
print("syncd CoPP:", copp[-3:])
check("libsai: CoPP field entries for the policed traps", copp and int(copp[-1][0]) >= 4 and int(copp[-1][1]) >= 2, copp)
check("libsai: policers programmed (sr_tcm packets)", "sr_tcm packets cir 600 cbs 600" in slog,
      [l for l in slog.split("\n") if "policer" in l][-10:])

# ---------------------------------------------------------- ACL: CONFIG_DB
st = db(6, "hgetall 'ACL_STAGE_CAPABILITY_TABLE|INGRESS'")
print(st)
check("STATE_DB: ingress ACL actions from the SAI", "PACKET_ACTION" in st, st)
port8 = db(2, "hget COUNTERS_PORT_NAME_MAP Ethernet8")
db(4, "hset 'ACL_TABLE|DROPTEST' type L3 stage ingress policy_desc dropper ports@ Ethernet8")
tables = wait("ACL table in ASIC_DB", lambda: asic_keys("ACL_TABLE"), 60) or []
check("one ACL_TABLE", len(tables) == 1, tables)
if tables:
    t = asic_all(tables[0])
    check("ACL table: ingress, DST_IP/L4 fields", t.get("SAI_ACL_TABLE_ATTR_ACL_STAGE") == "SAI_ACL_STAGE_INGRESS"
          and t.get("SAI_ACL_TABLE_ATTR_FIELD_DST_IP") == "true" and t.get("SAI_ACL_TABLE_ATTR_FIELD_L4_DST_PORT") == "true", t)
    check("advertised match fields kept (OUTER_VLAN_ID, TCP_FLAGS)", t.get("SAI_ACL_TABLE_ATTR_FIELD_OUTER_VLAN_ID") == "true"
          and t.get("SAI_ACL_TABLE_ATTR_FIELD_TCP_FLAGS") == "true", t)
grp = wait("ACL table group", lambda: asic_keys("ACL_TABLE_GROUP"), 30) or []
check("ACL table group + member", len(grp) == 1 and len(asic_keys("ACL_TABLE_GROUP_MEMBER")) == 1,
      (grp, asic_keys("ACL_TABLE_GROUP_MEMBER")))
pkey = "ASIC_STATE:SAI_OBJECT_TYPE_PORT:" + port8
bound = wait("Ethernet8 INGRESS_ACL", lambda: asic_all(pkey).get("SAI_PORT_ATTR_INGRESS_ACL"), 30)
check("Ethernet8 bound to the table group", bound and grp and bound == grp[0].split(":", 2)[2], (bound, grp))

db(4, "hset 'ACL_RULE|DROPTEST|R1' PRIORITY 9999 PACKET_ACTION DROP DST_IP 10.9.9.9/32")
ents = wait("ACL entry in ASIC_DB", lambda: asic_keys("ACL_ENTRY"), 60) or []
check("one ACL_ENTRY", len(ents) == 1, ents)
if ents:
    e = asic_all(ents[0])
    print(e)
    check("entry: DST_IP 10.9.9.9/32, DROP, counter, priority",
          "10.9.9.9" in e.get("SAI_ACL_ENTRY_ATTR_FIELD_DST_IP", "") and "255.255.255.255" in e.get("SAI_ACL_ENTRY_ATTR_FIELD_DST_IP", "")
          and e.get("SAI_ACL_ENTRY_ATTR_ACTION_PACKET_ACTION") == "SAI_PACKET_ACTION_DROP"
          and e.get("SAI_ACL_ENTRY_ATTR_ACTION_COUNTER", "").startswith("oid:")
          and e.get("SAI_ACL_ENTRY_ATTR_PRIORITY") == "9999", e)
check("ACL counter object", len(asic_keys("ACL_COUNTER")) == 1, asic_keys("ACL_COUNTER"))
state = wait("STATE_DB rule status", lambda: db(6, "hget 'ACL_RULE_TABLE|DROPTEST|R1' status"), 30)
check("STATE_DB ACL_RULE_TABLE status Active", state == "Active", state)
slog = journal("sonic-syncd")
inst = [l for l in slog.split("\n") if "ACL entry: SDK entry" in l]
check("libsai: SDK field entry installed on 1 port, drop, counted", inst and "1 ports, drop, counted" in inst[-1], inst)
rc, out = cli("sudo counterpoll acl enable")   # ACL_STAT_COUNTER group is off by default
check("counterpoll acl enable", rc == 0 and db(4, "hget 'FLEX_COUNTER_TABLE|ACL' FLEX_COUNTER_STATUS") == "enable", out)
kname = wait("ACL counter polled into COUNTERS_DB", lambda: [k for k in db(2, "--scan --pattern 'COUNTERS:oid:*'").split("\n")
                                                            if k and db(2, "hexists %s SAI_ACL_COUNTER_ATTR_PACKETS" % json.dumps(k)) == "1"], 60)
check("syncd polls SAI_ACL_COUNTER_ATTR_PACKETS", bool(kname), kname)
# show acl / aclshow: acl-loader needs pyangbind, aclshow is not installed (packaging gaps, reported)
rc, out = cli("show acl rule")
print("INFO show acl rule rc=%d: %s" % (rc, out[-300:]))

# ------------------------------------------------------------ ACL: the CLI
rc, out = cli("sudo config acl add table V6TEST L3V6 -p Ethernet12 -s ingress")   # SONiC names (aliases refused)
print(out)
check("config acl add table (L3V6)", rc == 0, out)
t2 = wait("second ACL table", lambda: len(asic_keys("ACL_TABLE")) == 2, 60)
check("ASIC_DB: 2 ACL tables", t2, asic_keys("ACL_TABLE"))
t6 = [asic_all(k) for k in asic_keys("ACL_TABLE")]
check("L3V6 table: DST_IPV6 field", any(t.get("SAI_ACL_TABLE_ATTR_FIELD_DST_IPV6") == "true" for t in t6), t6)

# --------------------------------------------------------------- teardown
db(4, "del 'ACL_RULE|DROPTEST|R1'")
wait("ACL entry removed", lambda: not asic_keys("ACL_ENTRY") and not asic_keys("ACL_COUNTER"), 60)
check("entry + counter removed", not asic_keys("ACL_ENTRY") and not asic_keys("ACL_COUNTER"))
db(4, "del 'ACL_TABLE|DROPTEST'")
rc, out = cli("sudo config acl remove table V6TEST")
check("config acl remove table", rc == 0, out)
wait("ACL tables removed", lambda: not asic_keys("ACL_TABLE"), 60)
check("tables, groups removed; Ethernet8 unbound", not asic_keys("ACL_TABLE") and not asic_keys("ACL_TABLE_GROUP")
      and asic_all(pkey).get("SAI_PORT_ATTR_INGRESS_ACL", "oid:0x0") == "oid:0x0", asic_all(pkey))
olog = journal("sonic-orchagent")
bad = [l for l in olog.split("\n") if re.search(r"(?i)acl", l) and re.search(r"(ERR|Failed)", l)]
check("orchagent: no ACL errors", not bad, bad)
rep = restarts()
check("still no restarts", all(s == "active 0" for s in rep.values()), rep)

print("%d failures: %s" % (len(FAILS), FAILS))
assert not FAILS, FAILS
