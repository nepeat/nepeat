#!/usr/bin/env bash
# Extract the stock kernel blob (Image.gz + 4 appended DTBs) from the stock
# recovery image, for use as TARGET_PREBUILT_KERNEL.
#
# The blob is NOT committed: it is ~10 MB of Amazon's kernel. Regenerate it
# with this script from your own device dump.
set -euo pipefail
SRC="${1:-../../../../fw-partitions/dump/recovery.img}"
OUT="$(dirname "$0")/prebuilt/Image.gz-dtb"
mkdir -p "$(dirname "$OUT")"
python3 - "$SRC" "$OUT" <<'PY'
import struct, sys, hashlib
src, out = sys.argv[1], sys.argv[2]
d = open(src, 'rb').read()
assert d[:8] == b'ANDROID!', "not an Android boot image"
ks, ka, rs, ra, ss, sa, tags, page, hdrv, osv = struct.unpack('<10I', d[8:48])
blob = d[page:page + ks]
assert blob[:2] == b'\x1f\x8b', "kernel is not gzip — layout changed"
open(out, 'wb').write(blob)
print(f"wrote {out}: {len(blob)} bytes  sha256={hashlib.sha256(blob).hexdigest()}")
PY
