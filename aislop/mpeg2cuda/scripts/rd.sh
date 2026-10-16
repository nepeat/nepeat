#!/usr/bin/env bash
# RD points (kbps,psnr_y) at fixed quantisers for one encoder configuration.
# usage: rd.sh <out.csv> <input> <frames> <encoder> [encoder args...]
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
F="$root/build/ffmpeg/ffmpeg"
out=$1 in=$2 n=$3 enc=$4; shift 4
tmp="$root/out/rd_$$.m2v"; : > "$out"
for q in ${QS:-3 5 8 12 18}; do
  "$F" -hide_banner -v error -y -i "$in" -frames:v "$n" -c:v "$enc" -qscale:v $q "$@" -f mpeg2video "$tmp"
  p=$("$root/scripts/psnr.sh" "$tmp" "$in" "$n" | grep -oE 'PSNR y:[0-9.]+' | cut -d: -f2)
  echo "$(stat -c %s "$tmp" | awk -v n=$n '{printf "%.1f", $1*8/(n/29.97)/1000}'),$p" >> "$out"
done
rm -f "$tmp"
