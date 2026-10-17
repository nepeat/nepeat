#!/usr/bin/env python3
"""Pick vast.ai offers for launch.sh: per GPU model, the best few hosts.

usage: pick.py [--anywhere] [--any-cpu] [-n N] "RTX 5070" "RTX 4090" ... > offers.tsv

Hosts must have a single GPU, a driver for CUDA >= 12.8, compute >= 8.0,
reliability > 0.95 and >= 200 Mbps down. Ranked by: newish desktop CPU
(Ryzen 7000/9000, EPYC 4004, Intel 12th-14th gen, Core Ultra) unless
--any-cpu, then US/Canada unless --anywhere, then the share of the host CPU
we get, then price. The host CPU matters as much as the GPU here (per-frame
launch work and ffmpeg's decode pipeline run on it). Writes NAME<TAB>offer,
offer,... lines (fallbacks for hosts that never accept ssh) and a comment per
offer.
"""
import argparse
import json
import re
import subprocess
import sys

NEW_CPU = re.compile(r"Ryzen (5|7|9) (7|9)\d{3}|EPYC 4\d{3}|i[579]-1[234]\d{3}|Core.*Ultra")
SHORT = {"Ryzen": "", "Processor": "", "Core™": "", "Intel": ""}

ap = argparse.ArgumentParser()
ap.add_argument("models", nargs="+")
ap.add_argument("-n", type=int, default=3, help="offers per model (fallbacks)")
ap.add_argument("--anywhere", action="store_true", help="don't prefer US/Canada")
ap.add_argument("--any-cpu", action="store_true", help="don't require a newish desktop CPU")
args = ap.parse_args()

q = ("num_gpus=1 compute_cap>=800 cuda_vers>=12.8 reliability>0.95 "
     "disk_space>=15 inet_down>=200")
offers = json.loads(subprocess.run(["vastai", "search", "offers", q, "-o", "dph",
                                    "--limit", "4000", "--raw"],
                                   capture_output=True, text=True, check=True).stdout)

for model in args.models:
    v = [o for o in offers if o["gpu_name"] == model]
    if not args.any_cpu:
        v = [o for o in v if NEW_CPU.search(o.get("cpu_name") or "")]
    us = lambda o: (o.get("geolocation") or "").endswith(("US", "CA"))
    share = lambda o: o["cpu_cores_effective"] / max(o["cpu_cores"], 1)
    v.sort(key=lambda o: (0 if args.anywhere or us(o) else 1, -round(share(o), 1), o["dph_total"]))
    v = v[:args.n]
    if not v:
        print(f"# {model}: no offers match (try --any-cpu / --anywhere)", file=sys.stderr)
        continue
    for o in v:
        print(f"# {model}: offer {o['id']} ${o['dph_total']:.3f}/h {o.get('geolocation')} "
              f"{o['cpu_name']} {o['cpu_cores_effective']:.0f}/{o['cpu_cores']} threads "
              f"rel {o['reliability']:.3f} net {o['inet_down']:.0f}/{o['inet_up']:.0f}")
    m = re.search(r"i[3579]-\w+|Ultra \d+ \w+|EPYC \w+|\b\d{4}\w*", v[0]["cpu_name"])
    cpu = (m.group(0) if m else v[0]["cpu_name"].split()[0]).replace(" ", "")
    print(f"{model} {cpu}\t{','.join(str(o['id']) for o in v)}")
