# The Rust al_eth on QEMU virt (no Alpine NIC model exists): the module built
# against kernel-rust's net-kernel (rustNet + debug: KFENCE, SLUB debug, lockdep,
# DMA-API debug, kmemleak), loaded and unloaded 100 times with an Intel e1000e
# on the bus (Ethernet class: the driver's class match makes it probe, and the
# probe must refuse it, vendor 8086, before touching anything), then checked
# for zero checker reports. Covers: module init/exit, PCI driver register/
# unregister, the probe's refusal path, the vtables and statics. Not covered:
# anything past the vendor check (BARs, FLR, rings, NAPI, DMA): no device model.
#   import ./rust-qemu-test.nix { inherit pkgs; }     # pkgs: the build host's
{ pkgs }:
let
  lib = pkgs.lib;
  cross = pkgs.pkgsCross.aarch64-multiplatform;
  kr = import ../kernel-rust { inherit pkgs; };
  kernel = kr.net-kernel;
  module = (cross.linuxPackagesFor kernel).callPackage ./rust-module.nix { };
  init = pkgs.writeText "al-eth-rs-qemu-init" ''
    #!/bin/sh
    mount -t proc proc /proc
    mount -t sysfs sys /sys
    mount -t devtmpfs dev /dev
    mount -t debugfs debugfs /sys/kernel/debug 2>/dev/null
    [ -e /sys/kernel/debug/kmemleak ] && echo clear > /sys/kernel/debug/kmemleak
    echo "TEST: start"
    n=$(grep -l 0x020000 /sys/bus/pci/devices/*/class | wc -l)
    echo "TEST: ethernet-class devices: $n"
    i=0; ok=1
    while [ $i -lt 100 ]; do
      insmod /m/al_eth.ko || ok=0
      [ -d /sys/bus/pci/drivers/al_eth ] || ok=0
      if ls /sys/bus/pci/drivers/al_eth/ | grep -q '^0000:'; then echo "TEST: FAILED: bound to a device"; ok=0; fi
      rmmod al_eth || ok=0
      [ -d /sys/bus/pci/drivers/al_eth ] && ok=0
      i=$((i + 1))
    done
    [ $ok = 1 ] && echo "TEST: 100 load/unload cycles, never bound"
    if [ -e /sys/kernel/debug/kmemleak ]; then
      echo scan > /sys/kernel/debug/kmemleak; sleep 5; echo scan > /sys/kernel/debug/kmemleak
      n=$(grep -c "unreferenced object" /sys/kernel/debug/kmemleak)
      echo "kmemleak: $n"
      [ "$n" = 0 ] && echo "TEST: kmemleak clean"
    fi
    t=$(cat /proc/sys/kernel/tainted)
    echo "tainted: $t"
    [ $((t & 512)) = 0 ] && echo "TEST: no warning taint"
    echo "TEST: done"
    poweroff -f
    sleep 60
  '';
  initrd = pkgs.runCommand "al-eth-rs-qemu-initrd" { nativeBuildInputs = [ pkgs.cpio pkgs.gzip pkgs.xz pkgs.zstd ]; } ''
    mkdir -p root/bin root/m root/proc root/sys root/dev
    cp ${cross.pkgsStatic.busybox}/bin/busybox root/bin/busybox
    for a in sh mount insmod rmmod lsmod poweroff ls sed head echo cat grep sleep wc; do ln -s busybox root/bin/$a; done
    m=$(ls ${module}/lib/modules/*/*/al_eth.ko* | grep -v '\.cer$' | head -1)
    case $m in
      *.xz) xz -dc $m > root/m/al_eth.ko ;;
      *.zst) zstd -dc $m > root/m/al_eth.ko ;;
      *) install -m644 $m root/m/al_eth.ko ;;
    esac
    cp ${init} root/init; chmod +x root/init
    (cd root && find . | cpio -o -H newc --quiet | gzip -9) > $out
  '';
  # the same list as kernel-rust's fake-nic test
  splats = "BUG:|WARNING:|Oops|Call trace:|KFENCE: |DMA-API: |possible circular locking|inconsistent lock state|possible recursive locking|sleeping function called from invalid context|unreferenced object|refcount_t: |list_(add|del) corruption|ref_tracker: |Unable to handle kernel|Kernel panic|general protection|slab-out-of-bounds|use-after-free|Redzone|Poison overwritten|Object already free|turning off the locking correctness validator";
in
pkgs.runCommand "al-eth-rs-qemu-test" {
  nativeBuildInputs = [ pkgs.qemu pkgs.coreutils pkgs.gnugrep ];
  passthru = { inherit module kernel; };
} ''
  mkdir -p $out
  timeout 1800 qemu-system-aarch64 -M virt -cpu cortex-a57 -smp 2 -m 1024 \
    -nographic -no-reboot -nic none -device e1000e \
    -kernel ${kernel}/Image -initrd ${initrd} \
    -append "console=ttyAMA0 panic=-1 slub_debug=FZPU kfence.sample_interval=5" \
    < /dev/null > $out/console.txt 2>&1 || true
  tr -d '\r' < $out/console.txt > $out/clean.txt
  grep '^TEST: ' $out/clean.txt > $out/steps.txt || true
  grep -E -n "${splats}" $out/clean.txt \
    | grep -v -E 'DMA-API: (preallocated [0-9]+ debug entries|debugging enabled by kernel config)' > $out/splats.txt || true
  cat $out/steps.txt
  for s in "TEST: ethernet-class devices: 1" "TEST: 100 load/unload cycles, never bound" "TEST: no warning taint" "TEST: done"; do
    grep -qxF "$s" $out/steps.txt || { echo "FAIL: missing '$s'" >&2; tail -80 $out/clean.txt >&2; exit 1; }
  done
  if [ -s $out/splats.txt ]; then echo "FAIL: kernel reported:" >&2; cat $out/splats.txt >&2; exit 1; fi
  echo "ok: al_eth (Rust) 100 load/unload cycles under the debug checkers, probe refuses a foreign NIC, no reports" | tee $out/result.txt
''
