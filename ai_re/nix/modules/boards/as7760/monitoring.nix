# AS7760-32X metrics: node exporter (lite on e6500, see
# ../../monitoring/node-exporter.nix) plus the board's switch_* producer
# (optics, fans, board temperatures). Read-only; fan control is untouched.
{ config, lib, pkgs, ... }:
let
  metrics = pkgs.callPackage ../../../packages/as7760-metrics { };
in {
  imports = [ ../../monitoring/node-exporter.nix ];
  monitoring.nodeExporter.enable = lib.mkDefault true;
  monitoring.nodeExporter.textfile.producers.as7760 = {
    command = lib.concatStringsSep " " [
      "${metrics}/bin/as7760-metrics"
      "--cache /run/prometheus-textfile-as7760/optics-static.json"
      "--quiet-duty ${toString config.hardware.as7760Fan.duty}"
      "--thermal-unit as7760-thermal.service"
    ];
    interval = 30;
    timeout = 20;
    path = [ config.systemd.package ];
  };
  environment.systemPackages = [ metrics ];
}
