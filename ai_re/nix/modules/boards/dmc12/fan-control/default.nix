{ config, lib, pkgs, ... }:

let
  cfg = config.services.dmc12-fancontrol;

  # The Python script is a verbatim port of the factory /usr/bin/fancontrol
  # (extracted from /boot/primary/rootfs.cpio.gz.uImage on the M.2 SSD). See
  # factory-fancontrol-and-cfg.txt for the original.
  scriptSrc = ./fancontrol.py;

  pyEnv = pkgs.python3.withPackages (ps: with ps; [ smbus2 ]);

  fanScript = pkgs.runCommand "dmc12-fancontrol" {
    nativeBuildInputs = [ pkgs.makeWrapper ];
  } ''
    install -D -m 0755 ${scriptSrc} $out/bin/dmc12-fancontrol
    wrapProgram $out/bin/dmc12-fancontrol \
      --prefix PATH : ${lib.makeBinPath [ pkgs.lm_sensors ]} \
      --set PYTHONPATH ${pyEnv}/${pyEnv.sitePackages} \
      --set PYTHONUNBUFFERED 1
    # Use the python from the env so it sees smbus2 on PYTHONPATH.
    sed -i "1s|.*|#!${pyEnv}/bin/python3|" $out/bin/.dmc12-fancontrol-wrapped 2>/dev/null || true
  '';

  configIni = pkgs.writeText "fancontrol.cfg" ''
    [fancontrol]
    timer=${toString cfg.timer}
    target_temperature=${toString cfg.targetTemperature}
    Kp=${toString cfg.Kp}
    Ki=${toString cfg.Ki}
  '';
in
{
  options.services.dmc12-fancontrol = {
    enable = lib.mkEnableOption "DMC-12 fan control daemon (CPLD PWM via i2c bus)";

    i2cBus = lib.mkOption {
      type = lib.types.int;
      default = 4;
      description = ''
        Linux i2c bus number that exposes the board CPLD at address 0x68.
        U-Boot calls this "dev 4". The matching Linux bus number depends on
        how the pca9548/pca9546 muxes enumerate via the DT — verify with
        `i2cdetect -l` once /sys/class/i2c-dev/* are present.
      '';
    };

    targetTemperature = lib.mkOption {
      type = lib.types.int;
      default = 45;
      description = "Setpoint (°C) for the PI loop. Minimum sustainable temp at 100% fan is ~35 °C; factory recommends 10 °C above that.";
    };

    timer = lib.mkOption {
      type = lib.types.int;
      default = 10;
      description = "Sample/update interval in seconds.";
    };

    Kp = lib.mkOption {
      type = lib.types.float;
      default = 3.0;
      description = "PI controller proportional gain (aggressiveness).";
    };

    Ki = lib.mkOption {
      type = lib.types.float;
      default = 0.1;
      description = "PI controller integral gain (fine tune).";
    };

    extraArgs = lib.mkOption {
      type = lib.types.listOf lib.types.str;
      default = [];
      description = "Extra arguments appended to the dmc12-fancontrol invocation (e.g. --sensors-cmd …).";
    };
  };

  config = lib.mkIf cfg.enable {
    # User-space lm_sensors output is what the daemon parses. Pull it in even
    # if it's already in environment.systemPackages so the closure dep is
    # explicit.
    environment.systemPackages = [ pkgs.lm_sensors fanScript ];

    # The factory script reads /etc/fancontrol.cfg. Symlink it.
    environment.etc."fancontrol.cfg".source = configIni;

    systemd.services.dmc12-fancontrol = {
      description = "DMC-12 fan control daemon (CPLD PWM via I2C)";
      after = [ "multi-user.target" "sysinit.target" ];
      wants = [ "sysinit.target" ];
      wantedBy = [ "multi-user.target" ];

      # Don't fail the unit while the I2C bus is missing (it won't appear
      # until the alpine-v2-dmc12.dts I2C controller node is added). Once
      # the bus is there, ConditionPathExists will pass and the service
      # actually starts.
      unitConfig.ConditionPathExists = "/dev/i2c-${toString cfg.i2cBus}";

      serviceConfig = {
        Type = "simple";
        ExecStart = lib.concatStringsSep " " ([
          "${fanScript}/bin/dmc12-fancontrol"
          "--bus" (toString cfg.i2cBus)
          "--config" "/etc/fancontrol.cfg"
        ] ++ cfg.extraArgs);
        Restart = "on-failure";
        RestartSec = "5s";
        # The daemon needs raw I2C access; CAP_SYS_RAWIO is not strictly
        # required when /dev/i2c-N is permissioned right, but it does need
        # write on that device node.
        SupplementaryGroups = [ "i2c" ];
      };
    };

    # Make sure /dev/i2c-* are accessible to the i2c group so the service
    # can drop privileges later if we want to. Udev rules ship with
    # lm_sensors/i2c-tools by default, but we set the group too.
    services.udev.extraRules = ''
      KERNEL=="i2c-[0-9]*", GROUP="i2c", MODE="0660"
    '';
    users.groups.i2c = { };
  };
}
