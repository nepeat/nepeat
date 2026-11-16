#!/usr/bin/env bash
# Layer 6 round trip: L4 netlist -> templates + instances + remainder -> L4 netlist -> image. Generic.
#
#   roundtrip6.sh IMAGE DEVICE OUTDIR [R4DIR] [-- extra xo3l6.py extract options]
#
# R4DIR = an earlier roundtrip4.sh output to reuse (its l1/, l3/ and l4/); without it,
# roundtrip4.sh runs first into OUTDIR/r4.
# Steps: xo3l6 extract -> OUTDIR/{template.json,inst/,remainder.json,report.txt,instances.txt},
# xo3l6 rebuild -> OUTDIR/rebuilt-l4.json, `xo3l4.py same` against the L4 netlist, tocfg ->
# cmp L3 config -> pack -> asm -> cmp IMAGE.  Result: OUTDIR/roundtrip6.txt.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
if [ -z "${TRELLIS:-}" ]; then
  rev=$(python3 -c 'import json,sys;print(json.load(open(sys.argv[1]))["nodes"]["nixpkgs"]["locked"]["rev"])' "$here/../../flake.lock")
  np=github:NixOS/nixpkgs/$rev
  exec nix shell "$np#python3" "$np#trellis" -c bash -c \
    'export TRELLIS=$(dirname $(dirname $(command -v ecpunpack))); export PYTHONPATH=$TRELLIS/lib/trellis; exec "$0" "$@"' "$0" "$@"
fi
img=$(realpath "$1"); dev=$2; out=$3; shift 3
r4=""
if [ $# -ge 1 ] && [ "$1" != "--" ]; then r4=$(realpath "$1"); shift; fi
[ $# -ge 1 ] && [ "$1" = "--" ] && shift
mkdir -p "$out"; out=$(realpath "$out")
if [ -z "$r4" ]; then r4=$out/r4; "$here/roundtrip4.sh" "$img" "$dev" "$r4" >/dev/null; fi
cd "$out"
rm -rf l2r; mkdir -p l2r
python3 "$here/xo3l6.py" extract "$r4/l4/netlist-l4.json" . "$@" | tee extract.txt
python3 "$here/xo3l6.py" rebuild . rebuilt-l4.json
python3 "$here/xo3l4.py" same "$r4/l4/netlist-l4.json" rebuilt-l4.json | tee same.txt
python3 "$here/xo3l4.py" tocfg rebuilt-l4.json design.config
cmp "$r4/l3/design.config" design.config
python3 "$here/xo3cfg.py" pack design.config "$dev" l2r/frames.txt
cp "$r4/l1/listing.txt" l2r/listing.txt
python3 "$here/xo3bit.py" asm l2r rebuilt6.bin
{
  echo "# roundtrip6.sh $(date -u +%Y-%m-%dT%H:%MZ): L4 netlist -> L6 (templates + instances + remainder) -> L4 -> L3 -> L2 -> image"
  cat extract.txt
  echo "templates: $(python3 -c 'import json;t=json.load(open("template.json"))["families"];print(", ".join("%s %d elements x %d" % (k, len(v["elements"]), len(v["members"])) for k, v in t.items()))')"
  echo "rebuilt L4 netlist vs L4: $(cat same.txt)"
  echo "L3 config from rebuilt netlist: identical (cmp)"
  sha256sum "$img" rebuilt6.bin
  if cmp "$img" rebuilt6.bin; then echo "IMAGE == rebuilt6.bin: BYTE-IDENTICAL ($(stat -c %s rebuilt6.bin) bytes)"; else echo "DIFFERENT"; exit 1; fi
} | tee roundtrip6.txt
