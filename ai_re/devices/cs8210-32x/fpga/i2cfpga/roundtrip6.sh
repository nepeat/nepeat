#!/usr/bin/env bash
# Layer 6 (module templates + instances) on top of the layer 4/5 round trip:
#
#   image -> roundtrip4.sh (layers 1-5, checks itself)        [skipped with REUSE=1 if OUTDIR has l4/l5]
#   netlist + logical view + blocks --modules extract--> l6/<n>.l6/
#        template.json (module i2c_channel, families A/B), chNN.json (placement,
#        bindings, packing, deviations, routing), remainder.json, meta.json
#   l6 --modules rebuild--> netlist'   [same as the layer-4 netlist]
#   netlist' --tofasm--> FASM'  [cmp]  --fasm2frames--> frames' [cmp] --encode/asm--> image'
#   cmp image image'
#
# usage: roundtrip6.sh [IMAGE [OUTDIR [NAMES]]]      (REUSE=1: reuse OUTDIR/l4, l5 from roundtrip4.sh)
set -euo pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
WORK=$HERE/../work
PY="$WORK/pyenv/bin/python3 -I -B"
IMG=${1:-$HERE/../../images/spi-2026-11-09/mtd5-i2cfpga.bin}
OUT=${2:-$WORK/rt4-$(basename "$IMG" .bin)}
NAMES=${3:-}

if [[ ${REUSE:-0} == 1 && -d $OUT/l4 && -d $OUT/l5 ]]; then
  echo "== reusing layers 1-5 in $OUT (REUSE=1)"
else
  "$HERE/roundtrip4.sh" "$IMG" "$OUT" ${NAMES:+"$NAMES"}
fi
names=$(sed -n 's/^image //p' "$OUT/l2/plan.txt" | tr '\n' ' ')
mkdir -p "$OUT/l6" "$OUT/l6r" "$OUT/l3r6" "$OUT/l2r6"
for n in $names; do
  echo "== layer 6: $n netlist -> module template + instances"
  rm -rf "$OUT/l6/$n.l6"
  $PY "$HERE/modules.py" extract "$OUT/l4/$n.net.json" "$OUT/l4/$n.logic.json" "$OUT/l5/$n.blocks.json" \
      "$OUT/l6" --name "$n" > "$OUT/l6/$n.extract.txt"
  sed -n '1,/^ch  fam/p' "$OUT/l6/$n.modules.txt" | grep -E 'template [AB]|generated|deviation|remainder' | cut -c1-160
  echo "== layer 6: $n template + instances + remainder -> netlist -> FASM -> frames"
  $PY "$HERE/modules.py" rebuild "$OUT/l6/$n.l6" "$OUT/l6r/$n.net.json"
  $PY "$HERE/fasmnet.py" same "$OUT/l4/$n.net.json" "$OUT/l6r/$n.net.json"
  $PY "$HERE/fasmnet.py" tofasm "$OUT/l6r/$n.net.json" "$OUT/l3r6/$n.fasm" 2>/dev/null
  cmp "$OUT/l3/$n.fasm" "$OUT/l3r6/$n.fasm" && echo "   $n: FASM(layer 6) == FASM"
  $PY "$HERE/fasmconv.py" fasm2frames "$OUT/l3r6/$n.fasm" "$OUT/l3/$n.unknown" "$OUT/l2r6/$n.frames"
  cmp "$OUT/l2/$n.frames" "$OUT/l2r6/$n.frames" && echo "   $n: frames(layer 6) == frames"
done
echo "== rebuild image from the layer-6 representation"
cp "$OUT/l2/plan.txt" "$OUT/l2r6/"
[[ -f $OUT/l2/extra.frames ]] && cp "$OUT/l2/extra.frames" "$OUT/l2r6/"
rm -rf "$OUT/l1r6"
$PY "$HERE/xc7frames.py" encode "$OUT/l2r6" "$OUT/l1r6"
$PY "$HERE/bitasm.py" asm "$OUT/l1r6" "$OUT/rebuilt6.bin"
if cmp "$IMG" "$OUT/rebuilt6.bin"; then
  echo "BYTE-IDENTICAL (layer 6): $IMG == $OUT/rebuilt6.bin ($(stat -c %s "$IMG") bytes)"
else
  echo "MISMATCH"; exit 1
fi
