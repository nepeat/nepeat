#!/usr/bin/env python3
"""Summarize bench.sh results: one markdown row per run.

usage: summarize.py [results_dir]   (default: sidequest/vast/results)

Reads every bench.log under the directory (top-level *_bench.log files from
the first batch and <run>/bench.log from run_remote.sh) and prints, per run:
GPU, host CPU, best encoder-only fps (touchdown I/P, park_joy B2), best NVDEC
fps, peak end-to-end fps and the 1080i29.97 channels that makes.
"""
import pathlib
import re
import sys

root = pathlib.Path(sys.argv[1] if len(sys.argv) > 1 else
                    pathlib.Path(__file__).parent / "results")
rows = []
for log in sorted(set(root.glob("*_bench.log")) | set(root.glob("*/bench.log"))):
    if ".fail" in str(log) or ".old" in str(log):
        continue
    text = log.read_text(errors="replace")
    gpu = re.search(r"^NVIDIA ([^,]+),", text, re.M)
    cpu = re.search(r"model name\s*:\s*(.+)", text)
    pl = re.search(r"^NVIDIA [^,]+, [^,]+, [^,]+, ([\d.]+) W", text, re.M)
    md5 = re.findall(r"^md5,[^,]*,[^,]*,[^,]*,,(\S+)", text, re.M)
    val = {}
    for t, clip, bf, m, fps in re.findall(r"^(encode|nvdec|e2e),([^,]*),([^,]*),(\d+),([\d.]+|FAIL)", text, re.M):
        if fps != "FAIL":
            val.setdefault((t, clip[:4], bf), []).append((float(fps), int(m)))
    best = lambda k: max(val.get(k, [(0, 0)]))
    e2e = best(("e2e", "mezz", "0"))
    fails = len(re.findall(r"^e2e,.*FAIL", text, re.M))
    rows.append((gpu.group(1) if gpu else "?", (cpu.group(1).strip() if cpu else "?"),
                 pl.group(1) if pl else "?", "ok" if md5 and all(x == "match" for x in md5) else "MISMATCH",
                 best(("encode", "touc", "0"))[0], best(("encode", "park", "2"))[0],
                 best(("nvdec", "mezz", ""))[0], e2e, fails, log.parent.name if log.name == "bench.log" else log.stem))

print("| GPU | host CPU | W | md5 | enc touchdown I/P | enc park_joy B2 | NVDEC | e2e peak (M) | channels | run |")
print("|---|---|---|---|---|---|---|---|---|---|")
for g, c, w, m, td, pj, nv, (e, em), f, run in sorted(rows, key=lambda r: -r[7][0]):
    c = re.sub(r"\(R\)|\(TM\)|CPU|Processor|\d+-Core|@.*|Core™|AMD |Intel ", "", c).strip()
    print(f"| {g} | {c} | {w} | {m} | {td:.0f} | {pj:.0f} | {nv:.0f} | {e:.0f} ({em}){' *' if f else ''} | {e / 29.97:.0f} | {run} |")
