#!/usr/bin/env bash
# Write the dev tree's commits (tag..mpeg2_cuda) back to patches/. Unnumbered
# subjects, so adding a commit leaves the other patch files unchanged.
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
tag=${FFMPEG_TAG:-n9.0.2}
rm -f "$root"/patches/*.patch
git -C "$root/ffmpeg" format-patch -q --no-numbered "$tag"..mpeg2_cuda -o "$root/patches"
ls "$root/patches"
