# AS7760-32X fan CPLD driver, sensor labels and the thermal monitor service.
# Other boards with the same Accton fan CPLD can import this as is.
{ config, lib, pkgs, ... }:
let
  kernel = config.boot.kernelPackages.kernel;
  inherit (import ../../packages/as7760-fan { inherit pkgs kernel; }) driver tool;
in {
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
  systemd.services.as7760-thermal = {
    description = "AS7760 full-speed bring-up cooling and thermal monitoring";
    wantedBy = [ "multi-user.target" ];
    after = [ "systemd-modules-load.service" ];
    serviceConfig = {
      Type = "simple";
      ExecStart = "${tool}/bin/as7760-fan monitor";
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
}
