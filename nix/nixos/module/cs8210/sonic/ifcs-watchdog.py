#!/usr/bin/env python3
"""sonic-cs8210-ifcs-watchdog: restart syncd when the vendor libifcs goes bad.

The learn thread of libifcs.so (_ii_pcie_rupt_thread_learn) trusts one read of
its ring producer index (BAR0 +0x4860). A garbage value (all-ones: a PCIe
read the ASIC did not answer) makes the pending count underflow, im_malloc()
asks for ~32 GiB (malloc, then memset to zero) and the loop never ends
(ai_re/devices/cs8210-32x/reviews/2026-11-10-sdk-learn-crash.md). What the
box sees depends on vm.overcommit_memory:
  0 (vendor stack): malloc fails, the kernel logs "__vm_enough_memory: ...
    comm: ifcs-learn", the thread spins at ~100 % of a core;
  1 (SONiC: the NixOS redis module's vmOverCommit): malloc succeeds, the
    memset touches it within seconds: syncd's MemoryMax (cgroup OOM, systemd
    restarts syncd) or, without one, the global OOM killer (2026-11-16 16:05).
In the 2026-11-16 case the CPU packet path died first: BFD, LACP and the
ASIC's own port TX counters stopped at 16:00:55, the bad read came at 16:05:45
(ai_re/devices/cs8210-32x/oom-20261116.txt).

Signals; each restarts sonic-syncd, at most once per --min-interval (later
hits are only logged):
  kernel     that kernel line for ifcs-learn since syncd started
  vmsize     syncd VmSize >= --vm-limit (the 32 GiB mapping)
  rss        syncd VmRSS >= --rss-limit (a leak, below MemoryMax)
  learn-cpu  the ifcs-learn thread >= --cpu-limit of a core, two polls running
  tx-stall   every oper-up port that was transmitting (COUNTERS_DB OUT
             packets rising) has sent nothing for --stall seconds
Every trigger and every syncd restart also logs the ASIC root port's DevSta /
AER status (its config space is on the SoC: a read never stalls).

  sonic-cs8210-ifcs-watchdog [--interval S] [...]     the daemon
  sonic-cs8210-ifcs-watchdog --once --dry-run          one poll, print the readings
Python stdlib only.
"""
import argparse
import json
import os
import socket
import subprocess
import sys
import time

UNIT = "sonic-syncd.service"
ROOT_PORT = "0001:00:00.0"
DB_CONFIG = "/var/run/redis/sonic-db/database_config.json"
SOCKET = "/var/run/redis/redis.sock"
KERNEL_PATTERN = "comm: ifcs-learn"
LEARN_THREAD = "ifcs-learn"
TX_FIELDS = ("SAI_PORT_STAT_IF_OUT_UCAST_PKTS", "SAI_PORT_STAT_IF_OUT_NON_UCAST_PKTS")


def log(msg):
    print(msg, flush=True)


# ---- /proc ---------------------------------------------------------------

def proc_status(pid, proc="/proc"):
    """VmSize / VmRSS of a process in bytes ({} if it is gone)."""
    out = {}
    try:
        with open("%s/%d/status" % (proc, pid)) as f:
            for line in f:
                k, _, v = line.partition(":")
                if k in ("VmSize", "VmRSS"):
                    out[k] = int(v.split()[0]) * 1024
    except (OSError, ValueError):
        return {}
    return out


def thread_ticks(pid, name, proc="/proc"):
    """utime + stime (clock ticks) summed over the threads called name, None if none."""
    total, seen = 0, False
    try:
        tids = os.listdir("%s/%d/task" % (proc, pid))
    except OSError:
        return None
    for tid in tids:
        try:
            with open("%s/%d/task/%s/stat" % (proc, pid, tid)) as f:
                s = f.read()
        except OSError:
            continue
        lp, rp = s.find("("), s.rfind(")")
        if lp < 0 or rp < 0 or s[lp + 1:rp] != name:
            continue
        rest = s[rp + 2:].split()
        # after "(comm) ": state is field 3, utime 14, stime 15 -> rest[11], rest[12]
        try:
            total += int(rest[11]) + int(rest[12])
            seen = True
        except (IndexError, ValueError):
            continue
    return total if seen else None


# ---- root port (sysfs config space, read only) ---------------------------

def root_port_status(dev=ROOT_PORT, sysfs="/sys/bus/pci/devices"):
    """'root port D: DevSta=0x.... UESta=0x........ CESta=0x........', or why not."""
    try:
        with open("%s/%s/config" % (sysfs, dev), "rb") as f:
            cfg = f.read(4096)
    except OSError as e:
        return "root port %s: %s" % (dev, e)

    def r16(o):
        return int.from_bytes(cfg[o:o + 2], "little")

    def r32(o):
        return int.from_bytes(cfg[o:o + 4], "little")

    parts = []
    p, hops = (cfg[0x34] if len(cfg) > 0x34 else 0), 0
    while p and p + 0x0c <= len(cfg) and hops < 48:
        if cfg[p] == 0x10:  # PCI Express capability: Device Status at +0x0a
            parts.append("DevSta=0x%04x" % r16(p + 0x0a))
            break
        p, hops = cfg[p + 1], hops + 1
    e, hops = 0x100, 0
    while e and e + 0x14 <= len(cfg) and hops < 64:
        h = r32(e)
        if h in (0, 0xFFFFFFFF):
            break
        if h & 0xFFFF == 0x0001:  # AER: Uncorrectable Status +0x04, Correctable Status +0x10
            parts.append("UESta=0x%08x CESta=0x%08x" % (r32(e + 4), r32(e + 0x10)))
            break
        e, hops = (h >> 20) & 0xFFC, hops + 1
    return "root port %s: %s" % (dev, " ".join(parts) if parts else "no PCIe/AER capability read")


# ---- redis (COUNTERS_DB / APPL_DB) ---------------------------------------

class Redis:
    """Just enough RESP2 for SELECT / HGET / HGETALL on a unix socket."""

    def __init__(self, path, timeout=3.0):
        self.s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        self.s.settimeout(timeout)
        self.s.connect(path)
        self.buf = b""

    def close(self):
        self.s.close()

    def _fill(self):
        d = self.s.recv(65536)
        if not d:
            raise OSError("redis closed the connection")
        self.buf += d

    def _line(self):
        while b"\r\n" not in self.buf:
            self._fill()
        line, self.buf = self.buf.split(b"\r\n", 1)
        return line

    def _exact(self, n):
        while len(self.buf) < n + 2:
            self._fill()
        v, self.buf = self.buf[:n], self.buf[n + 2:]
        return v

    def reply(self):
        line = self._line()
        t, rest = line[:1], line[1:]
        if t == b"+":
            return rest.decode()
        if t == b"-":
            raise OSError("redis: " + rest.decode())
        if t == b":":
            return int(rest)
        if t == b"$":
            n = int(rest)
            return None if n < 0 else self._exact(n).decode("utf-8", "replace")
        if t == b"*":
            n = int(rest)
            return None if n < 0 else [self.reply() for _ in range(n)]
        raise OSError("redis: bad reply %r" % line[:40])

    def cmd(self, *args):
        out = b"*%d\r\n" % len(args)
        for a in args:
            a = str(a).encode()
            out += b"$%d\r\n%s\r\n" % (len(a), a)
        self.s.sendall(out)
        return self.reply()


def db_ids(path=DB_CONFIG):
    ids = {"APPL_DB": 0, "COUNTERS_DB": 2, "CONFIG_DB": 4}
    try:
        with open(path) as f:
            dbs = json.load(f).get("DATABASES", {})
        for k in ids:
            if k in dbs:
                ids[k] = int(dbs[k]["id"])
    except (OSError, ValueError, KeyError, TypeError, AttributeError):
        pass
    return ids


def read_ports(sock=SOCKET, db_config=DB_CONFIG):
    """{port: (oper_up, tx_packets or None)} from APPL_DB PORT_TABLE + COUNTERS_DB; None without redis.

    Empty while port counter polling is not enabled (CONFIG_DB FLEX_COUNTER_TABLE|PORT):
    frozen counters then mean nothing.
    """
    ids = db_ids(db_config)
    try:
        r = Redis(sock)
    except OSError:
        return None
    try:
        r.cmd("SELECT", ids["CONFIG_DB"])
        if r.cmd("HGET", "FLEX_COUNTER_TABLE|PORT", "FLEX_COUNTER_STATUS") != "enable":
            return {}
        r.cmd("SELECT", ids["COUNTERS_DB"])
        m = r.cmd("HGETALL", "COUNTERS_PORT_NAME_MAP") or []
        names = dict(zip(m[0::2], m[1::2]))
        tx = {}
        for port, oid in names.items():
            vals = [r.cmd("HGET", "COUNTERS:" + oid, f) for f in TX_FIELDS]
            try:
                tx[port] = sum(int(v) for v in vals)
            except (TypeError, ValueError):
                tx[port] = None
        r.cmd("SELECT", ids["APPL_DB"])
        return {port: (r.cmd("HGET", "PORT_TABLE:" + port, "oper_status") == "up", tx[port]) for port in names}
    except (OSError, ValueError):
        return None
    finally:
        r.close()


# ---- detection (pure; test_ifcs_watchdog.py) -----------------------------

class TxStall:
    """Every port that was transmitting has stopped: the ASIC's CPU path is dead.

    A port counts as transmitting once its TX counter rose in >= min_rises polls
    while oper up; it is stalled when the counter has not moved for stall_s.
    An oper-down port, a counter that went backwards (COUNTERS_DB is flushed at
    every syncd start) or a port that disappears loses its history.
    """

    def __init__(self, stall_s=180, min_rises=3):
        self.stall_s, self.min_rises = stall_s, min_rises
        self.ports = {}  # port -> [last value, time it last rose, rises]

    def reset(self):
        self.ports = {}

    def update(self, now, readings):
        """readings {port: (oper_up, tx or None)} -> (all transmitting ports stalled, those ports)."""
        seen = set()
        for port, (up, tx) in readings.items():
            if not up or tx is None:
                self.ports.pop(port, None)
                continue
            seen.add(port)
            st = self.ports.get(port)
            if st is None or tx < st[0]:
                self.ports[port] = [tx, now, 0]
            elif tx > st[0]:
                st[0], st[1], st[2] = tx, now, st[2] + 1
        for port in list(self.ports):
            if port not in seen:
                del self.ports[port]
        active = sorted(p for p, st in self.ports.items() if st[2] >= self.min_rises)
        stalled = bool(active) and all(now - self.ports[p][1] >= self.stall_s for p in active)
        return stalled, active


class LearnCpu:
    """The ifcs-learn thread at >= limit of a core for a number of polls running."""

    def __init__(self, limit=0.8, polls=2, hz=100):
        self.limit, self.polls, self.hz = limit, polls, hz
        self.last = None  # (pid, t, ticks)
        self.hits = 0

    def update(self, pid, now, ticks):
        """-> (signal, fraction of a core since the last poll or None)."""
        if ticks is None:
            self.last, self.hits = None, 0
            return False, None
        frac = None
        if self.last and self.last[0] == pid and now > self.last[1]:
            frac = (ticks - self.last[2]) / self.hz / (now - self.last[1])
            self.hits = self.hits + 1 if frac >= self.limit else 0
        else:
            self.hits = 0
        self.last = (pid, now, ticks)
        return self.hits >= self.polls, frac


def memory_signals(status, vm_limit, rss_limit):
    """Signal names for a proc_status() dict."""
    sig = []
    if vm_limit and status.get("VmSize", 0) >= vm_limit:
        sig.append("vmsize")
    if rss_limit and status.get("VmRSS", 0) >= rss_limit:
        sig.append("rss")
    return sig


class RestartPolicy:
    """At most one restart per min_interval seconds."""

    def __init__(self, min_interval=900):
        self.min_interval, self.last = min_interval, None

    def allow(self, now):
        if self.last is not None and now - self.last < self.min_interval:
            return False
        self.last = now
        return True


# ---- systemd / journal ---------------------------------------------------

def unit_props(props=("ActiveState", "MainPID", "ActiveEnterTimestampMonotonic")):
    try:
        out = subprocess.run(["systemctl", "show", UNIT] + ["-p" + p for p in props],
                             capture_output=True, text=True, timeout=20).stdout
    except (OSError, subprocess.SubprocessError):
        return {}
    return dict(l.split("=", 1) for l in out.splitlines() if "=" in l)


def kernel_hit(since_epoch):
    try:
        return subprocess.run(["journalctl", "-k", "-q", "--no-pager", "--since", "@%d" % since_epoch,
                               "-g", KERNEL_PATTERN, "-n", "1"],
                              capture_output=True, text=True, timeout=30).stdout.strip()
    except (OSError, subprocess.SubprocessError):
        return ""


def restart_syncd():
    r = subprocess.run(["systemctl", "restart", "--no-block", UNIT], capture_output=True, text=True)
    if r.returncode:
        log("restart of %s failed: %s" % (UNIT, (r.stderr or r.stdout).strip()))


def main(argv=None):
    ap = argparse.ArgumentParser(description="restart syncd when the vendor libifcs goes bad (CS8210)")
    ap.add_argument("--interval", type=float, default=15)
    ap.add_argument("--kernel-every", type=float, default=60, help="seconds between journal checks")
    ap.add_argument("--vm-limit", type=int, default=16 << 30, help="bytes, 0 = off")
    ap.add_argument("--rss-limit", type=int, default=0, help="bytes, 0 = off")
    ap.add_argument("--cpu-limit", type=float, default=0.8)
    ap.add_argument("--stall", type=float, default=180, help="tx-stall seconds, 0 = off")
    ap.add_argument("--min-uptime", type=float, default=300, help="syncd up this long before tx-stall counts")
    ap.add_argument("--min-interval", type=float, default=900)
    ap.add_argument("--once", action="store_true", help="one poll, print the readings")
    ap.add_argument("--dry-run", action="store_true", help="log, never restart")
    a = ap.parse_args(argv)

    stall = TxStall(a.stall) if a.stall > 0 else None
    cpu = LearnCpu(a.cpu_limit, 2, os.sysconf("SC_CLK_TCK"))
    policy = RestartPolicy(a.min_interval)
    last_pid, last_kernel = None, None
    while True:
        now = time.monotonic()
        p = unit_props()
        pid = int(p.get("MainPID") or 0)
        if p.get("ActiveState") != "active" or pid <= 0:
            if stall:
                stall.reset()
            cpu.update(0, now, None)
            if a.once:
                log("syncd not active (%s)" % p.get("ActiveState"))
                return 0
            time.sleep(a.interval)
            continue
        if last_pid is not None and pid != last_pid:
            log("syncd restarted (pid %d -> %d); %s" % (last_pid, pid, root_port_status()))
            if stall:
                stall.reset()
        last_pid = pid
        up_s = now - int(p.get("ActiveEnterTimestampMonotonic") or 0) / 1e6

        sig, detail = [], []
        st = proc_status(pid)
        sig += memory_signals(st, a.vm_limit, a.rss_limit)
        detail.append("VmSize %.2f GiB VmRSS %.2f GiB" % (st.get("VmSize", 0) / 2**30, st.get("VmRSS", 0) / 2**30))
        hit, frac = cpu.update(pid, now, thread_ticks(pid, LEARN_THREAD))
        if hit:
            sig.append("learn-cpu")
        if frac is not None:
            detail.append("%s %.0f%% of a core" % (LEARN_THREAD, 100 * frac))
        if stall:
            ports = read_ports()
            if ports is None:
                detail.append("redis unreadable")
            else:
                stalled, active = stall.update(now, ports)
                detail.append("transmitting ports %s%s" % (",".join(active) or "none", " (all stalled)" if stalled else ""))
                if stalled and up_s >= a.min_uptime:
                    sig.append("tx-stall")
        if a.once or last_kernel is None or now - last_kernel >= a.kernel_every:
            last_kernel = now
            k = kernel_hit(time.time() - up_s)
            if k:
                sig.append("kernel")
                detail.append(k)
        if a.once:
            log("syncd pid %d up %.0f s: %s; signals: %s; %s"
                % (pid, up_s, "; ".join(detail), ",".join(sig) or "none", root_port_status()))
            return 0
        if sig:
            what = "IFCS watchdog: %s (%s); %s" % (",".join(sig), "; ".join(detail), root_port_status())
            if a.dry_run:
                log(what + "; dry run, no restart")
            elif policy.allow(now):
                log(what + "; restarting " + UNIT)
                restart_syncd()
                if stall:
                    stall.reset()
            else:
                log(what + "; restarted less than %d s ago, not again" % a.min_interval)
        time.sleep(a.interval)


if __name__ == "__main__":
    sys.exit(main())
