#!/usr/bin/env python3
"""Compare two Broadcom Tomahawk config.bcm files by what the SDK makes of them.

  sdk-config-compare.py [--full] A B   differences, grouped (exit 0 always;
                                       long groups are cut to 6 lines without --full)
  sdk-config-compare.py --dump A       the effective per-core view of one file

Keys are compared by physical core / lane, not by logical port number, so a
file that numbers its ports differently (factory: compact 1-8, 34-41, ...;
our generator: physical + pipe offset) still compares key by key. Rules
(logical ports, lanes per speed, which port a core's lane map is read from,
per-lane polarity) are the ones of nix/packages/openbcm/configs/tomahawk.py
(OpenBCM 6.5.27); an absent lane map is the SDK default 0x3210 (identity,
src/soc/esw/tomahawk/portctrl.c), an absent polarity is 0.
Run from anywhere; reads only the two files given.
"""
import importlib.util
import os
import re
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
spec = importlib.util.spec_from_file_location(
    "tomahawk", os.path.join(HERE, "../../nix/packages/openbcm/configs/tomahawk.py"))
th = importlib.util.module_from_spec(spec)
spec.loader.exec_module(th)

LANE_DEFAULT = 0x3210
PER_PORT = re.compile(r"^(portmap|xgxs_[rt]x_lane_map|phy_xaui_[rt]x_polarity_flip|serdes_preemphasis|"
                      r"serdes_driver_current|dport_map_port)(_lane\d)?_(\d+)$")


def view(path):
    with open(path) as f:
        kv = th.parse(f.read())
    ports, noop = th.sdk_ports(kv)
    front = {lp: v for lp, v in ports.items() if v[0] <= 128}
    p2l = {v[0]: lp for lp, v in front.items()}
    v = {"ports": sorted((phys, gbps, n) for phys, gbps, n in front.values()),
         "mgmt": sorted((lp, x[0], x[1]) for lp, x in ports.items() if x[0] > 128),
         "logical": {x[0]: lp for lp, x in ports.items()},
         "noop": noop, "lane_map": {}, "polarity": {}, "lane": {}, "dport": {}, "globals": {}, "stray": []}
    for core in sorted({(x[0] - 1) // 4 for x in front.values()}):
        first = core * 4 + 1
        for d in ("tx", "rx"):
            lp = p2l.get(first)
            raw = kv.get("xgxs_%s_lane_map_%d" % (d, lp)) if lp is not None else None
            v["lane_map"][(core, d)] = int(raw, 0) if raw is not None else LANE_DEFAULT
            mask = 0
            for lane in range(4):
                lp = p2l.get(first + lane)
                if lp is None:
                    continue
                x = int(kv.get("phy_xaui_%s_polarity_flip_%d" % (d, lp), "0"), 0)
                mask |= ((x & 1) << lane) if front[lp][2] == 1 else x
                if front[lp][2] >= 4:
                    break
            v["polarity"][(core, d)] = mask
    for lp, (phys, gbps, n) in front.items():
        for kind in ("preemphasis", "driver_current"):
            for i in range(n):
                got = kv.get("serdes_%s_lane%d_%d" % (kind, i, lp), kv.get("serdes_%s_%d" % (kind, lp)))
                if got is not None:
                    v["lane"][(kind, phys + i)] = got
    l2p = {lp: x[0] for lp, x in ports.items()}
    for k, val in kv.items():
        m = PER_PORT.match(k)
        if m:
            lp = int(m.group(3))
            if m.group(1) == "dport_map_port":
                v["dport"][l2p.get(lp, "logical %d (no port)" % lp)] = int(val)
            elif m.group(1) != "portmap" and lp not in l2p:
                v["stray"].append("%s=%s" % (k, val))
            continue
        if k in ("pbmp_oversubscribe", "pbmp_xport_xe"):
            bits = int(val, 0)
            v[k] = sorted(str(l2p.get(b, "logical %d (no port)" % b)) for b in range(bits.bit_length()) if (bits >> b) & 1)
            continue
        v["globals"][k] = val
    # lane maps on a port that is not the first lane of its core: the SDK does not read them
    for k, val in kv.items():
        m = re.match(r"^xgxs_([rt]x)_lane_map_(\d+)$", k)
        if m and int(m.group(2)) in front and (front[int(m.group(2))][0] - 1) % 4:
            lp = int(m.group(2)); core = (front[lp][0] - 1) // 4
            if int(val, 0) != v["lane_map"][(core, m.group(1))]:
                v["stray"].append("%s=%s on lane %d of core %d: not read, and differs from the core's %#06x" %
                                  (k, val, (front[lp][0] - 1) % 4, core, v["lane_map"][(core, m.group(1))]))
    return v


def cage_of_core():
    """core -> front-panel cage: both boards share the AS7712 cage order."""
    order = [12, 13, 14, 15, 16, 17, 18, 19, 8, 9, 10, 11, 20, 21, 22, 23, 24, 25, 26, 27, 4, 5, 6, 7, 28, 29, 30, 31, 0, 1, 2, 3]
    return {core: i + 1 for i, core in enumerate(order)}


def dump(path):
    v = view(path)
    cage = cage_of_core()
    print("ports: %d front (%s), management %s" % (
        len(v["ports"]), ", ".join(sorted({"%dG x%d lane" % (g, n) for _, g, n in v["ports"]})), v["mgmt"]))
    print("cage core lanes     tx_map rx_map tx_pol rx_pol")
    for core in sorted(cage, key=lambda c: cage[c]):
        if (core, "tx") in v["lane_map"]:
            print("%4d %4d %3d-%-3d    0x%04x 0x%04x 0x%x    0x%x" % (
                cage[core], core, core * 4 + 1, core * 4 + 4, v["lane_map"][(core, "tx")], v["lane_map"][(core, "rx")],
                v["polarity"][(core, "tx")], v["polarity"][(core, "rx")]))
    for k, val in sorted(v["globals"].items()):
        print("global %s=%s" % (k, val))
    for s in v["noop"] + v["stray"]:
        print("ignored by the SDK: %s" % s)


def compare(a, b, full=False):
    A, B = view(a), view(b)
    cage = cage_of_core()
    n = 0

    def out(section, lines):
        nonlocal n
        print("## %s: %s" % (section, "same" if not lines else "%d difference(s)" % len(lines)))
        for l in lines if full or len(lines) <= 8 else lines[:6] + ["... %d more (--full)" % (len(lines) - 6)]:
            print("  " + l)
        n += len(lines)

    print("A = %s\nB = %s" % (a, b))
    out("front-panel ports (first lane, Gb/s, lanes)",
        ["only A: %s" % (x,) for x in sorted(set(A["ports"]) - set(B["ports"]))] +
        ["only B: %s" % (x,) for x in sorted(set(B["ports"]) - set(A["ports"]))])
    out("management ports (logical, physical, Gb/s)",
        ["A %s, B %s" % (A["mgmt"], B["mgmt"])] if A["mgmt"] != B["mgmt"] else [])
    ren = sorted((p, A["logical"][p], B["logical"][p]) for p in A["logical"]
                 if p in B["logical"] and A["logical"][p] != B["logical"][p])
    out("logical port numbers (same physical port, other number)",
        ["%d ports renumbered, e.g. physical %d: A %d, B %d; physical %d: A %d, B %d" %
         (len(ren), ren[0][0], ren[0][1], ren[0][2], ren[-1][0], ren[-1][1], ren[-1][2])] if ren else [])
    for what, key in (("lane maps", "lane_map"), ("polarity (4-bit mask per core)", "polarity")):
        lines = []
        for core, d in sorted(set(A[key]) | set(B[key]), key=lambda x: (cage[x[0]], x[1])):
            x, y = A[key].get((core, d)), B[key].get((core, d))
            if x != y:
                fmt = (lambda z: "-" if z is None else ("0x%04x" % z if key == "lane_map" else "0x%x" % z))
                lines.append("cage %2d (core %2d, lanes %d-%d) %s: A %s, B %s" %
                             (cage[core], core, core * 4 + 1, core * 4 + 4, d, fmt(x), fmt(y)))
        out(what, lines)
    lines = []
    for k in sorted(set(A["lane"]) | set(B["lane"]), key=lambda x: (x[0], x[1])):
        if A["lane"].get(k) != B["lane"].get(k):
            lines.append("%s lane %d: A %s, B %s" % (k[0], k[1], A["lane"].get(k), B["lane"].get(k)))
    if len(lines) > 12:
        kinds = {}
        for k in set(A["lane"]) | set(B["lane"]):
            if A["lane"].get(k) != B["lane"].get(k):
                e = kinds.setdefault(k[0], [0, 0, 0])
                e[0 if k not in B["lane"] else 1 if k not in A["lane"] else 2] += 1
        lines = ["%s: %d lanes only in A, %d only in B, %d with another value" % (k, *e) for k, e in sorted(kinds.items())]
    out("per-lane TX keys (serdes_preemphasis, serdes_driver_current)", lines)
    for k in ("pbmp_oversubscribe", "pbmp_xport_xe"):
        x, y = A.get(k, []), B.get(k, [])
        out("%s (as physical ports)" % k,
            ["only A: %s" % sorted(set(x) - set(y), key=str), "only B: %s" % sorted(set(y) - set(x), key=str)] if x != y else [])
    out("dport_map_port (diag-shell number of each physical port)",
        ["physical %s: A %s, B %s" % (p, A["dport"].get(p), B["dport"].get(p))
         for p in sorted(set(A["dport"]) | set(B["dport"]), key=str) if A["dport"].get(p) != B["dport"].get(p)])
    out("global keys",
        ["%s: A %s, B %s" % (k, A["globals"].get(k, "(absent)"), B["globals"].get(k, "(absent)"))
         for k in sorted(set(A["globals"]) | set(B["globals"])) if A["globals"].get(k) != B["globals"].get(k)])
    ia, ib = A["noop"] + A["stray"], B["noop"] + B["stray"]
    out("lines the SDK ignores (fixed logical ports, lane maps off a core's first lane)",
        ["only A: " + s for s in ia if s not in ib] + ["only B: " + s for s in ib if s not in ia])
    if ia and ia == ib:
        print("  (both files: %s)" % "; ".join(x.split(":")[0] for x in ia))
    print("total: %d" % n)


if __name__ == "__main__":
    if len(sys.argv) == 3 and sys.argv[1] == "--dump":
        dump(sys.argv[2])
    elif len(sys.argv) == 3:
        compare(sys.argv[1], sys.argv[2])
    elif len(sys.argv) == 4 and sys.argv[1] == "--full":
        compare(sys.argv[2], sys.argv[3], True)
    else:
        sys.exit(__doc__)
