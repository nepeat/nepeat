#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# The oracle driver (workstation side): build the tracers and runners, copy
# them to the CS8210, run one SAI script on our SAI and on isai (box-run.sh),
# fetch the traces and diff them (oracle.py).
#   ifcs-oracle/run-oracle.sh SCRIPT.json [both|ours|isai]
# Environment: BOX (root@10.60.2.190), OUT (devices/cs8210-32x/analysis/oracle),
#   PROFILE_OURS / PROFILE_ISAI (SAI profiles on the box; default the
#   benchmark's /run/bench/{native,isai}-error.profile, see sai-qemu.md),
#   NAMES_OURS (local 0.12.41 pyctypes/ifcs_ctypes.py for attribute names).
# Builds go through the memory gate (MEMGATE, if set) with --cores 4.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
airoot=$(cd "$here/../../.." && pwd)               # ai_re/ (moved one level up from sai-teralynx/oracle, H5-1)
script=$(realpath "$1"); which=${2:-both}
BOX=${BOX:-root@10.60.2.190}
OUT=${OUT:-$airoot/devices/cs8210-32x/analysis/oracle}
PROFILE_OURS=${PROFILE_OURS:-/run/bench/native-error.profile}
PROFILE_ISAI=${PROFILE_ISAI:-/run/bench/isai-error.profile}
NAMES_OURS=${NAMES_OURS:-$airoot/devices/cs8210-32x/analysis/sai-teralynx/vendor012/ifcs_ctypes.py}
ssh_() { sudo -n ssh -o BatchMode=yes "$BOX" "$@"; }

build() {
  [ -z "${MEMGATE:-}" ] || bash "$MEMGATE" >&2
  nix build --impure --max-jobs 1 --cores 4 --no-link --print-out-paths --expr "
    let f = builtins.getFlake \"path:$airoot/nix\";
        pkgs = import f.inputs.nixpkgs { system = \"x86_64-linux\"; config.allowUnfree = true; };
        rs = $airoot/../rust/sai/nix;   # our side: rust/sai (outside path:./nix), as tests/nixtargets.py passes it
        sonicSrc = p: (import $airoot/nix/packages/sonic { pkgs = p; }).sources.sairedis;
    in (import $airoot/nix/packages/qemu-ipd {
      inherit pkgs;
      ifcsHeaders = { pkgs }: pkgs.callPackage (rs + \"/ifcs-headers.nix\") { };
      saiTeralynx = { pkgs }: pkgs.callPackage (rs + \"/sai-teralynx-rs.nix\") {
        saiRs = pkgs.callPackage (rs + \"/package.nix\") { saiSrc = sonicSrc pkgs; glueSrc = $airoot/nix/packages/sai-openbcm/src; };
      };
    }).$1"
}
oours=$(build oracle-ours); oisai=$(build oracle-isai); sai=$(build sai-teralynx); runner=$(build runner)
ld=$(nix build --impure --no-link --print-out-paths --expr "
  let f = builtins.getFlake \"path:$airoot/nix\"; pkgs = import f.inputs.nixpkgs { system = \"x86_64-linux\"; };
      c = pkgs.pkgsCross.aarch64-multiplatform;
  in c.symlinkJoin { name = \"oracle-sdk-deps\"; paths = map c.lib.getLib [ c.jansson c.libyaml c.zlib ]; }")
sudo -n nix copy --to "ssh://$BOX" --no-check-sigs "$oours" "$oisai" "$sai" "$runner" "$ld"

name=$(basename "$script" .json)-$(date +%Y%m%d-%H%M%S)
rdir=/run/oracle/$name
ssh_ mkdir -p "$rdir"
sudo -n scp -q "$script" "$here/box-run.sh" "$BOX:$rdir/"
mkdir -p "$OUT/$name"
cp "$script" "$OUT/$name/script.json"
run() {   # kind oracle dep profile
  ssh_ "ORACLE_LD=$ld/lib bash $rdir/box-run.sh $1 $2 $3 $4 $rdir/$(basename "$script") $rdir/$1" || echo "($1 run rc=$?)"
  mkdir -p "$OUT/$name/$1"
  sudo -n scp -q "$BOX:$rdir/$1/trace.txt" "$BOX:$rdir/$1/stdout.txt" "$OUT/$name/$1/" || true
}
case $which in both|ours) run ours "$oours" "$sai" "$PROFILE_OURS" ;; esac
case $which in both|isai) run isai "$oisai" "$runner" "$PROFILE_ISAI" ;; esac
if [ "$which" = both ]; then
  python3 -I "$here/oracle.py" diff "$OUT/$name/ours/trace.txt" "$OUT/$name/isai/trace.txt" \
    --names-ours "$NAMES_OURS" --names-isai "$(nix eval --impure --raw --expr "
      let f = builtins.getFlake \"path:$airoot/nix\"; pkgs = import f.inputs.nixpkgs { system = \"x86_64-linux\"; config.allowUnfree = true; };
      in \"\${(import $airoot/nix/packages/qemu-ipd { inherit pkgs; }).libsai}/share/isai/pyctypes/ifcs_ctypes.py\"")" \
    -o "$OUT/$name/report.txt"
  grep -E '^== |^# totals' "$OUT/$name/report.txt" | head -80
fi
echo "oracle: $OUT/$name"
