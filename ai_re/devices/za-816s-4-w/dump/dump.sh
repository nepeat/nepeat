#!/usr/bin/env bash
# Triple-read SPI dump of the PUYA P25Q64SH (8 MB) via CH341A.
# Run as: sudo bash dump.sh   (root needed for libusb claim on macOS)
set -euo pipefail

PROG="ch341a_spi"
OUT="/Users/nep/nocloud/git/nepeat/ai_re/devices/za-816s-4-w/dump"
cd "$OUT"

echo "=== PROBE ==="
flashrom -p "$PROG" 2>&1 | grep -viE 'libusb|entitlement|capture|detach|free software|source code' || true

echo
echo "=== READ x3 ==="
for i in 1 2 3; do
  echo "--- read $i ---"
  flashrom -p "$PROG" -r "p25q64_$i.bin" 2>&1 \
    | grep -viE 'libusb|entitlement|capture|detach' | tail -3
done

echo
echo "=== SHA-256 ==="
shasum -a 256 p25q64_1.bin p25q64_2.bin p25q64_3.bin

echo
if [ "$(shasum -a 256 p25q64_1.bin | cut -d' ' -f1)" = "$(shasum -a 256 p25q64_2.bin | cut -d' ' -f1)" ] \
&& [ "$(shasum -a 256 p25q64_2.bin | cut -d' ' -f1)" = "$(shasum -a 256 p25q64_3.bin | cut -d' ' -f1)" ]; then
  cp p25q64_1.bin p25q64_GOOD.bin
  echo "*** ALL THREE MATCH -> saved p25q64_GOOD.bin ***"
else
  echo "!!! READS DIFFER - clip/contact is flaky, do NOT trust. Reseat clip and retry."
fi
