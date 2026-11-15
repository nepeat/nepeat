#!/usr/bin/env python3
"""AS7760 FMan KeyGen / BMI RX register probe (read-mostly, via /dev/mem).

Compares the management RX path with ethtool rxhash off (BMI enqueues to
the default FQ) and on (parser -> KeyGen -> 128 hashed PCD FQs). Only writes:
KeyGen action-register *read* commands, and ethtool feature toggles; the
script always restores rxhash off before exiting.
"""
import mmap, os, struct, subprocess, sys, time

FMAN = 0xFFE400000           # CCSR 0xffe000000 + FMan1 0x400000
KG, BMI_RX = 0xC1000, 0x88000  # KeyGen; 1G RX port 0x08 (DTSEC1/eth0)
GW = sys.argv[1] if len(sys.argv) > 1 else "10.60.2.1"

fd = os.open("/dev/mem", os.O_RDWR | os.O_SYNC)
mm = mmap.mmap(fd, 0x100000, mmap.MAP_SHARED, mmap.PROT_READ | mmap.PROT_WRITE, offset=FMAN)
rd = lambda off: struct.unpack(">I", mm[off:off + 4])[0]
def wr(off, val): mm[off:off + 4] = struct.pack(">I", val)

def kg_action(ar):
    wr(KG + 0x1FC, ar)
    for _ in range(100000):
        v = rd(KG + 0x1FC)
        if not v & 0x80000000:
            return v
    return v

SCHEME_NAMES = ["mode", "ekfc", "ekdv", "bmch", "bmcl", "fqb", "hc", "ppc"] + \
    [f"gec{i}" for i in range(8)] + ["spc", "dv0", "dv1", "ccbs", "mv", "om", "vsp"]

def dump(label):
    out = {"label": label}
    g = {n: rd(KG + o) for n, o in [("gcr", 0), ("eer", 0xC), ("eeer", 0x10), ("seer", 0x1C),
                                       ("seeer", 0x20), ("gsr", 0x24), ("tpc", 0x28), ("serc", 0x2C), ("fdor", 0x34)]}
    out["kg"] = g
    # scheme 0 entry -> indirect window (GO | READ | SCHEME | dummy port | id<<16)
    ar = kg_action(0xC0000000 | (0 << 16))
    out["scheme0_ar"] = ar
    out["scheme0"] = {n: rd(KG + 0x100 + 4 * i) for i, n in enumerate(SCHEME_NAMES)}
    # port 0x08 partition entry: GO | READ | PORT_ENTRY | hwport | WSEL_SP
    out["port8_sp_ar"] = kg_action(0xC0000000 | 0x02000000 | 0x08 | 0x8000)
    out["port8_sp"] = rd(KG + 0x100)
    out["port8_cpp_ar"] = kg_action(0xC0000000 | 0x02000000 | 0x08 | 0x4000)
    out["port8_cpp"] = rd(KG + 0x104)
    b = {n: rd(BMI_RX + o) for n, o in [("rcfg", 0), ("rst", 4), ("rfne", 0x20), ("rfca", 0x24), ("rfpne", 0x28),
                                           ("rfqid", 0x60), ("refqid", 0x64), ("rfsdm", 0x68), ("rfsem", 0x6C), ("rfene", 0x70),
                                           ("rstc", 0x200), ("rfrc", 0x204), ("rfbc", 0x208), ("rlfc", 0x20C), ("rffc", 0x210),
                                           ("rfdc", 0x214), ("rfldec", 0x218), ("rodc", 0x21C), ("rbdc", 0x220), ("rpec", 0x224)]}
    out["bmi_rx"] = b
    out["ip"] = subprocess.run(["ip", "-s", "-j", "link", "show", "eth0"], capture_output=True, text=True).stdout.strip()
    out["ethtool_S"] = [l.strip() for l in subprocess.run(["ethtool", "-S", "eth0"], capture_output=True, text=True).stdout.splitlines()
                        if any(k in l for k in ("rx packets", "rx error", "rx dropped", "interrupts", "dma error", "physical", "qman", "congest", "tdrop"))
                        and not l.strip().endswith(": 0")]
    out["irq"] = [l.split()[0] + " " + " ".join(l.split()[1:9]) + " " + " ".join(l.split()[9:]) for l in open("/proc/interrupts")
                  if "QMan" in l or "qman" in l or "fman" in l.lower()]
    print(repr(out), flush=True)

def sh(*cmd):
    r = subprocess.run(list(cmd), capture_output=True, text=True)
    print("$", " ".join(cmd), "->", r.returncode, (r.stdout + r.stderr).strip().replace("\n", " | ")[-400:], flush=True)

try:
    print("fqids:", open("/sys/class/net/eth0/device/net/eth0/fqids").read() if os.path.exists("/sys/class/net/eth0/device/net/eth0/fqids") else
          subprocess.run("cat /sys/class/net/eth0/fqids 2>/dev/null || find /sys -name fqids -path '*eth0*' -exec cat {} +", shell=True, capture_output=True, text=True).stdout, flush=True)
    dump("A off/idle")
    sh("ping", "-c", "5", "-i", "0.2", "-W", "1", GW)
    dump("B off/after-ping")
    sh("ethtool", "-K", "eth0", "rxhash", "on")
    time.sleep(1)
    dump("C on/idle")
    sh("ping", "-c", "5", "-i", "0.2", "-W", "1", GW)
    dump("D on/after-ping")
finally:
    sh("ethtool", "-K", "eth0", "rxhash", "off")
    time.sleep(1)
    sh("ping", "-c", "3", "-i", "0.2", "-W", "1", GW)
    dump("E restored-off/after-ping")
    sh("sh", "-c", "dmesg | tail -15")
