# The AS7760 fan CPLD hwmon driver in Rust (./rust): hwmon name as7760_fan,
# pwm1 + fan1..12_input/_fault/_label. It replaced the C driver on 2026-11-23
# after the AS7772 trial (../kernel-rust-ppc64be/README.md "proof on hardware").
# Needs a kernel with CONFIG_RUST: linux-qoriq-t2080 { rust = true; } (NixOS
# hardware.qoriqT2080.rust, the default). The build runs the register logic's
# unit tests on the build host; ../kernel-rust-ppc64be/tests.nix (qemu-e6500)
# runs the module on i2c-stub against ./qemu-expected.txt, recorded from the C driver.
{ lib, stdenv, kernel, buildPackages }:
assert lib.assertMsg (kernel.withRust or false)
  "as7760-fan-rs: kernel ${kernel.name} is built without CONFIG_RUST";
stdenv.mkDerivation {
  pname = "as7760-fan-rs";
  inherit (kernel) version;
  src = builtins.path { path = ./rust; name = "as7760-fan-rs-src"; };
  __structuredAttrs = true;
  strictDeps = true;
  hardeningDisable = [ "all" ];
  # rustc + bindgen come with a Rust-enabled kernel's moduleBuildDependencies.
  nativeBuildInputs = kernel.moduleBuildDependencies;
  depsBuildBuild = [ buildPackages.stdenv.cc ]; # links the host unit tests
  makeFlags = kernel.commonMakeFlags ++ [
    "-C" "${kernel.dev}/lib/modules/${kernel.modDirVersion}/build"
    # no kernel.dev store path in the module (panic locations)
    "KRUSTFLAGS=--remap-path-prefix=${kernel.dev}/=/kernel/"
  ];
  buildPhase = ''
    runHook preBuild
    rustc --edition 2021 -D warnings -A dead_code -C linker=$CC_FOR_BUILD --test logic.rs -o $TMPDIR/logic-tests
    $TMPDIR/logic-tests
    make "''${makeFlags[@]}" M=$(pwd) modules
    runHook postBuild
  '';
  installPhase = ''
    runHook preInstall
    # as the C driver: kbuild install, so nixpkgs generates the per-module
    # certificate its module aggregator needs
    make "''${makeFlags[@]}" M=$(pwd) INSTALL_MOD_PATH=$out INSTALL_MOD_DIR=extra DEPMOD=true modules_install
    runHook postInstall
  '';
  meta = { description = "Edge-Core AS7760 fan CPLD hwmon driver (Rust)"; license = lib.licenses.gpl2Only; };
}
