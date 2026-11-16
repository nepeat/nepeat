#!/usr/bin/env bash
# Layer 1..4 round trip for a MachXO2/XO3 configuration image. Generic.
#
#   roundtrip4.sh IMAGE DEVICE OUTDIR        e.g. DEVICE = LCMXO3LF-2100C
#
# IMAGE -> L1 listing + frames (xo3bit disasm) -> L3 Trellis config (xo3cfg unpack)
# -> L4 lossless netlist (xo3l4 build, check) -> config (xo3l4 tocfg) -> frames
# (xo3cfg pack) -> image (xo3bit asm, with the L1 listing for the non-frame records)
# -> cmp with IMAGE.  Writes OUTDIR/{l1,l3,l4,l3r,l2r}/, OUTDIR/rebuilt4.bin and
# OUTDIR/roundtrip4.txt (the result with sha256 of both images).
# Runs inside the pinned nixpkgs of ai_re/flake.lock when TRELLIS is not set.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
if [ -z "${TRELLIS:-}" ]; then
  rev=$(python3 -c 'import json,sys;print(json.load(open(sys.argv[1]))["nodes"]["nixpkgs"]["locked"]["rev"])' "$here/../../flake.lock")
  np=github:NixOS/nixpkgs/$rev
  exec nix shell "$np#python3" "$np#trellis" -c bash -c \
    'export TRELLIS=$(dirname $(dirname $(command -v ecpunpack))); export PYTHONPATH=$TRELLIS/lib/trellis; exec "$0" "$@"' "$0" "$@"
fi
img=$(realpath "$1"); dev=$2; out=$3
mkdir -p "$out"/{l1,l3,l4,l3r,l2r}
cd "$out"
py() { python3 "$@"; }
py "$here/xo3bit.py" disasm "$img" l1
py "$here/xo3cfg.py" unpack l1/frames.txt "$dev" l3/design.config
py "$here/xo3l4.py" build l3/design.config l4/netlist-l4.json
py "$here/xo3l4.py" check l3/design.config l4/netlist-l4.json | tee l4/check.txt
py "$here/xo3l4.py" tocfg l4/netlist-l4.json l3r/design.config
cmp l3/design.config l3r/design.config
py "$here/xo3cfg.py" pack l3r/design.config "$dev" l2r/frames.txt
cmp l1/frames.txt l2r/frames.txt
cp l1/listing.txt l2r/listing.txt
py "$here/xo3bit.py" asm l2r rebuilt4.bin
{
  echo "# roundtrip4.sh $(date -u +%Y-%m-%dT%H:%MZ): IMAGE -> L1 -> L3 -> L4 netlist -> L3 -> L2 -> image"
  cat l4/check.txt
  sha256sum "$img" rebuilt4.bin
  if cmp "$img" rebuilt4.bin; then echo "IMAGE == rebuilt4.bin: BYTE-IDENTICAL ($(stat -c %s rebuilt4.bin) bytes)"; else echo "DIFFERENT"; exit 1; fi
} | tee roundtrip4.txt
