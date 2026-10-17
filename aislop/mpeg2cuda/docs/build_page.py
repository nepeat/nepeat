#!/usr/bin/env python3
"""Build docs/journey.html (the published page) from docs/data/*."""
import csv, json, os, re

H = os.path.dirname(__file__)
D = os.path.join(H, "data")
fps = list(csv.DictReader(open(os.path.join(D, "fps.csv"))))
cpu = list(csv.DictReader(open(os.path.join(D, "cpu.csv"))))
kern = list(csv.DictReader(open(os.path.join(D, "kernels_pj.csv"))))
rec = json.load(open(os.path.join(D, "recorded.json")))
journey = open(os.path.join(H, "JOURNEY.md")).read()

MS = [
    ("dea7407", "Phase 1", "I/P, frame DCT, half-pel ME, GPU VLC and slice packing; bit-exact on the first run"),
    ("f554c30", "Field tools", "field DCT + field motion, packed-byte SAD, lane-per-candidate refine, row-based transform, VBV-safe RC"),
    ("525ec8a", "Temporal ME", "previous-picture predictors, adaptive f_code, tuned dead zone, damped rate control"),
    ("9819724", "Review fixes", "Codex review: out-of-bounds half-pel reads, padding, MV cost wrap"),
    ("39fe7db", "B pictures", "reorder buffer, open GOPs, fwd/bwd/bi with frame and field vectors"),
    ("6fe6f2c", "Bandwidth", "fused coefficient VLC, vectorised load, constant-memory tables, one-round refine"),
    ("36c2f15", "Cleanup", "removed a no-op option"),
    ("0aa2090", "Progressive frames", "source field order, progressive_frame coding, first-I calibration"),
    ("e14023c", "RC carry clamp", "CBR carry-over clamped to the VBV, proportional VBV re-code"),
    ("9173f3e", 'Field weave', 'progressive 2x-rate input woven into interlaced pictures on the GPU (feature; speed unaffected)'),
    ("251534c", 'Pipelining', "the next picture's search is queued before the previous one is finished; read-backs on a second stream; GPU idle 148 -> 64 µs/pic"),
    ("143f7f5", 'Kernel trims', 'coarse search shares loads across 4 candidates, encode_mb shared arrays padded against bank conflicts, refine and field search read shared-memory windows'),
    ("ac0a6b1", 'SIMD + latency', 'inline-PTX vabsdiff4 accumulate, reference reads kept as LDG, refine and field half-pel steps from the shared windows'),
    ("ecbf52c", 'Decimation', 'references decimated once instead of up to 5x, source decimation in k_load, reference decimation in k_encode_mb; k_down4 gone'),
    ("3811bb7", 'Zero-copy + tail stream', 'coded pictures land in mapped pinned host memory (no read-back copies); VLC, pack and concat run on a second stream beside the next search'),
    ("e8612c6", 'Load stream', 'uploads and k_load on a third stream with per-slot events, overlapping the coding of earlier pictures'),
]


def g(c, clip, bf, m):
    v = [float(r["fps"]) for r in fps if r["commit"] == c and r["clip"] == clip
         and r["bf"] == str(bf) and r["instances"] == str(m)]
    return round(v[0]) if v and v[0] > 0 else None


def cpuv(clip, mode, bf, inst):
    for r in cpu:
        if r["clip"] == clip and r["mode"] == mode and r["bf"] == str(bf) and r["instances"] == str(inst):
            return round(float(r["aggregate_fps"]))


series = {}
for clip in ("parkjoy_1080i", "touchdown_1080p"):
    for bf in (0, 2):
        for m in (1, 4):
            series[f"{clip}|{bf}|{m}"] = [g(c, clip, bf, m) for c, _, _ in MS]
cpuref = {f"{clip}|{bf}|{inst}": cpuv(clip, "CBR", bf, inst)
          for clip in ("parkjoy_1080i", "touchdown_1080p") for bf in (0, 2) for inst in (1, 8)}

top = ["k_me_refine", "k_encode_mb", "k_me_field", "k_vlc", "k_me_coarse", "k_load"]
kstack = {}
for k in top + ["other"]:
    vals = []
    for c, _, _ in MS:
        rows = [r for r in kern if r["commit"] == c and r["kernel"] != "k_init_tables"]
        n = max(int(r["calls"]) for r in rows if r["kernel"] == "k_encode_mb")
        sel = [r for r in rows if (r["kernel"] not in top) if k == "other"] if k == "other" else [r for r in rows if r["kernel"] == k]
        vals.append(round(sum(float(r["us_per_call"]) * int(r["calls"]) for r in sel) / n, 1))
    kstack[k.replace("k_", "")] = vals

dev = [s for s in rec["gpu_dev_steps"] if "kernel_us" in s and "idle" not in s["step"]]
bd = rec["bd_rate_vs_cpu_ip_fixed_metric"]
data = {
    "milestones": [{"commit": c, "name": n, "what": w} for c, n, w in MS],
    "series": series, "cpu": cpuref, "kernels": kstack,
    "wall": [round(1e6 / v, 1) if v else None for v in series["parkjoy_1080i|0|1"]],
    "dev": [{"step": s["step"], "us": s["kernel_us"], "b": ("bf=2" in s["step"] or "B pictures" in s["step"] or i > 5)} for i, s in enumerate(dev)],
    "bd": bd,
}

# markdown tables from JOURNEY.md -> html
def md_table(block):
    lines = [l.strip() for l in block.strip().splitlines() if l.strip().startswith("|")]
    if len(lines) < 2:
        return ""
    def cells(l):
        return [c.strip() for c in l.strip("|").split("|")]
    def inline(t):
        t = re.sub(r"`([^`]+)`", r"<code>\1</code>", t)
        return t.replace("–", "&ndash;")
    head = "".join(f"<th>{inline(c)}</th>" for c in cells(lines[0]))
    body = "".join("<tr>" + "".join(f"<td>{inline(c)}</td>" for c in cells(l)) + "</tr>" for l in lines[2:])
    return f'<div class="tw"><table><thead><tr>{head}</tr></thead><tbody>{body}</tbody></table></div>'


def section_table(name):
    m = re.search(rf"<!--BEGIN {name}-->(.*?)<!--END {name}-->", journey, re.S)
    return md_table(m.group(1)) if m else ""


def table_after(heading):
    i = journey.index(heading)
    rest = journey[i:]
    m = re.search(r"(\n\|.*?\n)(?:\n[^|])", rest, re.S)
    return md_table(m.group(1))


fps_table = section_table("FPS")
cpu_table = section_table("CPU")
kern_table = section_table("KERNELS")
base_table = table_after("### The first question")
bd_table = table_after("BD-rate against CPU")
q_table = table_after("At 15 Mbps CBR")
bugs_table = table_after("## Bugs found")

tpl = open(os.path.join(H, "journey_template.html")).read()
out = (tpl.replace("/*DATA*/null", json.dumps(data))
          .replace("<!--FPS_TABLE-->", fps_table).replace("<!--CPU_TABLE-->", cpu_table)
          .replace("<!--KERNEL_TABLE-->", kern_table).replace("<!--BASE_TABLE-->", base_table)
          .replace("<!--BD_TABLE-->", bd_table).replace("<!--Q_TABLE-->", q_table)
          .replace("<!--BUGS_TABLE-->", bugs_table))
open(os.path.join(H, "journey.html"), "w").write(out)
print("wrote docs/journey.html", len(out), "bytes")
