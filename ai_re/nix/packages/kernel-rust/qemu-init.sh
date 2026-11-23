#!/bin/sh
# /init of the kernel-rust QEMU test (see default.nix). scenario= on the kernel
# command line picks what the dmc12_cpld_reset module meets at reboot:
#   restart   a device answers at 0x68 on the bus     -> both writes, 2 s, fall back
#   nak       nothing at 0x68 (the stub chip is 0x50) -> write error, fall back
#   nobus     bus= names an adapter that is not there -> fall back
#   poweroff  as restart, but the system powers off   -> the notifier does nothing
mount -t proc proc /proc
mount -t sysfs sys /sys
mount -t devtmpfs dev /dev
scenario=$(sed -n 's/.*scenario=\([a-z]*\).*/\1/p' /proc/cmdline)
echo "TEST: scenario $scenario"

# A Rust module of the kernel itself loads and unloads.
insmod /m/rust_minimal.ko && echo "TEST: rust_minimal loaded"
rmmod rust_minimal && echo "TEST: rust_minimal unloaded"

addr=0x68
[ "$scenario" = nak ] && addr=0x50
insmod /m/i2c-stub.ko chip_addr=$addr
bus=$(ls /sys/bus/i2c/devices | sed -n 's/^i2c-//p' | head -1)
[ "$scenario" = nobus ] && bus=9
echo "TEST: stub chip $addr, module bus $([ "$scenario" = nobus ] && echo absent || echo stub)"

# load, unload, load again: registration and unregistration both work
insmod /m/dmc12_cpld_reset.ko bus=$bus && echo "TEST: module loaded"
rmmod dmc12_cpld_reset && echo "TEST: module unloaded"
insmod /m/dmc12_cpld_reset.ko bus=$bus && echo "TEST: module loaded again"
echo "TEST: init done"
if [ "$scenario" = poweroff ]; then poweroff -f; else reboot -f; fi
sleep 60
