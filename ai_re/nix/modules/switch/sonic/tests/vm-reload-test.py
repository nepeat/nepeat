import json, time

FAILS = []

def check(name, cond, detail=""):
    d = str(detail)
    print(("PASS " if cond else "FAIL ") + name + ("" if cond else " :: " + (d if len(d) < 2500 else d[:500] + " ... " + d[-2000:])))
    if not cond:
        FAILS.append(name)

def sh(cmd):
    return switch.execute(cmd)

def cli(cmd):
    return sh("bash -lc " + json.dumps(cmd + " 2>&1"))

def db(n, cmd):
    return sh("redis-cli -s /var/run/redis/redis.sock -n %d %s" % (n, cmd))[1].strip()

def wait(name, fn, timeout=120):
    end = time.time() + timeout
    while time.time() < end:
        v = fn()
        if v:
            return v
        time.sleep(1)
    check(name, False, "timed out")
    return None

def members():
    return sh("systemctl show -p ConsistsOf --value sonic.target")[1].split()

def props(units):
    out = {}
    for u in units:
        v = sh("systemctl show -p ActiveState -p MainPID -p InactiveEnterTimestampMonotonic -p ActiveEnterTimestampMonotonic %s" % u)[1]
        out[u] = dict(l.split("=", 1) for l in v.strip().split("\n") if "=" in l)
    return out

def mono(tag):
    # journal monotonic time (us) of the last message with this text
    v = sh("journalctl -b -o json --no-pager -g %s | tail -1" % json.dumps(tag))[1].strip()
    return int(json.loads(v)["__MONOTONIC_TIMESTAMP"]) if v else None

def nloaded():
    return int(sh("journalctl -b -o cat --no-pager -g 'CONFIG_DB loaded' | wc -l")[1].strip() or 0)

def ready():
    switch.wait_for_unit("sonic.target", timeout=300)
    switch.wait_for_unit("sonic-orchagent.service", timeout=300)
    switch.wait_until_succeeds("redis-cli -s /var/run/redis/redis.sock -n 0 exists PORT_TABLE:PortInitDone | grep -q 1", timeout=240)
    switch.wait_until_succeeds("ip link show Ethernet124", timeout=120)

def vrf_ok():
    return (sh("ip link show Ethernet24 | grep -q 'master Vrf-b'")[0] == 0
            and sh("ip -4 addr show Ethernet24 | grep -q 10.9.0.1/31")[0] == 0
            and db(6, "hget 'VRF_OBJECT_TABLE|Vrf-b' state") == "ok"
            and db(0, "exists 'INTF_TABLE:Ethernet24:10.9.0.1/31'") == "1")

N = [0]
def run(label, cmd, strict=True):
    """Runs one reload variant; returns the units that were NOT down across the CONFIG_DB write."""
    N[0] += 1
    mark = "reload-test-%d-start" % N[0]
    units = [u for u in members() if sh("systemctl is-active -q %s" % u)[0] == 0]
    sh("logger -t reload-test " + mark)
    t_start = mono(mark)
    loads = nloaded()
    rc, out = sh(cmd)
    after = props(units)
    print("== %s: %s -> rc %d\n%s" % (label, cmd, rc, out[-1500:]))
    check("%s: command ok" % label, rc == 0, out[-1500:])
    nloads = nloaded()
    check("%s: CONFIG_DB loaded once" % label, nloads == loads + 1, (loads, nloads))
    t_load = mono("CONFIG_DB loaded")
    up = []
    for u in units:
        p = after[u]
        ie, ae = int(p.get("InactiveEnterTimestampMonotonic") or 0), int(p.get("ActiveEnterTimestampMonotonic") or 0)
        if not (t_start < ie < t_load < ae):
            up.append(u)
    print("%s: %d units of sonic.target active before; not down across the load (%d): %s" % (label, len(units), len(up), up))
    ready()
    if strict:
        wait("%s: saved VRF binding back" % label, vrf_ok, 120)
    else:
        end = time.time() + 120
        while not vrf_ok() and time.time() < end:
            time.sleep(1)
        print("%s: saved VRF binding back: %s (not checked: control)" % (label, vrf_ok()))
    return units, up

ready()
switch.succeed("systemctl is-active reload-probe")
# A VRF with a bound, addressed port, saved: what the CS8210's reload raced (intfmgrd vs vrfmgrd, H3).
for c in ["config vrf add Vrf-b", "config interface vrf bind Ethernet7/1 Vrf-b",
          "config interface ip add Ethernet7/1 10.9.0.1/31", "config save -y"]:
    rc, out = cli(c)
    check(c, rc == 0, out)
check("VRF binding up before the reloads", wait("VRF binding up", vrf_ok, 120))

# ---------------------------------------------------------------- negative control: the pre-fix sequence
OLD = "systemctl stop sonic.target && sonic-nix reload --no-service-restart && systemctl start sonic.target"
units, up = run("old sequence", OLD, strict=False)
switch.succeed("touch /run/reload-probe-slow")
units, up = run("old sequence, slow probe", OLD, strict=False)
check("control: the old sequence leaves units up across the load (race reproduced)", len(up) > 0, up)
check("control: syncd and orchagent among them", "sonic-syncd.service" in up and "sonic-orchagent.service" in up, up)

# ---------------------------------------------------------------- sonic-nix reload (the fix)
units, up = run("sonic-nix reload, slow probe", "sonic-nix reload")
check("sonic-nix reload: every sonic.target unit down across the load", up == [], up)
check("sonic-nix reload: syncd among the units", "sonic-syncd.service" in units, units)
switch.succeed("rm /run/reload-probe-slow")
units, up = run("config reload -y -f", "bash -lc 'config reload -y -f 2>&1'")
check("config reload: every sonic.target unit down across the load", up == [], up)
check("no leftover jobs", sh("systemctl list-jobs --no-legend | grep -c sonic")[1].strip() == "0", sh("systemctl list-jobs --no-legend")[1])
check("reload flag removed", sh("test -e /run/sonic-nix/reloading")[0] != 0)

print("FAILED CHECKS: %d %s" % (len(FAILS), FAILS))
assert not FAILS, FAILS
