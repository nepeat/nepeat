# Offline test of sonic-metrics: a real redis-server loaded with a fake SONiC
# dump (lab shape, then every cage split: 128 ports), redis missing, a broken
# table; output validated by promtool (strict lint). Native build, from ai_re/:
#   nix build -L --impure --expr 'let f = builtins.getFlake "path:'$PWD'/nix";
#     pkgs = import f.inputs.nixpkgs { system = "x86_64-linux"; };
#     in pkgs.callPackage ./nix/packages/sonic/metrics/test.nix { }'
{ runCommand, python3, redis, prometheus }:
runCommand "sonic-metrics-test" { nativeBuildInputs = [ python3 redis prometheus.cli ]; } ''
  mkdir src
  cp ${builtins.path { path = ./sonic_metrics.py; name = "sonic_metrics.py"; }} src/sonic_metrics.py
  cp ${builtins.path { path = ./test.py; name = "test.py"; }} src/test.py
  python3 -I src/test.py $out | tee test-output.txt
  python3 -I src/test.py $out/wide --wide | tee -a test-output.txt
  cp test-output.txt $out/
  for f in $out/metrics.prom $out/metrics-no-redis.prom $out/metrics-degraded.prom $out/wide/metrics.prom; do
    promtool check metrics < $f
  done
  echo "promtool check metrics: ok (4 files)" | tee -a $out/result.txt
''
