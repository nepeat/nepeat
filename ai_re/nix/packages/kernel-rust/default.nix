# Rust in the kernel, arm64 (Alpine): a Rust-enabled test kernel that also boots
# QEMU "virt", the Rust modules built against it, and QEMU tests. README.md has
# the facts (which boards can, what the kernel crate covers) and the inventory.
#   import ./kernel-rust { inherit pkgs; }   # pkgs: the build host's
{ pkgs }:
let
  lib = pkgs.lib;
  cross = pkgs.pkgsCross.aarch64-multiplatform;
  # The DMC-12's kernel recipe (allnoconfig + explicit) with CONFIG_RUST, plus
  # what the test needs: the virt machine's UART, a fake SMBus adapter, trace
  # events on the console, two of the kernel's own Rust samples.
  linuxAlpine = import ../linux-alpine {
    pkgs = cross;
    inherit lib;
    variant = "minimal";
    rust = true;
    boardConfig = with lib.kernel; {
      SERIAL_AMBA_PL011 = yes;
      SERIAL_AMBA_PL011_CONSOLE = yes;
      I2C_STUB = module;
      FTRACE = yes;
      ENABLE_DEFAULT_TRACERS = yes; # event tracing only (tp_printk)
      SAMPLES = yes;
      SAMPLES_RUST = yes;
      SAMPLE_RUST_MINIMAL = module;
      SAMPLE_RUST_DRIVER_I2C = module;
    };
  };
  inherit (linuxAlpine) kernel;
  kp = linuxAlpine.kernelPackages;
  cpldResetC = kp.callPackage ../dmc12-cpld-reset { };
  cpldResetRs = kp.callPackage ../dmc12-cpld-reset/rust.nix { };

  # initramfs: static busybox, the modules, ./qemu-init.sh as /init.
  initrd = cpldReset: pkgs.runCommand "kernel-rust-test-initrd" {
    nativeBuildInputs = [ pkgs.cpio pkgs.gzip pkgs.xz pkgs.zstd ];
  } ''
    mkdir -p root/bin root/m root/proc root/sys root/dev
    cp ${cross.pkgsStatic.busybox}/bin/busybox root/bin/busybox
    for a in sh mount insmod rmmod lsmod reboot poweroff ls sed head echo cat grep sleep; do
      ln -s busybox root/bin/$a
    done
    for m in ${kernel.modules}/lib/modules/*/kernel/drivers/i2c/i2c-stub.ko* \
             ${kernel.modules}/lib/modules/*/kernel/samples/rust/rust_minimal.ko* \
             ${cpldReset}/lib/modules/*/*/dmc12_cpld_reset.ko*; do
      case $m in
        *.cer) ;;
        *.xz) xz -dc $m > root/m/$(basename $m .xz) ;;
        *.zst) zstd -dc $m > root/m/$(basename $m .zst) ;;
        *) cp $m root/m/ ;;
      esac
    done
    ls -l root/m
    test -f root/m/i2c-stub.ko -a -f root/m/rust_minimal.ko -a -f root/m/dmc12_cpld_reset.ko
    cp ${./qemu-init.sh} root/init
    chmod +x root/init
    (cd root && find . | cpio -o -H newc --quiet | gzip -9) > $out
  '';

  # One boot per scenario; the console log is the result.
  run = impl: cpldReset: pkgs.runCommand "dmc12-cpld-reset-qemu-${impl}" {
    nativeBuildInputs = [ pkgs.qemu pkgs.coreutils ];
  } ''
    mkdir -p $out
    for scenario in restart nak nobus poweroff; do
      timeout 600 qemu-system-aarch64 -M virt -cpu cortex-a57 -smp 2 -m 512 \
        -nographic -no-reboot -kernel ${kernel}/Image -initrd ${initrd cpldReset} \
        -append "console=ttyAMA0 panic=-1 tp_printk trace_event=smbus:smbus_write scenario=$scenario" \
        < /dev/null > $out/$scenario.console.txt 2>&1 &
    done
    wait
    for scenario in restart nak nobus poweroff; do
      log=$out/$scenario.console.txt
      for marker in "rust_minimal loaded" "rust_minimal unloaded" "module loaded" "module unloaded" "module loaded again" "init done"; do
        grep -q "^TEST: $marker" $log || { cat $log; echo "missing: TEST: $marker" >&2; exit 1; }
      done
      # What the module did, in kernel-log order: its messages (name prefix
      # dropped: the C module says dmc12-cpld-reset, Rust dmc12_cpld_reset), the
      # SMBus writes on the bus, and how the kernel went on.
      tr -d '\r' < $log \
        | sed -n -E '/taints kernel|Kernel command line/d; s/^\[[ 0-9.]+\] *(dmc12[-_]cpld[-_]reset: )?//; /^(smbus_write|reboot): /p; /CPLD|falling back/p' \
        > $out/$scenario.txt
    done
  '';
  qemuC = run "c" cpldResetC;
  qemuRs = run "rust" cpldResetRs;
in
{
  inherit kernel;
  dmc12-cpld-reset-c = cpldResetC;
  dmc12-cpld-reset-rs = cpldResetRs;
  # console logs per scenario (*.console.txt) and what the module did (*.txt)
  dmc12-cpld-reset-qemu-c = qemuC;
  dmc12-cpld-reset-qemu-rust = qemuRs;
  # The Rust-enabled kernel boots, loads a Rust sample module, and the Rust
  # dmc12_cpld_reset does on the (fake) bus exactly what the C one does, in
  # every scenario; both against ./qemu-expected.
  dmc12-cpld-reset-qemu-test = pkgs.runCommand "dmc12-cpld-reset-qemu-test" { } ''
    mkdir -p $out
    ln -s ${qemuC} $out/c
    ln -s ${qemuRs} $out/rust
    for scenario in restart nak nobus poweroff; do
      for impl in c rust; do
        if ! diff -u ${./qemu-expected}/$scenario.txt $out/$impl/$scenario.txt; then
          echo "FAIL: $impl module, scenario $scenario" >&2
          cat $out/$impl/$scenario.console.txt >&2
          exit 1
        fi
      done
      echo "ok: $scenario (C and Rust)"
    done
  '';
}
