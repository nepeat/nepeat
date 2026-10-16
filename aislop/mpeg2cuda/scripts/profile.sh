#!/usr/bin/env bash
# Per-kernel GPU time for one encoder instance (nsys).
# usage: profile.sh [frames] [encoder opts...]
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
n=${1:-300}; shift || true
cd "$root/out"
nsys profile -t cuda -o prof -f true "$root/build/m2c_bench" "$root/media/touchdown_1080p.y4m" \
  mpeg2_cuda 60 "$n" 1 b=15M "$@" >/dev/null 2>&1
nsys stats --force-export=true -r cuda_gpu_kern_sum -f csv prof.nsys-rep 2>/dev/null | python3 -c '
import csv, sys
rows = list(csv.reader(l for l in sys.stdin if l.count(",") > 3))
tot = 0
for r in rows[1:]:
    print(f"{r[-1]:16s} {float(r[3])/1000:9.1f} us/call  {r[0]:>5s}%")
    tot += float(r[1])
print("total/frame      %9.1f us" % (tot/1000/'"$n"'))'
