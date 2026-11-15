import json, re, time

# vm.nix: the virtual SAI; vm-openbcm-fake.nix prepends SAI = "openbcm-fake"
# (sai-openbcm's fake backend: no breakout, TAP netdevs, CPU-path "wire").
SAI = globals().get("SAI", "vs")
FAKE = SAI == "openbcm-fake"
FAILS = []
NA = []
def na(name, why):
    print("N/A  " + name + " :: " + why)
    NA.append(name)

def check(name, cond, detail=""):
    d = str(detail)
    print(("PASS " if cond else "FAIL ") + name + ("" if cond else " :: " + (d if len(d) < 2500 else d[:500] + " ... " + d[-2000:])))
    if not cond:
        FAILS.append(name)

def sh(cmd, m=None):
    m = m or switch
    rc, out = m.execute(cmd)
    return rc, out

def cli(cmd):
    # login shell: SONIC_CLI_IFACE_MODE=alias from environment.variables
    return sh("bash -lc " + json.dumps(cmd + " 2>&1"))

def db(n, cmd):
    return sh("redis-cli -s /var/run/redis/redis.sock -n %d %s" % (n, cmd))[1].strip()

UNITS = ["sonic-tlm_teamd", "redis-sonic", "sonic-config-loader", "sonic-nix-apply", "sonic-nix-watch"] + ([] if FAKE else ["sonic-vs-ports"]) + [
         "sonic-syncd", "sonic-portsyncd", "sonic-orchagent", "sonic-portmgrd", "sonic-intfmgrd",
         "sonic-vlanmgrd", "sonic-nbrmgrd", "sonic-teammgrd", "sonic-teamsyncd", "sonic-neighsyncd",
         "sonic-fdbsyncd", "sonic-coppmgrd"]

def units_report():
    out = {}
    for u in UNITS:
        rc, o = sh("systemctl show -p ActiveState -p SubState -p NRestarts --value %s.service | tr '\\n' ' '" % u)
        out[u] = o.strip()
    return out

def diag(tag):
    print("==== diag " + tag)
    print(json.dumps(units_report(), indent=1))
    print(sh("journalctl -b --no-pager -o cat -p warning | tail -60")[1])

def wait_ports_ready():
    switch.wait_until_succeeds("redis-cli -s /var/run/redis/redis.sock -n 0 exists PORT_TABLE:PortInitDone | grep -q 1", timeout=240)
    switch.wait_until_succeeds("ip link show Ethernet124", timeout=120)

start_all()
switch.wait_for_unit("sonic.target", timeout=300)
switch.wait_for_unit("sonic-orchagent.service", timeout=300)
try:
    wait_ports_ready()
except Exception:
    diag("ports not ready")
    print(sh("journalctl -b --no-pager -u sonic-orchagent -u sonic-syncd -u sonic-portsyncd | tail -150")[1])
    raise

# ---------------------------------------------------------------- 1. services and ports
time.sleep(20)
rep = units_report()
print(json.dumps(rep, indent=1))
bad = {u: s for u, s in rep.items() if not s.startswith("active") or not s.endswith(" 0")}
check("all units active, no restarts", not bad, bad)

# vs: cage 1 split 4x25G (35 ports); fake: 32 x 100G (no breakout in the SAI).
NPORTS = 32 if FAKE else 35
ports_cfg = sorted(k for k in db(4, "--scan --pattern 'PORT|*'").split())
check("CONFIG_DB: %d ports" % NPORTS, len(ports_cfg) == NPORTS, ports_cfg)
appl = db(0, "--scan --pattern 'PORT_TABLE:Ethernet*'").split()
check("APPL_DB: %d ports" % NPORTS, len(appl) == NPORTS, appl)
asic_ports = int(db(1, "--scan --pattern 'ASIC_STATE:SAI_OBJECT_TYPE_PORT:*' | wc -l") or 0)
check("ASIC_DB: %d ports (+ CPU)" % (NPORTS + 1), asic_ports == NPORTS + 1, asic_ports)
PORTS_EXPECT = [("Ethernet8", "57,58,59,60", "Ethernet3/1", "100000"), ("Ethernet124", "13,14,15,16", "Ethernet32/1", "100000")]
if FAKE:
    PORTS_EXPECT = [("Ethernet0", "49,50,51,52", "Ethernet1/1", "100000")] + PORTS_EXPECT
    na("4x25G split ports Ethernet0..3", "breakout is config.bcm's job; sai-openbcm creates no ports")
else:
    PORTS_EXPECT = [("Ethernet0", "49", "Ethernet1/1", "25000"), ("Ethernet1", "50", "Ethernet1/2", "25000"),
                    ("Ethernet3", "52", "Ethernet1/4", "25000")] + PORTS_EXPECT
for n, lanes, alias, speed in PORTS_EXPECT:
    got = db(0, "hmget PORT_TABLE:%s lanes alias speed" % n).split()
    check("APPL_DB %s lanes/alias/speed" % n, got == [lanes, alias, speed], got)
    check("kernel netdev %s" % n, sh("ip link show %s" % n)[0] == 0)
# The virtual SAI creates its ports from the generated lanemap.ini; orchagent
# binds each CONFIG_DB port to the SAI port with the same lanes. A lane
# mismatch would make orchagent remove/create ports (HW_LANE_LIST in ASIC_DB).
pmap = sh("redis-cli -s /var/run/redis/redis.sock -n 2 hgetall COUNTERS_PORT_NAME_MAP")[1].split()
names = dict(zip(pmap[0::2], pmap[1::2]))
check("COUNTERS_PORT_NAME_MAP: %d ports, distinct SAI objects" % NPORTS, len(names) == NPORTS and len(set(names.values())) == NPORTS, names)
created = db(1, "--scan --pattern 'ASIC_STATE:SAI_OBJECT_TYPE_PORT:*' | xargs -r -n1 redis-cli -s /var/run/redis/redis.sock -n 1 hget | grep -c . || true")
lanes_set = sh("for k in $(redis-cli -s /var/run/redis/redis.sock -n 1 --scan --pattern 'ASIC_STATE:SAI_OBJECT_TYPE_PORT:*'); do "
               "redis-cli -s /var/run/redis/redis.sock -n 1 hexists $k SAI_PORT_ATTR_HW_LANE_LIST; done | grep -c 1 || true")[1].strip()
check("orchagent matched every port by lanes (no port re-created)", lanes_set == "0", lanes_set)
if FAKE:
    na("lanemap.ini", "virtual SAI only")
    sp = sh("cat /usr/share/sonic/hwsku/sai.profile")[1]
    check("sai.profile selects the fake backend", "SAI_OPENBCM_BACKEND=fake" in sp, sp)
    check("syncd log: FAKE backend, 32 front-panel ports", sh("journalctl -b -u sonic-syncd --no-pager | grep -q 'switch created: 32 front-panel ports, backend fake'")[0] == 0,
          sh("journalctl -b -u sonic-syncd --no-pager | tail -20")[1])
else:
    lm = sh("cat /usr/share/sonic/hwsku/lanemap.ini")[1]
    check("lanemap: split lanes 49..52 one per port, cage 3 = 57-60", all(("sw%d:%d" % (i, 49 + i)) in lm for i in range(4)) and "sw8:57,58,59,60" in lm and "eth1:65,66,67,68" in lm, lm[:300])
print(db(1, "hgetall ASIC_STATE:SAI_OBJECT_TYPE_PORT:" + names.get("Ethernet1" if not FAKE else "Ethernet0", "x")))

check("DEVICE_METADATA hostname mirrored from networking.hostName", db(4, "hget 'DEVICE_METADATA|localhost' hostname") == "switch")
check("fake-asic-temp stopped while syncd runs (Conflicts)", sh("systemctl is-active fake-asic-temp")[1].strip() != "active" and sh("systemctl is-active sonic-syncd")[1].strip() == "active")

rc, out = cli("show interfaces status")
print(out)
check("show interfaces status lists aliases", rc == 0 and "Ethernet1/1" in out and ("Ethernet1/4" in out or FAKE) and "Ethernet32/1" in out, out)
# intfutil shows both: SONiC name first, the alias in the Alias column.
if FAKE:
    row = [l.split() for l in out.splitlines() if "Ethernet2/1" in l.split()]
    check("show interfaces status: Ethernet2/1 = Ethernet4, lanes 53-56, 100G", row and row[0][:3] == ["Ethernet4", "53,54,55,56", "100G"], row)
    rc, out = cli("show interfaces alias")
    print(out)
    check("show interfaces alias maps Ethernet4 -> Ethernet2/1", re.search(r"Ethernet4\s+Ethernet2/1", out) is not None, out)
else:
    row = [l.split() for l in out.splitlines() if "Ethernet1/2" in l.split()]
    check("show interfaces status: Ethernet1/2 = Ethernet1, lane 50, 25G", row and row[0][:3] == ["Ethernet1", "50", "25G"], row)
    rc, out = cli("show interfaces alias")
    print(out)
    check("show interfaces alias maps Ethernet1 -> Ethernet1/2", re.search(r"Ethernet1\s+Ethernet1/2", out) is not None, out)

# ---------------------------------------------------------------- 2. admin state by alias
rc, out = cli("config interface startup Ethernet3/1")
check("config interface startup Ethernet3/1", rc == 0, out)
switch.wait_until_succeeds("redis-cli -s /var/run/redis/redis.sock -n 0 hget PORT_TABLE:Ethernet8 admin_status | grep -qx up", timeout=30)
check("CONFIG_DB PORT|Ethernet8 admin up", db(4, "hget 'PORT|Ethernet8' admin_status") == "up")
oid = db(2, "hget COUNTERS_PORT_NAME_MAP Ethernet8")
switch.wait_until_succeeds("ip link show Ethernet8 | grep -q ',UP'", timeout=30)
if oid:
    check("ASIC_DB Ethernet8 ADMIN_STATE true", db(1, "hget ASIC_STATE:SAI_OBJECT_TYPE_PORT:%s SAI_PORT_ATTR_ADMIN_STATE" % oid) == "true")
rc, out = cli("config interface shutdown Ethernet3/1")
check("config interface shutdown Ethernet3/1", rc == 0, out)
switch.wait_until_succeeds("redis-cli -s /var/run/redis/redis.sock -n 0 hget PORT_TABLE:Ethernet8 admin_status | grep -qx down", timeout=30)
rc, out = cli("show interfaces status Ethernet3/1")
check("show interfaces status Ethernet3/1: down", rc == 0 and re.search(r"Ethernet3/1.*down\s+down", out) is not None, out)

# alias ranges and lists (patch 0003)
def admin(pnames):
    return [db(4, "hget 'PORT|%s' admin_status" % n) for n in pnames]
RANGES = [("Ethernet1/1-3/1", ["Ethernet0", "Ethernet4", "Ethernet8"]), ("Ethernet9/1,Ethernet10/1", ["Ethernet32", "Ethernet36"])] if FAKE else [
    ("Ethernet1/1-4", ["Ethernet0", "Ethernet1", "Ethernet2", "Ethernet3"]),
    ("Ethernet1/3-3/1", ["Ethernet2", "Ethernet3", "Ethernet4", "Ethernet8"]),
    ("Ethernet9/1,Ethernet10/1", ["Ethernet32", "Ethernet36"])]
for rng, pnames in RANGES:
    rc, out = cli("config interface startup " + rng)
    check("config interface startup %s" % rng, rc == 0 and admin(pnames) == ["up"] * len(pnames), (out, admin(pnames)))
    rc, out = cli("config interface shutdown " + rng)
    check("config interface shutdown %s" % rng, rc == 0 and admin(pnames) == ["down"] * len(pnames), (out, admin(pnames)))
check("ranges wrote no alias keys", db(4, "keys 'PORT|Ethernet*/*'") == "", db(4, "keys 'PORT|Ethernet*/*'"))
if FAKE:
    na("Ethernet1/3-3/1 left Ethernet1/1, 1/2 alone", "no split cage")
else:
    check("Ethernet1/3-3/1 left Ethernet1/1, 1/2 alone", admin(["Ethernet0", "Ethernet1"]) == ["down", "down"])

# ---------------------------------------------------------------- 3. VLAN with a member by alias
for c in ["config vlan add 100", "config vlan member add -u 100 Ethernet4/1", "config interface startup Ethernet4/1"]:
    rc, out = cli(c)
    check(c, rc == 0, out)
check("CONFIG_DB VLAN_MEMBER|Vlan100|Ethernet12", db(4, "hget 'VLAN_MEMBER|Vlan100|Ethernet12' tagging_mode") == "untagged")
switch.wait_until_succeeds("bridge vlan show dev Ethernet12 | grep -q 100", timeout=60)
rc, out = cli("show vlan brief")
print(out)
check("show vlan brief: 100 with Ethernet4/1", "100" in out and "Ethernet4/1" in out, out)
check("Nix-declared Vlan200 in CONFIG_DB", db(4, "hget 'VLAN|Vlan200' vlanid") == "200")
switch.wait_until_succeeds("redis-cli -s /var/run/redis/redis.sock -n 1 --scan --pattern 'ASIC_STATE:SAI_OBJECT_TYPE_VLAN_MEMBER:*' | grep -q .", timeout=60)
rc, out = cli("config interface ip add Ethernet7/1 10.1.0.1/24")
check("config interface ip add Ethernet7/1 10.1.0.1/24", rc == 0, out)
check("INTERFACE key uses the SONiC name (Ethernet24)", db(4, "exists 'INTERFACE|Ethernet24|10.1.0.1/24'") == "1", db(4, "keys 'INTERFACE*'"))
switch.wait_until_succeeds("ip addr show Ethernet24 | grep -q 10.1.0.1/24", timeout=60)

# ---------------------------------------------------------------- 4. PortChannel, LACP to the peer
for c in ["config portchannel add PortChannel0001", "config portchannel member add PortChannel0001 Ethernet5/1",
          "config portchannel member add PortChannel0001 Ethernet6/1", "config interface startup Ethernet5/1",
          "config interface startup Ethernet6/1", "config interface ip add PortChannel0001 10.0.0.1/24"]:
    rc, out = cli(c)
    check(c, rc == 0, out)
try:
    switch.wait_until_succeeds("pgrep -f 'teamd.*PortChannel0001'", timeout=60)
    check("teamd running for PortChannel0001", True)
except Exception:
    check("teamd running for PortChannel0001", False, sh("journalctl -b -u sonic-teammgrd --no-pager | tail -40")[1])
lacp_ok = True
try:
    switch.wait_until_succeeds("teamdctl PortChannel0001 state item get ports.Ethernet16.runner.selected | grep -qx true && teamdctl PortChannel0001 state item get ports.Ethernet20.runner.selected | grep -qx true", timeout=120)
except Exception:
    lacp_ok = False
print(sh("teamdctl PortChannel0001 state")[1])
print(sh("cat /proc/net/bonding/bond0", peer)[1])
check("LACP: both members selected on the switch", lacp_ok, sh("teamdctl PortChannel0001 state dump")[1])
if not lacp_ok:
    print("==== LACP debug")
    print(sh("ip -s link show eth1; ip -s link show eth2; ip -s link show Ethernet16; ip -d link show PortChannel0001")[1])
    print(sh("ip -s link show eth1; ip -s link show bond0", peer)[1])
    print(db(1, "hgetall ASIC_STATE:SAI_OBJECT_TYPE_PORT:" + names.get("Ethernet16", "x")))
    print(sh("journalctl -b --no-pager -u sonic-syncd | grep -i 'eth1\\|Ethernet16' | tail -20")[1])
rc, out = sh("grep -i 'Partner Mac Address' /proc/net/bonding/bond0", peer)
check("peer bond sees the switch MAC as LACP partner", "52:54:00:aa:bb:01" in out, out)
try:
    peer.wait_until_succeeds("ping -c1 -W2 10.0.0.1", timeout=90)
    check("ping across the PortChannel (peer -> 10.0.0.1)", True)
except Exception:
    check("ping across the PortChannel (peer -> 10.0.0.1)", False, sh("ip addr show PortChannel0001; ip neigh")[1])
rc, out = cli("show interfaces portchannel")
print(out)
check("show interfaces portchannel: members by alias", "PortChannel0001" in out and "Ethernet5/1" in out and "Ethernet6/1" in out, out)
check("APPL_DB LAG_TABLE:PortChannel0001", db(0, "exists LAG_TABLE:PortChannel0001") == "1")
check("ASIC_DB LAG members 2", int(db(1, "--scan --pattern 'ASIC_STATE:SAI_OBJECT_TYPE_LAG_MEMBER:*' | wc -l") or 0) == 2)

# ---------------------------------------------------------------- 5. ownership: enforce conflict, seed, host table
rc, out = cli("config interface mtu Ethernet4/1 1500")
check("config interface mtu Ethernet4/1 1500 (seeded field)", rc == 0, out)
# Ethernet1/1's speed is enforced: vs by the breakout, fake by fieldModes (100G cage -> 40G).
SPEED_NIX, SPEED_RT = ("100000", "40000") if FAKE else ("25000", "10000")
rc, out = cli("config interface speed Ethernet1/1 " + SPEED_RT)
check("config interface speed Ethernet1/1 %s (enforced)" % SPEED_RT, rc == 0, out)
check("runtime speed is %s (not reverted live)" % SPEED_RT, db(4, "hget 'PORT|Ethernet0' speed") == SPEED_RT)
rc, out = sh("redis-cli -s /var/run/redis/redis.sock -n 4 hset 'NTP|global' src_intf eth0")
sh("redis-cli -s /var/run/redis/redis.sock -n 4 hset 'DEVICE_METADATA|localhost' hostname renamed")
switch.wait_until_succeeds("grep -q 'PORT' /run/sonic-nix/conflicts.json && grep -q '\"speed\"' /run/sonic-nix/conflicts.json", timeout=30)
c = json.loads(sh("cat /run/sonic-nix/conflicts.json")[1])
print(json.dumps(c, indent=1))
check("watcher: runtime conflict PORT|Ethernet0 speed", any(x["key"] == "Ethernet0" and x["field"] == "speed" and x["source"] == "runtime" for x in c["conflicts"]), c)
check("watcher: host table NTP flagged", any(x["table"] == "NTP" for x in c["conflicts"]), c)
switch.wait_until_succeeds("grep -q '\"hostname\"' /run/sonic-nix/conflicts.json", timeout=30)
c = json.loads(sh("cat /run/sonic-nix/conflicts.json")[1])
check("watcher: hostname change is an enforce conflict", any(x["field"] == "hostname" and x["kind"] == "enforce" and x["current"] == "renamed" for x in c["conflicts"]), c)
check("watcher warning in the journal", sh("journalctl -b -t sonic-nix --no-pager | grep -q 'Ethernet0 speed changed to " + SPEED_RT + "'")[0] == 0,
      sh("journalctl -b -t sonic-nix --no-pager | tail")[1])
rc, out = cli("config save -y")
check("config save -y", rc == 0, out)
saved = json.loads(sh("cat /var/lib/sonic/config_db.json")[1])
check("saved file has VLAN 100, PortChannel0001 and the %s speed" % SPEED_RT, "Vlan100" in saved.get("VLAN", {}) and "PortChannel0001" in saved.get("PORTCHANNEL", {}) and saved["PORT"]["Ethernet0"]["speed"] == SPEED_RT, list(saved))
rc, out = sh("sonic-nix-diff")
print(out)
check("sonic-nix-diff lists PORT|Ethernet0 speed (exit 1)", rc == 1 and "PORT|Ethernet0 speed" in out and "nix=" + SPEED_NIX in out, out)
check("sonic-nix-diff lists the seeded mtu as user value", "Ethernet12 mtu" in out, out)

# ---------------------------------------------------------------- 6. reboot: Nix wins, seed sticks, notices
switch.shutdown()
switch.start()
switch.wait_for_unit("sonic.target", timeout=300)
wait_ports_ready()
check("after reboot: enforced speed back to Nix " + SPEED_NIX, db(4, "hget 'PORT|Ethernet0' speed") == SPEED_NIX, db(4, "hgetall 'PORT|Ethernet0'"))
check("after reboot: APPL_DB speed " + SPEED_NIX, db(0, "hget PORT_TABLE:Ethernet0 speed") == SPEED_NIX)
check("after reboot: seeded mtu 1500 sticks", db(4, "hget 'PORT|Ethernet12' mtu") == "1500")
check("after reboot: VLAN 100 member restored", db(4, "hget 'VLAN_MEMBER|Vlan100|Ethernet12' tagging_mode") == "untagged")
check("after reboot: NTP host table not loaded", db(4, "exists 'NTP|global'") == "0")
check("after reboot: hostname back to networking.hostName", db(4, "hget 'DEVICE_METADATA|localhost' hostname") == "switch")
rc, log = sh("journalctl -b -t sonic-nix --no-pager")
print(log[-3000:])
check("loader warning: saved speed overridden by Nix", "PORT|Ethernet0 speed = %s overridden by Nix" % SPEED_RT in log, log[-2000:])
c = json.loads(sh("cat /run/sonic-nix/conflicts.json")[1])
check("conflicts.json: saved conflict pending", any(x["key"] == "Ethernet0" and x["field"] == "speed" and "saved" in x["source"] for x in c["conflicts"]), c)
issue = sh("cat /etc/issue.d/sonic-nix.issue")[1]
print(issue)
check("/etc/issue.d/sonic-nix.issue shows the conflict", "PORT|Ethernet0 speed" in issue, issue)
# Kernel messages scroll tty1 (console=tty0 in test VMs): silence them, then
# ask agetty to redraw (what the loader does after writing the issue file).
sh("dmesg -n 1; agetty --reload")
tty_ok = True
try:
    switch.wait_until_tty_matches("1", "PORT.Ethernet0 speed", timeout=20)
except Exception:
    tty_ok = False
print("agetty --reload redraw: %s" % tty_ok)
if not tty_ok:
    sh("systemctl restart getty@tty1")
    try:
        switch.wait_until_tty_matches("1", "PORT.Ethernet0 speed", timeout=30)
        tty_ok = True
    except Exception:
        pass
check("agetty on tty1 shows the conflict", tty_ok, switch.get_tty_text("1"))
sh("mkdir -p /root/.ssh && ssh-keygen -q -t ed25519 -N '' -f /root/.ssh/id_ed25519 && cat /root/.ssh/id_ed25519.pub >> /root/.ssh/authorized_keys")
print(sh("grep motd /etc/pam.d/sshd")[1])
# sshd shows the PAM motd for interactive logins only (no command).
rc, out = sh("printf 'exit\\n' | timeout 30 ssh -tt -o BatchMode=yes -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null root@localhost")
print(out)
check("ssh login shows the conflict (motd)", "PORT|Ethernet0 speed" in out, out)
try:
    switch.wait_until_succeeds("teamdctl PortChannel0001 state item get ports.Ethernet16.runner.selected | grep -qx true && teamdctl PortChannel0001 state item get ports.Ethernet20.runner.selected | grep -qx true", timeout=120)
    check("after reboot: LACP up again", True)
    peer.wait_until_succeeds("ping -c1 -W2 10.0.0.1", timeout=60)
except Exception:
    check("after reboot: LACP up again", False, sh("teamdctl PortChannel0001 state")[1])

# ---------------------------------------------------------------- 7. nixos-rebuild switch: apply changed keys only
cli("config vlan add 400")      # unsaved runtime change: must survive the switch
loads_before = sh("journalctl -b -t sonic-nix --no-pager | grep -c 'CONFIG_DB loaded'")[1].strip()
rc, out = sh("/run/booted-system/specialisation/changed/bin/switch-to-configuration test 2>&1")
print(out[-3000:])
check("switch-to-configuration test", rc == 0, out[-1500:])
switch.wait_until_succeeds("redis-cli -s /var/run/redis/redis.sock -n 4 hget 'VLAN|Vlan300' vlanid | grep -qx 300", timeout=30)
check("rebuild: new enforced Vlan300 written", True)
check("rebuild: seed change applied (Ethernet24 mtu 1500)", db(4, "hget 'PORT|Ethernet24' mtu") == "1500")
check("rebuild: seed change does not override the user value (Ethernet12 mtu stays 1500)", db(4, "hget 'PORT|Ethernet12' mtu") == "1500")
check("rebuild: unsaved runtime Vlan400 kept (no reload)", db(4, "exists 'VLAN|Vlan400'") == "1")
check("rebuild: loader not re-run", sh("journalctl -b -t sonic-nix --no-pager | grep -c 'CONFIG_DB loaded'")[1].strip() == loads_before)
check("rebuild: orchagent not restarted", sh("systemctl show -p NRestarts --value sonic-orchagent")[1].strip() == "0")
switch.wait_until_succeeds("redis-cli -s /var/run/redis/redis.sock -n 0 hget PORT_TABLE:Ethernet24 mtu | grep -qx 1500", timeout=30)

# ---------------------------------------------------------------- 8. config reload goes through the loader
rc, out = cli("config reload -y -f")
print(out[-2000:])
check("config reload -y -f", rc == 0, out[-1500:])
check("config reload used sonic-nix", "sonic-nix reload" in out, out[-800:])
switch.wait_for_unit("sonic-orchagent.service", timeout=120)
wait_ports_ready()
check("after reload: Vlan400 gone (not saved), Vlan100 back", db(4, "exists 'VLAN|Vlan400'") == "0" and db(4, "exists 'VLAN|Vlan100'") == "1")

# ---------------------------------------------------------------- 9. sai.restartOnStop
sh("systemctl restart sonic-syncd")
time.sleep(15)
check("syncd restart: fake-asic-temp left stopped", sh("systemctl is-active fake-asic-temp")[1].strip() != "active" and sh("systemctl is-active sonic-syncd")[1].strip() == "active",
      sh("systemctl status fake-asic-temp sonic-syncd --no-pager | head -30")[1])
sh("systemctl stop sonic.target")
try:
    switch.wait_until_succeeds("systemctl is-active fake-asic-temp", timeout=40)
    check("stack stopped: fake-asic-temp started again", True)
except Exception:
    check("stack stopped: fake-asic-temp started again", False, sh("journalctl -b --no-pager | grep -i 'restart-on-stop\\|syncd stopped' | tail")[1])
sh("systemctl start sonic.target")
switch.wait_for_unit("sonic-orchagent.service", timeout=120)
check("stack started: fake-asic-temp stopped again", sh("systemctl is-active fake-asic-temp")[1].strip() != "active")

print("SAI %s: N/A CHECKS: %d %s" % (SAI, len(NA), NA))
print("FAILED CHECKS: %d %s" % (len(FAILS), FAILS))
diag("end")
assert not FAILS, FAILS
