# services.cs8210-fancontrol: temperature-driven fan PWM through the two fan
# CPLDs. Port of dmc12-nixos' fan-control module to this board's hardware; see
# fancontrol.py for the register contract and the fail-safe behaviour.
{
  config,
  lib,
  pkgs,
  ...
}:

let
  cfg = config.services.cs8210-fancontrol;
  pyEnv = pkgs.python3.withPackages (ps: [ ps.smbus2 ]);
  fanScript = pkgs.writeShellScriptBin "cs8210-fancontrol" ''
    exec ${pyEnv}/bin/python3 -u ${./fancontrol.py} "$@"
  '';
in
{
  options.services.cs8210-fancontrol = {
    enable = lib.mkEnableOption "CS8210 fan control daemon (fan CPLD PWM via I2C)";

    interval = lib.mkOption {
      type = lib.types.int;
      default = 10;
      description = "Seconds between temperature samples.";
    };
    lowTemp = lib.mkOption {
      type = lib.types.int;
      # The MACsec card LM75s idle at ~38 C with the PHYs configured; the
      # vendor's board warning threshold is 53 C.
      default = 40;
      description = "Hottest-sensor temperature (C) at or below which fans run at minPwm.";
    };
    highTemp = lib.mkOption {
      type = lib.types.int;
      default = 55;
      description = "Hottest-sensor temperature (C) at or above which fans run at full speed.";
    };
    asicLowTemp = lib.mkOption {
      type = lib.types.int;
      default = 70;
      description = "Switch ASIC die temperature (C) at or below which it asks for no more than minPwm.";
    };
    asicHighTemp = lib.mkOption {
      type = lib.types.int;
      default = 95;
      description = "Switch ASIC die temperature (C) at or above which fans run at full speed (vendor limit 105).";
    };
    minPwm = lib.mkOption {
      type = lib.types.ints.between 0 255;
      default = 127;
      description = ''
        PWM floor (0-255). 127 is the vendor's own quiet preset (~50%, about
        11000 rpm). Going lower is untested on this chassis.
      '';
    };
  };

  config = lib.mkIf cfg.enable {
    environment.systemPackages = [ fanScript ];

    systemd.services.cs8210-fancontrol = {
      description = "CS8210 fan control daemon (fan CPLD PWM via I2C)";
      wantedBy = [ "multi-user.target" ];
      after = [ "systemd-modules-load.service" ];
      # Needs the i2c-pld controller; without it the CPLDs keep their
      # power-on default (full speed, watchdog armed).
      unitConfig.ConditionPathExists = "/sys/bus/platform/devices/fd880000.i2c";
      serviceConfig = {
        ExecStart = lib.concatStringsSep " " [
          "${fanScript}/bin/cs8210-fancontrol"
          "--interval ${toString cfg.interval}"
          "--low-temp ${toString cfg.lowTemp}"
          "--high-temp ${toString cfg.highTemp}"
          "--min-pwm ${toString cfg.minPwm}"
          "--asic-low-temp ${toString cfg.asicLowTemp}"
          "--asic-high-temp ${toString cfg.asicHighTemp}"
        ];
        Restart = "always";
        RestartSec = "5s";
      };
    };
  };
}
