# fake_nic: knet's test NIC on QEMU's edu PCI device (./fake_nic.rs + ../knet),
# built against a Rust kernel with the net bindings patch (linux-alpine
# rust = true + rustNet = true; without it the build fails on missing bindings).
# Run by ../../../kernel-rust (fake-nic-qemu-test). Never loaded on hardware.
#   kernelPackages.callPackage ./qemu { }
{ lib, stdenv, kernel }:
assert lib.assertMsg (kernel.withRust or false)
  "fake_nic: kernel ${kernel.name} is built without CONFIG_RUST";
stdenv.mkDerivation {
  pname = "fake-nic";
  inherit (kernel) version;
  src = builtins.path {
    path = ../.;
    name = "al-eth-knet-src";
    filter = p: t: let r = lib.removePrefix (toString ../. + "/") (toString p); in
      r == "knet" || r == "qemu" || lib.hasPrefix "knet/" r
      || (lib.hasPrefix "qemu/" r && (lib.hasSuffix ".rs" r || baseNameOf r == "Kbuild"));
  };
  __structuredAttrs = true;
  strictDeps = true;
  hardeningDisable = [ "all" ];
  nativeBuildInputs = kernel.moduleBuildDependencies;
  makeFlags = kernel.commonMakeFlags ++ [
    "-C" "${kernel.dev}/lib/modules/${kernel.modDirVersion}/build"
    "KRUSTFLAGS=--remap-path-prefix=${kernel.dev}/=/kernel/ -Dwarnings"
  ];
  buildPhase = ''
    runHook preBuild
    make "''${makeFlags[@]}" M=$(pwd)/qemu modules
    runHook postBuild
  '';
  installPhase = ''
    runHook preInstall
    make "''${makeFlags[@]}" M=$(pwd)/qemu INSTALL_MOD_PATH=$out modules_install
    runHook postInstall
  '';
  meta = { description = "knet test NIC on QEMU edu"; license = lib.licenses.gpl2Only; platforms = [ "aarch64-linux" ]; };
}
