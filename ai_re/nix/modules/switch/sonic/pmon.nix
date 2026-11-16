# SONiC's platform monitor (pmon) as NixOS services over a board's
# sonic_platform package (SONiC platform API 2.0): xcvrd (TRANSCEIVER_* in
# STATE_DB), thermalctld (FAN_INFO, TEMPERATURE_INFO; monitor only, see
# below), psud (PSU_INFO, CHASSIS_INFO), syseepromd (EEPROM_INFO), and the
# CLI scripts that read them: show platform fan|temperature|psustatus|syseeprom,
# show interfaces transceiver eeprom|presence|lpmode, sfputil.
# Fan control stays with the board (e.g. fand): thermalctld runs a fan policy
# only when the platform's Chassis.get_thermal_manager() returns one, and no
# thermal_policy.json is installed. Packages: ../../../packages/sonic/platform.
{ config, lib, pkgs, ... }:
let
  inherit (lib) mkOption mkEnableOption mkIf types listToAttrs nameValuePair;
  cfg = config.services.sonic;
  pcfg = cfg.pmon;
  plat = cfg.packages.platform;
  pmon = plat.pmon { platformPackages = [ pcfg.platformPackage ]; };
  cli = plat.cli { platformPackages = [ pcfg.platformPackage ]; };
in {
  options.services.sonic.pmon = {
    enable = mkEnableOption "SONiC's platform daemons (xcvrd, thermalctld, psud, syseepromd) and the show platform CLI";
    platformPackage = mkOption {
      type = types.package;
      description = "Python package providing sonic_platform (Platform().get_chassis()) for this board.";
    };
    daemons = mkOption {
      type = types.listOf (types.enum [ "xcvrd" "thermalctld" "psud" "syseepromd" ]);
      default = [ "xcvrd" "thermalctld" "psud" "syseepromd" ];
      description = "pmon daemons to run.";
    };
    args = mkOption {
      type = types.attrsOf (types.listOf types.str);
      default = { };
      example = { xcvrd = [ "--skip_cmis_mgr" ]; };
      description = "Extra arguments per daemon (e.g. xcvrd --skip_cmis_mgr on a read-only platform).";
    };
    environment = mkOption {
      type = types.attrsOf types.str;
      default = { };
      example = { AS7760_PLATFORM_ROOT = "/var/lib/as7760-fake"; };
      description = "Extra environment of the daemons (e.g. a board package's fake-hardware root in tests).";
    };
  };

  config = mkIf (cfg.enable && pcfg.enable) {
    environment.systemPackages = [ cli ];
    system.build.sonicPmon = { inherit pmon cli; };
    systemd.services = listToAttrs (map (d: nameValuePair "sonic-${d}" {
      description = "SONiC ${d} (pmon)";
      wantedBy = [ "sonic.target" ];
      partOf = [ "sonic.target" ];
      requires = [ "redis-sonic.service" "sonic-config-loader.service" ];
      after = [ "redis-sonic.service" "sonic-config-loader.service" ];
      # device_info.get_platform() reads PLATFORM before CONFIG_DB.
      environment = { PLATFORM = cfg.platform; } // pcfg.environment;
      unitConfig = { StartLimitIntervalSec = 300; StartLimitBurst = 10; };
      serviceConfig = {
        ExecStart = lib.concatStringsSep " " ([ "${pmon}/bin/${d}" ] ++ pcfg.args.${d} or [ ]);
        Restart = "always";
        RestartSec = 5;
      };
    }) pcfg.daemons);
  };
}
