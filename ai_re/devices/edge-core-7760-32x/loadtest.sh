#!/bin/sh
# Contained ASIC load test: MAC loopback on every front port + a broadcast
# storm in VLAN 1 (CPU removed), while feeding fand's ASIC sensor from
# "show temp". Aborts (ports off) at ABORT_C. Usage: loadtest.sh SECONDS
set -u
DUR=${1:-600}; ABORT_C=${ABORT_C:-85}
mkdir -p /run/asic
d=$(mktemp -d); cd "$d" || exit 1
cp /root/config-32x100.bcm config.bcm
mkfifo cmd
(
  echo "init all"; sleep 8
  echo "vlan remove 1 pbm=cpu"
  echo "port ce lb=mac"; sleep 3
  echo "port ce txpause=off rxpause=off"; sleep 1
  echo "pw start"; sleep 1
  echo "show c ce5"
  echo "tx 1 pbm=ce0 vlan=1 dm=ff:ff:ff:ff:ff:ff sm=00:00:00:00:00:01 len=1500"
  echo "STORM_START $(date +%s)" >&2
  end=$(( $(date +%s) + DUR ))
  while [ "$(date +%s)" -lt "$end" ]; do
    echo "show temp"; sleep 5
    t=$(cat /run/asic/temperature 2>/dev/null || echo 0)
    if awk -v t="$t" -v a="$ABORT_C" 'BEGIN{exit !(t>=a)}'; then echo "ABORT at $t C" >&2; break; fi
  done
  echo "show c ce5"
  echo "port ce en=0"; echo "port ce lb=none"; sleep 3
  echo "show temp"; sleep 2; echo "exit"
) > cmd 2>"$d/harness.err" &
script -qfec bcm.user /dev/null < cmd 2>&1 | tee /root/loadtest-out.txt | awk '
  { gsub("\r", "") } /temperature monitor/ { v = $5; gsub(",", "", v); v += 0; if (v > m) m = v }
  /maximum peak/ { f = "/run/asic/temperature"; printf "%.1f\n", m > (f ".tmp"); close(f ".tmp"); system("mv " f ".tmp " f); m = -100 }'
cat "$d/harness.err" >> /root/loadtest-out.txt
cd /; rm -rf "$d"
echo LOADTEST_DONE >> /root/loadtest-out.txt