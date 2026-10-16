#!/usr/bin/env bash
# PSNR-Y / SSIM-Y of an encoded file against its source, frame by frame.
# Decodes to y4m first: timestamps of raw ES with B pictures are not reliable
# enough for the psnr filter's frame pairing.
# usage: psnr.sh <encoded> <source> <frames> [psnr|ssim|both]
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
F="$root/build/ffmpeg/ffmpeg"
enc=$1 src=$2 n=$3 what=${4:-psnr}
tmp="$root/out/psnr_$$.y4m"
"$F" -hide_banner -v error -y -i "$enc" -frames:v "$n" -f yuv4mpegpipe "$tmp"
# identical metadata on both inputs: the decoded stream is tagged range=tv and
# the source untagged, which otherwise lets format negotiation insert a range
# conversion depending on which input configures the graph first
# conversion depending on which input configures the graph first. Both
# inputs are trimmed to n frames and the metrics stop at the shorter one:
# otherwise the filter thread can run ahead of -frames:v and average extra
# pairs of (repeated last decoded frame, later source frame).
sp="setparams=range=tv:color_primaries=unknown:color_trc=unknown:colorspace=unknown"
pre="[0:v]trim=end_frame=$n,$sp[x];[1:v]trim=end_frame=$n,$sp[y]"
case $what in
  psnr) lavfi="$pre;[x][y]psnr=shortest=1" ;;
  ssim) lavfi="$pre;[x][y]ssim=shortest=1" ;;
  *)    lavfi="$pre;[x]split[a][b];[y]split[c][d];[a][c]psnr=shortest=1;[b][d]ssim=shortest=1" ;;
esac
"$F" -hide_banner -i "$tmp" -i "$src" -frames:v "$n" -lavfi "$lavfi" -f null - 2>&1 |
  grep -oE '(PSNR y:[0-9.]+|SSIM Y:[0-9.]+)' | tr '\n' ' '
rm -f "$tmp"
