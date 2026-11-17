#!/usr/bin/env python3
"""Tests for ifcs-watchdog.py (run at build: python3 -I test_ifcs_watchdog.py ifcs-watchdog.py).

The numbers are the 2026-11-16 16:05 incident's (ai_re/devices/cs8210-32x/oom-20261116.txt):
syncd VmSize 1692496 kB normally, 35246804 kB at the kill; the loop's two ports sent
~5.6 packets/s until 16:00:55 and nothing after.
"""
import importlib.util
import io
import os
import socket
import sys
import tempfile
import threading
import contextlib

spec = importlib.util.spec_from_file_location("wd", sys.argv[1])
wd = importlib.util.module_from_spec(spec)
spec.loader.exec_module(wd)

FAILS = []
COUNT = [0]


def check(name, cond, info=""):
    COUNT[0] += 1
    if not cond:
        FAILS.append(name)
        print("FAIL %s %s" % (name, info))


# ---- /proc parsing --------------------------------------------------------
with tempfile.TemporaryDirectory() as d:
    pid = 4101
    os.makedirs("%s/%d/task" % (d, pid))
    with open("%s/%d/status" % (d, pid), "w") as f:
        f.write("Name:\tsyncd\nVmPeak:\t35246804 kB\nVmSize:\t35246804 kB\nVmRSS:\t5510836 kB\nThreads:\t48\n")
    threads = {4101: ("syncd", 10, 5), 4200: ("ifcs-learn", 7000, 2500), 4201: ("ifcs-pic", 3, 1),
               4202: ("odd ) name", 1, 1), 4203: ("ifcs-learn", 1, 2)}
    for tid, (comm, ut, stt) in threads.items():
        os.makedirs("%s/%d/task/%d" % (d, pid, tid))
        fields = ["S"] + ["0"] * 10 + [str(ut), str(stt)] + ["0"] * 30
        with open("%s/%d/task/%d/stat" % (d, pid, tid), "w") as f:
            f.write("%d (%s) %s\n" % (tid, comm, " ".join(fields)))
    st = wd.proc_status(pid, proc=d)
    check("proc_status VmSize", st.get("VmSize") == 35246804 * 1024, st)
    check("proc_status VmRSS", st.get("VmRSS") == 5510836 * 1024, st)
    check("proc_status gone process", wd.proc_status(999, proc=d) == {})
    check("thread_ticks sums ifcs-learn threads", wd.thread_ticks(pid, "ifcs-learn", proc=d) == 9503,
          wd.thread_ticks(pid, "ifcs-learn", proc=d))
    check("thread_ticks comm with ') '", wd.thread_ticks(pid, "odd ) name", proc=d) == 2)
    check("thread_ticks no such thread", wd.thread_ticks(pid, "ifcs-tx", proc=d) is None)
    check("thread_ticks gone process", wd.thread_ticks(999, "ifcs-learn", proc=d) is None)

# ---- memory signals -------------------------------------------------------
GiB = 1 << 30
check("vmsize at the incident", wd.memory_signals({"VmSize": 35246804 * 1024, "VmRSS": 5510836 * 1024},
                                                  16 * GiB, 0) == ["vmsize"])
check("normal syncd: nothing", wd.memory_signals({"VmSize": 1692496 * 1024, "VmRSS": 332448 * 1024},
                                                16 * GiB, 1536 << 20) == [])
check("rss limit", wd.memory_signals({"VmSize": 3 * GiB, "VmRSS": 1600 << 20}, 16 * GiB, 1536 << 20) == ["rss"])
check("rss off", wd.memory_signals({"VmSize": 3 * GiB, "VmRSS": 9 * GiB}, 16 * GiB, 0) == [])
check("vm off", wd.memory_signals({"VmSize": 40 * GiB}, 0, 0) == [])
check("both", wd.memory_signals({"VmSize": 40 * GiB, "VmRSS": 5 * GiB}, 16 * GiB, 1536 << 20) == ["vmsize", "rss"])

# ---- learn thread CPU -----------------------------------------------------
c = wd.LearnCpu(0.8, 2, hz=100)
check("cpu first poll: no fraction", c.update(1, 0, 100) == (False, None))
check("cpu idle thread", c.update(1, 15, 102) == (False, 2 / 100 / 15))
s1 = c.update(1, 30, 102 + 1500)
check("cpu one spinning poll: not yet", s1[0] is False and abs(s1[1] - 1.0) < 1e-9, s1)
check("cpu two spinning polls: signal", c.update(1, 45, 102 + 3000)[0] is True)
check("cpu pid change resets", c.update(2, 60, 0) == (False, None) and c.update(2, 75, 1500)[0] is False)
check("cpu half a core: no signal", wd.LearnCpu(0.8, 2, 100).update(3, 0, 0) == (False, None))
c2 = wd.LearnCpu(0.8, 2, 100)
c2.update(3, 0, 0)
for t in range(1, 10):
    r = c2.update(3, 15 * t, 750 * t)
check("cpu 50 % for minutes: never", r[0] is False)
check("cpu thread gone resets", c.update(2, 90, None) == (False, None))

# ---- tx stall -------------------------------------------------------------
def incident(stop_at, until, step=15, idle_port=True, other=None):
    """Two loop ports at 5.6 pps until stop_at, then frozen (the 16:00:55 shape)."""
    ts = wd.TxStall(180, 3)
    first = None
    for t in range(0, until, step):
        n = int(5.6 * min(t, stop_at))
        rd = {"Ethernet0": (True, 1000 + n), "Ethernet120": (True, 2000 + n)}
        if idle_port:
            rd["Ethernet8"] = (True, 0)       # oper up, never sends: ignored
        rd["Ethernet16"] = (False, 5)         # oper down: ignored
        if other:
            other(t, rd)
        stalled, active = ts.update(t, rd)
        if stalled and first is None:
            first = (t, active)
    return first


f = incident(600, 1200)
check("tx-stall fires 180 s after the stop", f is not None and 780 <= f[0] <= 795 and f[1] == ["Ethernet0", "Ethernet120"], f)
check("tx-stall: traffic never stops", incident(10 ** 6, 1200) is None)


def one_port_alive(t, rd):
    rd["Ethernet120"] = (True, 2000 + int(5.6 * t))


check("tx-stall: one port still sending", incident(600, 1200, other=one_port_alive) is None)


def flap(t, rd):
    if 600 <= t < 900:
        rd["Ethernet0"] = (False, rd["Ethernet0"][1])
        rd["Ethernet120"] = (False, rd["Ethernet120"][1])


check("tx-stall: ports oper down are not stalled", incident(600, 900, other=flap) is None)


def flushed(t, rd):
    if t >= 600:  # COUNTERS_DB flushed by a syncd start: counters restart from 0 and rise
        rd["Ethernet0"] = (True, int(5.6 * (t - 600)))
        rd["Ethernet120"] = (True, int(5.6 * (t - 600)))


check("tx-stall: counters reset and rising again", incident(600, 1200, other=flushed) is None)
ts = wd.TxStall(180, 3)
check("tx-stall: idle-only ports never fire",
      all(ts.update(t, {"Ethernet8": (True, 7)}) == (False, []) for t in range(0, 900, 15)))
ts = wd.TxStall(180, 3)
for t in range(0, 45, 15):   # first sample + two rises
    ts.update(t, {"Ethernet0": (True, t)})
check("tx-stall: two rises are not enough", ts.update(400, {"Ethernet0": (True, 30)}) == (False, []))
ts = wd.TxStall(180, 3)
for t in range(0, 300, 15):
    ts.update(t, {"Ethernet0": (True, t)})
check("tx-stall: unknown counter drops the port", ts.update(600, {"Ethernet0": (True, None)}) == (False, []))
check("tx-stall: port gone drops it", ts.update(615, {}) == (False, []) and ts.ports == {})

# ---- restart policy -------------------------------------------------------
p = wd.RestartPolicy(900)
check("policy first restart", p.allow(100))
check("policy no second within 900 s", not p.allow(500) and not p.allow(999))
check("policy again after 900 s", p.allow(1000))

# ---- root port status -----------------------------------------------------
cfg = bytearray(4096)
cfg[0x34] = 0x40
cfg[0x40], cfg[0x41] = 0x01, 0x70           # PM cap -> 0x70
cfg[0x70], cfg[0x71] = 0x10, 0x00           # PCIe cap
cfg[0x7a:0x7c] = (0x0003).to_bytes(2, "little")   # DevSta CorrErr+ NonFatalErr+
cfg[0x100:0x104] = (0x0001 | (1 << 16) | (0x148 << 20)).to_bytes(4, "little")  # AER, next 0x148
cfg[0x104:0x108] = ((1 << 14) | (1 << 16)).to_bytes(4, "little")              # CmpltTO+ UnxCmplt+
cfg[0x110:0x114] = ((1 << 6) | (1 << 12) | (1 << 13)).to_bytes(4, "little")    # BadTLP+ Timeout+ AdvNonFatalErr+
with tempfile.TemporaryDirectory() as d:
    os.makedirs(d + "/0001:00:00.0")
    with open(d + "/0001:00:00.0/config", "wb") as f:
        f.write(bytes(cfg))
    s = wd.root_port_status("0001:00:00.0", sysfs=d)
    check("root port status decode", s == "root port 0001:00:00.0: DevSta=0x0003 UESta=0x00014000 CESta=0x00003040", s)
    with open(d + "/0001:00:00.0/config", "wb") as f:
        f.write(bytes(cfg[:64]))  # non-root reader: 64 bytes only
    s = wd.root_port_status("0001:00:00.0", sysfs=d)
    check("root port short config", s.endswith("no PCIe/AER capability read"), s)
    check("root port missing", "No such file" in wd.root_port_status("0009:00:00.0", sysfs=d))

# ---- redis client + read_ports (fake RESP server) -------------------------
DBS = {
    4: {"FLEX_COUNTER_TABLE|PORT": {"FLEX_COUNTER_STATUS": "enable"}},
    2: {"COUNTERS_PORT_NAME_MAP": {"Ethernet0": "oid:0x1000000000002", "Ethernet120": "oid:0x1000000000011"},
        "COUNTERS:oid:0x1000000000002": {"SAI_PORT_STAT_IF_OUT_UCAST_PKTS": "100",
                                         "SAI_PORT_STAT_IF_OUT_NON_UCAST_PKTS": "12140"},
        "COUNTERS:oid:0x1000000000011": {"SAI_PORT_STAT_IF_OUT_UCAST_PKTS": "100"}},
    0: {"PORT_TABLE:Ethernet0": {"oper_status": "up"}, "PORT_TABLE:Ethernet120": {"oper_status": "down"}},
}


def bulk(v):
    if v is None:
        return b"$-1\r\n"
    v = v.encode()
    return b"$%d\r\n%s\r\n" % (len(v), v)


def serve(conn):
    db, buf = 0, b""
    f = conn.makefile("rb")
    while True:
        line = f.readline()
        if not line:
            return
        n = int(line[1:])
        args = []
        for _ in range(n):
            ln = int(f.readline()[1:])
            args.append(f.read(ln + 2)[:-2].decode())
        cmd = args[0].upper()
        if cmd == "SELECT":
            db = int(args[1])
            conn.sendall(b"+OK\r\n")
        elif cmd == "HGET":
            conn.sendall(bulk(DBS.get(db, {}).get(args[1], {}).get(args[2])))
        elif cmd == "HGETALL":
            h = DBS.get(db, {}).get(args[1], {})
            out = b"*%d\r\n" % (2 * len(h))
            for k, v in h.items():
                out += bulk(k) + bulk(v)
            conn.sendall(out)
        else:
            conn.sendall(b"-ERR unknown\r\n")


with tempfile.TemporaryDirectory() as d:
    sp = d + "/redis.sock"
    srv = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    srv.bind(sp)
    srv.listen(4)

    def accept():
        while True:
            try:
                conn, _ = srv.accept()
            except OSError:
                return
            threading.Thread(target=serve, args=(conn,), daemon=True).start()

    threading.Thread(target=accept, daemon=True).start()
    dbc = d + "/database_config.json"
    with open(dbc, "w") as f:
        f.write('{"DATABASES": {"APPL_DB": {"id": 0}, "COUNTERS_DB": {"id": 2}, "CONFIG_DB": {"id": 4}}}')
    rp = wd.read_ports(sp, dbc)
    check("read_ports values", rp == {"Ethernet0": (True, 12240), "Ethernet120": (False, None)}, rp)
    DBS[4]["FLEX_COUNTER_TABLE|PORT"]["FLEX_COUNTER_STATUS"] = "disable"
    check("read_ports: polling off -> no ports", wd.read_ports(sp, dbc) == {})
    del DBS[4]["FLEX_COUNTER_TABLE|PORT"]
    check("read_ports: no flex counter entry -> no ports", wd.read_ports(sp, dbc) == {})
    check("read_ports: no redis", wd.read_ports(d + "/nope.sock", dbc) is None)
    check("db_ids defaults", wd.db_ids(d + "/missing.json") == {"APPL_DB": 0, "COUNTERS_DB": 2, "CONFIG_DB": 4})
    srv.close()

# ---- main: --once and the daemon loop (systemd/journal/proc stubbed) ------
class Stop(Exception):
    pass


def run_main(args, props, status, kernel="", ticks=None, ports=None, polls=3):
    calls = {"restart": 0, "sleeps": 0}
    saved = {k: getattr(wd, k) for k in ("unit_props", "proc_status", "thread_ticks", "read_ports",
                                         "kernel_hit", "restart_syncd", "root_port_status")}
    saved_sleep, saved_mono = wd.time.sleep, wd.time.monotonic
    clock = [10000.0]

    def sleep(s):
        calls["sleeps"] += 1
        clock[0] += s
        if calls["sleeps"] >= polls:
            raise Stop()

    wd.unit_props = lambda: dict(props)
    wd.proc_status = lambda pid: dict(status)
    wd.thread_ticks = lambda pid, name: ticks
    wd.read_ports = lambda: ports
    wd.kernel_hit = lambda since: kernel
    wd.restart_syncd = lambda: calls.__setitem__("restart", calls["restart"] + 1)
    wd.root_port_status = lambda: "root port 0001:00:00.0: UESta=0x00014000"
    wd.time.sleep, wd.time.monotonic = sleep, lambda: clock[0]
    out = io.StringIO()
    try:
        with contextlib.redirect_stdout(out):
            try:
                rc = wd.main(args)
            except Stop:
                rc = None
    finally:
        for k, v in saved.items():
            setattr(wd, k, v)
        wd.time.sleep, wd.time.monotonic = saved_sleep, saved_mono
    return rc, out.getvalue(), calls


ACTIVE = {"ActiveState": "active", "MainPID": "4101", "ActiveEnterTimestampMonotonic": str(int(7400 * 1e6))}
NORMAL = {"VmSize": 1692496 * 1024, "VmRSS": 332448 * 1024}
BAD = {"VmSize": 35246804 * 1024, "VmRSS": 5510836 * 1024}
rc, out, calls = run_main(["--once"], ACTIVE, NORMAL, ports={})
check("--once healthy prints the readings", rc == 0 and "signals: none" in out and "VmSize 1.61 GiB" in out
      and "UESta" in out, out)
rc, out, calls = run_main(["--once"], ACTIVE, BAD, kernel="kernel: __vm_enough_memory: pid: 1, comm: ifcs-learn")
check("--once incident: vmsize + kernel", "signals: vmsize,kernel" in out and calls["restart"] == 0, out)
rc, out, calls = run_main(["--once"], {"ActiveState": "inactive", "MainPID": "0"}, NORMAL)
check("--once inactive", rc == 0 and "not active" in out, out)
rc, out, calls = run_main([], ACTIVE, BAD, polls=4)
check("daemon: one restart, then not again within 900 s", calls["restart"] == 1 and out.count("restarting") == 1
      and out.count("not again") == 3, out)
rc, out, calls = run_main(["--dry-run"], ACTIVE, BAD, polls=3)
check("daemon --dry-run never restarts", calls["restart"] == 0 and out.count("dry run") == 3, out)
rc, out, calls = run_main([], ACTIVE, NORMAL, ports={"Ethernet0": (True, 5)}, polls=5)
check("daemon healthy: silent", calls["restart"] == 0 and out == "", out)
rc, out, calls = run_main(["--rss-limit", str(1536 << 20)], ACTIVE, {"VmSize": 3 * GiB, "VmRSS": 1600 << 20}, polls=2)
check("daemon rss limit restarts", calls["restart"] == 1 and "IFCS watchdog: rss" in out, out)

print("%d checks, %d failed" % (COUNT[0], len(FAILS)))
sys.exit(1 if FAILS else 0)
