# CS8210 fan control: temperature-driven PWM on the two fan CPLDs (0x66 behind
# the kernel's 0x70:2 -> 0x74:2/3 mux channels), from the ASIC die
# temperature (MCP3425) and the 13 board LM75s.
#
# hardware.cs8210.fan.controller picks the daemon:
#   * "fand" (default): rust/crates/fand from this monorepo (type: i2c
#     outputs, failsafe on any fault, tach checks, ExecStopPost safety net).
#   * "python": the original services.cs8210-fancontrol (./fancontrol.py),
#     kept as the fallback. Same curves, no tach checks.
# Both use the curve options below. Register contract: ./fancontrol.py and
# ai_re/devices/cs8210-32x/board-control.md.
{
  config,
  lib,
  pkgs,
  ...
}:

let
  fan = config.hardware.cs8210.fan;
  cfg = config.services.cs8210-fancontrol;
  pyEnv = pkgs.python3.withPackages (ps: [ ps.smbus2 ]);
  fanScript = pkgs.writeShellScriptBin "cs8210-fancontrol" ''
    exec ${pyEnv}/bin/python3 -u ${./fancontrol.py} "$@"
  '';

  hex = n: "0x${lib.toLower (lib.fixedWidthString 2 "0" (lib.toHexString n))}";

  # ---- fand ---------------------------------------------------------------
  # Linux numbers the mux channels in probe order, so fand finds the fan
  # board buses by devicetree node; the scripts use the channel-N links.
  fanBoards = {
    top = "i2c-mux@74/i2c@2";
    bottom = "i2c-mux@74/i2c@3";
  };
  slots = lib.range 1 fan.slots;
  slotBase = n: 16 + 16 * n; # 0x20, 0x30 .. 0x80: +0 front tach, +1 rear tach, +2 PWM, +4 LED, +6 presence
  minPct = fan.minPwm * 100.0 / 255;

  tools = lib.makeBinPath [
    pkgs.i2c-tools
    pkgs.coreutils
    pkgs.gawk
    pkgs.util-linux
  ];
  busLib = ''
    export PATH=${tools}
    # bus <glob below the i2c-pld adapter> -> Linux bus number
    bus() {
      local p
      for p in /sys/bus/platform/devices/fd880000.i2c/i2c-*/$1; do
        if [ -e "$p" ]; then p=$(readlink -f "$p"); echo "''${p##*/i2c-}"; return 0; fi
      done
      echo "i2c path not found: $1" >&2
      return 1
    }
    SYS_CPLD_BUS='*-0070/channel-2/*-0074/channel-0/*-0071/channel-2'
  '';

  # Switch ASIC die temperature: MCP3425 at 0x68 behind 0x70:2 -> 0x74:0 ->
  # 0x72:7. A plain 3-byte read (an SMBus register access would write the
  # ADC's config register first); the vendor's check_switch_temp conversion,
  # as in fancontrol.py.
  asicTemp = pkgs.writeShellScript "cs8210-asic-temp" ''
    set -eu
    ${busLib}
    b=$(bus '*-0070/channel-2/*-0074/channel-0/*-0072/channel-7')
    read -r hi lo cf <<<"$(i2ctransfer -y "$b" r3@0x68)"
    if [ $(( cf & 0x7f )) -ne $(( 0x10 )) ]; then
      echo "MCP3425 config $cf, expected 0x10" >&2
      exit 1
    fi
    raw=$(( (hi << 8) | lo ))
    [ $(( raw & 0x8000 )) -eq 0 ] || raw=$(( raw - 65536 ))
    awk -v r="$raw" 'BEGIN { v = r / 1000; printf "%.2f\n", -124.28 * v * v - 422.03 * v + 384.62 }'
  '';

  # System CPLD 0x60 reg 0x05 bits 7/5 (active-low fan_max_speed): cleared =
  # the CPLD drives every fan at full speed whatever the PWM registers say.
  # The CPLD clears them once the ASIC is initialised (every SDK start), so
  # this exec output releases them every tick below 100 % and clears them at
  # 100 % (failsafe, startup, exit): a second route to full speed, on another
  # mux channel than the fan boards. Read-modify-write: the other bits of
  # 0x05 are not ours.
  cpldForce = pkgs.writeShellScript "cs8210-fan-force" ''
    set -eu
    ${busLib}
    pct=$1
    b=$(bus "$SYS_CPLD_BUS")
    cur=$(( $(i2cget -y "$b" 0x60 0x05) ))
    if [ "$pct" -ge 100 ]; then new=$(( cur & ~0xa0 & 0xff )); else new=$(( cur | 0xa0 )); fi
    [ "$new" -ne "$cur" ] || exit 0
    i2cset -y "$b" 0x60 0x05 "$new"
    got=$(( $(i2cget -y "$b" 0x60 0x05) ))
    if [ "$got" -ne "$new" ]; then
      printf 'system CPLD 0x05 reads 0x%02x after writing 0x%02x\n' "$got" "$new" >&2
      exit 1
    fi
    if [ "$pct" -lt 100 ]; then
      logger -t fand "system CPLD was forcing full speed (0x05 $(printf 0x%02x "$cur")); released"
    fi
  '';

  # After every stop, crash or watchdog kill, after "fand failsafe": every
  # route to full speed, each on its own bus so one failing does not skip the
  # others, then the CPLD fan watchdogs re-armed (fand disarms them and
  # nothing pets them). fancontrol.py's exit path.
  exitScript = pkgs.writeShellScript "cs8210-fan-exit" ''
    set -u
    ${busLib}
    rc=0
    if b=$(bus "$SYS_CPLD_BUS") && v=$(i2cget -y "$b" 0x60 0x05) \
      && i2cset -y "$b" 0x60 0x05 $(( v & ~0xa0 & 0xff )); then :; else
      echo "could not force full speed via the system CPLD" >&2; rc=1
    fi
    for ch in 2 3; do
      if ! b=$(bus "*-0070/channel-2/*-0074/channel-$ch"); then rc=1; continue; fi
      for r in ${lib.concatMapStringsSep " " (n: hex (slotBase n + 2)) (lib.range 1 7)}; do
        i2cset -y "$b" 0x66 "$r" 0xff || rc=1
      done
      if w=$(i2cget -y "$b" 0x66 0x0c) && i2cset -y "$b" 0x66 0x0c $(( w | 1 )); then :; else
        echo "could not re-arm the fan watchdog on 0x74 channel $ch" >&2; rc=1
      fi
    done
    [ "$rc" -ne 0 ] || echo "fans at full speed, system CPLD forcing max, fan watchdogs armed"
    exit "$rc"
  '';

  slotOutput = node: n: {
    type = "i2c";
    i2c = {
      bus.of_node = node;
      address = "0x66";
      register = hex (slotBase n + 2);
    };
    raw_min = fan.minPwm;
    # Fan CPLD version register; on anything else fand writes nothing.
    expect = [
      {
        register = "0x00";
        value = fan.cpldVersion;
      }
    ];
    # Fan watchdog, reg 0x0c: bit 0 enable, bits 7:1 timeout / 5 s. Power-on
    # 0x19 (armed, 60 s); like the vendor's disable_fcs.sh, fand disarms it
    # (0x18) and owns the fans. cs8210-fan-exit re-arms it.
    arm = {
      register = "0x0c";
      mask = "0x01";
      value = 0;
      write = "0x18";
    };
    tach = {
      i2c = [
        {
          register = hex (slotBase n);
          label = "front";
        }
        {
          register = hex (slotBase n + 1);
          label = "rear";
        }
      ];
      # raw x 150 rpm: the vendor fan_cpld.ko fan_show_value multiplies by
      # 150 (mov w2,#0x96 at 0x9fc); vendor fans.yaml: NIDEC R3181-FN047-010AD
      # max front 28000 / panel 31000 rpm (AVC 24919 / 30814). Ours at PWM
      # 255: raw 189-191 front / 207-211 rear = 28.5k / 31.5k rpm.
      scale = 150;
      max_rpm_by_label = {
        front = 28500;
        rear = 31500;
      };
      min_rpm = fan.minRpm;
    };
    # Module presence, base+6 bit 0: 0 present, 1 absent (bit 1 is the airflow
    # direction). Vendor fan_cpld.ko disassembly, board-control.md "Fan CPLD
    # register map". Absent = a fault of this output (failsafe for the rest),
    # its rotors are not checked.
    present = {
      register = hex (slotBase n + 6);
      mask = "0x01";
      value = 0;
    };
    # Tray LED, base+4 bits 1:0: 1 green, 2 red, 3 off; software-driven only
    # (no CPLD auto mode, nothing else in the vendor stack sets it), same
    # sources. fand (owner's policy): green = present, both rotors fine;
    # blinking red (red / off, 1 s) = fand is handling it (a stall recovery,
    # a rotor that should come up after an insertion, the start or a
    # release); solid red = given up / isolated, another rotor fault, or
    # absent. On exit a blinking LED goes solid red, the others stay.
    # Read-modify-write: the other bits are kept.
    led = {
      register = hex (slotBase n + 4);
      mask = "0x03";
      ok = "0x01";
      fault = "0x02";
      absent = "0x02";
      blink = {
        off = "0x03";
        period = "1s";
      };
    };
    # No per-slot power control exists, and the duty drives both rotors. A
    # stalled rotor (below min_rpm) first gets stall_recovery (below); when
    # that gives up, or for a rotor that is not following / flagged, the
    # module gets duty 0 (after the 15 s grace from start / reinsertion)
    # instead of any zone or failsafe duty. Latched until reinserted, "fand
    # release <slot>" or a restart (given-up modules are remembered in
    # /run/fand/recovery.json across restarts). After the 2026-11-26
    # incident (a stalled rear rotor driven at 100 % for 35 min until it
    # smelt burnt).
    isolate_on_rotor_fault = true;
    isolate_raw = 0;
  };
  fanOutputs = lib.concatMapAttrs (
    board: node: lib.listToAttrs (map (n: lib.nameValuePair "${board}${toString n}" (slotOutput node n)) slots)
  ) fanBoards;
  outputNames = lib.attrNames fanOutputs ++ [ "cpld_force" ];

  # The 13 LM75s of the board DTS, by devicetree node (unique path tails).
  lm75 = {
    i2cfpga_card = "i2c-mux@71/i2c@0/temperature-sensor@48";
    switchboard_left = "i2c-mux@71/i2c@1/temperature-sensor@48";
    switchboard_center = "i2c-mux@71/i2c@1/temperature-sensor@4b";
    switchboard_right = "i2c-mux@72/i2c@7/temperature-sensor@49";
    macsec_left = "i2c-mux@73/i2c@5/temperature-sensor@49";
    macsec_center = "i2c-mux@73/i2c@5/temperature-sensor@4a";
    macsec_right = "i2c-mux@73/i2c@5/temperature-sensor@4c";
    top_fanboard_right = "i2c-mux@74/i2c@2/i2c-mux@73/i2c@0/temperature-sensor@4d";
    top_fanboard_center = "i2c-mux@74/i2c@2/i2c-mux@73/i2c@0/temperature-sensor@4e";
    top_fanboard_left = "i2c-mux@74/i2c@2/i2c-mux@73/i2c@0/temperature-sensor@4f";
    bottom_fanboard_right = "i2c-mux@74/i2c@3/i2c-mux@73/i2c@0/temperature-sensor@4d";
    bottom_fanboard_center = "i2c-mux@74/i2c@3/i2c-mux@73/i2c@0/temperature-sensor@4e";
    bottom_fanboard_left = "i2c-mux@74/i2c@3/i2c-mux@73/i2c@0/temperature-sensor@4f";
  };

  fandCfg = config.services.fand;
in
{
  # The monorepo's fand module; reachable because the nepeat/nix flake is
  # evaluated as git+file:<monorepo>?dir=nix (whole repository as source).
  imports = [ ../../../../../rust/crates/fand/nix/module.nix ];

  options.hardware.cs8210.fan = {
    controller = lib.mkOption {
      type = lib.types.enum [
        "fand"
        "python"
      ];
      default = "fand";
      description = ''
        Fan daemon: "fand" (services.fand, configured here) or "python"
        (services.cs8210-fancontrol, the original script, kept as fallback).
      '';
    };
    interval = lib.mkOption {
      type = lib.types.int;
      default = 5;
      description = "Seconds between control ticks (fand) or samples (python).";
    };
    lowTemp = lib.mkOption {
      type = lib.types.int;
      # The MACsec card LM75s idle at ~38 C with the PHYs configured; the
      # vendor's board warning threshold is 53 C.
      default = 40;
      description = "Hottest board sensor (C) at or below which fans run at minPwm.";
    };
    highTemp = lib.mkOption {
      type = lib.types.int;
      default = 55;
      description = "Hottest board sensor (C) at or above which fans run at full speed.";
    };
    asicLowTemp = lib.mkOption {
      type = lib.types.int;
      default = 70;
      description = "Switch ASIC die temperature (C) at or below which it asks for no more than minPwm.";
    };
    asicHighTemp = lib.mkOption {
      type = lib.types.int;
      default = 95;
      description = "Switch ASIC die temperature (C) at or above which fans run at full speed (vendor PID setpoint 97, limit 105).";
    };
    asicStartupGrace = lib.mkOption {
      type = lib.types.str;
      default = "60s";
      description = ''
        fand: how long after its start the ASIC sensor may have no reading yet
        without that being a fault ("waiting": the board zone drives the fans
        meanwhile). The ASIC diode is an exec sensor (MCP3425 over I2C,
        independent of the SDK); its first result lands one interval after
        fand starts (4.8 s on the 2026-11-25 boots), which without a grace was
        a failsafe at 100 % plus the 60 s calm period on every boot. After the
        grace, or once it has read, a missing reading is the usual fault.
      '';
    };
    startupDuty = lib.mkOption {
      type = lib.types.ints.between 0 100;
      default = 60;
      description = "fand: duty (%) every zone starts from before its curve takes over (fand's default: 100). Any fault is still failsafe 100 % at once.";
    };
    asicCriticalC = lib.mkOption {
      type = lib.types.int;
      default = 100;
      description = "fand: ASIC die temperature (C) at which every zone goes failsafe.";
    };
    hysteresis = lib.mkOption {
      type = lib.types.int;
      default = 2;
      description = "Falling-temperature deadband (C) of both curves.";
    };
    minPwm = lib.mkOption {
      type = lib.types.ints.between 0 255;
      default = 127;
      description = ''
        PWM floor (0-255), fand's hardware minimum. 127 is the vendor's own
        quiet preset (~50%, about 11000 rpm). Going lower is untested on this
        chassis.
      '';
    };
    slots = lib.mkOption {
      type = lib.types.ints.between 1 7;
      default = 7;
      description = "fand: fan slots per fan board it drives and checks (7 on this chassis, 14 dual fans).";
    };
    stallRecovery = lib.mkOption {
      type = lib.types.attrsOf lib.types.anything;
      default = {
        hold_raw = "0x00";
        hold = "10s";
        ramp = [
          {
            raw = "0x30";
            for = "5s";
          }
        ];
        settle = "15s";
        max_attempts = 3;
        # Between attempts for one stall; any two dips are min_gap apart; a
        # hot-swapped module that comes up at 0 rpm gets one insert_delay
        # after its presence edge (2026-11-26: two reinserted modules came up
        # unpowered and sat in failsafe until a manual dip).
        interval = "5m";
        min_gap = "30s";
        insert_delay = "5s";
        max_temp = {
          board = 45;
          asic = 70;
        };
      };
      description = ''
        fand stall_recovery ("calm restart"), merged per key: a rotor that
        stopped in a running chassis is windmilled backwards by the others'
        backdraft and cannot start; every fan at hold_raw for hold, then the
        ramp, lets it (coordinator, live: 5 of 5 with ~12 s at 0 then 0x30
        for 6 s; ~2 min of low duty took the ASIC to 82.9 C, hence the short
        dip, the 5 min interval and the guards: board >= 45 C or ASIC >= 70 C
        skips or aborts it). cpld_force is released first (its force bits
        would hold every fan at 0xff). Gives up after max_attempts (isolated,
        solid red).
      '';
    };
    degradedMaxFailedRotors = lib.mkOption {
      type = lib.types.ints.unsigned;
      default = 4;
      description = ''
        fand rotor_fault_policy degraded: stalled, isolated or absent rotors
        keep the zones on their curves with the remaining fans; more failed
        rotors than this (an isolated or absent module counts both) is
        failsafe 100 %. Two given-up modules are 4.
      '';
    };
    minRpm = lib.mkOption {
      type = lib.types.int;
      default = 3000;
      description = "fand: a rotor below this is a fault (stalled reads 0; ~11000 rpm at the floor).";
    };
    cpldVersion = lib.mkOption {
      type = lib.types.int;
      default = 3;
      description = "fand: expected fan CPLD version (reg 0x00); on a mismatch fand writes nothing.";
    };
    prometheusFile = lib.mkOption {
      type = lib.types.nullOr lib.types.str;
      default =
        if config.monitoring.nodeExporter.enable or false then
          "${config.monitoring.nodeExporter.textfile.directory}/fand.prom"
        else
          null;
      defaultText = lib.literalExpression ''"''${config.monitoring.nodeExporter.textfile.directory}/fand.prom" when monitoring.nodeExporter is enabled, else null'';
      description = "fand: node_exporter textfile output (fand_* metrics).";
    };
  };

  options.services.cs8210-fancontrol.enable = lib.mkEnableOption "the original CS8210 fan control script; set hardware.cs8210.fan.controller = \"python\" instead";

  config = lib.mkMerge [
    {
      assertions = [
        {
          assertion = !(cfg.enable && fandCfg.enable);
          message = "cs8210: services.cs8210-fancontrol and services.fand both drive the fan CPLDs; pick one with hardware.cs8210.fan.controller.";
        }
      ];
      services.cs8210-fancontrol.enable = lib.mkDefault (fan.controller == "python");
    }

    (lib.mkIf cfg.enable {
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
            "--interval ${toString fan.interval}"
            "--low-temp ${toString fan.lowTemp}"
            "--high-temp ${toString fan.highTemp}"
            "--min-pwm ${toString fan.minPwm}"
            "--hysteresis ${toString fan.hysteresis}"
            "--asic-low-temp ${toString fan.asicLowTemp}"
            "--asic-high-temp ${toString fan.asicHighTemp}"
          ];
          Restart = "always";
          RestartSec = "5s";
        };
      };
    })

    (lib.mkIf (fan.controller == "fand") {
      services.fand = {
        enable = true;
        # The hardware minimum (raw_min = minPwm) is the floor.
        profile = "none";
        settings = {
          interval = "${toString fan.interval}s";
          calm_period = "60s";
          startup_duty = fan.startupDuty;
          # Fan problems: keep temperature control with the remaining fans
          # (owner, 2026-11-26); sensor loss, critical temperatures, write
          # errors and too many failed rotors stay failsafe.
          rotor_fault_policy = "degraded";
          stall_recovery = fan.stallRecovery // {
            release = [ "cpld_force" ];
          };
          sensors =
            lib.mapAttrs (_: node: {
              type = "hwmon";
              hwmon.of_node = node;
              plausible = [
                (-10)
                110
              ];
            }) lm75
            // {
              asic = {
                type = "exec";
                command = [ "${asicTemp}" ];
                interval = "${toString fan.interval}s";
                timeout = "3s";
                plausible = [
                  5
                  130
                ];
                critical = fan.asicCriticalC;
                startup_grace = fan.asicStartupGrace;
              };
            };
          outputs = fanOutputs // {
            cpld_force = {
              type = "exec";
              command = [
                "${cpldForce}"
                "{percent}"
              ];
              timeout = "3s";
              # Every tick: releases the CPLD's force soon after an SDK start.
              reassert = "${toString fan.interval}s";
              # At 100 % this forces every fan to full speed, whatever its PWM
              # register says, the isolated module too: below 100 while any
              # slot is isolated (the others still get 0xff through their PWM).
              isolation_max_duty = 99;
            };
          };
          # As fancontrol.py: linear from (low, minPwm) to (high, 255) on the
          # hottest board sensor, and on the ASIC die; the higher one wins.
          zones = {
            board = {
              outputs = outputNames;
              sensors = lib.attrNames lm75;
              degraded_max_failed_rotors = fan.degradedMaxFailedRotors;
              policy.curve = {
                points = [
                  [ fan.lowTemp minPct ]
                  [ fan.highTemp 100 ]
                ];
                inherit (fan) hysteresis;
              };
            };
            asic = {
              outputs = outputNames;
              sensors = [ "asic" ];
              degraded_max_failed_rotors = fan.degradedMaxFailedRotors;
              policy.curve = {
                points = [
                  [ fan.asicLowTemp minPct ]
                  [ fan.asicHighTemp 100 ]
                ];
                inherit (fan) hysteresis;
              };
            };
          };
        }
        // lib.optionalAttrs (fan.prometheusFile != null) { runtime.prometheus_file = fan.prometheusFile; };
      };

      # The textfile directory (tmpfiles) must exist before fand's namespace
      # is set up (ReadWritePaths "-dir" skips a missing one): matters on a
      # switch that adds both.
      systemd.services.fand.after = lib.mkIf (fan.prometheusFile != null) [
        "systemd-tmpfiles-resetup.service"
      ];

      # fand's own safety net ("fand failsafe": every output to 100 %, which
      # disarms the watchdogs again on the way), then cs8210-fan-exit. "-" so
      # a failing first step never skips the second.
      systemd.services.fand.serviceConfig.ExecStopPost = lib.mkForce [
        "-${lib.getExe fandCfg.package} failsafe --config ${config.environment.etc."fand/config.yaml".source}"
        "${exitScript}"
      ];
    })
  ];
}
