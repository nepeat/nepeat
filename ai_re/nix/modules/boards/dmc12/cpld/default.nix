# DMC-12 board CPLD (Lattice MachXO3, I2C bus 4 / 0x68, raw from userspace).
# Register map and evidence: ai_re/devices/dmc-12-48r-as0/vendor-tooling.md.
#   - board reset on `reboot` (the factory kernel's al-dmc12-reboot driver)
#   - interrupt monitor: alarms to the journal, uplink PHY re-sync
{ config, lib, pkgs, ... }:
let
  cfg = config.hardware.dmc12.cpld;

  pyEnv = pkgs.python3.withPackages (ps: [ ps.smbus2 ps.libgpiod ]);
  interruptmon = pkgs.writeScriptBin "dmc12-interruptmon" ''
    #!${pyEnv}/bin/python3
    ${builtins.readFile ./interruptmon.py}
  '';
in
{
  options.hardware.dmc12.cpld = {
    bus = lib.mkOption {
      type = lib.types.int;
      default = 4;
      description = "Linux I2C bus of the CPLD (pinned by the board DT aliases).";
    };
    reboot = lib.mkEnableOption "full board reset through the CPLD on reboot" // { default = true; };
    interruptmon = {
      enable = lib.mkEnableOption "CPLD interrupt monitor (alarms + uplink re-sync)" // { default = true; };
      alarms = lib.mkOption {
        type = lib.types.listOf (lib.types.enum [ "temperature" "fans" "psu1" "psu2" ]);
        default = [ "temperature" "fans" "psu1" "psu2" ];
        description = "Alarms to log; a missing PSU on a single-PSU box can be left out.";
      };
    };
  };

  config = lib.mkMerge [
    (lib.mkIf cfg.reboot {
      # Kernel reboot notifier (packages/dmc12-cpld-reset). u01 2026-11-15: a
      # systemd-shutdown hook never ran in NixOS's exitrd, although the same
      # script run by hand resets the board (CPLD WDT flag 0x03 afterwards).
      boot.extraModulePackages = [ (config.boot.kernelPackages.callPackage ../../../../packages/dmc12-cpld-reset { }) ];
      boot.kernelModules = [ "dmc12_cpld_reset" ];
      boot.extraModprobeConfig = "options dmc12_cpld_reset bus=${toString cfg.bus}";
    })
    (lib.mkIf cfg.interruptmon.enable {
      environment.systemPackages = [ interruptmon ];
      systemd.services.dmc12-interruptmon = {
        description = "DMC-12 CPLD interrupt monitor";
        wantedBy = [ "multi-user.target" ];
        after = [ "dmc12-onieeeprom.service" ];
        wants = [ "dmc12-onieeeprom.service" ];
        unitConfig.ConditionPathExists = "/dev/i2c-${toString cfg.bus}";
        serviceConfig = {
          ExecStart = lib.concatStringsSep " " ([
            "${interruptmon}/bin/dmc12-interruptmon"
            "--bus" (toString cfg.bus)
            # dmc12-tools puts setphyspeed in the system profile.
            "--setphyspeed" "/run/current-system/sw/bin/setphyspeed"
          ] ++ map (a: "--no-${a}") (lib.subtractLists cfg.interruptmon.alarms [ "temperature" "fans" "psu1" "psu2" ]));
          Restart = "on-failure";
          RestartSec = "10s";
        };
      };
    })
  ];
}
