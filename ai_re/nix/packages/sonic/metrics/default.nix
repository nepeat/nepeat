# sonic-metrics: SONiC switch state (ports, counters, PortChannels, BFD, BGP,
# LLDP, transceivers, ACL/CoPP, routes, sonic-nix conflicts, unit restarts) as
# sonic_* Prometheus metrics for the node exporter textfile collector. Reads
# redis directly, stdlib Python only. Used by modules/switch/sonic/metrics.nix;
# test: ./test.nix.
{ writeShellApplication, python3 }:
writeShellApplication {
  name = "sonic-metrics";
  runtimeInputs = [ python3 ];
  # -S: no site module (start-up time on the e6500); stdlib only.
  text = ''exec python3 -I -S ${builtins.path { path = ./sonic_metrics.py; name = "sonic_metrics.py"; }} "$@"'';
}
