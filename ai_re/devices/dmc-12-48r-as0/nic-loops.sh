#!/usr/bin/env bash
# Unload/reload and ifdown/ifup loops of a NIC module on the DMC-12, run ON the box,
# detached (systemd-run), under the board CPLD watchdog like nic-swap.sh:
#   systemd-run --unit nic-loops /run/current-system/sw/bin/bash /root/nic-loops.sh <module.ko> <reloads> <updowns>
# Each reload: rmmod, insmod, wait for the network (gateway + workstation ping); every
# 10th reload also <updowns> ip link down/up cycles with a network check after each.
# Any failure: stop, restore al_eth (C), disarm (or reboot if the network does not come
# back; the watchdog backstops a hang). Log: /var/lib/nic-swap/loops.log.
set -u
export PATH=/run/current-system/sw/bin:/run/wrappers/bin:$PATH
new=${1:?module.ko} reloads=${2:-100} updowns=${3:-10}
name=$(modinfo -F name "$new")
gw=10.60.2.1 ws=10.60.2.208 log=/var/lib/nic-swap/loops.log
mkdir -p /var/lib/nic-swap
say() { echo "$(date -Is) nic-loops: $*" | tee -a "$log"; }
cpld() { i2cset -y 4 0x68 "$1" "$2"; }
arm() { cpld 0x29 90 && cpld 0x28 1 && cpld 0x2a 0 && [ "$(i2cget -y 4 0x68 0x28)" = 0x01 ]; }
kick() { cpld 0x2a 0; }
disarm() { cpld 0x28 0; }
netok() {
  [ "$(cat /sys/class/net/eth0/carrier 2>/dev/null)" = 1 ] &&
  ip -4 addr show dev eth0 scope global | grep -q 'inet ' &&
  ping -c1 -W2 "$gw" >/dev/null 2>&1 && ping -c1 -W2 "$ws" >/dev/null 2>&1
}
waitnet() { local t=0; while [ "$t" -lt "$1" ]; do netok && return 0; kick; sleep 1; t=$((t + 1)); done; return 1; }
restore() {
  rmmod "$name" 2>/dev/null; modprobe al_eth
  if waitnet 90; then disarm; say "al_eth back, watchdog disarmed"; exit "$1"; fi
  say "network did not come back with al_eth: rebooting"; systemctl reboot; exit 1
}
arm || { say "cannot arm the watchdog"; exit 1; }
cur=$(awk '$1 == "al_eth" || $1 == "al_eth_rs" {print $1}' /proc/modules)
say "start: $reloads reloads of $name, $updowns down/up every 10th; was $cur"
[ -z "$cur" ] || rmmod "$cur" || { say "rmmod $cur failed"; disarm; exit 1; }
start=$(date +%s); worst=0
for i in $(seq 1 "$reloads"); do
  t0=$(date +%s%N)
  insmod "$new" || { say "FAIL: insmod #$i"; restore 1; }
  waitnet 60 || { say "FAIL: no network after load #$i"; dmesg | tail -n 30 >> "$log"; restore 1; }
  t=$(( ($(date +%s%N) - t0) / 1000000 )); [ "$t" -le "$worst" ] || worst=$t
  if [ $((i % 10)) = 0 ]; then
    for j in $(seq 1 "$updowns"); do
      ip link set eth0 down; sleep 0.2; ip link set eth0 up
      waitnet 60 || { say "FAIL: no network after down/up #$j of load #$i"; dmesg | tail -n 30 >> "$log"; restore 1; }
    done
    say "load #$i ok (+ $updowns down/up); worst load-to-network $worst ms"
  fi
  if [ "$i" -lt "$reloads" ]; then rmmod "$name" || { say "FAIL: rmmod #$i"; restore 1; }; fi
done
say "done: $reloads reloads, $(( reloads / 10 * updowns )) down/up cycles in $(( $(date +%s) - start )) s; worst load-to-network $worst ms"
restore 0
