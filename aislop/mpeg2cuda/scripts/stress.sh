#!/usr/bin/env bash
# Strict decode + bit-exact check over a matrix of rate/feature settings.
set -uo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
fail=0
for clip in parkjoy_1080i touchdown_1080p; do
  for rc in "-qscale:v 2" "-qscale:v 8" "-qscale:v 31" "-b:v 15M" "-b:v 4M"; do
    for feat in "" "-field_me 0" "-field_dct 0 -field_me 0"; do
      r=$("$root/scripts/verify.sh" "$root/media/$clip.y4m" 60 $rc $feat 2>&1 | head -1)
      case "$r" in BITEXACT*) ;; *) fail=1 ;; esac
      printf '%-16s %-14s %-26s %s\n' "$clip" "$rc" "${feat:-default}" "$r"
    done
  done
done
exit $fail
