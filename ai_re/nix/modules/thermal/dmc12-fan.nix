# DMC-12-48R-AS0 fan control: three fans on one PWM step register of the
# board CPLD (I2C, raw from userspace, no kernel driver). Register facts:
# ai_re/devices/dmc-12-48r-as0/vendor-tooling.md.
#
# The controller is fand (rust/crates/fand, `type: i2c`): failsafe on any fault, exit duty
# (100 %) on stop, WDT-flag handshake, tach checks. It comes from the monorepo's rust flake,
# which only the ai_re root flake (git+file://<monorepo>?dir=ai_re, deploy.sh) passes in; a
# `path:./nix` build has none and fails the assertion below instead of running without
# fan control. (The ported factory PI script was retired 2026-11-15 after fand ran on u01.)
{ config, lib, pkgs, ... }:
let
  rust = import ../rust-modules.nix;   # fand from the monorepo tree (null under path:./nix)
  cfg = config.hardware.dmc12Fan;
  # The CPLD module's bus (pinned by the board DT aliases), else 4.
  bus = config.hardware.dmc12.cpld.bus or 4;
  # duty% per step: v7+ CPLD 4.6 + (step-1) * 1.54 (1..63), older
  # 17.6 + (step-1) * 5.88 (1..15); 0 = off. From the factory `show`.
  v7 = cfg.pwmSteps == 64;
  top = if v7 then 63 else 15;
  stepPct = if v7 then 95.4 / 62 else 82.4 / 14;
  hex = n: "0x${lib.toLower (lib.toHexString n)}";
  sensor = name: device: { type = "hwmon"; hwmon = { inherit name device; }; plausible = [ (-10) 110 ]; };
  promDir = config.monitoring.nodeExporter.textfile.directory or null;
  prom = (config.monitoring.nodeExporter.enable or false) && promDir != null;
in {
  imports = lib.optional (rust != null) rust.nixosModules.fand;

  options.hardware.dmc12Fan = {
    pwmSteps = lib.mkOption {
      type = lib.types.enum [ 64 16 ];
      default = 64;
      description = ''
        PWM steps of the CPLD: 64 when its version register 0x01 is above 134
        ("v7+", u01 = 0x88), else 16. fand checks the version and writes
        nothing (zone failsafe) when it does not match.
      '';
    };
    minStep = lib.mkOption {
      type = lib.types.ints.between 1 63;
      default = 9;
      description = ''
        Lowest PWM step while running (hardware minimum). 9 = 16.9 % on a
        64-step CPLD, ~2000 rpm: the AS0 chassis raises fan alarms below that
        (factory >= 1.0.526).
      '';
    };
    fans = lib.mkOption {
      type = lib.types.ints.between 1 3;
      default = 3;
      description = "Fans with a tach (DN0 chassis have no fan 3).";
    };
    targetC = lib.mkOption {
      type = lib.types.int;
      default = 45;
      description = "Setpoint (C) of the PI loop on the hottest board sensor (factory: 45).";
    };
    criticalC = lib.mkOption {
      type = lib.types.int;
      default = 60;
      description = "Board sensor (C) at which fand goes failsafe (100 %).";
    };
    # The K2 uC (CPU proxy) is passively cooled: on u01 it sat at 52.5 C at the
    # 2000 RPM floor and 50 C at 5700 RPM (step test, 2026-11-15). A 45 C target
    # on it would pin the fans at 100 % for ~2.5 C. Owner: keep the CPU < 60 C
    # to start (maybe 70); tune from data.
    cpuTargetC = lib.mkOption {
      type = lib.types.int;
      default = 60;
      description = "Setpoint (C) for the K2 uC sensor; board sensors use targetC.";
    };
    cpuCriticalC = lib.mkOption {
      type = lib.types.int;
      default = 75;
      description = "K2 uC temperature (C) at which fand goes failsafe (its LM75 alarm is 80/75).";
    };
  };

  config = lib.mkMerge [
    {
      assertions = [{
        assertion = rust != null;
        message = ''
          DMC-12 fan control is fand, from the monorepo's rust flake, which this
          evaluation lacks (a `path:./nix` build can't reach ../rust). Build from the
          ai_re root flake: git+file://<monorepo>?dir=ai_re#... (deploy.sh does).
        '';
      }];
      # /dev/i2c-N (built in on linux-alpine; harmless there).
      boot.kernelModules = [ "i2c-dev" ];
    }
    (lib.optionalAttrs (rust != null) {
      # services.fand exists only when the fand module is imported (../rust-modules.nix).
      # On u01 since 2026-11-15 (devices/dmc-12-48r-as0/PROGRESS.md).
      services.fand = {
        enable = true;
        # No extra floor: the hardware minimum (minStep) is the floor.
        profile = "none";
        extraReadWritePaths = lib.optional prom promDir;
        settings = {
          interval = "2s";
          calm_period = "60s";
          sensors = {
            # Labels: boards/dmc12/conf/dmc12_sensors.conf.
            k2_uc = sensor "lm75" "5-004a" // { critical = cfg.cpuCriticalC; };
            sfp_console = sensor "tmp75" "7-0048" // { critical = cfg.criticalC; };
            m2 = sensor "tmp75" "7-0049" // { critical = cfg.criticalC; };
          };
          outputs.fans = {
            type = "i2c";
            i2c = { inherit bus; address = "0x68"; register = "0x50"; };
            map = { points = [ [ 1 (if v7 then 4.6 else 17.6) ] [ top 100 ] ]; min = lib.min cfg.minStep top; };
            # The step map above holds only for this CPLD generation.
            expect = [ ({ register = "0x01"; } // (if v7 then { min = 135; } else { max = 134; })) ];
            # WDT flag: 3 after a reset (CPLD forces full speed), 1 once
            # cleared; bit 1 is the takeover. Writing 0 is what U-Boot does on
            # every boot.
            arm = { register = "0x2a"; mask = "0x02"; value = 0; };
            tach = {
              i2c = lib.genList (n: { register = hex (96 + n); label = "fan${toString (n + 1)}"; }) cfg.fans;
              scale = 100;
              max_rpm = 14400;    # ~0x90 at full speed (u01)
              min_rpm = 1000;     # a stalled fan reads 0; ~2000 at the floor
              # Not linear (u01: ~2000 @ 16.9 %, 3200 @ 36.9 %, 8700 @ 67.7 %,
              # 14400 @ 100 %): 3200 is 60 % of the linear model, so the
              # default 0.35 would fault mid-range. 0.6 still flags a fan
              # below 40 % of it; min_rpm catches a stall.
              follow_tolerance = 0.6;
            };
          };
          zones.board = {
            outputs = [ "fans" ];
            # Per-sensor setpoint: the zone tracks whichever sensor is furthest
            # above its own target (board sensors: targetC).
            sensors = [ { sensor = "k2_uc"; target = cfg.cpuTargetC; } "sfp_console" "m2" ];
            # The factory PI (Kp 3 steps/C, Ki 0.1 steps/(C * 10 s)) in percent.
            policy = { target = cfg.targetC; kp = 3 * stepPct; ki = 0.01 * stepPct; };
          };
        } // lib.optionalAttrs prom { runtime.prometheus_file = "${promDir}/fand.prom"; };
      };
    })
  ];
}
