# Our U-Boot's environment from Linux (the U-Boot in SPI, devices/dmc-12-48r-as0/u-boot-port.md):
# redundant 2 x 64 KiB at CS0 0xf90000 / 0xfa0000. The DT exposes that window as its own
# writable mtd partition "u-boot-env"; the rest of both chips stays read-only.
#   dmc12-uboot-env print [VAR...]          read (fw_printenv)
#   dmc12-uboot-env set VAR [VALUE...]      write one change (no VALUE: delete VAR)
# Every write is logged (journal tag dmc12-uboot-env) and counted; more than 20 writes in a
# day needs --force (the flash is rated for >= 100,000 cycles: one save = one erase of one
# copy, so ~200,000 saves). Never call it from a timer or a loop.
{ pkgs, lib, ... }:
let
  # Only U-Boot's env tools (libc only). pkgs.ubootTools would add ~23 MiB (gnutls, p11-kit,
  # unbound, python cryptography) for tools the box never runs.
  tools = pkgs.stdenv.mkDerivation {
    pname = "uboot-envtools";
    inherit (pkgs.ubootTools) version src;
    depsBuildBuild = [ pkgs.buildPackages.stdenv.cc ];
    nativeBuildInputs = with pkgs.buildPackages; [ bison flex ];
    postPatch = "patchShebangs tools scripts";
    dontConfigure = true;
    makeFlags = [ "HOSTCC=$(CC_FOR_BUILD)" "CROSS_COMPILE=${pkgs.stdenv.cc.targetPrefix}" ];
    buildPhase = ''
      make $makeFlags tools-only_defconfig
      make $makeFlags -j$NIX_BUILD_CORES envtools
    '';
    installPhase = ''
      install -Dm755 tools/env/fw_printenv $out/bin/fw_printenv
      ln -s fw_printenv $out/bin/fw_setenv
    '';
  };
  envTool = pkgs.writeShellApplication {
    name = "dmc12-uboot-env";
    runtimeInputs = [ tools pkgs.coreutils pkgs.util-linux ];
    text = ''
      state=/var/lib/dmc12-uboot-env
      usage() { echo "usage: dmc12-uboot-env print [VAR...] | set [--force] VAR [VALUE...]" >&2; exit 2; }
      [ $# -ge 1 ] || usage
      cmd=$1; shift
      case $cmd in
        print) exec fw_printenv "$@" ;;
        set)
          force=0
          if [ "''${1:-}" = --force ]; then force=1; shift; fi
          [ $# -ge 1 ] || usage
          mkdir -p "$state"
          today=$(date +%F)
          n=$(grep -c "^$today " "$state/writes" 2>/dev/null || true)
          n=''${n:-0}   # no log yet: grep prints nothing
          if [ "$n" -ge 20 ] && [ $force = 0 ]; then
            echo "dmc12-uboot-env: $n env writes today already; this wears the SPI flash. Use --force if you mean it." >&2
            exit 1
          fi
          fw_setenv "$@"
          echo "$today $(date +%T) ''${SUDO_USER:-$USER} $1" >> "$state/writes"
          total=$(wc -l < "$state/writes")
          logger -t dmc12-uboot-env "set $1 by ''${SUDO_USER:-$USER} (Linux writes so far: $total)"
          ;;
        *) usage ;;
      esac
    '';
  };
  fwPrintenv = pkgs.runCommand "fw-printenv" { } ''
    mkdir -p $out/bin
    ln -s ${tools}/bin/fw_printenv $out/bin/fw_printenv
  '';
in
{
  # Linux-only (U-Boot builds use the same DTS: don't touch it for this). CS0's one read-only
  # partition becomes boot objects (ro) + the env window (rw) + the tail with the factory env (ro).
  hardware.deviceTree.overlays = [{
    name = "dmc12-uboot-env-partition";
    dtsText = ''
      /dts-v1/;
      /plugin/;
      / {
        compatible = "edgecore,dmc-12-48r-as0";
        fragment@0 {
          target-path = "/soc/io-bus@fc000000/spi@1882000/flash@0/partitions";
          __overlay__ {
            partition@0 { reg = <0x0 0xf90000>; };
            partition@f90000 { label = "u-boot-env"; reg = <0xf90000 0x20000>; };
            partition@fb0000 { label = "spi0-tail"; reg = <0xfb0000 0x50000>; read-only; };
          };
        };
      };
    '';
  }];

  # fw_env.config: device, offset, env size, erase block per copy. The redundant flag
  # byte is used because there are two lines.
  environment.etc."fw_env.config".text = ''
    /dev/mtd-u-boot-env 0x00000 0x10000 0x10000
    /dev/mtd-u-boot-env 0x10000 0x10000 0x10000
  '';
  # mtd numbers move with the DT; name the env partition (char device, not mtdNro).
  services.udev.extraRules = ''
    SUBSYSTEM=="mtd", KERNEL=="mtd[0-9]*", KERNEL!="*ro", ATTR{name}=="u-boot-env", SYMLINK+="mtd-u-boot-env"
  '';
  # The wrapper writes; fw_printenv (read-only) for habit. Raw fw_setenv is not on PATH.
  environment.systemPackages = [ envTool fwPrintenv ];
}
