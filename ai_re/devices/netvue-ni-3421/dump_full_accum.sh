#!/usr/bin/env bash
# Full-sequential-read accumulator (region reads are broken on this rig).
# Each pass: probe, then a full 16MB read. A read is "clean" only if its pre-brownout
# prefix matches the known-good fulldump.bin (rejects garbage reads). Track the deepest
# clean read. Declare success when two consecutive clean reads are byte-IDENTICAL over the
# whole 16MB (rejects brownout truncation + any one-off corruption). Logs deepest depth.
set -u
DIR="/Users/nep/nocloud/git/nepeat/ai_re/devices/netvue-ni-3421"
LOG="$DIR/dump_loop.log"
GOOD="$DIR/fulldump.bin"      # known-good prefix reference (valid to 0x5abc94)
PREV="$DIR/.accum_prev.bin"
BEST="$DIR/fulldump_best.bin"
OUT="$DIR/fulldump_full.bin"
FR_NOISE='aborting all transactions|no capture entitlements|capture requires|Cannot detach|darwin_claim_interface|another process has device|free software|get the source|flashrom.org|^$'
ts(){ date '+%Y-%m-%d %H:%M:%S'; }
best=0
echo "[$(ts)] === FULL-READ accumulator start (deepest-clean + 2x-identical) ===" >> "$LOG"
pass=0
while true; do
  pass=$((pass+1))
  if ! flashrom -p ch341a_spi 2>&1 | grep -aq "Found "; then
    echo "[$(ts)] pass$pass: no contact (retry 3s)" >> "$LOG"; sleep 3; continue
  fi
  echo "[$(ts)] pass$pass: full read..." >> "$LOG"
  rm -f /tmp/accum.bin
  flashrom -p ch341a_spi -c GD25Q128C -r /tmp/accum.bin 2>&1 | grep -avE "$FR_NOISE" >> "$LOG"
  [ -f /tmp/accum.bin ] || { echo "[$(ts)] pass$pass: read errored" >> "$LOG"; sleep 3; continue; }

  verdict=$(python3 - "$best" <<'PY'
import sys
best=int(sys.argv[1])
d=open("/tmp/accum.bin","rb").read()
good=open("/Users/nep/nocloud/git/nepeat/ai_re/devices/netvue-ni-3421/fulldump.bin","rb").read()
if len(d)!=16777216: print("BAD size"); sys.exit()
last=len(d)-1
while last>=0 and d[last]==0xff: last-=1
depth=last+1
lim=min(depth,0x5abc94)
clean = d[:lim]==good[:lim]           # prefix must match known-good => real read, not garbage
if not clean: print("DIRTY depth=0x%06x"%depth); sys.exit()
# compare to prev clean read for a 2x-identical full capture
import os
prev="/Users/nep/nocloud/git/nepeat/ai_re/devices/netvue-ni-3421/.accum_prev.bin"
identical = os.path.exists(prev) and open(prev,"rb").read()==d
open(prev,"wb").write(d)
tag="NEWBEST" if depth>best else "clean"
print("%s depth=0x%06x %.2fMB identical_to_prev=%s"%(tag,depth,depth/1048576.0,identical))
PY
)
  echo "[$(ts)] pass$pass: $verdict" >> "$LOG"
  set -- $verdict
  # update best
  case "$verdict" in
    NEWBEST*) cp -f /tmp/accum.bin "$BEST"; best=$(python3 -c "d=open('/tmp/accum.bin','rb').read(); l=len(d)-1
while l>=0 and d[l]==0xff: l-=1
print(l+1)");;
  esac
  if echo "$verdict" | grep -q "identical_to_prev=True"; then
    cp -f /tmp/accum.bin "$OUT"
    sha=$(shasum -a 256 "$OUT" | awk '{print $1}')
    echo "[$(ts)] ###### TWO IDENTICAL FULL READS — verified dump $OUT sha256=$sha ######" >> "$LOG"
    exit 0
  fi
  sleep 4
done
