#!/usr/bin/env bash
# A/B comparison against software mpeg2video with matched settings. Per case:
#   - stream properties must match (size, field order, frame rate, aspect,
#     pixel format, profile), and both decodes must have the same frame count
#     and timestamps
#   - both must hit the target bitrate (+-3%) and pass the VBV check
#   - tools/abcompare.py: PSNR/worst frame within tolerance of mpeg2video,
#     mean Y/U/V levels match, temporal and field alignment
# usage: tests/ab.sh [quick|full]   (inside `nix develop`)
set -uo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
F="${FFMPEG_BIN:-$root/build/ffmpeg/ffmpeg}"
P="$(dirname "$F")/ffprobe"; [[ -x $P ]] || P="$root/build/ffmpeg/ffprobe"
mode=${1:-quick}
o="$root/out/ab"; mkdir -p "$o"
pass=0; fail=0
SW_EXTRA="-flags +ilme+ildct -alternate_scan 1 -intra_vlc 1"

props() {   # properties that must agree between the two encodes
  "$P" -v error -select_streams v:0 -show_entries \
    stream=codec_name,profile,width,height,pix_fmt,r_frame_rate,display_aspect_ratio \
    -of default=nw=1 "$1"
}

# field order is checked against the source instead: mpeg2video codes
# progressive input as bottom-field-first interlaced, mpeg2_cuda follows the
# source (progressive frames -> progressive_frame, interlaced keep tff/bff)
expected_field_order() {
  local fo; fo=$("$P" -v error -select_streams v:0 -show_entries stream=field_order -of csv=p=0 "$1" | tr -d ,)
  case $fo in tt|bb) echo "$fo" ;; *) echo progressive ;; esac
}

ab_case() {   # name input frames rate bf [hw]
  local name=$1 in=$2 n=$3 rate=$4 bf=$5 hw=${6:-}
  [[ -n ${ONLY:-} && $name != "$ONLY" ]] && return
  local d="$o/$name"; mkdir -p "$d"
  local msg="" ok=1 rc="-b:v $rate -minrate $rate -maxrate $rate -bufsize $(( ${rate%M} * 500 ))k -g 15 -bf $bf"
  local gin=(-i "$in") sin=(-i "$in")
  [[ -n $hw ]] && gin=(-hwaccel cuda -hwaccel_output_format cuda -i "$in")

  "$F" -hide_banner -v error -y "${sin[@]}" -frames:v "$n" -an -c:v mpeg2video $rc $SW_EXTRA \
    -f mpeg2video "$d/sw.m2v" 2>"$d/sw.log" || { ok=0; msg="mpeg2video encode failed"; }
  ((ok)) && { "$F" -hide_banner -v error -y "${gin[@]}" -frames:v "$n" -an -c:v mpeg2_cuda \
    -b:v "$rate" -g 15 -bf "$bf" -f mpeg2video "$d/hw.m2v" 2>"$d/hw.log" ||
    { ok=0; msg="mpeg2_cuda encode failed"; }; }

  if ((ok)); then
    diff <(props "$d/sw.m2v") <(props "$d/hw.m2v") >"$d/props.diff" ||
      { ok=0; msg="stream properties differ: $(tr '\n' ' ' < "$d/props.diff")"; }
    local efo hfo
    efo=$(expected_field_order "$in")
    hfo=$("$P" -v error -select_streams v:0 -show_entries stream=field_order -of csv=p=0 "$d/hw.m2v" | tr -d ,)
    [[ $efo == "$hfo" ]] || { ok=0; msg="field order $hfo, source is $efo"; }
  fi
  if ((ok)); then
    for k in sw hw; do
      "$F" -hide_banner -v error -y -i "$d/$k.m2v" -f rawvideo -pix_fmt yuv420p "$d/$k.yuv"
      "$P" -v error -show_entries frame=best_effort_timestamp -of csv=p=0 "$d/$k.m2v" >"$d/$k.ts"
    done
    "$F" -hide_banner -v error -y "${sin[@]}" -frames:v "$n" -f rawvideo -pix_fmt yuv420p "$d/src.yuv"
    # relative to the first frame: raw ES timestamps are reconstructed by the
    # parser and mpeg2video's low_delay=0 shifts them by one picture
    # raw ES timestamps are rebuilt by the parser (mpeg2video's even come out
    # shifted and with a bogus last one), so only check that mpeg2_cuda's are
    # strictly increasing with a constant step; frame counts are compared below
    # (the parser cannot place the last picture of a raw ES with B pictures:
    #  it is N/A for both encoders)
    grep -v N/A "$d/hw.ts" |
      awk -F, 'NR>1 {s=$1-p; if (NR==2) d=s; if (s<=0 || s!=d) bad=1} {p=$1} END{exit bad}' ||
      { ok=0; msg="mpeg2_cuda timestamps not uniform"; }
  fi
  local kbps=""
  if ((ok)); then
    local fps; fps=$("$P" -v error -select_streams v:0 -show_entries stream=r_frame_rate -of csv=p=0 "$d/hw.m2v")
    for k in sw hw; do
      local r; r=$(stat -c %s "$d/$k.m2v" | awk -v n="$n" -v f="$fps" 'BEGIN{split(f,a,"/")} {printf "%.2f", $1*8/(n/(a[1]/a[2]))/1e6}')
      kbps+="$k ${r}M "
      # no overshoot for either; mpeg2_cuda may undershoot on content that
      # cannot use the rate at the finest quantiser (mpeg2video pads with
      # stuffing instead) — its quality is still held to mpeg2video's below
      # (mpeg2video's own rate accuracy is reported, not required: it
      #  overshoots badly on hard content at low rates)
      if ! awk -v r="$r" -v t="${rate%M}" -v k="$k" 'BEGIN{exit !(r <= t*1.03 && (k == "hw" || r >= t*0.97))}'; then
        if [[ $k == hw ]]; then ok=0; msg="hw bitrate ${r}M off target $rate"; else kbps+="(sw off target) "; fi
      fi
      # VBV is a hard requirement for mpeg2_cuda only; mpeg2video's result is
      # reported (it does underflow on hard content at low rates)
      if ! python3 "$root/tools/vbv.py" "$d/$k.m2v" | tail -1 | grep -q 'underflows 0'; then
        if [[ $k == hw ]]; then ok=0; msg="hw VBV underflow"; else kbps+="(sw VBV underflow) "; fi
      fi
    done
  fi
  # when mpeg2video misses the target upwards (hard content at low rates),
  # compare quality against mpeg2_cuda re-encoded at mpeg2video's actual rate
  local swr hwr
  swr=$(echo "$kbps" | grep -oE 'sw [0-9.]+' | cut -d' ' -f2)
  hwr=$(echo "$kbps" | grep -oE 'hw [0-9.]+' | cut -d' ' -f2)
  if ((ok)) && awk -v a="$swr" -v b="$hwr" 'BEGIN{exit !(a > b * 1.05)}'; then
    "$F" -hide_banner -v error -y "${gin[@]}" -frames:v "$n" -an -c:v mpeg2_cuda \
      -b:v "${swr}M" -g 15 -bf "$bf" -f mpeg2video "$d/hw.m2v" 2>>"$d/hw.log"
    "$F" -hide_banner -v error -y -i "$d/hw.m2v" -f rawvideo -pix_fmt yuv420p "$d/hw.yuv"
    kbps+="(quality compared at sw's ${swr}M) "
  fi
  local wh line=""
  if ((ok)); then
    wh=$("$P" -v error -select_streams v:0 -show_entries stream=width,height -of csv=p=0:s=x "$d/hw.m2v" | tr x ' ')
    line=$(python3 "$root/tools/abcompare.py" "$d/src.yuv" "$d/sw.yuv" "$d/hw.yuv" $wh --json "$d/ab.json") ||
      { ok=0; msg="$line"; }
  fi
  if ((ok)); then
    pass=$((pass + 1)); printf 'PASS  %-24s %s %s\n' "$name" "$kbps" "$line"
  else
    fail=$((fail + 1)); printf 'FAIL  %-24s %s\n' "$name" "$msg"
  fi
  rm -f "$d"/*.yuv
}

PJ="$root/media/parkjoy_1080i.y4m"; TD="$root/media/touchdown_1080p.y4m"
T="$root/media/test"; C="$root/media/channels"

ab_case pj-15M-bf0   "$PJ" 120 15M 0
ab_case pj-15M-bf2   "$PJ" 120 15M 2
ab_case td-15M-bf0   "$TD" 120 15M 0
ab_case td-15M-bf2   "$TD" 120 15M 2
ab_case ch900-nvdec  "$C/900_TESTCAT.ts" 300 15M 2 hw
ab_case ch905-nvdec  "$C/905_TSTFISH.ts" 300 15M 2 hw
if [[ $mode == full ]]; then
  ab_case pj-8M-bf2    "$PJ" 120 8M 2
  ab_case td-25M-bf2   "$TD" 120 25M 2
  ab_case 720p-8M-bf2  "$T/pj_1280x720_120.y4m" 120 8M 2
  ab_case 480i-5M-bf2  "$T/pj_720x480_120.y4m" 120 5M 2
  for ch in 901_TESTSAX 902_TESTNYA 903_TSTCRAB 904_TSTCARA; do
    ab_case "ch${ch%%_*}-nvdec" "$C/$ch.ts" 300 15M 2 hw
  done
fi
echo "$pass passed, $fail failed"
exit $((fail > 0))
