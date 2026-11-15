# Broadcom OpenBCM (SDK 6.5.27) Linux kernel modules: BDE (device access) and,
# later, KNET. GPL-2 parts of the SDK. Built with a generated Kbuild file
# instead of the vendor makefiles, which assume WindRiver/ELDK toolchains and
# 2.6-era kernels. endian: "be" for big-endian hosts (PowerPC), "le" otherwise.
{ lib, stdenv, kernel, src, modules ? [ "linux-kernel-bde" "linux-user-bde" ] }:
let
  be = stdenv.hostPlatform.isBigEndian;
  is64 = stdenv.hostPlatform.is64bit;
  defs = [
    "-DLINUX" "-DNO_FILEIO" "-DNO_CTRL_C" "-DNO_MEMTUNE"
    "-DUSE_LINUX_BDE_MMAP=1" "-DSAL_BDE_DMA_MEM_DEFAULT=32"
    "-include" "$(src)/sdk/systems/linux/kernel/modules/include/linux7-compat.h"
  ] ++ lib.optionals is64 [ "-DPTRS_ARE_64BITS" "-DLONGS_ARE_64BITS" "-DPHYS_ADDRS_ARE_64BITS" ]
    # Same settings as the SDK's own PowerPC target (make/Makefile.linux-gto).
    ++ (if be then [ "-DSYS_BE_PIO=1" "-DSYS_BE_PACKET=0" "-DSYS_BE_OTHER=1" "-DBE_HOST=1" ]
        else [ "-DSYS_BE_PIO=0" "-DSYS_BE_PACKET=0" "-DSYS_BE_OTHER=0" "-DLE_HOST=1" ]);
  groups = {
    linux-kernel-bde = [
      "systems/bde/linux/kernel/linux-kernel-bde.c" "systems/bde/linux/kernel/linux_shbde.c"
      "systems/bde/linux/kernel/linux_dma.c" "systems/bde/linux/shared/mpool.c"
      "systems/bde/shared/shbde_pci.c" "systems/bde/shared/shbde_iproc.c" "systems/bde/shared/shbde_mdio.c"
      "systems/linux/kernel/modules/shared/gmodule.c"
    ];
    linux-user-bde = [
      "systems/bde/linux/user/kernel/linux-user-bde.c" "systems/linux/kernel/modules/shared/gmodule.c"
    ];
    linux-bcm-knet = [
      "systems/linux/kernel/modules/bcm-knet/bcm-knet.c" "systems/linux/kernel/modules/shared/gmodule.c"
    ];
  };
  includes = [ "include" "systems/bde/linux/include" "systems/bde/shared/include"
    "systems/linux/kernel/modules/include" "systems/bde/linux/kernel" "systems/bde/linux/shared"
    "systems/bde/linux/user/kernel" "systems/linux/kernel/modules/bcm-knet" ];
  kbuild = lib.concatStringsSep "\n" ([
    "ccflags-y := ${lib.concatMapStringsSep " " (i: "-I$(src)/sdk/${i}") includes} ${toString defs}"
  ] ++ lib.concatMap (m: [
    "obj-m += ${m}.o"
    "${m}-y := ${lib.concatStringsSep " " (lib.imap0 (i: f: "${m}_${toString i}.o") groups.${m})}"
  ]) modules);
in stdenv.mkDerivation {
  pname = "openbcm-kmods";
  version = "6.5.27-${kernel.version}";
  inherit src;
  nativeBuildInputs = kernel.moduleBuildDependencies;
  hardeningDisable = [ "all" ];
  __structuredAttrs = true;
  strictDeps = true;
  makeFlags = kernel.commonMakeFlags ++ [ "-C" "${kernel.dev}/lib/modules/${kernel.modDirVersion}/build" ];
  postUnpack = ''
    mkdir kb && ln -s "$PWD/$sourceRoot" kb/sdk
    printf '%s\n' ${lib.escapeShellArg kbuild} > kb/Makefile
${lib.concatStrings (lib.concatMap (m: lib.imap0 (i: f: "    ln -s sdk/${f} kb/${m}_${toString i}.c\n") groups.${m}) modules)}  '';
  buildPhase = ''make "''${makeFlags[@]}" M="$PWD/../kb" -k modules'';
  installPhase = ''
    make "''${makeFlags[@]}" M="$PWD/../kb" INSTALL_MOD_PATH="$out" INSTALL_MOD_DIR=extra DEPMOD=true modules_install
  '';
  meta.license = lib.licenses.gpl2Only;
}
