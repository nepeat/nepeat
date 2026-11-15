# dmc12-metrics: DMC-12-48R-AS0 CPLD (fans, PSUs, alarms, WDT flag), labelled
# board temperatures and serial-port state for the node exporter textfile
# collector. Read-only; see README.md.
{ writeShellApplication, python3 }:
let
  py = python3.withPackages (ps: [ ps.smbus2 ]);   # same env as boards/dmc12/cpld
  src = builtins.path { path = ./metrics.py; name = "dmc12-metrics.py"; };
in writeShellApplication {
  name = "dmc12-metrics";
  runtimeInputs = [ py ];
  text = ''exec python3 -s -E ${src} "$@"'';
}
