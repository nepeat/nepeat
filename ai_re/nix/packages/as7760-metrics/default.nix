# as7760-metrics: AS7760-32X optics / fan / board-temperature metrics for the
# node exporter textfile collector. Shares the EEPROM decoder with as7760-qsfp.
{ runCommand, writeShellApplication, python3 }:
let
  src = runCommand "as7760-metrics-py" { } ''
    mkdir $out
    cp ${builtins.path { path = ./metrics.py; name = "metrics.py"; }} $out/metrics.py
    cp ${builtins.path { path = ../as7760-qsfp/qsfp.py; name = "qsfp.py"; }} $out/qsfp.py
  '';
in writeShellApplication {
  name = "as7760-metrics";
  runtimeInputs = [ python3 ];
  text = ''exec python3 -s -E ${src}/metrics.py "$@"'';
}
