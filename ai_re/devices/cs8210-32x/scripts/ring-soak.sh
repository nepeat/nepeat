#!/usr/bin/env bash
# Runs ON the CS8210 (root): sonic-ring.sh check, up, inject, then a sample every 60 s for MINUTES (default 35),
# stopping early on any sign of the box's known failure modes; then stop + down (CONFIG_DB restored and compared).
#   ring-soak.sh [MINUTES]  > ring-<date>.txt
# Stops on: a watchdog trigger / restart / alarm or any learn-ring guard event (learn-wedge.md), syncd RSS over
# 1.5 GiB (MemoryMax 2 GiB), MemAvailable under 1 GiB, a ring port down, a failed unit, the I2C FPGA marked dead,
# the ASIC over 90 C, fand in failsafe.
set -u
here=$(cd "$(dirname "$0")" && pwd)
R="$here/sonic-ring.sh"
min=${1:-35}
# SONIC_RING_LOOPS=2 (sonic-ring.sh): the two loops 3 <-> 17 and 31 <-> 32 only
if [ "${SONIC_RING_LOOPS:-3}" = 2 ]; then ports="Ethernet16 Ethernet128 Ethernet240 Ethernet248"
else ports="Ethernet0 Ethernet8 Ethernet16 Ethernet128 Ethernet240 Ethernet248"; fi
P=/run/prometheus-textfile
failsafe() { grep -E '^fand_zone_failsafe' $P/fand.prom | awk '{s+=$2} END {print s+0}'; }
bad() {   # the counters that must stay 0
  { grep -E '^cs8210_ifcs_watchdog_(triggers_total|restarts_total|alarm|escalations_total)' $P/cs8210-ifcs.prom
    grep -E '^sai_teralynx_ifcs_guard_(invalid_total|invalid_rc_total|recovered_total|substituted_total|allones_reads_total|im_malloc_refused_total)' $P/cs8210-ifcs.prom
  } | awk '{s+=$NF} END {print s+0}'
}
stopreason() {
  local rss avail asic fs up fpga
  [ "$(bad)" = "$base" ] || { echo "watchdog / guard counters moved ($base -> $(bad))"; return; }
  rss=$(ps -o rss= -C syncd | head -1); [ "${rss:-0}" -lt 1572864 ] || { echo "syncd RSS $rss kB"; return; }
  avail=$(awk '/MemAvailable/ {print $2}' /proc/meminfo); [ "$avail" -gt 1048576 ] || { echo "MemAvailable $avail kB"; return; }
  asic=$(cut -d. -f1 /run/asic/temperature); [ "${asic:-0}" -lt 90 ] || { echo "ASIC $asic C"; return; }
  fs=$(failsafe); [ "$fs" -le "$base_fs" ] || { echo "fand went to failsafe ($base_fs -> $fs zones)"; return; }
  fpga=$(grep '^cs8210_optics_fpga_up' $P/cs8210-optics.prom | awk '{print $2}'); [ "$fpga" = 1 ] || { echo "I2C FPGA not up"; return; }
  [ "$(systemctl --failed --no-legend | wc -l)" = 0 ] || { echo "failed unit: $(systemctl --failed --no-legend)"; return; }
  up=$(for p in $ports; do redis-cli -s /var/run/redis/redis.sock -n 0 hget "PORT_TABLE:$p" oper_status; done | grep -c '^up$')
  [ "$up" = "$(echo $ports | wc -w)" ] || { echo "only $up ring ports up"; return; }
}
echo "== ring-soak $(date -u +%FT%TZ) $min min, $(uptime)"
"$R" check || exit 1
"$R" up || exit 1
base=$(bad) base_fs=$(failsafe)
echo "== baseline watchdog/guard event sum $base, fand zones in failsafe $base_fs (a failsafe already there, e.g. failed fan modules, is not a stop reason; a new one is)"
"$R" sample
"$R" inject
end=$(( $(date +%s) + 60 * min )) why=
while [ "$(date +%s)" -lt "$end" ]; do
  sleep 60
  "$R" sample
  why=$(stopreason)
  [ -z "$why" ] || { echo "== STOP EARLY $(date -u +%FT%TZ): $why"; break; }
done
"$R" sample
echo "== stop $(date -u +%FT%TZ)"
"$R" stop
"$R" sample
"$R" down
echo "== end $(date -u +%FT%TZ) ${why:-completed}"
