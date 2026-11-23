#!/bin/sh
# Run inside the QEMU guest of ../kernel-rust-ppc64be/tests.nix (as /extra.sh):
# the same scenario against the C driver (/m/fan-c) and the Rust one
# (/m/fan-rs), each bound to an i2c-stub chip at 0x66. Every observable result
# is a "FAN <impl> ..." line (stdout, collected by qemu-init.sh); the host
# compares the two sets line by line.
export PATH=/bin
insmod /m/i2c-stub.ko chip_addr=0x66 || { echo "TEST: FAIL i2c-stub"; exit 0; }
B=
for a in /sys/bus/i2c/devices/i2c-*; do
  grep -q "SMBus stub" "$a/name" && B=${a##*i2c-}
done
[ -n "$B" ] || { echo "TEST: FAIL no stub bus"; exit 0; }
set_() { i2cset -f -y "$B" 0x66 "$1" "$2" b; }
get_() { i2cget -f -y "$B" 0x66 "$1" b; }
regs() { # the CPLD state the test starts from
  set_ 0x11 5; set_ 0x33 0xaa; set_ 0x0f 0x04
  set_ 0x12 10; set_ 0x13 20; set_ 0x14 0; set_ 0x15 40; set_ 0x16 50; set_ 0x17 255
  set_ 0x22 11; set_ 0x23 0; set_ 0x24 33; set_ 0x25 44; set_ 0x26 55; set_ 0x27 1
}
D=/sys/bus/i2c/devices/i2c-$B
run() { # impl dir
  impl=$1
  insmod "/m/$2/as7760_fan.ko" || echo "FAN $impl insmod failed"
  regs
  echo as7760-fan 0x66 > "$D/new_device"
  hw=$(ls -d "/sys/bus/i2c/devices/$B-0066/hwmon/"hwmon* 2>/dev/null)
  echo "FAN $impl bound $( [ -n "$hw" ] && echo yes || echo no) driver=$(basename "$(readlink "/sys/bus/i2c/devices/$B-0066/driver")")"
  echo "FAN $impl name=$(cat "$hw/name")"
  for f in $(cd "$hw" && ls | grep -E '^(pwm|fan)' | sort); do
    m=$(ls -l "$hw/$f" | cut -c1-10)
    v=$(cat "$hw/$f" 2>&1 | sed 's/^cat: [^:]*: //')
    echo "FAN $impl $f mode=$m value=$v"
  done
  for v in 100 127 128 129 170 200 254 255 256 -1 abc; do
    set_ 0x33 0xaa
    if echo "$v" > "$hw/pwm1" 2>/dev/null; then rc=ok; else rc=err; fi
    echo "FAN $impl write $v $rc duty=$(get_ 0x11) pre=$(get_ 0x33) pwm1=$(cat "$hw/pwm1" 2>&1 | sed 's/^cat: [^:]*: //')"
  done
  set_ 0x11 0x20
  echo "FAN $impl duty-out-of-range pwm1=$(cat "$hw/pwm1" 2>&1 | sed 's/^cat: [^:]*: //')"
  set_ 0x11 5; set_ 0x0f 0x3f
  echo "FAN $impl all-absent faults=$(cat "$hw"/fan*_fault | tr '\n' ' ')"
  echo 0x66 > "$D/delete_device"
  set_ 0x11 0x20
  echo as7760-fan 0x66 > "$D/new_device"
  echo "FAN $impl probe-duty-0x20 hwmon=$(ls "/sys/bus/i2c/devices/$B-0066/hwmon" 2>/dev/null | wc -l)"
  echo 0x66 > "$D/delete_device"
  rmmod as7760_fan && echo "FAN $impl rmmod ok"
}
run c fan-c
run rust fan-rs
run rust fan-rs
echo "TEST: fan scenarios done"
