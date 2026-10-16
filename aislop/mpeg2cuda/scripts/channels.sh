#!/usr/bin/env bash
# Run the test channels from media/channels/lineup.tsv as looping live streams.
#
#   scripts/channels.sh start [dest]   dest: IP of the EQAM GigE port, or "null"
#   scripts/channels.sh stop
#   scripts/channels.sh status
#
# Channel NNN is sent to udp://<dest>:<PORT_BASE + NNN - 900> as an SPTS with
# program number NNN. lineup.tsv's mode column picks the pipeline (mezz,
# raw-mpeg2, raw-h264); ONLY="906 907" limits start/stop to those channels. Env: PORT_BASE (5900), VBITRATE (15M), MUXRATE (16M),
# COPIES (1; >1 duplicates each channel for stress tests, null dest only).
# UDP output is paced at MUXRATE (ffmpeg udp bitrate/burst_bits) so the EQAM
# sees a smooth stream instead of one burst per picture.
set -uo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
F="$root/build/ffmpeg/ffmpeg"
run="$root/out/channels"; mkdir -p "$run"
export LD_LIBRARY_PATH=/run/opengl-driver/lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}
PORT_BASE=${PORT_BASE:-5900}; VBITRATE=${VBITRATE:-15M}; MUXRATE=${MUXRATE:-16M}
COPIES=${COPIES:-1}

start() {
  local dest=${1:-null}
  grep -v '^#' "$root/media/channels/lineup.tsv" | while IFS=$'\t' read -r ch name _ mode source overlay; do
    [[ -n ${ONLY:-} && " $ONLY " != *" $ch "* ]] && continue
    mode=${mode:-mezz}
    local src vopts=() vin=(-hwaccel cuda -hwaccel_output_format cuda) mrate=$MUXRATE ff=$F
    # optional 5th column: raw source basename shared with another channel
    local raw="$root/media/channels/raw/${source:-${ch}_${name}}"
    local h264v="${H264_VAR_BITRATE:-6M}"
    # H.264 test channels carry an on-screen label (channel, name, what the
    # A/B variant changes); drawtext needs the system ffmpeg (libfreetype)
    local label="$run/${ch}_label.txt" font="$root/media/fonts/LiberationSans-Bold.ttf"
    [[ -f $font ]] || font=$(fc-match -f '%{file}' 'Liberation Sans:bold' 2>/dev/null)
    printf '%s-1  %s\n%s' "$ch" "$name" "${overlay:-}" > "$label"
    # inside the 5% overscan-safe area; 720p scaled down to match
    local fs=40 pad=96; [[ $mode == h264-720 ]] && { fs=27; pad=64; }
    local dt="drawtext=fontfile=$font:textfile=$label:fontsize=$fs:fontcolor=white:line_spacing=10:box=1:boxcolor=black@0.65:boxborderw=14:x=$pad:y=$pad"
    local sysff=/etc/profiles/per-user/erin/bin/ffmpeg
    # labelled channels decode on NVDEC into system memory: hwdownload ->
    # drawtext -> hwupload_cuda dies with ENOSYS when -stream_loop wraps
    # (ffmpeg 7.1), and the 1080p source needs no GPU scaling anyway
    local vin_sw=(-hwaccel cuda)
    case $mode in
      mezz)
        # pre-built 1080 H.264 source -> MPEG-2 1080i (PsF for progressive)
        src="$root/media/channels/${ch}_${name}.ts"
        vopts=(-c:v mpeg2_cuda -b:v "$VBITRATE" -g 15) ;;
      raw-mpeg2)
        # original download, everything on the GPU: NVDEC, scale_cuda,
        # 60p -> 59.94p, and mpeg2_cuda weaves frame pairs into 1080i fields
        src=$(ls "$raw".* 2>/dev/null | head -1)
        vopts=(-vf "fps=60000/1001,scale_cuda=1920:1080:interp_algo=lanczos"
               -c:v mpeg2_cuda -field_weave 1 -b:v "$VBITRATE" -g 15) ;;
      raw-h264)
        # cable-style H.264 for the TV test: 1080p29.97 High@4.0 CBR, 1 s
        # GOPs (NVDEC -> drawtext -> NVENC). No B-frames as references and
        # no CBR filler NAL units (older cable/ATSC H.264 decoders handle
        # neither well; the TS mux pads with null packets instead); AUDs are
        # inserted by h264_metadata.
        src=$(ls "$raw".* 2>/dev/null | head -1)
        mrate=${H264_MUXRATE:-10M}
        ff=$sysff; vin=("${vin_sw[@]}")
        vopts=(-vf "fps=30000/1001,scale=1920:1080:flags=lanczos,format=nv12,$dt"
               -c:v h264_nvenc -preset p5 -profile:v high -level 4.0 -rc cbr
               -b:v "${H264_BITRATE:-8M}" -maxrate "${H264_BITRATE:-8M}" -bufsize "${H264_BITRATE:-8M}"
               -g 30 -bf 2 -b_ref_mode disabled -spatial-aq 1 -forced-idr 1 -aud 0
               -bsf:v "filter_units=remove_types=12,h264_metadata=aud=insert") ;;
      # --- temporary H.264 TV-compatibility variants (6M video, 8M mux) ---
      h264-b0|h264-main|h264-720)
        src=$(ls "$raw".* 2>/dev/null | head -1)
        mrate=${H264_VAR_MUXRATE:-8M}
        local prof=high fps=30000/1001 size=1920:1080 gop=30
        [[ $mode == h264-main ]] && prof=main
        [[ $mode == h264-720 ]] && { fps=60000/1001; size=1280:720; gop=60; }
        ff=$sysff; vin=("${vin_sw[@]}")
        vopts=(-vf "fps=$fps,scale=$size:flags=lanczos,format=nv12,$dt"
               -c:v h264_nvenc -preset p5 -profile:v "$prof" -level 4.0 -rc cbr
               -b:v "$h264v" -maxrate "$h264v" -bufsize "$h264v"
               -g "$gop" -bf 0 -refs 1 -spatial-aq 1 -forced-idr 1 -aud 0
               -bsf:v "filter_units=remove_types=12,h264_metadata=aud=insert") ;;
      x264)
        # libx264 on the CPU (system ffmpeg), same structure as 907
        src=$(ls "$raw".* 2>/dev/null | head -1)
        mrate=${H264_VAR_MUXRATE:-8M}
        # NVDEC decode, libx264 (veryfast) on the CPU
        ff=$sysff; vin=("${vin_sw[@]}")
        vopts=(-vf "fps=30000/1001,scale=1920:1080:flags=lanczos,format=yuv420p,$dt"
               -c:v libx264 -preset veryfast -profile:v high -level 4.0
               -b:v "$h264v" -maxrate "$h264v" -bufsize "$h264v"
               -x264-params "nal-hrd=cbr:force-cfr=1:keyint=30:min-keyint=30:scenecut=0:bframes=2:b-pyramid=none:ref=3:aud=1") ;;
      h265)
        # does the EQAM / TV accept HEVC (stream_type 0x24) at all? NVENC Main
        # 1080p29.97 CBR, no B-frames, AUDs, no filler NAL units (type 38)
        src=$(ls "$raw".* 2>/dev/null | head -1)
        mrate=${H264_VAR_MUXRATE:-8M}
        ff=$sysff; vin=("${vin_sw[@]}")
        vopts=(-vf "fps=30000/1001,scale=1920:1080:flags=lanczos,format=nv12,$dt"
               -c:v hevc_nvenc -preset p5 -profile:v main -level 4.1 -tier main -rc cbr
               -b:v "$h264v" -maxrate "$h264v" -bufsize "$h264v"
               -g 30 -bf 0 -forced-idr 1 -aud 1 -bsf:v filter_units=remove_types=38) ;;
      *) echo "unknown mode $mode for $ch"; continue ;;
    esac
    [[ -s "$src" ]] || { echo "missing source for $ch $name"; continue; }
    for c in $(seq 1 "$COPIES"); do
      local out port=$((PORT_BASE + ch - 900)) tag="${ch}_${name}_$c"
      if [[ $dest == null ]]; then out=/dev/null
      else out="udp://$dest:$port?pkt_size=1316&bitrate=$(numfmt --from=si "$mrate")&burst_bits=$((7 * 188 * 8))"; fi
      nohup "$ff" -hide_banner -nostdin -y -loglevel warning -stats \
        -re -readrate_initial_burst 0 -stream_loop -1 "${vin[@]}" -i "$src" \
        -map 0:v:0 -map 0:a:0 "${vopts[@]}" \
        -c:a ac3 -b:a 192k -ar 48000 -ac 2 \
        -f mpegts -muxrate "$mrate" -pcr_period 20 \
        -mpegts_service_id "$ch" -mpegts_pmt_start_pid 0x100 -mpegts_start_pid 0x101 \
        -metadata service_name="$name" -metadata service_provider="mpeg2cuda" \
        "$out" >"$run/$tag.log" 2>&1 &
      echo $! > "$run/$tag.pid"
      echo "started $tag -> $out (pid $!)"
    done
  done
}

stop() {
  for p in "$run"/*.pid; do
    [[ -e $p ]] || continue
    [[ -n ${ONLY:-} && " $ONLY " != *" $(basename "$p" | cut -d_ -f1) "* ]] && continue
    kill "$(cat "$p")" 2>/dev/null && echo "stopped $(basename "$p" .pid)"
    rm -f "$p"
  done
}

status() {
  for p in "$run"/*.pid; do
    [[ -e $p ]] || continue
    local tag; tag=$(basename "$p" .pid)
    if kill -0 "$(cat "$p")" 2>/dev/null; then
      printf '%-22s up    %s\n' "$tag" "$(tr '\r' '\n' < "$run/$tag.log" | grep -aoE 'speed= *[0-9.]+x' | tail -1)"
    else
      printf '%-22s DOWN  %s\n' "$tag" "$(tail -c 200 "$run/$tag.log" | tr '\r\n' '  ')"
    fi
  done
}

case ${1:-} in
  start) start "${2:-null}" ;;
  stop) stop ;;
  status) status ;;
  *) echo "usage: $0 start [dest|null] | stop | status"; exit 1 ;;
esac
