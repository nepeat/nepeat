# Broadcom OpenBCM SDK 6.5.27 (github.com/Broadcom-Network-Switching-Software/
# OpenBCM; Broadcom Switch Software License, kernel modules GPL-2), patched to
# build with current toolchains for big-endian PowerPC. See README.md.
#   import ./openbcm { pkgs = <target package set>; }
#     .src        patched sdk-6.5.27 tree
#     .kmodsFor   kernel -> BDE + KNET modules
#     .bcm-user   the diag shell (links the whole SDK; proves the userspace port)
{ pkgs }:
let
  inherit (pkgs) lib stdenv;
  rev = "6d2330ce1b4fc49d681cffcf09af408661be4c62";
  upstream = pkgs.fetchgit {
    url = "https://github.com/Broadcom-Network-Switching-Software/OpenBCM";
    inherit rev;
    sparseCheckout = [ "sdk-6.5.27" ];
    hash = "sha256-t+x156UXJyVGJ/1XcAAF6op7hwHY/H4OAWP0xUjSU04=";
  };
  src = pkgs.buildPackages.runCommand "openbcm-sdk-6.5.27-patched" {
    nativeBuildInputs = [ pkgs.buildPackages.perl pkgs.buildPackages.patch ];
  } ''
    cp -r ${upstream}/sdk-6.5.27 $out
    chmod -R u+w $out
    cd $out
    patch -p1 < ${./patches/0001-kernel-modules-build-on-Linux-7.patch}
    # The makefiles hard-code /bin/echo, /usr/bin/test, ...
    find . \( -name 'Makefile*' -o -name 'Make.*' -o -name '*.mk' \) -type f -print0 \
      | xargs -0 perl ${./fixpaths.pl} > /dev/null
    patch -p1 < ${./patches/0002-userspace-platform-for-64-bit-big-endian-PowerPC.patch}
  '';
in rec {
  inherit src;
  kmodsFor = kernel: pkgs.callPackage ./kmods.nix { inherit kernel src; };

  # chips: Make.local fragment selecting the chip drivers to build.
  bcm-user = { makeLocal ? ./Make.local.as7760 }:
    assert lib.assertMsg (stdenv.hostPlatform.isPower64 && stdenv.hostPlatform.isBigEndian)
      "openbcm userspace: only the ppc64be platform is ported (patch 0002)";
    stdenv.mkDerivation {
      pname = "openbcm-bcm-user";
      version = "6.5.27";
      inherit src;
      nativeBuildInputs = [ pkgs.buildPackages.perl pkgs.buildPackages.stdenv.cc pkgs.buildPackages.which ];
      enableParallelBuilding = true;
      hardeningDisable = [ "all" ];
      postPatch = ''
        cat make/Make.local.template ${makeLocal} > make/Make.local
        mkdir -p build/linux/user/ppc64be   # the makefile does not create it
      '';
      buildPhase = ''
        runHook preBuild
        export SDK=$PWD
        make -C systems/linux/user/ppc64be bcm -j$NIX_BUILD_CORES \
          CROSS_COMPILE=${stdenv.cc.targetPrefix} HCC=gcc
        runHook postBuild
      '';
      installPhase = ''
        install -Dm755 build/linux/user/ppc64be/bcm.user $out/bin/bcm.user
      '';
      meta = {
        description = "Broadcom SDK diagnostic shell (OpenBCM 6.5.27)";
        license = lib.licenses.unfreeRedistributable; # Broadcom Switch Software License
        platforms = [ "powerpc64-linux" ];
      };
    };
}
