# NixOS module for fand. Self-contained: import this file, then e.g.
#
#   services.fand = {
#     enable = true;
#     profile = "medium";
#     settings = { sensors = { ... }; outputs = { ... }; zones = { ... }; };
#   };
#
# settings is the YAML configuration (see rust/crates/fand/README.md) as a
# Nix attrset; it is rendered to /etc/fand/config.yaml and checked at build
# time with "fand check --syntax-only".
{
  config,
  lib,
  pkgs,
  ...
}:
let
  inherit (lib) mkOption types;
  cfg = config.services.fand;
  yaml = pkgs.formats.yaml { };

  rendered = yaml.generate "fand-config.yaml" cfg.settings;
  # The check needs a fand that runs on the build machine.
  checker =
    if pkgs.stdenv.buildPlatform.canExecute pkgs.stdenv.hostPlatform then cfg.package else cfg.checkPackage;
  configFile =
    if cfg.checkConfig then
      pkgs.runCommand "fand-config.yaml" { nativeBuildInputs = [ checker ]; } ''
        fand check --syntax-only --config ${rendered}
        cp ${rendered} $out
      ''
    else
      rendered;

  exe = lib.getExe cfg.package;
  promFile = (cfg.settings.runtime or { }).prometheus_file or null;
in
{
  options.services.fand = {
    enable = lib.mkEnableOption "fand, the fan control daemon";

    package = mkOption {
      type = types.package;
      default = pkgs.callPackage ./package.nix { };
      defaultText = lib.literalExpression "pkgs.callPackage ./package.nix { }";
      description = "The fand package.";
    };

    checkPackage = mkOption {
      type = types.package;
      default = pkgs.buildPackages.callPackage ./package.nix { };
      defaultText = lib.literalExpression "pkgs.buildPackages.callPackage ./package.nix { }";
      description = "fand for the build machine, used for the build-time config check when cross compiling.";
    };

    checkConfig = mkOption {
      type = types.bool;
      default = true;
      description = "Validate the generated configuration at build time (fand check --syntax-only).";
    };

    profile = mkOption {
      type = types.nullOr (types.either types.str (types.ints.between 0 100));
      default = null;
      example = "fast";
      description = ''
        Global minimum-speed profile applied after every zone's policy:
        "none", "slow", "medium", "fast", a percentage, or a speed such as
        "9000rpm". Sets settings.profile (whose own default is medium).
      '';
    };

    exitDuty = mkOption {
      type = types.ints.between 0 100;
      default = 100;
      description = ''
        Duty (percent) set on every exit: signal, panic, normal return, and
        by the ExecStopPost safety net after any stop or crash.
      '';
    };

    failsafeDuty = mkOption {
      type = types.ints.between 0 100;
      default = 100;
      description = "Duty (percent) a zone runs at while it is in failsafe.";
    };

    watchdogSec = mkOption {
      type = types.ints.positive;
      default = 30;
      description = ''
        systemd watchdog timeout. The daemon pings once per control interval,
        so this must be well above settings.interval. A hung daemon is killed
        and ExecStopPost puts the fans at the exit duty before the restart.
      '';
    };

    hardening = mkOption {
      type = types.bool;
      default = true;
      description = ''
        Apply systemd sandboxing that still allows writes under /sys
        (ProtectKernelTunables and PrivateDevices are deliberately not set,
        so hwmon writes and vendor tools used by exec sensors keep working).
      '';
    };

    extraReadWritePaths = mkOption {
      type = types.listOf types.str;
      default = [ ];
      description = "Extra writable paths under the strict ProtectSystem sandbox (e.g. for exec tools).";
    };

    settings = mkOption {
      default = { };
      description = ''
        fand configuration, rendered to YAML. See the README for the full
        reference. sensors, outputs and zones vary per appliance.
      '';
      example = lib.literalExpression ''
        {
          sensors.board = { type = "hwmon"; hwmon = "lm75"; };
          outputs.fans = {
            type = "hwmon"; hwmon = "as7760_fan"; raw_min = 128;
            tach = { auto = true; max_rpm = 21000; min_rpm = 1000; };
          };
          zones.chassis = { outputs = [ "fans" ]; sensors = [ "board" ]; policy = "delta-balanced"; };
        }
      '';
      type = types.submodule {
        freeformType = yaml.type;
        options = {
          interval = mkOption {
            type = types.either types.str types.number;
            default = "2s";
            description = "Control loop period.";
          };
          sensors = mkOption {
            type = types.attrsOf yaml.type;
            default = { };
            description = "Input sensors by name (hwmon, file, exec, thermal_zone).";
          };
          outputs = mkOption {
            type = types.attrsOf yaml.type;
            default = { };
            description = "Fan outputs by name (hwmon, file, exec).";
          };
          zones = mkOption {
            type = types.attrsOf yaml.type;
            default = { };
            description = "Control zones: outputs, sensors and a policy each.";
          };
        };
      };
    };
  };

  config = lib.mkIf cfg.enable {
    services.fand.settings = {
      exit_duty = lib.mkDefault cfg.exitDuty;
      failsafe_duty = lib.mkDefault cfg.failsafeDuty;
    }
    // lib.optionalAttrs (cfg.profile != null) { profile = lib.mkDefault cfg.profile; };

    # For "fand status", "fand set" and "fand calibrate" (default config path).
    environment.etc."fand/config.yaml".source = configFile;
    environment.systemPackages = [ cfg.package ];

    systemd.services.fand = {
      description = "fand fan control daemon";
      wantedBy = [ "multi-user.target" ];
      after = [ "systemd-modules-load.service" ];
      wants = [ "systemd-modules-load.service" ];
      # Fan control must never give up restarting.
      startLimitIntervalSec = 0;
      serviceConfig = {
        Type = "notify";
        NotifyAccess = "main";
        ExecStart = "${exe} run --config ${configFile}";
        # Safety net after every stop, crash or watchdog kill.
        ExecStopPost = "${exe} failsafe --config ${configFile}";
        WatchdogSec = cfg.watchdogSec;
        Restart = "always";
        RestartSec = "2s";
        TimeoutStartSec = "60s";
        TimeoutStopSec = "15s";
        RuntimeDirectory = "fand";
        RuntimeDirectoryMode = "0750";
        StateDirectory = "fand";
        OOMScoreAdjust = -900;
      }
      // lib.optionalAttrs cfg.hardening {
        NoNewPrivileges = true;
        ProtectSystem = "strict";
        ProtectHome = true;
        PrivateTmp = true;
        ProtectKernelModules = true;
        ProtectKernelLogs = true;
        ProtectControlGroups = true;
        ProtectClock = true;
        ProtectHostname = true;
        RestrictRealtime = true;
        RestrictSUIDSGID = true;
        RestrictNamespaces = true;
        LockPersonality = true;
        SystemCallArchitectures = "native";
        RestrictAddressFamilies = [
          "AF_UNIX"
          "AF_NETLINK"
          "AF_INET"
          "AF_INET6"
        ];
        ReadWritePaths = lib.optional (promFile != null) "-${dirOf promFile}" ++ cfg.extraReadWritePaths;
      };
    };
  };
}
