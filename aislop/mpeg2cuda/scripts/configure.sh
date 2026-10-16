#!/usr/bin/env bash
# Configure the in-tree FFmpeg build. Run inside `nix develop`.
# --enable-cuda-nvcc requires --enable-nonfree: the resulting binary is for
# local use only and must not be redistributed.
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
mkdir -p "$root/build/ffmpeg"
cd "$root/build/ffmpeg"

"$root/ffmpeg/configure" \
  --prefix="$root/build/install" \
  --enable-gpl --enable-nonfree \
  --enable-cuda-nvcc --enable-ffnvcodec --enable-nvdec --enable-cuvid --enable-nvenc \
  --nvccflags="-gencode arch=compute_86,code=sm_86 -O3 -lineinfo" \
  --disable-doc --disable-debug \
  --disable-ffplay \
  --disable-autodetect --enable-ffnvcodec --enable-pthreads \
  "$@"
