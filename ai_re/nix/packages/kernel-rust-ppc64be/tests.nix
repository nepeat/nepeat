# Proof for ./default.nix (README.md "Proof"): Rust-enabled ppc64 big-endian
# kernels, the ABI conformance module, the bindgen layout check, the kernel's
# Rust samples and doctests, and the Rust as7760-fan against the C one, booted
# in QEMU on two CPU families:
#   e6500        Book3E, the AS7760/AS7772 kernel recipe (+ QEMU ppce500 platform
#                and test drivers as modules): the hardware candidate
#   e6500-kunit  the same + KUnit doctests of the kernel crate, debug assertions
#   pseries      Book3S POWER8 guest, big endian, ELFv2
#   import ./tests.nix { pkgs; ppc; }   pkgs: build host, ppc: the AS7760's
#   cross package set (powerpc64-unknown-linux-gnuabielfv2)
{ pkgs, ppc }:
let
  lib = pkgs.lib;
  rp = import ./. { inherit lib; };
  e6500Extra = with lib.kernel; {
    PPC_QEMU_E500 = yes;
    I2C_STUB = module;
    CONFIGFS_FS = yes;
    SAMPLES = yes;
    SAMPLES_RUST = yes;
    SAMPLE_RUST_MINIMAL = module;
    SAMPLE_RUST_MISC_DEVICE = module;
    SAMPLE_RUST_PRINT = module;
    SAMPLE_RUST_DMA = module;
    SAMPLE_RUST_DEBUGFS = module;
    SAMPLE_RUST_DEBUGFS_SCOPED = module;
    SAMPLE_RUST_DRIVER_I2C = module;
    SAMPLE_RUST_I2C_CLIENT = module;
    SAMPLE_RUST_DRIVER_PCI = module;
    SAMPLE_RUST_DRIVER_PLATFORM = module;
    SAMPLE_RUST_DRIVER_USB = module;
    SAMPLE_RUST_DRIVER_FAUX = module;
    SAMPLE_RUST_DRIVER_AUXILIARY = module;
    SAMPLE_RUST_SOC = module;
    SAMPLE_RUST_CONFIGFS = module;
    RUST_PHYLIB_ABSTRACTIONS = yes;
    AX88796B_PHY = module;
    AX88796B_RUST_PHY = yes;
  };
  kunitExtra = with lib.kernel; {
    KUNIT = yes;
    RUST_KERNEL_DOCTESTS = yes;
    RUST_DEBUG_ASSERTIONS = yes;
  };
  e6500 = import ../linux-qoriq-t2080 { pkgs = ppc; inherit lib; rust = true; extraConfig = e6500Extra; };
  e6500-kunit = import ../linux-qoriq-t2080 { pkgs = ppc; inherit lib; rust = true; extraConfig = e6500Extra // kunitExtra; };
  pseries = rp.keepBindings (ppc.linux_latest.override {
    defconfig = "allnoconfig";
    enableCommonConfig = false;
    autoModules = false;
    preferBuiltin = true;
    ignoreConfigErrors = false;
    buildDTBs = false;
    kernelPatches = rp.kernelPatches;
    features = { efiBootStub = false; ia32Emulation = false; };
    structuredExtraConfig = with lib.kernel; {
      PPC64 = yes;
      PPC_BOOK3S_64 = yes;
      CPU_BIG_ENDIAN = yes;
      POWER8_CPU = yes;
      PPC_PSERIES = yes;
      # QEMU's pseries machine: SLOF loads the kernel at 0x400000 and enters it
      # through prom_init
      RELOCATABLE = yes;
      PPC_OF_BOOT_TRAMPOLINE = yes;
      EXPERT = yes;
      SMP = yes;
      NR_CPUS = freeform "8";
      HVC_CONSOLE = yes;
      # the static busybox userspace uses VMX/VSX registers
      ALTIVEC = yes;
      VSX = yes;
      PRINTK = yes;
      BUG = yes;
      MULTIUSER = yes;
      FUTEX = yes;
      BINFMT_ELF = yes;
      BINFMT_SCRIPT = yes;
      BLK_DEV_INITRD = yes;
      RD_GZIP = yes;
      DEVTMPFS = yes;
      DEVTMPFS_MOUNT = yes;
      TMPFS = yes;
      PROC_FS = yes;
      SYSFS = yes;
      TTY = yes;
      MODULES = yes;
      MODULE_UNLOAD = yes;
      KALLSYMS = yes;
      DEBUG_INFO_NONE = yes;
      CC_OPTIMIZE_FOR_SIZE = yes;
      SAMPLES = yes;
      SAMPLES_RUST = yes;
      SAMPLE_RUST_MINIMAL = module;
      SAMPLE_RUST_PRINT = module;
      SAMPLE_RUST_MISC_DEVICE = module;
    } // rp.config;
  });

  # An out-of-tree module against a kernel; the .ko files land in $out.
  kmod = kernel: name: src: extra: ppc.stdenv.mkDerivation ({
    pname = name;
    inherit (kernel) version;
    inherit src;
    __structuredAttrs = true;
    strictDeps = true;
    hardeningDisable = [ "all" ];
    nativeBuildInputs = kernel.moduleBuildDependencies ++ [ pkgs.python3 ];
    makeFlags = kernel.commonMakeFlags ++ [
      "-C" "${kernel.dev}/lib/modules/${kernel.modDirVersion}/build"
      "KRUSTFLAGS=--remap-path-prefix=${kernel.dev}/=/kernel/"
    ];
    buildPhase = ''
      runHook preBuild
      make "''${makeFlags[@]}" M=$(pwd) modules
      runHook postBuild
    '';
    installPhase = "install -Dm444 -t $out *.ko";
  } // extra);
  abiTest = kernel: kmod kernel "rust-ppc64-abi" (builtins.path { path = ./abi-test; name = "rust-ppc64-abi-src"; }) {
    preBuild = "python3 -I gen_abi.py .";
  };

  # rustc's layout of every bindgen struct == GCC's (layout-check/layout_check.py)
  layoutCheck = kernel: ppc.stdenv.mkDerivation {
    pname = "rust-ppc64-layout-check";
    inherit (kernel) version;
    src = builtins.path { path = ./layout-check; name = "rust-ppc64-layout-check-src"; };
    __structuredAttrs = true;
    strictDeps = true;
    hardeningDisable = [ "all" ];
    nativeBuildInputs = kernel.moduleBuildDependencies ++ [ pkgs.python3 pkgs.xz ];
    makeFlags = kernel.commonMakeFlags;
    buildPhase = ''
      runHook preBuild
      mkdir ksrc
      tar -xJf ${kernel.src} -C ksrc --strip-components=1 --wildcards '*/drivers/base/base.h'
      build=${kernel.dev}/lib/modules/${kernel.modDirVersion}/build
      python3 -I layout_check.py $build/rust/bindings_generated.rs $build ksrc $PWD $out "''${makeFlags[@]}" | tee report.txt
      runHook postBuild
    '';
    installPhase = "cp report.txt $out/";
  };

  fanRs = kernel: ppc.callPackage ../as7760-fan/rust.nix { inherit kernel; };

  busybox = ppc.pkgsStatic.busybox;
  # initramfs: static busybox, ./qemu-init.sh as /init, files = [ [ src dst ] ],
  # and (samplesOf = a kernel) every Rust sample module that kernel built.
  initrd = name: files: extraSh: samplesOf: pkgs.runCommand "rust-ppc64-initrd-${name}" {
    nativeBuildInputs = [ pkgs.cpio pkgs.gzip pkgs.xz pkgs.zstd ];
  } ''
    mkdir -p root/bin root/m/samples root/proc root/sys root/dev
    cp ${busybox}/bin/busybox root/bin/
    for a in sh mount insmod rmmod lsmod poweroff ls cat grep sleep echo nproc basename dirname \
             readlink dmesg i2cset i2cget head tr sed printf test cut wc sort; do
      ln -s busybox root/bin/$a
    done
    ${lib.concatMapStrings (f: ''
      mkdir -p "root/$(dirname ${lib.elemAt f 1})"
      case ${lib.elemAt f 0} in
        *.xz) xz -dc ${lib.elemAt f 0} > root/${lib.elemAt f 1} ;;
        *.zst) zstd -dc ${lib.elemAt f 0} > root/${lib.elemAt f 1} ;;
        *) cp ${lib.elemAt f 0} root/${lib.elemAt f 1} ;;
      esac
    '') files}
    ${lib.optionalString (samplesOf != null) ''
      for m in $(find ${samplesOf.modules}/lib/modules -path '*samples/rust*' -name '*.ko*'); do
        case $m in
          *.xz) xz -dc $m > root/m/samples/$(basename $m .xz) ;;
          *.zst) zstd -dc $m > root/m/samples/$(basename $m .zst) ;;
          *) cp $m root/m/samples/ ;;
        esac
      done
      ls root/m/samples | grep -q rust_minimal
    ''}
    ${lib.optionalString (extraSh != null) "cp ${extraSh} root/extra.sh; chmod +x root/extra.sh"}
    cp ${./qemu-init.sh} root/init
    chmod +x root/init
    (cd root && find . | cpio -o -H newc --quiet | gzip -9) > $out
  '';

  machines = {
    e6500 = "-M ppce500 -cpu e6500 -smp 4 -m 2G -nographic -no-reboot -accel tcg,thread=multi";
    pseries = "-M pseries,cap-cfpc=broken,cap-sbbc=broken,cap-ibs=broken,cap-ccf-assist=off -cpu power8 -smp 4 -m 2G -nographic -vga none -no-reboot -accel tcg,thread=multi";
  };
  consoles = { e6500 = "ttyS0"; pseries = "hvc0"; };

  # Boot, run /init, check the console (./check-console.sh).
  qemuTest = { name, machine, kernel, files, extraSh ? null, samples ? false, kunit ? false, fan ? false }:
    pkgs.runCommand "rust-ppc64-qemu-${name}" {
      nativeBuildInputs = [ pkgs.qemu pkgs.python3 pkgs.gnugrep pkgs.gnused pkgs.diffutils ];
    } ''
      mkdir -p $out
      python3 -I ${./qemu-run.py} $out/console.txt 1500 -- qemu-system-ppc64 ${machines.${machine}} \
        -kernel ${kernel}/vmlinux -initrd ${initrd name files extraSh (if samples then kernel else null)} \
        -append "console=${consoles.${machine}} panic=-1" || { tail -50 $out/console.txt; exit 1; }
      KUNIT=${if kunit then "1" else "0"} FAN=${if fan then "1" else "0"} CPUS=4 \
        FAN_EXPECTED=${../as7760-fan/qemu-expected.txt} bash ${./check-console.sh} $out/console.txt | tee $out/result.txt
    '';
  abiFiles = kernel: [ [ "${abiTest kernel}/rust_ppc64_abi.ko" "m/rust_ppc64_abi.ko" ] ];
  stub = kernel: "${kernel.modules}/lib/modules/${kernel.modDirVersion}/kernel/drivers/i2c/i2c-stub.ko";
  fanFiles = kernel: [
    [ (stub kernel) "m/i2c-stub.ko" ]
    [ "${fanRs kernel}/lib/modules/${kernel.modDirVersion}/extra/as7760_fan.ko" "m/fan-rs/as7760_fan.ko" ]
  ];
  # The series with CONFIG_RUST off must not change the C kernel: every section
  # of vmlinux equal to the T2080 kernel's without the series, except the build
  # id (.notes) and the kernel's own store path in .rodata (compared with the
  # hashes masked). Both without IKCONFIG: the series adds CONFIG_HAVE_RUST=y
  # to .config (compared as text), and the embedded copy would move .rodata.
  noIkconfig = with lib.kernel; { IKCONFIG = no; IKCONFIG_PROC = option no; };
  defaultKernel = import ../linux-qoriq-t2080 { pkgs = ppc; inherit lib; extraConfig = noIkconfig; };
  patchedNoRust = import ../linux-qoriq-t2080 { pkgs = ppc; inherit lib; boardPatches = rp.kernelPatches; extraConfig = noIkconfig; };
  cNeutral = pkgs.runCommand "rust-ppc64-c-neutral" {
    nativeBuildInputs = [ ppc.buildPackages.binutils-unwrapped pkgs.diffutils pkgs.python3 ];
  } ''
    od=${ppc.stdenv.cc.targetPrefix}objdump; oc=${ppc.stdenv.cc.targetPrefix}objcopy
    sections() { $od -h "$1" | awk '$2 ~ /^\./ { print $2 }' | sort; }
    a=${defaultKernel}/vmlinux b=${patchedNoRust}/vmlinux
    diff <(sections $a) <(sections $b)
    mkdir -p $out; : > $out/sections.txt
    for s in $(sections $a); do
      $oc -O binary --only-section=$s $a A.bin; $oc -O binary --only-section=$s $b B.bin
      if [ $s = .rodata ]; then
        for f in A B; do python3 -c 'import re,sys; d=open(sys.argv[1],"rb").read(); open(sys.argv[1],"wb").write(re.sub(rb"/nix/store/[0-9a-z]{32}-", b"/nix/store/" + b"x"*32 + b"-", d))' $f.bin; done
      fi
      if cmp -s A.bin B.bin; then echo "same $s" >> $out/sections.txt; else echo "DIFF $s" >> $out/sections.txt; fi
    done
    cat $out/sections.txt
    grep '^DIFF' $out/sections.txt > diffs.txt || true
    # only the build id may differ
    if grep -v '^DIFF \.notes$' diffs.txt; then echo "C code changed" >&2; exit 1; fi
    diff <(grep -v '^#' ${defaultKernel.configfile}) <(grep -v '^#' ${patchedNoRust.configfile}) > $out/config.diff || true
    cat $out/config.diff
    if grep '^[<>]' $out/config.diff | grep -v 'CONFIG_HAVE_RUST=y'; then echo "config changed beyond HAVE_RUST" >&2; exit 1; fi
  '';
in
{
  # for a board kernel (the hardware trial): kernel -> derivation
  abiTestFor = abiTest;
  layoutCheckFor = layoutCheck;
  inherit e6500 e6500-kunit pseries;
  c-neutral = cNeutral;
  abi-e6500 = abiTest e6500;
  abi-pseries = abiTest pseries;
  layout-e6500 = layoutCheck e6500;
  layout-pseries = layoutCheck pseries;
  as7760-fan-rs = fanRs e6500;
  # ABI module (twice), every Rust sample, the Rust fan driver against the C
  # one on i2c-stub, 4 vCPUs
  qemu-e6500 = qemuTest {
    name = "e6500"; machine = "e6500"; kernel = e6500; fan = true;
    files = abiFiles e6500 ++ fanFiles e6500; samples = true;
    extraSh = ../as7760-fan/qemu-fan-test.sh;
  };
  qemu-e6500-kunit = qemuTest {
    name = "e6500-kunit"; machine = "e6500"; kernel = e6500-kunit; kunit = true;
    files = abiFiles e6500-kunit;
  };
  qemu-pseries = qemuTest {
    name = "pseries"; machine = "pseries"; kernel = pseries;
    files = abiFiles pseries; samples = true;
  };
}
