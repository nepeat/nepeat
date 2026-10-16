#!/usr/bin/env bash
# Build the looping test-channel sources listed in media/channels/lineup.tsv:
# raw downloads (media/channels/raw/<ch>_<name>.mkv) -> 1920x1080 29.97p
# H.264 + AC-3 MPEG-TS, capped at one hour (channels loop with -stream_loop).
# Uses the system ffmpeg (needs libdav1d for AV1 sources and h264_nvenc).
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
d="$root/media/channels"
FF=${FF:-/etc/profiles/per-user/erin/bin/ffmpeg}
jobs=${JOBS:-3}

build() {
  local ch=$1 name=$2 in="$d/raw/$1_$2.mkv" out="$d/$1_$2.ts"
  [[ -s "$out" ]] && { echo "skip $ch $name (exists)"; return; }
  "$FF" -hide_banner -v error -nostdin -y -i "$in" -t 3600 -map 0:v:0 -map 0:a:0 \
    -vf "scale=1920:1080:force_original_aspect_ratio=decrease:flags=lanczos,pad=1920:1080:(ow-iw)/2:(oh-ih)/2:black,setsar=1,fps=30000/1001,format=yuv420p" \
    -c:v h264_nvenc -preset p5 -profile:v high -rc vbr -cq 21 -b:v 10M -maxrate 15M -bufsize 20M -g 60 \
    -c:a ac3 -b:a 192k -ar 48000 -ac 2 \
    -metadata service_name="$name" -f mpegts "$out.tmp" && mv "$out.tmp" "$out"
  echo "done $ch $name $(du -h "$out" | cut -f1)"
}
export -f build; export d FF
grep -v '^#' "$d/lineup.tsv" | awk -F'\t' '$4 == "" || $4 == "mezz"' | cut -f1,2 | xargs -P "$jobs" -L1 bash -c 'build "$0" "$1"'
