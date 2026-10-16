#!/usr/bin/env bash
# Measure every milestone build (docs/hist_build.sh) on an idle GPU with the
# same harness: encoder-only fps (tools/m2c_bench, touchdown 1080p, K=60
# preloaded frames, N=900, 15 Mbps) at M=1 and M=4 instances, bf=0 and (where
# supported) bf=2; per-kernel GPU time with nsys; CPU mpeg2video baselines.
# Writes docs/data/fps.csv, docs/data/kernels.csv, docs/data/cpu.csv.
# Run inside `nix develop` from aislop/mpeg2cuda.
set -uo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
IN="$root/media/touchdown_1080p.y4m"
d="$root/docs/data"; mkdir -p "$d"
commits=$(git -C "$root/ffmpeg" rev-list --reverse n9.0.2..HEAD | cut -c1-7)
bpics=39fe7db   # first commit with B pictures

parse() { sed -nE 's/.*aggregate ([0-9.]+) fps, slowest instance ([0-9.]+) fps, avg ([0-9.]+) Mbps.*/\1,\2,\3/p'; }

echo "commit,idx,bf,instances,aggregate_fps,slowest_fps,mbps" > "$d/fps.csv"
echo "commit,bf,kernel,us_per_call,calls" > "$d/kernels.csv"
i=0; seen_b=0
for c in $commits; do
  B="$root/build/hist/b-$c/m2c_bench"
  [[ $c == "$bpics" ]] && seen_b=1
  bfs="0"; ((seen_b)) && bfs="0 2"
  for bf in $bfs; do
    for m in 1 4; do
      r=$("$B" "$IN" mpeg2_cuda 60 900 $m b=15M bf=$bf 2>/dev/null | parse)
      echo "$c,$i,$bf,$m,$r" | tee -a "$d/fps.csv"
    done
    # per-kernel time, 300 pictures, one instance
    ( cd "$root/out" && nsys profile -t cuda -o hist_${c}_${bf} -f true "$B" "$IN" mpeg2_cuda 60 300 1 b=15M bf=$bf >/dev/null 2>&1 &&
      nsys stats --force-export=true -r cuda_gpu_kern_sum -f csv hist_${c}_${bf}.nsys-rep 2>/dev/null ) |
      python3 -c '
import csv, sys
rows = list(csv.reader(l for l in sys.stdin if l.count(",") > 3))
for r in rows[1:]:
    print(f"'"$c"','"$bf"',{r[-1]},{float(r[3])/1000:.1f},{r[2]}")' >> "$d/kernels.csv"
  done
  i=$((i + 1))
done

# CPU baselines (same harness, current build), see docs/JOURNEY.md
B="$root/build/hist/b-$(echo "$commits" | tail -1)/m2c_bench"
COMMON="g=15 flags=+ilme+ildct alternate_scan=1 intra_vlc=1"
echo "mode,bf,instances,threads,aggregate_fps,slowest_fps,mbps" > "$d/cpu.csv"
for mode in CBR ABR; do
  rc="b=15M"; [[ $mode == CBR ]] && rc="b=15M minrate=15M maxrate=15M bufsize=7.5M"
  for bf in 0 2; do
    r=$("$B" "$IN" mpeg2video 60 300 1 $rc $COMMON bf=$bf threads=0 2>/dev/null | parse); echo "$mode,$bf,1,auto,$r" | tee -a "$d/cpu.csv"
    r=$("$B" "$IN" mpeg2video 60 150 8 $rc $COMMON bf=$bf threads=4 2>/dev/null | parse); echo "$mode,$bf,8,4,$r" | tee -a "$d/cpu.csv"
  done
done
