#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Normalise and diff libifcs call traces (trace_rt.c format) of the same SAI
script run on two SAIs (ours and isai), step by step.

  oracle.py diff OURS.txt ISAI.txt [--names-ours CTYPES] [--names-isai CTYPES]
                 [--accessors] [--raw-handles] [-o REPORT]
  oracle.py show TRACE.txt [--names CTYPES]          one trace, normalised
  oracle.py summary TRACE.txt                         per step: calls, IFCS time

Attribute ids are numbers in the traces (the ABIs differ: 0.12.41 vs
0.16.2); with --names-* (a local pyctypes/ifcs_ctypes.py of that version)
they become IFCS_<OBJ>_ATTR_* names, so the two sides compare by name.
Dynamic handles are renamed H<module>.<n> in order of first use (the module
number is from the handle, bits 31..25); sysport, L2VNI and L3VNI handles,
whose values the caller picks, stay as they are. Accessor calls (*_t_*:
key/prefix/attr builders) are dropped unless --accessors.
"""
import argparse, collections, difflib, re, sys

STATIC_MODULES = {31, 18, 19}     # sysport, l2vni, l3vni (value = devport / vid / vrf)
OPS = ["attr_get", "attr_set", "create", "delete", "get_all", "stats_get", "stats_clear", "usage_get", "member_add",
       "member_remove", "bulk_create", "bulk_delete", "bulk_attr_set", "add", "remove", "replace", "get"]


def load_names(path):
    """{OBJ: {id: NAME}} from IFCS_<OBJ>_ATTR_<NAME> = <n> lines of a pyctypes file,
    and {"STATS:" + OBJ: {id: NAME}} from IFCS_<OBJ>_STATS_ID_<NAME> = <n>."""
    if not path:
        return {}
    names = collections.defaultdict(dict)
    rx = re.compile(r"^IFCS_([A-Z0-9_]+?)_(ATTR|STATS_ID)_([A-Z0-9_]+) = (\d+)\s*(#.*)?$")
    for line in open(path, errors="replace"):
        m = rx.match(line)
        if m and not m.group(3).endswith("MAX_COUNT"):
            obj = m.group(1) if m.group(2) == "ATTR" else "STATS:" + m.group(1)
            names[obj].setdefault(int(m.group(4)), m.group(3))
    return names


def obj_of(fn, names):
    base = fn[5:] if fn.startswith("ifcs_") else fn
    for op in OPS:
        if base.endswith("_" + op):
            base = base[: -len(op) - 1]
            break
    parts = base.upper().split("_")
    for n in range(len(parts), 0, -1):
        cand = "_".join(parts[:n])
        if cand in names:
            return cand
    return None


class Norm:
    def __init__(self, names, raw_handles):
        self.names, self.raw = names, raw_handles
        self.map, self.count = {}, collections.Counter()

    def handle(self, v):
        if self.raw or v >> 32 or (v >> 25) == 0:
            return None
        mod = v >> 25
        if mod in STATIC_MODULES:
            return None
        if v not in self.map:
            self.count[mod] += 1
            self.map[v] = "H%d.%d" % (mod, self.count[mod])
        return self.map[v]

    def hex_sub(self, m):
        v = int(m.group(1), 16)
        h = self.handle(v)
        return h if h else m.group(0)

    def attrs(self, s, obj):
        names = self.names.get(obj, {}) if obj else {}
        def one(m):
            i = int(m.group(1))
            return "{" + names.get(i, str(i)) + m.group(2)
        return re.sub(r"\{(\d+)( v=)", one, s)

    def stats(self, s, fn):
        """First [..] list of a *_stats_get/_clear call: counter ids -> names."""
        base = fn[5:]
        for op in ("_stats_get", "_stats_clear"):
            if base.endswith(op):
                base = base[: -len(op)]
        tab = self.names.get("STATS:" + base.upper())
        if not tab:
            return s
        m = re.search(r"\[([0-9,]+)\]", s)
        if not m:
            return s
        ids = ",".join(tab.get(int(x), x) for x in m.group(1).split(","))
        return s[: m.start()] + "[" + ids + "]" + s[m.end():]

    def line(self, fn, args, ret):
        obj = obj_of(fn, self.names)
        a = self.attrs(args, obj)
        if "_stats_" in fn:
            a = self.stats(a, fn)
        a = re.sub(r"0x([0-9a-f]{6,8})\b", self.hex_sub, a)
        a = re.sub(r"(v=)([0-9a-f]{6,8})\b", lambda m: m.group(1) + (self.handle(int(m.group(2), 16)) or m.group(2)), a)
        return "%s(%s) = %s" % (fn, a, ret)


T_RX = re.compile(r"^T (\d+) (\d+) (\d+) (\d+) (\d+) (\w+)\((.*)\)(?: = (\S+))?$")


def steps(path, names, accessors=False, raw=False):
    """[(step name, [(normalised line, dt_us, fn)])] in file order; calls before the first step go to 'prologue'."""
    out, cur, name = [], [], "prologue"
    norm = Norm(names, raw)
    for line in open(path, errors="replace"):
        line = line.rstrip("\n")
        if line.startswith("M step "):
            if cur or name != "prologue": out.append((name, cur))
            name, cur = line[7:], []
        elif line.startswith("M end "):
            out.append((name, cur))
            name, cur = "(between steps)", []
        elif line.startswith("T "):
            m = T_RX.match(line)
            if not m: continue
            fn = m.group(6)
            if not accessors and "_t_" in fn: continue
            cur.append((norm.line(fn, m.group(7), m.group(8) or ""), int(m.group(4)), fn))
    if cur: out.append((name, cur))
    # repeated labels (foreach / repeat bodies) get an occurrence number
    seen, res = collections.Counter(), []
    for n, c in out:
        if not c and n.startswith("("): continue
        k = key(n)
        seen[k] += 1
        res.append((k if seen[k] == 1 else "%s #%d" % (k, seen[k]), c))
    return res


def key(name):
    """Step names match by their label ('0.12 create LLDP'), not by the trailing marker fields."""
    return re.sub(r" status=.*$", "", name)


def cmd_diff(a):
    so = steps(a.ours, load_names(a.names_ours), a.accessors, a.raw_handles)
    si = steps(a.isai, load_names(a.names_isai), a.accessors, a.raw_handles)
    di = collections.OrderedDict((key(n), c) for n, c in si)
    out = []
    w = out.append
    w("# oracle diff: ours=%s isai=%s" % (a.ours, a.isai))
    w("# per SAI step: IFCS calls and IFCS time on each side, then a diff of the normalised calls (- ours, + isai)")
    tot = collections.Counter()
    fn_ours, fn_isai = collections.Counter(), collections.Counter()
    for n, c in so:
        k = key(n)
        ci = di.pop(k, [])
        lo, li = [x[0] for x in c], [x[0] for x in ci]
        to, ti = sum(x[1] for x in c) / 1000.0, sum(x[1] for x in ci) / 1000.0
        tot["ours_calls"] += len(c); tot["isai_calls"] += len(ci); tot["ours_ms"] += to; tot["isai_ms"] += ti
        for x in c: fn_ours[x[2]] += 1
        for x in ci: fn_isai[x[2]] += 1
        w("")
        w("== %s | ours %d calls %.1f ms | isai %d calls %.1f ms" % (k, len(c), to, len(ci), ti))
        if lo == li:
            w("   (same calls)")
            continue
        for d in difflib.unified_diff(lo, li, lineterm="", n=a.context):
            if d.startswith(("---", "+++")): continue
            w("   " + d)
    for k, ci in di.items():
        w("")
        w("== %s | only in the isai trace: %d calls" % (k, len(ci)))
        for x in ci: w("   + " + x[0])
    w("")
    w("# totals: ours %d calls %.1f ms, isai %d calls %.1f ms" % (tot["ours_calls"], tot["ours_ms"], tot["isai_calls"], tot["isai_ms"]))
    w("# functions isai calls that ours never does (count):")
    for f, n in sorted(((f, n) for f, n in fn_isai.items() if f not in fn_ours), key=lambda x: -x[1]):
        w("#   %-50s %d" % (f, n))
    w("# functions only ours calls:")
    for f, n in sorted(((f, n) for f, n in fn_ours.items() if f not in fn_isai), key=lambda x: -x[1]):
        w("#   %-50s %d" % (f, n))
    text = "\n".join(out) + "\n"
    if a.o: open(a.o, "w").write(text)
    else: sys.stdout.write(text)


def cmd_show(a):
    for n, c in steps(a.trace, load_names(a.names), a.accessors, a.raw_handles):
        print("== %s (%d calls, %.1f ms)" % (n, len(c), sum(x[1] for x in c) / 1000.0))
        for x in c: print("   %8.1f us  %s" % (x[1], x[0]))


def cmd_summary(a):
    for n, c in steps(a.trace, {}, True, True):
        print("%-60s %6d calls %10.1f ms" % (n[:60], len(c), sum(x[1] for x in c) / 1000.0))


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sp = ap.add_subparsers(dest="cmd", required=True)
    d = sp.add_parser("diff"); d.add_argument("ours"); d.add_argument("isai")
    d.add_argument("--names-ours"); d.add_argument("--names-isai"); d.add_argument("-o")
    d.add_argument("--context", type=int, default=3)
    s = sp.add_parser("show"); s.add_argument("trace"); s.add_argument("--names")
    m = sp.add_parser("summary"); m.add_argument("trace")
    for p in (d, s):
        p.add_argument("--accessors", action="store_true"); p.add_argument("--raw-handles", action="store_true")
    a = ap.parse_args()
    {"diff": cmd_diff, "show": cmd_show, "summary": cmd_summary}[a.cmd](a)


if __name__ == "__main__":
    main()
