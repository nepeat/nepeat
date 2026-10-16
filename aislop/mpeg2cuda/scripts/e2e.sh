#!/usr/bin/env bash
# End-to-end: N concurrent live (-re) NVDEC H.264 -> mpeg2_cuda + AC-3 -> MPEG-TS
# pipelines; reports the slowest stream's final speed and CPU use.
# usage: e2e.sh <source.ts> <N> [seconds]
set -uo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
F="$root/build/ffmpeg/ffmpeg"
src=$1; n=$2; t=${3:-45}
o="$root/out/e2e"; rm -rf "$o"; mkdir -p "$o"
pids=()
read -r _ u0 n0 s0 i0 _ < /proc/stat
for k in $(seq 1 "$n"); do
  "$F" -hide_banner -nostdin -re -stream_loop -1 -hwaccel cuda -hwaccel_output_format cuda \
    -i "$src" -t "$t" -c:v mpeg2_cuda -b:v 15M -c:a ac3 -b:a 192k \
    -f mpegts -muxrate 16M -y /dev/null >"$o/$k.log" 2>&1 &
  pids+=($!)
done
wait "${pids[@]}"
read -r _ u1 n1 s1 i1 _ < /proc/stat
busy=$(( (u1-u0)+(n1-n0)+(s1-s0) )); tot=$(( busy + (i1-i0) ))
min=$(for k in $(seq 1 "$n"); do tr '\r' '\n' < "$o/$k.log" | grep -oE 'speed= *[0-9.]+' | tail -1 | grep -oE '[0-9.]+'; done | sort -g | head -1)
errs=$(grep -il 'error' "$o"/*.log | wc -l)
echo "N=$n slowest speed=${min}x cpu=$(( 100 * busy / tot ))% logs_with_errors=$errs"
