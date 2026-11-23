#!/usr/bin/env python3
"""Every bindgen-generated kernel struct: rustc's layout == GCC's.

The Rust abstractions use the types bindgen derived from the kernel headers
with libclang. If libclang lays a struct out differently from the GCC that
built the kernel (endianness, a GCC-only attribute, an alignment rule) or
rustc lays the bindgen output out differently from libclang, Rust code reads
the wrong bytes. This check closes the loop rustc == GCC directly:

  1. parse rust/bindings_generated.rs (kept in the kernel's dev output by
     ../default.nix keepBindings): every named struct/union with its named
     fields (anonymous members, bit-field storage and padding are skipped;
     their neighbours' offsets still pin them);
  2. emit one C table (sizeof, alignof, offsetof) and compile it with the
     kernel's own flags (kbuild, M=); entries GCC rejects (a name that is a
     typedef and not a tag, a field bindgen renamed) are retried as a typedef
     and otherwise dropped and counted;
  3. emit the same table in Rust (size_of, align_of, offset_of! on
     kernel::bindings::*) and compile it with the kernel's rustc flags;
  4. extract both tables from the objects and compare them entry by entry.

usage: layout_check.py <bindings_generated.rs> <kernel build dir> <full kernel source> <work dir> <out dir> [make args...]
(the full source gives drivers/base/base.h, which bindings_helper.h includes and
the dev output does not keep)
"""
import json
import re
import subprocess
import sys
from pathlib import Path

# Known mismatches: (struct, reason). The check fails on any other mismatch,
# and also when a known one disappears (so the entry gets removed).
KNOWN = {
    "mce_error_info": "bindgen 0.72.1 lays out the bit-fields inside its anonymous union "
                      "one after another ([u8; 7] storage) instead of overlapping, so the union is 8 "
                      "bytes, not 4; libclang and GCC agree on 4. bindgen's own layout test would "
                      "reject it, but the kernel generates bindings with --no-layout-tests. "
                      "Architecture-independent (also x86_64 and ppc64le); no Rust code uses it.",
}

RUST_KW = set("as async await box break const continue crate dyn else enum extern false fn for gen if impl in let loop match mod move mut priv pub ref return self Self static struct super trait true try type typeof unsafe unsized use virtual where while yield abstract become do final macro override".split())
SKIP_FIELD = re.compile(r"^(__bindgen_anon_\d+|_bitfield_\d+|_bitfield_align_\d+|__bindgen_padding_\d+|_bindgen_opaque_blob|_bindgen_union_align)$")


def parse(text):
    out = []
    for m in re.finditer(r"pub (struct|union) (\w+) \{([^{}]*)\}", text):
        kind, name, body = m.groups()
        if "__bindgen_ty_" in name or name.startswith("__Bindgen") or name.startswith("__Incomplete"):
            continue
        fields = re.findall(r"(?:^ ?|, )pub (\w+) :", body)
        if fields == ["_address"] or not fields:
            continue   # forward declaration: incomplete in C
        out.append((kind, name, [f for f in fields if not SKIP_FIELD.match(f)]))
    return out


def cfield(f):
    return f[:-1] if f.endswith("_") and f[:-1] in RUST_KW else f


def entries(structs, cnames):
    """(struct index, field or None) -> one table slot each (size+align for None)."""
    e = []
    for si, (kind, name, fields) in enumerate(structs):
        if si not in cnames:
            continue
        e.append((si, None))
        for f in fields:
            e.append((si, f))
    return e


def c_table(structs, cnames, drop, helper):
    lines = [f'#include "{helper}"',
             "__attribute__((section(\".layout_c\"), used)) static const u64 layout_c[] = {"]
    where = {}
    for si, f in entries(structs, cnames):
        if (si, f) in drop:
            continue
        t = cnames[si]
        where[len(lines) + 1] = (si, f)
        lines.append(f"\t(u64)sizeof({t}), (u64)__alignof__({t}),"
                     if f is None else f"\t(u64)__builtin_offsetof({t}, {cfield(f)}),")
    lines.append("};")
    return "\n".join(lines) + "\n", where


def make(kdir, work, args, target):
    return subprocess.run(["make", "-C", kdir, f"M={work}"] + args + [target],
                          capture_output=True, text=True)


def main():
    bindings, kdir, ksrc, work, out = sys.argv[1:6]
    args = sys.argv[6:]
    work, out = Path(work), Path(out)
    # bindings_helper.h: #include <../../drivers/base/base.h>; -I inc/a/b resolves it
    (work / "inc/a/b").mkdir(parents=True, exist_ok=True)
    (work / "inc/drivers/base").mkdir(parents=True, exist_ok=True)
    (work / "inc/drivers/base/base.h").write_bytes((Path(ksrc) / "drivers/base/base.h").read_bytes())
    structs = parse(Path(bindings).read_text())
    helper = str(Path(kdir) / "source/rust/bindings/bindings_helper.h")
    cnames = {si: f"{kind} {name}" for si, (kind, name, _) in enumerate(structs)}
    drop, typedef_tried, dropped = set(), set(), []
    for rnd in range(6):
        src, where = c_table(structs, cnames, drop, helper)
        (work / "layout_c.c").write_text(src)
        r = make(kdir, work, args, "layout_c.o")
        if r.returncode == 0:
            break
        bad = set()
        for m in re.finditer(r"layout_c\.c:(\d+):\d+: error", r.stdout + r.stderr):
            ln = int(m.group(1))
            if ln in where:
                bad.add(where[ln])
        if not bad:
            sys.stderr.write(r.stdout[-4000:] + r.stderr[-4000:])
            raise SystemExit("layout_check: C table does not compile and no entry is to blame")
        renamed = {si for si, f in bad if f is None}
        for si, f in sorted(bad, key=lambda x: (x[0], x[1] or "")):
            if f is not None and si in renamed:
                continue        # its struct name failed: retry the fields with the new name
            if f is None and si not in typedef_tried:
                typedef_tried.add(si)
                cnames[si] = structs[si][1]     # retry as a typedef name
            elif f is None:
                dropped.append(structs[si][1])
                del cnames[si]
            else:
                drop.add((si, f))
    else:
        raise SystemExit("layout_check: C table still does not compile after 6 rounds")
    kept = [(si, f) for si, f in entries(structs, cnames) if (si, f) not in drop]
    rs = ["// generated by layout_check.py", "#![allow(missing_docs, non_upper_case_globals)]",
          "use kernel::bindings as b;", "#[unsafe(link_section = \".layout_rs\")]", "#[used]",
          f"static LAYOUT_RS: [u64; {sum(2 if f is None else 1 for _, f in kept)}] = ["]
    for si, f in kept:
        n = structs[si][1]
        if f is None:
            rs.append(f"    core::mem::size_of::<b::{n}>() as u64, core::mem::align_of::<b::{n}>() as u64,")
        else:
            rs.append(f"    core::mem::offset_of!(b::{n}, {f}) as u64,")
    rs.append("];")
    (work / "layout_rs.rs").write_text("\n".join(rs) + "\n")
    r = make(kdir, work, args, "modules")
    if r.returncode != 0:
        sys.stderr.write(r.stdout[-6000:] + r.stderr[-6000:])
        raise SystemExit("layout_check: module build failed")
    objcopy = next(a.split("=", 1)[1] for a in args if a.startswith("OBJCOPY="))
    tabs = {}
    for o, sec in (("layout_c.o", ".layout_c"), ("layout_rs.o", ".layout_rs")):
        subprocess.run([objcopy, "-O", "binary", f"--only-section={sec}", str(work / o), str(work / (o + ".bin"))], check=True)
        raw = (work / (o + ".bin")).read_bytes()
        tabs[o] = [int.from_bytes(raw[i:i + 8], "big") for i in range(0, len(raw), 8)]
    c, r_ = tabs["layout_c.o"], tabs["layout_rs.o"]
    labels = []
    for si, f in kept:
        n = structs[si][1]
        labels += [f"sizeof({n})", f"alignof({n})"] if f is None else [f"offsetof({n}, {f})"]
    allm = [(i, labels[i]) for i in range(min(len(c), len(r_))) if c[i] != r_[i]]
    struct_of = lambda lab: lab.split("(", 1)[1].split(",")[0].rstrip(")")
    known_hit = sorted({struct_of(lab) for _, lab in allm if struct_of(lab) in KNOWN})
    mism = [f"{lab}: gcc {c[i]} rustc {r_[i]}" for i, lab in allm if struct_of(lab) not in KNOWN]
    known_seen = [f"{lab}: gcc {c[i]} rustc {r_[i]}" for i, lab in allm if struct_of(lab) in KNOWN]
    for k in KNOWN:
        if k not in known_hit and any(structs[si][1] == k for si, _ in kept):
            mism.append(f"known mismatch of {k} is gone: remove it from KNOWN")
    if len(c) != len(r_) or len(c) != len(labels):
        mism.append(f"table lengths differ: gcc {len(c)} rustc {len(r_)} labels {len(labels)}")
    nstruct = len({si for si, _ in kept})
    report = {"structs_in_bindings": len(structs), "structs_checked": nstruct,
              "fields_checked": sum(1 for _, f in kept if f is not None),
              "values_compared": len(labels), "structs_dropped": sorted(dropped),
              "fields_dropped": sorted(f"{structs[si][1]}.{f}" for si, f in drop),
              "mismatches": mism, "known_mismatches": known_seen,
              "known_reasons": {k: KNOWN[k] for k in known_hit}}
    out.mkdir(parents=True, exist_ok=True)
    (out / "layout-report.json").write_text(json.dumps(report, indent=1) + "\n")
    print(f"layout: {nstruct} structs, {report['fields_checked']} fields, {len(labels)} values: "
          f"{len(mism)} mismatches; dropped {len(dropped)} structs, {len(drop)} fields (GCC cannot name them)")
    for m in known_seen:
        print("KNOWN", m)
    for m in mism[:50]:
        print("MISMATCH", m)
    if mism:
        raise SystemExit(1)


main()
