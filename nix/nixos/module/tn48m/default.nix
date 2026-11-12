# Delta TN48M hardware profile (48x1G + 4x10G SFP+, Marvell Armada 7040 +
# Prestera 98DX3265 AC3X).
#
# Vendored from https://github.com/andrewhamon/delta-tn48m at 0ab0f8d (MIT,
# see LICENSE.delta-tn48m): hardware.nix, dtb.nix, thermal.nix, watchdog.nix
# and the board DTS (GPL-2.0-or-later OR X11 notice kept in the file). That
# port was validated on a TN48M-P. What differs here:
#
#   * One module with options under hardware.tn48m instead of
#     hardware.deltaTn48m.*; no PoE/PD69200 or PMBus PSU support (the plain
#     TN48M has neither -- dentOS's factory boot instantiated no PMBus devices).
#   * Only the Prestera firmware directory is shipped, not all of
#     linux-firmware.
#   * Fan floor defaults higher: upstream's PWM 48 was measured on aftermarket
#     Noctua fans; this unit has stock Delta fans (to be measured).
#
# Everything the board needs is in nixpkgs' stock aarch64 linuxPackages_latest
# (7.3.3 at the time of writing: MFD_SIMPLE_MFD_I2C=y, GPIO/RESET_TN48M_CPLD,
# PRESTERA_PCI, MVPP2, SFP, NVMEM_LAYOUT_ONIE_TLV, SENSORS_ADT7475, SBSA wdt,
# TCG_TIS_SPI), and it is in cache.nixos.org, so no custom kernel.
#
# Boot: factory U-Boot `sysboot` reads /boot/extlinux/extlinux.conf (raw
# Image, not a FIT -- U-Boot requires Amazon-signed FITs, but booti/sysboot of
# a raw Image is not checked). See nepeat/ai_re/devices/delta-tn48m/uboot.md.
#
# RE notes: nepeat/ai_re/devices/delta-tn48m/PROGRESS.md
{
  config,
  lib,
  pkgs,
  ...
}:

let
  cfg = config.hardware.tn48m;

  # Where the EFI path keeps the board DTB on the ESP (read by U-Boot's
  # nos_bootcmd, see machine/tn48m).
  efiDtbPath = "dtbs/delta-tn48m.dtb";

  presteraFirmware = pkgs.runCommand "prestera-firmware" { } ''
    mkdir -p $out/lib/firmware/mrvl
    cp -r ${pkgs.linux-firmware}/lib/firmware/mrvl/prestera $out/lib/firmware/mrvl/
  '';

  # thermal (upstream thermal.nix): fancontrol on the ADT7473, all three PWM
  # channels following the CP0 on-die sensor.
  fan = "/run/tn48m-fancontrol/adt7473";
  cp0Temperature = "/sys/devices/virtual/thermal/thermal_zone5/temp";
  pwm = channel: "${fan}/pwm${toString channel}";
  tach = channel: "${fan}/fan${toString channel}_input";
  channels = [
    1
    2
    3
  ];
  assignments =
    makeValue:
    lib.concatMapStringsSep " " (channel: "${pwm channel}=${toString (makeValue channel)}") channels;

  resolveAdt7473 = pkgs.writeShellScript "tn48m-resolve-adt7473" ''
    set -eu
    for attempt in $(${pkgs.coreutils}/bin/seq 1 30); do
      for candidate in /sys/bus/i2c/devices/1-002e/hwmon/hwmon*; do
        if [ -r "$candidate/name" ] && [ "$(<"$candidate/name")" = adt7473 ]; then
          ${pkgs.coreutils}/bin/ln -sfn "$candidate" ${fan}
          exit 0
        fi
      done
      ${pkgs.coreutils}/bin/sleep 1
    done
    echo "ADT7473 not found at I2C 1-002e after 30 seconds; fans stay at the hardware default PWM" >&2
    exit 1
  '';
in
{
  options.hardware.tn48m = {
    boot = lib.mkOption {
      type = lib.types.enum [
        "efi"
        "extlinux"
      ];
      default = "efi";
      description = ''
        How the factory U-Boot starts NixOS. "efi": `bootefi` of systemd-boot
        from the ESP, with the DTB at /${efiDtbPath} on the ESP. "extlinux":
        `sysboot` of /boot/extlinux/extlinux.conf (upstream's tested path).
      '';
    };

    thermal = {
      enable = lib.mkEnableOption "TN48M chassis fan control (lm-sensors fancontrol on the ADT7473)";

      minimumPwm = lib.mkOption {
        type = lib.types.ints.between 24 254;
        default = 96;
        description = ''
          Minimum chassis-fan PWM (0-255). Upstream's 48 was measured on
          Noctua 40 mm fans; 96 is a cautious placeholder for the stock fans
          until their stall/restart floor is measured on this unit.
        '';
      };

      minimumTemperature = lib.mkOption {
        type = lib.types.ints.between 0 100;
        default = 78;
        description = "CP0 temperature (C) at or below which minimumPwm is used.";
      };

      maximumTemperature = lib.mkOption {
        type = lib.types.ints.between 1 110;
        default = 90;
        description = "CP0 temperature (C) at or above which full PWM is used.";
      };

      pollInterval = lib.mkOption {
        type = lib.types.ints.between 2 60;
        default = 5;
        description = "Seconds between fancontrol samples.";
      };
    };

    watchdog = {
      enable = lib.mkEnableOption "the Armada 7040 SBSA hardware watchdog via systemd (fixed 10 s timeout)";
      timeout = lib.mkOption {
        type = lib.types.str;
        default = "10s";
        description = "systemd RuntimeWatchdogSec; the SBSA watchdog's hardware timeout is a fixed 10 s.";
      };
    };
  };

  config = lib.mkMerge [
    {
      # Plain priority: base.nix sets an mkDefault x86_64-linux (as for cs8210).
      nixpkgs.hostPlatform = "aarch64-linux";

      # base.nix turns this on for every host; here it would drag in all of
      # linux-firmware (and Intel microcode). The ASIC firmware is all we need.
      hardware.enableRedistributableFirmware = lib.mkForce false;
      # base.nix also enables x86 microcode unconditionally (cs8210 does the same).
      hardware.cpu.intel.updateMicrocode = lib.mkForce false;
      hardware.cpu.amd.updateMicrocode = lib.mkForce false;
      hardware.firmware = [ presteraFirmware ];

      boot.initrd.kernelModules = [
        "marvell"
        "mvpp2"
        "ext4"
        "vfat"
        "usb_storage"
        "xhci_hcd"
        "ahci"
        "sd_mod"
      ];

      boot.kernelModules = [
        "prestera_pci"
        "tpm_tis_spi"
        "at24"
        "i2c_mux_gpio"
        "spi_nor"
        "onie_tlv"
        "gpio_tn48m"
        "reset_tn48m"
      ];

      # /dev/mtd1 is the U-Boot env (64 KiB at SPI 0x3f0000, 4 KiB sectors).
      environment.etc."fw_env.config".text = ''
        /dev/mtd1 0x0 0x10000 0x1000
      '';

      boot.kernelParams = [
        "console=ttyS0,115200n8"
        "earlycon=uart8250,mmio32,0xf0512000"
      ];

      boot.loader.grub.enable = lib.mkForce false;

      hardware.deviceTree = {
        enable = true;
        dtbSource = pkgs.callPackage ./dtb.nix {
          kernelSrc = config.boot.kernelPackages.kernel.src;
        };
        name = "marvell/delta-tn48m.dtb";
      };

      environment.systemPackages = with pkgs; [
        ubootTools # fw_printenv / fw_setenv
        mtdutils
        ethtool
        pciutils
        usbutils
        i2c-tools
        lm_sensors
        libgpiod
        tpm2-tools
      ];
    }

    (lib.mkIf (cfg.boot == "efi") {
      # Factory U-Boot 2018.03 `bootefi` -> systemd-boot. U-Boot's EFI has no
      # persistent variables, so systemd-boot runs without touching them and
      # just follows loader.conf. The DTB is NOT installed by systemd-boot:
      # U-Boot passes it to `bootefi` from the fixed path below so its fixups
      # (mvpp2 MACs, memory banks) are applied.
      boot.loader.systemd-boot = {
        enable = true;
        graceful = true;
        installDeviceTree = false;
        extraFiles.${efiDtbPath} = "${config.hardware.deviceTree.package}/${config.hardware.deviceTree.name}";
      };
      boot.loader.efi.canTouchEfiVariables = lib.mkForce false;
      boot.loader.efi.efiSysMountPoint = "/boot";
      boot.loader.generic-extlinux-compatible.enable = false;
    })

    (lib.mkIf (cfg.boot == "extlinux") {
      boot.loader.systemd-boot.enable = lib.mkForce false;
      boot.loader.efi.canTouchEfiVariables = lib.mkForce false;
      boot.loader.generic-extlinux-compatible.enable = true;
    })

    (lib.mkIf cfg.thermal.enable {
      assertions = [
        {
          assertion = cfg.thermal.minimumTemperature < cfg.thermal.maximumTemperature;
          message = "hardware.tn48m.thermal.minimumTemperature must be below maximumTemperature";
        }
      ];

      boot.kernelModules = [
        "adt7475"
        "lm75"
      ];

      hardware.fancontrol = {
        enable = true;
        config = ''
          INTERVAL=${toString cfg.thermal.pollInterval}
          FCTEMPS=${assignments (_: cp0Temperature)}
          FCFANS=${lib.concatMapStringsSep " " (channel: "${pwm channel}=${tach channel}") channels}
          MINTEMP=${assignments (_: cfg.thermal.minimumTemperature)}
          MAXTEMP=${assignments (_: cfg.thermal.maximumTemperature)}
          MINSTART=${assignments (_: cfg.thermal.minimumPwm)}
          MINSTOP=${assignments (_: cfg.thermal.minimumPwm)}
          MINPWM=${assignments (_: cfg.thermal.minimumPwm)}
          MAXPWM=${assignments (_: 255)}
          AVERAGE=${assignments (_: 1)}
        '';
      };

      systemd.services.fancontrol = {
        unitConfig = {
          StartLimitIntervalSec = 120;
          StartLimitBurst = 6;
        };
        serviceConfig = {
          ExecStartPre = resolveAdt7473;
          RestartSec = 5;
          RuntimeDirectory = "tn48m-fancontrol";
          RuntimeDirectoryMode = "0755";
        };
      };
    })

    (lib.mkIf cfg.watchdog.enable {
      systemd.settings.Manager.RuntimeWatchdogSec = cfg.watchdog.timeout;
    })
  ];
}
