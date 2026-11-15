# DMC-12-48R-AS0 metrics: the shared node exporter (upstream node_exporter,
# Go runs on the A57) plus the board's dmc12_* producer (CPLD fans, PSUs,
# alarms, WDT flag; labelled temperatures; P1..P48 state). Read-only: fan
# control owns CPLD 0x50/0x2a, dmc12-interruptmon owns the mask 0x21.
# Opens TCP 9100 (monitoring.nodeExporter.openFirewall).
{ config, lib, pkgs, ... }:
let
  metrics = pkgs.callPackage ../../../packages/dmc12-metrics { };
in {
  imports = [ ../../monitoring/node-exporter.nix ];
  monitoring.nodeExporter.enable = lib.mkDefault true;
  monitoring.nodeExporter.textfile.producers.dmc12 = {
    command = lib.concatStringsSep " " [
      "${metrics}/bin/dmc12-metrics"
      "--bus ${toString (config.hardware.dmc12.cpld.bus or 4)}"
      "--fans ${toString (config.hardware.dmc12Fan.fans or 3)}"
    ];
    interval = 30;
    timeout = 20;
  };
  environment.systemPackages = [ metrics ];
}
