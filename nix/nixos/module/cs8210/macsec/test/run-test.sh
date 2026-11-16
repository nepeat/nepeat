#!/usr/bin/env bash
# Offline test of the cs8210-macsec shim + client against a fake phy_server
# and a stub DDK (same symbol names and layouts, small behaviour model that
# flags misuse with "BUG"). Runs as root in a user namespace so the shim's
# and client's ownership checks hold. No hardware, no vendor code.
#   run-test.sh CLIENT.py     (needs gcc, python3 with cryptography)
set -euo pipefail
client=$(realpath "$1")
here=$(cd "$(dirname "$0")" && pwd)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
t=$work/t
mkdir -p "$t"
gcc -std=gnu11 -O1 -Wall -fPIC -shared -o "$work/libcredoCmd.so" "$here/fake-vendor.c"
gcc -std=gnu11 -O1 -Wall -o "$work/phy_server" "$here/fake-phy-server.c" -rdynamic -L"$work" -lcredoCmd -Wl,-rpath,"$work"
gcc -std=gnu11 -O2 -Wall -Wextra -Werror -fPIC -shared -DBASEDIR="\"$t/cs8210-macsec\"" \
  -o "$work/cs8210-macsec-shim.so" "$here/../shim/cs8210-macsec-shim.c"
exec unshare -r bash -euo pipefail -c '
  work=$1; t=$2; client=$3; here=$4
  LD_PRELOAD=$work/cs8210-macsec-shim.so "$work/phy_server" "$t" 2>"$work/server.txt"
  for _ in $(seq 50); do [ -p "$t/cls_phy_fifo" ] && break; sleep 0.1; done
  mkdir -p "$t/net/fpp1" "$t/net/fpp16"
  echo 0e:48:c6:7a:55:cf > "$t/net/fpp1/address"
  echo 0e:48:c6:7a:55:de > "$t/net/fpp16/address"
  export CS8210_SDK_TMP=$t CS8210_SDK_RUN=$t CS8210_NET_SYSFS=$t/net
  c() { echo "+ cs8210-macsec $*"; python3 "$client" "$@"; }
  c hello
  c keygen --loop 1:16 --cipher gcm-aes-xpn-256 --out "$t/keys.json"
  c up --config "$t/keys.json"
  c status
  c rekey --config "$t/keys.json" --link 1:16 --drain 0
  c pn --slice 1 --dir tx --an 1 --set 0xC0000000
  python3 "$here/reset_check.py" "$client"
  c metrics | grep -c "^cs8210_macsec" | sed "s/^/metric lines: /"
  c down --slice 1 --slice 16
  c keygen --loop 1:16 --cipher gcm-aes-128 --out "$t/k128.json"
  c up --config "$t/k128.json"
  c status --slice 16 | head -3
  if c up --config "$t/k128.json" 2>&1; then echo "FAIL: second up accepted"; exit 1; fi
  if ls "$t"/cs8210-macsec/keys/* >/dev/null 2>&1; then echo "FAIL: key files left behind"; exit 1; fi
  if grep -rqi -E "$(python3 -c "import json;print(json.load(open(\"$t/k128.json\"))[\"ports\"][\"1\"][\"tx\"][\"sak\"])")" "$work/server.txt" "$t"/cs8210-macsec 2>/dev/null; then echo "FAIL: key in logs"; exit 1; fi
  printf "exit\r" > "$t/cls_phy_fifo"
  sleep 0.2
  echo "--- fake vendor log"; cat "$work/server.txt"
  if grep -q BUG "$work/server.txt"; then echo FAIL; exit 1; fi
  echo PASS
' bash "$work" "$t" "$client" "$here"
