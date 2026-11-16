# CS8210-32X SONiC platform integration: this board's sonic_platform
# (./sonic_platform, read only in this phase) for the pmon daemons
# (services.sonic.pmon: xcvrd, thermalctld monitor-only, psud, syseepromd)
# and LLDP (services.sonic.lldp). Everything is mkDefault and nothing is set
# until services.sonic.enable. Sources per object: sonic_platform/__init__.py.
{ config, lib, pkgs, ... }:
let
  inherit (lib) mkOption mkDefault mkIf types;
  cfg = config.services.sonic;
  pcfg = config.hardware.cs8210.sonicPlatform;
  platformPackage = pkgs.callPackage ./sonic-platform-package.nix { sonic = cfg.packages; };
  # fand's curve ends (nix/nixos/module/cs8210/fan-control, when imported)
  fan = config.hardware.cs8210.fan or { };
in {
  options.hardware.cs8210.sonicPlatform = {
    asicTemperatureFile = mkOption {
      type = types.str;
      default = "/run/asic/temperature";
      description = ''
        ASIC die temperature file syncd publishes (sai.profile
        SAI_OPENBCM_TEMP_FILE, set here with mkDefault); the platform
        reads it (CS8210_ASIC_TEMP_FILE) when fand has no fresh "asic" value.
      '';
    };
    qsfpPrepare = mkOption {
      type = types.bool;
      default = false;
      description = ''
        Let the cage code do what `cs8210-qsfp --enable --init P` does
        (CS8210_QSFP_PREPARE=1): turn on the I2C FPGA's PCI memory decode
        (a write to its PCI command register) when it is off, and write the
        vendor probe values to a channel still at power-up values before
        using it. Off: cages read "not present" while decode is off, and an
        uninitialised channel's EEPROM is unreadable. Module registers,
        LPMode and ResetL are never written either way.
      '';
    };
  };

  config = mkIf cfg.enable {
    services.sonic.pmon = {
      enable = mkDefault true;
      platformPackage = mkDefault platformPackage;
      # Read-only platform: CMIS state machine writes are refused (it would retry forever).
      args.xcvrd = mkDefault [ "--skip_cmis_mgr" ];
      # No xcvrd by default: in the first full run on the chip (2026-11-16) it
      # took 114 % CPU with 55 % of all CPU time in hard IRQs, and SSH to the
      # box timed out until the stack stopped (devices/cs8210-32x/sonic.md).
      daemons = mkDefault [ "thermalctld" "psud" "syseepromd" ];
      environment = {
        CS8210_ASIC_TEMP_FILE = pcfg.asicTemperatureFile;
        CS8210_BOARD_HIGH_C = toString (fan.highTemp or 55);
        CS8210_ASIC_HIGH_C = toString (fan.asicHighTemp or 95);
        CS8210_ASIC_CRIT_C = toString (fan.asicCriticalC or 100);
      } // lib.optionalAttrs pcfg.qsfpPrepare { CS8210_QSFP_PREPARE = "1"; };
    };
    services.sonic.lldp.enable = mkDefault true;
    services.sonic.sai.profile.SAI_OPENBCM_TEMP_FILE = mkDefault pcfg.asicTemperatureFile;
    systemd.tmpfiles.rules = [ "d ${dirOf pcfg.asicTemperatureFile} 0755 root root -" ];
    system.build.cs8210SonicPlatform = platformPackage;
  };
}
