#!/usr/bin/env python3
"""Broadcom Tomahawk (BCM5696x/5693x, SDK 6.5) config.bcm from front-panel ports.

  tomahawk.py generate --base BASE --spec SPEC.json --out OUT
  tomahawk.py check    --base BASE --spec SPEC.json CONFIG
  tomahawk.py base     --globals REFERENCE --cores CORES.json --out BASE

BASE is a reference config with one 4-lane 100G port per SerDes core (e.g.
sonic-buildimage's Accton AS7712-32X th-as7712-32x100G.config.bcm). It
supplies the global keys and the board's per-core/per-lane values: lane maps,
polarity flips, TX pre-emphasis and driver current. SPEC is JSON:
  ports:      [{name, alias, lanes: [physical ports], speed: Mb/s}] (SONiC PORT)
  management: [{logical, phys, speed: Gb/s}] (TSC-E management ports)
  settings:   {key: value | null}  global keys added/overridden/removed
  dportMap:   bool  diag-shell port numbers in front-panel order
  laneKeys:   which per-lane TX keys of BASE to carry: preemphasis, driver_current
  label:      text for the header (where BASE came from)

base makes a BASE out of a board's own per-core values: REFERENCE's lines
without the keys generate writes (ports, per-port keys, port bitmaps), then
one 100G port per core of CORES, [{lane, tx, rx, txPolarity, rxPolarity}]
(lane = the core's first physical port; all but lane optional).

generate writes the config; check re-reads a config with the SDK's own rules
(independent of generate) and fails unless its ports are exactly SPEC's ports
(lanes, speeds) and the SDK-effective lane maps, polarity and pre-emphasis
equal BASE's on every lane in use. Rules, from OpenBCM 6.5.27:
  src/soc/esw/tomahawk.c  soc_tomahawk_port_config_init, the "Tomahawk port
    mapping" table and _soc_tomahawk_phy_dev_port_check: logical (device) port
    ranges per pipe 1-32 / 34-66 / 68-100 / 102-133 for physical 1-32 / 33-64 /
    65-96 / 97-128; 0 = CPU, 33/67/101/135 = loopback (fixed, a portmap there
    is ignored), 134 unused; management port 129 in pipe 1, 131 in pipe 2.
    Lanes per port from the speed ("Setup port_num_lanes"). All ports are
    oversubscribed when pbmp_oversubscribe is non-empty.
  src/soc/esw/drv.c  a CLPORT/XLPORT port outside pbmp_xport_xe is HiGig.
  src/soc/esw/tomahawk/portctrl.c  lane map read from the logical port on the
    core's first lane; polarity of a 1-lane port is bit 0 of its value shifted
    to its lane, of a 2/4-lane port a core-relative mask, OR-ed per core.
  src/soc/esw/portctrl.c  serdes_preemphasis_lane<i>_<port> (lane i of the
    port; value post<<16 | main<<8 | pre), same for serdes_driver_current.
    The base's values are tuned for its 25.78G lanes: they go to ports on the
    same VCO (25/50/100G); 10G/40G lanes keep the SDK defaults.
  src/soc/esw/tdm/tomahawk/tdm_th_main.c  a core with any lane in use must use
    its lane 0; tomahawk.c port ratio modes: 4 | 2+2 | 2+1+1 | 1+1+2 | 1+1+1+1.
  src/soc/portmod/pms/pm4x25.c  _pm4x25_speed_to_vco: one VCO per core here
    (TSCF, single PLL): 10G and 40G-on-4-lanes need 20.625G, 25/50/100G and
    40G-on-2-lanes 25.78G, so a core does not mix the two.
Logical numbers: physical + 0/1/3/5 by pipe, as the public TH configs do
(factory and sonic-buildimage 32x100G / 128x25G / Celestica DX010): stable,
a breakout never renumbers other ports.
Line length: include/sal/appl/config.h SAL_CONFIG_STR_MAX is 256 and
src/sal/appl/config.c reads the file with fgets into that buffer, so the
SDK takes a longer line (comments too) as two: the rest is a "format error
... (ignored)" or, for a key, a lost value. generate and check refuse them.
"""
import json
import re
import sys
import textwrap

FIXED = {0: "CPU", 33: "loopback 0", 67: "loopback 1", 101: "loopback 2", 134: "unused", 135: "loopback 3"}
PIPE_OFFSET = [0, 1, 3, 5]
BANDWIDTHS = {1, 10, 11, 20, 21, 25, 27, 40, 42, 50, 53, 100, 106}
VCO = {(10, 1): "20.625G", (20, 1): "20.625G", (20, 2): "20.625G", (40, 4): "20.625G",
       (25, 1): "25.78G", (40, 2): "25.78G", (50, 2): "25.78G", (100, 4): "25.78G"}
BASE_VCO = VCO[(100, 4)]   # the base's ports: its TX taps are for this lane rate
MGMT_PHYS = {129: range(34, 67), 131: range(68, 101)}
GENERATED = re.compile(r"^(portmap|xgxs_[rt]x_lane_map|phy_xaui_[rt]x_polarity_flip|serdes_preemphasis|"
                       r"serdes_driver_current|pbmp_oversubscribe|pbmp_xport_xe|dport_map)")
MAX_LINE = 254   # characters before the newline that fgets(buf, SAL_CONFIG_STR_MAX) returns as one line


class ConfigError(Exception):
    pass


def parse(text):
    """key -> value, in file order (later wins); '#' starts a comment anywhere."""
    out = {}
    for raw in text.splitlines():
        line = raw.split("#", 1)[0].strip()
        if not line:
            continue
        if "=" not in line:
            raise ConfigError("not key=value: %r" % raw)
        k, v = line.split("=", 1)
        out[k.strip()] = v.strip()
    return out


def dev_range(phys):
    if 1 <= phys <= 128:
        lo = [1, 34, 68, 102][(phys - 1) // 32]
        hi = [32, 66, 100, 133][(phys - 1) // 32]
        return range(lo, hi + 1)
    return MGMT_PHYS.get(phys, range(0))


def logical_of(phys):
    return phys + PIPE_OFFSET[(phys - 1) // 32]


def long_lines(text):
    return ["line %d: %d characters, the SDK reads at most %d as one line" % (i, len(l), MAX_LINE)
            for i, l in enumerate(text.split("\n"), 1) if len(l) > MAX_LINE]


def make_base(reference, cores):
    """A base config: the reference without the generated keys + one 100G port per core."""
    out = [l if l.endswith("\n") else l + "\n"
           for l in reference.splitlines(keepends=True) if not GENERATED.match(l.lstrip())]
    for c in cores:
        phys = c["lane"]
        if not (1 <= phys <= 128) or phys % 4 != 1:
            raise ConfigError("cores: lane %s is not the first physical port of a core (1, 5, ... 125)" % phys)
        lp = logical_of(phys)
        out.append("portmap_%d=%d:100\n" % (lp, phys))
        for key, attr in (("xgxs_tx_lane_map", "tx"), ("xgxs_rx_lane_map", "rx"),
                          ("phy_xaui_tx_polarity_flip", "txPolarity"), ("phy_xaui_rx_polarity_flip", "rxPolarity")):
            if attr in c:
                out.append("%s_%d=%s\n" % (key, lp, c[attr]))
    return "".join(out)


# ---------------------------------------------------------------- base config
class Base:
    def __init__(self, text):
        kv = parse(text)
        self.globals = {}
        self.core_port = {}       # core -> logical port in base
        self.lane_map = {}        # (core, "tx"/"rx") -> value text
        self.polarity = {}        # (core, "tx"/"rx") -> 4-bit core mask
        self.lane = {}            # (kind, physical lane) -> value text
        per_port = []
        for k, v in kv.items():
            m = re.match(r"^portmap_(\d+)$", k)
            if m:
                lp = int(m.group(1))
                phys, _, rest = v.partition(":")
                phys, speed = int(phys), rest.split(":")[0]
                if not (1 <= phys <= 128):
                    continue          # management ports: from the spec
                if speed != "100" or phys % 4 != 1:
                    raise ConfigError("base %s=%s: expected one 100G port per core" % (k, v))
                self.core_port[(phys - 1) // 4] = lp
            elif GENERATED.match(k):
                if not k.startswith(("pbmp_", "dport_map")):
                    per_port.append((k, v))
            else:
                self.globals[k] = v
        by_port = {lp: c for c, lp in self.core_port.items()}
        for k, v in per_port:
            m = (re.match(r"^xgxs_([rt]x)_lane_map_(\d+)$", k) or
                 re.match(r"^phy_xaui_([rt]x)_polarity_flip_(\d+)$", k))
            if m:
                core = by_port.get(int(m.group(2)))
                if core is None:
                    raise ConfigError("base %s: not a port of the base" % k)
                if "lane_map" in k:
                    self.lane_map[(core, m.group(1))] = v
                else:
                    self.polarity[(core, m.group(1))] = int(v, 0) & 0xF
                continue
            m = re.match(r"^serdes_(preemphasis|driver_current)(?:_lane(\d))?_(\d+)$", k)
            if m:
                core = by_port.get(int(m.group(3)))
                if core is None:
                    raise ConfigError("base %s: not a port of the base" % k)
                lanes = [int(m.group(2))] if m.group(2) is not None else range(4)
                for i in lanes:
                    # a per-lane key beats the port-wide one (portctrl.c)
                    if m.group(2) is not None or (m.group(1), core * 4 + 1 + i) not in self.lane:
                        self.lane[(m.group(1), core * 4 + 1 + i)] = v
                continue
            raise ConfigError("base %s: per-port key the generator cannot split" % k)
        for k in self.globals:
            m = re.match(r".*_(\d+)$", k)
            if m and int(m.group(1)) in by_port:
                raise ConfigError("base %s: looks per-port; the generator cannot split it" % k)


# ---------------------------------------------------------------- generate
def port_plan(ports):
    """SONiC ports -> [(port, phys, nlanes, logical, gbps, option)], validated."""
    plan, used = [], {}
    for p in ports:
        lanes, where = p["lanes"], "%s (%s)" % (p.get("alias", "?"), p.get("name", "?"))
        n = len(lanes)
        if n not in (1, 2, 4) or lanes != list(range(lanes[0], lanes[0] + n)):
            raise ConfigError("%s: lanes %s: need 1, 2 or 4 consecutive lanes" % (where, lanes))
        phys = lanes[0]
        if not (1 <= phys and phys + n - 1 <= 128) or (phys - 1) // 4 != (phys + n - 2) // 4:
            raise ConfigError("%s: lanes %s: not within one 4-lane core of physical ports 1-128" % (where, lanes))
        off = (phys - 1) % 4
        if (n == 4 and off) or (n == 2 and off not in (0, 2)):
            raise ConfigError("%s: lanes %s: a %d-lane port must start at lane %s of its core" %
                              (where, lanes, n, "0" if n == 4 else "0 or 2"))
        if p["speed"] % 1000:
            raise ConfigError("%s: speed %d Mb/s" % (where, p["speed"]))
        gbps = p["speed"] // 1000
        if (gbps, n) not in VCO:
            raise ConfigError("%s: %dG on %d lane(s) is not a Tomahawk port mode" % (where, gbps, n))
        for l in lanes:
            if l in used:
                raise ConfigError("%s: lane %d also in %s" % (where, l, used[l]))
            used[l] = where
        plan.append([p, phys, n, logical_of(phys), gbps, ""])
    cores = {}
    for e in plan:
        cores.setdefault((e[1] - 1) // 4, []).append(e)
    for core, es in cores.items():
        if not any((e[1] - 1) % 4 == 0 for e in es):
            raise ConfigError("core of lanes %d-%d: lane %d unused while others are (TDM rule)" %
                              (core * 4 + 1, core * 4 + 4, core * 4 + 1))
        vcos = {VCO[(e[4], e[2])] for e in es}
        if len(vcos) > 1:
            raise ConfigError("core of lanes %d-%d: mixes %s (one VCO per core): %s" %
                              (core * 4 + 1, core * 4 + 4, " and ".join(sorted(vcos)),
                               ", ".join("%s %dG" % (e[0].get("alias"), e[4]) for e in es)))
        for e in es:
            if e[4] == 40 and e[2] == 2:
                e[5] = ":2"           # else the SDK takes 4 lanes without a sister port
    return plan


def lane_keys(spec):
    keys = spec.get("laneKeys", ["preemphasis", "driver_current"])
    for k in keys:
        if k not in ("preemphasis", "driver_current"):
            raise ConfigError("laneKeys: %s (preemphasis, driver_current)" % k)
    return keys


def pbmp(ports):
    v = 0
    for p in ports:
        v |= 1 << p
    return "0x%x" % v


def generate(base, spec):
    plan = port_plan(spec["ports"])
    mgmt = spec.get("management", [])
    settings = spec.get("settings", {})
    for k in settings:
        if GENERATED.match(k):
            raise ConfigError("settings.%s: generated from the ports, not settable" % k)
    glob = dict(base.globals)
    for k, v in settings.items():
        if v is None:
            glob.pop(k, None)
        else:
            glob[k] = str(v)
    front = [e[3] for e in plan]
    out = ["# Broadcom SDK config for a Tomahawk, generated by ai_re/nix/packages/openbcm/configs/tomahawk.py"]
    out += textwrap.wrap("from " + spec.get("label", "a reference config"), width=140, initial_indent="# ",
                         subsequent_indent="# ", break_long_words=False, break_on_hyphens=False)
    out += ["# (%d ports, %d management). Do not edit; change the Nix ports instead." % (len(plan), len(mgmt)), ""]
    out += ["%s=%s" % kv for kv in glob.items()]
    out += ["", "# every port oversubscribed and Ethernet (outside pbmp_xport_xe a port is HiGig)",
            "pbmp_oversubscribe=" + pbmp(front),
            "pbmp_xport_xe=" + pbmp(front + [m["logical"] for m in mgmt])]
    if spec.get("dportMap", True):
        out += ["", "# diag-shell port numbers in front-panel order", "dport_map_enable=1", "dport_map_direct=0"]
        out += ["dport_map_port_%d=%d" % (e[3], i + 1) for i, e in enumerate(plan)]
        out += ["dport_map_port_%d=%d" % (m["logical"], len(plan) + i + 1) for i, m in enumerate(mgmt)]
    for p, phys, n, lp, gbps, opt in plan:
        core, off = (phys - 1) // 4, (phys - 1) % 4
        out += ["", "# %s (%s): lanes %s, %dG" % (p.get("alias", ""), p.get("name", ""),
                                                    ",".join(map(str, p["lanes"])), gbps),
                "portmap_%d=%d:%d%s" % (lp, phys, gbps, opt)]
        for d in ("tx", "rx"):
            if (core, d) in base.lane_map:
                out.append("xgxs_%s_lane_map_%d=%s" % (d, lp, base.lane_map[(core, d)]))
        for d in ("tx", "rx"):
            if (core, d) in base.polarity:
                m = base.polarity[(core, d)]
                v = (m >> off) & 1 if n == 1 else m & (((1 << n) - 1) << off)
                out.append("phy_xaui_%s_polarity_flip_%d=0x%x" % (d, lp, v))
        for kind in lane_keys(spec):
            for i in range(n if VCO[(gbps, n)] == BASE_VCO else 0):
                if (kind, phys + i) in base.lane:
                    out.append("serdes_%s_lane%d_%d=%s" % (kind, i, lp, base.lane[(kind, phys + i)]))
    for m in mgmt:
        out += ["", "# management port (TSC-E), not a SONiC port",
                "portmap_%d=%d:%d" % (m["logical"], m["phys"], m["speed"])]
    text = "\n".join(out) + "\n"
    if long_lines(text):
        raise ConfigError("; ".join(long_lines(text)))
    return text


# ---------------------------------------------------------------- check
def sdk_ports(kv):
    """portmap keys -> {logical: (phys, gbps, nlanes)}, SDK 6.5.27 rules."""
    pm, errors = {}, []
    for k, v in kv.items():
        m = re.match(r"^portmap_(\d+)$", k)
        if not m:
            continue
        lp = int(m.group(1))
        if lp in FIXED:
            errors.append("%s: logical port %d is fixed (%s); the SDK ignores it" % (k, lp, FIXED[lp]))
            continue
        f = v.split(":")
        phys, gbps = int(f[0]), int(f[1])
        opt = f[2] if len(f) > 2 else None
        if gbps not in BANDWIDTHS:
            errors.append("%s=%s: bandwidth" % (k, v))
        if lp not in dev_range(phys):
            errors.append("%s=%s: logical %d outside the range of physical %d" % (k, v, lp, phys))
        pm[lp] = [phys, gbps, opt]
    phys_used = {}
    for lp, (phys, gbps, opt) in pm.items():
        if phys in phys_used:
            errors.append("physical %d mapped twice (%d, %d)" % (phys, phys_used[phys], lp))
        phys_used[phys] = lp
    res = {}
    for lp, (phys, gbps, opt) in pm.items():
        if gbps > 53:
            n = 4
        elif gbps in (40, 42):
            sis = phys + 2 if phys % 4 == 1 else phys - 2 if phys % 4 == 3 else None
            n = 2 if opt == "2" or (sis in phys_used and phys <= 127) else 4
        elif gbps >= 20 and gbps not in (25, 27):
            n = 2
        else:
            n = int(opt) if opt in ("2", "4") else 1
        res[lp] = (phys, gbps, n)
    return res, errors


def check(base, spec, text):
    kv = parse(text)
    ports, errors = sdk_ports(kv)
    errors += long_lines(text)
    xe = int(kv.get("pbmp_xport_xe", "0"), 0)
    if not int(kv.get("pbmp_oversubscribe", "0"), 0):
        errors.append("pbmp_oversubscribe empty: the ports would not be oversubscribed")
    lanes_of = {}
    for lp, (phys, gbps, n) in sorted(ports.items()):
        if not (xe >> lp) & 1:
            errors.append("logical %d not in pbmp_xport_xe: it would come up HiGig" % lp)
        if phys <= 128:
            for l in range(phys, phys + n):
                if l in lanes_of:
                    errors.append("lane %d in logical %d and %d" % (l, lanes_of[l], lp))
                lanes_of[l] = lp
    front = {lp: v for lp, v in ports.items() if v[0] <= 128}
    want = sorted((tuple(p["lanes"]), p["speed"]) for p in spec["ports"])
    have = sorted((tuple(range(phys, phys + n)), gbps * 1000) for phys, gbps, n in front.values())
    if want != have:
        errors.append("front-panel ports differ from the spec: missing %s, extra %s" %
                      (sorted(set(want) - set(have)), sorted(set(have) - set(want))))
    want_m = sorted((m["logical"], m["phys"], m["speed"]) for m in spec.get("management", []))
    have_m = sorted((lp, v[0], v[1]) for lp, v in ports.items() if v[0] > 128)
    if want_m != have_m:
        errors.append("management ports %s, spec %s" % (have_m, want_m))
    # SDK-effective per-core and per-lane values vs the base
    cores = sorted({(v[0] - 1) // 4 for v in front.values()})
    p2l = {v[0]: lp for lp, v in front.items()}
    for core in cores:
        first = core * 4 + 1
        if first not in p2l:
            errors.append("core %d-%d: lane %d unused (TDM rule)" % (first, first + 3, first))
            continue
        for d in ("tx", "rx"):
            got = kv.get("xgxs_%s_lane_map_%d" % (d, p2l[first]))
            if got != base.lane_map.get((core, d)):
                errors.append("core %d-%d: %s lane map %s, base %s" % (first, first + 3, d, got, base.lane_map.get((core, d))))
            mask = 0
            for lane in range(4):
                lp = p2l.get(first + lane)
                if lp is None:
                    continue
                v = int(kv.get("phy_xaui_%s_polarity_flip_%d" % (d, lp), "0"), 0)
                mask |= ((v & 1) << lane) if front[lp][2] == 1 else v
                if front[lp][2] >= 4:
                    break
            if mask != base.polarity.get((core, d), 0):
                errors.append("core %d-%d: effective %s polarity 0x%x, base 0x%x" %
                              (first, first + 3, d, mask, base.polarity.get((core, d), 0)))
    for lp, (phys, gbps, n) in front.items():
        for kind in ("preemphasis", "driver_current"):
            for i in range(n):
                got = kv.get("serdes_%s_lane%d_%d" % (kind, i, lp), kv.get("serdes_%s_%d" % (kind, lp)))
                want = (base.lane.get((kind, phys + i))
                        if VCO.get((gbps, n)) == BASE_VCO and kind in lane_keys(spec) else None)
                if got != want:
                    errors.append("lane %d (%dG): %s %s, want %s" % (phys + i, gbps, kind, got, want))
    want_g = dict(base.globals)
    want_g.update(spec.get("settings", {}))
    for k, v in want_g.items():
        if v is None:
            if k in kv:
                errors.append("global %s present, settings remove it" % k)
        elif kv.get(k) != str(v):
            errors.append("global %s=%s, want %s" % (k, kv.get(k), v))
    return errors


def main(argv):
    import argparse
    ap = argparse.ArgumentParser()
    ap.add_argument("mode", choices=["generate", "check", "base"])
    ap.add_argument("--base")
    ap.add_argument("--spec")
    ap.add_argument("--globals")
    ap.add_argument("--cores")
    ap.add_argument("--out")
    ap.add_argument("config", nargs="?")
    a = ap.parse_args(argv)
    try:
        if a.mode == "base":
            with open(a.globals) as f, open(a.cores) as g:
                text = make_base(f.read(), json.load(g))
            with open(a.out, "w") as f:
                f.write(text)
            return 0
        with open(a.base) as f:
            base = Base(f.read())
        with open(a.spec) as f:
            spec = json.load(f)
        if a.mode == "generate":
            text = generate(base, spec)
            with open(a.out, "w") as f:
                f.write(text)
            return 0
        with open(a.config) as f:
            errors = check(base, spec, f.read())
    except ConfigError as e:
        print("config.bcm: %s" % e, file=sys.stderr)
        return 1
    for e in errors:
        print("config.bcm check: %s" % e, file=sys.stderr)
    if not errors:
        print("config.bcm check: %d front-panel ports match the SONiC ports; lane maps, polarity and "
              "pre-emphasis match the base on every lane in use" % len(spec["ports"]))
    return 1 if errors else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
