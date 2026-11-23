# The DMC-12 CPLD board reset in Rust (./rust): the module the board uses
# (boards/dmc12/cpld). It replaced a C module (deleted 2026-11-23 after this one
# reset u01 through the CPLD; same module name, parameter and behaviour). Needs a
# kernel with CONFIG_RUST (linux-alpine: `rust = true`; NixOS:
# hardware.alpineV2.kernelRust). The build runs the sequence logic's unit tests on
# the build host; ../kernel-rust runs it in QEMU against the C module's recorded behaviour. Notes and unsafe inventory: ../kernel-rust/README.md.
{ lib, stdenv, kernel, buildPackages }:
assert lib.assertMsg (kernel.withRust or false)
  "dmc12-cpld-reset-rs: kernel ${kernel.name} is built without CONFIG_RUST";
stdenv.mkDerivation {
  pname = "dmc12-cpld-reset-rs";
  inherit (kernel) version;
  src = builtins.path { path = ./rust; name = "dmc12-cpld-reset-rs-src"; };
  __structuredAttrs = true;
  strictDeps = true;
  hardeningDisable = [ "all" ];
  # rustc + bindgen come with a Rust-enabled kernel's moduleBuildDependencies.
  nativeBuildInputs = kernel.moduleBuildDependencies;
  depsBuildBuild = [ buildPackages.stdenv.cc ]; # links the host unit tests
  makeFlags = kernel.commonMakeFlags ++ [
    "-C" "${kernel.dev}/lib/modules/${kernel.modDirVersion}/build"
    # no kernel.dev store path in the module (panic locations)
    "KRUSTFLAGS=--remap-path-prefix=${kernel.dev}/=/kernel/ -Dwarnings"
  ];
  # The logic's unit tests, on the build host (a cross build has no check phase).
  buildPhase = ''
    runHook preBuild
    rustc --edition 2021 -D warnings -C linker=$CC_FOR_BUILD --test sequence.rs -o $TMPDIR/sequence-tests
    $TMPDIR/sequence-tests
    make "''${makeFlags[@]}" M=$(pwd) modules
    runHook postBuild
  '';
  installPhase = ''
    runHook preInstall
    make "''${makeFlags[@]}" M=$(pwd) INSTALL_MOD_PATH=$out modules_install
    runHook postInstall
  '';
  meta = { description = "DMC-12 CPLD board reset on reboot (Rust)"; license = lib.licenses.gpl2Only; platforms = [ "aarch64-linux" ]; };
}
