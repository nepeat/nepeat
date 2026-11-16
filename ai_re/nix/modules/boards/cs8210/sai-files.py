#!/usr/bin/env python3
"""CS8210-32X runtime SAI files from the vendor SDK tree (tuning data that
stays out of the store and of git), for the Teralynx SAI's profile.

  sai-files.py [--sdk DIR] [--layout layout.json] [--out DIR] [isg|preemphasis|all]

isg:          <out>/isg.yaml -> <sdk>/cel_configs/isg.yaml, or isg_R01.yaml when the
              board version is 2, as the vendor's auto_load_user.sh intends it
              (SAI_TERALYNX_CONFIG_APPEND). Board version = system CPLD (I2C 0x60,
              behind 0x70 ch2 -> 0x74 ch0 -> 0x71 ch2) register 0x01 bits 7:4, the
              value the stock sys_cpld.ko shows as board_version; --board-version
              overrides, an unreadable CPLD falls back to isg.yaml (with a warning).
              <out>/board-version records what was used.
preemphasis:  <out>/preemphasis.cmd, the per-devport TX/RX equaliser lines of the
              vendor tables for the devports of layout.json (one SAI_TERALYNX_INIT_CMDS
              file), re-indexed as the vendor's dynamic_config.sh does: the table of
              the cage's side, lane rate and media, the cage's lines, each subport
              taking its lanes' values. Only "ifcs set devport" lines; the vendor's
              "port enable", "shell" and link-training lines are not copied (admin
              state is SAI's, link training is in the generated link-training.cmd).
Writes are atomic (temp file + rename). Exit 1 when a needed vendor file is missing.
"""
import argparse
import glob
import json
import os
import re
import subprocess
import sys

SDK = "/var/lib/cs8210-sdk/root/root/sdk/R3181-J0001-01_V0.1.7_Briggs_SDK"
I2C_ROOT = "/sys/bus/platform/devices/fd880000.i2c"
SYS_CPLD_PATH = "*-0070/channel-2/*-0074/channel-0/*-0071/channel-2"
SYS_CPLD, REG_BOARD = 0x60, 0x01

# Vendor tables (cel_cmds/, file names from dynamic_config.sh): host side by
# lane rate, line side by lane rate and media.
HOST = {50: "preemphasis_PAM4_400G_32_HostLT.cmd", 25: "preemphasis_NRZ_200G_32_Host.cmd",
        10: "preemphasis_NRZ_40G_32_Host.cmd"}
LINE = {(50, "copper"): "rx_gs_ctle_PAM4_400G_32_DAC_v2.txt",
        (50, "optics"): "preemphasis_PAM4_400G_32_Optics.cmd",
        (25, "copper"): "preemphasis_NRZ_200G_32_DAC.cmd",
        (25, "optics"): "preemphasis_NRZ_200G_32_Optics.cmd",
        (10, "copper"): "preemphasis_NRZ_40G_32_DAC.cmd",
        (10, "optics"): "preemphasis_NRZ_40G_32_Optics.cmd"}
LINE_RE = re.compile(r"^\s*ifcs\s+set\s+devport\s+(\d+)\s+(\S+)\s+\[([^\]]*)\]\s*$")


def warn(msg):
    print("sai-files: " + msg, file=sys.stderr)


def put(path, text):
    tmp = path + ".tmp"
    with open(tmp, "w") as f:
        f.write(text)
    os.replace(tmp, path)


def link(path, target):
    tmp = path + ".tmp"
    if os.path.lexists(tmp):
        os.unlink(tmp)
    os.symlink(target, tmp)
    os.replace(tmp, path)


def board_version(root=I2C_ROOT):
    hits = sorted(glob.glob(os.path.join(root, "i2c-*", SYS_CPLD_PATH)))
    if not hits:
        raise OSError("system CPLD bus not found below %s" % root)
    bus = os.path.basename(os.path.realpath(hits[0])).split("-")[-1]
    out = subprocess.run(["i2cget", "-y", bus, hex(SYS_CPLD), hex(REG_BOARD)],
                         check=True, capture_output=True, text=True).stdout.strip()
    return int(out, 16) >> 4


def do_isg(sdk, out, override):
    if override is not None:
        ver, how = override, "--board-version"
    else:
        try:
            ver, how = board_version(), "system CPLD reg 0x01"
        except (OSError, subprocess.CalledProcessError, ValueError) as e:
            warn("board version unreadable (%s): using isg.yaml" % e)
            ver, how = None, "unreadable"
    name = "isg_R01.yaml" if ver == 2 else "isg.yaml"
    target = os.path.join(sdk, "cel_configs", name)
    if not os.path.isfile(target):
        warn("missing %s" % target)
        return False
    link(os.path.join(out, "isg.yaml"), target)
    shown = "unknown" if ver is None else ver
    put(os.path.join(out, "board-version"), "%s %s %s\n" % (shown, how, name))
    print("sai-files: board version %s (%s): %s" % (shown, how, target))
    return True


def table(path, cage):
    rows = []
    with open(path) as f:
        for line in f:
            m = LINE_RE.match(line)
            if m and int(m.group(1)) == cage:
                vals = [v.strip() for v in m.group(3).split(",") if v.strip()]
                rows.append((m.group(2), [int(v) for v in vals]))
    return rows


def preemphasis(sdk, layout):
    out, ok, missing = [], True, set()
    for d in layout:
        rate = d["speed"] // d["lanes"] // 1000
        name = HOST.get(rate) if d["side"] == "host" else LINE.get((rate, d["media"]))
        if name is None:
            warn("devport %d: no vendor table for %s side, %d G lanes, %s" % (d["devport"], d["side"], rate, d["media"]))
            continue
        path = os.path.join(sdk, "cel_cmds", name)
        if not os.path.isfile(path):
            if path not in missing:
                warn("missing %s" % path)
                missing.add(path)
            ok = False
            continue
        rows = table(path, d["cage"])
        if not rows:
            warn("devport %d: cage %d has no lines in %s" % (d["devport"], d["cage"], name))
        lo = (d["sub"] - 1) * d["lanes"]
        for attr, vals in rows:
            part = vals[lo:lo + d["lanes"]]
            if len(part) != d["lanes"]:
                warn("devport %d: %s %s has %d values, lanes %d-%d wanted: skipped"
                     % (d["devport"], name, attr, len(vals), lo, lo + d["lanes"] - 1))
                continue
            out.append("ifcs set devport %d %s [%s]" % (d["devport"], attr, ",".join(str(v) for v in part)))
    return out, ok


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--sdk", default=SDK)
    ap.add_argument("--layout")
    ap.add_argument("--out", default="/run/cs8210-sai")
    ap.add_argument("--board-version", type=int)
    ap.add_argument("what", nargs="?", default="all", choices=["all", "isg", "preemphasis"])
    a = ap.parse_args()
    os.makedirs(a.out, exist_ok=True)
    ok = True
    if a.what in ("all", "isg"):
        ok &= do_isg(a.sdk, a.out, a.board_version)
    if a.what in ("all", "preemphasis"):
        if not a.layout:
            ap.error("preemphasis needs --layout")
        with open(a.layout) as f:
            layout = json.load(f)
        lines, good = preemphasis(a.sdk, layout)
        ok &= good
        put(os.path.join(a.out, "preemphasis.cmd"), "".join(l + "\n" for l in lines))
        print("sai-files: %s: %d lines for %d devports" % (os.path.join(a.out, "preemphasis.cmd"), len(lines), len(layout)))
    sys.exit(0 if ok else 1)


if __name__ == "__main__":
    main()
