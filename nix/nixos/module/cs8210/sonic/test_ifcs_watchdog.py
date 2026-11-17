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


# ---- root port registers, ring_stat, state, metrics -----------------------
cfg2 = bytearray(cfg)
cfg2[0x80:0x82] = (0x0c01).to_bytes(2, "little")   # LnkCtl: ASPM L0s (as mainline left the ASIC link)
cfg2[0x82:0x84] = (0x3043).to_bytes(2, "little")   # LnkSta: 8 GT/s x4, DLActive
RING = """Ring Stats
Chip - 0
 Ring           Descriptor              Packets                Bytes                Drops                 Full	Type
 TX-0                    0                    0                    0                    0                    0	USER
 TX-3                30273                10091               964044                    0                   17	KERNEL

 Ring           Descriptor              Packets                Bytes                Drops	Type
 RX-0                 3087                 3085               322403                    2	KERNEL
NA: Data not available in kernel space
"""
with tempfile.TemporaryDirectory() as d:
    os.makedirs(d + "/0001:00:00.0")
    with open(d + "/0001:00:00.0/config", "wb") as f:
        f.write(bytes(cfg2))
    r = wd.root_port_regs("0001:00:00.0", sysfs=d)
    check("root_port_regs", r == {"DevSta": 3, "LnkCtl": 0x0c01, "LnkSta": 0x3043, "UESta": 0x14000, "CESta": 0x3040,
                                  "_exp": 0x70, "_aer": 0x100}, r)
    with open(d + "/ring_stat", "w") as f:
        f.write(RING)
    rs = wd.ring_stat(d + "/ring_stat")
    check("ring_stat TX-3", rs.get("TX-3") == {"descs": 30273, "packets": 10091, "bytes": 964044, "drops": 0, "full": 17}, rs)
    check("ring_stat RX-0 (no Full column)", rs.get("RX-0") == {"descs": 3087, "packets": 3085, "bytes": 322403, "drops": 2}, rs)
    check("tx_ring_line", wd.tx_ring_line(d + "/ring_stat") == "ipd TX-3: packets 10091 full 17 drops 0")
    check("tx_ring_line no ipd", "no ring_stat" in wd.tx_ring_line(d + "/none"))
    sp = d + "/state.json"
    s1 = wd.State(sp, boot="b1")
    s1.trigger(["tx-stall", "vmsize"], 1794873900.0)
    s1.restarts += 1
    s1.save(1794873905.0)
    s2 = wd.State(sp, boot="b1")
    check("state survives a watchdog restart", s2.triggers["tx-stall"] == 1 and s2.triggers["vmsize"] == 1
          and s2.triggers["kernel"] == 0 and s2.restarts == 1 and s2.last_trigger == 1794873900.0, vars(s2))
    s3 = wd.State(sp, boot="b2")
    check("state reset on a new boot", s3.restarts == 0 and s3.triggers["tx-stall"] == 0)
    with open(d + "/guard.prom", "w") as f:
        f.write('# HELP sai_teralynx_ifcs_guard_installed x\n# TYPE sai_teralynx_ifcs_guard_installed gauge\n'
                'sai_teralynx_ifcs_guard_installed 1\nsai_teralynx_ifcs_guard_substituted_total{reg="0x4860"} 2\n'
                'other_family 5\n')
    os.utime(d + "/guard.prom", (1794873955.0, 1794873955.0))
    saved_boot = wd.boot_id
    wd.boot_id = lambda: "b1"
    try:
        m = wd.metrics_text(sp, guard_path=d + "/guard.prom", sysfs=d, ring_path=d + "/ring_stat", now_wall=1794873965.0)
    finally:
        wd.boot_id = saved_boot
    lines_ = m.splitlines()
    check("metrics: triggers by signal", 'cs8210_ifcs_watchdog_triggers_total{signal="tx-stall"} 1' in lines_
          and 'cs8210_ifcs_watchdog_triggers_total{signal="learn-cpu"} 0' in lines_, m)
    check("metrics: restarts, state age", "cs8210_ifcs_watchdog_restarts_total 1" in lines_
          and "cs8210_ifcs_watchdog_state_age_seconds 60" in lines_, m)
    check("metrics: AER bits", 'cs8210_asic_rootport_aer_error{type="uncorrectable",error="CmpltTO"} 1' in lines_
          and 'cs8210_asic_rootport_aer_error{type="uncorrectable",error="UnxCmplt"} 1' in lines_
          and 'cs8210_asic_rootport_aer_error{type="correctable",error="Timeout"} 1' in lines_
          and 'cs8210_asic_rootport_aer_error{type="correctable",error="Rollover"} 0' in lines_, m)
    check("metrics: link + ASPM", "cs8210_asic_link_up 1" in lines_ and "cs8210_asic_link_speed_gts 8" in lines_
          and "cs8210_asic_link_width 4" in lines_ and 'cs8210_asic_link_aspm_enabled{state="L0s"} 1' in lines_, m)
    check("metrics: ipd rings", 'cs8210_ipd_tx_ring_full_total{ring="TX-3"} 17' in lines_
          and 'cs8210_ipd_ring_packets_total{ring="RX-0"} 3085' in lines_, m)
    check("metrics: guard lines passed through, others not", 'sai_teralynx_ifcs_guard_substituted_total{reg="0x4860"} 2' in lines_
          and "other_family 5" not in m, m)
    os.utime(d + "/guard.prom", (1794873965.0 - 300, 1794873965.0 - 300))
    m2 = wd.metrics_text(sp, guard_path=d + "/guard.prom", sysfs=d, ring_path=d + "/ring_stat", now_wall=1794873965.0)
    check("metrics: a stale guard file is not served", "sai_teralynx_ifcs_guard_" not in m2
          and "cs8210_ifcs_guard_file_age_seconds 300" in m2, m2)
    m0 = wd.metrics_text(None, guard_path=d + "/none", sysfs=d + "/none", ring_path=d + "/none")
    check("metrics: nothing readable still prints the counters", "cs8210_ifcs_watchdog_restarts_total 0" in m0
          and "cs8210_asic_rootport_aer_status" not in m0, m0)

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


def run_main(args, props, status, kernel="", ticks=None, ports=None, polls=3, rings=None, guard=None, hold="",
             host=None):
    """rings / guard: a list (one value per poll, the last repeats) or one value."""
    calls = {"restart": 0, "sleeps": 0, "reload": 0}
    saved = {k: getattr(wd, k) for k in ("unit_props", "proc_status", "thread_ticks", "read_ports",
                                         "kernel_hit", "restart_syncd", "root_port_status", "tx_ring_line",
                                         "aer_poll", "ring_stat", "read_guard", "hold_reason", "linux_tx",
                                         "reload_ipd")}

    def seq(v):
        if isinstance(v, list):
            return v[min(calls["sleeps"], len(v) - 1)]
        return v
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
    wd.tx_ring_line = lambda: "ipd TX-3: packets 1 full 0 drops 0"
    wd.aer_poll = lambda *x, **k: []
    wd.ring_stat = lambda *x: seq(rings) or {}
    wd.read_guard = lambda *x: seq(guard)
    wd.hold_reason = lambda *x: hold
    wd.linux_tx = lambda ps, *x: dict(host or {})

    def reload():
        calls["reload"] += 1
        return True
    wd.reload_ipd = reload
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


# ---- guard / cpu-tx signals, AER clear-and-count, hold, escalation ------------
gs = wd.GuardSignal(3)
G0 = {("substituted_total", "0x4860"): 0, ("invalid_total", "0x4860"): 0}
check("guard: first reading, nothing", gs.update(dict(G0)) == (False, 0))
check("guard: one new invalid (recovered) is reported, no signal",
      gs.update({("substituted_total", "0x4860"): 0, ("invalid_total", "0x4860"): 1}) == (False, 1))
res = [gs.update({("substituted_total", "0x4860"): n, ("invalid_total", "0x4860"): 1 + n}) for n in (1, 2, 3)]
check("guard: substituted rising 3 polls -> signal", [r[0] for r in res] == [False, False, True], res)
check("guard: file gone resets", gs.update(None) == (False, 0) and gs.rises == 0)
ct = wd.CpuTx(4)
ring = lambda p, f: {"TX-3": {"descs": 0, "packets": p, "bytes": 0, "drops": 0, "full": f}}
hits = [ct.update(ring(100, f)) for f in (0, 5, 10, 15, 20)]
check("cpu-tx: Full rising, nothing queued, 4 polls -> signal", hits == [False, False, False, False, True], hits)
ct2 = wd.CpuTx(4)
hits = [ct2.update(ring(100 + 50 * i, 5 * i)) for i in range(6)]
check("cpu-tx: backpressure (packets rising too) is not a stall", not any(hits), hits)
check("cpu-tx: no ipd", wd.CpuTx().update({}) is False)

ts = wd.TxStall(stall_s=180)
for i in range(4):
    ts.update(100 + 15 * i, {"Ethernet0": (True, 100 + i)}, {"Ethernet0": 500 + i})
check("tx-stall witness: quiet port (host tx flat too) is not a stall",
      ts.update(400, {"Ethernet0": (True, 103)}, {"Ethernet0": 503}) == (False, ["Ethernet0"]))
check("tx-stall witness: host kept sending, ASIC flat -> stall",
      ts.update(410, {"Ethernet0": (True, 103)}, {"Ethernet0": 540}) == (True, ["Ethernet0"]))

with tempfile.TemporaryDirectory() as d:
    for dev in ("0001:00:00.0", "0002:00:00.0"):
        os.makedirs(d + "/" + dev)
        c = bytearray(cfg2)
        c[0x82:0x84] = (0x3043 | 0x4000).to_bytes(2, "little")   # + LBMS: the link retrained
        with open(d + "/" + dev + "/config", "wb") as f:
            f.write(bytes(c))
    writes = []

    def fake_w1c(dev, off, size, val):
        writes.append((dev, off, size, val))
        path = d + "/" + dev + "/config"
        with open(path, "r+b") as f:
            f.seek(off)
            cur = int.from_bytes(f.read(size), "little")
            f.seek(off)
            f.write((cur & ~val).to_bytes(size, "little"))
    stt = wd.State(None, boot="x")
    ports = (("0001:00:00.0", "asic", True), ("0002:00:00.0", "i2c-fpga", False), ("0009:00:00.0", "gone", True))
    lines_ = wd.aer_poll(stt, 0, ports=ports, sysfs=d, writer=fake_w1c)
    check("aer: one line per port with news", len(lines_) == 2 and "UE:CmpltTO" in lines_[0] and "LnkSta:LBMS" in lines_[0]
          and "cleared" in lines_[0], lines_)
    check("aer: asic port cleared UESta, CESta, LnkSta LBMS, DevSta", ("0001:00:00.0", 0x104, 4, 0x14000) in writes
          and ("0001:00:00.0", 0x110, 4, 0x3040) in writes and ("0001:00:00.0", 0x82, 2, 0x4000) in writes
          and ("0001:00:00.0", 0x7a, 2, 0x3) in writes, writes)
    check("aer: fpga port UESta and DevSta NonFatal kept (cage guard reads them)",
          not any(w[0] == "0002:00:00.0" and w[1] == 0x104 for w in writes)
          and ("0002:00:00.0", 0x7a, 2, 0x1) in writes, writes)
    check("aer: counted", stt.aer.get("asic uncorrectable CmpltTO") == 1 and stt.aer.get("asic link LBMS") == 1
          and stt.aer.get("i2c-fpga uncorrectable UnxCmplt") == 1 and stt.aer.get("asic correctable Timeout") == 1, stt.aer)
    lines2 = wd.aer_poll(stt, 0, ports=ports, sysfs=d, writer=fake_w1c)
    check("aer: second poll: asic quiet (cleared), fpga UE not counted again",
          lines2 == [] and stt.aer.get("i2c-fpga uncorrectable CmpltTO") == 1, (lines2, stt.aer))
    # repeating correctable bits (L0s on): counted every poll, one journal line per 10 min
    def set_ce(v):
        with open(d + "/0001:00:00.0/config", "r+b") as f:
            f.seek(0x110)
            f.write(v.to_bytes(4, "little"))
    logged = []
    for i in range(5):
        set_ce(0x1000)
        logged += wd.aer_poll(stt, 1000 + 15 * i, ports=ports[:1], sysfs=d, writer=fake_w1c)
    set_ce(0x1000)
    logged += wd.aer_poll(stt, 1700, ports=ports[:1], sysfs=d, writer=fake_w1c)
    check("aer: repeating CE logged once per 10 min, counted every poll", len(logged) == 2
          and "seen in 4 polls" in logged[1] and stt.aer.get("asic correctable Timeout") == 1 + 6, (logged, stt.aer))
    os.makedirs(d + "/lease")
    with open(d + "/lease/owner", "w") as f:
        f.write("qos-agent 1794870000\n")
    check("hold: lease", wd.hold_reason(d + "/lease", d + "/nomarker") == "ASIC lease held (qos-agent)")
    open(d + "/marker", "w").close()
    check("hold: reload marker", wd.hold_reason(d + "/nolease", d + "/marker") == "SONiC config reload running")
    check("hold: none", wd.hold_reason(d + "/nolease", d + "/nomarker") == "")
    with open(d + "/g.prom", "w") as f:
        f.write('sai_teralynx_ifcs_guard_substituted_total{reg="0x4860"} 7\nsai_teralynx_ifcs_guard_installed 1\n')
    gv = wd.read_guard(d + "/g.prom", now_wall=os.stat(d + "/g.prom").st_mtime + 5)
    check("read_guard parses", gv == {("substituted_total", "0x4860"): 7.0, ("installed", ""): 1.0}, gv)
    check("read_guard stale -> None", wd.read_guard(d + "/g.prom", now_wall=os.stat(d + "/g.prom").st_mtime + 500) is None)

rv = wd.Recovery(300)
rv.start("restart")
check("recovery: baseline", rv.update(0, True, ring(10, 3)) is None)
check("recovery: CPU TX moving", rv.update(15, True, ring(20, 3)) == "recovered" and rv.kind is None)
rv.start("restart")
rv.update(0, True, ring(10, 3))
check("recovery: still full", rv.update(15, True, ring(10, 9)) is None)
check("recovery: nothing within the window", rv.update(400, True, ring(10, 9)) == "no-tx")

# daemon: dead CPU TX ring -> restart; dead again within 15 min -> ipd reload; again -> alarm
dead = [ring(100, 5 * i) for i in range(40)]
rc, out, calls = run_main([], ACTIVE, NORMAL, rings=dead, polls=17)
check("escalation: restart, then ipd reload, then alarm", calls["restart"] == 1 and calls["reload"] == 1
      and "reloading ipd" in out and "ALARM: still dead" in out, out)
rc, out, calls = run_main([], ACTIVE, NORMAL, rings=dead, polls=7, hold="ASIC lease held (qos)")
check("hold: no action under the lease", calls["restart"] == 0 and calls["reload"] == 0
      and out.count("not acting") == 1, out)
GSUB = [{("substituted_total", "0x4860"): i, ("invalid_total", "0x4860"): i} for i in range(10)]
rc, out, calls = run_main([], ACTIVE, NORMAL, guard=GSUB, polls=5)
check("guard signal restarts syncd", calls["restart"] == 1 and "IFCS watchdog: guard" in out
      and "new invalid" in out, out)

with tempfile.TemporaryDirectory() as d:
    rc, out, calls = run_main(["--state", d + "/s.json"], ACTIVE, BAD, polls=4)
    st = wd.State(d + "/s.json")
    check("daemon counts onsets: 1 vmsize trigger, 1 restart (the condition lasted 4 polls)", st.triggers["vmsize"] == 1
          and st.restarts == 1 and st.suppressed == 0 and st.last_restart > 0, vars(st))
    out2 = io.StringIO()
    with contextlib.redirect_stdout(out2):
        rc = wd.main(["--metrics", "--state", d + "/s.json"])
    check("--metrics from the daemon's state", rc == 0 and 'cs8210_ifcs_watchdog_triggers_total{signal="vmsize"} 1' in out2.getvalue()
          and "cs8210_ifcs_watchdog_restarts_total 1" in out2.getvalue(), out2.getvalue())
    # L3: a restarted watchdog keeps the rate limit (state.last_restart)
    rc, out, calls = run_main(["--state", d + "/s.json"], ACTIVE, BAD, polls=2)
    check("rate limit survives a watchdog restart", calls["restart"] == 0 and "not again" in out, out)

print("%d checks, %d failed" % (COUNT[0], len(FAILS)))
sys.exit(1 if FAILS else 0)
