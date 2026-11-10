#!/usr/bin/env bash
# End-to-end lossless round trip of an i2cfpga configuration image:
#
#   image --bitasm dis--> listing+blobs --xc7frames decode--> plan + frames
#         --fasmconv frames2fasm--> FASM + unknown bits
#         --fasmconv fasm2frames--> frames' --xc7frames encode (plan)-->
#         listing'+blobs' --bitasm asm--> image'          cmp image image'
#
# usage: roundtrip.sh [IMAGE [OUTDIR [NAMES]]]
#   IMAGE  default: ../../images/spi-2026-11-09/mtd5-i2cfpga.bin
#   OUTDIR default: ../work/rt-<basename of IMAGE>
#   NAMES  comma list naming the frame-bearing streams in order
#          (default golden,update for the flash dump; image0,.. otherwise)
# Needs ../work/pyenv (python + textx/intervaltree/simplejson/pyyaml/numpy)
# and ../work/src/{prjxray,prjxray-db,fasm}; see LAYER2.md for setup.
set -euo pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
WORK=$HERE/../work
PY="$WORK/pyenv/bin/python3 -I -B"
IMG=${1:-$HERE/../../images/spi-2026-11-09/mtd5-i2cfpga.bin}
OUT=${2:-$WORK/rt-$(basename "$IMG" .bin)}
NAMES=${3:-}
if [[ -z $NAMES && $(basename "$IMG") == mtd5-i2cfpga.bin ]]; then NAMES=golden,update; fi

rm -rf "$OUT"; mkdir -p "$OUT"
echo "== layer 1: packets ($IMG)"
$PY "$HERE/bitasm.py" dis "$IMG" "$OUT/l1"
echo "== layer 2: frames + packing plan"
$PY "$HERE/xc7frames.py" decode "$OUT/l1" "$OUT/l2" ${NAMES:+--names $NAMES}
names=$(sed -n 's/^image //p' "$OUT/l2/plan.txt" | tr '\n' ' ')
echo "== layer 3: FASM + unknown bits, and back (images: $names)"
mkdir -p "$OUT/l3" "$OUT/l2r"
pids=()
for n in $names; do
  ( $PY "$HERE/fasmconv.py" frames2fasm "$OUT/l2/$n.frames" "$OUT/l3/$n.fasm" "$OUT/l3/$n.unknown" > "$OUT/l3/$n.stats.txt"
    $PY "$HERE/fasmconv.py" fasm2frames "$OUT/l3/$n.fasm" "$OUT/l3/$n.unknown" "$OUT/l2r/$n.frames" ) &
  pids+=($!)
done
for p in "${pids[@]}"; do wait "$p"; done
for n in $names; do
  echo "   $n: $(cat "$OUT/l3/$n.stats.txt")"
  cmp -s "$OUT/l2/$n.frames" "$OUT/l2r/$n.frames" && echo "   $n: frames(FASM+unknown) == frames" \
    || { echo "   $n: FRAMES DIFFER"; exit 1; }
done
echo "== rebuild: frames' + plan -> packets -> image"
cp "$OUT/l2/plan.txt" "$OUT/l2r/"
[[ -f $OUT/l2/extra.frames ]] && cp "$OUT/l2/extra.frames" "$OUT/l2r/"
$PY "$HERE/xc7frames.py" encode "$OUT/l2r" "$OUT/l1r"
$PY "$HERE/bitasm.py" asm "$OUT/l1r" "$OUT/rebuilt.bin"
if cmp "$IMG" "$OUT/rebuilt.bin"; then
  echo "BYTE-IDENTICAL: $IMG == $OUT/rebuilt.bin ($(stat -c %s "$IMG") bytes)"
else
  echo "MISMATCH"; exit 1
fi
