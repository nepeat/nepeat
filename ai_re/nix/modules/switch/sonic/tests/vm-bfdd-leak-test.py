import json, time

def sh(m, c):
    return m.execute(c)[1]

def rss(m):
    # by path: under qemu-user (ppc64 binfmt) the process name is not "bfdd"
    out = sh(m, "for p in $(pgrep -f 'libexec/frr/bfdd'); do ps -o rss= -p $p; done | sort -n | tail -1").strip()
    return int(out) if out else 0

UNITS = {"bytes": 1, "KiB": 1024, "MiB": 1024 ** 2, "GiB": 1024 ** 3}
def used_heap(mem):
    # FRR "show memory": mallinfo "Used ordinary blocks:  2016 KiB" (bytes)
    for l in mem.splitlines():
        if "Used ordinary blocks" in l:
            v, u = l.split(":")[1].split()
            return int(v) * UNITS.get(u, 1)
    return None

FAILS = []
def check(name, cond, detail=""):
    print(("PASS " if cond else "FAIL ") + name + ("" if cond else " :: " + str(detail)))
    if not cond:
        FAILS.append(name)

def up(m, peer, vrf=None):
    try:
        for s in json.loads(sh(m, "vtysh -c 'show bfd peers json'")):
            if s.get("peer") == peer and s.get("vrf", "default") == (vrf or "default"):
                return s.get("status") == "up", s
    except ValueError:
        pass
    return False, None

def slope(samples):
    # least squares, kB per minute
    n = len(samples)
    xs = [t / 60.0 for t, _ in samples]; ys = [r for _, r in samples]
    mx, my = sum(xs) / n, sum(ys) / n
    den = sum((x - mx) ** 2 for x in xs) or 1
    return sum((x - mx) * (y - my) for x, y in zip(xs, ys)) / den

def phase(name, peers, minutes, also=None):
    for p, v in peers:
        dut.wait_until_succeeds("vtysh -c 'show bfd peers json' | grep -q .", timeout=60)
        t0 = time.time()
        while not up(dut, p, v)[0]:
            if time.time() - t0 > 90:
                print(sh(dut, "vtysh -c 'show bfd peers'"))
                raise Exception("%s: session %s %s not up" % (name, p, v))
            time.sleep(1)
    print("%s: sessions up: %s" % (name, peers))
    time.sleep(30)   # settle
    mem0 = sh(dut, "vtysh -c 'show memory bfdd'")
    samples, psamples = [], []
    t0 = time.time()
    while time.time() - t0 < minutes * 60:
        samples.append((time.time() - t0, rss(dut)))
        if also:
            psamples.append((time.time() - t0, rss(also)))
        time.sleep(30)
    samples.append((time.time() - t0, rss(dut)))
    mem1 = sh(dut, "vtysh -c 'show memory bfdd'")
    try:
        cs = json.loads(sh(dut, "vtysh -c 'show bfd peers counters json'") or "[]")
    except ValueError:
        cs = []
    pk = sum(c.get("control-packet-input", 0) + c.get("control-packet-output", 0) for c in cs)
    s = slope(samples[1:]) if all(r for _, r in samples) else None
    h0, h1 = used_heap(mem0), used_heap(mem1)
    dh = (h1 - h0) if h0 is not None and h1 is not None else None
    # The switch leaked ~2.9 MB/min with 2 sessions: these limits are far below that.
    check("%s: bfdd heap in use (mallinfo) grew < 512 KiB in %d min (%s bytes)" % (name, minutes, dh), dh is not None and dh < 512 * 1024, (h0, h1))
    if s is not None:
        check("%s: bfdd RSS slope < 30 kB/min (%.1f)" % (name, s), s < 30, samples)
    print("%s: bfdd RSS kB %s" % (name, [r for _, r in samples]))
    print("%s: RSS slope %s kB/min over %d min, heap in use %s -> %s bytes, %d control packets in+out total so far" % (name, None if s is None else round(s, 1), minutes, h0, h1, pk))
    if also:
        print("%s: peer (nixpkgs FRR) bfdd RSS kB %s slope %.1f kB/min" % (name, [r for _, r in psamples], slope(psamples)))
    heap = [l for l in (mem0 + "\n----\n" + mem1).splitlines() if "heap" in l.lower() or "ordinary" in l.lower() or l == "----"]
    print("%s: show memory bfdd (before / after):\n%s" % (name, "\n".join(heap)))
    return s

start_all()
dut.wait_for_unit("frr-bfdd.service")
peer.wait_for_unit("frr.service")
print(sh(dut, "ip -br addr; ip vrf"))
res = {}
res["vrf"] = phase("vrf", [("10.0.0.1", None), ("10.0.0.0", "Vrf-test")], MINUTES)
if MODE == "bgp":
    print(sh(dut, "vtysh -c 'show bgp vrf all summary'; vtysh -c 'show bfd peers brief'"))
if TRACE:
    print(sh(dut, "ls -la /var/lib/mt; grep -c '^=== t=' /var/lib/mt/bfdd.mt; p=$(pgrep -x bfdd); echo bfdd pid $p; cat /proc/$p/task/*/comm; grep -E 'Uid|Cap' /proc/$p/status"))
    print("==== malloc-trace, last dump:")
    print(sh(dut, "awk '/^=== /{buf=\"\"} {buf=buf $0 \"\\n\"} END{printf \"%s\", buf}' /var/lib/mt/bfdd.mt | head -150"))
if MODE == "bgp":
    print("RESULT " + json.dumps(res))
    print("FAILED CHECKS: %d %s" % (len(FAILS), FAILS))
    assert not FAILS, FAILS
    raise SystemExit(0)
# no VRF: drop the loop sessions, two sessions to the peer VM in the default VRF
dut.succeed("vtysh -c 'conf t' -c 'bfd' -c 'no peer 10.0.0.1 local-address 10.0.0.0 interface veth0' "
            "-c 'no peer 10.0.0.0 local-address 10.0.0.1 interface veth1 vrf Vrf-test' "
            "-c 'peer 10.1.0.2 local-address 10.1.0.1 interface eth1' -c 'exit' "
            "-c 'peer 10.2.0.2 local-address 10.2.0.1 interface eth2'")
res["novrf"] = phase("novrf", [("10.1.0.2", None), ("10.2.0.2", None)], MINUTES, also=peer)
print("RESULT " + json.dumps(res))
print("FAILED CHECKS: %d %s" % (len(FAILS), FAILS))
assert not FAILS, FAILS
if HEAPTRACK:
    dut.succeed("systemctl stop frr-bfdd.service")
    time.sleep(5)
    print(sh(dut, "ls -la /var/lib/heaptrack"))
    print(sh(dut, "for f in /var/lib/heaptrack/bfdd*; do heaptrack_print -f $f --print-leaks -n 15 -s 0 2>&1 | head -250; done"))
