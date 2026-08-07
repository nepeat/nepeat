#!/usr/bin/env bash
# Chunked, brownout-resistant CH341A dumper.
# Reads the flash as 16x1MB regions. Each chunk is validated by a DOUBLE read
# (two identical reads = stable/complete; a brownout truncates at a random point
# so its two reads differ and get rejected). Good chunks are banked; only missing
# chunks are retried. Staggered region starts let the appfs tail be reached with a
# fresh power rail. When all 16 are captured, they're stitched into fulldump_full.bin.
set -u
DIR="/Users/nep/nocloud/git/nepeat/ai_re/devices/netvue-ni-3421"
LOG="$DIR/dump_loop.log"
LAYOUT="$DIR/layout.txt"
CHDIR="$DIR/extract/chunks"
OUT="$DIR/fulldump_full.bin"
NCH=16
FR_NOISE='aborting all transactions|no capture entitlements|capture requires|Cannot detach|darwin_claim_interface|another process has device|free software|get the source|flashrom.org|Using region|^$'
mkdir -p "$CHDIR"
ts(){ date '+%Y-%m-%d %H:%M:%S'; }

read_region(){ # idx outfile -> 0 if flashrom produced a file
  local c; c=$(printf "c%02d" "$1"); rm -f "$2"
  flashrom -V -p ch341a_spi -c GD25Q128C -l "$LAYOUT" -i "$c" -r "$2" 2>&1 | grep -avE "$FR_NOISE" >> "$LOG"
  [ -f "$2" ]
}

echo "[$(ts)] === CHUNKED dumper: 16x1MB, double-read validated, staggered ===" >> "$LOG"
pass=0
while true; do
  pass=$((pass+1))
  # fast contact probe — don't spam 16 chunks when the clip isn't gripping
  if ! flashrom -p ch341a_spi 2>&1 | grep -aq "Found "; then
    echo "[$(ts)] pass$pass: no contact — reseat clip (retry 3s)" >> "$LOG"; sleep 3; continue
  fi
  for i in $(seq 0 $((NCH-1))); do
    c=$(printf "c%02d" "$i")
    [ -f "$CHDIR/$c.good" ] && continue
    read_region "$i" "/tmp/$c.a" || { echo "[$(ts)] pass$pass $c: read A miss" >> "$LOG"; continue; }
    read_region "$i" "/tmp/$c.b" || { echo "[$(ts)] pass$pass $c: read B miss" >> "$LOG"; continue; }
    verdict=$(python3 - "$i" "/tmp/$c.a" "/tmp/$c.b" "$CHDIR/$c.bin" <<'PY'
import sys
idx=int(sys.argv[1]); A,B,save=sys.argv[2],sys.argv[3],sys.argv[4]
def region(p):
    d=open(p,'rb').read(); s=idx*0x100000
    if len(d)>=16*1024*1024: return d[s:s+0x100000]
    if len(d)==0x100000: return d
    return None
a=region(A); b=region(B)
if a is None or b is None: print("BADSIZE"); sys.exit()
if a!=b:
    print("MISMATCH nonFF_a=%d nonFF_b=%d"%(len(a)-a.count(255), len(b)-b.count(255))); sys.exit()
open(save,'wb').write(a); print("OK nonFF=%d"%(len(a)-a.count(255)))
PY
)
    if [ "${verdict%% *}" = "OK" ]; then
      touch "$CHDIR/$c.good"; echo "[$(ts)] pass$pass $c: GOOD ($verdict)" >> "$LOG"
    else
      echo "[$(ts)] pass$pass $c: reject ($verdict)" >> "$LOG"
    fi
  done
  ng=$(ls "$CHDIR"/*.good 2>/dev/null | wc -l | tr -d ' ')
  echo "[$(ts)] pass$pass complete: $ng/$NCH chunks banked" >> "$LOG"
  if [ "$ng" -eq "$NCH" ]; then
    files=""; for i in $(seq 0 $((NCH-1))); do files="$files $(printf "$CHDIR/c%02d.bin" "$i")"; done
    cat $files > "$OUT"
    sha=$(shasum -a 256 "$OUT" | awk '{print $1}')
    echo "[$(ts)] ###### ALL 16 CHUNKS CAPTURED — stitched $OUT ######" >> "$LOG"
    echo "[$(ts)] sha256: $sha" >> "$LOG"
    exit 0
  fi
  sleep 2
done
