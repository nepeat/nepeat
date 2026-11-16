#!/usr/bin/env bash
# Layer 5 round trip: the L4 netlist split into one file per block rebuilds the image. Generic.
#
#   roundtrip5.sh IMAGE DEVICE BLOCKS.json OUTDIR [R4DIR]
#
# BLOCKS.json = {"cells": {cell: "block/path"}, "nets": {net: "block/path"}} (nets may be
# left out: they take their driver cell's block; anything unlisted -> "unannotated").
# R4DIR = an earlier roundtrip4.sh output to reuse (its l1/ and l4/); without it,
# roundtrip4.sh runs first into OUTDIR/r4.
# Steps: xo3l4 split -> OUTDIR/split/ (one JSON per block + _meta.json), xo3l4 merge ->
# OUTDIR/merged.json, `same` against the L4 netlist, tocfg -> pack -> asm -> cmp IMAGE.
# Result: OUTDIR/roundtrip5.txt.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
if [ -z "${TRELLIS:-}" ]; then
  rev=$(python3 -c 'import json,sys;print(json.load(open(sys.argv[1]))["nodes"]["nixpkgs"]["locked"]["rev"])' "$here/../../flake.lock")
  np=github:NixOS/nixpkgs/$rev
  exec nix shell "$np#python3" "$np#trellis" -c bash -c \
    'export TRELLIS=$(dirname $(dirname $(command -v ecpunpack))); export PYTHONPATH=$TRELLIS/lib/trellis; exec "$0" "$@"' "$0" "$@"
fi
img=$(realpath "$1"); dev=$2; blocks=$(realpath "$3"); out=$4
mkdir -p "$out"; out=$(realpath "$out")
if [ $# -ge 5 ]; then r4=$(realpath "$5"); else r4=$out/r4; "$here/roundtrip4.sh" "$img" "$dev" "$r4" >/dev/null; fi
cd "$out"
rm -rf split; mkdir -p l2r
python3 "$here/xo3l4.py" split "$r4/l4/netlist-l4.json" "$blocks" split | tee split.txt
python3 "$here/xo3l4.py" merge split merged.json
python3 "$here/xo3l4.py" same "$r4/l4/netlist-l4.json" merged.json | tee same.txt
python3 "$here/xo3l4.py" tocfg merged.json design.config
cmp "$r4/l3/design.config" design.config
python3 "$here/xo3cfg.py" pack design.config "$dev" l2r/frames.txt
cp "$r4/l1/listing.txt" l2r/listing.txt
python3 "$here/xo3bit.py" asm l2r rebuilt5.bin
{
  echo "# roundtrip5.sh $(date -u +%Y-%m-%dT%H:%MZ): L4 netlist -> split by $(basename "$blocks") -> merge -> L3 -> L2 -> image"
  echo "blocks: $(ls split | grep -vc '^_meta.json$') files; annotation sha256 $(sha256sum "$blocks" | cut -c1-16)"
  echo "merge vs L4 netlist: $(cat same.txt)"
  sha256sum "$img" rebuilt5.bin
  if cmp "$img" rebuilt5.bin; then echo "IMAGE == rebuilt5.bin: BYTE-IDENTICAL ($(stat -c %s rebuilt5.bin) bytes)"; else echo "DIFFERENT"; exit 1; fi
} | tee roundtrip5.txt
