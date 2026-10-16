#!/usr/bin/env bash
# mpeg2_cuda conformance tests. Every case:
#   1. encodes with -recon_file
#   2. decodes with FFmpeg (-err_detect explode -xerror -idct simple) and
#      requires the reconstruction to be bit-exact
#   3. runs tools/m2vcheck.py (independent syntax/semantics checker)
#   4. bitrate modes: tools/vbv.py must report no underflow
#   5. "x" cases: cross-decodes with libmpeg2 and compares to FFmpeg's decode
# usage: tests/run.sh [quick|full]   (run inside `nix develop`)
set -uo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
F="${FFMPEG_BIN:-$root/build/ffmpeg/ffmpeg}"
mode=${1:-quick}
o="$root/out/tests"; mkdir -p "$o"
pass=0; fail=0

run_case() {   # name input frames flags(x=xdec) encoder-args...
  local name=$1 in=$2 n=$3 flags=$4; shift 4
  local d="$o/$name"; mkdir -p "$d"
  local msg="" ok=1
  if ! "$F" -hide_banner -v error -y -i "$in" -frames:v "$n" -c:v mpeg2_cuda \
       -recon_file "$d/rec.yuv" "$@" -f mpeg2video "$d/enc.m2v" 2>"$d/enc.log"; then
    ok=0; msg="encode failed"
  fi
  if ((ok)) && ! "$F" -hide_banner -v error -err_detect explode -xerror -idct simple \
       -i "$d/enc.m2v" -f rawvideo -pix_fmt yuv420p -y "$d/dec.yuv" 2>"$d/dec.log"; then
    ok=0; msg="ffmpeg strict decode failed: $(head -c 120 "$d/dec.log")"
  fi
  if ((ok)) && ! cmp -s "$d/rec.yuv" "$d/dec.yuv"; then
    ok=0; msg="recon mismatch"
  fi
  if ((ok)) && ! python3 "$root/tools/m2vcheck.py" "$d/enc.m2v" >"$d/check.log" 2>&1; then
    ok=0; msg="m2vcheck: $(grep -m1 ERROR "$d/check.log")"
  fi
  if ((ok)) && [[ " $* " == *" -b:v "* ]]; then
    local v; v=$(python3 "$root/tools/vbv.py" "$d/enc.m2v" | tail -1)
    [[ "$v" == *"underflows 0"* ]] || { ok=0; msg="VBV: $v"; }
  fi
  if ((ok)) && [[ "$flags" == *x* ]]; then
    local wh; wh=$(head -c 64 "$in" | grep -aoE 'W[0-9]+ H[0-9]+' | tr -d 'WH')
    python3 "$root/tools/xdec.py" "$d/enc.m2v" "$d/dec.yuv" $wh >"$d/xdec.log" 2>&1 ||
      { ok=0; msg="libmpeg2: $(cat "$d/xdec.log")"; }
  fi
  if ((ok)); then
    pass=$((pass + 1)); printf 'PASS  %-34s %s\n' "$name" "$(stat -c %s "$d/enc.m2v") bytes"
    rm -f "$d/rec.yuv" "$d/dec.yuv"
  else
    fail=$((fail + 1)); printf 'FAIL  %-34s %s\n' "$name" "$msg"
  fi
}

memcheck_case() {   # name lavfi-or-file frames encoder-args...
  local name=$1 in=$2 n=$3; shift 3
  local d="$o/$name"; mkdir -p "$d"
  local src=(-i "$in"); [[ $in == lavfi:* ]] && src=(-f lavfi -i "${in#lavfi:}")
  if compute-sanitizer --tool memcheck --error-exitcode 77 --log-file "$d/memcheck.log" \
       "$F" -v error "${src[@]}" -frames:v "$n" -c:v mpeg2_cuda "$@" -y -f mpeg2video "$d/enc.m2v" \
       >/dev/null 2>"$d/enc.log" && grep -q 'ERROR SUMMARY: 0 errors' "$d/memcheck.log"; then
    pass=$((pass + 1)); printf 'PASS  %-34s memcheck clean\n' "$name"
  else
    fail=$((fail + 1)); printf 'FAIL  %-34s %s\n' "$name" "$(grep -m1 -E 'Invalid|ERROR SUMMARY' "$d/memcheck.log")"
  fi
}

PJ="$root/media/parkjoy_1080i.y4m"; TD="$root/media/touchdown_1080p.y4m"
T="$root/media/test"

# quick: the configurations that matter for the EQAM target
run_case pj-15M-default        "$PJ" 45 x -b:v 15M
run_case td-15M-default        "$TD" 45 x -b:v 15M
run_case pj-q2                 "$PJ" 20 "" -qscale:v 2
run_case pj-q31                "$PJ" 30 "" -qscale:v 31
run_case pj-4M-starved         "$PJ" 45 "" -b:v 4M
run_case 720p-8M               "$T/pj_1280x720.y4m" 30 x -b:v 8M
run_case 480i-5M               "$T/pj_720x480.y4m" 30 x -b:v 5M
run_case pj-15M-bf0            "$PJ" 45 x -b:v 15M -bf 0
run_case td-15M-bf3-odd        "$TD" 47 x -b:v 15M -bf 3
# memory safety (compute-sanitizer): edges, bottom-field half-pel, no row slack
memcheck_case mc-tiny-fieldme   "lavfi:testsrc2=size=256x64:rate=30000/1001" 3 -q:v 8
memcheck_case mc-2048-scroll    "lavfi:testsrc2=size=2048x64:rate=30000/1001,scroll=v=-0.03" 4 -q:v 6

if [[ $mode == full ]]; then
  for clip in PJ TD; do
    for rc in "-qscale:v 2" "-qscale:v 8" "-qscale:v 31" "-b:v 15M" "-b:v 4M"; do
      for feat in "" "-field_me 0" "-field_dct 0 -field_me 0" "-g 1" "-g 30"; do
        # all-intra 1080 cannot fit 4 Mbps even DC-only: VBV failure is expected
        [[ "$rc $feat" == "-b:v 4M -g 1" ]] && continue
        tag=$(echo "$clip $rc $feat" | tr -cs 'A-Za-z0-9' '-'); tag=${tag%-}
        eval in=\$$clip
        run_case "$tag" "$in" 45 "" $rc $feat
      done
    done
  done
  for sz in 1280x720 720x480 720x576 352x288 1440x1080; do
    for rc in "-qscale:v 4" "-b:v 6M"; do
      run_case "$sz-$(echo $rc | tr -cs 'A-Za-z0-9' '-')" "$T/pj_$sz.y4m" 30 x $rc
    done
  done
  memcheck_case mc-pj-1080       "$PJ" 3 -b:v 15M
  memcheck_case mc-352-scroll     "lavfi:testsrc2=size=352x288:rate=30000/1001,scroll=v=0.02:h=0.01" 6 -q:v 4
  for bf in 0 1 3; do
    for rc in "-qscale:v 2" "-qscale:v 31" "-b:v 4M"; do
      for clip in PJ TD; do
        eval in=\$$clip
        run_case "$clip-bf$bf-$(echo $rc | tr -cs 'A-Za-z0-9' '-')" "$in" 37 x -bf $bf $rc
      done
    done
  done
  run_case pj-bf2-fieldme0       "$PJ" 37 "" -b:v 15M -field_me 0
  run_case pj-bf2-g1             "$PJ" 20 "" -qscale:v 6 -g 1
  run_case pj-bf2-g4             "$PJ" 37 "" -qscale:v 6 -g 4
  run_case pj-nonlinear-q        "$PJ" 30 x -b:v 15M -nonlinear_q 1
  run_case pj-me_range-1         "$PJ" 30 "" -b:v 15M -me_range 1
  run_case pj-me_range-15        "$PJ" 30 "" -b:v 15M -me_range 15
fi

echo "$pass passed, $fail failed"
exit $((fail > 0))
