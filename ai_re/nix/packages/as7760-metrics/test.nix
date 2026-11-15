# Offline test of as7760-metrics and as7760-qsfp's decoder (fake I2C and hwmon),
# its output validated by promtool (strict lint). Native build:
#   nix build path:.#as7760-metrics-test   (log: nix log ...)
{ runCommand, python3, prometheus }:
runCommand "as7760-metrics-test" { nativeBuildInputs = [ python3 prometheus.cli ]; } ''
  mkdir src
  cp ${builtins.path { path = ./metrics.py; name = "metrics.py"; }} src/metrics.py
  cp ${builtins.path { path = ./test.py; name = "test.py"; }} src/test.py
  cp ${builtins.path { path = ../as7760-qsfp/qsfp.py; name = "qsfp.py"; }} src/qsfp.py
  python3 -s -E src/test.py $out
  promtool check metrics < $out/metrics.prom
  echo "promtool check metrics: ok" | tee $out/result.txt
''
