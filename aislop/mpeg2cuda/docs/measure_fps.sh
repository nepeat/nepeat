#!/usr/bin/env bash
# Encoder fps per milestone build, both test clips, idle GPU.
# Bench: tools/m2c_bench.c plus av_frame_copy_props() after the GPU upload
# (build/hist/m2c_bench_hist.c) so interlaced sources stay interlaced.
# K=60 preloaded frames, N=6000 per instance (startup amortised), 15 Mbps.
# M=1: the instance's own rate; M=4: total frames / wall time. Best of two.
# Writes docs/data/fps.csv and docs/data/kernels_pj.csv (nsys, park_joy, bf=0).
# Run inside `nix develop` from aislop/mpeg2cuda.
set -uo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
d="$root/docs/data"
commits=$(git -C "$root/ffmpeg" rev-list --reverse n9.0.2..HEAD | cut -c1-7)
parse() { sed -nE 's/.*aggregate ([0-9.]+) fps, slowest instance ([0-9.]+) fps, avg ([0-9.]+) Mbps.*/\1 \2 \3/p'; }
echo "commit,idx,clip,bf,instances,fps,mbps" > "$d/fps.csv"
echo "commit,bf,kernel,us_per_call,calls" > "$d/kernels_pj.csv"
i=0; seen_b=0
for c in $commits; do
  b="$root/build/hist/b-$c"; wt="$root/build/hist/wt-$c"
  gcc -O2 -o "$b/m2c_bench_hist" "$root/build/hist/m2c_bench_hist.c" -I"$wt" -I"$b" \
    "$b/libavformat/libavformat.a" "$b/libavcodec/libavcodec.a" "$b/libavutil/libavutil.a" -lpthread -lm -ldl
  B="$b/m2c_bench_hist"
  [[ $c == 39fe7db ]] && seen_b=1
  bfs="0"; ((seen_b)) && bfs="0 2"
  for clip in touchdown_1080p parkjoy_1080i; do
    for bf in $bfs; do
      for m in 1 4; do
        best=0; mb=0
        for rep in 1 2; do
          read -r agg slow r < <("$B" "$root/media/$clip.y4m" mpeg2_cuda 60 6000 $m b=15M bf=$bf 2>/dev/null | parse)
          f=$agg; [[ $m == 1 ]] && f=$slow
          awk -v a="$f" -v b="$best" 'BEGIN{exit !(a > b)}' && { best=$f; mb=$r; }
        done
        echo "$c,$i,$clip,$bf,$m,$best,$mb" | tee -a "$d/fps.csv"
      done
    done
  done
  ( cd "$root/out" && nsys profile -t cuda -o histpj_${c} -f true "$B" "$root/media/parkjoy_1080i.y4m" mpeg2_cuda 60 240 1 b=15M bf=0 >/dev/null 2>&1 &&
    nsys stats --force-export=true -r cuda_gpu_kern_sum -f csv histpj_${c}.nsys-rep 2>/dev/null ) |
    python3 -c '
import csv, sys
rows = list(csv.reader(l for l in sys.stdin if l.count(",") > 3))
for r in rows[1:]:
    print(f"'"$c"',0,{r[-1]},{float(r[3])/1000:.1f},{r[2]}")' >> "$d/kernels_pj.csv"
  i=$((i + 1))
done
