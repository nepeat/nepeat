#!/usr/bin/env bash
# Encode a clip with mpeg2video (CPU) and mpeg2_cuda (GPU), report size,
# PSNR/SSIM and encoder speed. Input is decoded from raw y4m so the speed is
# (almost) encoder-only.
# usage: compare.sh <input.y4m> <frames> <ratecontrol args...>
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
F="$root/build/ffmpeg/ffmpeg"
in=$1; frames=$2; shift 2
o="$root/out/compare"; mkdir -p "$o"
il="-flags +ilme+ildct -alternate_scan 1"

run() {
  local name=$1; shift
  local log="$o/$name.log"
  "$F" -hide_banner -y -i "$in" -frames:v "$frames" "$@" -an -f mpeg2video "$o/$name.m2v" -benchmark >"$log" 2>&1
  local fps; fps=$(grep -oE 'frame= *[0-9]+ fps= *[0-9.]+' "$log" | tail -1 | grep -oE '[0-9.]+$')
  local rt; rt=$(grep -oE 'rtime=[0-9.]+s' "$log" | grep -oE '[0-9.]+')
  local bytes; bytes=$(stat -c %s "$o/$name.m2v")
  local q; q=$("$root/scripts/psnr.sh" "$o/$name.m2v" "$in" "$frames" both)
  awk -v n="$name" -v b="$bytes" -v f="$frames" -v rt="$rt" -v q="$q" \
    'BEGIN { printf "%-14s %7.2f Mbps  %7.1f fps(wall)  %s\n", n, b*8/(f/29.97)/1e6, f/rt, q }'
}

run cpu_fast   -c:v mpeg2video -bf 0 $il -intra_vlc 1 "$@"
run cpu_fast_b2 -c:v mpeg2video -bf 2 $il -intra_vlc 1 "$@"
run gpu        -c:v mpeg2_cuda "$@"
