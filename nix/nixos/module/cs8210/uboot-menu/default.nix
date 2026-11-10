# CS8210 U-Boot bootmenu: the on-disk loader for the cs8210 hardware profile.
# Adapted from dmc12-nixos/uboot-menu (same idea, different U-Boot).
#
# Boot chain, with stock U-Boot 2018.05-alpine_db and no firmware changes
# other than the `nos_bootcmd` environment variable:
#
#   bootcmd -> run nos_bootcmd:
#       scsi init; ext4load scsi 0:2 <scriptAddr> /boot/boot.scr; source <scriptAddr>
#   /boot/boot.scr:
#       ext4load /boot/u-boot.menu; env import -t; bootmenu
#   bootmenu entry:
#       ext4load Image + initrd.uImage + dtb; setenv bootargs; booti
#
# If any of that fails, U-Boot returns to its stock bootcmd, which goes on to
# `run onie_bootcmd` -- ONIE in SPI flash stays the fallback.
#
# generic-extlinux-compatible is not usable here: `sysboot` hands the initrd
# to booti as raw "addr:size", which this U-Boot build rejects ("Wrong Ramdisk
# Image Format"). It needs a mkimage-wrapped ramdisk, so builder.sh makes one.
{
  config,
  lib,
  pkgs,
  ...
}:

let
  cfg = config.hardware.cs8210.boot;

  bootCmd = pkgs.writeText "cs8210-boot-cmd" ''
    echo "== CS8210 NixOS bootmenu =="
    scsi init
    if ext4load ${cfg.device} ${cfg.menuAddr} ${cfg.prefix}/u-boot.menu; then
      env import -t ${cfg.menuAddr} ''${filesize}
      bootmenu
    else
      echo "[boot.scr] ${cfg.prefix}/u-boot.menu missing on ${cfg.device}"
    fi
  '';

  bootScr =
    pkgs.runCommand "cs8210-boot-scr"
      {
        nativeBuildInputs = [ pkgs.buildPackages.ubootTools ];
      }
      ''
        mkimage -A arm64 -O linux -T script -C none -a 0 -e 0 \
          -n "CS8210 NixOS bootmenu launcher" \
          -d ${bootCmd} $out
      '';

  builder = pkgs.replaceVarsWith {
    src = ./builder.sh;
    isExecutable = true;
    replacements = {
      bash = pkgs.bash;
      path = lib.makeBinPath [
        pkgs.coreutils
        pkgs.gnused
        pkgs.ubootTools
      ];
      timeout = toString cfg.timeout;
      numGenerations = toString cfg.numGenerations;
      dtbName = config.hardware.deviceTree.name;
      bootScr = "${bootScr}";
      bootDev = cfg.device;
      bootPrefix = cfg.prefix;
      inherit (cfg) kernelAddr initrdAddr fdtAddr;
    };
  };
in
{
  options.hardware.cs8210.boot = {
    device = lib.mkOption {
      type = lib.types.str;
      default = "scsi 0:2";
      description = "U-Boot device:partition holding the staged boot files.";
    };
    prefix = lib.mkOption {
      type = lib.types.str;
      default = "/boot";
      description = "Path of the NixOS /boot directory as U-Boot sees it on that partition.";
    };
    timeout = lib.mkOption {
      type = lib.types.int;
      default = 5;
      description = "bootmenu_delay in seconds.";
    };
    numGenerations = lib.mkOption {
      type = lib.types.int;
      default = 8;
      description = ''
        Older generations to list. Every entry is imported into U-Boot's
        runtime environment, and the saved environment is already 13.7 of
        16 KiB, so keep this small.
      '';
    };
    # RAM layout. U-Boot itself sits at ~0x03ac0000-0x04000000. The Image is
    # ~73 MiB in memory and is booted in place (its header allows that), so
    # 0x08000000 leaves it clear of the FDT at 0x10000000.
    kernelAddr = lib.mkOption { type = lib.types.str; default = "0x08000000"; };
    fdtAddr = lib.mkOption { type = lib.types.str; default = "0x10000000"; };
    initrdAddr = lib.mkOption { type = lib.types.str; default = "0x20000000"; };
    scriptAddr = lib.mkOption { type = lib.types.str; default = "0x07000000"; };
    menuAddr = lib.mkOption { type = lib.types.str; default = "0x07100000"; };

    nosBootcmd = lib.mkOption {
      type = lib.types.str;
      readOnly = true;
      default = "scsi init; ext4load ${cfg.device} ${cfg.scriptAddr} ${cfg.prefix}/boot.scr; source ${cfg.scriptAddr}";
      description = "Value to store in U-Boot's `nos_bootcmd` (fw_setenv / setenv + saveenv).";
    };
  };

  config = {
    system.build.installBootLoader = lib.mkForce builder;
    system.boot.loader.id = lib.mkForce "cs8210-uboot-menu";

    # nix build .#nixosConfigurations.cs8210.config.system.build.cs8210UbootMenu
    system.build.cs8210UbootMenu = pkgs.runCommand "cs8210-uboot-menu" { } ''
      mkdir -p $out
      install -m 0755 ${builder} $out/install
      install -m 0644 ${bootCmd} $out/boot.cmd
      install -m 0644 ${bootScr} $out/boot.scr
    '';

    boot.loader.grub.enable = lib.mkForce false;
    boot.loader.systemd-boot.enable = lib.mkForce false;
    boot.loader.generic-extlinux-compatible.enable = lib.mkForce false;
    boot.loader.efi.canTouchEfiVariables = lib.mkForce false;
  };
}
