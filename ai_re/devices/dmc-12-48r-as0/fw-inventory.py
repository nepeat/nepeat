#!/usr/bin/env python3
"""Per-unit firmware/component inventory for DMC-12s, and diffs between units. Read-only.

  fw-inventory.py collect --unit u02 --ip 10.60.2.x   # live box (NixOS, root ssh) + its factory dumps
  fw-inventory.py collect --unit u01 --offline        # factory dumps only (no box)
  fw-inventory.py show u02                            # one unit's components
  fw-inventory.py table [u01 u02 ...]                 # fleet: component x unit (versions/hashes)
  fw-inventory.py diff u01 u02 [--bytes]              # what differs; --bytes: byte ranges of blobs
  fw-inventory.py drift u01                           # SPI objects changed since the factory dump

Records, per unit:
  inventory/<unit>/firmware.json   tracked: versions, sizes, sha256 per component; no identifiers
  dumps/<unit>/firmware/           git-ignored: the blobs (SPI objects per TOC slot, CPLD), for bin diffs
Components: SPI CS0/CS1 objects per TOC entry (factory = dumps/<unit>/mtd{0,1}.bin.zst, current =
live mtd read), CPLD (IDCODE, USERCODE, version register, config/UFM flash readback via
cpld-readback.py), K2 uC versions (from provision's factory U-Boot capture), board EEPROM (ONIE
TLV, safe fields only), DDR SPD part, SSD model/firmware, TPM (vendor, firmware version),
USB hubs and CP2108 bridges (bcdDevice), PCI devices (revision), SoC (MIDR), running software.
Serials, MACs, service tags, the CPLD TraceID and env/EEPROM contents never go in firmware.json
(EEPROM/env objects: state and size only; their blobs stay in dumps/).
"""
import argparse, hashlib, importlib.util, json, pathlib, subprocess, sys, time

ROOT = pathlib.Path(__file__).resolve().parent
AIRE = ROOT.parents[1]
NO_SUDO = False
SSH = ["sudo", "-n", "ssh", "-o", "BatchMode=yes", "-o", "StrictHostKeyChecking=no",
       "-o", "UserKnownHostsFile=/dev/null", "-o", "LogLevel=ERROR"]


def mod(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    m = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(m)
    return m


alboot = mod("alboot", ROOT / "alboot.py")


def sha(b):
    return hashlib.sha256(b).hexdigest()


def ssh(ip, cmd, stdin=None, binary=False):
    pre = SSH[2:] if NO_SUDO else SSH          # --no-sudo: the invoking user's key/agent
    r = subprocess.run(pre + [f"root@{ip}", cmd], input=stdin, capture_output=True,
                       text=not binary, timeout=600)
    if r.returncode:
        raise SystemExit(f"ssh {cmd[:40]!r}: exit {r.returncode} {r.stderr[-200:]!r}")
    return r.stdout


# --- SPI: one component per TOC entry -----------------------------------------------------------

def spi_objects(d, chip, blobdir):
    """TOC walk (alboot's checks); per entry: state, version, sha256 of the object, blob saved."""
    out = {}
    toc, n, ok = alboot.find_toc(d)
    if toc is None:
        return {f"spi.{chip}.toc": {"state": "no TOC", "sha256": sha(d)}}
    out[f"spi.{chip}.toc"] = {"offset": f"{toc:#x}", "entries": n, "sum_ok": ok}
    for i in range(n):
        e = toc + 0x14 + i * 0x20
        eid, name, dev, off, mx = alboot.struct.unpack_from("<I8sIII", d, e)
        oid, inst = eid & 0x0FFFFFFF, eid >> 28
        nm = alboot.NAMES.get(oid, f"{oid:#x}").lower()
        key = f"spi.{chip}.{nm}.{inst}"
        region = d[off:off + mx]
        c = {"id": f"{oid:#x}", "offset": f"{off:#x}", "max": f"{mx:#x}"}
        private = oid in alboot.PRIVATE_IDS
        blank = alboot.state(region[:64])
        if blank and alboot.state(region):
            c["state"] = blank
        elif oid in alboot.BOOTROM_IDS:
            st, info = alboot.check_s2(d, off, mx)
            c["state"] = st
            size = info.get("hdr", {}).get("size") or len(region.rstrip(b"\xff"))
            if "hdr" in info:
                c["version"] = alboot.vstr(info["hdr"])
            c["size"], c["sha256"] = size, sha(d[off:off + size])
        elif oid in alboot.ENV_IDS:
            c["state"] = "env"                       # contents: dumps/ only
        else:
            st, f = alboot.check_object(d, off, mx, oid)
            c["state"] = st
            if f:
                c["version"], c["size"] = alboot.vstr(f), f["size"]
                if not private:
                    c["sha256"] = sha(d[off:off + alboot.HDR + f["size"]])
        if private:
            c["private"] = True
        if blobdir and "blank" not in c["state"]:
            blobdir.mkdir(parents=True, exist_ok=True)
            (blobdir / f"{key}.bin").write_bytes(region)
        out[key] = c
    return out


# --- live probes (one remote script) ------------------------------------------------------------

REMOTE = r'''
import json, os, glob, struct
def rd(p, mode="r"):
    try:
        with open(p, mode) as f: return f.read().strip() if mode == "r" else f.read()
    except OSError: return None
o = {}
o["usb"] = []
for d in sorted(glob.glob("/sys/bus/usb/devices/[0-9]*")):
    if ":" in os.path.basename(d) or not rd(d + "/idVendor"): continue
    o["usb"].append({k: rd(f"{d}/{k}") for k in ("idVendor", "idProduct", "bcdDevice", "manufacturer", "product", "version")})
o["pci"] = []
for d in sorted(glob.glob("/sys/bus/pci/devices/*")):
    o["pci"].append({"slot": os.path.basename(d), "id": f"{rd(d+'/vendor')[2:]}:{rd(d+'/device')[2:]}",
                     "rev": rd(d + "/revision"), "class": rd(d + "/class")})
o["ssd"] = {k: rd(f"/sys/block/sda/device/{k}") for k in ("vendor", "model", "rev")}
o["ssd"]["sectors"] = rd("/sys/block/sda/size")
o["midr"] = rd("/sys/devices/system/cpu/cpu0/regs/identification/midr_el1")
o["kernel"] = os.uname().release + " " + os.uname().version
o["system"] = os.path.realpath("/run/current-system")
o["eeprom"] = (rd("/sys/bus/i2c/devices/7-0051/eeprom", "rb") or b"")[:256].hex()
o["preload"] = (rd("/sys/bus/i2c/devices/0-0057/eeprom", "rb") or b"").hex()
try:
    sm = json.loads(os.popen("smartctl -i -j /dev/sda 2>/dev/null").read() or "{}")
    o["ssd"].update({"ata_model": sm.get("model_name"), "firmware": sm.get("firmware_version"),
                     "ata_version": (sm.get("ata_version") or {}).get("string")})
except ValueError:
    pass
t = {}
if os.path.exists("/dev/tpmrm0"):
    # TPM2_GetCapability(TPM_PROPERTIES, PT_FIXED 0x100, 32): read-only query
    cmd = struct.pack(">HIIIII", 0x8001, 22, 0x17A, 6, 0x100, 32)
    fd = os.open("/dev/tpmrm0", os.O_RDWR); os.write(fd, cmd); r = os.read(fd, 4096); os.close(fd)
    tag, size, rc = struct.unpack_from(">HII", r)
    if rc == 0:
        cnt = struct.unpack_from(">I", r, 15)[0]
        props = {f"{p:#x}": v for p, v in (struct.unpack_from(">II", r, 19 + 8 * i) for i in range(cnt))}
        t = {"rc": 0, "props": props}
    else:
        t = {"rc": rc}
t["sysfs_version"] = rd("/sys/class/tpm/tpm0/tpm_version_major")
o["tpm"] = t
o["cpld_version"] = os.popen("i2cget -y 4 0x68 0x01 2>/dev/null").read().strip()
print(json.dumps(o))
'''


# Hosts without python3 (older images): the same fields via sysfs + a static busybox on the box.
BB_REMOTE = r'''
B=__BB__
for d in /sys/bus/usb/devices/[0-9]*; do case ${d##*/} in *:*) continue;; esac; [ -e $d/idVendor ] || continue
  printf 'usb\t%s\t%s\t%s\t%s\t%s\n' "$(cat $d/idVendor)" "$(cat $d/idProduct)" "$(cat $d/bcdDevice)" "$(cat $d/manufacturer 2>/dev/null)" "$(cat $d/product 2>/dev/null)"; done
for d in /sys/bus/pci/devices/*; do printf 'pci\t%s\t%s:%s\t%s\t%s\n' "${d##*/}" "$(cut -c3- $d/vendor)" "$(cut -c3- $d/device)" "$(cat $d/revision)" "$(cat $d/class)"; done
printf 'sectors\t%s\n' "$(cat /sys/block/sda/size)"
# drive identify; the serial number never leaves the box
$B hdparm -i /dev/sda 2>/dev/null | tr ',' '\n' | sed -n 's/^ *\(Model\|FwRev\)=\(.*\)/hdparm_\1\t\2/p'
printf 'midr\t%s\n' "$(cat /sys/devices/system/cpu/cpu0/regs/identification/midr_el1)"
printf 'kernel\t%s %s\n' "$(uname -r)" "$(uname -v)"
printf 'system\t%s\n' "$(readlink -f /run/current-system)"
printf 'eeprom\t%s\n' "$($B xxd -p /sys/bus/i2c/devices/7-0051/eeprom 2>/dev/null | head -c 1024 | tr -d '\n')"
printf 'preload\t%s\n' "$($B xxd -p /sys/bus/i2c/devices/0-0057/eeprom 2>/dev/null | tr -d '\n')"
printf 'tpm\t%s\n' "$(cat /sys/class/tpm/tpm0/tpm_version_major 2>/dev/null)"
printf 'cpld_version\t%s\n' "$($B i2cget -y 4 0x68 0x01)"
printf 'mtd\t%s\n' "$(grep -c mtd /proc/mtd)"
'''


def bb_live(ip, bb):
    out = ssh(ip, "sh -s", stdin=BB_REMOTE.replace("__BB__", bb))
    live = {"usb": [], "pci": [], "ssd": {}, "tpm": {}}
    for line in out.splitlines():
        f = line.split("\t")
        if f[0] == "usb":
            live["usb"].append(dict(zip(("idVendor", "idProduct", "bcdDevice", "manufacturer", "product"),
                                        [x or None for x in f[1:]])))
        elif f[0] == "pci":
            live["pci"].append({"slot": f[1], "id": f[2], "rev": f[3], "class": f[4]})
        elif f[0] == "sectors":
            live["ssd"]["sectors"] = f[1]
        elif f[0] == "hdparm_Model":
            live["ssd"]["ata_model"] = f[1].strip()
        elif f[0] == "hdparm_FwRev":
            live["ssd"]["firmware"] = f[1].strip()
        elif f[0] == "tpm":
            live["tpm"] = {"sysfs_version": f[1] or None}
        elif f[0] in ("midr", "kernel", "system", "eeprom", "preload", "cpld_version", "mtd"):
            live[f[0]] = f[1] if len(f) > 1 else ""
    return live


def tpm_component(t):
    p = t.get("props", {})
    if not p:
        return {"state": "absent" if not t.get("sysfs_version") else f"rc {t.get('rc')}"}
    def s4(k):
        v = p.get(k)
        return v.to_bytes(4, "big").decode(errors="replace").strip("\0 ") if v is not None else ""
    fw1, fw2 = p.get("0x10b", 0), p.get("0x10c", 0)
    return {"manufacturer": s4("0x105"), "vendor": "".join(s4(f"0x{k:x}") for k in range(0x106, 0x10a)),
            "family": s4("0x100"), "level": p.get("0x101"), "revision": p.get("0x102"),
            "version": f"{fw1 >> 16}.{fw1 & 0xffff}.{fw2 >> 16}.{fw2 & 0xffff}"}


def preload_component(h):
    """The Alpine preload EEPROM at 0-0057 (register writes run by the boot ROM; alpine_preload.py)."""
    b = bytes.fromhex(h)
    return {"bytes": len(b), "sha256": sha(b), "used": len(b.rstrip(b"\xff"))} if b else {"state": "absent"}


def uc_versions(unit):
    """K2 uC versions from provision's factory U-Boot capture (uc_get_version), if any."""
    for name in ("factory-uboot-info.txt", "factory-uboot-info-private.txt"):
        p = ROOT / "inventory" / unit / name
        if p.exists():
            t = p.read_text()
            seg = t.split("=> uc_get_version", 1)[-1].split("=> ", 1)[0] if "uc_get_version" in t else ""
            lines = [l.strip() for l in seg.splitlines() if "ersion" in l or "boot" in l.lower()]
            return {"source": name.replace("-private", ""), "lines": lines[:6]}
    return {"state": "not captured (provision.py register step)"}


def collect(a):
    unit = a.unit
    dumps = ROOT / "dumps" / unit
    blobs = dumps / "firmware"
    comp = {}
    for chip, f in (("cs0", "mtd0.bin.zst"), ("cs1", "mtd1.bin.zst")):
        if (dumps / f).exists():
            comp.update({k.replace("spi.", "spi.factory."): v for k, v in
                         spi_objects(alboot.load(str(dumps / f)), chip, blobs / "factory").items()})
    comp["uc"] = uc_versions(unit)
    cpld_info = dumps / "cpld" / "info.json"
    if not a.offline:
        live = bb_live(a.ip, a.busybox) if a.busybox else json.loads(ssh(a.ip, "python3 -", stdin=REMOTE))
        # current SPI, reassembled from the named mtd partitions
        # every partition of each chip, ordered by offset (layouts differ: the firmware windows)
        rows = ssh(a.ip, "for m in /sys/class/mtd/mtd[0-9]*; do case $m in *ro) continue;; esac; "
                         "echo $(basename $m) $(basename $(readlink -f $m/device)) $(cat $m/offset) $(cat $m/size) $(cat $m/name); done; true")
        chips = {}
        for l in rows.splitlines():
            f = l.split()
            if len(f) == 5:
                chips.setdefault(f[1], []).append((int(f[2]), int(f[3]), f[0], f[4]))
        part = {r[3]: r[2] for rs in chips.values() for r in rs}
        def chip(ch):
            rs = sorted(chips.get(ch, []))
            if not rs or rs[0][0] != 0 or any(a_[0] + a_[1] != b_[0] for a_, b_ in zip(rs, rs[1:])):
                return None
            return b"".join(ssh(a.ip, f"cat /dev/{r[2]}", binary=True) for r in rs)
        cs0, cs1 = chip("spi0.0"), chip("spi0.1")
        if cs0 and cs1:
            for chip, d in (("cs0", cs0), ("cs1", cs1)):
                comp[f"spi.current.{chip}.image"] = {"bytes": len(d)}
                comp.update({k.replace("spi.", "spi.current."): v for k, v in spi_objects(d, chip, blobs / "current").items()})
        else:
            comp["spi.current"] = {"state": f"not readable live (no SPI flash in this kernel's DT: mtd {sorted(part) or 'none'})"}
        # CPLD flash readback (read-only allowlist; cpld-readback.py)
        if a.cpld and a.busybox:
            cr = mod("cpldrb", ROOT / "cpld-readback.py")
            out = ssh(a.ip, "sh -s", stdin=cr.emit_sh(a.busybox))
            tmp = dumps / "cpld" / "readback-sh.txt"
            (dumps / "cpld").mkdir(parents=True, exist_ok=True)
            tmp.write_text(out)
            cr.parse_sh(str(tmp), str(dumps / "cpld"))
            tmp.unlink()
        elif a.cpld:
            ssh(a.ip, "rm -rf /tmp/cpld-inv; python3 - --out /tmp/cpld-inv >/dev/null",
                stdin=(ROOT / "cpld-readback.py").read_text())
            (dumps / "cpld").mkdir(parents=True, exist_ok=True)
            for f in ("cfg.bin", "ufm.bin", "info.json"):
                (dumps / "cpld" / f).write_bytes(ssh(a.ip, f"cat /tmp/cpld-inv/{f}", binary=True))
            ssh(a.ip, "rm -rf /tmp/cpld-inv")
        comp["cpld.version_reg"] = {"version": live["cpld_version"]}
        comp["tpm"] = tpm_component(live["tpm"])
        comp["preload_eeprom"] = preload_component(live["preload"])
        if live["eeprom"]:
            imgs = mod("images", AIRE / "tools" / "images.py")
            tlv = imgs.onie_tlv(bytes.fromhex(live["eeprom"]))
            comp["board_eeprom"] = {"fields": tlv["fields"], "withheld": tlv["withheld"], "crc_ok": tlv["crc_ok"]}
        comp["ssd"] = {k: live["ssd"].get(k) for k in ("ata_model", "firmware", "ata_version", "sectors")}
        comp["soc"] = {"midr": live["midr"]}
        usb = {}
        for u in live["usb"]:
            k = f"usb.{u['idVendor']}:{u['idProduct']}"
            e = usb.setdefault(k, {"product": u["product"], "manufacturer": u["manufacturer"], "count": 0, "bcdDevice": set()})
            e["count"] += 1
            e["bcdDevice"].add(u["bcdDevice"])
        for k, e in usb.items():
            e["bcdDevice"] = sorted(e["bcdDevice"])
            comp[k] = e
        for p in live["pci"]:
            comp[f"pci.{p['slot']}"] = {"id": p["id"], "rev": p["rev"], "class": p["class"]}
        comp["software"] = {"kernel": live["kernel"], "system": live["system"]}
    if cpld_info.exists():
        ci = json.loads(cpld_info.read_text())
        comp["cpld.part"] = {"idcode": ci["idcode"], "usercode": ci.get("usercode_isc", ci["usercode"]),
                             "feature_row": ci.get("feature_row_isc"), "feabits": ci.get("feabits_isc")}
        for f in ("cfg.bin", "ufm.bin"):
            b = (dumps / "cpld" / f).read_bytes()
            comp[f"cpld.{f[:3]}"] = {"bytes": len(b), "sha256": sha(b),
                                     "data_pages": sum(b[i:i + 16] not in (b"\0" * 16, b"\xff" * 16) for i in range(0, len(b), 16))}
    rec = {"unit": unit, "date": time.strftime("%Y-%m-%dT%H:%M:%S%z"), "mode": "offline" if a.offline else "live",
           "components": comp}
    out = ROOT / "inventory" / unit / "firmware.json"
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(rec, indent=1, sort_keys=True) + "\n")
    print(f"{out.relative_to(ROOT)}: {len(comp)} components; blobs in dumps/{unit}/firmware/")


# --- reading the records ------------------------------------------------------------------------

def load(unit):
    p = ROOT / "inventory" / unit / "firmware.json"
    if not p.exists():
        raise SystemExit(f"no {p.relative_to(ROOT)}: fw-inventory.py collect --unit {unit} first")
    return json.loads(p.read_text())["components"]


def brief(c):
    if not isinstance(c, dict):
        return str(c)
    for k in ("version", "sha256", "state"):
        if k in c and c[k] is not None:
            v = c[k]
            return v[:12] if k == "sha256" else str(v)
    return json.dumps(c, sort_keys=True)[:40]


def units_all():
    return sorted(p.parent.name for p in (ROOT / "inventory").glob("u*/firmware.json"))


def cmd_show(a):
    for k, v in sorted(load(a.unit).items()):
        print(f"{k:34} {json.dumps(v, sort_keys=True)}")


def cmd_table(a):
    us = a.units or units_all()
    recs = {u: load(u) for u in us}
    keys = sorted(set().union(*recs.values()))
    print(f"{'component':34} " + " ".join(f"{u:>14}" for u in us))
    for k in keys:
        vals = [brief(recs[u].get(k, "-")) for u in us]
        mark = " " if len(set(vals)) == 1 else "*"
        print(f"{k:33}{mark} " + " ".join(f"{v[:14]:>14}" for v in vals))


def byte_ranges(x, y, limit=12):
    n = max(len(x), len(y))
    ranges, start = [], None
    for i in range(n):
        diff = i >= len(x) or i >= len(y) or x[i] != y[i]
        if diff and start is None:
            start = i
        elif not diff and start is not None:
            ranges.append((start, i)); start = None
    if start is not None:
        ranges.append((start, n))
    total = sum(e - s for s, e in ranges)
    shown = ", ".join(f"{s:#x}+{e - s}" for s, e in ranges[:limit])
    return f"{total} bytes in {len(ranges)} ranges: {shown}{' ...' if len(ranges) > limit else ''}"


def cmd_diff(a):
    x, y = load(a.a), load(a.b)
    for u, mine, other in ((a.a, x, y), (a.b, y, x)):
        only = sorted(k for k in mine if k not in other)
        if only:
            groups = sorted({".".join(k.split(".")[:3]) for k in only})
            print(f"only in {u}: {len(only)} components ({', '.join(groups)})")
    for k in sorted(set(x) & set(y)):
        if x.get(k) == y.get(k):
            continue
        print(f"{k}:\n  {a.a}: {json.dumps(x.get(k), sort_keys=True)}\n  {a.b}: {json.dumps(y.get(k), sort_keys=True)}")
        if a.bytes:
            if k.startswith("spi."):
                kind, rest = k.split(".")[1], ".".join(k.split(".")[2:])
                f = f"spi.{rest}.bin"
                pa, pb = (ROOT / "dumps" / u / "firmware" / kind / f for u in (a.a, a.b))
            elif k in ("cpld.cfg", "cpld.ufm"):
                pa, pb = (ROOT / "dumps" / u / "cpld" / f"{k[5:]}.bin" for u in (a.a, a.b))
            else:
                continue
            if pa.exists() and pb.exists():
                print("  bytes: " + byte_ranges(pa.read_bytes(), pb.read_bytes()))


def cmd_drift(a):
    """SPI objects that differ between the factory dump and the live read of one unit."""
    c = load(a.unit)
    for k in sorted(c):
        if not k.startswith("spi.factory."):
            continue
        cur = k.replace("spi.factory.", "spi.current.")
        if c.get(cur) != c[k]:
            print(f"{k[12:]:24} factory {brief(c[k]):>14}  now {brief(c.get(cur, '-')):>14}")
    for k in sorted(c):
        if k.startswith("spi.current.") and k.replace("spi.current.", "spi.factory.") not in c \
                and not k.endswith(".image") and "blank" not in str(c[k].get("state")):
            print(f"{k[12:]:24} factory {'(blank)':>14}  now {brief(c[k]):>14}")


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="cmd", required=True)
    c = sub.add_parser("collect")
    c.add_argument("--unit", required=True)
    c.add_argument("--ip")
    c.add_argument("--offline", action="store_true", help="factory dumps only")
    c.add_argument("--no-cpld", dest="cpld", action="store_false", help="skip the CPLD flash readback")
    c.add_argument("--busybox", metavar="PATH", help="no python3 on the box: a static busybox there (e.g. in /dev/shm)")
    c.add_argument("--no-sudo", action="store_true", help="ssh as the invoking user (agent) instead of root's key")
    s = sub.add_parser("show"); s.add_argument("unit")
    t = sub.add_parser("table"); t.add_argument("units", nargs="*")
    d = sub.add_parser("diff"); d.add_argument("a"); d.add_argument("b"); d.add_argument("--bytes", action="store_true")
    r = sub.add_parser("drift"); r.add_argument("unit")
    a = ap.parse_args()
    if a.cmd == "collect":
        global NO_SUDO
        NO_SUDO = a.no_sudo
        if not a.offline and not a.ip:
            ap.error("collect needs --ip (or --offline)")
        collect(a)
    else:
        {"show": cmd_show, "table": cmd_table, "diff": cmd_diff, "drift": cmd_drift}[a.cmd](a)


if __name__ == "__main__":
    main()
