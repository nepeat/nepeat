#!/usr/bin/env bash
# Layers 4 and 5 on top of the layer 1-3 round trip (roundtrip.sh):
#
#   image -> (roundtrip.sh: packets, frames+plan, FASM+unknown; checks itself)
#   FASM --fasmnet build--> netlist (cells, nets, unassigned)       [check: lossless]
#   netlist --xc7bels--> logical view --blocks--> block annotation
#   netlist --split by block--> one file per block --merge--> netlist'  [same]
#   netlist' --tofasm--> FASM'  [cmp with FASM]
#   FASM' + unknown --fasm2frames--> frames' --encode (plan)--> packets' --asm--> image'
#   cmp image image'
#
# usage: roundtrip4.sh [IMAGE [OUTDIR [NAMES]]]   (same arguments as roundtrip.sh)
set -euo pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
WORK=$HERE/../work
PY="$WORK/pyenv/bin/python3 -I -B"
IMG=${1:-$HERE/../../images/spi-2026-11-09/mtd5-i2cfpga.bin}
OUT=${2:-$WORK/rt4-$(basename "$IMG" .bin)}
NAMES=${3:-}

"$HERE/roundtrip.sh" "$IMG" "$OUT" ${NAMES:+"$NAMES"}
names=$(sed -n 's/^image //p' "$OUT/l2/plan.txt" | tr '\n' ' ')
mkdir -p "$OUT/l4" "$OUT/l5" "$OUT/l4r" "$OUT/l3r" "$OUT/l2r4"
for n in $names; do
  echo "== layer 4: $n FASM -> netlist (lossless check)"
  $PY "$HERE/fasmnet.py" check "$OUT/l3/$n.fasm" "$OUT/l4/$n.net.json" 2>/dev/null | tail -4
  $PY "$HERE/fasmnet.py" stats "$OUT/l4/$n.net.json" > "$OUT/l4/$n.stats.txt"
  echo "== layer 4b/5: $n logical view + blocks"
  $PY "$HERE/xc7bels.py" logic "$OUT/l4/$n.net.json" "$OUT/l4/$n.logic.json" > "$OUT/l4/$n.logic.txt"
  $PY "$HERE/blocks.py" analyse "$OUT/l4/$n.net.json" "$OUT/l4/$n.logic.json" "$OUT/l5" --name "$n" > "$OUT/l5/$n.summary.txt"
  echo "== split by block -> merge -> FASM: $n"
  rm -rf "$OUT/l5/$n.split"
  $PY "$HERE/fasmnet.py" split "$OUT/l4/$n.net.json" "$OUT/l5/$n.blocks.json" "$OUT/l5/$n.split"
  $PY "$HERE/fasmnet.py" merge "$OUT/l5/$n.split" "$OUT/l4r/$n.net.json"
  $PY "$HERE/fasmnet.py" same "$OUT/l4/$n.net.json" "$OUT/l4r/$n.net.json"
  $PY "$HERE/fasmnet.py" tofasm "$OUT/l4r/$n.net.json" "$OUT/l3r/$n.fasm" 2>/dev/null
  cmp "$OUT/l3/$n.fasm" "$OUT/l3r/$n.fasm" && echo "   $n: FASM(per-block netlist) == FASM"
  $PY "$HERE/fasmconv.py" fasm2frames "$OUT/l3r/$n.fasm" "$OUT/l3/$n.unknown" "$OUT/l2r4/$n.frames"
  cmp "$OUT/l2/$n.frames" "$OUT/l2r4/$n.frames" && echo "   $n: frames(per-block netlist) == frames"
done
echo "== rebuild image from the per-block netlists"
cp "$OUT/l2/plan.txt" "$OUT/l2r4/"
[[ -f $OUT/l2/extra.frames ]] && cp "$OUT/l2/extra.frames" "$OUT/l2r4/"
rm -rf "$OUT/l1r4"
$PY "$HERE/xc7frames.py" encode "$OUT/l2r4" "$OUT/l1r4"
$PY "$HERE/bitasm.py" asm "$OUT/l1r4" "$OUT/rebuilt4.bin"
if cmp "$IMG" "$OUT/rebuilt4.bin"; then
  echo "BYTE-IDENTICAL (layer 4/5): $IMG == $OUT/rebuilt4.bin ($(stat -c %s "$IMG") bytes)"
else
  echo "MISMATCH"; exit 1
fi
