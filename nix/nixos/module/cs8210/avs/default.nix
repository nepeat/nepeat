# hardware.cs8210.asicAvs: the Teralynx 7 core voltage (AVS) the vendor U-Boot
# sets on every boot (main_loop hook 0x01104600), as a Linux oneshot that runs
# before the switch stack: four ASIC fuse words (BAR0 + 0x11178) -> RoV + lot
# ID -> VDD core 760..840 mV -> PMBus VOUT_COMMAND of the "vddcore" TPS53688
# (0x62 behind the kernel muxes 0x70:2 -> 0x74:0 -> 0x72:2). Writes only when
# the regulator differs, records /run/cs8210-avs.done; syncd (../sonic) and
# cs8210-switch (../vendor-sdk) Require= it. Decode, table, chip test:
# ai_re/devices/cs8210-32x/avs.md.
#
# The fuse read needs the BAR unclaimed (IO_STRICT_DEVMEM refuses a sysfs
# resource0 mmap while ipd holds it), so ipd is blacklisted for udev's alias
# autoload and loaded here, after the AVS step. A later run with ipd bound
# (manual restart, a deploy onto a running box) takes the fuse words from this
# boot's state file or the last BAR read (/var/lib/cs8210-asic-avs/fuses).
{
  config,
  lib,
  pkgs,
  ...
}:

let
  cfg = config.hardware.cs8210.asicAvs;
  vendorSdk = config.hardware.cs8210.vendorSdk.enable;
  mon = config.monitoring.nodeExporter;

  avs =
    pkgs.runCommand "cs8210-asic-avs"
      {
        nativeBuildInputs = [ pkgs.buildPackages.python3 ];
      }
      ''
        # unit tests: this unit's fuse vector (vendor U-Boot log), every RoV /
        # lot branch, the vendor's closest-code step, the PMBus whitelist
        python3 -I ${./test_avs.py} ${./cs8210-asic-avs.py}
        install -D -m 0755 ${./cs8210-asic-avs.py} $out/bin/cs8210-asic-avs
        sed -i '1s|.*|#!${pkgs.python3}/bin/python3 -I|' $out/bin/cs8210-asic-avs
      '';
in
{
  options.hardware.cs8210.asicAvs = {
    enable = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = ''
        Set the switch ASIC core voltage from its fuses before the switch stack
        starts (cs8210-asic-avs.service), as the vendor U-Boot does. syncd and
        cs8210-switch require it: if it fails (fuse read, decode outside
        760..840 mV, unexpected regulator, VOUT_MODE not linear -10, read-back
        mismatch) neither starts, and ipd is not loaded.
      '';
    };
    package = lib.mkOption {
      type = lib.types.package;
      default = avs;
      defaultText = lib.literalMD "the tool from ./cs8210-asic-avs.py (unit tests run at build)";
      readOnly = true;
      description = "cs8210-asic-avs (apply / status / metrics / decode).";
    };
  };

  config = lib.mkIf cfg.enable {
    environment.systemPackages = [ avs ];
    boot.kernelModules = [ "i2c-dev" ];
    boot.blacklistedKernelModules = lib.mkIf vendorSdk [ "ipd" ];

    systemd.services.cs8210-asic-avs = {
      description = "CS8210: switch ASIC core voltage (AVS) from its fuses, as the vendor U-Boot";
      wantedBy = [ "multi-user.target" ];
      # All-ones instead of an SError if the ASIC does not answer the fuse read
      # (the tool also checks the root port itself).
      wants = [ "cs8210-pcie-errors.service" ];
      after = [
        "cs8210-pcie-errors.service"
        "systemd-modules-load.service"
      ];
      # A restart would restart syncd / cs8210-switch through their Requires=.
      restartIfChanged = false;
      serviceConfig = {
        Type = "oneshot";
        RemainAfterExit = true;
        ExecStart = "${avs}/bin/cs8210-asic-avs apply";
        # ipd after the fuse read: its BAR claim blocks the resource0 mmap.
        # Options (legacy_abi, psample) from modprobe.d (../vendor-sdk).
        ExecStartPost = lib.optional vendorSdk "${pkgs.kmod}/bin/modprobe ipd";
        StateDirectory = "cs8210-asic-avs";
        TimeoutStartSec = "120s";
      };
    };

    # Regulator readings (VOUT_COMMAND, READ_VOUT/IOUT/TEMPERATURE_1/VIN/POUT,
    # STATUS_WORD) and the recorded target: cs8210_asic_avs_* (reads only).
    monitoring.nodeExporter.textfile.producers.cs8210-asic-avs = lib.mkIf mon.enable {
      command = "${avs}/bin/cs8210-asic-avs metrics";
      interval = 30;
      timeout = 10;
      serviceConfig.LogLevelMax = "notice";
    };
  };
}
