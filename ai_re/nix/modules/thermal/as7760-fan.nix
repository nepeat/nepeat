# AS7760-32X fan CPLD driver, sensor labels and the thermal monitor service.
# Other boards with the same Accton fan CPLD can import this as is.
{ config, lib, pkgs, aiReRust ? null, ... }:
let
  kernel = config.boot.kernelPackages.kernel;
  inherit (import ../../packages/as7760-fan { inherit pkgs kernel; }) driver tool;
  cfg = config.hardware.as7760Fan;
  board = id: { type = "hwmon"; hwmon = { name = "lm75"; of_node = "pca9548@76/i2c@1/temperature-sensor@${id}"; }; plausible = [ (-10) 110 ]; };
  # Die temperature from openbcm-asic-temp (modules/switch/openbcm.nix).
  asicTemp = config.hardware.openbcm.temperature.enable or false;
in {
  # fand (rust/crates/fand) when the monorepo's rust flake is available.
  imports = lib.optional (aiReRust != null) aiReRust.nixosModules.fand;

  options.hardware.as7760Fan = {
    controller = lib.mkOption {
      type = lib.types.enum [ "monitor" "fand" ];
      default = if aiReRust != null then "fand" else "monitor";
      description = ''
        "fand": the fan daemon with a temperature curve (quietest; 50% floor
        set by the hardware). "monitor": the original guard that holds
        `duty` and goes to 100% at `hotC`.
      '';
    };
    duty = lib.mkOption {
      type = lib.types.ints.between 50 100;
      default = 80;
      description = "Fan duty (percent) while all board sensors are cool and healthy; 100 = always full speed.";
    };
    hotC = lib.mkOption { type = lib.types.int; default = 45; description = "Board temperature (C) at which fans go to 100%."; };
    coolC = lib.mkOption { type = lib.types.int; default = 40; description = "Board temperature (C) below which the quiet duty resumes."; };
  };
  config = {
  boot.extraModulePackages = [ driver ];
  boot.kernelModules = [ "as7760_fan" ];
  environment.systemPackages = [ tool ];
  environment.etc."sensors.d/as7760.conf".text = ''
    chip "lm75-i2c-*-48"
      label temp1 "board_1"
    chip "lm75-i2c-*-49"
      label temp1 "board_2"
    chip "lm75-i2c-*-4a"
      label temp1 "board_3"
    chip "lm75-i2c-*-4b"
      label temp1 "cpu_board"
  '';
  systemd.services.as7760-thermal = lib.mkIf (cfg.controller == "monitor") {
    description = "AS7760 fan duty with temperature guard and monitoring";
    wantedBy = [ "multi-user.target" ];
    after = [ "systemd-modules-load.service" ];
    serviceConfig = {
      Type = "simple";
      ExecStart = "${tool}/bin/as7760-fan monitor --duty ${toString cfg.duty} --hot ${toString cfg.hotC} --cool ${toString cfg.coolC}";
      ExecStopPost = "${tool}/bin/as7760-fan set 100";
      Restart = "on-failure";
      RestartSec = "5s";
      TimeoutStopSec = "10s";
      # Needs hwmon writes, but no direct /dev/mem or raw i2c transactions.
      NoNewPrivileges = true;
      PrivateTmp = true;
      ProtectHome = true;
    };
  };
  } // lib.optionalAttrs (aiReRust != null) {
  # services.fand exists only when the fand module is imported (aiReRust).
  # Tested 2026-11-15: 10 min at the hardware minimum (53% duty, ~12.4k rpm)
  # warmed the hottest board sensor by ~1.3 C, then flat (0.0 C/min).
  services.fand = lib.mkIf (cfg.controller == "fand") {
    enable = true;
    # No extra floor: the hardware minimum (53.3%) is the floor.
    profile = "none";
    extraReadWritePaths = [ "/run/prometheus-textfile" ];
    settings = {
      interval = "2s";
      calm_period = "60s";
      runtime.prometheus_file = "/run/prometheus-textfile/fand.prom";
      sensors = { board_1 = board "48"; board_2 = board "49"; board_3 = board "4a"; cpu_board = board "4b"; }
        // lib.optionalAttrs asicTemp {
          # Required: no reading (reader stopped, e.g. for a bcm.user session,
          # or failing) means failsafe, fans at 100%.
          # grace: hold the last reading through writer restarts/handovers
          # (reader restart ~2 s, reader -> syncd ~10 s) instead of a
          # failsafe blip; never applies to a hot or a first reading.
          asic = { type = "file"; path = config.hardware.openbcm.temperature.file; max_age = "20s";
                   grace = "15s"; plausible = [ 0 130 ]; critical = 100; };
        };
      outputs.fans = {
        type = "hwmon"; hwmon = "as7760_fan"; pwm = "pwm1";
        # Hardware minimum: the driver rejects < 128 and the CPLD has 16 steps
        # (raw = step * 17, rounded up), so 128 always reads back as 136 (53.3%).
        raw_min = 136; raw_max = 255;
        readback = "at_least";          # 16-step CPLD register, rounded up
        tach = { auto = true; max_rpm_by_label = { front = 21000; rear = 18000; }; min_rpm = 1000; follow_tolerance = 0.35; follow_grace = "20s"; };
      };
      zones.board = {
        outputs = [ "fans" ]; sensors = [ "board_1" "board_2" "board_3" "cpu_board" ];
        aggregate = "max";
        # A real curve to 100% at 45 C (where the old monitor went to full
        # speed), so heat is handled by control, not failsafe. Idle (~31 C)
        # sits at the hardware minimum.
        # 53% writes raw 136, the hardware minimum; anything above it rounds up
        # to the next CPLD step (raw 153 = 60%), so the idle point must be 53.
        policy.curve = { points = [ [ 35 53 ] [ 40 70 ] [ 45 100 ] ]; hysteresis = 2; };
        critical = 50;   # emergency only: above the curve's 100% point
      };
      # Built-in curve: floor below 60 C, 100% at 95 C. Failsafe at 100 C
      # (the SDK's own over-temperature shutdown, armed only by "init all",
      # defaults to 125 C). Not validated against a datasheet limit.
      zones.asic = lib.mkIf asicTemp { outputs = [ "fans" ]; sensors = [ "asic" ]; policy = "delta-asic"; };
    };
  };
  };
}
