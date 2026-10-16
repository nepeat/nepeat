#!/usr/bin/env bash
# GPU-only quality check: bitrate, PSNR-Y, SSIM-Y.
# usage: gpuq.sh <input> <frames> [encoder args...]
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
F="$root/build/ffmpeg/ffmpeg"
in=$1; frames=$2; shift 2
o="$root/out/gpuq.m2v"
"$F" -hide_banner -v error -y -i "$in" -frames:v "$frames" -c:v mpeg2_cuda "$@" -f mpeg2video "$o"
q=$("$root/scripts/psnr.sh" "$o" "$in" "$frames" both)
awk -v b="$(stat -c %s "$o")" -v f="$frames" -v q="$q" -v a="$*" \
  'BEGIN { printf "%6.2f Mbps  %s  [%s]\n", b*8/(f/29.97)/1e6, q, a }'
