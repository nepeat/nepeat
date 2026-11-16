#!/usr/bin/env bash
# Offline MKA test: two wpa_supplicant -D macsec_cs8210 (PSK CAK) on a veth
# pair named fpp1/fpp16, both driving the shim in a fake phy_server (stub
# DDK, ./fake-*.c). Checks that MKA installs SAs on both slices, rekeys on
# transmit PN exhaustion (hitless: Chain + Switch), and fails closed on exit.
#   run-mka-test.sh WPA_SUPPLICANT CLIENT.py
set -euo pipefail
wpa=$(realpath "$1"); client=$(realpath "$2")
here=$(cd "$(dirname "$0")" && pwd)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
t=$work/t
mkdir -p "$t"
gcc -std=gnu11 -O1 -Wall -fPIC -shared -o "$work/libcredoCmd.so" "$here/fake-vendor.c"
gcc -std=gnu11 -O1 -Wall -o "$work/phy_server" "$here/fake-phy-server.c" -rdynamic -L"$work" -lcredoCmd -Wl,-rpath,"$work"
gcc -std=gnu11 -O2 -Wall -Wextra -Werror -fPIC -shared -DBASEDIR="\"$t/cs8210-macsec\"" \
  -o "$work/cs8210-macsec-shim.so" "$here/../shim/cs8210-macsec-shim.c"
for p in 1 16; do
  cat > "$work/wpa-fpp$p.conf" <<EOF
ap_scan=0
network={
  key_mgmt=NONE
  eapol_flags=0
  macsec_policy=1
  macsec_integ_only=0
  macsec_csindex=1
  macsec_replay_protect=1
  macsec_replay_window=0
  mka_priority=$([ $p = 1 ] && echo 1 || echo 255)
  mka_cak=0123456789abcdef0123456789abcdef
  mka_ckn=6162636465666768696a6b6c6d6e6f70
}
EOF
done
exec unshare -rn bash -euo pipefail -c '
  work=$1; t=$2; client=$3; wpa=$4
  ip link add fpp1 type veth peer name fpp16
  ip link set fpp1 up; ip link set fpp16 up; ip link set lo up
  # the stub takes transmit PN pushes here; real hardware does not (the PN
  # grows with traffic, see macsec.md)
  FAKE_TX_PN_OK=1 LD_PRELOAD=$work/cs8210-macsec-shim.so "$work/phy_server" "$t" 2>"$work/server.txt"
  for _ in $(seq 50); do [ -p "$t/cls_phy_fifo" ] && break; sleep 0.1; done
  export CS8210_SDK_TMP=$t CS8210_SDK_RUN=$t
  c() { python3 "$client" "$@"; }
  "$wpa" -D macsec_cs8210 -i fpp1 -c "$work/wpa-fpp1.conf" -dd > "$work/wpa1.txt" 2>&1 &
  w1=$!
  "$wpa" -D macsec_cs8210 -i fpp16 -c "$work/wpa-fpp16.conf" -dd > "$work/wpa16.txt" 2>&1 &
  w16=$!
  ok=0
  for _ in $(seq 60); do
    sleep 1
    n=$(c status --json | python3 -c "import json,sys; d=json.load(sys.stdin); print(sum(1 for p in d.values() for s in p.get(\"sa\",[]) if s[\"state\"] in (\"active\",\"in_use\")))")
    if [ "$n" -ge 4 ]; then ok=1; break; fi
  done
  c status | grep -E "^port|controlled|  sa |data " || true
  [ $ok = 1 ] || { echo "FAIL: MKA did not install 4 SAs"; tail -40 "$work/wpa1.txt"; exit 1; }
  ans=$(c status --json | python3 -c "import json,sys; d=json.load(sys.stdin); print(sorted({s[\"an\"] for p in d.values() for s in p.get(\"sa\",[]) if s[\"dir\"]==\"tx\"}))")
  echo "tx ANs before: $ans"
  # each member reports its transmit PN as lpn (802.1X-2010 cl. 9); past
  # 0xC0000000 the key server distributes a new SAK
  for s in 1 16; do
    an=$(c status --json | python3 -c "import json,sys; d=json.load(sys.stdin); print([x[\"an\"] for x in d[\"$s\"][\"sa\"] if x[\"dir\"]==\"tx\" and x[\"state\"]==\"active\"][0])")
    c pn --slice $s --dir tx --an "$an" --set 0xC0000010
  done
  rek=0
  for _ in $(seq 30); do
    sleep 1
    now=$(c status --json | python3 -c "import json,sys; d=json.load(sys.stdin); print(sorted({s[\"an\"] for p in d.values() for s in p.get(\"sa\",[]) if s[\"dir\"]==\"tx\" and s[\"state\"]==\"active\"}))")
    if [ "$now" != "$ans" ]; then rek=1; break; fi
  done
  c status | grep -E "  sa " || true
  [ $rek = 1 ] || { echo "FAIL: no rekey after PN exhaustion"; grep -E "exhaustion|SAK" "$work/wpa1.txt" | tail; exit 1; }
  echo "tx ANs after rekey: $now"
  grep -h -E "exhaustion" "$work/wpa1.txt" "$work/wpa16.txt" | head -3
  kill $w1 $w16; wait $w1 $w16 2>/dev/null || true
  c status --json | python3 -c "import json,sys; d=json.load(sys.stdin); bad=[k for k in (\"1\",\"16\") if not (d[k][\"tx_drop\"]==1 and d[k][\"rx_drop\"]==1 and not d[k][\"sa\"])]; print(\"after wpa_supplicant exit: closed, no SAs:\", not bad); sys.exit(1 if bad else 0)" || { echo "FAIL: not closed after exit"; exit 1; }
  if ls "$t"/cs8210-macsec/keys/* >/dev/null 2>&1; then echo "FAIL: key files left"; exit 1; fi
  if grep -qi "0123456789abcdef0123456789abcdef" "$work/server.txt"; then echo "FAIL: CAK in server log"; exit 1; fi
  printf "exit\r" > "$t/cls_phy_fifo"; sleep 0.2
  grep -c "SA_Chain" "$work/server.txt" | sed "s/^/SA_Chain calls: /"
  if grep -q BUG "$work/server.txt"; then grep BUG "$work/server.txt"; echo FAIL; exit 1; fi
  echo PASS
' bash "$work" "$t" "$client" "$wpa"
