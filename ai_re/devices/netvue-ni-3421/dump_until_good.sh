#!/usr/bin/env bash
# Retry a full CH341A read every 5s until we get a GOOD dump:
#   - full 16 MB
#   - uImage magic intact at 0x48000 (boot+kernel came through)
#   - rootfs region (>=0x200000) has real data + a filesystem magic (squashfs/jffs2),
#     i.e. NOT the flaky-clip brownout that returns all-0xFF.
# On success: saves fulldump.bin (+ sha256) and exits 0. Otherwise loops forever.
set -u
DIR="/Users/nep/nocloud/git/nepeat/ai_re/devices/netvue-ni-3421"
LOG="$DIR/dump_loop.log"
CAND="$DIR/fulldump_cand.bin"
OUT="$DIR/fulldump.bin"
FR_NOISE='aborting all transactions|no capture entitlements|capture requires|Cannot detach|darwin_claim_interface|another process has device|free software|get the source|flashrom.org|^$'
ts(){ date '+%Y-%m-%d %H:%M:%S'; }

echo "[$(ts)] === dump loop started (5s retry until rootfs reads clean) ===" >> "$LOG"
attempt=0
while true; do
  attempt=$((attempt+1))
  # fast probe first: if the clip isn't contacting, retry every 2s without a full read
  if ! flashrom -p ch341a_spi 2>&1 | grep -aq "Found "; then
    echo "[$(ts)] attempt $attempt: no contact — reseat clip (retry 2s)" >> "$LOG"
    sleep 2; continue
  fi
  echo "[$(ts)] attempt $attempt: >>> CHIP DETECTED — reading 16MB (verbose), hold the clip steady <<<" >> "$LOG"
  rm -f "$CAND"
  # -V makes flashrom verbose about the read (block layout, addresses, progress)
  flashrom -V -p ch341a_spi -c GD25Q128C -r "$CAND" 2>&1 | grep -avE "$FR_NOISE" >> "$LOG"

  if [ ! -f "$CAND" ]; then
    echo "[$(ts)] attempt $attempt: FAIL — chip not detected (clip not contacting; 0 bytes read)" >> "$LOG"
    sleep 5; continue
  fi

  result=$(python3 - "$CAND" <<'PY'
import sys
d=open(sys.argv[1],"rb").read()
if len(d)!=16777216:
    print("BAD size=%d bytes"%len(d)); sys.exit()
uimg = d[0x48000:0x48004]==b'\x27\x05\x19\x56'
tail = d[0x200000:]
nonff = len(tail) - tail.count(b'\xff')
sq = tail.find(b'hsqs')
jf = tail.find(b'\x85\x19')
# furthest real data = last byte that isn't 0xFF (the brownout point on a partial read)
last = len(d) - 1
while last >= 0 and d[last] == 0xFF:
    last -= 1
got = last + 1
ok = uimg and nonff>500000 and (sq!=-1 or jf!=-1)
print("%s | read_to=0x%06x (%.2f/16.00 MB real data) | uimg=%s rootfs_nonFF=%d squashfs=%s jffs2=%s"%(
    "GOOD" if ok else "PARTIAL", got, got/1048576.0, bool(uimg), nonff,
    hex(0x200000+sq) if sq!=-1 else "-", hex(0x200000+jf) if jf!=-1 else "-"))
PY
)
  echo "[$(ts)] attempt $attempt: $result" >> "$LOG"

  if [ "${result%% *}" = "GOOD" ]; then
    mv -f "$CAND" "$OUT"
    sha=$(shasum -a 256 "$OUT" | awk '{print $1}')
    echo "[$(ts)] ###### GOOD READ on attempt $attempt ######" >> "$LOG"
    echo "[$(ts)] saved: $OUT" >> "$LOG"
    echo "[$(ts)] sha256: $sha" >> "$LOG"
    exit 0
  fi

  echo "[$(ts)] attempt $attempt: still partial/brownout — reseat clip; retrying in 5s" >> "$LOG"
  sleep 5
done
