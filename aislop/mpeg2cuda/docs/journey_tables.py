#!/usr/bin/env python3
"""Fill the generated tables in docs/JOURNEY.md from docs/data/*.csv.
Replaces the text between <!--BEGIN name--> and <!--END name--> markers."""
import csv, json, os, re

H = os.path.dirname(__file__)
D = os.path.join(H, "data")
rows = list(csv.DictReader(open(os.path.join(D, "fps.csv"))))
cpu = list(csv.DictReader(open(os.path.join(D, "cpu.csv"))))
kern = list(csv.DictReader(open(os.path.join(D, "kernels_pj.csv"))))
MS = {
    "dea7407": "1. Phase 1: I/P, frame DCT",
    "f554c30": "2. Field DCT + field ME, vectorised ME, row transform, VBV RC",
    "525ec8a": "3. Temporal ME, adaptive f_code, damped RC",
    "9819724": "4. Codex review fixes (OOB reads, MV cost wrap)",
    "39fe7db": "5. B pictures",
    "6fe6f2c": "6. Fused VLC, bandwidth, constant tables, 32-cand refine",
    "36c2f15": "7. Drop field_skip",
    "0aa2090": "8. Source field order, progressive_frame, first-I calibration",
    "e14023c": "9. CBR carry-over clamp, proportional VBV re-code",
}


def g(c, clip, bf, m):
    v = [float(r["fps"]) for r in rows if r["commit"] == c and r["clip"] == clip
         and r["bf"] == str(bf) and r["instances"] == str(m)]
    return f"{v[0]:.0f}" if v and v[0] > 0 else "–"


def kernel_total(c):
    rs = [r for r in kern if r["commit"] == c and r["kernel"] != "k_init_tables"]
    if not rs:
        return "–"
    n = max(int(r["calls"]) for r in rs if r["kernel"] == "k_encode_mb")
    return f"{sum(float(r['us_per_call']) * int(r['calls']) for r in rs) / n:.0f}"


commits = list(dict.fromkeys(r["commit"] for r in rows))
t = ["| milestone | commit | park_joy 1080i I/P: 1 / 4 inst | park_joy B2: 1 / 4 | touchdown I/P: 1 / 4 | touchdown B2: 1 / 4 | kernel µs/pic (pj, I/P) |",
     "|---|---|---|---|---|---|---|"]
for c in commits:
    t.append(f"| {MS.get(c, c)} | `{c}` | {g(c,'parkjoy_1080i',0,1)} / {g(c,'parkjoy_1080i',0,4)} | "
             f"{g(c,'parkjoy_1080i',2,1)} / {g(c,'parkjoy_1080i',2,4)} | "
             f"{g(c,'touchdown_1080p',0,1)} / {g(c,'touchdown_1080p',0,4)} | "
             f"{g(c,'touchdown_1080p',2,1)} / {g(c,'touchdown_1080p',2,4)} | {kernel_total(c)} |")
fps_table = "\n".join(t)

t = ["| clip | rate control | B pictures | 1 stream (threads=auto) | 8 processes x 4 threads |", "|---|---|---|---|---|"]
for clip in ("parkjoy_1080i", "touchdown_1080p"):
    for mode in ("CBR", "ABR"):
        for bf in ("0", "2"):
            one = [r for r in cpu if r["clip"] == clip and r["mode"] == mode and r["bf"] == bf and r["instances"] == "1"]
            agg = [r for r in cpu if r["clip"] == clip and r["mode"] == mode and r["bf"] == bf and r["instances"] == "8"]
            if one:
                t.append(f"| {clip} | {mode} | {bf} | {float(one[0]['aggregate_fps']):.0f} | {float(agg[0]['aggregate_fps']):.0f} |")
cpu_table = "\n".join(t)

kn = {}
for r in kern:
    if r["kernel"] != "k_init_tables":
        kn.setdefault(r["kernel"], {})[r["commit"]] = float(r["us_per_call"])
names = sorted(kn, key=lambda k: -max(kn[k].values()))
t = ["| kernel (µs per call, park_joy I/P) | " + " | ".join(f"`{c}`" for c in commits) + " |",
     "|---|" + "---|" * len(commits)]
for k in names:
    t.append(f"| {k} | " + " | ".join(f"{kn[k][c]:.0f}" if c in kn[k] else "–" for c in commits) + " |")
kernel_table = "\n".join(t)

p = os.path.join(H, "JOURNEY.md")
s = open(p).read()
for name, val in (("FPS", fps_table), ("CPU", cpu_table), ("KERNELS", kernel_table)):
    s = re.sub(rf"(<!--BEGIN {name}-->)(.*?)(<!--END {name}-->)", lambda m: f"{m.group(1)}\n{val}\n{m.group(3)}", s, flags=re.S)
open(p, "w").write(s)
print("tables updated")
