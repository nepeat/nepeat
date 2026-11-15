#!/usr/bin/env python3
"""Summarise / diff Annapurna al-boot DT objects (the stage2/3 + U-Boot "board config" FDT).

  alpine_dtobj.py <dt.bin|.fimg|.dtb>                board-cfg summary
  alpine_dtobj.py --diff <a> <b>                     summary diff (what makes b differ from a)
  alpine_dtobj.py --raw <a> [<b>]                    every property as path=value (diffable)

Input: a carved DT object (al-boot header c7 9e 0b 00 + FDT), or a bare FDT. Offline only.
Which stage consumes which node: ai_re/alpine-v2-boot-chain.md ("DT object").
Pure stdlib (no dtc needed); `dtc -I dtb` on the payload gives the full source.
"""
import struct, sys, difflib

OBJ_MAGIC = 0x000B9EC7
FDT_MAGIC = 0xD00DFEED


def load(path):
    b = open(path, "rb").read()
    if struct.unpack_from("<I", b, 0)[0] == OBJ_MAGIC:
        size = struct.unpack_from("<Q", b, 0x28)[0]
        b = b[0x48:0x48 + size]
    if struct.unpack_from(">I", b, 0)[0] != FDT_MAGIC:
        sys.exit(f"{path}: no FDT")
    return b


def parse(b):
    """Return {path: {prop: bytes}} plus phandle -> path."""
    _, tot, off_st, off_str = struct.unpack_from(">4I", b, 0)
    nodes, phandles, stack, p = {}, {}, [], off_st
    while True:
        tok = struct.unpack_from(">I", b, p)[0]; p += 4
        if tok == 1:
            e = b.index(b"\0", p); name = b[p:e].decode(); p = (e + 4) & ~3
            stack.append(name)
            nodes["/" + "/".join(x for x in stack if x)] = {}
        elif tok == 2:
            stack.pop()
        elif tok == 3:
            ln, no = struct.unpack_from(">II", b, p); p += 8
            val = b[p:p + ln]; p = (p + ln + 3) & ~3
            e = b.index(b"\0", off_str + no); pn = b[off_str + no:e].decode()
            path = "/" + "/".join(x for x in stack if x)
            nodes[path][pn] = val
            if pn == "phandle":
                phandles[struct.unpack(">I", val)[0]] = path
        elif tok == 9:
            break
    return nodes, phandles


def s(v):
    if v and v[-1] == 0 and v[0] and all(32 <= c < 127 or c == 0 for c in v[:-1]) and b"\0\0" not in v:
        return "/".join(x.decode() for x in v[:-1].split(b"\0"))
    return None


def cells(v): return list(struct.unpack(f">{len(v) // 4}I", v)) if len(v) % 4 == 0 else None


def fmt(v):
    t = s(v)
    if t is not None: return repr(t)
    c = cells(v)
    if c is not None: return "<" + " ".join(f"{x:#x}" for x in c) + ">" if c else "(empty)"
    return v.hex()


def summary(nodes, ph):
    out = []
    g = lambda p, k, d=None: (s(nodes.get(p, {}).get(k, b"")) or d) if k in nodes.get(p, {}) else d
    root = nodes.get("/", {})
    out.append(f"model: {s(root.get('model', b'')) or '?'}")
    bc = "/soc/board-cfg"
    out.append(f"board-cfg id: {g(bc, 'id', '(none)')}")
    pc = nodes.get(bc + "/pinctrl_init", {}).get("pinctrl-0")
    if pc:
        out.append("pinctrl_init: " + ", ".join(ph.get(x, f"#{x:#x}").rsplit("/", 1)[-1] for x in cells(pc)))
    gl = nodes.get(bc + "/gpio_init", {})
    for k in ("gpio-list", "gpio-list-s2"):
        if k in gl:
            c = cells(gl[k])
            ins = [c[i] for i in range(0, len(c), 3) if c[i + 1] == 0]
            outs = [f"{c[i]}={c[i + 2]}" for i in range(0, len(c), 3) if c[i + 1]]
            out.append(f"gpio_init {k} <gpio dir val>: inputs {ins or '-'}; outputs {outs or '-'}")
    for i in range(8):
        p = f"{bc}/serdes/group{i}"
        if p in nodes:
            n = nodes[p]
            lanes = cells(n.get("active-lanes", b"")) or []
            inv = [x for x in ("inv-tx-lanes", "inv-rx-lanes") if x in n]
            out.append(f"serdes group{i}: {g(p, 'interface')} ref {g(p, 'ref-clock')} lanes {lanes} "
                       f"ssc {g(p, 'ssc')} {' '.join(inv)}")
    pp = bc + "/pcie"
    if pp in nodes:
        n = nodes[pp]
        ep = cells(n.get("ep-ports", b"")) if "ep-ports" in n else None
        extra = [f"{k}={fmt(v)}" for k, v in n.items() if k not in ("ep-ports",)]
        out.append(f"pcie: ep-ports {ep if ep else '(none: all RC)'} {' '.join(extra)}")
        for i in range(4):
            q = f"{pp}/port{i}"
            if q in nodes:
                m = nodes[q]
                out.append(f"  port{i}: {g(q, 'status')} gen {cells(m.get('gen', b''))} width {cells(m.get('width', b''))}"
                           + "".join(f" {k}" for k in m if k not in ('status', 'gen', 'width')))
    for i in range(4):
        q = f"{bc}/ethernet/port{i}"
        if q in nodes:
            m = nodes[q]
            sub = [c.rsplit("/", 1)[-1] for c in nodes if c.startswith(q + "/") and c.count("/") == q.count("/") + 1]
            out.append(f"eth port{i}: {g(q, 'status')} mode {g(q, 'mode')} serdes {cells(m.get('serdes-grp', b''))}"
                       f"/{cells(m.get('serdes-lane', b''))} {' '.join(sub)}")
    dp = bc + "/dram"
    if dp in nodes:
        out.append("dram: " + " ".join(f"{k}={fmt(v)}" for k, v in nodes[dp].items()))
        for c in sorted(x for x in nodes if x.startswith(dp + "/") and "impedance_ctrl" not in x):
            out.append(f"  {c[len(dp) + 1:]}: " + " ".join(f"{k}={fmt(v)}" for k, v in nodes[c].items()))
        imp = [x for x in nodes if x.startswith(dp) and x.endswith("single_dimm_single_rank_per_dimm")]
        for x in imp:
            out.append(f"  {x[len(dp) + 1:]}: " + " ".join(f"{k}={fmt(v)}" for k, v in nodes[x].items()))
    else:
        out.append("dram: (no node: stage2 falls back to EEPROM early-init info)")
    for k in ("uart2",):
        if f"{bc}/{k}" in nodes:
            out.append(f"{k}: {g(bc + '/' + k, 'status')}")
    i2c = [p for p in nodes if p.startswith("/soc/i2c-pld/") and "reg" in nodes[p] and "compatible" in nodes[p]]
    out.append("i2c-pld devices: " + ", ".join(f"{p[len('/soc/i2c-pld/'):]}({g(p, 'compatible')})" for p in i2c))
    other = sorted(k for k in nodes if k.startswith(bc + "/") and k.count("/") == bc.count("/") + 1)
    out.append("board-cfg children: " + " ".join(k.rsplit("/", 1)[-1] for k in other))
    return out


def raw(nodes):
    return [f"{p}:{k}={fmt(v)}" for p in sorted(nodes) for k, v in sorted(nodes[p].items())
            if k not in ("phandle", "linux,phandle")]


def main(a):
    if not a:
        sys.exit(__doc__)
    if a[0] == "--diff":
        A, B = (summary(*parse(load(x))) for x in a[1:3])
        sys.stdout.writelines(l + "\n" for l in difflib.unified_diff(A, B, a[1], a[2], n=0, lineterm=""))
    elif a[0] == "--raw":
        R = [raw(parse(load(x))[0]) for x in a[1:]]
        if len(R) == 1:
            print("\n".join(R[0]))
        else:
            sys.stdout.writelines(l + "\n" for l in difflib.unified_diff(R[0], R[1], a[1], a[2], n=0, lineterm=""))
    else:
        print("\n".join(summary(*parse(load(a[0])))))


if __name__ == "__main__":
    main(sys.argv[1:])
