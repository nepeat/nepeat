#!/usr/bin/env bash
# Runs ON the CS8210 (root): sonic-ring.sh check, up, inject, then a sample every 60 s for MINUTES (default 35),
# stopping early on any sign of the box's known failure modes; then stop + down (CONFIG_DB restored and compared).
#   ring-soak.sh [MINUTES]  > ring-<date>.txt
# Stops on: a watchdog trigger / restart / alarm or any learn-ring guard event (learn-wedge.md: invalid / recovered /
# substituted reads of 0x4860 / 0x4864 / 0x485c, a refused im_malloc), syncd RSS over 1.5 GiB (MemoryMax 2 GiB),
# MemAvailable under 1 GiB, a ring port down, a failed unit, the I2C FPGA marked dead, the ASIC over 90 C, fand in failsafe.
# An all-ones read of any other register (the guard's allones_reads_total, e.g. the one 0x104 read seen at SDK init,
# H1/H3) is counted and printed, not a stop reason (H3 soak part 1 stopped on it, 2026-11-26).
#   ring-soak.sh --counters   prints "stop_sum N other_allones M" for $RING_SOAK_PROM_DIR (default /run/prometheus-textfile)
set -u
here=$(cd "$(dirname "$0")" && pwd)
R="$here/sonic-ring.sh"
min=${1:-35}
# SONIC_RING_LOOPS (sonic-ring.sh): 2 = the two loops 3 <-> 17 and 31 <-> 32 only, 1 = 3 <-> 17 alone
case "${SONIC_RING_LOOPS:-3}" in
1) ports="Ethernet16 Ethernet128" ;;
2) ports="Ethernet16 Ethernet128 Ethernet240 Ethernet248" ;;
*) ports="Ethernet0 Ethernet8 Ethernet16 Ethernet128 Ethernet240 Ethernet248" ;;
esac
P=${RING_SOAK_PROM_DIR:-/run/prometheus-textfile}
LEARN_REGS='reg="0x48(60|64|5c)"'
failsafe() { grep -E '^fand_zone_failsafe' $P/fand.prom | awk '{s+=$2} END {print s+0}'; }
bad() {   # the counters that must stay 0: the watchdog's, the learn-ring registers' guard events, refused im_mallocs
  { grep -E '^cs8210_ifcs_watchdog_(triggers_total|restarts_total|alarm|escalations_total)' $P/cs8210-ifcs.prom
    grep -E "^sai_teralynx_ifcs_guard_(invalid_total|invalid_rc_total|recovered_total|substituted_total|allones_reads_total)\{$LEARN_REGS\}" $P/cs8210-ifcs.prom
    grep -E '^sai_teralynx_ifcs_guard_im_malloc_refused_total' $P/cs8210-ifcs.prom
  } | awk '{s+=$NF} END {print s+0}'
}
other() {   # all-ones reads of any other register: counted only
  grep -E '^sai_teralynx_ifcs_guard_allones_reads_total' $P/cs8210-ifcs.prom | grep -v -E "$LEARN_REGS" | awk '{s+=$NF} END {print s+0}'
}
[ "${1:-}" = --counters ] && { echo "stop_sum $(bad) other_allones $(other)"; exit 0; }
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
base=$(bad) base_fs=$(failsafe) base_other=$(other) last_other=$(other)
echo "== baseline watchdog/learn-ring guard event sum $base, other all-ones reads $base_other (counted only), fand zones in failsafe $base_fs (a failsafe already there, e.g. failed fan modules, is not a stop reason; a new one is)"
"$R" sample
"$R" inject
end=$(( $(date +%s) + 60 * min )) why=
while [ "$(date +%s)" -lt "$end" ]; do
  sleep 60
  "$R" sample
  o=$(other); [ "$o" = "$last_other" ] || { echo "== NOTE $(date -u +%FT%TZ): all-ones reads of other registers $last_other -> $o (counted, not a stop reason): $(grep -E '^sai_teralynx_ifcs_guard_allones_reads_total' $P/cs8210-ifcs.prom | grep -v -E "$LEARN_REGS" | tr '\n' ' ')"; last_other=$o; }
  why=$(stopreason)
  [ -z "$why" ] || { echo "== STOP EARLY $(date -u +%FT%TZ): $why"; break; }
done
"$R" sample
echo "== stop $(date -u +%FT%TZ)"
"$R" stop
"$R" sample
"$R" down
echo "== end $(date -u +%FT%TZ) ${why:-completed}; other all-ones reads $base_other -> $(other) (counted only)"
