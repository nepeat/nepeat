#!/usr/bin/env bash
# Configure the dev-tree FFmpeg build (ffmpeg/ = upstream + patches/, see
# scripts/ffmpeg_tree.sh). Run inside `nix develop`. The flags live in
# scripts/ffmpeg_flags, shared with the flake's ffmpeg package.
# --enable-cuda-nvcc requires --enable-nonfree: the resulting binary is for
# local use only and must not be redistributed.
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
mapfile -t flags < <(grep -v '^\s*$' "$root/scripts/ffmpeg_flags")
mkdir -p "$root/build/ffmpeg"
cd "$root/build/ffmpeg"
"$root/ffmpeg/configure" --prefix="$root/build/install" "${flags[@]}" "$@"
