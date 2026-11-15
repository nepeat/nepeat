# DMC-12 board CPLD (Lattice MachXO3, I2C bus 4 / 0x68, raw from userspace).
# Register map and evidence: ai_re/devices/dmc-12-48r-as0/vendor-tooling.md.
#   - board reset on `reboot` (the factory kernel's al-dmc12-reboot driver)
#   - interrupt monitor: alarms to the journal, uplink PHY re-sync
{ config, lib, pkgs, ... }:
let
  cfg = config.hardware.dmc12.cpld;
  i2cset = "${pkgs.i2c-tools}/bin/i2cset";
  # systemd-shutdown runs this after unmounting, with "reboot", "poweroff",
  # "halt" or "kexec" as $1, right before the kernel's own reboot call. The
  # CPLD then resets the whole board (USB hubs, PHYs, uC), not only the SoC.
  # Factory sequence, disassembled from al_dmc12_restart(): 0x29 <- 1, then
  # 0x28 <- 1; the CPLD acts within a second (u01, 2026-11-15). If the writes fail
  # we return and the kernel falls back to the sp805 restart handler.
  cpldReboot = pkgs.writeShellScript "dmc12-cpld-reboot" ''
    [ "$1" = reboot ] || exit 0
    echo "dmc12: board reset via CPLD" > /dev/kmsg
    ${i2cset} -y ${toString cfg.bus} 0x68 0x29 0x01 && ${i2cset} -y ${toString cfg.bus} 0x68 0x28 0x01
    sleep 5
    echo "dmc12: CPLD reset did not happen, falling back to the kernel" > /dev/kmsg
  '';
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
      # The final step runs from NixOS's shutdown ramfs, where /nix/store is gone
      # and hooks are read from /lib/systemd/system-shutdown (u01 2026-11-15: as
      # systemd.shutdown, i.e. /etc/systemd/system-shutdown, it never ran and the
      # sp805 did a SoC-only reset). contents' store closures go into the ramfs.
      systemd.shutdownRamfs.contents."/lib/systemd/system-shutdown/dmc12-cpld-reboot".source = cpldReboot;
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
