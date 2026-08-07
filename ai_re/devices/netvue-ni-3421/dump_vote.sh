#!/usr/bin/env bash
# Majority-vote full-read accumulator. Region reads are broken; full reads sometimes
# complete (depth ~16MB) but carry occasional bit errors, and contact is intermittent.
# Strategy: bank every COMPLETE full read (depth >= 0xF00000). Once >=3 are banked,
# majority-vote per byte to cancel random errors, validate the voted result against the
# known-good prefix (fulldump.bin, 0..0x5abc94 must match exactly) and require the vote to
# be STABLE (unchanged when the newest read is included). On success -> fulldump_full.bin.
set -u
DIR="/Users/nep/nocloud/git/nepeat/ai_re/devices/netvue-ni-3421"
LOG="$DIR/dump_loop.log"
VOTES="$DIR/votes"; mkdir -p "$VOTES"
OUT="$DIR/fulldump_full.bin"
FR_NOISE='aborting all transactions|no capture entitlements|capture requires|Cannot detach|darwin_claim_interface|another process has device|free software|get the source|flashrom.org|^$'
ts(){ date '+%Y-%m-%d %H:%M:%S'; }
echo "[$(ts)] === VOTING accumulator: bank complete reads, majority-vote, validate ===" >> "$LOG"
n=$(ls "$VOTES"/read_*.bin 2>/dev/null | wc -l | tr -d ' ')
pass=0
while true; do
  pass=$((pass+1))
  if ! flashrom -p ch341a_spi 2>&1 | grep -aq "Found "; then
    echo "[$(ts)] pass$pass: no contact (retry 3s)" >> "$LOG"; sleep 3; continue
  fi
  echo "[$(ts)] pass$pass: full read (banked so far: $n)..." >> "$LOG"
  rm -f /tmp/vote.bin
  flashrom -p ch341a_spi -c GD25Q128C -r /tmp/vote.bin 2>&1 | grep -avE "$FR_NOISE" >> "$LOG"
  [ -f /tmp/vote.bin ] || { echo "[$(ts)] pass$pass: read errored" >> "$LOG"; sleep 3; continue; }

  res=$(python3 - "$VOTES" "$OUT" <<'PY'
import sys,os,glob
VOTES,OUT=sys.argv[1],sys.argv[2]
GOOD="/Users/nep/nocloud/git/nepeat/ai_re/devices/netvue-ni-3421/fulldump.bin"
d=open("/tmp/vote.bin","rb").read()
good=open(GOOD,"rb").read()
if len(d)!=16777216: print("BADSIZE"); sys.exit()
last=len(d)-1
while last>=0 and d[last]==0xff: last-=1
depth=last+1
if depth < 0xF00000:
    print("BROWNOUT depth=0x%06x"%depth); sys.exit()   # incomplete read, don't bank
# bank it
idx=len(glob.glob(os.path.join(VOTES,"read_*.bin")))
open(os.path.join(VOTES,"read_%02d.bin"%idx),"wb").write(d)
reads=[open(f,"rb").read() for f in sorted(glob.glob(os.path.join(VOTES,"read_*.bin")))]
nb=len(reads)
if nb<3:
    lim=min(depth,0x5abc94); diffs=sum(1 for a,b in zip(d[:lim],good[:lim]) if a!=b)
    print("BANKED n=%d depth=0x%06x prefixerrs=%d"%(nb,depth,diffs)); sys.exit()
# majority vote — only at positions where reads disagree (found fast via big-int xor vs read0)
base=bytearray(reads[0]); r0i=int.from_bytes(reads[0],"big")
diffbits=0
for r in reads[1:]: diffbits |= r0i ^ int.from_bytes(r,"big")
# collect differing byte indices
diffidx=set()
b=diffbits
while b:
    lb=b & -b; bit=lb.bit_length()-1; diffidx.add(len(base)-1-(bit//8)); b^=lb
from collections import Counter
for i in diffidx:
    base[i]=Counter(r[i] for r in reads).most_common(1)[0][0]
voted=bytes(base)
lim=0x5abc94; diffs=sum(1 for a,b in zip(voted[:lim],good[:lim]) if a!=b)
# stability: does the newest read change the vote vs voting without it?
prev=reads[:-1]
if len(prev)>=1:
    pb=bytearray(prev[0]); p0=int.from_bytes(prev[0],"big"); db=0
    for r in prev[1:]: db|=p0^int.from_bytes(r,"big")
    pidx=set(); bb=db
    while bb:
        lb=bb&-bb; bit=lb.bit_length()-1; pidx.add(len(pb)-1-(bit//8)); bb^=lb
    for i in pidx: pb[i]=Counter(r[i] for r in prev).most_common(1)[0][0]
    stable = bytes(pb)==voted
else: stable=False
if diffs==0 and stable:
    open(OUT,"wb").write(voted); print("DONE n=%d prefixerrs=0 STABLE"%nb)
else:
    print("VOTED n=%d prefixerrs=%d stable=%s diffpositions=%d"%(nb,diffs,stable,len(diffidx)))
PY
)
  echo "[$(ts)] pass$pass: $res" >> "$LOG"
  if echo "$res" | grep -q "^DONE"; then
    sha=$(shasum -a 256 "$OUT" | awk '{print $1}')
    echo "[$(ts)] ###### VERIFIED via majority vote -> $OUT sha256=$sha ######" >> "$LOG"
    exit 0
  fi
  n=$(ls "$VOTES"/read_*.bin 2>/dev/null | wc -l | tr -d ' ')
  sleep 4
done
