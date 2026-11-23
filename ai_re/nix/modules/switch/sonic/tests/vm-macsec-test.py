import re, time

FAILS = []
def check(name, cond, detail=""):
    d = str(detail)
    print(("PASS " if cond else "FAIL ") + name + ("" if cond else " :: " + (d if len(d) < 2500 else d[:500] + " ... " + d[-2000:])))
    if not cond:
        FAILS.append(name)

def sh(cmd):
    return switch.execute(cmd)

def wait(desc, f, timeout=60):
    for _ in range(timeout * 2):
        v = f()
        if v:
            return v
        time.sleep(0.5)
    check(desc + " (timed out)", False)
    return None

SWITCH_MAC = "52:54:00:aa:bb:01"
A, B = "Ethernet16", "Ethernet20"         # Ethernet5/1 (no mac_addr), Ethernet6/1 (mac_addr)

def netdev(port):
    return sh("cat /sys/class/net/%s/address" % port)[1].strip()

def mono():
    return float(sh("cut -d' ' -f1 /proc/uptime")[1])

def mgr_log(since):
    """macsecmgrd's journal since a moment on the monotonic clock: [(t, message)]."""
    out = sh("journalctl -b --no-pager -o short-monotonic -u sonic-macsecmgrd")[1]
    lines = []
    for l in out.split("\n"):
        m = re.match(r"\[ *([0-9.]+)\] \S+ (.*)", l)
        if m and float(m.group(1)) >= since:
            lines.append((float(m.group(1)), m.group(2)))
    return lines

def sessions(since, port):
    """Addresses of the MKA sessions started on the port since then, in order."""
    return [m.group(1) for _, l in mgr_log(since)
            for m in [re.search(r"on the port '%s' loading success \(address ([0-9a-f:]+)\)" % port, l)] if m]

def supplicant(port):
    """What the port's wpa_supplicant says about itself: (pid, address)."""
    pid = sh("pgrep -f -- '-g /var/run/%s$' | head -1" % port)[1].strip()
    st = sh("wpa_cli -g /var/run/%s IFNAME=%s status" % (port, port))[1]
    m = re.search(r"^address=([0-9a-f:]+)", st, re.M)
    return pid, (m.group(1) if m else st)

def stable(since, expect):
    """Every session started on the port since then had the expected address, and its supplicant agrees."""
    for port, addr in expect.items():
        s = sessions(since, port)
        check("%s: every MKA session since then started with %s" % (port, addr), len(s) >= 1 and set(s) == {addr}, s)
        check("%s: netdev has %s" % (port, addr), netdev(port) == addr, netdev(port))
        check("%s: wpa_supplicant runs with %s" % (port, addr), supplicant(port)[1] == addr, supplicant(port))

def sa_keys(table, port):
    return [k for k in sh("sonic-db-cli STATE_DB keys '%s|%s|*'" % (table, port))[1].split() if k]

def mka_up(desc, timeout=90):
    """Both ports have an egress SA with the SCI of their own address and an ingress SA with the peer's."""
    def ok():
        for port, peer in ((A, B), (B, A)):
            own, far = netdev(port).replace(":", ""), netdev(peer).replace(":", "")
            if not any(k.split("|")[2].startswith(own) for k in sa_keys("MACSEC_EGRESS_SA_TABLE", port)):
                return False
            if not any(k.split("|")[2].startswith(far) for k in sa_keys("MACSEC_INGRESS_SA_TABLE", port)):
                return False
        return True
    t = time.time()
    r = wait(desc + ": MKA up on both ports (SAs with the SCIs of the addresses the ports have)", ok, timeout)
    if r:
        check(desc + ": MKA up on both ports (SAs with the SCIs of the addresses the ports have)", True)
        print("INFO %s: SAs %.0f s after the check began" % (desc, time.time() - t))
    else:
        print("INFO STATE_DB MACsec objects: %s" % sh("sonic-db-cli STATE_DB keys 'MACSEC*' | sort | tr '\\n' ' '")[1])

def no_duplicate(since):
    out = sh("journalctl -b --no-pager -o short-monotonic -t wpa_supplicant | grep -E 'duplicate MAC|duplicated SCI'")[1]
    late = [l for l in out.split("\n") for m in [re.match(r"\[ *([0-9.]+)\]", l)] if m and float(m.group(1)) >= since]
    check("no 'duplicate MAC' / 'duplicated SCI' from the KaYs", late == [], late[:4])

UNITS = ["sonic-syncd", "sonic-orchagent", "sonic-macsecmgrd", "sonic-intfmgrd"]
def restarts():
    return {u: sh("systemctl show -p ActiveState -p NRestarts --value %s.service | tr '\\n' ' '" % u)[1].strip() for u in UNITS}

switch.start()
switch.wait_for_unit("sonic.target", timeout=300)

with subtest("boot: each session starts with the address its port keeps"):
    wait("both sessions started", lambda: sessions(0, A) and sessions(0, B), 180)
    time.sleep(5)                                   # a wrong start would be followed by a second session
    stable(0, {A: SWITCH_MAC, B: "52:54:00:aa:bb:02"})
    waited = [l for _, l in mgr_log(0) if "waits for the netdev" in l]
    print("INFO sessions that waited for their address at boot: %s" % waited)
    mka_up("boot")
    no_duplicate(0)
    check("no wpa_supplicant unit-level wait is left (ExecStartPre)", "ExecStartPre" not in sh("systemctl cat sonic-macsecmgrd")[1])

with subtest("syncd restart: new netdevs, the sessions start with the right addresses again"):
    before = restarts()
    t = mono()
    switch.succeed("systemctl restart sonic-syncd")
    wait("both sessions started again", lambda: sessions(t, A) and sessions(t, B), 180)
    time.sleep(5)
    stable(t, {A: SWITCH_MAC, B: "52:54:00:aa:bb:02"})
    mka_up("syncd restart")
    no_duplicate(t)
    check("macsecmgrd was started once by the restart", len([1 for _, l in mgr_log(t) if "--- Starting macsecmgrd ---" in l]) == 1)

with subtest("ip link set address: that port's session starts again with the new address, the other is left alone"):
    t = mono()
    pid_b = supplicant(B)[0]
    switch.succeed("ip link set dev %s address 52:54:00:aa:bb:07" % A)
    wait("new session on " + A, lambda: sessions(t, A), 30)
    time.sleep(3)
    stable(t, {A: "52:54:00:aa:bb:07"})
    mka_up("address changed")
    check("the change is logged", any("The address of the port '%s' changed from '%s' to '52:54:00:aa:bb:07'" % (A, SWITCH_MAC) in l for _, l in mgr_log(t)), mgr_log(t))
    check(B + ": no new session", sessions(t, B) == [], sessions(t, B))
    check(B + ": same wpa_supplicant process", pid_b != "" and supplicant(B)[0] == pid_b, (pid_b, supplicant(B)))

with subtest("CONFIG_DB mac_addr changed at run time: the session waits for intfmgrd and starts with the new address"):
    t = mono()
    pid_a = supplicant(A)[0]
    switch.succeed("sonic-db-cli CONFIG_DB hset 'INTERFACE|%s' mac_addr 52:54:00:aa:bb:09" % B)
    wait("netdev has the new address", lambda: netdev(B) == "52:54:00:aa:bb:09", 30)
    wait("new session on " + B, lambda: sessions(t, B), 30)
    time.sleep(3)
    stable(t, {B: "52:54:00:aa:bb:09"})
    mka_up("mac_addr changed")
    no_duplicate(t)
    check(A + ": same wpa_supplicant process", pid_a != "" and supplicant(A)[0] == pid_a, (pid_a, supplicant(A)))

with subtest("nothing died"):
    r = restarts()
    check("units active, not restarted by systemd", all(v.startswith("active 0") for v in r.values()), r)
    print("INFO STATE_DB MACsec objects: %s" % sh("sonic-db-cli STATE_DB keys 'MACSEC*' | sort | tr '\\n' ' '")[1])
    print("INFO wpa_supplicant: %s" % sh("journalctl -b --no-pager -o cat -t wpa_supplicant | grep -v -i -E 'sak|cak|key' | sort | uniq -c | sort -rn | head -12")[1])
    print("INFO orchagent on MACsec: %s" % sh("journalctl -b --no-pager -o cat -u sonic-orchagent | grep -i macsec | sort | uniq -c | sort -rn | head -5")[1])

if FAILS:
    raise Exception("FAILED: " + ", ".join(FAILS))
