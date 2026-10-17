#!/usr/bin/env python3
"""Summarize bench.sh runs as a markdown table.

usage: summarize.py            table from results.csv
       summarize.py --collect  first fold every run's bench.log under results/
                               (gitignored) into results.csv, then print

results.csv has one row per measurement with the run, GPU, host CPU and GPU
power limit, so the table can be rebuilt from git alone. Per run the table
shows the best encoder-only fps (touchdown I/P, park_joy B2), the best NVDEC
fps and the peak end-to-end fps with the 1080i29.97 channels that makes.
"""
import csv
import pathlib
import re
import sys
from collections import defaultdict

here = pathlib.Path(__file__).parent
res = here / "results"
out = here / "results.csv"
FIELDS = ["run", "gpu", "host_cpu", "power_w", "test", "clip", "bf", "instances", "fps", "note"]


def collect():
    rows = []
    for log in sorted(res.glob("*/bench.log")):
        if "." in log.parent.name:          # *.fail1, *.old
            continue
        text = log.read_text(errors="replace")
        hdr = re.search(r"^NVIDIA ([^,]+), [^,]+, [^,]+, ([\d.]+) W", text, re.M)
        cpu = re.search(r"model name\s*:\s*(.+)", text)
        base = {"run": log.parent.name, "gpu": hdr.group(1) if hdr else "?",
                "host_cpu": cpu.group(1).strip() if cpu else "?", "power_w": hdr.group(2) if hdr else ""}
        for line in text.splitlines():
            p = line.split(",", 5)
            if len(p) == 6 and p[0] in ("md5", "encode", "nvdec", "e2e"):
                rows.append(base | dict(zip(FIELDS[4:], p)))
    with out.open("w", newline="") as f:
        w = csv.DictWriter(f, FIELDS)
        w.writeheader()
        w.writerows(rows)
    print(f"{out.name}: {len(rows)} rows from {len({r['run'] for r in rows})} runs", file=sys.stderr)


def table():
    runs = defaultdict(lambda: {"best": defaultdict(lambda: (0.0, 0)), "md5": [], "fail": 0})
    for r in csv.DictReader(out.open()):
        run = runs[r["run"]]
        run["meta"] = r
        if r["test"] == "md5":
            run["md5"].append(r["note"])
        elif r["fps"] in ("", "FAIL"):
            run["fail"] += r["test"] == "e2e"
        else:
            k = (r["test"], r["clip"][:4], r["bf"])
            run["best"][k] = max(run["best"][k], (float(r["fps"]), int(r["instances"])))

    print("| GPU | host CPU | W | md5 | enc touchdown I/P | enc park_joy B2 | NVDEC | e2e peak (M) | channels | run |")
    print("|---|---|---|---|---|---|---|---|---|---|")
    for name, run in sorted(runs.items(), key=lambda kv: -kv[1]["best"][("e2e", "mezz", "0")][0]):
        m, b = run["meta"], run["best"]
        cpu = re.sub(r"\(R\)|\(TM\)|CPU|Processor|\d+-Core|@.*|Core™|AMD |Intel |with Radeon Graphics", "", m["host_cpu"]).strip()
        e, em = b[("e2e", "mezz", "0")]
        md5 = "ok" if run["md5"] and all(x == "match" for x in run["md5"]) else "MISMATCH"
        print(f"| {m['gpu']} | {cpu} | {m['power_w']} | {md5} | {b[('encode', 'touc', '0')][0]:.0f} | "
              f"{b[('encode', 'park', '2')][0]:.0f} | {b[('nvdec', 'mezz', '')][0]:.0f} | "
              f"{e:.0f} ({em}){' *' if run['fail'] else ''} | {e / 29.97:.0f} | {name} |")


if "--collect" in sys.argv:
    collect()
table()
