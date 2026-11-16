# SONiC state as Prometheus metrics: the sonic-metrics textfile producer
# (../../../packages/sonic/metrics) under monitoring.nodeExporter, on by
# default when both services.sonic and the node exporter are enabled (e.g. by
# boards/as7760/monitoring.nix). It reads redis (CONFIG/APPL/STATE/COUNTERS/
# ASIC DBs), /run/sonic-nix/conflicts.json and "systemctl show" of the
# sonic-* units; no SONiC CLI. Families and labels: the package's script
# header and README.md (metrics); dashboard tools/grafana/dashboards/sonic.json.
{ config, lib, ... }:
let
  inherit (lib) mkOption mkIf types;
  cfg = config.services.sonic;
  ne = config.monitoring.nodeExporter;
  m = cfg.metrics;
in {
  imports = [ ../../monitoring/node-exporter.nix ];

  options.services.sonic.metrics = {
    enable = mkOption {
      type = types.bool;
      default = ne.enable;
      defaultText = lib.literalExpression "config.monitoring.nodeExporter.enable";
      description = "Publish sonic_* metrics through the node exporter's textfile collector (producer \"sonic\").";
    };
    package = mkOption {
      type = types.package;
      default = cfg.packages.metrics;
      defaultText = lib.literalExpression "config.services.sonic.packages.metrics";
      description = "The sonic-metrics producer.";
    };
    interval = mkOption {
      type = types.ints.between 10 300;
      default = 30;
      description = "Seconds between runs (one Python start, a few dozen redis round trips; about 0.5 s on an e6500).";
    };
  };

  config = mkIf (cfg.enable && ne.enable && m.enable) {
    monitoring.nodeExporter.textfile.producers.sonic = {
      command = "${m.package}/bin/sonic-metrics";
      inherit (m) interval;
      timeout = 10;
      path = [ config.systemd.package ];   # systemctl show (unit restarts)
    };
    environment.systemPackages = [ m.package ];
  };
}
