#!/usr/bin/env bash
# Build tools/m2c_bench against the in-tree static FFmpeg libraries.
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
b="$root/build/ffmpeg"
gcc -O2 -o "$root/build/m2c_bench" "$root/tools/m2c_bench.c" \
  -I"$root/ffmpeg" -I"$b" \
  "$b/libavformat/libavformat.a" "$b/libavcodec/libavcodec.a" \
  "$b/libswresample/libswresample.a" "$b/libavutil/libavutil.a" \
  -lpthread -lm -ldl
