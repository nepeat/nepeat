#!/usr/bin/env python3
"""Compare our generated CS8210 IFCS config with the vendor's dynamic_config.sh.

  python3 -I sonic_sdk_config_check.py SDK_DIR CONFIG_YAML SIDECARS_DIR [--tool sai-files.py]

SDK_DIR      the Briggs SDK V0.1.7 tree (local copy; vendor files stay out of git)
CONFIG_YAML  our generated config.yaml (boards/cs8210/sdk-config.nix, $out)
SIDECARS_DIR its sidecars output (port-map, init.cmd, link-training.cmd, layout.json)

The vendor port spec is derived from our layout.json (cage -> media_<n>x<speed>G:fec-def),
the vendor script runs on a temporary copy of dynamic_config.sh + cel_configs + cel_cmds
(its hard-coded /tmp/tfile moved into that copy), and its dynamic_config.yaml,
dynamic_misc.cmd and dynamic_preemphasis.cmd are compared with ours field by field.
Our pre-emphasis is what boards/cs8210/sai-files.py writes at runtime from the same
vendor tables. Prints a report; exit 1 on an unexpected difference.
"""
import argparse
import hashlib
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
TOOL = os.path.normpath(os.path.join(HERE, "../../../nix/modules/boards/cs8210/sai-files.py"))
TAP = re.compile(r"^ifcs set devport (\d+) (\S+) \[([^\]]*)\]$")
LT = re.compile(r"^ifcs set devport (\d+) link_training (\S+)$")
SET = re.compile(r"^ifcs set devport (\d+) (\S+) (\S+)$")
PORT = re.compile(r"^port (enable|disable) (\d+)$")
problems = []


def sha(path):
    with open(path, "rb") as f:
        return hashlib.sha256(f.read()).hexdigest()


def norm(line):
    return " ".join(line.replace("\t", " ").split())


def lines(path):
    with open(path) as f:
        return [norm(l) for l in f if norm(l)]


def bad(msg):
    problems.append(msg)
    print("  DIFF " + msg)


def parse_devports(text):
    devs, cur, inside, base = [], None, False, None
    for raw in text.splitlines():
        line = raw.rstrip()
        s = line.strip()
        if not s or s.startswith("#"):
            continue
        ind = len(line) - len(line.lstrip(" "))
        if s == "devports:":
            inside, base = True, ind
            continue
        if not inside:
            continue
        if ind <= base and not s.startswith("- "):
            inside = False
            continue
        if s.startswith("- "):
            cur = {}
            devs.append(cur)
            s = s[2:]
        m = re.match(r'^([A-Za-z_0-9]+):\s*"?([^"]*)"?\s*$', s)
        if not m or cur is None:
            raise SystemExit("cannot parse devport line %r" % raw)
        cur[m.group(1)] = m.group(2).strip()
    return devs


def node_block(text):
    out = []
    for raw in text.splitlines():
        if raw.strip().startswith("#") or not raw.strip():
            continue
        if raw.strip() == "devports:":
            break
        out.append(raw.rstrip())
    return out


def spec_of(layout):
    per = {}
    for d in layout:
        per.setdefault(d["cage"], set()).add((d["media"], d["count"], d["speed"]))
    modes = []
    for cage in sorted(per):
        if len(per[cage]) != 1:
            raise SystemExit("cage %d mixes modes: the vendor spec cannot express it" % cage)
        media, count, speed = per[cage].pop()
        modes.append((cage, "%s_%dx%dG:fec-def" % (media, count, speed // 1000)))
    parts, i = [], 0
    while i < len(modes):
        j = i
        while j + 1 < len(modes) and modes[j + 1][1] == modes[i][1] and modes[j + 1][0] == modes[j][0] + 1:
            j += 1
        rng = str(modes[i][0]) if i == j else "%d-%d" % (modes[i][0], modes[j][0])
        parts.append("%s:%s" % (rng, modes[i][1]))
        i = j + 1
    return ";".join(parts)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("sdk")
    ap.add_argument("config")
    ap.add_argument("sidecars")
    ap.add_argument("--tool", default=TOOL)
    a = ap.parse_args()
    side = lambda n: os.path.join(a.sidecars, n)
    with open(side("layout.json")) as f:
        layout = json.load(f)
    spec = spec_of(layout)
    print("== spec (from our layout): %s" % spec)
    print("inputs (sha256):")
    for p in [os.path.join(a.sdk, "dynamic_config.sh"), os.path.join(a.sdk, "cel_configs/common_PAM4_400G_32_nd.yaml"),
              os.path.join(a.sdk, "cel_configs/isg.yaml"), a.config, side("port-map"), side("init.cmd"),
              side("link-training.cmd"), side("layout.json"), a.tool]:
        print("  %s  %s" % (sha(p), p))
    for d in layout:
        if (d["side"] == "host") != (d["cage"] <= 16):
            bad("devport %d: cage %d side %s, the vendor's host side is cages 1-16" % (d["devport"], d["cage"], d["side"]))

    tmp = tempfile.mkdtemp(prefix="cs8210-vendor-")
    try:
        shutil.copy(os.path.join(a.sdk, "dynamic_config.sh"), tmp)
        for d in ("cel_configs", "cel_cmds"):
            shutil.copytree(os.path.join(a.sdk, d), os.path.join(tmp, d))
        script = os.path.join(tmp, "dynamic_config.sh")
        with open(script) as f:
            s = f.read()
        if s.count("tfile=/tmp/tfile") != 1:
            raise SystemExit("dynamic_config.sh: expected one tfile=/tmp/tfile")
        with open(script, "w") as f:
            f.write(s.replace("tfile=/tmp/tfile", "tfile=$runPath/tfile"))
        r = subprocess.run(["bash", script, spec], cwd=tmp, capture_output=True, text=True)
        print("vendor dynamic_config.sh: exit %d%s" % (r.returncode, (", stderr: " + r.stderr.strip()) if r.stderr.strip() else ""))
        if r.returncode:
            raise SystemExit(1)
        vy = os.path.join(tmp, "cel_configs/dynamic_config.yaml")
        vm = os.path.join(tmp, "cel_cmds/dynamic_misc.cmd")
        vp = os.path.join(tmp, "cel_cmds/dynamic_preemphasis.cmd")
        print("vendor outputs (sha256):")
        for p in (vy, vm, vp):
            print("  %s  %s" % (sha(p), os.path.relpath(p, tmp)))

        # 1. devports
        with open(vy) as f:
            vdev = parse_devports(f.read())
        with open(a.config) as f:
            ctext = f.read()
        odev = parse_devports(ctext)
        print("== devports: vendor %d, ours %d" % (len(vdev), len(odev)))
        vids, oids = [d.get("id") for d in vdev], [d.get("id") for d in odev]
        if vids != oids:
            bad("devport id order: vendor %s ours %s" % (vids, oids))
        fields = {}
        for v, o in zip(vdev, odev):
            for k in sorted(set(v) | set(o)):
                fields[k] = fields.get(k, 0) + 1
                if v.get(k) != o.get(k):
                    bad("devport %s field %s: vendor %r ours %r" % (v.get("id"), k, v.get(k), o.get(k)))
        print("  compared fields (devports carrying them): %s" % ", ".join("%s %d" % kv for kv in sorted(fields.items())))
        # serdes groups per cage = the script's isgMap
        m = re.search(r"^isgMap=\(([^)]*)\)", s, re.M)
        isg = [int(x) for x in m.group(1).split()]
        groups = {}
        for d in layout:
            groups.setdefault(d["cage"], set()).add(d["serdes_group"])
        wrong = [c for c, g in groups.items() if g != {isg[c - 1]}]
        if wrong:
            bad("serdes group differs from isgMap for cages %s" % wrong)
        with open(os.path.join(a.sdk, "cel_configs/isg.yaml")) as f:
            fp = re.findall(r"front panel port (\d+)\s*\n\s*- id: \"(\d+)\"", f.read())
        if len(fp) != 32 or any(isg[int(p)] != int(g) for p, g in fp):
            bad("isgMap differs from the isg.yaml front-panel comments")
        print("  serdes group of every cage = isgMap = isg.yaml comments: %s" % ("yes" if not wrong else "NO"))

        # 2. node options = common_PAM4_400G_32_nd.yaml (sku path aside)
        with open(os.path.join(a.sdk, "cel_configs/common_PAM4_400G_32_nd.yaml")) as f:
            vnode = node_block(f.read())
        onode = node_block(ctext)
        sku = lambda l: re.sub(r'sku: ".*"', 'sku: <sku>', l)
        vn, on = [sku(l) for l in vnode], [sku(l) for l in onode]
        print("== node options vs common_PAM4_400G_32_nd.yaml (%d lines; sku path not compared)" % len(vn))
        if vn != on:
            for i in range(max(len(vn), len(on))):
                if (vn[i] if i < len(vn) else None) != (on[i] if i < len(on) else None):
                    bad("node line %d: vendor %r ours %r" % (i + 1, vn[i] if i < len(vn) else None, on[i] if i < len(on) else None))
        print("  vendor sku %s, ours %s" % ([l.strip() for l in vnode if "sku:" in l], [l.strip() for l in onode if "sku:" in l]))

        # 3. misc
        vmisc, omisc = lines(vm), lines(side("init.cmd"))
        vset = [l for l in vmisc if SET.match(l)]
        print("== misc: vendor %d lines (%d 'ifcs set devport'), ours %d" % (len(vmisc), len(vset), len(omisc)))
        if vset != omisc:
            bad("ifcs set lines differ: vendor-only %s ours-only %s"
                % ([l for l in vset if l not in omisc][:6], [l for l in omisc if l not in vset][:6]))
        kinds = {}
        for l in vmisc:
            if not SET.match(l):
                k = " ".join(l.split()[:2])
                kinds[k] = kinds.get(k, 0) + 1
        # Counted by command, not quoted: the values are vendor configuration.
        print("  vendor lines not generated (outside the backend's CLI subset): %s"
              % ", ".join("%s x%d" % kv for kv in sorted(kinds.items())))

        # 4. pre-emphasis
        out = os.path.join(tmp, "ours")
        r = subprocess.run([sys.executable, "-I", a.tool, "--sdk", tmp, "--layout", side("layout.json"),
                            "--out", out, "preemphasis"], capture_output=True, text=True)
        print("our sai-files.py preemphasis: exit %d; %s%s" % (r.returncode, r.stdout.strip(),
              ("; stderr: " + r.stderr.strip()) if r.stderr.strip() else ""))
        if r.returncode:
            bad("sai-files.py failed")
        vpre = lines(vp)
        vtap = [l for l in vpre if TAP.match(l) and not LT.match(l)]
        vlt = [l for l in vpre if LT.match(l)]
        vport = [l for l in vpre if PORT.match(l)]
        vother = [l for l in vpre if l not in vtap and l not in vlt and l not in vport]
        otap = lines(os.path.join(out, "preemphasis.cmd"))
        olt = lines(side("link-training.cmd"))
        print("== pre-emphasis: vendor %d tap lines, %d link_training, %d port enable/disable, %d other (%s)"
              % (len(vtap), len(vlt), len(vport), len(vother), ", ".join(sorted({" ".join(l.split()[:2]) for l in vother}))))
        print("  ours: %d tap lines (runtime), %d link_training (generated)" % (len(otap), len(olt)))
        canon = lambda l: (lambda m: (int(m.group(1)), m.group(2), tuple(int(x) for x in m.group(3).split(","))))(TAP.match(l))
        if [canon(l) for l in vtap] != [canon(l) for l in otap]:
            vs, os_ = [canon(l) for l in vtap], [canon(l) for l in otap]
            bad("tap lines differ: vendor-only %s ours-only %s" % ([x for x in vs if x not in os_][:4], [x for x in os_ if x not in vs][:4]))
        else:
            print("  tap lines: identical (devport, attribute, values, order)")
        if vlt != olt:
            bad("link_training: vendor %s ours %s" % (vlt, olt))
        else:
            print("  link_training lines: identical (%s)" % (", ".join(LT.match(l).group(1) for l in vlt) or "none"))
        bad_admin = [l for l in vport if not l.startswith("port enable")]
        print("  port enable lines: %d (devports %s); disable: %s; SAI admin state replaces them"
              % (len(vport) - len(bad_admin), ",".join(PORT.match(l).group(2) for l in vport if l.startswith("port enable")), bad_admin))
        # Could the static vendor tables be used as they are?
        used, seen = [], set()
        tool_ns = {}
        with open(a.tool) as f:
            exec(compile(f.read(), a.tool, "exec"), tool_ns)
        for d in layout:
            rate = d["speed"] // d["lanes"] // 1000
            n = tool_ns["HOST"].get(rate) if d["side"] == "host" else tool_ns["LINE"].get((rate, d["media"]))
            if n and n not in seen:
                seen.add(n)
                used.append(n)
        static = [l for n in used for l in lines(os.path.join(tmp, "cel_cmds", n)) if TAP.match(l)]
        same = sorted(canon(l) for l in static) == sorted(canon(l) for l in vtap)
        print("  static tables %s %s the vendor's dynamic tap lines for this spec" % (" + ".join(used), "EQUAL" if same else "do NOT equal"))
    finally:
        shutil.rmtree(tmp)
    print("== result: %s" % ("PASS" if not problems else "FAIL (%d differences)" % len(problems)))
    sys.exit(1 if problems else 0)


if __name__ == "__main__":
    main()
