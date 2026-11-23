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

  # The same kernel plus the net bindings patch (rustNet) and the debug option
  # set (debug: KFENCE, SLUB debug, lockdep, DMA-API debug, kmemleak, ...), for
  # the knet tests: the Rust NIC abstractions under every checker a GCC-built
  # Rust kernel can have (no KASAN: RUST needs clang for it).
  linuxAlpineNet = import ../linux-alpine {
    pkgs = cross;
    inherit lib;
    variant = "minimal";
    rust = true;
    rustNet = true;
    debug = true;
    boardConfig = with lib.kernel; {
      SERIAL_AMBA_PL011 = yes;
      SERIAL_AMBA_PL011_CONSOLE = yes;
    };
  };
  netKernel = linuxAlpineNet.kernel;
  fakeNic = linuxAlpineNet.kernelPackages.callPackage ../al-eth/rust/qemu { };
  # AF_PACKET traffic generator/checker for fake_nic (static, musl).
  pktloop = cross.pkgsStatic.runCommandCC "pktloop" { } ''
    mkdir -p $out/bin
    $CC -static -O2 -Wall -Werror -o $out/bin/pktloop ${../al-eth/rust/qemu/pktloop.c}
  '';
  fakeNicInitrd = pkgs.runCommand "fake-nic-test-initrd" {
    nativeBuildInputs = [ pkgs.cpio pkgs.gzip pkgs.xz pkgs.zstd ];
  } ''
    mkdir -p root/bin root/m root/proc root/sys root/dev
    cp ${cross.pkgsStatic.busybox}/bin/busybox root/bin/busybox
    for a in sh mount insmod rmmod lsmod reboot poweroff ls sed head echo cat grep sleep ip wc; do
      ln -s busybox root/bin/$a
    done
    install -m755 ${pktloop}/bin/pktloop root/bin/pktloop
    m=$(ls ${fakeNic}/lib/modules/*/*/fake_nic.ko* | head -1)
    case $m in
      *.xz) xz -dc $m > root/m/fake_nic.ko ;;
      *.zst) zstd -dc $m > root/m/fake_nic.ko ;;
      *) install -m644 $m root/m/fake_nic.ko ;;
    esac
    cp ${../al-eth/rust/qemu/qemu-init.sh} root/init
    chmod +x root/init
    (cd root && find . | cpio -o -H newc --quiet | gzip -9) > $out
  '';
  # Kernel messages that fail the test (any checker's report, any WARN/BUG).
  splats = "BUG:|WARNING:|Oops|Call trace:|KFENCE: |DMA-API: |possible circular locking|inconsistent lock state|possible recursive locking|sleeping function called from invalid context|unreferenced object|refcount_t: |list_(add|del) corruption|ref_tracker: |Unable to handle kernel|Kernel panic|general protection|slab-out-of-bounds|use-after-free|Redzone|Poison overwritten|Object already free|turning off the locking correctness validator";
  # One boot of the fake_nic test; args: kernel command line counts (and inject).
  fakeNicRun = name: args: pkgs.runCommand name {
    nativeBuildInputs = [ pkgs.qemu pkgs.coreutils pkgs.gnugrep ];
  } ''
    mkdir -p $out
    # edu: 32-bit DMA (see fake_nic.rs).
    timeout 3000 qemu-system-aarch64 -M virt -cpu cortex-a57 -smp 2 -m 1024 \
      -nographic -no-reboot -nic none -device edu,dma_mask=0xffffffff \
      -kernel ${netKernel}/Image -initrd ${fakeNicInitrd} \
      -append "console=ttyAMA0 panic=-1 slub_debug=FZPU kfence.sample_interval=5 ${args}" \
      < /dev/null > $out/console.txt 2>&1 || true
    tr -d '\r' < $out/console.txt > $out/clean.txt
    # kernel console lines can land inside a TEST line (no line buffering between
    # them): take the TEST text wherever it starts, cut a kernel timestamp after it
    grep -o 'TEST: .*' $out/clean.txt | sed -E 's/\[ *[0-9]+\.[0-9]+\].*$//' > $out/steps.txt || true
    grep -E -n "${splats}" $out/clean.txt \
      | grep -v -E 'DMA-API: (preallocated [0-9]+ debug entries|debugging enabled by kernel config)' > $out/splats.txt || true
    grep -E '^(pktloop|stats|kmemleak|dma-api errors|tainted):' $out/clean.txt > $out/summary.txt || true
  '';
  # The real test: traffic, MTU, rx mode, MAC, 200 up/down, 100 unload/reload, teardown
  # while DMA is in flight; every step as expected, and not one checker report.
  fakeNicQemuTest = let r = fakeNicRun "fake-nic-qemu-run" "updown=200 reload=100 frames=40"; in
    pkgs.runCommand "fake-nic-qemu-test" { nativeBuildInputs = [ pkgs.diffutils ]; } ''
      mkdir -p $out; ln -s ${r} $out/run
      if ! diff -u ${../al-eth/rust/qemu/expected.txt} ${r}/steps.txt; then
        echo "FAIL: steps differ" >&2; tail -150 ${r}/clean.txt >&2; exit 1
      fi
      if [ -s ${r}/splats.txt ]; then
        echo "FAIL: kernel reported:" >&2; cat ${r}/splats.txt >&2; exit 1
      fi
      cat ${r}/summary.txt
      echo "ok: fake_nic under KFENCE/slub_debug/lockdep/DMA-API debug/kmemleak, no reports" | tee $out/result.txt
    '';
  # Test of the test: the module leaks every TX packet and its mappings (inject=1,
  # safe code can leak); kmemleak and DMA-API debug must both say so.
  fakeNicDetectsLeak = let r = fakeNicRun "fake-nic-qemu-leak-run" "updown=2 reload=2 frames=6 inject=1"; in
    pkgs.runCommand "fake-nic-qemu-detects-leak" { nativeBuildInputs = [ pkgs.gnugrep ]; } ''
      mkdir -p $out; ln -s ${r} $out/run
      grep -q '^TEST: done' ${r}/steps.txt || { echo "FAIL: did not finish" >&2; exit 1; }
      if grep -q -x 'TEST: kmemleak clean' ${r}/steps.txt; then echo "FAIL: kmemleak missed the leak" >&2; exit 1; fi
      if grep -q -x 'TEST: dma-api clean' ${r}/steps.txt; then echo "FAIL: DMA-API debug missed the leaked mappings" >&2; exit 1; fi
      grep -q 'unreferenced object' ${r}/splats.txt
      grep -q 'DMA-API' ${r}/splats.txt
      cat ${r}/summary.txt
      echo "ok: injected leak reported by kmemleak and DMA-API debug" | tee $out/result.txt
    '';
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
      # dropped: the deleted C module said dmc12-cpld-reset, Rust says dmc12_cpld_reset),
      # the SMBus writes on the bus, and how the kernel went on.
      tr -d '\r' < $log \
        | sed -n -E '/taints kernel|Kernel command line/d; s/^\[[ 0-9.]+\] *(dmc12[-_]cpld[-_]reset: )?//; /^(smbus_write|reboot): /p; /CPLD|falling back/p' \
        > $out/$scenario.txt
    done
  '';
  qemuRs = run "rust" cpldResetRs;
in
{
  inherit kernel;
  # knet (Rust NIC abstractions, ../al-eth/rust/knet) tests
  net-kernel = netKernel;
  fake-nic = fakeNic;
  inherit pktloop;
  fake-nic-qemu-test = fakeNicQemuTest;
  fake-nic-qemu-detects-leak = fakeNicDetectsLeak;
  dmc12-cpld-reset-rs = cpldResetRs;
  # console logs per scenario (*.console.txt) and what the module did (*.txt)
  dmc12-cpld-reset-qemu-rust = qemuRs;
  # The Rust-enabled kernel boots, loads a Rust sample module, and the Rust
  # dmc12_cpld_reset does on the (fake) bus exactly what the C one did, in every
  # scenario: ./qemu-expected was recorded from the C module and matched by both
  # until the C one was deleted (2026-11-23, after the Rust one ran on u01).
  dmc12-cpld-reset-qemu-test = pkgs.runCommand "dmc12-cpld-reset-qemu-test" { } ''
    mkdir -p $out
    ln -s ${qemuRs} $out/rust
    for scenario in restart nak nobus poweroff; do
      if ! diff -u ${./qemu-expected}/$scenario.txt $out/rust/$scenario.txt; then
        echo "FAIL: scenario $scenario" >&2
        cat $out/rust/$scenario.console.txt >&2
        exit 1
      fi
      echo "ok: $scenario (Rust, as the C module did)"
    done
  '';
}
