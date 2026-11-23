# Annapurna Labs al_eth: the integrated NIC of Alpine v2 and v3 SoCs, out of
# tree. Source: QNAP GPL drop, via delroth and codeberg dma/dmc12-nixos; this
# copy is the CS8210's patched one (nepeat/nix/nixos/module/cs8210, 9e23110):
# SPEED_UNKNOWN reopen fix, C22 MDIO accessors, carrier from MAC link status
# for ports with no PHY/LM, minimal ethtool ops, and a rev 4 -> 2 mapping
# guarded to v3 parts; since b9a74d0 also the al_eth_close() guard for ports
# whose link task was never set up. The one copy in the repo: the DMC-12 and
# the CS8210 (nepeat/nix nixos/module/cs8210/kernel.nix) build this package,
# and U-Boot's patch 0016 vendors its HAL files from ./src (uboot/series.nix).
#   pkgs.callPackage ./al-eth { kernel = config.boot.kernelPackages.kernel; }
{ lib, stdenv, kernel }:
stdenv.mkDerivation {
  pname = "al_eth";
  inherit (kernel) version;
  src = builtins.path { path = ./src; name = "al_eth-src"; };
  # makeFlags must reach bash as an array: the kernel's flags contain
  # spaces-by-$(space) tricks that do not survive string flattening.
  __structuredAttrs = true;
  strictDeps = true;
  hardeningDisable = [ "all" ];
  nativeBuildInputs = kernel.moduleBuildDependencies;
  # The V2 HAL define is what both the DMC-12 (v2) and CS8210 (v3) use.
  makeFlags = kernel.commonMakeFlags ++ [
    "-C" "${kernel.dev}/lib/modules/${kernel.modDirVersion}/build"
    "KCFLAGS=-DCONFIG_AL_ETH_ALPINE_V2"
  ];
  buildPhase = ''
    runHook preBuild
    make "''${makeFlags[@]}" M=$(pwd) modules
    runHook postBuild
  '';
  installPhase = ''
    runHook preInstall
    make "''${makeFlags[@]}" M=$(pwd) INSTALL_MOD_PATH=$out modules_install
    runHook postInstall
  '';
  meta = {
    description = "Annapurna Labs al_eth NIC (Alpine v2/v3), out-of-tree";
    license = lib.licenses.gpl2Only;
    platforms = [ "aarch64-linux" ];
  };
}
