#!/bin/sh
# /init of the kernel-rust-ppc64be QEMU tests (tests.nix). Steps log to
# /results; at the end console printk is silenced and the results and the
# whole kernel log are printed in one piece (console output of the kernel and
# of this script would otherwise interleave). check-console.sh reads that part.
export PATH=/bin
mount -t proc proc /proc
mount -t sysfs sys /sys
mount -t devtmpfs dev /dev 2>/dev/null
r() { echo "TEST: $*" >> /results; }
r "cpus $(nproc) $(grep -m1 '^cpu' /proc/cpuinfo)"
cycle() { # module file, module name, times
  i=0
  while [ $i -lt $3 ]; do
    if insmod "$1"; then r "loaded $2"; else r "FAIL load $2"; fi
    if rmmod "$2"; then r "unloaded $2"; else r "FAIL unload $2"; fi
    i=$((i + 1))
  done
}
[ -f /m/rust_ppc64_abi.ko ] && cycle /m/rust_ppc64_abi.ko rust_ppc64_abi 2
for m in /m/samples/*.ko; do
  [ -f "$m" ] || continue
  cycle "$m" "$(basename "$m" .ko)" 1
done
[ -x /extra.sh ] && /extra.sh >> /results 2>&1
r "tainted $(cat /proc/sys/kernel/tainted)"
echo 1 > /proc/sys/kernel/printk
echo "=== results ==="
cat /results
echo "=== kernel log ==="
dmesg
echo "TEST: done"
poweroff -f
