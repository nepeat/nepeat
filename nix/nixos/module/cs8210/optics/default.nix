# hardware.cs8210.optics: cs8210-optics, the one process that touches the I2C FPGA
# (PCI 1d0f:8571: the 32 cages' presence, ResetL, LPMode, IntL and module I2C).
# It releases declared cages' modules from reset, brings them up (power class,
# CMIS ModuleReady, the application the cage's ports need), holds every other
# cage in reset, serves SONiC's sonic_platform (Unix socket, optoe flat offsets),
# writes switch_optic_* / cs8210_optics_* to the node-exporter textfile directory,
# and keeps the FPGA-stall safeguards (one flock, root-port check before every poll
# and request, all-ones detection, /run/cs8210-qsfp.dead). Replaces the Python
# cs8210-qsfp and the "read-only platform".
#
# hardware.cs8210.leds: cs8210-leds monitor (system LED green while fand runs
# without failsafe and the FPGA is alive) and the LED test tool.
#
# Rust: rust/cs8210 in the monorepo (README there: design, unsafe inventory).
{
  config,
  lib,
  pkgs,
  ...
}:

let
  inherit (lib) mkOption mkIf types;
  cfg = config.hardware.cs8210.optics;
  lcfg = config.hardware.cs8210.leds;
  hostSystem = pkgs.stdenv.hostPlatform.system;
  # Cross-compiled from this system like the kernel and the SONiC packages
  # (native aarch64 rustc would run under emulation on the desktop).
  bpkgs =
    if cfg.buildSystem == null || cfg.buildSystem == hostSystem then
      pkgs
    else
      import pkgs.path {
        localSystem = cfg.buildSystem;
        crossSystem = hostSystem;
      };
  defaultPackage = bpkgs.callPackage ../../../../../rust/cs8210/nix/package.nix { };
  mon = config.monitoring.nodeExporter;
  metricsFile = if mon.enable or false then "${mon.textfile.directory}/cs8210-optics.prom" else null;
  cageArgs = lib.concatStringsSep " " (
    lib.mapAttrsToList (c: m: "--cage ${c}${lib.optionalString (m != null) ":${m}"}") cfg.cages
  );
in
{
  options.hardware.cs8210.optics = {
    enable = mkOption {
      type = types.bool;
      default = true;
      description = "cs8210-optics, the owner of the I2C FPGA (cages, module bring-up, optics metrics).";
    };
    package = mkOption {
      type = types.package;
      default = defaultPackage;
      defaultText = lib.literalExpression "rust/cs8210 (cross-built from buildSystem)";
      description = "cs8210-optics + cs8210-leds.";
    };
    buildSystem = mkOption {
      type = types.nullOr types.str;
      default = "x86_64-linux";
      description = "Cross-compile the Rust tools from this system; null builds them natively.";
    };
    cages = mkOption {
      type = types.attrsOf (types.nullOr (types.strMatching "[1248]x[0-9]+"));
      default = { };
      example = {
        "1" = "1x400";
        "17" = "4x100";
      };
      description = ''
        Declared cages (number -> ports x Gb/s of the cage, or null: leave the
        module's default application). Only their modules are released from
        reset and brought up; every other cage is held in reset. With SONiC on,
        derived from the admin-up ports (nix/nixos/module/cs8210/sonic).
      '';
    };
  };
  options.hardware.cs8210.leds.enable = mkOption {
    type = types.bool;
    default = true;
    description = "cs8210-leds monitor: the system LED (port LEDs are the ASIC's LED stream).";
  };

  config = mkIf cfg.enable {
    environment.systemPackages = [ cfg.package ];

    systemd.services.cs8210-optics = {
      description = "CS8210: cs8210-optics, the owner of the I2C FPGA (cages, modules, optics metrics)";
      wantedBy = [ "multi-user.target" ];
      # All-ones instead of an SError for an unanswered read (module/cs8210
      # pcieErrorsAsAllOnes) before the first BAR access; the textfile
      # directory before the metrics file.
      wants = [ "cs8210-pcie-errors.service" ];
      after = [
        "cs8210-pcie-errors.service"
        "systemd-tmpfiles-resetup.service"
      ];
      serviceConfig = {
        ExecStart =
          "${cfg.package}/bin/cs8210-optics daemon ${cageArgs}"
          + lib.optionalString (metricsFile != null) " --metrics ${metricsFile}";
        Restart = "on-failure";
        RestartSec = 5;
        RuntimeDirectory = "cs8210-optics";
        # A restart re-adopts the modules (no reset, no module write): links stay up.
      };
    };

    systemd.services.cs8210-leds = mkIf lcfg.enable {
      description = "CS8210: system LED from fand and the I2C FPGA's health";
      wantedBy = [ "multi-user.target" ];
      after = [
        "fand.service"
        "cs8210-optics.service"
      ];
      serviceConfig = {
        ExecStart = "${cfg.package}/bin/cs8210-leds monitor";
        Restart = "always";
        RestartSec = 10;
        LogLevelMax = "info";
      };
    };
  };
}
