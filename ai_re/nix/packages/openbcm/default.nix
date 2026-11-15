# Broadcom OpenBCM SDK 6.5.27 (github.com/Broadcom-Network-Switching-Software/
# OpenBCM; Broadcom Switch Software License, kernel modules GPL-2), patched to
# build with current toolchains for big-endian PowerPC. See README.md.
#   import ./openbcm { pkgs = <target package set>; }
#     .src        patched sdk-6.5.27 tree
#     .kmodsFor   kernel -> BDE + KNET modules
#     .bcm-user   the diag shell (links the whole SDK; proves the userspace port)
#     .libsdk     the SDK userspace as one shared library + headers + the
#                 compiler flags it was built with (for sai-openbcm)
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

  assertPlatform = lib.assertMsg (stdenv.hostPlatform.isPower64 && stdenv.hostPlatform.isBigEndian)
    "openbcm userspace: only the ppc64be platform is ported (patch 0002)";
  userMake = "make -C systems/linux/user/ppc64be -j$NIX_BUILD_CORES CROSS_COMPILE=${stdenv.cc.targetPrefix} HCC=gcc";
in rec {
  inherit src;
  kmodsFor = kernel: pkgs.callPackage ./kmods.nix { inherit kernel src; };

  # chips: Make.local fragment selecting the chip drivers to build.
  bcm-user = { makeLocal ? ./Make.local.as7760 }:
    assert assertPlatform;
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

  # The whole SDK userspace (driver, BCM API, diag shell library, BDE/KNET
  # userspace glue) as lib/libsdk.so, via the SDK's own BCM_COMPILE_SHARED_LIB
  # rule (systems/linux/user/common/Makefile, non-OpenNSA branch: every
  # archive linked --whole-archive, no export map, so every SDK symbol stays
  # visible). LINUX_MAKE_SHARED_LIB stays 0 (that would turn every
  # sub-library into its own .so); -fPIC is added to CFGFLAGS instead.
  # Outputs: lib/libsdk.so, include/openbcm/{include,bde,kmod,phymod} (public
  # SDK headers, BDE and KNET kcom headers, phymod headers), share/openbcm/cflags (the exact
  # defines the library was compiled with; consumers of the headers must
  # use them, they select chip, endianness, word size and features).
  libsdk = { makeLocal ? ./Make.local.as7760 }:
    assert assertPlatform;
    stdenv.mkDerivation {
      pname = "openbcm-libsdk";
      version = "6.5.27";
      inherit src;
      nativeBuildInputs = [ pkgs.buildPackages.perl pkgs.buildPackages.stdenv.cc pkgs.buildPackages.which ];
      enableParallelBuilding = true;
      hardeningDisable = [ "all" ];
      postPatch = ''
        cat make/Make.local.template ${makeLocal} > make/Make.local
        cat >> make/Make.local <<'EOF'
        # libsdk: position-independent objects, and the flag that makes
        # systems/linux/user/common/Makefile produce libsdk.so.
        CFGFLAGS += -fPIC -DBCM_COMPILE_SHARED_LIB
        EOF
        mkdir -p build/linux/user/ppc64be   # the makefile does not create it
      '';
      buildPhase = ''
        runHook preBuild
        export SDK=$PWD
        # "bcm" links bcm.user against libsdk.so when BCM_COMPILE_SHARED_LIB
        # is set, so it builds the library first and proves it links.
        ${userMake} bcm
        ${userMake} --no-print-directory -s variable VAR=CFLAGS > cflags.raw
        runHook postBuild
      '';
      installPhase = ''
        runHook preInstall
        lib=$(find build -name libsdk.so -type f | head -1)
        test -n "$lib"
        install -Dm755 "$lib" $out/lib/libsdk.so
        mkdir -p $out/include/openbcm $out/share/openbcm
        cp -r include $out/include/openbcm/include
        cp -r systems/bde/linux/include $out/include/openbcm/bde
        cp -r systems/linux/kernel/modules/include $out/include/openbcm/kmod
        # soc/drv.h (via soc/phyctrl.h) reaches into phymod's public headers.
        cp -r libs/phymod/include $out/include/openbcm/phymod
        # Keep only the preprocessor flags (-D...) of the SDK's CFLAGS.
        tr ' ' '\n' < cflags.raw | grep -E '^-D' | sort -u | tr '\n' ' ' > $out/share/openbcm/cflags
        grep -q BE_HOST $out/share/openbcm/cflags
        runHook postInstall
      '';
      meta = {
        description = "Broadcom SDK userspace as a shared library (OpenBCM 6.5.27)";
        license = lib.licenses.unfreeRedistributable; # Broadcom Switch Software License
        platforms = [ "powerpc64-linux" ];
      };
    };
}
