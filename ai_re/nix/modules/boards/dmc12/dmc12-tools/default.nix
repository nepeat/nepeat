{ config, lib, pkgs, ... }:

let
  cfg = config.services.dmc12-tools;

  # All runtime state lives under /run/dmc12/, populated at boot by the
  # oneshot units below. tmpfs lifetime is right: every boot rebuilds the
  # files from the live hardware, /etc stays a stock NixOS read-only mount.
  stateDir = "/run/dmc12";

  showScript = pkgs.runCommand "dmc12-show" {
    nativeBuildInputs = [ pkgs.makeWrapper ];
  } ''
    install -D -m 0755 ${./show} $out/bin/show
    wrapProgram $out/bin/show \
      --prefix PATH : ${lib.makeBinPath [
        pkgs.bash pkgs.coreutils pkgs.gawk pkgs.gnugrep pkgs.i2c-tools pkgs.lm_sensors
      ]} \
      --set DMC12_STATE_DIR ${stateDir}
  '';

  setledScript = pkgs.runCommand "dmc12-setled" {
    nativeBuildInputs = [ pkgs.makeWrapper ];
  } ''
    install -D -m 0755 ${./setled} $out/bin/setled
    wrapProgram $out/bin/setled \
      --prefix PATH : ${lib.makeBinPath [ pkgs.bash pkgs.coreutils pkgs.i2c-tools pkgs.util-linux ]}
  '';

  setphyspeedScript = pkgs.runCommand "dmc12-setphyspeed" {
    nativeBuildInputs = [ pkgs.makeWrapper ];
  } ''
    install -D -m 0755 ${./setphyspeed} $out/bin/setphyspeed
    wrapProgram $out/bin/setphyspeed \
      --prefix PATH : ${lib.makeBinPath [ pkgs.bash pkgs.coreutils pkgs.i2c-tools pkgs.util-linux ]}
  '';

  envmonBootScript = pkgs.runCommand "dmc12-envmon-boot" {
    nativeBuildInputs = [ pkgs.makeWrapper ];
  } ''
    install -D -m 0755 ${./envmon-boot} $out/bin/dmc12-envmon-boot
    wrapProgram $out/bin/dmc12-envmon-boot \
      --prefix PATH : ${lib.makeBinPath [ pkgs.bash pkgs.coreutils ]}
  '';

  onieepromTool = pkgs.runCommand "dmc12-onieeeprom" {
    nativeBuildInputs = [ pkgs.makeWrapper ];
  } ''
    install -D -m 0755 ${./onieeeprom} $out/bin/onieeeprom
    wrapProgram $out/bin/onieeeprom \
      --prefix PATH : ${lib.makeBinPath [ pkgs.python3 ]}
    sed -i '1s|.*|#!${pkgs.python3}/bin/python3|' $out/bin/.onieeeprom-wrapped
  '';

  eepromSysfs = "/sys/bus/i2c/devices/7-0051/eeprom";

  # Common preamble for the systemd oneshots: create /run/dmc12 and start
  # after /dev/i2c-4 exists (the gate the rest of the I2C-dependent units
  # already use).
  mkOneshot = { description, execStart, needsI2c ? true }: {
    inherit description;
    wantedBy = [ "multi-user.target" ];
    after = [ "sysinit.target" ];
    unitConfig = lib.mkIf needsI2c { ConditionPathExists = "/dev/i2c-4"; };
    serviceConfig = {
      Type = "oneshot";
      RemainAfterExit = true;
      ExecStartPre = "${pkgs.coreutils}/bin/mkdir -p ${stateDir}";
      ExecStart = execStart;
    };
  };
in
{
  options.services.dmc12-tools = {
    enable = lib.mkEnableOption "Factory-port hardware info command (`show ...`) + /dev/P{1..48} udev rules + boot-time CPLD state writers";
  };

  config = lib.mkIf cfg.enable {
    environment.systemPackages = [
      showScript onieepromTool setledScript setphyspeedScript envmonBootScript
    ];

    # /etc/deviceconfigured for setled's solid-green branch.
    environment.etc.deviceconfigured.text = "";

    # /run/dmc12/ tmpfiles entry — guarantees the dir exists even if all the
    # oneshot units below are disabled.
    systemd.tmpfiles.rules = [ "d ${stateDir} 0755 root root - -" ];

    systemd.services = {
      dmc12-setled = mkOneshot {
        description = "Set DMC-12 status LED to solid green after boot";
        execStart = "${setledScript}/bin/setled";
      };

      dmc12-setphyspeed = mkOneshot {
        description = "Sync DMC-12 uplink upper-PHY to lower-PHY speed";
        execStart = "${setphyspeedScript}/bin/setphyspeed -q";
      };

      dmc12-envmon-boot = mkOneshot {
        description = "Populate ${stateDir}/boot.env from kernel cmdline";
        execStart = "${envmonBootScript}/bin/dmc12-envmon-boot";
        needsI2c = false;     # purely reads /proc/cmdline
      };

      dmc12-onieeeprom = {
        description = "Dump ONIE TLV board EEPROM to ${stateDir}/eeprom.json";
        wantedBy = [ "multi-user.target" ];
        after = [ "sysinit.target" "systemd-udev-settle.service" ];
        wants = [ "systemd-udev-settle.service" ];
        unitConfig.ConditionPathExists = eepromSysfs;
        serviceConfig = {
          Type = "oneshot";
          RemainAfterExit = true;
          ExecStartPre = "${pkgs.coreutils}/bin/mkdir -p ${stateDir}";
          ExecStart = "${pkgs.bash}/bin/sh -c '${onieepromTool}/bin/onieeeprom dump -F json ${eepromSysfs} > ${stateDir}/eeprom.json.new && mv ${stateDir}/eeprom.json.new ${stateDir}/eeprom.json'";
          StandardError = "journal";
        };
      };
    };
  };
}
