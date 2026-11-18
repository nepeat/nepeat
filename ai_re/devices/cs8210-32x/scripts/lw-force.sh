#!/usr/bin/env bash
# lw-force: forced L0s A/B on the CS8210 ASIC link (learn-wedge.md section 7; Fable review 2026-11-17-fable-asic-l0s.md).
# Waits for lw-crossover's second off block to end, stops it, then runs 1 h blocks of
#   on  = L0s on (kernel default: root-port-transmit only)
#   es  = L0s on + Extended Synch (root port LnkCtl bit 7: 4096 FTS per L0s exit)
#   off = L0s off
# each started like lw-crossover (lease, AER clear, SONiC restart, BFD, unlease), all under the same bursty
# CPU-path stress (ping bursts Vrf-a <-> Vrf-b over the 1<->16 loop; nothing leaves the loop), plus an ASIC-side
# AER witness: the ASIC's UESta/CESta polled every 5 s, UE logged at once (dated), CE summarised per minute, cleared.
set -u
export PATH=/run/current-system/sw/bin:$PATH
SCHED=(on es off es off on off on es on off es); BLOCK=${1:-3600}
D=/var/lib/lw-soak; OUT=$D/force.txt; mkdir -p $D
L=/sys/bus/pci/devices/0001:01:00.0/link/l0s_aspm
RP=0001:00:00.0; EP=0001:01:00.0
WDS=/run/sonic-cs8210-ifcs-watchdog/state.json
log() { echo "$(date +%FT%T) $*" >> $OUT; }
bfd() { for k in $(redis-cli -s /run/redis/redis.sock -n 6 keys 'BFD_SESSION_TABLE|*' 2>/dev/null); do printf '%s=%s ' "${k##*|}" "$(redis-cli -s /run/redis/redis.sock -n 6 hget "$k" state)"; done; }
txfull() { awk '$1=="TX-3"{print $6}' /sys/devices/ipd/ring_stat 2>/dev/null || echo NA; }
wdj() { [ -r $WDS ] && tr -d '\n' < $WDS | cut -c1-800 || echo "no-watchdog-state"; }
lease() { local n; for n in $(seq 120); do if mkdir /run/asic-lease 2>/dev/null; then echo "learn-wedge(force) $(date +%s)" > /run/asic-lease/owner; return 0; fi; [ $n = 1 ] && log "waiting for the ASIC lease ($(cat /run/asic-lease/owner 2>/dev/null))"; sleep 30; done; return 1; }
unlease() { rm -rf /run/asic-lease; }
cages() { local n q; for n in $(seq 10); do q=$(cs8210-qsfp --enable 2>&1) && break; sleep 0.5; done; case "$q" in *"present=00008001 "*) return 0;; *) log "CAGES NOT 1+16: $q"; return 1;; esac; }
es() { setpci -s $RP CAP_EXP+0x10.w=$1:0080; }   # 0080 set, 0000 clear
lnk() { echo "rpLnkCtl=$(setpci -s $RP CAP_EXP+0x10.w) epLnkCtl=$(setpci -s $EP CAP_EXP+0x10.w) rpLnkSta=$(setpci -s $RP CAP_EXP+0x12.w)"; }
stress() {  # bursts of 20-219 pings, 0.5 ms apart, 56-8055 B, 10-90 ms gaps; both directions
  local src=$1 dst=$2
  while :; do
    ip vrf exec $src ping -q -c $((RANDOM%200+20)) -i 0.0005 -s $((RANDOM%8000+56)) -W 1 $dst >/dev/null 2>&1
    sleep 0.0$((RANDOM%9+1))
  done
}
witness() {
  local u c ce=0 bits=0 t=$(date +%s)
  while :; do
    u=$(setpci -s $EP ECAP_AER+0x04.l); c=$(setpci -s $EP ECAP_AER+0x10.l)
    if [ "$u" != 00000000 ]; then log "ASIC UE UESta=$u CESta=$c cap=$(setpci -s $EP ECAP_AER+0x18.l) hdr=$(setpci -s $EP ECAP_AER+0x1c.l),$(setpci -s $EP ECAP_AER+0x20.l) $(lnk) txfull=$(txfull)"; setpci -s $EP ECAP_AER+0x04.l=$u; fi
    if [ "$c" != 00000000 ]; then ce=$((ce+1)); bits=$((bits | 0x$c)); setpci -s $EP ECAP_AER+0x10.l=$c; fi
    if [ $(( $(date +%s) - t )) -ge 60 ]; then [ $ce -gt 0 ] && log "ASIC CE polls=$ce bits=$(printf %08x $bits)"; ce=0; bits=0; t=$(date +%s); fi
    sleep 5
  done
}
P=()
stopkids() { if [ ${#P[@]} -gt 0 ]; then kill ${P[@]} 2>/dev/null; pkill -P $$ ping 2>/dev/null; wait 2>/dev/null; fi; P=(); }
finish() { stopkids; es 0000; echo 0 > $L; log "EXIT: L0s off, Extended Synch cleared ($(lnk))"; }
trap 'finish; exit 0' TERM INT
log "WAIT for lw-crossover's 2nd off block to end"
until [ "$(grep -c 'arm=off end after' $D/crossover.txt)" -ge 2 ]; do sleep 20; done
systemctl stop lw-crossover.service
[ -d /run/asic-lease ] && grep -q 'learn-wedge(crossover)' /run/asic-lease/owner 2>/dev/null && { log "removing the stopped crossover's lease"; unlease; }
log "START (crossover stopped after block 3 off) boot=$(cat /proc/sys/kernel/random/boot_id) schedule='${SCHED[*]}' block=${BLOCK}s; ASIC AER at start UESta=$(setpci -s $EP ECAP_AER+0x04.l) CESta=$(setpci -s $EP ECAP_AER+0x10.l)"
b=0
for arm in "${SCHED[@]}"; do
  b=$((b+1))
  lease || { log "no lease in 60 min: stop"; finish; exit 2; }
  case $arm in on) echo 1 > $L; es 0000;; es) echo 1 > $L; es 0080;; off) echo 0 > $L; es 0000;; esac
  setpci -s $RP ECAP_AER+0x10.l=ffffffff ECAP_AER+0x04.l=ffffffff
  setpci -s $EP ECAP_AER+0x10.l=ffffffff ECAP_AER+0x04.l=ffffffff
  cages || { unlease; finish; exit 3; }
  systemctl stop sonic.target; for k in $(seq 60); do pgrep -x syncd >/dev/null || break; sleep 1; done
  sync; systemctl start sonic.target
  up=0; for k in $(seq 300); do case "$(bfd)" in *Up*Up*) up=$k; break;; esac; sleep 1; done
  unlease
  t0=$(date +%s); since="$(date '+%F %T')"; sinceT="$(date +%FT%T)"; want="$(lnk | cut -d' ' -f1)"
  log "BLOCK $b arm=$arm l0s=$(cat $L) $(lnk) start, BFD up after ${up}s, syncd $(systemctl show sonic-syncd -p MainPID --value), txfull=$(txfull)"
  stress Vrf-a 10.0.1.1 & P+=($!); stress Vrf-b 10.0.1.0 & P+=($!); witness & P+=($!)
  f0=$(txfull)
  while [ $(( $(date +%s) - t0 )) -lt $BLOCK ]; do
    sleep 60
    c="$(lnk | cut -d' ' -f1)"; [ "$c" = "$want" ] || { log "LnkCtl drift in block $b: $c (want $want)"; want=$c; }
  done
  stopkids
  j() { journalctl "$@" --since "$since" -o cat --no-pager 2>/dev/null; }
  mine() { awk -v s="$sinceT" 'substr($0,1,19) >= s' $OUT; }
  log "BLOCK $b arm=$arm end after $(( $(date +%s) - t0 ))s: tx-stall=$(j -u sonic-cs8210-ifcs-watchdog | grep -c 'tx-stall') wd-restarts=$(j -u sonic-cs8210-ifcs-watchdog | grep -c 'syncd restarted') guard-learn=$(j -u sonic-syncd | grep -c -E 'ifcs guard: 0x48(60|64|5c) read') guard-other=$(j -u sonic-syncd | grep -c 'ifcs guard: register 0x') rp-UE=$(j -u sonic-cs8210-ifcs-watchdog | grep -c 'UE:') rp-CE-lines=$(j -u sonic-cs8210-ifcs-watchdog | grep -c 'CE:') asic-UE=$(mine | grep -c 'ASIC UE') asic-CE-min=$(mine | grep -c 'ASIC CE') kernel=$(j -k | grep -c -E 'ifcs-learn|oom-kill|NETDEV WATCHDOG|soft lockup') txfull=$f0->$(txfull) $(lnk) watchdog: $(wdj)"
done
log "DONE"; finish