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
  guard      libsai's learn-ring guard substituted 0x4860 in 3 polls running
             (the chip does not answer; the guard keeps libifcs quiet, so this
             is what notices)
  cpu-tx     ipd's CPU TX ring: Full rising, nothing queued, 4 polls running
  tx-stall   every oper-up port that was transmitting (COUNTERS_DB OUT
             packets rising) has sent nothing for --stall seconds while its
             host netdev kept sending (the witness: a quiet port is not a stall)
The CPU-path signals (guard, cpu-tx, tx-stall) escalate: still dead within
--min-interval of a restart -> ipd reload (transient unit) -> ALARM, no more
action until CPU TX is seen moving. Nothing is done while an agent holds the
ASIC lease (/run/asic-lease) or a SONiC config reload runs. Each poll also
counts, logs (dated) and clears the root ports' AER / LnkSta retrain bits.
Every trigger and every syncd restart also logs the ASIC root port's DevSta /
AER status (its config space is on the SoC: a read never stalls) and ipd's
TX-3 ring line (the CPU TX ring: "Full" rising = the ASIC stopped consuming).
Counters (triggers per signal, restarts, suppressed) live in --state (JSON in
/run, kept across watchdog restarts within one boot).

  sonic-cs8210-ifcs-watchdog [--interval S] [...]     the daemon
  sonic-cs8210-ifcs-watchdog --once --dry-run          one poll, print the readings
  sonic-cs8210-ifcs-watchdog --metrics [--state F]     Prometheus text (node-exporter textfile producer):
      the counters, libsai's learn-ring guard file (sai_teralynx_ifcs_guard_*), the root port's AER bits,
      link state and ASPM, ipd's ring counters
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
GUARD_FILE = "/run/sai-teralynx/ifcs-guard.prom"
RING_STAT = "/sys/devices/ipd/ring_stat"
SIGNALS = ("kernel", "vmsize", "rss", "learn-cpu", "tx-stall", "cpu-tx", "guard")
CPU_PATH = ("tx-stall", "cpu-tx", "guard")   # the chip / its CPU path is dead: restart, then ipd reload, then alarm
LEASE_DIR = "/run/asic-lease"                  # an agent's chip test (ai_re cs8210 PROGRESS): never act under it
RELOAD_MARKER = "/run/sonic-nix/reloading"     # services.sonic recovery.nix: a config reload is running
# Root ports polled, cleared and counted (their config space is on the SoC: never stalls). UESta / DevSta error
# bits of the FPGA ports are only counted on a 0->1 change, not cleared: the cage guard and the platform's FPGA
# health check read them to declare the I2C FPGA dead.
ROOT_PORTS = (("0001:00:00.0", "asic", True), ("0002:00:00.0", "i2c-fpga", False), ("0004:00:00.0", "mdio-fpga", False))
UE_BITS = {4: "DLP", 5: "SDES", 12: "PoisonedTLP", 13: "FCP", 14: "CmpltTO", 15: "CmpltAbrt", 16: "UnxCmplt",
           17: "RxOF", 18: "MalfTLP", 19: "ECRC", 20: "UnsupReq"}
CE_BITS = {0: "RxErr", 6: "BadTLP", 7: "BadDLLP", 8: "Rollover", 12: "Timeout", 13: "AdvNonFatalErr",
           14: "CorrIntErr", 15: "HeaderOF"}
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

def root_port_regs(dev=ROOT_PORT, sysfs="/sys/bus/pci/devices"):
    """{DevSta, LnkCtl, LnkSta, UESta, CESta, _exp, _aer} (those found); an OSError if unreadable.

    Reads only the standard header (256 B), the extended capability headers and the AER block (pread), not
    the whole 4 KiB (the DesignWare port-logic registers live above 0x700)."""
    fd = os.open("%s/%s/config" % (sysfs, dev), os.O_RDONLY)
    try:
        cfg = os.pread(fd, 256, 0)

        def r16(o):
            return int.from_bytes(cfg[o:o + 2], "little")

        regs = {}
        p, hops = (cfg[0x34] if len(cfg) > 0x34 else 0), 0
        while p and p + 0x14 <= len(cfg) and hops < 48:
            if cfg[p] == 0x10:  # PCI Express capability: DevSta +0x0a, LnkCtl +0x10, LnkSta +0x12
                regs.update(DevSta=r16(p + 0x0a), LnkCtl=r16(p + 0x10), LnkSta=r16(p + 0x12), _exp=p)
                break
            p, hops = cfg[p + 1], hops + 1
        e, hops = 0x100, 0
        while e and hops < 64:
            hb = os.pread(fd, 4, e)
            if len(hb) < 4:
                break
            h = int.from_bytes(hb, "little")
            if h in (0, 0xFFFFFFFF):
                break
            if h & 0xFFFF == 0x0001:  # AER: Uncorrectable Status +0x04, Correctable Status +0x10
                blk = os.pread(fd, 0x14, e)
                if len(blk) == 0x14:
                    regs.update(UESta=int.from_bytes(blk[4:8], "little"), CESta=int.from_bytes(blk[0x10:0x14], "little"),
                                _aer=e)
                break
            e, hops = (h >> 20) & 0xFFC, hops + 1
        return regs
    finally:
        os.close(fd)


def config_write(dev, off, size, value, sysfs="/sys/bus/pci/devices"):
    """Write-1-to-clear helper: one aligned config write (root ports only)."""
    fd = os.open("%s/%s/config" % (sysfs, dev), os.O_WRONLY)
    try:
        os.pwrite(fd, value.to_bytes(size, "little"), off)
    finally:
        os.close(fd)


def aer_poll(state, now_wall, ports=ROOT_PORTS, sysfs="/sys/bus/pci/devices", writer=None):
    """Count, log and clear the root ports' latched error bits; -> log lines (one per port with news).

    Counted per bit in state.aer["<role> <type> <name>"]. Cleared (W1C): CESta, LnkSta LBMS/LABS (a retrain /
    autonomous bandwidth change), DevSta CorrErr; on the ASIC's port also UESta and DevSta NonFatal/Fatal/UnsupReq,
    so every new CmpltTO is a new count and a new dated line."""
    writer = writer or (lambda d, o, s, v: config_write(d, o, s, v, sysfs))
    lines = []
    for dev, role, clear_ue in ports:
        try:
            r = root_port_regs(dev, sysfs)
        except OSError:
            continue
        names, writes = [], []
        ue, ce = r.get("UESta", 0), r.get("CESta", 0)
        seen = state.aer_seen.setdefault(dev, 0)
        for bit, name in sorted(UE_BITS.items()):
            if (ue >> bit) & 1 and (clear_ue or not (seen >> bit) & 1):
                names.append("UE:" + name)
                state.count_aer("%s uncorrectable %s" % (role, name))
        state.aer_seen[dev] = ue if not clear_ue else 0
        if "_aer" in r and clear_ue and ue:
            writes.append((r["_aer"] + 4, 4, ue))
        for bit, name in sorted(CE_BITS.items()):
            if (ce >> bit) & 1:
                names.append("CE:" + name)
                state.count_aer("%s correctable %s" % (role, name))
        if "_aer" in r and ce:
            writes.append((r["_aer"] + 0x10, 4, ce))
        ls = r.get("LnkSta", 0)
        for bit, name in ((14, "LBMS"), (15, "LABS")):
            if (ls >> bit) & 1:
                names.append("LnkSta:" + name)
                state.count_aer("%s link %s" % (role, name))
        if "_exp" in r and ls & 0xC000:
            writes.append((r["_exp"] + 0x12, 2, ls & 0xC000))
        ds = r.get("DevSta", 0) & (0xF if clear_ue else 0x1)
        if "_exp" in r and ds:
            writes.append((r["_exp"] + 0x0a, 2, ds))
        if not names:
            continue
        ok = True
        for off, size, val in writes:
            try:
                writer(dev, off, size, val)
            except OSError:
                ok = False
        # Journal: always for uncorrectable bits and link retrains; repeating correctable bits (ASPM L0s on:
        # every poll) once per 10 min per port, with how often they were seen meanwhile.
        key = " ".join(names)
        last = _aer_logged.get(dev)
        urgent = any(not n.startswith("CE:") for n in names)
        if urgent or not last or last[0] != key or now_wall - last[1] >= 600:
            rep_ = " (seen in %d polls since the last line)" % last[2] if last and last[2] else ""
            lines.append("PCIe %s root port %s: %s (UESta=0x%08x CESta=0x%08x LnkSta=0x%04x DevSta=0x%04x)%s%s"
                         % (role, dev, key, ue, ce, ls, r.get("DevSta", 0), " cleared" if ok and writes else "", rep_))
            _aer_logged[dev] = [key, now_wall, 0]
        else:
            last[2] += 1
    return lines


_aer_logged = {}   # root port -> [bits last logged, when, polls with the same bits since]


def read_guard(path=GUARD_FILE, now_wall=None, max_age=120):
    """libsai's learn-ring guard counters {(name, reg): value} while its file is fresh, else None."""
    now_wall = time.time() if now_wall is None else now_wall
    try:
        if now_wall - os.stat(path).st_mtime > max_age:
            return None
        out = {}
        with open(path) as f:
            for line in f:
                if not line.startswith("sai_teralynx_ifcs_guard_"):
                    continue
                k, _, v = line.rpartition(" ")
                name, _, rest = k.partition("{")
                reg = rest.partition('"')[2].partition('"')[0] if rest else ""
                try:
                    out[(name[len("sai_teralynx_ifcs_guard_"):], reg)] = float(v)
                except ValueError:
                    pass
        return out
    except OSError:
        return None


class GuardSignal:
    """The guard keeps substituting 0x4860 (the chip does not answer): its counter rose in `polls` polls running."""

    def __init__(self, polls=3):
        self.polls, self.last_sub, self.last_inv, self.rises = polls, None, None, 0

    def reset(self):
        self.last_sub = self.last_inv = None
        self.rises = 0

    def update(self, g):
        """g = read_guard() -> (signal, new invalid reads since the last poll)."""
        if g is None:
            self.last_sub = self.last_inv = None
            self.rises = 0
            return False, 0
        sub = g.get(("substituted_total", "0x4860"), 0)
        inv = sum(v for (n, _), v in g.items() if n in ("invalid_total", "allones_reads_total"))
        new_inv = int(inv - self.last_inv) if self.last_inv is not None and inv > self.last_inv else 0
        if self.last_sub is not None and sub > self.last_sub:
            self.rises += 1
        else:
            self.rises = 0
        self.last_sub, self.last_inv = sub, inv
        return self.rises >= self.polls, new_inv


class CpuTx:
    """ipd's CPU TX ring is stuck: its Full count rose and nothing was queued, `polls` polls running.

    ipd counts TX "packets" when inno_enet_tx queues a frame and "Full" when it returns NETDEV_TX_BUSY; a ring
    the ASIC stopped consuming shows Full rising with packets flat (2026-11-16 16:03:45). Backpressure under load
    shows both rising."""

    def __init__(self, polls=4, ring="TX-3"):
        self.polls, self.ring, self.last, self.hits = polls, ring, None, 0

    def reset(self):
        self.last, self.hits = None, 0

    def update(self, rings):
        r = (rings or {}).get(self.ring)
        if not r or "full" not in r:
            self.last, self.hits = None, 0
            return False
        if self.last and r["full"] > self.last["full"] and r["packets"] == self.last["packets"]:
            self.hits += 1
        else:
            self.hits = 0
        self.last = r
        return self.hits >= self.polls


def hold_reason(lease=LEASE_DIR, marker=RELOAD_MARKER):
    """Why the watchdog must not touch the chip now ("" = it may)."""
    if os.path.isdir(lease):
        try:
            with open(lease + "/owner") as f:
                who = f.read().split()[0]
        except (OSError, IndexError):
            who = "?"
        return "ASIC lease held (%s)" % who
    if os.path.exists(marker):
        return "SONiC config reload running"
    return ""


def linux_tx(ports, sysfs="/sys/class/net"):
    """{port: tx_packets} of the host netdevs (what the kernel handed to ipd)."""
    out = {}
    for p in ports:
        try:
            with open("%s/%s/statistics/tx_packets" % (sysfs, p)) as f:
                out[p] = int(f.read())
        except (OSError, ValueError):
            pass
    return out


def root_port_status(dev=ROOT_PORT, sysfs="/sys/bus/pci/devices"):
    """'root port D: DevSta=0x.... UESta=0x........ CESta=0x........', or why not."""
    try:
        r = root_port_regs(dev, sysfs)
    except OSError as e:
        return "root port %s: %s" % (dev, e)
    parts = []
    if "DevSta" in r:
        parts.append("DevSta=0x%04x" % r["DevSta"])
    if "UESta" in r:
        parts.append("UESta=0x%08x CESta=0x%08x" % (r["UESta"], r["CESta"]))
    return "root port %s: %s" % (dev, " ".join(parts) if parts else "no PCIe/AER capability read")


def ring_stat(path=RING_STAT):
    """ipd's ring counters {"TX-3": {"descs", "packets", "bytes", "drops", "full"}, "RX-0": {...}} (kernel memory, no MMIO)."""
    out = {}
    try:
        with open(path) as f:
            for line in f:
                w = line.split()
                if len(w) >= 5 and (w[0].startswith("TX-") or w[0].startswith("RX-")):
                    try:
                        nums = [int(x) for x in w[1:6 if w[0].startswith("TX-") else 5]]
                    except ValueError:
                        continue
                    keys = ("descs", "packets", "bytes", "drops", "full")
                    out[w[0]] = dict(zip(keys, nums))
    except OSError:
        pass
    return out


def tx_ring_line(path=RING_STAT, ring="TX-3"):
    r = ring_stat(path).get(ring)
    if not r:
        return "ipd %s: no ring_stat" % ring
    return "ipd %s: packets %d full %d drops %d" % (ring, r["packets"], r.get("full", 0), r["drops"])


# ---- counters (--state) and metrics (--metrics) ---------------------------

def boot_id():
    try:
        with open("/proc/sys/kernel/random/boot_id") as f:
            return f.read().strip()
    except OSError:
        return ""


class State:
    """Trigger / restart counters, saved as JSON so a watchdog restart keeps them (same boot only)."""

    INTS = ("restarts", "suppressed", "held", "escalations", "recoveries", "alarm", "level")
    FLOATS = ("last_trigger", "last_restart")

    def __init__(self, path=None, boot=None):
        self.path, self.boot = path, boot if boot is not None else boot_id()
        self.triggers = {s: 0 for s in SIGNALS}
        for k in self.INTS:
            setattr(self, k, 0)
        for k in self.FLOATS:
            setattr(self, k, 0.0)
        self.aer = {}        # "<role> <type> <name>" -> times seen (and cleared)
        self.aer_seen = {}   # root port -> UESta bits already counted (ports whose UESta is not cleared)
        if path:
            try:
                with open(path) as f:
                    d = json.load(f)
                if d.get("boot_id") == self.boot:
                    for s, n in d.get("triggers", {}).items():
                        self.triggers[s] = int(n)
                    for k in self.INTS:
                        setattr(self, k, int(d.get(k, 0)))
                    for k in self.FLOATS:
                        setattr(self, k, float(d.get(k, 0)))
                    self.aer = {k: int(v) for k, v in d.get("aer", {}).items()}
                    self.aer_seen = {k: int(v) for k, v in d.get("aer_seen", {}).items()}
            except (OSError, ValueError, TypeError, AttributeError):
                pass

    def count_aer(self, key):
        self.aer[key] = self.aer.get(key, 0) + 1

    def trigger(self, sigs, now_wall):
        for s in sigs:
            self.triggers[s] = self.triggers.get(s, 0) + 1
        self.last_trigger = now_wall

    def save(self, now_wall):
        if not self.path:
            return
        d = {"boot_id": self.boot, "triggers": self.triggers, "aer": self.aer, "aer_seen": self.aer_seen,
             "updated": now_wall}
        d.update({k: getattr(self, k) for k in self.INTS + self.FLOATS})
        tmp = self.path + ".tmp"
        try:
            with open(tmp, "w") as f:
                json.dump(d, f)
            os.replace(tmp, self.path)
        except OSError:
            pass


def metrics_text(state_path, guard_path=GUARD_FILE, dev=ROOT_PORT, sysfs="/sys/bus/pci/devices",
                 ring_path=RING_STAT, now_wall=None):
    """Prometheus text for the node-exporter textfile producer."""
    now_wall = time.time() if now_wall is None else now_wall
    out = []

    def fam(name, typ, help_):
        out.append("# HELP %s %s" % (name, help_))
        out.append("# TYPE %s %s" % (name, typ))

    st, updated = State(state_path, boot=boot_id()) if state_path else State(None, boot=""), None
    if state_path:
        try:
            with open(state_path) as f:
                d = json.load(f)
            if d.get("boot_id") == st.boot:
                updated = float(d.get("updated", 0))
        except (OSError, ValueError, TypeError, AttributeError):
            pass
    fam("cs8210_ifcs_watchdog_triggers_total", "counter", "IFCS watchdog signal onsets (this boot), by signal")
    for s in sorted(st.triggers):
        out.append('cs8210_ifcs_watchdog_triggers_total{signal="%s"} %d' % (s, st.triggers[s]))
    for name, help_ in (("restarts", "syncd restarts by the IFCS watchdog (this boot)"),
                        ("suppressed", "triggers within --min-interval of a restart (logged only)"),
                        ("held", "triggers not acted on: ASIC lease held or a SONiC reload running"),
                        ("escalations", "ipd reloads after a restart did not revive the CPU path"),
                        ("recoveries", "CPU TX seen moving again after a watchdog action")):
        fam("cs8210_ifcs_watchdog_%s_total" % name, "counter", help_)
        out.append("cs8210_ifcs_watchdog_%s_total %d" % (name, getattr(st, name)))
    fam("cs8210_ifcs_watchdog_alarm", "gauge", "1: restart and ipd reload did not revive the chip; the watchdog gave up")
    out.append("cs8210_ifcs_watchdog_alarm %d" % st.alarm)
    fam("cs8210_pcie_aer_events_total", "counter",
        "root-port error bits seen and cleared by the watchdog (FPGA ports' uncorrectable: 0->1 changes)")
    for k in sorted(st.aer):
        role, typ, name = (k.split(" ") + ["", "", ""])[:3]
        out.append('cs8210_pcie_aer_events_total{port="%s",type="%s",error="%s"} %d' % (role, typ, name, st.aer[k]))
    fam("cs8210_ifcs_watchdog_last_trigger_timestamp_seconds", "gauge", "time of the last trigger (0: none this boot)")
    out.append("cs8210_ifcs_watchdog_last_trigger_timestamp_seconds %.0f" % st.last_trigger)
    if updated:
        fam("cs8210_ifcs_watchdog_state_age_seconds", "gauge", "seconds since the watchdog last saved its state")
        out.append("cs8210_ifcs_watchdog_state_age_seconds %.0f" % max(0.0, now_wall - updated))

    try:
        r = root_port_regs(dev, sysfs)
    except OSError:
        r = {}
    if "UESta" in r:
        fam("cs8210_asic_rootport_aer_status", "gauge", "ASIC root port AER status registers (raw, sticky until cleared)")
        out.append('cs8210_asic_rootport_aer_status{reg="uncorrectable"} %d' % r["UESta"])
        out.append('cs8210_asic_rootport_aer_status{reg="correctable"} %d' % r["CESta"])
        fam("cs8210_asic_rootport_aer_error", "gauge", "ASIC root port AER status bits (1 = latched)")
        for bit, name in sorted(UE_BITS.items()):
            out.append('cs8210_asic_rootport_aer_error{type="uncorrectable",error="%s"} %d' % (name, (r["UESta"] >> bit) & 1))
        for bit, name in sorted(CE_BITS.items()):
            out.append('cs8210_asic_rootport_aer_error{type="correctable",error="%s"} %d' % (name, (r["CESta"] >> bit) & 1))
    if "LnkSta" in r:
        speed = {1: 2.5, 2: 5.0, 3: 8.0, 4: 16.0}.get(r["LnkSta"] & 0xF, 0)
        fam("cs8210_asic_link_up", "gauge", "ASIC PCIe link: data link layer active (root port LnkSta)")
        out.append("cs8210_asic_link_up %d" % ((r["LnkSta"] >> 13) & 1))
        fam("cs8210_asic_link_speed_gts", "gauge", "ASIC PCIe link speed, GT/s")
        out.append("cs8210_asic_link_speed_gts %g" % speed)
        fam("cs8210_asic_link_width", "gauge", "ASIC PCIe link width")
        out.append("cs8210_asic_link_width %d" % ((r["LnkSta"] >> 4) & 0x3F))
        fam("cs8210_asic_link_aspm_enabled", "gauge", "ASPM states enabled on the root port side of the ASIC link (LnkCtl)")
        out.append('cs8210_asic_link_aspm_enabled{state="L0s"} %d' % (r["LnkCtl"] & 1))
        out.append('cs8210_asic_link_aspm_enabled{state="L1"} %d' % ((r["LnkCtl"] >> 1) & 1))

    rings = ring_stat(ring_path)
    if rings:
        fam("cs8210_ipd_ring_packets_total", "counter", "ipd DMA ring packets (kernel counters)")
        for k in sorted(rings):
            out.append('cs8210_ipd_ring_packets_total{ring="%s"} %d' % (k, rings[k]["packets"]))
        fam("cs8210_ipd_tx_ring_full_total", "counter", "ipd TX ring full events (xmit returned BUSY)")
        for k in sorted(rings):
            if "full" in rings[k]:
                out.append('cs8210_ipd_tx_ring_full_total{ring="%s"} %d' % (k, rings[k]["full"]))

    # libsai's learn-ring guard (sai-teralynx ifcs_guard.c) rewrites its file every 30 s while
    # libifcs' learn thread polls; an older file is a stopped syncd's (or a guard-less libsai's): not served.
    try:
        age = max(0.0, now_wall - os.stat(guard_path).st_mtime)
        fam("cs8210_ifcs_guard_file_age_seconds", "gauge", "age of libsai's learn-ring guard file (served while <= 120 s)")
        out.append("cs8210_ifcs_guard_file_age_seconds %.0f" % age)
        if age <= 120:
            with open(guard_path) as f:
                for line in f:
                    if line.startswith(("sai_teralynx_ifcs_guard_", "# HELP sai_teralynx_ifcs_guard_",
                                        "# TYPE sai_teralynx_ifcs_guard_")):
                        out.append(line.rstrip("\n"))
    except OSError:
        pass
    return "\n".join(out) + "\n"


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
        self.ports = {}  # port -> [last value, time it last rose, rises, host tx when it last rose]

    def reset(self):
        self.ports = {}

    def update(self, now, readings, host=None):
        """readings {port: (oper_up, tx or None)} -> (all transmitting ports stalled, those ports).

        host {port: host netdev tx_packets}: when given, "stalled" also needs a witness that the CPU kept
        sending: some stalled port's host counter rose after its ASIC counter stopped (the kernel handed frames
        to ipd that never left the chip). A port that merely went quiet has neither counter moving."""
        seen = set()
        host = host if host is not None else None
        for port, (up, tx) in readings.items():
            if not up or tx is None:
                self.ports.pop(port, None)
                continue
            seen.add(port)
            st = self.ports.get(port)
            h = host.get(port) if host is not None else None
            if st is None or tx < st[0]:
                self.ports[port] = [tx, now, 0, h]
            elif tx > st[0]:
                st[0], st[1], st[2], st[3] = tx, now, st[2] + 1, h
        for port in list(self.ports):
            if port not in seen:
                del self.ports[port]
        active = sorted(p for p, st in self.ports.items() if st[2] >= self.min_rises)
        stalled = bool(active) and all(now - self.ports[p][1] >= self.stall_s for p in active)
        if stalled and host is not None:
            stalled = any(host.get(p) is not None and self.ports[p][3] is not None and host[p] > self.ports[p][3]
                          for p in active)
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


def run(cmd, timeout=60):
    """-> (ok, output); never raises."""
    try:
        r = subprocess.run(cmd, capture_output=True, text=True, timeout=timeout)
        return r.returncode == 0, (r.stderr or r.stdout).strip()
    except (OSError, subprocess.SubprocessError) as e:
        return False, str(e)


def restart_syncd():
    ok, out = run(["systemctl", "restart", "--no-block", UNIT])
    if not ok:
        log("restart of %s failed: %s" % (UNIT, out))


RELOAD_SCRIPT = ("set -u; systemctl stop sonic.target; "
                 "for i in $(seq 60); do pgrep -x syncd >/dev/null || break; sleep 1; done; "
                 "modprobe -r ipd || echo 'ipd reload: modprobe -r ipd failed' >&2; "
                 "modprobe ipd || echo 'ipd reload: modprobe ipd failed' >&2; "
                 "systemctl start --no-block sonic.target")


def reload_ipd():
    """The escalation step: SONiC down, ipd unloaded and loaded again (its DMA rings and the device re-initialised
    from scratch, as on 2026-11-16 16:07), SONiC up. In a transient unit of its own: this watchdog is part of
    sonic.target and is stopped by the first step (its state is saved before; it comes back with sonic.target)."""
    ok, out = run(["systemd-run", "--unit=sonic-cs8210-ipd-reload", "--no-block", "--collect",
                   "-p", "TimeoutStartSec=600", "--setenv=PATH=/run/current-system/sw/bin",
                   "/run/current-system/sw/bin/sh", "-c", RELOAD_SCRIPT])
    if not ok:
        log("ipd reload: systemd-run failed: %s" % out)
    return ok


class Recovery:
    """After a watchdog action: does the CPU path move again? Logged once either way (the first real observation
    of a syncd restart reviving a dead TX ring is still missing, 2026-11-16)."""

    def __init__(self, window=300):
        self.window, self.kind, self.base, self.t_active = window, None, None, None

    def start(self, kind):
        self.kind, self.base, self.t_active = kind, None, None

    def update(self, now, active, rings):
        """-> "recovered" / "no-tx" / None."""
        if not self.kind or not active:
            return None
        r = (rings or {}).get("TX-3")
        if not r:
            return None
        if self.base is None:
            self.base, self.t_active = dict(r), now
            return None
        if r["packets"] >= self.base["packets"] + 3 and r.get("full", 0) == self.base.get("full", 0):
            self.kind = None
            return "recovered"
        if r.get("full", 0) != self.base.get("full", 0):
            self.base = dict(r)   # still backing up: measure from here
        if now - self.t_active >= self.window:
            self.kind = None
            return "no-tx"
        return None


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
    ap.add_argument("--no-aer-clear", action="store_true", help="count root-port error bits but never clear them")
    ap.add_argument("--state", help="JSON counter file (kept across watchdog restarts within a boot)")
    ap.add_argument("--metrics", action="store_true", help="print Prometheus text (counters from --state) and exit")
    a = ap.parse_args(argv)
    if a.metrics:
        sys.stdout.write(metrics_text(a.state))
        return 0
    state = State(a.state)

    stall = TxStall(a.stall) if a.stall > 0 else None
    cpu = LearnCpu(a.cpu_limit, 2, os.sysconf("SC_CLK_TCK"))
    guard, cputx, recovery = GuardSignal(), CpuTx(), Recovery()
    policy = RestartPolicy(a.min_interval)
    if state.last_restart:   # a watchdog restart (deploy, ipd reload) keeps the rate limit
        policy.last = time.monotonic() - max(0.0, time.time() - state.last_restart)
    if state.level:          # started again by the ipd reload: watch whether it helped
        recovery.start("ipd reload")
    last_pid, last_kernel, prev_sig, inv_running = None, None, set(), False
    while True:
        now = time.monotonic()
        for line in ([] if a.once else aer_poll(state, time.time(), writer=(lambda *x: None) if a.no_aer_clear or a.dry_run else None)):
            log(line)
        p = unit_props()
        pid = int(p.get("MainPID") or 0)
        rings = ring_stat()
        if p.get("ActiveState") != "active" or pid <= 0:
            if stall:
                stall.reset()
            cpu.update(0, now, None)
            guard.update(None)
            cputx.update(None)
            if a.once:
                log("syncd not active (%s)" % p.get("ActiveState"))
                return 0
            state.save(time.time())
            time.sleep(a.interval)
            continue
        if last_pid is not None and pid != last_pid:
            log("syncd restarted (pid %d -> %d); %s; %s" % (last_pid, pid, root_port_status(), tx_ring_line()))
            if stall:
                stall.reset()
            guard.update(None)
            cputx.update(None)
        last_pid = pid
        up_s = now - int(p.get("ActiveEnterTimestampMonotonic") or 0) / 1e6

        rec = recovery.update(now, True, rings)
        if rec == "recovered":
            state.recoveries += 1
            state.level = state.alarm = 0
            log("IFCS watchdog: CPU TX moving again after the last action (%s)" % tx_ring_line())
        elif rec == "no-tx":
            log("IFCS watchdog: no CPU TX within %d s of syncd starting after the last action (%s)"
                % (recovery.window, tx_ring_line()))

        sig, detail = [], []
        st = proc_status(pid)
        sig += memory_signals(st, a.vm_limit, a.rss_limit)
        detail.append("VmSize %.2f GiB VmRSS %.2f GiB" % (st.get("VmSize", 0) / 2**30, st.get("VmRSS", 0) / 2**30))
        hit, frac = cpu.update(pid, now, thread_ticks(pid, LEARN_THREAD))
        if hit:
            sig.append("learn-cpu")
        if frac is not None:
            detail.append("%s %.0f%% of a core" % (LEARN_THREAD, 100 * frac))
        g_hit, new_inv = guard.update(read_guard())
        if new_inv and not inv_running:   # the onset is what dates the event; a dead chip would log every poll
            log("IFCS watchdog: libsai guard saw %d new invalid / all-ones register reads; %s" % (new_inv, root_port_status()))
        inv_running = bool(new_inv)
        if g_hit:
            sig.append("guard")
        if cputx.update(rings):
            sig.append("cpu-tx")
        if stall:
            ports = read_ports()
            if ports is None:
                detail.append("redis unreadable")
            else:
                stalled, active = stall.update(now, ports, linux_tx(list(ports)))
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
            log("syncd pid %d up %.0f s: %s; signals: %s; %s; %s"
                % (pid, up_s, "; ".join(detail), ",".join(sig) or "none", root_port_status(), tx_ring_line()))
            return 0
        onset = [s for s in sig if s not in prev_sig]
        prev_sig = set(sig)
        if onset:
            state.trigger(onset, time.time())
        if sig:
            what = "IFCS watchdog: %s (%s); %s; %s" % (",".join(sig), "; ".join(detail), root_port_status(), tx_ring_line())
            cpu_path = any(s in CPU_PATH for s in sig)
            hold = hold_reason()
            if a.dry_run:
                log(what + "; dry run, no restart")
            elif hold:
                if onset:
                    state.held += 1
                    log(what + "; %s: not acting" % hold)
            elif state.alarm:
                if onset:
                    log(what + "; ALARM already raised (restart and ipd reload did not help): not acting")
            elif policy.allow(now):
                log(what + "; restarting " + UNIT)
                state.restarts += 1
                state.last_restart = time.time()
                restart_syncd()
                recovery.start("restart")
                for x in (stall, guard, cputx):
                    if x:
                        x.reset()
            elif cpu_path and state.level == 0:
                log(what + "; still dead after a restart %d s ago: reloading ipd" % (now - policy.last))
                state.escalations += 1
                state.level = 1
                state.last_restart = time.time()
                policy.last = now
                state.save(time.time())   # the reload stops this unit (sonic.target) before it returns
                reload_ipd()
                recovery.start("ipd reload")
            elif cpu_path:
                state.alarm = 1
                log(what + "; ALARM: still dead after a restart and an ipd reload; giving up until it recovers")
            else:
                if onset:
                    state.suppressed += 1
                log(what + "; restarted less than %d s ago, not again" % a.min_interval)
        state.save(time.time())
        time.sleep(a.interval)


if __name__ == "__main__":
    sys.exit(main())
