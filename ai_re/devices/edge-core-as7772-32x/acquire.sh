#!/usr/bin/env bash
# Read-only acquisition from ONIE over the mgmt net. Source sha256 (on device), transfer sha256
# and saved-file reread sha256 must agree before .partial is renamed. Nothing is written to the device.
# The first SSD run (2026-11-21) was finalized by hand without the on-device hash; see PROGRESS.
# usage: acquire.sh [small|sda|all]   (ONIE at $ONIE_IP, default 10.60.2.23)
set -euo pipefail
cd "$(dirname "$0")/dumps"
IP=${ONIE_IP:-10.60.2.23}
SSH=(ssh -o ConnectTimeout=8 -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o LogLevel=ERROR
     -o HostKeyAlgorithms=+ssh-rsa -o PubkeyAcceptedAlgorithms=+ssh-rsa -o ServerAliveInterval=30 root@$IP)
rec() { printf '{"source":"%s","file":"%s","bytes":%s,"sha256":"%s"%s,"verified":true,"date":"%s"}\n' "$@" "$(date -Is)" >> manifest.jsonl; }
small() { # source file
  local src=$1 f=$2
  [ -e "$f" ] && { echo "skip $f (exists)"; return; }
  local want got reread
  want=$("${SSH[@]}" "sha256sum $src" | cut -d' ' -f1)
  "${SSH[@]}" "cat $src" > "$f.partial"
  got=$(sha256sum "$f.partial" | cut -d' ' -f1)
  reread=$("${SSH[@]}" "sha256sum $src" | cut -d' ' -f1)
  [ "$want" = "$got" ] && [ "$got" = "$reread" ] || { echo "FAILED $f: src $want xfer $got reread $reread"; return 1; }
  mv "$f.partial" "$f"; echo "$got  $f" >> SHA256SUMS
  rec "$src" "$f" "$(stat -c %s "$f")" "$got" ""
  echo "ok $f $got"
}
sda() {
  local f=sda-64GB.img.zst size=64023257088 port=19100
  [ -e "$f" ] && { echo "skip $f"; return; }
  "${SSH[@]}" "nc -l -p $port < /dev/sda" & local bg=$!
  sleep 2
  head -c $size < /dev/tcp/$IP/$port | tee >(sha256sum | cut -d' ' -f1 > sda.xfer.sha) | nice zstd -T6 -12 --long=27 -q -o "$f.partial"
  kill $bg 2>/dev/null || true; "${SSH[@]}" "killall nc" 2>/dev/null || true
  sleep 1
  local got want reread
  got=$(cat sda.xfer.sha)
  echo "transfer done $got; hashing source on device (slow)"
  want=$("${SSH[@]}" "sha256sum /dev/sda" | cut -d' ' -f1)
  reread=$(zstd -dc --long=27 "$f.partial" | sha256sum | cut -d' ' -f1)
  [ "$want" = "$got" ] && [ "$got" = "$reread" ] || { echo "FAILED $f: src $want xfer $got reread $reread"; return 1; }
  mv "$f.partial" "$f"; rm sda.xfer.sha
  sha256sum "$f" >> SHA256SUMS; echo "$got  sda-64GB.img" >> RAW-SHA256SUMS
  rec /dev/sda "$f" "$(stat -c %s "$f")" "$(sha256sum "$f" | cut -d' ' -f1)" ",\"raw_bytes\":$size,\"raw_sha256\":\"$got\""
  echo "ok $f raw $got"
}
case "${1:-all}" in
  small|all)
    for i in 0 1 2 3 4 5 6 7; do
      n=mtd$i.bin; [ $i = 6 ] && n=mtd6-uboot-env-private.bin
      small /dev/mtd$i $n
    done
    small /sys/bus/i2c/devices/0-0050/eeprom board-eeprom-24c256.bin
    if [ ! -e nor-full-128MiB-private.bin ]; then
      cat mtd0.bin mtd1.bin mtd2.bin mtd3.bin mtd4.bin mtd5.bin mtd6-uboot-env-private.bin mtd7.bin > nor-full-128MiB-private.bin
      sha256sum nor-full-128MiB-private.bin >> SHA256SUMS
    fi ;;&
  sda|all) sda ;;
esac
