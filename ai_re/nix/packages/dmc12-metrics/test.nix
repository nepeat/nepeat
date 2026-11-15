# Offline test of dmc12-metrics (fake smbus2, u01's CPLD dump, fake sysfs,
# /dev and /proc), output validated by promtool. Native build:
#   nix build path:.#dmc12-metrics-test   (log: nix log ...)
{ runCommand, python3, prometheus }:
runCommand "dmc12-metrics-test" { nativeBuildInputs = [ python3 prometheus.cli ]; } ''
  mkdir src
  cp ${builtins.path { path = ./metrics.py; name = "metrics.py"; }} src/metrics.py
  cp ${builtins.path { path = ./test.py; name = "test.py"; }} src/test.py
  python3 -s -E src/test.py $out
  promtool check metrics < $out/metrics.prom
  echo "promtool check metrics: ok" | tee $out/result.txt
''
