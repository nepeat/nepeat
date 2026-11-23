#!/bin/sh
# /init of the fake_nic QEMU test (packages/kernel-rust: fake-nic-qemu-test).
# Kernel command line: updown=N reload=M frames=K (cycle and frame counts).
# Every step prints "TEST: ..."; the runner compares those lines and fails on
# any kernel splat in the console log.
mount -t proc proc /proc
mount -t sysfs sys /sys
mount -t devtmpfs dev /dev
mount -t debugfs debugfs /sys/kernel/debug 2>/dev/null
arg() { sed -n "s/.*\<$1=\([0-9]*\).*/\1/p" /proc/cmdline; }
UPDOWN=$(arg updown); RELOAD=$(arg reload); FRAMES=$(arg frames); INJECT=$(arg inject)
# leaks from boot are not ours: start kmemleak from a clean slate
[ -e /sys/kernel/debug/kmemleak ] && echo clear > /sys/kernel/debug/kmemleak
echo "TEST: start"

iface() { ls /sys/bus/pci/drivers/fake_nic/*/net/ 2>/dev/null | head -1; }
check() { if "$@"; then :; else echo "TEST: FAILED: $*"; fi; }
stat() { cat /sys/class/net/$IF/statistics/$1; }

insmod /m/fake_nic.ko inject=${INJECT:-0} && echo "TEST: module loaded"
IF=$(iface)
[ -n "$IF" ] && echo "TEST: netdev registered"
ip link set $IF up && echo "TEST: link up"
[ "$(cat /sys/class/net/$IF/carrier)" = 1 ] && echo "TEST: carrier on"

# Traffic: small (copy path) to one-buffer frames, then multi-buffer frames.
/bin/pktloop $IF $FRAMES 60 1514 1 && echo "TEST: traffic mtu 1500"
ip link set $IF mtu 3800 && echo "TEST: mtu 3800"
/bin/pktloop $IF $FRAMES 1515 3800 2 && echo "TEST: traffic mtu 3800"
ip link set $IF mtu 1500
ip link set $IF mtu 9000 2>/dev/null || echo "TEST: mtu 9000 refused"
ip link set $IF promisc on && ip link set $IF multicast on && ip link set $IF promisc off && echo "TEST: rx mode"
ip link set $IF address 02:00:00:00:00:42 && echo "TEST: mac set"
ip link set $IF address 01:00:00:00:00:42 2>/dev/null || echo "TEST: multicast mac refused"
tx=$(stat tx_packets); rx=$(stat rx_packets)
echo "stats: tx $tx rx $rx dropped $(stat rx_dropped)/$(stat tx_dropped)"
[ "$tx" -ge $((FRAMES * 2)) ] && [ "$rx" -ge $((FRAMES * 2)) ] && echo "TEST: stats"

# up/down with a little traffic now and then
i=0; ok=1
while [ $i -lt $UPDOWN ]; do
  ip link set $IF down || ok=0
  ip link set $IF up || ok=0
  if [ $((i % 50)) = 0 ]; then /bin/pktloop $IF 2 60 1514 $((i + 7)) >/dev/null || ok=0; fi
  i=$((i + 1))
done
[ $ok = 1 ] && echo "TEST: up/down cycles"

# down with frames in flight (teardown waits for the DMA engine, drains the rings)
/bin/pktloop $IF 8 1000 3000 99 >/dev/null 2>&1 &
sleep 1
ip link set $IF down && echo "TEST: down while busy"
wait
ip link set $IF up

# unload/reload; every tenth time up + traffic, unload while up
i=0; ok=1
while [ $i -lt $RELOAD ]; do
  rmmod fake_nic || ok=0
  insmod /m/fake_nic.ko inject=${INJECT:-0} || ok=0
  IF=$(iface)
  if [ $((i % 10)) = 0 ]; then
    ip link set $IF up || ok=0
    /bin/pktloop $IF 2 60 1514 $((i + 3)) >/dev/null || ok=0
  fi
  i=$((i + 1))
done
[ $ok = 1 ] && echo "TEST: unload/reload cycles"
IF=$(iface); ip link set $IF up
/bin/pktloop $IF 4 60 1514 5 >/dev/null 2>&1 &
sleep 1
rmmod fake_nic && echo "TEST: unload while busy"
wait

if [ -e /sys/kernel/debug/kmemleak ]; then
  echo scan > /sys/kernel/debug/kmemleak; sleep 5; echo scan > /sys/kernel/debug/kmemleak
  n=$(grep -c "unreferenced object" /sys/kernel/debug/kmemleak)
  echo "kmemleak: $n"
  cat /sys/kernel/debug/kmemleak | head -60
  [ "$n" = 0 ] && echo "TEST: kmemleak clean"
fi
if [ -e /sys/kernel/debug/dma-api/error_count ]; then
  echo "dma-api errors: $(cat /sys/kernel/debug/dma-api/error_count)"
  [ "$(cat /sys/kernel/debug/dma-api/error_count)" = 0 ] && echo "TEST: dma-api clean"
fi
t=$(cat /proc/sys/kernel/tainted)
echo "tainted: $t"
[ $((t & 512)) = 0 ] && echo "TEST: no warning taint"
echo "TEST: done"
poweroff -f
sleep 60
