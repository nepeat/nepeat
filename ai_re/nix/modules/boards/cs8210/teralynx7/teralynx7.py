#!/usr/bin/env python3
"""Teralynx 7 (IVM77700) IFCS init config (config.yaml) from front-panel ports.

  teralynx7.py generate --spec spec.json --out config.yaml --sidecars DIR
  teralynx7.py check --spec spec.json config.yaml

spec.json (from default.nix): label, logLevel, node (node options),
settings (node option -> value or null; "netdev.<key>" for the netdev entry,
"log_level" for ifcs.options), cages (front-panel cages: lanes of each),
cageOptions (per cage: side host|line, media copper|optics), hostIpg, cpu,
extraDevports, ports (SONiC ports: name, alias, lanes, speed).

Rules re-expressed from the Celestica Briggs SDK V0.1.7 dynamic_config.sh
(sha256 805eef454263006dc025697640e1b98cc0ecb49b1b328fbe4a5a0a8a2695d1d5);
numeric values and field names checked against its libifcs.so 0.12.41
(388031e7...) and pyctypes/ifcs_ctypes.py (IFCS_DEVPORT_FEC_MODE_NONE/KR/KP/FC
= 0/1/2/3):

- lane numbering: lane = serdes_group * 8 + lane_in_group + 1 (Innovium SONiC);
  a devport is "lanes: <first lane in group>:<count>" of one serdes group.
- devport ids: front ports in cage order, breakout subports consecutive in
  lane order, from 1; sysport = devport id.
- port modes (the script's "dic" table: lanes per <n>x<speed>): a serdes
  group runs one of GROUP_MODES; lane rate 50G = PAM4, 25G/10G = NRZ.
- FEC ("fec-def" option): FEC_PLAIN is what every devport gets on the host
  side (lanes to a PHY) and what a line-side mode without a default gets;
  FEC_DEFAULT (media, mode) is the line-side default.
- link training: copper media in the PAM4 modes 1x400, 4x100, 8x50.
- host-side devports carry "ipg" (hostIpg).
Output is plain text in the vendor's field order, deterministic.
"""
import argparse
import json
import os
import re
import sys

LANES_PER_GROUP = 8

# (ports per group, lanes per port, speed Mb/s) the vendor configures; lanes
# of a group above count * lanes stay unused.
GROUP_MODES = {
    (1, 8, 400000), (1, 8, 200000), (1, 4, 100000), (1, 4, 40000),
    (2, 4, 200000), (2, 4, 100000), (4, 2, 100000), (4, 1, 25000),
    (8, 1, 50000), (4, 1, 10000),
}
# speed / lanes = lane rate (Gb/s): 50 PAM4, 25 and 10 NRZ.
MODULATION = {50: "PAM4", 25: "NRZ", 10: "NRZ"}

FEC_WORDS = {"NONE": "none", "KRFEC": "rs", "KPFEC": "rs", "FCFEC": "fc"}
# get_fec_disable_type: KP FEC in the PAM4 modes 1x400, 4x100, 8x50, else none.
FEC_PLAIN_KP = {"1x400", "4x100", "8x50"}
# defFecDic (line side, fec-def).
FEC_DEFAULT = {
    ("optics", "4x100"): "KPFEC", ("copper", "4x100"): "KPFEC",
    ("optics", "4x25"): "FCFEC", ("optics", "8x50"): "KPFEC",
    ("optics", "2x200"): "KPFEC", ("copper", "2x200"): "KPFEC",
    ("optics", "1x400"): "KPFEC", ("copper", "1x400"): "KPFEC",
    ("optics", "1x200"): "KPFEC", ("copper", "1x200"): "KPFEC",
    ("optics", "1x100"): "KRFEC", ("copper", "2x100"): "KRFEC",
    ("copper", "8x50"): "KPFEC",
}
LINK_TRAINING_MODES = {"1x400", "4x100", "8x50"}

NODE_ORDER = ["sku", "netdev", "buffer_management_mode", "max_lossless_tc", "mac_clk",
              "skip_pll_check", "sys_clk", "mbist_on_init", "ifc_clk", "txring", "rxring"]
ITEM_ORDER = ["txring_id", "rxring_id", "auto_create", "multi_interface", "desc_count",
              "prio", "queues", "netdev"]
DEVPORT_ORDER = {
    "cpu": ["id", "sysport", "type"],
    "eth": ["fec", "id", "lanes", "serdes_group", "speed", "sysport", "ipg", "type"],
    "other": ["id", "lanes", "serdes_group", "speed", "sysport", "type", "fec"],
}


class SpecError(Exception):
    pass


def fail(msg):
    raise SpecError("teralynx7: " + msg)


def ordered(d, order):
    keys = [k for k in order if k in d] + sorted(k for k in d if k not in order)
    return [(k, d[k]) for k in keys]


def q(v):
    s = str(v)
    if '"' in s or "\n" in s:
        fail("value %r cannot be written" % s)
    return '"%s"' % s


def emit(lines, indent, key, value):
    pad = " " * indent
    if isinstance(value, dict):
        lines.append("%s%s:" % (pad, key))
        for k, v in ordered(value, ITEM_ORDER):
            emit(lines, indent + 2, k, v)
    elif isinstance(value, list):
        if not value:
            lines.append("%s%s: []" % (pad, key))
            return
        lines.append("%s%s:" % (pad, key))
        for item in value:
            if not isinstance(item, dict) or not item:
                fail("%s: list items must be non-empty maps" % key)
            first = True
            for k, v in ordered(item, ITEM_ORDER):
                if isinstance(v, (dict, list)):
                    fail("%s.%s: nested values in list items are not supported" % (key, k))
                lines.append("%s%s %s: %s" % (pad, "-" if first else " ", k, q(v)))
                first = False
    else:
        lines.append("%s%s: %s" % (pad, key, q(value)))


def apply_settings(node, log_level, settings):
    node = json.loads(json.dumps(node))
    for k, v in sorted(settings.items()):
        if k == "log_level":
            log_level = v if v is not None else log_level
        elif k.startswith("netdev."):
            sub = k[len("netdev."):]
            nd = node.setdefault("netdev", [{}])
            if not nd:
                nd.append({})
            if v is None:
                nd[0].pop(sub, None)
            else:
                nd[0][sub] = v
        elif v is None:
            node.pop(k, None)
        else:
            node[k] = v
    return node, log_level


def mode_key(count, speed):
    return "%dx%d" % (count, speed // 1000)


def plan(spec):
    """Ports -> eth devports (dicts in devport order) with board fields."""
    cages = spec["cages"]
    opts = spec["cageOptions"]
    if len(opts) != len(cages):
        fail("cageOptions must have one entry per cage")
    where = {}
    for ci, lanes in enumerate(cages):
        for li, lane in enumerate(lanes):
            if lane in where:
                fail("lane %d is in two cages" % lane)
            where[lane] = (ci, li)
    ports = []
    for p in spec["ports"]:
        lanes = list(p["lanes"])
        name = "%s (%s)" % (p.get("alias", "?"), p.get("name", "?"))
        if not lanes:
            fail("%s: no lanes" % name)
        groups = {(l - 1) // LANES_PER_GROUP for l in lanes}
        if len(groups) != 1:
            fail("%s: lanes %s span serdes groups %s" % (name, lanes, sorted(groups)))
        g = groups.pop()
        first = (lanes[0] - 1) % LANES_PER_GROUP
        if lanes != list(range(lanes[0], lanes[0] + len(lanes))):
            fail("%s: lanes %s are not consecutive in lane order" % (name, lanes))
        for l in lanes:
            if l not in where:
                fail("%s: lane %d is not a front-panel lane" % (name, l))
        cis = {where[l][0] for l in lanes}
        if len(cis) != 1:
            fail("%s: lanes %s span cages" % (name, lanes))
        ci = cis.pop()
        n = len(lanes)
        if p["speed"] % n or (p["speed"] // n) // 1000 not in MODULATION:
            fail("%s: %d Mb/s on %d lane(s) is no Teralynx 7 lane rate" % (name, p["speed"], n))
        ports.append(dict(port=p, name=name, cage=ci + 1, group=g, first=first, n=n,
                          speed=p["speed"], order=(ci, where[lanes[0]][1])))
    ports.sort(key=lambda x: x["order"])
    by_group = {}
    for x in ports:
        by_group.setdefault(x["group"], []).append(x)
    for g, xs in sorted(by_group.items()):
        count, n, speed = len(xs), xs[0]["n"], xs[0]["speed"]
        if any(x["n"] != n or x["speed"] != speed for x in xs):
            fail("serdes group %d (cage %d): ports %s mix lane counts or speeds; a group runs one mode"
                 % (g, xs[0]["cage"], ", ".join(x["name"] for x in xs)))
        if (count, n, speed) not in GROUP_MODES:
            fail("serdes group %d (cage %d): %d port(s) of %d lane(s) at %d Mb/s is not a supported mode (%s)"
                 % (g, xs[0]["cage"], count, n, speed,
                    ", ".join("%dx%dG/%d lanes" % (c, s // 1000, l) for c, l, s in sorted(GROUP_MODES))))
        for k, x in enumerate(xs):
            if x["first"] != k * n:
                fail("%s: starts at lane %d of serdes group %d, the mode needs lane %d"
                     % (x["name"], x["first"], g, k * n))
            x["mode"] = mode_key(count, speed)
            x["count"] = count
            x["sub"] = k + 1
    dps = []
    for i, x in enumerate(ports):
        o = opts[x["cage"] - 1]
        side, media = o["side"], o["media"]
        if side not in ("host", "line") or media not in ("copper", "optics"):
            fail("cage %d: side must be host|line and media copper|optics" % x["cage"])
        fec = "KPFEC" if x["mode"] in FEC_PLAIN_KP else "NONE"
        if side == "line":
            fec = FEC_DEFAULT.get((media, x["mode"]), fec)
        dp = {"fec": fec, "id": i + 1, "lanes": "%d:%d" % (x["first"], x["n"]),
              "serdes_group": x["group"], "speed": "%dG" % (x["speed"] // 1000),
              "sysport": i + 1, "type": "eth"}
        if side == "host" and spec.get("hostIpg") is not None:
            dp["ipg"] = spec["hostIpg"]
        dps.append(dict(dp=dp, x=x, side=side, media=media,
                        link_training=media == "copper" and x["mode"] in LINK_TRAINING_MODES))
    extra_ids = {int(d["id"]) for d in spec.get("extraDevports", [])} | {int(spec["cpu"]["id"])}
    clash = [d["dp"]["id"] for d in dps if d["dp"]["id"] in extra_ids]
    if clash:
        fail("front-panel devport ids %s collide with the cpu/extra devports %s" % (clash, sorted(extra_ids)))
    extra_groups = {int(d["serdes_group"]) for d in spec.get("extraDevports", []) if "serdes_group" in d}
    if extra_groups & set(by_group):
        fail("serdes groups %s are used by front ports and extra devports" % sorted(extra_groups & set(by_group)))
    return dps


def render(spec, dps):
    node, log_level = apply_settings(spec["node"], spec.get("logLevel", "info"), spec.get("settings", {}))
    out = ["# IFCS init config, generated: %s" % spec.get("label", ""),
           "# The board's isg section (lane swap, polarity) is appended at runtime.",
           "ifcs:", "  options:", "    log_level: %s" % q(log_level), "nodes:",
           "- node_id: %s" % q(spec.get("nodeId", 0)), "  options:"]
    for k, v in ordered(node, NODE_ORDER):
        emit(out, 4, k, v)
    out.append("  devports:")
    devs = [dict(spec["cpu"], type="cpu")] + list(spec.get("extraDevports", [])) + [d["dp"] for d in dps]
    for d in devs:
        kind = "cpu" if d["type"] == "cpu" else "eth" if d["type"] == "eth" else "other"
        first = True
        for k, v in ordered(d, DEVPORT_ORDER[kind]):
            out.append("  %s %s: %s" % ("-" if first else " ", k, q(v)))
            first = False
    return "\n".join(out) + "\n"


def sidecars(spec, dps, d):
    os.makedirs(d, exist_ok=True)
    with open(os.path.join(d, "port-map"), "w") as f:
        f.write("# devport lanes speed_mbps fec (SAI_TERALYNX_PORT_MAP), front-panel devports\n")
        f.write("# generated: %s\n" % spec.get("label", ""))
        for x in dps:
            f.write("%d %s %d %s\n" % (x["dp"]["id"], ",".join(str(l) for l in x["x"]["port"]["lanes"]),
                                         x["x"]["speed"], FEC_WORDS[x["dp"]["fec"]]))
    with open(os.path.join(d, "init.cmd"), "w") as f:
        for x in dps:
            f.write("ifcs set devport %d rx_eq_fine_tune 1\n" % x["dp"]["id"])
        for e in spec.get("extraDevports", []):
            if e.get("rxEqFineTune", True):
                f.write("ifcs set devport %s rx_eq_fine_tune 1\n" % e["id"])
    with open(os.path.join(d, "link-training.cmd"), "w") as f:
        for x in dps:
            if x["link_training"]:
                f.write("ifcs set devport %d link_training 1\n" % x["dp"]["id"])
    layout = [{"devport": x["dp"]["id"], "cage": x["x"]["cage"], "sub": x["x"]["sub"],
               "count": x["x"]["count"], "lanes": x["x"]["n"], "speed": x["x"]["speed"],
               "mode": x["x"]["mode"], "side": x["side"], "media": x["media"],
               "serdes_group": x["x"]["group"], "lane_start": x["x"]["first"],
               "fec": x["dp"]["fec"], "link_training": x["link_training"],
               "name": x["x"]["port"].get("name"), "alias": x["x"]["port"].get("alias")} for x in dps]
    with open(os.path.join(d, "layout.json"), "w") as f:
        json.dump(layout, f, indent=1, sort_keys=True)
        f.write("\n")


def parse_devports(text):
    """devports of an IFCS yaml (this generator's or the vendor's subset)."""
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
            fail("cannot parse devport line %r" % raw)
        cur[m.group(1)] = m.group(2).strip()
    return devs


def check(spec, path):
    with open(path) as f:
        text = f.read()
    devs = parse_devports(text)
    ids = [d.get("id") for d in devs]
    if len(set(ids)) != len(ids):
        fail("%s: devport ids used twice" % path)
    cpu = [d for d in devs if d.get("type") == "cpu"]
    if len(cpu) != 1 or cpu[0].get("id") != str(spec["cpu"]["id"]):
        fail("%s: no single cpu devport %s" % (path, spec["cpu"]["id"]))
    for e in spec.get("extraDevports", []):
        got = [d for d in devs if d.get("id") == str(e["id"])]
        want = {k: str(v) for k, v in e.items() if k != "rxEqFineTune"}
        if len(got) != 1 or got[0] != want:
            fail("%s: devport %s is %s, expected %s" % (path, e["id"], got, want))
    eth = [d for d in devs if d.get("type") == "eth"]
    got = {}
    used = set()
    for d in eth:
        start, n = (int(v) for v in d["lanes"].split(":"))
        g = int(d["serdes_group"])
        lanes = tuple(g * LANES_PER_GROUP + start + i + 1 for i in range(n))
        if used & set(lanes):
            fail("%s: devport %s reuses lanes %s" % (path, d["id"], sorted(used & set(lanes))))
        used |= set(lanes)
        if d.get("sysport") != d["id"]:
            fail("%s: devport %s has sysport %s" % (path, d["id"], d.get("sysport")))
        if d.get("fec") not in FEC_WORDS:
            fail("%s: devport %s fec %r" % (path, d["id"], d.get("fec")))
        m = re.match(r"^(\d+)G$", d.get("speed", ""))
        if not m:
            fail("%s: devport %s speed %r" % (path, d["id"], d.get("speed")))
        got[lanes] = int(m.group(1)) * 1000
    want = {tuple(p["lanes"]): p["speed"] for p in spec["ports"]}
    if got != want:
        missing = sorted(set(want) - set(got))
        extra = sorted(set(got) - set(want))
        speeds = sorted(l for l in set(want) & set(got) if want[l] != got[l])
        fail("%s: eth devports differ from the ports: missing lanes %s, extra %s, speed differs on %s"
             % (path, missing, extra, speeds))
    # The written config read back must give the same plan (ids, modes, fec).
    dps = plan(spec)
    exp = [{k: str(v) for k, v in x["dp"].items()} for x in dps]
    if exp != eth:
        fail("%s: eth devports are not the planned ones" % path)
    return len(eth)


def main():
    ap = argparse.ArgumentParser()
    sub = ap.add_subparsers(dest="cmd", required=True)
    g = sub.add_parser("generate")
    g.add_argument("--spec", required=True)
    g.add_argument("--out", required=True)
    g.add_argument("--sidecars")
    c = sub.add_parser("check")
    c.add_argument("--spec", required=True)
    c.add_argument("config")
    a = ap.parse_args()
    with open(a.spec) as f:
        spec = json.load(f)
    try:
        if a.cmd == "generate":
            dps = plan(spec)
            with open(a.out, "w") as f:
                f.write(render(spec, dps))
            if a.sidecars:
                sidecars(spec, dps, a.sidecars)
        else:
            n = check(spec, a.config)
            print("teralynx7: %s: %d front-panel devports match the ports" % (a.config, n))
    except SpecError as e:
        print(str(e), file=sys.stderr)
        sys.exit(1)


if __name__ == "__main__":
    main()
