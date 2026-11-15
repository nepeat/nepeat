#!/usr/bin/env python3
"""node-exporter-lite: a node_exporter-compatible subset in stdlib Python.

For CPUs that the Go toolchain cannot target (Go's ppc64 port needs POWER8,
ISA 2.07; e6500 and other Book E cores fault on its VSX/direct-move
instructions). Metric names and labels follow node_exporter 1.x for the
collectors implemented here, so its dashboards and alerts mostly work.

Flags follow node_exporter: --collector.NAME / --no-collector.NAME,
--collector.textfile.directory=DIR, --web.listen-address=[::]:9100,
--path.procfs / --path.sysfs / --path.rootfs. --once prints one scrape.
"""
import argparse
import http.server
import os
import re
import socket
import subprocess
import sys
import threading
import time

VERSION = "0.1"
DEFAULT_ON = ["cpu", "stat", "loadavg", "meminfo", "netdev", "netclass", "filesystem",
              "diskstats", "hwmon", "thermal_zone", "pressure", "uname", "time", "os",
              "edac", "filefd", "conntrack", "vmstat", "textfile"]
DEFAULT_OFF = ["systemd"]
FS_TYPES_EXCLUDE = (r"^(autofs|binfmt_misc|bpf|cgroup2?|configfs|debugfs|devpts|devtmpfs|fusectl|"
                    r"hugetlbfs|iso9660|mqueue|nsfs|overlay|proc|procfs|pstore|rpc_pipefs|"
                    r"securityfs|selinuxfs|squashfs|erofs|sysfs|tracefs)$")
MOUNT_POINTS_EXCLUDE = r"^/(dev|proc|run/credentials/.+|sys|var/lib/docker/.+|var/lib/containers/storage/.+)($|/)"
DISK_EXCLUDE = r"^(z?ram|loop|fd|(h|s|v|xv)d[a-z]|nvme\d+n\d+p)\d+$"
UNIT_EXCLUDE = r".+\.(automount|device|mount|scope|slice)"
NAME_RE = re.compile(r"^[a-zA-Z_:][a-zA-Z0-9_:]*$")
SAMPLE_RE = re.compile(r'^([a-zA-Z_:][a-zA-Z0-9_:]*)(\{(?:[^"}]|"(?:[^"\\]|\\.)*")*\})?[ \t]+(\S+)[ \t]*$')


def esc(v):
    return str(v).replace("\\", "\\\\").replace("\n", "\\n").replace('"', '\\"')


def num(v):
    if isinstance(v, bool):
        return "1" if v else "0"
    if isinstance(v, int):
        return str(v)
    if v != v:
        return "NaN"
    if v in (float("inf"), float("-inf")):
        return "+Inf" if v > 0 else "-Inf"
    # Not repr(): Python cross-built for some targets lacks short float repr
    # (PY_NO_SHORT_FLOAT_REPR) and prints 3.2999999999999998 for 3.3.
    return "%.15g" % v


class Out:
    """Metric families in insertion order; one HELP/TYPE per family."""

    def __init__(self):
        self.fams = {}

    def family(self, name, typ, help_):
        f = self.fams.get(name)
        if f is None:
            f = self.fams[name] = [typ, help_, []]
        return f

    def add(self, name, typ, help_, value, labels=None):
        lab = ""
        if labels:
            lab = "{" + ",".join(f'{k}="{esc(v)}"' for k, v in labels.items()) + "}"
        self.family(name, typ, help_)[2].append(f"{name}{lab} {num(value)}")

    def render(self):
        lines = []
        for name, (typ, help_, samples) in self.fams.items():
            if not samples:
                continue
            lines.append(f"# HELP {name} {help_.replace(chr(92), chr(92) * 2).replace(chr(10), chr(92) + 'n')}")
            lines.append(f"# TYPE {name} {typ}")
            lines.extend(samples)
        return "\n".join(lines) + "\n"


def rd(path):
    with open(path) as f:
        return f.read()


def rds(path):
    return rd(path).strip()


def clean(name):
    return re.sub(r"[^a-zA-Z0-9_]", "_", name).strip("_").lower()


# --- collectors ---------------------------------------------------------------
def c_cpu(o, a):
    tck = os.sysconf("SC_CLK_TCK")
    modes = ["user", "nice", "system", "idle", "iowait", "irq", "softirq", "steal"]
    for line in rd(f"{a.procfs}/stat").splitlines():
        if not (line.startswith("cpu") and line[3:4].isdigit()):
            continue
        p = line.split()
        cpu, vals = p[0][3:], [int(x) for x in p[1:]]
        for m, v in zip(modes, vals):
            o.add("node_cpu_seconds_total", "counter", "Seconds the CPUs spent in each mode.",
                  v / tck, {"cpu": cpu, "mode": m})
        for m, v in zip(["user", "nice"], vals[8:10]):
            o.add("node_cpu_guest_seconds_total", "counter",
                  "Seconds the CPUs spent in guests (VMs) for each mode.", v / tck, {"cpu": cpu, "mode": m})


def c_stat(o, a):
    for line in rd(f"{a.procfs}/stat").splitlines():
        p = line.split()
        if not p:
            continue
        k = p[0]
        if k == "btime":
            o.add("node_boot_time_seconds", "gauge", "Node boot time, in unixtime.", int(p[1]))
        elif k == "ctxt":
            o.add("node_context_switches_total", "counter", "Total number of context switches.", int(p[1]))
        elif k == "processes":
            o.add("node_forks_total", "counter", "Total number of forks.", int(p[1]))
        elif k == "intr":
            o.add("node_intr_total", "counter", "Total number of interrupts serviced.", int(p[1]))
        elif k == "procs_running":
            o.add("node_procs_running", "gauge", "Number of processes in runnable state.", int(p[1]))
        elif k == "procs_blocked":
            o.add("node_procs_blocked", "gauge", "Number of processes blocked waiting for I/O to complete.", int(p[1]))


def c_loadavg(o, a):
    p = rd(f"{a.procfs}/loadavg").split()
    for i, n in enumerate(("1", "5", "15")):
        o.add(f"node_load{n}", "gauge", f"{n}m load average.", float(p[i]))


def c_meminfo(o, a):
    for line in rd(f"{a.procfs}/meminfo").splitlines():
        p = line.split()
        if len(p) < 2:
            continue
        key, v = p[0].rstrip(":"), float(p[1])
        if len(p) == 3:
            v *= 1024
            key += "_bytes"
        key = re.sub(r"\((.*)\)", r"_\1", key)
        if NAME_RE.match("node_memory_" + key):
            o.add(f"node_memory_{key}", "gauge", f"Memory information field {key}.", v)


RX = ["bytes", "packets", "errs", "drop", "fifo", "frame", "compressed", "multicast"]
TX = ["bytes", "packets", "errs", "drop", "fifo", "colls", "carrier", "compressed"]


def c_netdev(o, a):
    for line in rd(f"{a.procfs}/net/dev").splitlines()[2:]:
        dev, _, rest = line.partition(":")
        dev, vals = dev.strip(), rest.split()
        if len(vals) < 16:
            continue
        for d, names, off in (("receive", RX, 0), ("transmit", TX, 8)):
            for i, n in enumerate(names):
                o.add(f"node_network_{d}_{n}_total", "counter",
                      f"Network device statistic {d}_{n}.", int(vals[off + i]), {"device": dev})


def c_netclass(o, a):
    base = f"{a.sysfs}/class/net"
    for dev in sorted(os.listdir(base)):
        d = f"{base}/{dev}"

        def attr(name, d=d):
            try:
                return rds(f"{d}/{name}")
            except OSError:
                return None

        oper, flags = attr("operstate"), attr("flags")
        try:
            admin = "up" if int(flags or "0", 16) & 1 else "down"
        except ValueError:
            admin = ""
        o.add("node_network_info", "gauge", "Non-numeric data from /sys/class/net/<iface>, value is always 1.", 1,
              {"address": attr("address") or "", "adminstate": admin, "broadcast": attr("broadcast") or "",
               "device": dev, "duplex": attr("duplex") or "", "ifalias": attr("ifalias") or "",
               "operstate": oper or ""})
        o.add("node_network_up", "gauge", "Value is 1 if operstate is 'up', 0 otherwise.",
              1 if oper == "up" else 0, {"device": dev})
        for f, name, help_, typ, scale in (
                ("carrier", "node_network_carrier", "Network device property: carrier", "gauge", None),
                ("mtu", "node_network_mtu_bytes", "Network device property: mtu_bytes", "gauge", None),
                ("speed", "node_network_speed_bytes", "Network device property: speed_bytes", "gauge", 125000),
                ("carrier_changes", "node_network_carrier_changes_total", "Network device property: carrier_changes_total", "counter", None),
                ("carrier_up_count", "node_network_carrier_up_changes_total", "Network device property: carrier_up_changes_total", "counter", None),
                ("carrier_down_count", "node_network_carrier_down_changes_total", "Network device property: carrier_down_changes_total", "counter", None),
                ("ifindex", "node_network_iface_id", "Network device property: iface_id", "gauge", None),
                ("tx_queue_len", "node_network_transmit_queue_length", "Network device property: transmit_queue_length", "gauge", None),
                ("type", "node_network_protocol_type", "Network device property: protocol_type", "gauge", None)):
            v = attr(f)
            try:
                v = int(v)
            except (TypeError, ValueError):
                continue
            if scale:
                if v < 0:
                    continue
                v *= scale
            o.add(name, typ, help_, v, {"device": dev})
        if flags is not None:
            o.add("node_network_flags", "gauge", "Network device property: flags", int(flags, 16), {"device": dev})


def unoct(s):
    return re.sub(r"\\([0-7]{3})", lambda m: chr(int(m.group(1), 8)), s)


def c_filesystem(o, a):
    try:
        mounts = rd(f"{a.procfs}/1/mounts")
    except OSError:
        mounts = rd(f"{a.procfs}/self/mounts")
    seen = {}
    for line in mounts.splitlines():
        p = line.split()
        if len(p) < 4:
            continue
        dev, mp, fstype, opts = unoct(p[0]), unoct(p[1]), p[2], p[3].split(",")
        if re.match(FS_TYPES_EXCLUDE, fstype) or re.match(MOUNT_POINTS_EXCLUDE, mp):
            continue
        seen[mp] = (dev, fstype, "ro" in opts)
    for mp, (dev, fstype, ro) in seen.items():
        lab = {"device": dev, "fstype": fstype, "mountpoint": mp}
        try:
            st = os.statvfs(os.path.join(a.rootfs, mp.lstrip("/")))
        except OSError:
            o.add("node_filesystem_device_error", "gauge", "Whether an error occurred while getting statistics for the given device.", 1, lab)
            continue
        o.add("node_filesystem_device_error", "gauge", "Whether an error occurred while getting statistics for the given device.", 0, lab)
        o.add("node_filesystem_size_bytes", "gauge", "Filesystem size in bytes.", st.f_blocks * st.f_frsize, lab)
        o.add("node_filesystem_free_bytes", "gauge", "Filesystem free space in bytes.", st.f_bfree * st.f_frsize, lab)
        o.add("node_filesystem_avail_bytes", "gauge", "Filesystem space available to non-root users in bytes.", st.f_bavail * st.f_frsize, lab)
        o.add("node_filesystem_files", "gauge", "Filesystem total file nodes.", st.f_files, lab)
        o.add("node_filesystem_files_free", "gauge", "Filesystem total free file nodes.", st.f_ffree, lab)
        o.add("node_filesystem_readonly", "gauge", "Filesystem read-only status.", 1 if ro else 0, lab)


DISK_FIELDS = [  # (index after the name, metric, scale, help)
    (0, "reads_completed_total", 1, "The total number of reads completed successfully."),
    (1, "reads_merged_total", 1, "The total number of reads merged."),
    (2, "read_bytes_total", 512, "The total number of bytes read successfully."),
    (3, "read_time_seconds_total", 0.001, "The total number of seconds spent by all reads."),
    (4, "writes_completed_total", 1, "The total number of writes completed successfully."),
    (5, "writes_merged_total", 1, "The number of writes merged."),
    (6, "written_bytes_total", 512, "The total number of bytes written successfully."),
    (7, "write_time_seconds_total", 0.001, "This is the total number of seconds spent by all writes."),
    (8, "io_now", 1, "The number of I/Os currently in progress."),
    (9, "io_time_seconds_total", 0.001, "Total seconds spent doing I/Os."),
    (10, "io_time_weighted_seconds_total", 0.001, "The weighted # of seconds spent doing I/Os."),
    (11, "discards_completed_total", 1, "The total number of discards completed successfully."),
    (12, "discards_merged_total", 1, "The total number of discards merged."),
    (13, "discarded_sectors_total", 1, "The total number of sectors discarded successfully."),
    (14, "discard_time_seconds_total", 0.001, "This is the total number of seconds spent by all discards."),
    (15, "flush_requests_total", 1, "The total number of flush requests completed successfully"),
    (16, "flush_requests_time_seconds_total", 0.001, "This is the total number of seconds spent by all flush requests."),
]


def c_diskstats(o, a):
    for line in rd(f"{a.procfs}/diskstats").splitlines():
        p = line.split()
        if len(p) < 14 or re.match(a.disk_exclude, p[2]):
            continue
        vals = p[3:]
        for i, name, scale, help_ in DISK_FIELDS:
            if i < len(vals):
                v = int(vals[i]) * scale
                o.add(f"node_disk_{name}", "gauge" if name == "io_now" else "counter", help_, v, {"device": p[2]})


HWMON_UNITS = {"temp": ("celsius", 0.001), "in": ("volts", 0.001), "curr": ("amps", 0.001),
               "power": ("watts", 0.000001), "energy": ("joules", 0.000001), "fan": ("rpm", 1),
               "humidity": ("ratio", 0.00001)}
HWMON_RAW = {"alarm", "beep", "fault", "enable", "min_alarm", "max_alarm", "crit_alarm",
             "lcrit_alarm", "type", "mode", "pulses"}


def hwmon_chip(d):
    try:
        dev = os.path.realpath(f"{d}/device")
        if os.path.exists(f"{d}/device"):
            parent, name = os.path.split(dev)
            t, n = clean(os.path.basename(parent)), clean(name)
            if t and n:
                return t + "_" + n
            if n:
                return n
    except OSError:
        pass
    try:
        n = clean(rds(f"{d}/name"))
        if n:
            return n
    except OSError:
        pass
    return clean(os.path.basename(os.path.realpath(d)))


def c_hwmon(o, a):
    base = f"{a.sysfs}/class/hwmon"
    for h in sorted(os.listdir(base)):
        d = f"{base}/{h}"
        chip = hwmon_chip(d)
        try:
            o.add("node_hwmon_chip_names", "gauge", "Annotation metric for human-readable chip names", 1,
                  {"chip": chip, "chip_name": clean(rds(f"{d}/name"))})
        except OSError:
            pass
        for f in sorted(os.listdir(d)):
            m = re.match(r"^([a-z]+)(\d+)(?:_(\w+))?$", f)
            if not m:
                continue
            typ, idx, prop = m.group(1), m.group(2), m.group(3) or "input"
            sensor = typ + idx
            lab = {"chip": chip, "sensor": sensor}
            try:
                raw = rds(f"{d}/{f}")
            except OSError:
                continue
            if prop == "label":
                o.add("node_hwmon_sensor_label", "gauge", "Label for given chip and sensor", 1,
                      {"chip": chip, "label": raw, "sensor": sensor})
                continue
            try:
                v = int(raw)
            except ValueError:
                try:
                    v = float(raw)
                except ValueError:
                    continue
            if typ == "pwm":
                name = "node_hwmon_pwm" if prop == "input" else f"node_hwmon_pwm_{prop}"
                o.add(name, "gauge", f"Hardware monitor pwm element {prop}", v, lab)
            elif typ in HWMON_UNITS and prop not in HWMON_RAW:
                unit, scale = HWMON_UNITS[typ]
                name = f"node_hwmon_{typ}_{unit}" if prop == "input" else f"node_hwmon_{typ}_{prop}_{unit}"
                o.add(name, "gauge", f"Hardware monitor for {typ} ({prop})", v * scale, lab)
            elif typ in HWMON_UNITS:
                o.add(f"node_hwmon_{typ}_{prop}", "gauge", f"Hardware sensor {prop} status ({typ})", v, lab)


def c_thermal_zone(o, a):
    base = f"{a.sysfs}/class/thermal"
    if not os.path.isdir(base):
        return
    for z in sorted(os.listdir(base)):
        if not z.startswith("thermal_zone"):
            continue
        try:
            t = int(rds(f"{base}/{z}/temp"))
            typ = rds(f"{base}/{z}/type")
        except (OSError, ValueError):
            continue
        o.add("node_thermal_zone_temp", "gauge", "Zone temperature in Celsius", t / 1000,
              {"type": typ, "zone": z[len("thermal_zone"):]})


def c_pressure(o, a):
    for res in ("cpu", "io", "memory", "irq"):
        try:
            text = rd(f"{a.procfs}/pressure/{res}")
        except OSError:
            continue
        for line in text.splitlines():
            p = line.split()
            total = int(dict(x.split("=") for x in p[1:])["total"]) / 1e6
            if p[0] == "some" and res != "irq":
                o.add(f"node_pressure_{res}_waiting_seconds_total", "counter",
                      f"Total time in seconds that processes have waited for {res}", total)
            elif p[0] == "full" and res != "cpu":
                o.add(f"node_pressure_{res}_stalled_seconds_total", "counter",
                      f"Total time in seconds no process could make progress due to {res}", total)


def c_uname(o, a):
    u = os.uname()
    try:
        dom = rds(f"{a.procfs}/sys/kernel/domainname")
    except OSError:
        dom = "(none)"
    o.add("node_uname_info", "gauge", "Labeled system information as provided by the uname system call.", 1,
          {"domainname": dom, "machine": u.machine, "nodename": u.nodename, "release": u.release,
           "sysname": u.sysname, "version": u.version})


def c_time(o, a):
    now = time.time()
    o.add("node_time_seconds", "gauge", "System time in seconds since epoch (1970).", now)
    lt = time.localtime(now)
    o.add("node_time_zone_offset_seconds", "gauge", "System time zone offset in seconds.", lt.tm_gmtoff,
          {"time_zone": lt.tm_zone})
    cs = f"{a.sysfs}/devices/system/clocksource"
    if os.path.isdir(cs):
        for c in sorted(os.listdir(cs)):
            try:
                o.add("node_time_clocksource_current_info", "gauge", "Current clocksource.", 1,
                      {"clocksource": rds(f"{cs}/{c}/current_clocksource"), "device": c[len("clocksource"):]})
            except OSError:
                pass


def c_os(o, a):
    for path in (f"{a.rootfs}/etc/os-release", f"{a.rootfs}/usr/lib/os-release"):
        try:
            text = rd(path)
            break
        except OSError:
            continue
    else:
        return
    kv = {}
    for line in text.splitlines():
        k, _, v = line.partition("=")
        if k and v:
            kv[k] = v.strip().strip('"')
    o.add("node_os_info", "gauge", "A metric with a constant '1' value labeled by build_id, id, id_like, image_id, image_version, name, pretty_name, variant, variant_id, version, version_codename, version_id.", 1,
          {"build_id": kv.get("BUILD_ID", ""), "id": kv.get("ID", ""), "id_like": kv.get("ID_LIKE", ""),
           "name": kv.get("NAME", ""), "pretty_name": kv.get("PRETTY_NAME", ""),
           "version": kv.get("VERSION", ""), "version_codename": kv.get("VERSION_CODENAME", ""),
           "version_id": kv.get("VERSION_ID", "")})


def c_edac(o, a):
    base = f"{a.sysfs}/devices/system/edac/mc"
    if not os.path.isdir(base):
        return
    for mc in sorted(os.listdir(base)):
        if not re.match(r"^mc\d+$", mc):
            continue
        lab = {"controller": mc[2:]}
        for f, name, help_ in (("ce_count", "correctable_errors_total", "Total correctable memory errors."),
                               ("ue_count", "uncorrectable_errors_total", "Total uncorrectable memory errors."),
                               ("ce_noinfo_count", "csrow_correctable_errors_total", "Total correctable memory errors for this csrow."),
                               ("ue_noinfo_count", "csrow_uncorrectable_errors_total", "Total uncorrectable memory errors for this csrow.")):
            try:
                v = int(rds(f"{base}/{mc}/{f}"))
            except (OSError, ValueError):
                continue
            if f.endswith("noinfo_count"):
                o.add(f"node_edac_{name}", "counter", help_, v, dict(lab, csrow="unknown"))
            else:
                o.add(f"node_edac_{name}", "counter", help_, v, lab)


def c_filefd(o, a):
    p = rd(f"{a.procfs}/sys/fs/file-nr").split()
    o.add("node_filefd_allocated", "gauge", "File descriptor statistics: allocated.", int(p[0]))
    o.add("node_filefd_maximum", "gauge", "File descriptor statistics: maximum.", int(p[2]))


def c_conntrack(o, a):
    base = f"{a.procfs}/sys/net/netfilter"
    try:
        n, m = int(rds(f"{base}/nf_conntrack_count")), int(rds(f"{base}/nf_conntrack_max"))
    except OSError:
        return
    o.add("node_nf_conntrack_entries", "gauge", "Number of currently allocated flow entries for connection tracking.", n)
    o.add("node_nf_conntrack_entries_limit", "gauge", "Maximum size of connection tracking table.", m)


def c_vmstat(o, a):
    for line in rd(f"{a.procfs}/vmstat").splitlines():
        p = line.split()
        if len(p) == 2 and re.match(r"^(oom_kill|pgpg|pswp|pg.*fault).*", p[0]):
            o.add(f"node_vmstat_{p[0]}", "untyped", f"/proc/vmstat information field {p[0]}.", int(p[1]))


SYSTEMD_STATES = ["activating", "active", "deactivating", "inactive", "failed"]


def c_systemd(o, a):
    r = subprocess.run(["systemctl", "list-units", "--all", "--plain", "--no-legend", "--no-pager", "--full"],
                       capture_output=True, text=True, timeout=10, check=True)
    counts = dict.fromkeys(SYSTEMD_STATES, 0)
    for line in r.stdout.splitlines():
        p = line.lstrip("●* ").split(None, 4)
        if len(p) < 4:
            continue
        unit, active = p[0], p[2]
        if not re.fullmatch(a.unit_include, unit) or re.fullmatch(a.unit_exclude, unit):
            continue
        if active in counts:
            counts[active] += 1
        for s in SYSTEMD_STATES:
            o.add("node_systemd_unit_state", "gauge", "Systemd unit", 1 if s == active else 0,
                  {"name": unit, "state": s})
    for s, n in counts.items():
        o.add("node_systemd_units", "gauge", "Summary of systemd unit states", n, {"state": s})
    r = subprocess.run(["systemctl", "is-system-running"], capture_output=True, text=True, timeout=10)
    o.add("node_systemd_system_running", "gauge", "Whether the system is operational (see 'systemctl is-system-running')",
          1 if r.stdout.strip() == "running" else 0)


def parse_textfile(path, text):
    """Families from one .prom file: {name: [type, help, [samples]]}."""
    fams, order = {}, []
    declared = {}
    for line in text.splitlines():
        line = line.strip()
        if not line:
            continue
        if line.startswith("#"):
            p = line.split(None, 3)
            if len(p) >= 3 and p[1] in ("HELP", "TYPE"):
                name = p[2]
                if not NAME_RE.match(name):
                    raise ValueError(f"bad metric name {name!r}")
                f = fams.setdefault(name, [None, None, []])
                if p[1] == "TYPE":
                    t = p[3] if len(p) > 3 else ""
                    if t not in ("counter", "gauge", "histogram", "summary", "untyped"):
                        raise ValueError(f"bad type {t!r} for {name}")
                    if f[0] is not None or f[2]:
                        raise ValueError(f"TYPE for {name} repeated or after its samples")
                    f[0] = t
                    declared[name] = t
                else:
                    f[1] = p[3] if len(p) > 3 else ""
            continue
        m = SAMPLE_RE.match(line)
        if not m:
            raise ValueError(f"malformed sample line {line!r} (sample timestamps are not supported)")
        float(m.group(3).replace("Inf", "inf"))
        name = m.group(1)
        fam = name
        for suffix in ("_bucket", "_sum", "_count"):
            base = name[: -len(suffix)]
            if name.endswith(suffix) and declared.get(base) in ("histogram", "summary"):
                fam = base
        fams.setdefault(fam, [None, None, []])[2].append(line)
    for name, f in fams.items():
        f[0] = f[0] or "untyped"
        f[1] = f[1] if f[1] is not None else f"Metric read from {path}"
    return fams


def c_textfile(o, a):
    if not a.textfile_dir:
        return
    err = 0
    try:
        names = sorted(n for n in os.listdir(a.textfile_dir) if n.endswith(".prom"))
    except FileNotFoundError:
        names = []
    for n in names:
        path = os.path.join(a.textfile_dir, n)
        try:
            st = os.stat(path)
            fams = parse_textfile(path, rd(path))
            for name, (typ, help_, samples) in fams.items():
                old = o.fams.get(name)
                if old is not None and old[0] != typ:
                    raise ValueError(f"{name}: type {typ} conflicts with {old[0]}")
            for name, (typ, help_, samples) in fams.items():
                o.family(name, typ, help_)[2].extend(samples)
            o.add("node_textfile_mtime_seconds", "gauge", "Unixtime mtime of textfiles successfully read.",
                  st.st_mtime, {"file": path})
        except (OSError, ValueError, UnicodeDecodeError) as exc:
            print(f"textfile {path}: {exc}", file=sys.stderr)
            err = 1
    o.add("node_textfile_scrape_error", "gauge", "1 if there was an error opening or reading a file, 0 otherwise", err)


COLLECTORS = {n[2:]: f for n, f in globals().items() if n.startswith("c_")}


def scrape(a):
    o = Out()
    # textfile last, so collector families exist before files are merged in.
    for name in sorted(a.collectors, key=lambda n: n == "textfile"):
        t0 = time.monotonic()
        ok = 1
        try:
            COLLECTORS[name](o, a)
        except Exception as exc:  # one broken collector must not break the scrape
            print(f"collector {name}: {exc!r}", file=sys.stderr)
            ok = 0
        lab = {"collector": name}
        o.add("node_scrape_collector_duration_seconds", "gauge", "node_exporter: Duration of a collector scrape.",
              time.monotonic() - t0, lab)
        o.add("node_scrape_collector_success", "gauge", "node_exporter: Whether a collector succeeded.", ok, lab)
    o.add("node_exporter_build_info", "gauge", "A metric with a constant '1' value labeled by version, revision, branch, goversion from which node_exporter was built, and the goos and goarch for the build.", 1,
          {"version": f"lite-{VERSION}", "revision": "", "branch": "",
           "goversion": "python" + ".".join(map(str, sys.version_info[:3])),
           "goos": "linux", "goarch": os.uname().machine})
    return o.render()


def parse_args(argv):
    enabled = set(DEFAULT_ON)
    rest = []
    for x in argv:
        m = re.fullmatch(r"--(no-)?collector\.([a-z_]+)", x)
        if not m:
            rest.append(x)
            continue
        name = m.group(2)
        if m.group(1):
            enabled.discard(name)
        elif name in COLLECTORS:
            enabled.add(name)
        else:
            print(f"node-exporter-lite: collector {name!r} not implemented, ignored", file=sys.stderr)
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--web.listen-address", dest="listen", default=":9100")
    ap.add_argument("--collector.textfile.directory", dest="textfile_dir", default="")
    ap.add_argument("--collector.systemd.unit-include", dest="unit_include", default=".+")
    ap.add_argument("--collector.systemd.unit-exclude", dest="unit_exclude", default=UNIT_EXCLUDE)
    ap.add_argument("--collector.diskstats.device-exclude", dest="disk_exclude", default=DISK_EXCLUDE)
    ap.add_argument("--path.procfs", dest="procfs", default="/proc")
    ap.add_argument("--path.sysfs", dest="sysfs", default="/sys")
    ap.add_argument("--path.rootfs", dest="rootfs", default="/")
    ap.add_argument("--once", action="store_true", help="print one scrape to stdout and exit")
    ap.add_argument("--version", action="version", version=f"node-exporter-lite {VERSION}")
    a = ap.parse_args(rest)
    a.collectors = sorted(enabled & set(COLLECTORS))
    return a


def serve(a):
    host, _, port = a.listen.rpartition(":")
    host = host.strip("[]")
    lock = threading.Lock()

    class Handler(http.server.BaseHTTPRequestHandler):
        def do_GET(self):
            if self.path.split("?")[0] == "/metrics":
                with lock:
                    body = scrape(a).encode()
                ctype = "text/plain; version=0.0.4; charset=utf-8"
            else:
                body = b'<html><body><a href="/metrics">metrics</a></body></html>\n'
                ctype = "text/html"
            self.send_response(200)
            self.send_header("Content-Type", ctype)
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)

        def log_message(self, *args):
            pass

    class Server(http.server.ThreadingHTTPServer):
        daemon_threads = True
        address_family = socket.AF_INET6 if host in ("", "::") or ":" in host else socket.AF_INET

        def server_bind(self):
            if self.address_family == socket.AF_INET6:
                self.socket.setsockopt(socket.IPPROTO_IPV6, socket.IPV6_V6ONLY, 0)
            super().server_bind()

    try:
        srv = Server((host or "::", int(port)), Handler)
    except OSError:
        if host not in ("", "::"):
            raise
        Server.address_family = socket.AF_INET
        srv = Server(("0.0.0.0", int(port)), Handler)
    print(f"node-exporter-lite {VERSION} listening on {a.listen}, collectors: {' '.join(a.collectors)}",
          file=sys.stderr, flush=True)
    srv.serve_forever()


def main():
    a = parse_args(sys.argv[1:])
    if a.once:
        sys.stdout.write(scrape(a))
    else:
        serve(a)


if __name__ == "__main__":
    main()
