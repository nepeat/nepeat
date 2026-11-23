# DMC-12 hooks for profile-trial-boot (../../profiles/trial-boot): only when a
# system enables system.trialBoot; nothing changes otherwise.
#   request sector: LBA 2047 of the SSD, the last sector before sda1 (2048),
#     zero on u01 (2026-11-23); our U-Boot boots from that disk (scsi 0).
#   loader: our U-Boot's standard boot with bootmeths = "script extlinux", one
#     persistent env write by `trial-boot setup` (dmc12-uboot-env: the env window
#     0xf90000/0xfa0000, redundant; nothing else in SPI). Without it, or with the
#     script missing or failing, standard boot runs extlinux.conf as before.
#   watchdog: the board CPLD's (i2c-4 / 0x68, devices/dmc-12-48r-as0/cpld.md):
#     0x29 = load value / count, 1.034 s ticks, 0x28 b0 = run; at 0 the CPLD
#     resets the whole board (GSR), which also disarms it. Max 255 ticks (263 s).
#     Measured on u01 2026-11-23: writing 0x29 while counting does NOT restart the
#     count (it only sets the load value); writing 0x2a = 0 reloads it (the kick),
#     which also clears the wdt_fired flag (0x2a b1 = fans full after a WDT reset).
#     So a trial's kicks end that fan failsafe early: fand owns the fans from ~10 s.
#     The sp805 is left alone (SoC-only reset, and the kernel pings a running one).
{ config, lib, pkgs, ... }:
let
  cfg = config.system.trialBoot;
  bus = toString config.hardware.dmc12.cpld.bus;
  ticks = s: let t = (s * 1000 + 1033) / 1034; in if t > 255 then 255 else t;
  hex = n: "0x" + lib.toLower (lib.toHexString n);
  i2c = "${pkgs.i2c-tools}/bin/i2c";
  ubootEnv = "/run/current-system/sw/bin/dmc12-uboot-env";   # boards/dmc12/uboot-env
in
{
  imports = [ ../../profiles/trial-boot ];
  config = lib.mkIf cfg.enable {
    system.trialBoot = {
      flag = { device = lib.mkDefault "/dev/sda"; lba = lib.mkDefault 2047; };
      dtbName = lib.mkDefault "amazon/${config.hardware.alpineV2.dtbName}.dtb";
      loader = {
        confPartition = lib.mkDefault 2;   # DMC12ROOT (disk.nix)
        # U-Boot: bus 4 = mux70_3 by the DT aliases (as Linux)
        ubootArm = [
          "i2c dev ${bus}"
          "i2c mw 0x68 0x29 ${hex (ticks cfg.watchdog.bootTimeout)}"
          "i2c mw 0x68 0x28 1"
          "i2c read 0x68 0x28 1 \${tb_chk}"
          "itest.b *\${tb_chk} == 1"
        ];
        ubootDisarm = [ "i2c dev ${bus}" "i2c mw 0x68 0x28 0" ];
        # A panic=10 reboot of a trial is the sp805's SoC-only reset: the CPLD keeps counting.
        ubootDisarmOnDefault = lib.mkDefault true;
        check = ''[ "$(${ubootEnv} print -n bootmeths 2>/dev/null)" = "script extlinux" ]'';
        setup = ''${ubootEnv} set bootmeths "script extlinux"'';
      };
      verify.window = lib.mkDefault 180;   # boot to DHCP lease is ~40 s on u01
      watchdog = {
        arm = ''
          local t=$(( ("$1" * 1000 + 1033) / 1034 )); [ "$t" -le 255 ] || t=255
          ${i2c}set -y ${bus} 0x68 0x29 "$t" && ${i2c}set -y ${bus} 0x68 0x28 1 && ${i2c}set -y ${bus} 0x68 0x2a 0 \
            && [ "$(${i2c}get -y ${bus} 0x68 0x28)" = 0x01 ]
        '';
        kick = ''
          local t=$(( ("$1" * 1000 + 1033) / 1034 )); [ "$t" -le 255 ] || t=255
          ${i2c}set -y ${bus} 0x68 0x29 "$t" && ${i2c}set -y ${bus} 0x68 0x2a 0
        '';
        disarm = ''${i2c}set -y ${bus} 0x68 0x28 0 && [ "$(${i2c}get -y ${bus} 0x68 0x28)" = 0x00 ]'';
      };
    };
    assertions = [{
      assertion = (config.hardware.dmc12.bootLoader or null) == "standard";
      message = "dmc12 trial-boot needs hardware.dmc12.bootLoader = \"standard\" (our U-Boot's standard boot)";
    }];
  };
}
