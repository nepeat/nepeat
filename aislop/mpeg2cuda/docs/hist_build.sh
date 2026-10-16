#!/usr/bin/env bash
# Build a minimal FFmpeg (mpeg2_cuda + mpeg2video + y4m input) and the
# encoder-only benchmark for each milestone commit, in git worktrees under
# build/hist/. Run inside `nix develop` from aislop/mpeg2cuda.
# usage: docs/hist_build.sh [commit...]
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
commits=("$@")
[[ ${#commits[@]} -gt 0 ]] || mapfile -t commits < <(git -C "$root/ffmpeg" rev-list --reverse n9.0.2..HEAD | cut -c1-7)
for c in "${commits[@]}"; do
  wt="$root/build/hist/wt-$c"; b="$root/build/hist/b-$c"
  [[ -x "$b/m2c_bench" ]] && { echo "have $c"; continue; }
  [[ -d $wt ]] || git -C "$root/ffmpeg" worktree add --detach "$wt" "$c" >/dev/null
  mkdir -p "$b"
  ( cd "$b" && "$wt/configure" --enable-gpl --enable-nonfree \
      --enable-cuda-nvcc --enable-ffnvcodec \
      --nvccflags="-gencode arch=compute_86,code=sm_86 -O3 -lineinfo" \
      --disable-doc --disable-debug --disable-programs --disable-autodetect \
      --enable-ffnvcodec --enable-pthreads --disable-everything \
      --enable-encoder=mpeg2_cuda,mpeg2video --enable-decoder=rawvideo \
      --enable-demuxer=yuv4mpegpipe,rawvideo --enable-protocol=file \
      --disable-swscale --disable-avfilter --disable-avdevice --disable-swresample >/dev/null )
  make -C "$b" -j16 >"$b/make.log" 2>&1
  gcc -O2 -o "$b/m2c_bench" "$root/tools/m2c_bench.c" -I"$wt" -I"$b" \
    "$b/libavformat/libavformat.a" "$b/libavcodec/libavcodec.a" "$b/libavutil/libavutil.a" \
    -lpthread -lm -ldl
  echo "built $c"
done
