#!/usr/bin/env bash
# Run ON the AS7772: print LED processor <n>'s data RAM bytes <first>..<last> (decimal or 0x..) as hex,
# read through the SDK diag shell inside syncd (register reads only).
#   led-ram.sh 1 0x80 0x9b
set -euo pipefail
n=${1:?usage: led-ram.sh <0|1> <first> <last>}; a=$(( ${2:?} )); b=$(( ${3:?} ))
out=""
for ((i = a; i <= b; i++)); do
  v=$(openbcm-diag "getreg CMIC_LEDUP${n}_DATA_RAM($i)" | sed -n 's/.*]=\(0x\)\?\([0-9a-f]*\):.*/\2/p')
  out+=$(printf ' %02x' "0x${v:-ee}")
done
printf 'LEDUP%s %#04x..%#04x:%s\n' "$n" "$a" "$b" "$out"
