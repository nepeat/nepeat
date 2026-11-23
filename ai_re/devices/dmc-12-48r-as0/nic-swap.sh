#!/usr/bin/env bash
# Guarded live swap of the DMC-12's only NIC driver, run ON the box, detached
# (systemd-run), because the ssh session that starts it loses the network:
#   scp nic-swap.sh root@10.60.2.10:/root/ && ssh root@10.60.2.10 \
#     systemd-run --unit nic-swap /run/current-system/sw/bin/bash /root/nic-swap.sh <module.ko> [hold-s] [capture-cmd] [diag-cmd-on-failure]
# 1. arms the board CPLD watchdog (90 ticks = 93 s; a hung or crashed kernel
#    gets a full board reset -> the default generation), kicked while healthy;
# 2. unloads al_eth, loads <module.ko>, waits up to 90 s for eth0: carrier, IPv4,
#    gateway + workstation answer ping; runs [capture-cmd]; holds [hold-s] seconds
#    (kicking only while the network stays good);
# 3. unloads it, loads al_eth again, waits for the network; disarms the watchdog.
#    If the network does not come back: reboot (watchdog armed: reboots anyway).
# Log: /var/lib/nic-swap/log (+ journal, unit nic-swap). Watchdog semantics:
# nix/modules/boards/dmc12/trial-boot.nix (writing 0x2a = 0 reloads the count).
set -u
export PATH=/run/current-system/sw/bin:/run/wrappers/bin:$PATH
new=${1:?usage: nic-swap.sh <module.ko> [hold-s] [capture-cmd]}
hold=${2:-0}
capture=${3:-}
diag=${4:-}
gw=10.60.2.1 ws=10.60.2.208 dir=/var/lib/nic-swap
mkdir -p "$dir"
log() { echo "$(date -Is) nic-swap: $*" | tee -a "$dir/log"; }
cpld() { i2cset -y 4 0x68 "$1" "$2"; }
arm() { cpld 0x29 90 && cpld 0x28 1 && cpld 0x2a 0 && [ "$(i2cget -y 4 0x68 0x28)" = 0x01 ]; }
kick() { cpld 0x2a 0; }
disarm() { cpld 0x28 0; }
netok() {
  [ "$(cat /sys/class/net/eth0/carrier 2>/dev/null)" = 1 ] &&
  ip -4 addr show dev eth0 scope global | grep -q 'inet ' &&
  ping -c1 -W2 "$gw" >/dev/null 2>&1 && ping -c1 -W2 "$ws" >/dev/null 2>&1
}
waitnet() {   # $1: seconds
  local t=0
  while [ "$t" -lt "$1" ]; do netok && { log "network up after $t s"; return 0; }; kick; sleep 2; t=$((t + 2)); done
  return 1
}
back() {
  rmmod "$(modinfo -F name "$new" 2>/dev/null || basename "$new" .ko)" 2>/dev/null
  modprobe al_eth
  if waitnet 90; then disarm; log "al_eth back, watchdog disarmed"; exit "$1"; fi
  log "network did not come back with al_eth: rebooting (watchdog armed)"
  systemctl reboot; exit 1
}
arm || { log "could not arm the CPLD watchdog: not swapping"; exit 1; }
log "watchdog armed; swapping $(awk '$1 == "al_eth" || $1 == "al_eth_rs" {print $1}' /proc/modules) -> $new (hold $hold s)"
cur=$(awk '$1 == "al_eth" || $1 == "al_eth_rs" {print $1}' /proc/modules)
rmmod $cur || { log "rmmod $cur failed"; disarm; exit 1; }
if ! insmod "$new"; then log "insmod $new failed"; back 1; fi
if ! waitnet 90; then
  log "FAIL: no network with $new"; dmesg | tail -n 40 >> "$dir/log"
  [ -z "$diag" ] || { log "diag: $diag"; sh -c "$diag" >> "$dir/log" 2>&1; }
  back 1
fi
if [ -n "$capture" ]; then log "capture: $capture"; sh -c "$capture" >> "$dir/log" 2>&1; fi
end=$(( $(date +%s) + hold ))
fails=0
while [ "$(date +%s)" -lt "$end" ]; do
  # a single failed check under heavy load is not an outage: three in a row are
  if netok; then kick; fails=0; else fails=$((fails + 1)); log "network check failed ($fails/3)"; fi
  if [ "$fails" -ge 3 ]; then log "network lost during hold"; dmesg | tail -n 40 >> "$dir/log"; back 1; fi
  sleep 5
done
log "done with $new"
back 0
