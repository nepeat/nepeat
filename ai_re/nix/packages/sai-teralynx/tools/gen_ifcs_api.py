#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Generate src/ifcs_api*.h (the IFCS declarations the backend uses) at build time.

  gen_ifcs_api.py --ctypes <ifcs_ctypes.py> [--overrides ifcs_overrides.txt] \
                  [--source "<what the ctypes file is>"] TEMPLATE.h.in -o OUT.h

TEMPLATE (ours, in git) is C with names only; the IFCS facts are filled in
from the ctypes file (ctypesgen output of the SDK headers, read as data by
ifcs_ctypes_model.py, never executed):

  NAME = @                 enum member / constant: NAME's value
  @{NAME} / @{NAME:#x}     NAME's value (optional Python format spec)
  @typedef A B ...         typedef of a scalar / array / function pointer
  @struct A B ...          typedef struct { all fields } A; (anonymous, as the
                           tag is not part of our ABI); trailing 1-bit
                           _dirty_* bitfields become one "uint8_t dirty"
  @union A m1 m2 ...       typedef union with only members m1 m2 ... (none
                           listed: all), padded to the union's full size
  @alias A = B             typedef B A; A's own field list must equal B's
  @fn f g ...              IFCS_FN(ret, f, (args)) per function
  @prefix A f1 f2 ...      typedef struct with only the leading fields f1 f2 ...
                           (+ the dirty byte): the target's struct is shorter
                           than the public one (an override; say where verified)
  @bits A ...              A (a struct of one-bit fields of one integer type)
                           is written as that integer type where used
  @as T A ...              the 4-byte enum types A ... are written as T (a
                           same-size integer type) where used

A directive line may end in a /* comment */ (kept on the last line it
produces). Every other line is copied as is. Types in the output use our
selected typedef names where the ctypes file names one, else the base C
type (enums: int; String: const char*). A pointer to a CFUNCTYPE typedef
is that function pointer (how ctypesgen writes a function-typed parameter).

The overrides file (ours) holds NAME VALUE lines for constants whose value
in the public ctypes differs from the target library's; each needs a
comment saying where it was verified. Any name the template uses that the
ctypes file lacks is an error, as is a field type with no selected typedef.
"""
import argparse, ast, ctypes, hashlib, os, re, sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from ifcs_ctypes_model import Model, PRIM, T  # noqa: E402

DIRECTIVE = re.compile(r"^@(typedef|struct|union|alias|fn|bits|prefix|as)\s+(.*?)\s*(/\*.*)?$")


class Gen:
    def __init__(self, model, overrides, selected):
        self.m, self.ov, self.sel = model, overrides, selected
        self.used_ov = set()
        self.errors = []
        self.bits = set()
        self.as_types = {}
        self.declared = set()      # typedef names emitted so far (C: declare before use)
        self.in_comment = False

    def err(self, msg):
        self.errors.append(msg)
        return "/* ERROR */"

    # ------------------------------------------------------------ values
    def value(self, name):
        if name in self.ov:
            self.used_ov.add(name)
            return self.ov[name]
        v = self.m.value(name)
        if v is None:
            self.err("no integer value for %s in %s" % (name, self.m.path))
            return 0
        return v

    # ------------------------------------------------------------ types
    def cname(self, t, decl=""):
        """C spelling of a type expression around declarator decl."""
        sp = (" " + decl) if decl else ""
        if t.kind == "void": return "void" + sp
        if t.kind == "prim": return PRIM[t.a][0] + sp
        if t.kind == "name":
            n = t.a
            if n in self.bits: return self.bits_storage(n) + sp
            if n in self.as_types: return self.as_types[n] + sp
            if n in self.sel and n in self.declared: return n + sp   # our typedef, if already declared above
            if n in self.m.aggs:
                for s in sorted(self.sel & self.declared):   # struct_x -> our typedef for it
                    if self.m.agg_of(s) == n: return s + sp
                return self.err("%s: no selected typedef for %s" % (decl or "?", n)) + sp
            e = self.m.assign.get(n)
            if e is None: return self.err("type %s not in %s" % (n, self.m.path)) + sp
            if isinstance(e, ast.Name) and e.id.startswith("enum_"): return "int" + sp
            return self.cname(self.m.typ(e), decl)
        if t.kind == "ptr":
            tgt = self.peel(t.a)
            if tgt.kind == "fn" or (tgt.kind == "name" and self.is_fn(tgt.a)):
                return self.cname(t.a, decl)               # pointer to a CFUNCTYPE = the function pointer
            s = self.cname(t.a, "*" + decl if decl else "*")
            return re.sub(r" (\*+)(?=\w|$)", r"\1 ", s).rstrip()   # "T *x" -> "T* x"
        if t.kind == "arr":
            return self.cname(t.a, "%s[%d]" % (decl, t.b))
        if t.kind == "fn":
            args = ", ".join(self.cname(a) for a in t.b) or "void"
            return "%s (*%s)(%s)" % (self.cname(t.a), decl, args)
        return self.err("type %r" % (t,))

    def peel(self, t):
        """One alias step at a time until a selected name or a non-name."""
        while t.kind == "name" and t.a not in self.sel and t.a not in self.m.aggs:
            e = self.m.assign.get(t.a)
            if e is None or (isinstance(e, ast.Name) and e.id.startswith("enum_")): break
            t = self.m.typ(e)
        return t

    def is_fn(self, n):
        e = self.m.assign.get(n)
        return e is not None and self.m.resolve(self.m.typ(e)).kind == "fn"

    # ------------------------------------------------------------ declarations
    def fields(self, agg):
        if agg not in self.m.fields: return [self.err("%s has no field list" % agg)]
        out, fl = [], self.m.fields[agg]
        dirty = [f for f in fl if f[0].startswith("_dirty_")]
        if dirty:
            if fl[-len(dirty):] != dirty or any(b != 1 for _, _, b in dirty) or len(dirty) > 8:
                return [self.err("%s: dirty bits not a trailing run of <= 8 one-bit fields" % agg)]
            fl = fl[:-len(dirty)]
        for n, e, bits in fl:
            if bits: return [self.err("%s.%s: bitfield" % (agg, n))]
            out.append(self.cname(self.m.typ(e), n) + ";")
        if dirty: out.append("uint8_t dirty;")
        return out

    def agg(self, name, kind):
        a = self.m.agg_of(name)
        if a is None: return [self.err("%s: not a struct/union typedef in %s" % (name, self.m.path))]
        if self.m.aggs[a] != kind: return [self.err("%s is a %s, not a %s" % (name, self.m.aggs[a], kind))]
        return a

    def d_struct(self, names):
        out = []
        for n in names:
            a = self.agg(n, "Structure")
            if isinstance(a, list): out += a; continue
            out.append(self.wrap("typedef struct {", self.fields(a), "} %s;" % n))
        return out

    def d_prefix(self, args):
        n, keep = args[0], args[1:]
        a = self.agg(n, "Structure")
        if isinstance(a, list): return a
        fl = self.m.fields.get(a, [])
        names = [f for f, _, _ in fl if not f.startswith("_dirty_")]
        if names[:len(keep)] != keep or any(b for f, _, b in fl if f in keep):
            return [self.err("@prefix %s: %s are not its leading fields (%s)" % (n, " ".join(keep), " ".join(names)))]
        body = [self.cname(self.m.typ(e), f) + ";" for f, e, _ in fl if f in keep]
        if len(names) < len(fl): body.append("uint8_t dirty;")
        return [self.wrap("typedef struct {", body, "} %s;" % n)]

    def d_union(self, args):
        n, members = args[0], args[1:]
        a = self.agg(n, "Union")
        if isinstance(a, list): return a
        fl = {f[0]: f for f in self.m.fields.get(a, [])}
        if not members: members = list(fl)
        body = []
        for mname in members:
            if mname not in fl: body.append(self.err("%s has no member %s" % (n, mname))); continue
            body.append(self.cname(self.m.typ(fl[mname][1]), mname) + ";")
        size = ctypes.sizeof(self.m.agg_ctype(a))
        if len(members) < len(fl): body.append("uint8_t _size[%d];" % size)
        return [self.wrap("typedef union {", body, "} %s;" % n)]

    def d_alias(self, args):
        if len(args) != 3 or args[1] != "=": return [self.err("@alias A = B")]
        a, b = self.m.agg_of(args[0]), self.m.agg_of(args[2])
        if a is None or b is None or [(f, ast.dump(e), x) for f, e, x in self.m.fields.get(a, [])] != \
                [(f, ast.dump(e), x) for f, e, x in self.m.fields.get(b, [])]:
            return [self.err("@alias %s = %s: field lists differ" % (args[0], args[2]))]
        return ["typedef %s %s;" % (args[2], args[0])]

    def d_typedef(self, names):
        out = []
        for n in names:
            e = self.m.assign.get(n)
            if e is None: out.append(self.err("typedef %s not in %s" % (n, self.m.path))); continue
            t = self.m.typ(e)
            if t.kind == "name" and t.a.startswith("enum_"):
                out.append("typedef int %s;" % n); continue
            saved = self.sel
            self.sel = self.sel - {n}
            out.append("typedef %s;" % self.cname(t, n))
            self.sel = saved
        return out

    def bits_storage(self, n):
        a = self.m.agg_of(n)
        fl = self.m.fields.get(a, []) if a else []
        kinds = {self.cname(self.m.typ(e)) for _, e, b in fl if b}
        if not fl or any(b is None for _, _, b in fl) or len(kinds) != 1:
            return self.err("@bits %s: not a struct of bitfields of one type" % n)
        st = kinds.pop()
        if ctypes.sizeof(self.m.agg_ctype(a)) != ctypes.sizeof(self.m.ctype(self.m.typ(fl[0][1]))):
            return self.err("@bits %s: wider than its storage type %s" % (n, st))
        return st

    def d_as(self, args):
        t, names = args[0], args[1:]
        if t not in ("uint32_t", "int32_t", "int", "unsigned int"): return [self.err("@as %s: not a 4-byte integer type" % t)]
        for n in names:
            e = self.m.assign.get(n)
            if not (isinstance(e, ast.Name) and e.id.startswith("enum_")) or ctypes.sizeof(self.m.ctype(self.m.typ(e))) != 4:
                self.err("@as %s: %s is not a 4-byte enum type" % (t, n)); continue
            self.as_types[n] = t
        return []

    def d_bits(self, names):
        self.bits.update(names)
        return []

    def d_fn(self, names):
        out = []
        for n in names:
            if n not in self.m.args: out.append(self.err("function %s not in %s" % (n, self.m.path))); continue
            ret = self.cname(self.m.typ(self.m.ret[n])) if n in self.m.ret else "void"
            args = ", ".join(self.cname(self.m.typ(a)) for a in self.m.args[n])
            out.append("IFCS_FN(%s, %s, (%s))" % (ret, n, args))
        return out

    @staticmethod
    def wrap(head, body, tail):
        one = " ".join([head] + body + [tail])
        if len(one) <= 120: return one
        return "\n".join([head] + ["    " + b for b in body] + [tail])

    # ------------------------------------------------------------ template
    def line(self, line):
        m = DIRECTIVE.match(line)
        if m:
            kind, args, comment = m.group(1), m.group(2).split(), m.group(3)
            out = getattr(self, "d_" + kind)(args)
            self.declared.update(args[:1] if kind in ("union", "prefix", "alias") else args if kind in ("typedef", "struct") else [])
            if comment:
                self.in_comment = "*/" not in comment
                if out: out[-1] += "   " + comment
                else: out = [comment]
            return "\n".join(out)
        parts, self.in_comment = split_comments(line, self.in_comment)
        return "".join(self.fill(seg) if code else seg for code, seg in parts)

    def fill(self, code):
        code = re.sub(r"\b([A-Za-z_]\w*)(\s*=\s*)@(?![{\w])", lambda x: "%s%s%d" % (x.group(1), x.group(2), self.value(x.group(1))), code)
        code = re.sub(r"(#define\s+(\w+)\s+)@(?![{\w])", lambda x: "%s%d" % (x.group(1), self.value(x.group(2))), code)
        code = re.sub(r"@\{(\w+)(?::([^}]*))?\}", lambda x: format(self.value(x.group(1)), x.group(2) or "d"), code)
        if "@" in code: self.err("stray @ in: %s" % code.strip())
        return code


def split_comments(line, in_comment):
    """([(is_code, text)], still in a comment at the end): the parts outside
    /* */ comments are filled in, comments are not (they may span lines)."""
    out, i = [], 0
    while i < len(line):
        if in_comment:
            e = line.find("*/", i)
            if e < 0:
                out.append((False, line[i:])); break
            out.append((False, line[i:e + 2]))
            i, in_comment = e + 2, False
            continue
        s = line.find("/*", i)
        if s < 0:
            out.append((True, line[i:])); break
        out.append((True, line[i:s]))
        i, in_comment = s, True
    return out, in_comment


def selected_names(templates):
    """Every name declared by a directive (the typedefs the output defines)."""
    sel = set()
    for txt in templates:
        for line in txt.splitlines():
            m = DIRECTIVE.match(line)
            if not m: continue
            a = m.group(2).split()
            if m.group(1) in ("typedef", "struct"): sel.update(a)
            elif m.group(1) in ("union", "prefix"): sel.add(a[0])
            elif m.group(1) == "alias": sel.add(a[0])
    return sel


def read_overrides(path):
    ov = {}
    if not path: return ov
    for line in open(path):
        code = line.split("#", 1)[0].strip()
        if not code: continue
        name, val = code.split()
        if "#" not in line: sys.exit("%s: override %s has no comment saying where it was verified" % (path, name))
        ov[name] = int(val, 0)
    return ov


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--ctypes", required=True)
    ap.add_argument("--overrides")
    ap.add_argument("--source", help="what the ctypes file is (provenance line of the output)")
    ap.add_argument("--keep-going", action="store_true", help="write the outputs despite errors (comparisons)")
    ap.add_argument("-o", "--outdir", required=True, help="OUTDIR/<template minus .in> per template")
    ap.add_argument("templates", nargs="+")
    a = ap.parse_args()
    tmpls = [(p, open(p).read()) for p in a.templates]
    g = Gen(Model(a.ctypes), read_overrides(a.overrides), selected_names([t for _, t in tmpls]))
    sha = hashlib.sha256(open(a.ctypes, "rb").read()).hexdigest()
    outs = []
    for p, t in tmpls:
        name = os.path.basename(p)[:-3] if p.endswith(".in") else os.path.basename(p)
        head = ("/* GENERATED by tools/gen_ifcs_api.py from %s + %s (sha256 %s)%s.\n"
                " * Do not edit or commit: it holds IFCS facts from vendor material. */\n"
                % (os.path.basename(p), os.path.basename(a.ctypes), sha, (", " + a.source) if a.source else ""))
        outs.append((os.path.join(a.outdir, name), head + "\n".join(g.line(l) for l in t.split("\n"))))
    unused = set(g.ov) - g.used_ov
    if unused: g.err("overrides not used: %s" % " ".join(sorted(unused)))
    if g.errors and not a.keep_going:
        sys.exit("gen_ifcs_api.py:\n  " + "\n  ".join(g.errors))
    os.makedirs(a.outdir, exist_ok=True)
    for path, text in outs:
        with open(path + ".tmp", "w") as f:
            f.write(text)
        os.replace(path + ".tmp", path)
    if g.errors:
        sys.exit("gen_ifcs_api.py (outputs written, --keep-going):\n  " + "\n  ".join(g.errors))


if __name__ == "__main__":
    main()
