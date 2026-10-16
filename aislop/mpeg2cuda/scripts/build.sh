#!/usr/bin/env bash
# Incremental build. nvcc has no dependency tracking in FFmpeg's build, so
# force the encoder's PTX to rebuild every time. Fails if make fails.
set -uo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
rm -f "$root"/build/ffmpeg/libavcodec/mpeg2_cuda.ptx*
log="$root/build/make_last.log"
make -C "$root/build/ffmpeg" -j"$(nproc)" ffmpeg ffprobe >"$log" 2>&1
status=$?
grep -E "error|mpeg2_cuda.*warning|warning:.*mpeg2_cuda" "$log"
if [ "$status" -ne 0 ]; then
  echo "make failed ($status), see $log" >&2
  exit "$status"
fi
"$root/scripts/build_bench.sh"
