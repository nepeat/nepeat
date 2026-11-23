# The U-Boot half of trial-boot: boot.scr (+ its source boot.cmd), for standard
# boot's script bootmeth. Protocol: ./default.nix.
{ lib, pkgs, loader, lba }:
let
  lbaHex = lib.toHexString lba;
  chain = lib.concatStringsSep " && ";
  # "TRIALBT1" as two little-endian words
  m0 = "0x41495254";
  m1 = "0x3154424c";
  cmd = pkgs.writeText "trial-boot.cmd" ''
    # trial-boot (nix/modules/profiles/trial-boot): one-shot request in block 0x${lbaHex}
    setenv tb_ok 0
    setenv tb_buf ''${kernel_addr_r}
    setexpr tb_b4 ''${tb_buf} + 4
    setexpr tb_chk ''${tb_buf} + 0x1000
    if ''${devtype} dev ''${devnum} && ''${devtype} read ''${tb_buf} ${lbaHex} 1; then
      if itest.l *''${tb_buf} == ${m0} && itest.l *''${tb_b4} == ${m1}; then setenv tb_ok 1; fi
    else
      echo "trial-boot: cannot read block ${lbaHex} of ''${devtype} ''${devnum}"
    fi
    if test "''${tb_ok}" = 1; then
      echo "trial-boot: trial requested"
      mw.l ''${tb_buf} 0 0x80
      mw.l ''${tb_chk} 0xffffffff 0x80
      if ''${devtype} write ''${tb_buf} ${lbaHex} 1 && ''${devtype} read ''${tb_chk} ${lbaHex} 1 && itest.l *''${tb_chk} == 0; then
        echo "trial-boot: request cleared"
        if ${chain loader.ubootArm}; then
          echo "trial-boot: watchdog armed, booting the trial"
          sysboot ''${devtype} ''${devnum}:${toString loader.confPartition} any ''${pxefile_addr_r} ${loader.confPath}
          echo "trial-boot: the trial did not boot, going on with the default"
          ${chain loader.ubootDisarm}
        else
          echo "trial-boot: could not arm the watchdog, going on with the default"
          ${chain loader.ubootDisarm}
        fi
      else
        echo "trial-boot: could not clear the request, going on with the default"
      fi
    else
      echo "trial-boot: no request"
      ${if loader.ubootDisarmOnDefault then chain loader.ubootDisarm else "true"}
    fi
    # fail, so standard boot goes on to the next bootflow: extlinux.conf, the default
    false
  '';
in
pkgs.runCommand "trial-boot-scr" { nativeBuildInputs = [ pkgs.buildPackages.ubootTools ]; } ''
  mkdir $out
  cp ${cmd} $out/boot.cmd
  mkimage -A arm64 -O linux -T script -C none -n trial-boot -d ${cmd} $out/boot.scr
''
