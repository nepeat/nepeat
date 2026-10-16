#!/usr/bin/env bash
# Create the FFmpeg dev tree: upstream $FFMPEG_TAG with patches/ applied as
# commits on branch mpeg2_cuda. patches/ is the source of truth; edit in
# ffmpeg/, commit there, then run scripts/export_patches.sh.
#   scripts/ffmpeg_tree.sh          clone (if missing) and apply the series
#   scripts/ffmpeg_tree.sh --reset  re-apply onto the tag (drops local commits)
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
tag=${FFMPEG_TAG:-n9.0.2}
t="$root/ffmpeg"
if [[ ! -d $t/.git ]]; then
  git clone --branch "$tag" --depth 1 https://git.ffmpeg.org/ffmpeg.git "$t"
elif [[ ${1:-} != --reset ]]; then
  echo "ffmpeg/ exists; use --reset to rebuild it from patches/" >&2; exit 1
fi
cd "$t"
if [[ -n $(git status --porcelain --untracked-files=no) ]]; then
  echo "ffmpeg/ has uncommitted changes, not touching it" >&2; exit 1
fi
git checkout -q -B mpeg2_cuda "$tag"
git am -q "$root"/patches/*.patch
git log --oneline "$tag"..HEAD
