#!/usr/bin/env python3
"""Charts for docs/JOURNEY.md from docs/data/*. PNG + SVG into docs/img/.

usage (from aislop/mpeg2cuda):
  nix shell nixpkgs#python3Packages.matplotlib nixpkgs#python3Packages.numpy \
    --command python3 docs/make_charts.py
"""
import csv, json, os
import matplotlib
matplotlib.use("Agg")
import matplotlib.pyplot as plt

D = os.path.join(os.path.dirname(__file__), "data")
O = os.path.join(os.path.dirname(__file__), "img")
os.makedirs(O, exist_ok=True)

# reference palette (validated: tools in the dataviz skill), light mode
SERIES = ["#2a78d6", "#eb6834", "#1baf7a", "#eda100", "#e87ba4", "#008300", "#6250d6", "#e34948"]
INK, INK2, MUTED, GRID, AXIS, SURF = "#0b0b0b", "#52514e", "#898781", "#e1e0d9", "#c3c2b7", "#fcfcfb"

plt.rcParams.update({
    "font.family": "sans-serif", "font.size": 10, "axes.edgecolor": AXIS,
    "axes.labelcolor": INK2, "xtick.color": MUTED, "ytick.color": MUTED,
    "axes.titlesize": 12, "axes.titleweight": "bold", "axes.titlecolor": INK,
    "axes.spines.top": False, "axes.spines.right": False,
    "figure.facecolor": SURF, "axes.facecolor": SURF, "savefig.facecolor": SURF,
    "axes.grid": True, "axes.grid.axis": "y", "grid.color": GRID, "grid.linewidth": 0.8,
    "legend.frameon": False,
})

LABELS = {
    "dea7407": "1 phase 1\nI/P frame DCT",
    "f554c30": "2 field tools\nvector ME",
    "525ec8a": "3 temporal ME\nRC",
    "9819724": "4 review fixes",
    "39fe7db": "5 B pictures",
    "6fe6f2c": "6 fused VLC\nbandwidth",
    "36c2f15": "7 cleanup",
    "0aa2090": "8 progressive\nframes",
}


def save(fig, name):
    for ext in ("png", "svg"):
        fig.savefig(os.path.join(O, f"{name}.{ext}"), dpi=160, bbox_inches="tight")
    plt.close(fig)


def read_csv(name):
    with open(os.path.join(D, name)) as f:
        return list(csv.DictReader(f))


fps = read_csv("fps.csv")
cpu = read_csv("cpu.csv")
commits = list(dict.fromkeys(r["commit"] for r in fps))
LABELS.setdefault("e14023c", "9 RC carry\nclamp")
LABELS.setdefault("9173f3e", '10 field weave')
LABELS.setdefault("251534c", '11 pipelining')
LABELS.setdefault("143f7f5", '12 kernel trims')
LABELS.setdefault("ac0a6b1", '13 SIMD +\nlatency')
LABELS.setdefault("ecbf52c", '14 decimation')
LABELS.setdefault("3811bb7", '15 zero-copy\n+ tail stream')
LABELS.setdefault("e8612c6", '16 load stream')
CLIP = "parkjoy_1080i"      # interlaced: the 1080i target


def cpu_val(clip, bf, inst):
    for r in cpu:
        if r["clip"] == clip and r["mode"] == "CBR" and r["bf"] == str(bf) and r["instances"] == str(inst):
            return float(r["aggregate_fps"])


def gpu_val(c, clip, bf, inst):
    v = [float(r["fps"]) for r in fps if r["commit"] == c and r["clip"] == clip
         and r["bf"] == str(bf) and r["instances"] == str(inst)]
    return v[0] if v else None


def bars(inst, title, name, cpu_inst, cpu_label):
    fig, ax = plt.subplots(figsize=(15, 4.6))
    x = list(range(len(commits)))
    w = 0.38
    top = 0
    for k, bf in enumerate((0, 2)):
        pts = [(i + (k - 0.5) * w, gpu_val(c, CLIP, bf, inst)) for i, c in enumerate(commits)]
        pts = [(a, b) for a, b in pts if b]
        top = max([top] + [b for _, b in pts])
        ax.bar([a for a, _ in pts], [b for _, b in pts], width=w - 0.04, color=SERIES[k],
               label=f"GPU, {'I/P' if bf == 0 else '2 B pictures'}", zorder=3)
        for a, b in pts:
            ax.text(a, b + top * 0.01, f"{b:.0f}", ha="center", va="bottom", fontsize=7.5, color=INK2)
    for bf, style in ((0, "--"), (2, ":")):
        val = cpu_val(CLIP, bf, cpu_inst)
        ax.axhline(val, color=INK2, lw=1, ls=style, zorder=2,
                   label=f"{cpu_label} {'I/P' if bf == 0 else 'B2'}: {val:.0f}")
    ax.set_xticks(x, [LABELS.get(c, c).replace("\n", " ") for c in commits], fontsize=8,
                  rotation=35, ha="right", rotation_mode="anchor")
    ax.set_ylabel("frames per second")
    ax.set_title(title, loc="left")
    ax.legend(loc="upper left", ncols=2, fontsize=8.5)
    ax.set_ylim(0, top * 1.22)
    save(fig, name)


bars(1, "One stream: park_joy 1080i, 15 Mbps CBR, encoder only", "fps_single", 1, "CPU 1 stream")
bars(4, "Whole GPU (4 instances) vs whole CPU (8 x 4 threads)", "fps_aggregate", 8, "CPU 16 cores")

# recorded development steps: kernel time per picture
rec = json.load(open(os.path.join(D, "recorded.json")))
steps = [s for s in rec["gpu_dev_steps"] if "kernel_us" in s and "idle" not in s["step"]]
fig, ax = plt.subplots(figsize=(10, 4.2))
ys = [s["kernel_us"] for s in steps]
xs = list(range(len(steps)))
cols = [SERIES[1] if "bf=2" in s["step"] or "B pictures" in s["step"] or i > 5 else SERIES[0]
        for i, s in enumerate(steps)]
ax.bar(xs, ys, color=cols, width=0.7, zorder=3)
for a, b in zip(xs, ys):
    ax.text(a, b + 6, f"{b:.0f}", ha="center", fontsize=8, color=INK2)
short = ["phase 1", "vsadu4 SAD", "lane/cand\nrefine", "row encode\n+ field DCT", "field ME v1",
         "field ME\nfast", "B pictures", "vector load", "fused VLC", "const tables", "32-cand\nrefine"]
ax.set_xticks(xs, short[:len(xs)], fontsize=8)
ax.set_ylabel("GPU kernel time per picture (µs)")
ax.set_title("Kernel time per picture during development (blue: I/P, orange: with B pictures)", loc="left")
save(fig, "dev_steps")

# per-kernel breakdown per milestone (bf=0)
kern = read_csv("kernels_pj.csv")
names = {}
for r in kern:
    if r["bf"] == "0" and r["kernel"] != "k_init_tables":
        names[r["kernel"]] = names.get(r["kernel"], 0) + float(r["us_per_call"]) * int(r["calls"])
top = [k for k, _ in sorted(names.items(), key=lambda kv: -kv[1])][:7]
fig, ax = plt.subplots(figsize=(15, 4.8))
bottom = [0.0] * len(commits)
for si, k in enumerate(top + ["other"]):
    vals = []
    for ci, c in enumerate(commits):
        rows = [r for r in kern if r["commit"] == c and r["bf"] == "0" and r["kernel"] != "k_init_tables"]
        n = max([int(r["calls"]) for r in rows if r["kernel"] == "k_encode_mb"] or [300])
        if k == "other":
            v = sum(float(r["us_per_call"]) * int(r["calls"]) for r in rows if r["kernel"] not in top) / n
        else:
            v = sum(float(r["us_per_call"]) * int(r["calls"]) for r in rows if r["kernel"] == k) / n
        vals.append(v)
    ax.bar(range(len(commits)), vals, bottom=bottom, color=(SERIES + ["#c3c2b7"])[si], width=0.62,
           label=k.replace("k_", ""), edgecolor=SURF, linewidth=1.2, zorder=3)
    bottom = [a + b for a, b in zip(bottom, vals)]
# wall time per picture (one stream, I/P): from milestone 15 on the kernels
# overlap on three streams, so they each run longer while a picture gets faster
wall = [1e6 / v if v else None for v in (gpu_val(c, CLIP, 0, 1) for c in commits)]
for i, t in enumerate(bottom):
    ax.text(i, max(t, wall[i] or 0) + 12, f"{t:.0f}", ha="center", fontsize=8, color=INK2)
ax.plot(range(len(commits)), wall, color=INK, lw=1.8, marker="o", ms=4, zorder=4,
        label="wall time per picture")
ax.set_xticks(range(len(commits)), [LABELS.get(c, c).replace("\n", " ") for c in commits], fontsize=8,
              rotation=35, ha="right", rotation_mode="anchor")
ax.set_ylabel("µs per picture (park_joy 1080i, I/P)")
ax.set_title("Where the GPU time goes, per milestone (bars: kernel time, line: wall time)", loc="left")
ax.legend(ncols=4, fontsize=8, loc="upper right")
save(fig, "kernels")

# BD-rate vs CPU I/P
bd = rec["bd_rate_vs_cpu_ip_fixed_metric"]
cfg = [("cpu_bf2", "CPU mpeg2video B2"), ("gpu_bf0", "GPU I/P"),
       ("gpu_bf2_39fe7db", "GPU B2 (5 B pictures)"), ("gpu_bf2_current", "GPU B2 (current)")]
fig, ax = plt.subplots(figsize=(8, 3.6))
h = 0.36
for k, (clip, lab) in enumerate((("td", "touchdown 1080p"), ("pj", "park_joy 1080i"))):
    ys = [i + (k - 0.5) * h for i in range(len(cfg))]
    vs = [bd[c][clip] for c, _ in cfg]
    ax.barh(ys, vs, height=h - 0.04, color=SERIES[k], label=lab, zorder=3)
    for y, v in zip(ys, vs):
        ax.text(v + (0.25 if v >= 0 else -0.25), y, f"{v:+.1f}%", va="center",
                ha="left" if v >= 0 else "right", fontsize=8, color=INK2)
ax.axvline(0, color=AXIS, lw=1)
ax.set_yticks(range(len(cfg)), [l for _, l in cfg])
ax.invert_yaxis()
ax.grid(axis="x"); ax.grid(axis="y", visible=False)
ax.set_xlabel("BD-rate vs CPU mpeg2video I/P (negative = fewer bits at equal PSNR)")
ax.set_title("Bits needed for the same quality", loc="left")
ax.legend(loc="lower right", fontsize=8)
save(fig, "bdrate")

# RD curves
for clip, title in (("td", "touchdown 1080p (progressive)"), ("pj", "park_joy 1080i")):
    fig, ax = plt.subplots(figsize=(6.4, 4.2))
    for si, (f, lab) in enumerate(((f"rdc_{clip}_b0.csv", "CPU I/P"), (f"rdc_{clip}_b2.csv", "CPU B2"),
                                   (f"rdg_{clip}_b0.csv", "GPU I/P"), (f"rdg_{clip}_b2.csv", "GPU B2"))):
        p = os.path.join(D, f)
        if not os.path.exists(p):
            continue
        pts = sorted(tuple(map(float, l.split(","))) for l in open(p) if l.strip())
        ax.plot([a / 1000 for a, _ in pts], [b for _, b in pts], marker="o", ms=4, lw=2,
                color=SERIES[[0, 1, 2, 3][si]], label=lab)
    ax.set_xscale("log")
    ax.set_xlabel("Mbps (log)")
    ax.set_ylabel("PSNR-Y (dB)")
    ax.grid(axis="x")
    ax.set_title(f"Rate-distortion, {title}", loc="left")
    ax.legend(fontsize=8)
    save(fig, f"rd_{clip}")
print("charts written to", O)
