#!/usr/bin/env bash
# CPU (mpeg2video) side quest, see README.md. Run from anywhere, inside
# `nix develop` for `setup`.
#
#   run.sh setup               worktree of upstream n9.0.2 + the patches here,
#                              minimal static build in build/cpu, bench binary
#                              (first time only; rm -rf build/cpu and
#                              git worktree prune to redo)
#   run.sh bench N M [opts]    encoder-only fps, live-channel settings, M instances
#                              (IN=clip.y4m to change the clip)
#   run.sh sweep label "opts"  1-thread fps + PSNR/SSIM (150 frames) on
#                              touchdown 1080p and park_joy 1080i
#
# Settings match the live channels: 15M CBR, GOP 15, +ilme+ildct, alternate
# scan, intra VLC. Don't combine `nice` with `taskset` for whole-box runs: the
# scheduler stacks the low-weight threads and halves throughput.
set -euo pipefail
root="$(cd "$(dirname "$0")/../.." && pwd)"
here="$root/sidequest/cpu"; b="$root/build/cpu"
B="$b/m2c_bench"; F="$b/obj/ffmpeg"
RC=${RC:-"b=15M minrate=15M maxrate=15M bufsize=7.5M"}
COMMON="g=15 flags=+ilme+ildct alternate_scan=1 intra_vlc=1 $RC"

setup() {
  git -C "$root/ffmpeg" worktree add -f -B cpu_sidequest "$b/src" n9.0.2
  git -C "$b/src" am "$here"/*.patch
  mkdir -p "$b/obj"; cd "$b/obj"
  ../src/configure --disable-everything --disable-doc --disable-ffplay --disable-ffprobe \
    --disable-autodetect --enable-pthreads --enable-gpl --extra-cflags=-g \
    --enable-encoder=mpeg2video,wrapped_avframe,rawvideo \
    --enable-decoder=mpeg2video,rawvideo,wrapped_avframe \
    --enable-demuxer=yuv4mpegpipe,mpegvideo --enable-muxer=mpeg2video,null,rawvideo \
    --enable-parser=mpegvideo --enable-protocol=file,pipe \
    --enable-filter=psnr,ssim,null,format,scale,trim --enable-swscale
  make -j16 ffmpeg
  gcc -O2 -g -o "$B" "$root/tools/m2c_bench.c" -I"$b/src" -I"$b/obj" \
    "$b/obj/libavformat/libavformat.a" "$b/obj/libavcodec/libavcodec.a" \
    "$b/obj/libswscale/libswscale.a" "$b/obj/libavutil/libavutil.a" -lpthread -lm
}

bench() {
  local n=$1 m=$2; shift 2
  "$B" "${IN:-$root/media/touchdown_1080p.y4m}" mpeg2video 60 "$n" "$m" $COMMON "$@" 2>&1 |
    sed -nE 's/.*aggregate ([0-9.]+) fps, slowest instance ([0-9.]+) fps, avg ([0-9.]+) Mbps.*/\1 fps (slowest \2) \3 Mbps/p'
}

sweep() {
  local lab=$1 o=$2 line=$1 out
  out=$(mktemp --suffix=.m2v)
  for clip in touchdown_1080p parkjoy_1080i; do
    local fps cli=()
    fps=$(IN="$root/media/$clip.y4m" bench 200 1 threads=1 $o)
    for kv in $COMMON threads=1 $o; do cli+=(-"${kv%%=*}" "${kv#*=}"); done
    "$F" -hide_banner -v error -y -i "$root/media/$clip.y4m" -frames:v 150 \
      -c:v mpeg2video "${cli[@]}" -f mpeg2video "$out" 2>/dev/null
    line+=" | $clip: $fps, $("$root/scripts/psnr.sh" "$out" "$root/media/$clip.y4m" 150 both)"
  done
  rm -f "$out"; echo "$line"
}

case ${1:-} in
  setup) setup ;;
  bench) shift; bench "$@" ;;
  sweep) shift; sweep "$@" ;;
  *) sed -n 2,14p "$0"; exit 1 ;;
esac
