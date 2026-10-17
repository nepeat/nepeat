#!/usr/bin/env bash
# Portable mpeg2_cuda benchmark for rented GPUs (vast.ai etc.). Runs from the
# bundle directory made by sidequest/vast/bundle.sh; needs only the NVIDIA
# driver in the container (libcuda; libnvcuvid for the NVDEC tests).
#
#   ./bench.sh [quick]    -> results/<gpu>_<time>.csv + summary on stdout
#
# 1. correctness: md5 of 4 encodes vs the RTX 3090 reference (the encoder is
#    deterministic integer code, so every GPU must produce the same bytes)
# 2. encoder only (m2c_bench, 60 preloaded frames from lossless FFV1 clips,
#    15M): touchdown 1080p and park_joy 1080i, bf 0/2, M parallel instances
# 3. NVDEC only: M parallel decodes of a 1080p H.264 mezz clip
# 4. end to end: M parallel NVDEC -> mpeg2_cuda (the live channel pipeline,
#    video only, as fast as possible)
# A failed parallel run records the first error line (e.g. out of memory).
set -uo pipefail

D="$(cd "$(dirname "$0")" && pwd)"
LP="$D/lib:/run/opengl-driver/lib:/usr/lib/x86_64-linux-gnu:/usr/lib64:/usr/local/nvidia/lib64:/usr/local/nvidia/lib"
MEZZ="$D/media/mezz_h264.ts"

run() { local b=$1; shift; "$D/lib/ld-linux-x86-64.so.2" --library-path "$LP" "$D/bin/$b" "$@"; }
row() { echo "$1" >> "$csv"; echo "$1"; }

dec() {   # dec M extra-args... : M parallel ffmpeg on the mezz clip -> "fps,note"
  local m=$1; shift; local pids=() t0 t1 k p ok=1
  t0=$(date +%s.%N)
  for k in $(seq 1 "$m"); do
    run ffmpeg -hide_banner -nostdin -v error -hwaccel cuda -hwaccel_output_format cuda \
      -i "$MEZZ" -map 0:v:0 "$@" -f null - 2>"$D/results/.dec$k.log" &
    pids+=($!)
  done
  for p in "${pids[@]}"; do wait "$p" || ok=0; done
  t1=$(date +%s.%N)
  if ((ok)); then
    awk -v m="$m" -v n="$frames" -v a="$t0" -v b="$t1" 'BEGIN { printf "%.1f,", m * n / (b - a) }'
  else
    echo "FAIL,$(cat "$D"/results/.dec*.log | grep -m1 -v '^$' | tr ',\n' '; ' | cut -c1-100)"
  fi
  rm -f "$D"/results/.dec*.log
}

main() {
  local quick=${1:-} gpu ref i bad c bf m n f ms s
  gpu=$(nvidia-smi --query-gpu=name --format=csv,noheader | head -1 | tr ' ' '_')
  mkdir -p "$D/results"; csv="$D/results/${gpu}_$(date +%Y%m%d-%H%M%S).csv"
  echo "test,clip,bf,instances,fps,note" > "$csv"

  echo "== system"
  nvidia-smi --query-gpu=name,driver_version,memory.total,power.limit,clocks.max.sm,clocks.max.mem,pcie.link.gen.max,pcie.link.width.max --format=csv
  nvidia-smi | grep -o 'CUDA Version: [0-9.]*'
  grep -m1 'model name' /proc/cpuinfo; nproc
  echo "NVIDIA_DRIVER_CAPABILITIES=${NVIDIA_DRIVER_CAPABILITIES:-unset}"

  echo "== correctness (md5 vs RTX 3090 reference)"
  ref=(4afa05c6b32d 2321f9d24fc4 4142c752f697 a48b911b8a1d); i=0; bad=0
  for c in touchdown_1080p parkjoy_1080i; do
    for bf in 0 2; do
      m=$(run ffmpeg -v error -i "$D/media/$c.mkv" -c:v mpeg2_cuda -b:v 15M -g 15 -bf $bf -f mpeg2video - | md5sum | cut -c1-12)
      [[ $m == "${ref[$i]}" ]] && s=match || { s="MISMATCH($m)"; bad=1; }
      row "md5,$c,$bf,1,,$s"; i=$((i + 1))
    done
  done
  ((bad)) && echo "WARNING: output differs from the 3090; speed numbers below are still measured"

  echo "== encoder only (fps, aggregate over instances)"
  ms="1 2 4 8"; [[ $quick ]] && ms="1 4"
  for c in touchdown_1080p parkjoy_1080i; do
    for bf in 0 2; do
      for m in $ms; do
        n=$((3600 / m)); ((n < 900)) && n=900
        f=$(run m2c_bench "$D/media/$c.mkv" mpeg2_cuda 60 $n $m b=15M bf=$bf 2>&1 |
            sed -nE 's/.*aggregate ([0-9.]+) fps.*/\1/p')
        row "encode,$c,$bf,$m,${f:-FAIL},"
      done
    done
  done

  if run ffmpeg -v error -hwaccel cuda -hwaccel_output_format cuda -i "$MEZZ" -frames:v 10 -f null - 2>/dev/null; then
    frames=$(run ffmpeg -hide_banner -i "$MEZZ" -map 0:v:0 -c copy -f null - 2>&1 |
             grep -oE 'frame= *[0-9]+' | tail -1 | tr -dc 0-9)
    echo "== NVDEC decode only (H.264 1080p, $frames frames, aggregate fps)"
    ms="1 2 4 8 16"; [[ $quick ]] && ms="1 8"
    for m in $ms; do row "nvdec,mezz_h264,,$m,$(dec "$m")"; done
    echo "== end to end NVDEC -> mpeg2_cuda 15M (aggregate fps; /29.97 = realtime channels)"
    ms="1 4 8 16 24 32"; [[ $quick ]] && ms="1 16"
    for m in $ms; do row "e2e,mezz_h264,0,$m,$(dec "$m" -c:v mpeg2_cuda -b:v 15M -g 15 -bf 0)"; done
  else
    row "nvdec,mezz_h264,,,,unavailable (no libnvcuvid / video capability in container)"
  fi
  echo "results: $csv"
}

main "$@"; exit
