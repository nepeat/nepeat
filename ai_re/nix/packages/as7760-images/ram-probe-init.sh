#!/bin/sh
export PATH=/bin
mount -t proc proc /proc
mount -t sysfs sysfs /sys
mount -t devtmpfs devtmpfs /dev
mkdir -p /dev/pts
mount -t devpts devpts /dev/pts
mount -t tmpfs tmpfs /run
mount -t tmpfs tmpfs /tmp
exec </dev/console >/dev/console 2>&1
echo 'AS7760 RAM PROBE: no persistent filesystem mounted'
uname -a
insmod /as7760_fan.ko || echo 'fan module load failed; inspect dmesg'
echo '--- memory ---'
free -m
echo '--- block devices ---'
cat /proc/partitions
echo '--- network ---'
if ethtool -K eth0 rxhash off; then
  ip link set eth0 up
  # Link negotiation and the upstream port may take longer than 18 seconds.
  udhcpc -i eth0 -n -q -t 30 -T 3 -s /etc/udhcpc.script || echo 'DHCP unavailable; serial shell remains available'
fi
ip addr
echo '--- hwmon ---'
for hw in /sys/class/hwmon/hwmon*; do
  [ -e "$hw/name" ] || continue
  echo "$hw: $(cat "$hw/name")"
  for value in "$hw"/temp*_input "$hw"/fan*_input "$hw"/pwm1; do
    [ -f "$value" ] && echo "$value: $(cat "$value")"
  done
done
echo 'serial root shell; all state is in RAM'
while true; do
  setsid cttyhack sh
  sleep 1
done
