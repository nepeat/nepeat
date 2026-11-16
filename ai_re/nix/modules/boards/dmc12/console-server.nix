# The 48 front serial ports on the network: consoled (rust/consoled, from the monorepo's rust
# flake: root flake / deploy.sh only, like fand). `ssh -t console@<box> P12` attaches (one
# writer, others watch, 1 MiB RAM scrollback per port); `console` on the box lists them.
# Line settings default to the factory's 9600 8N1; hosts set labels/baud per port, e.g.
#   services.consoled.ports.P1 = { label = "cs8210"; baud = 115200; };
{ config, lib, aiReRust ? null, ... }:
{
  imports = lib.optional (aiReRust != null) aiReRust.nixosModules.consoled;

  config = lib.mkMerge [
    {
      assertions = [{
        assertion = aiReRust != null;
        message = "DMC-12 serial ports are served by consoled, from the monorepo's rust flake: build from the ai_re root flake (deploy.sh).";
      }];
    }
    (lib.optionalAttrs (aiReRust != null) {
      services.consoled = {
        enable = true;
        # /dev/P1..P48: udev names by USB path (conf/serial-ports.rules).
        ports = lib.listToAttrs (map (n: lib.nameValuePair "P${toString n}" { }) (lib.range 1 48));
      };
      monitoring.nodeExporter.textfile.producers.consoled = {
        command = "${config.services.consoled.package}/bin/console --metrics";
        interval = 30;
        timeout = 10;
      };
      # quiet journal: no per-run start/stop lines (board-notes, disk writes)
      systemd.services.prometheus-textfile-consoled.serviceConfig.LogLevelMax = "notice";
    })
  ];
}
