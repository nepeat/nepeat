#!/usr/bin/env bash
# Full-design simulation of the I2C FPGA (update image) with the PCIe core in the loop.
#
#   simnet.py: layer-4b logical view -> top.v (all user-clock logic, Verilator)
#   simtb.py:  testbench (TRN TLPs in, completions out, I2C device model, bus monitor)
#   1. scenario suite on a family-B channel (ch21 = driver channel 17) and a
#      family-A channel (ch26 = driver channel 1), logs compared line by line
#   2. random register-write co-simulation A vs B (seeds 1..10)
#   3. sideband mapping (0x140/0x160/0x170/0x178 <-> pins), per-port pin table
#
# usage: simrun.sh [RTDIR]    (RTDIR = a roundtrip6.sh output dir; default work/rt6-mtd5)
# needs the ai_re devshell (verilator); runs it through `nix develop` if not on PATH.
set -euo pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
WORK=$HERE/../work
PY="$WORK/pyenv/bin/python3 -I -B"
RT=${1:-$WORK/rt6-mtd5}
S=$WORK/sim
mkdir -p "$S/cosim"
VL="verilator"
command -v verilator >/dev/null || VL="nix develop $HERE/../../../.. --command verilator"
build() { # top tbfile exe mdir
  (cd "$S" && $VL --binary --timing -j 0 -Wno-fatal -Wno-lint -Wno-style --top-module "$1" top.v "$2" -o "$3" --Mdir "$4" > "$4.build.txt" 2>&1)
}
echo "== netlist + testbenches"
$PY "$HERE/simnet.py" verilog "$RT/l4/update.logic.json" "$S/top.v" --map "$S/top.map.json" --sync-reset
$PY "$HERE/simtb.py" "$S/top.v" "$RT/l6/update.l6/ch21.json" "$S/tbB.sv"
$PY "$HERE/simtb.py" "$S/top.v" "$RT/l6/update.l6/ch26.json" "$S/tbA.sv"
sed -i 's/^module tb;/module tbB;/' "$S/tbB.sv"; sed -i 's/^module tb;/module tbA;/' "$S/tbA.sv"
build tbB tbB.sv VtbB obj_B; build tbA tbA.sv VtbA obj_A
scen=(
 "rd_sel0|+sel=0" "rd_sel1|+sel=1" "rd_sel2|+sel=2 +maxpoll=200 +pollgap=2000"
 "rd_len1_off7f|+len=1 +off=7f" "rd_len0|+len=0" "nack_addr|+dev=51" "nack_then_ok|+dev=51 +again=1"
 "w1c|+dev=51 +w1c=2" "wr3|+rd=0 +len=3 +off=20 +wdat=33221100" "wr_nack|+rd=0 +len=2 +dev=51"
 "wr_sel1|+rd=0 +len=4 +sel=1 +wdat=a5a55a5a" "ctx1_rd|+ctx=1" "ctx1_nack|+ctx=1 +dev=51"
 "busclear|+cmdx=20000000 +rd=0 +len=2" "cmdbits|+cmdx=80ff0000 +len=2"
)
norm() { grep -E "wire|status|window|SCL|profile|profsel|poll start reg 0x0|second|rd 0x|final" "$1" | grep -v "MWr\|MRd"; }
same=0; diffn=0
echo "== scenario suite, family A (driver ch 1) vs family B (driver ch 17)"
for s in "${scen[@]}"; do
  n=${s%%|*}; a=${s#*|}
  # shellcheck disable=SC2086
  "$S/obj_B/VtbB" +ch=17 $a +maxpoll=60 > "$S/cosim/B_$n.txt" 2>&1
  # shellcheck disable=SC2086
  "$S/obj_A/VtbA" +ch=1 $a +maxpoll=60 > "$S/cosim/A_$n.txt" 2>&1
  if diff <(norm "$S/cosim/A_$n.txt") <(norm "$S/cosim/B_$n.txt") > "$S/cosim/diff_$n.txt"; then
    echo "SAME      $n  ($(norm "$S/cosim/B_$n.txt" | grep -c wire) bus events)"; same=$((same+1))
  else echo "DIFFERENT $n"; diffn=$((diffn+1)); fi
done
echo "== random co-simulation"
for seed in 1 2 3 4 5 6 7 8 9 10; do
  "$S/obj_B/VtbB" +ch=17 +rnd=$seed +rnd_n=40 > "$S/cosim/Br_$seed.txt" 2>&1
  "$S/obj_A/VtbA" +ch=1 +rnd=$seed +rnd_n=40 > "$S/cosim/Ar_$seed.txt" 2>&1
  if diff <(norm "$S/cosim/Ar_$seed.txt") <(norm "$S/cosim/Br_$seed.txt") > /dev/null; then
    echo "SAME      seed $seed  ($(norm "$S/cosim/Br_$seed.txt" | grep -c wire) bus events, $(norm "$S/cosim/Br_$seed.txt" | grep -c 'rd 0x\|final') reads)"; same=$((same+1))
  else echo "DIFFERENT seed $seed"; diffn=$((diffn+1)); fi
done
echo "co-simulation: identical $same, different $diffn"
echo "== sideband mapping"
"$S/obj_B/VtbB" +sideband=1 +sbwait=2000 > "$S/sideband.txt" 2>&1
$PY "$HERE/sbmap.py" "$S/sideband.txt" "$HERE/mtd5/update.channel-map.txt" "$S/port-pins.txt"
