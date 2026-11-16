# The 48 front serial ports on the network: consoled (rust/consoled, from the monorepo's rust
# flake: root flake / deploy.sh only, like fand). `ssh -t console@<box> P12` attaches (one
# writer, others watch, 1 MiB RAM scrollback per port); `line` on the box (menu, list, ...).
# Line settings default to the factory's 9600 8N1; hosts set labels/baud per port, e.g.
#   services.consoled.ports.P1 = { label = "cs8210"; baud = 115200; };
# hardware.dmc12.consoleServer.enable = false leaves the ports to other tools (picocom, ...).
{ config, lib, ... }:
let
  rust = import ../../rust-modules.nix;
in
{
  imports = lib.optional (rust != null) rust.nixosModules.consoled;

  options.hardware.dmc12.consoleServer.enable = lib.mkOption {
    type = lib.types.bool;
    default = true;
    description = "Serve the 48 front serial ports with consoled (it opens every port exclusively).";
  };

  config = lib.mkIf config.hardware.dmc12.consoleServer.enable (lib.mkMerge [
    {
      assertions = [{
        assertion = rust != null;
        message = "DMC-12 serial ports are served by consoled, from the monorepo's rust flake: build from the ai_re root flake (deploy.sh).";
      }];
    }
    (lib.optionalAttrs (rust != null) {
      services.consoled = {
        enable = true;
        # /dev/P1..P48: udev names by USB path (conf/serial-ports.rules).
        ports = lib.listToAttrs (map (n: lib.nameValuePair "P${toString n}" { }) (lib.range 1 48));
      };
      monitoring.nodeExporter.textfile.producers.consoled = {
        command = "${config.services.consoled.package}/bin/line metrics";
        interval = 30;
        timeout = 10;
      };
      # quiet journal: no per-run start/stop lines (board-notes, disk writes)
      systemd.services.prometheus-textfile-consoled.serviceConfig.LogLevelMax = "notice";
    })
  ]);
}
