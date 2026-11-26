# CS8210-32X SONiC platform integration: this board's sonic_platform
# (./sonic_platform; its cages through cs8210-optics, the Rust daemon that owns
# the I2C FPGA) for the pmon daemons
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
  };

  config = mkIf cfg.enable {
    services.sonic.pmon = {
      enable = mkDefault true;
      platformPackage = mkDefault platformPackage;
      # Module bring-up (power class, CMIS application select) is cs8210-optics' (the
      # I2C FPGA's owner, nix/nixos/module/cs8210/optics in the monorepo): xcvrd's CMIS
      # manager would be a second state machine on the same modules.
      args.xcvrd = mkDefault [ "--skip_cmis_mgr" ];
      environment = {
        CS8210_ASIC_TEMP_FILE = pcfg.asicTemperatureFile;
        CS8210_BOARD_HIGH_C = toString (fan.highTemp or 55);
        CS8210_ASIC_HIGH_C = toString (fan.asicHighTemp or 95);
        CS8210_ASIC_CRIT_C = toString (fan.asicCriticalC or 100);
      };
    };
    services.sonic.lldp.enable = mkDefault true;
    services.sonic.sai.profile.SAI_OPENBCM_TEMP_FILE = mkDefault pcfg.asicTemperatureFile;
    systemd.tmpfiles.rules = [ "d ${dirOf pcfg.asicTemperatureFile} 0755 root root -" ];
    system.build.cs8210SonicPlatform = platformPackage;
  };
}
