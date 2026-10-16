#!/usr/bin/env bash
# Bit-exactness + syntax check: the encoder's reconstruction must equal what
# FFmpeg's MPEG-2 decoder (simple_idct) produces from the bitstream.
# usage: verify.sh <input> <frames> [encoder options...]
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
F="$root/build/ffmpeg/ffmpeg"
in=$1; frames=$2; shift 2
o="$root/out/verify"; mkdir -p "$o"

"$F" -hide_banner -v error -y -i "$in" -frames:v "$frames" -c:v mpeg2_cuda \
  -recon_file "$o/rec.yuv" "$@" -f mpeg2video "$o/enc.m2v"
"$F" -hide_banner -v error -err_detect explode -xerror -idct simple -i "$o/enc.m2v" \
  -f rawvideo -pix_fmt yuv420p -y "$o/dec.yuv"

if cmp -s "$o/rec.yuv" "$o/dec.yuv"; then
  echo "BITEXACT ($(stat -c %s "$o/enc.m2v") bytes)"
else
  echo "MISMATCH"; cmp "$o/rec.yuv" "$o/dec.yuv" | head -3
  exit 1
fi
"$F" -hide_banner -i "$o/enc.m2v" -i "$in" -frames:v "$frames" \
  -lavfi "[0:v][1:v]psnr" -f null - 2>&1 | grep -oE 'PSNR y:[0-9.]+ u:[0-9.]+ v:[0-9.]+ average:[0-9.]+'
