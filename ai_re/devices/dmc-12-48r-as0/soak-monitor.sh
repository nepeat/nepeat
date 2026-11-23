#!/usr/bin/env bash
# Soak monitor for the DMC-12's NIC, run on the workstation: every 5 min ping + box stats,
# every 15 min iperf3 both ways and a 100 MiB sha256 round trip. Stops if the box leaves the
# expected generation (a revert). usage: soak-monitor.sh <toplevel> <hours> <log>
set -u
top=$1 hours=$2 log=$3 box=10.60.2.10 iperf=/nix/store/726ia175hgkzr45520m3qarw77lcrcf1-iperf-3.21/bin/iperf3
ssh_() { sudo -n ssh -o BatchMode=yes -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o LogLevel=ERROR -o ConnectTimeout=10 root@$box "$@"; }
head -c 104857600 /dev/urandom > /tmp/soak-$$.bin; want=$(sha256sum /tmp/soak-$$.bin | cut -d' ' -f1)
end=$(( $(date +%s) + hours * 3600 )); i=0
while [ "$(date +%s)" -lt "$end" ]; do
  i=$((i + 1)); t=$(date -Is)
  p=$(ping -c 50 -i 0.2 -q $box | tail -2 | tr '\n' ' ')
  s=$(ssh_ "readlink /run/booted-system; cut -d' ' -f1 /proc/uptime; grep -E 'SUnreclaim|^Slab' /proc/meminfo | tr -s ' ' | tr '\n' ' '; echo; ethtool -S eth0 | grep -E ' (rx_dropped|rx_error_frames|rx_refill_short|tx_dropped|tx_timeouts|mac_rx_errors|mac_tx_errors|ec_rfw_in_mac_drop|udma_s2m_drop_pkt): ' | tr -s ' ' | tr '\n' ' '; echo; echo warn=\$(dmesg | grep -c -E 'WARNING|BUG|Oops|rust_kernel|watchdog|timed out') tainted=\$(cat /proc/sys/kernel/tainted); tail -n 2 /var/lib/nic-swap/guard.log" 2>&1 | tr '\n' ' ')
  echo "$t #$i ping: $p" >> "$log"; echo "$t #$i box: $s" >> "$log"
  case $s in *"$top"*) ;; *) echo "$t REVERTED or unreachable: $s" >> "$log" ;; esac
  if [ $((i % 3)) = 1 ]; then
    ssh_ "nft add element inet nixos-fw temp-ports '{ tcp . 5201 }' 2>/dev/null; systemd-run --unit iperf3-s\$RANDOM $iperf -s -1 >/dev/null 2>&1"; sleep 2
    tx=$(iperf3 -c $box -t 10 -R 2>&1 | grep -E 'sender|receiver' | tr -s ' ' | tr '\n' ' ')
    ssh_ "systemd-run --unit iperf3-s\$RANDOM $iperf -s -1 >/dev/null 2>&1"; sleep 2
    rx=$(iperf3 -c $box -t 10 2>&1 | grep -E 'sender|receiver' | tr -s ' ' | tr '\n' ' ')
    up=$(ssh_ "cat > /dev/shm/soak.bin; sha256sum /dev/shm/soak.bin" < /tmp/soak-$$.bin | cut -d' ' -f1)
    down=$(ssh_ "cat /dev/shm/soak.bin; rm -f /dev/shm/soak.bin" | sha256sum | cut -d' ' -f1)
    [ "$up" = "$want" ] && [ "$down" = "$want" ] && sha=ok || sha="MISMATCH up=$up down=$down"
    echo "$t #$i box_tx: $tx" >> "$log"; echo "$t #$i box_rx: $rx" >> "$log"; echo "$t #$i sha256 100MiB both ways: $sha" >> "$log"
  fi
  sleep 300
done
rm -f /tmp/soak-$$.bin; echo "$(date -Is) soak done" >> "$log"
