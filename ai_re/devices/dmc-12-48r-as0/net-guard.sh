#!/usr/bin/env bash
# Network guard for a long run of a new NIC driver on the DMC-12 (run ON the box, detached):
#   systemd-run --unit net-guard /run/current-system/sw/bin/bash /root/net-guard.sh <seconds>
# Arms the board CPLD watchdog (90 ticks = 93 s) and kicks it every 5 s while the network is good
# (carrier, IPv4, gateway + workstation ping). Three failed checks in a row, or a hung kernel (no
# kicks), reboot the board: on a trial boot that is the default generation. Ends by disarming.
# Log: /var/lib/nic-swap/guard.log. Watchdog semantics: nix/modules/boards/dmc12/trial-boot.nix.
set -u
export PATH=/run/current-system/sw/bin:/run/wrappers/bin:$PATH
dur=${1:?seconds} gw=10.60.2.1 ws=10.60.2.208 log=/var/lib/nic-swap/guard.log
mkdir -p /var/lib/nic-swap
say() { echo "$(date -Is) net-guard: $*" >> "$log"; }
cpld() { i2cset -y 4 0x68 "$1" "$2"; }
netok() {
  [ "$(cat /sys/class/net/eth0/carrier 2>/dev/null)" = 1 ] &&
  ip -4 addr show dev eth0 scope global | grep -q 'inet ' &&
  ping -c1 -W2 "$gw" >/dev/null 2>&1 && ping -c1 -W2 "$ws" >/dev/null 2>&1
}
cpld 0x29 90 && cpld 0x28 1 && cpld 0x2a 0 && [ "$(i2cget -y 4 0x68 0x28)" = 0x01 ] || { say "cannot arm"; exit 1; }
say "armed for $dur s"
end=$(( $(date +%s) + dur )); fails=0
while [ "$(date +%s)" -lt "$end" ]; do
  if netok; then cpld 0x2a 0; fails=0; else fails=$((fails + 1)); say "check failed ($fails/3)"; fi
  if [ "$fails" -ge 3 ]; then say "network lost: rebooting"; dmesg | tail -n 40 >> "$log"; systemctl reboot; exit 1; fi
  sleep 5
done
cpld 0x28 0; say "done, disarmed"
