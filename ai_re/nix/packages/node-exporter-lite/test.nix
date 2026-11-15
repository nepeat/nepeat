# Offline test of node-exporter-lite and the textfile producer wrapper
# (../../modules/monitoring/textfile-producer.nix): fake sysfs, the build
# sandbox's /proc, producers that succeed / fail / hang, a malformed file,
# HTTP on IPv4 and [::]. promtool validates the output. Native build:
#   nix build path:.#node-exporter-lite-test
{ runCommand, writeShellScript, callPackage, python3, prometheus }:
let
  lite = builtins.path { path = ./node_exporter_lite.py; name = "node_exporter_lite.py"; };
  mkProducer = callPackage ../../modules/monitoring/textfile-producer.nix { };
  # The directory is expanded by the wrapper at run time.
  producer = name: timeout: script: mkProducer {
    inherit name timeout;
    directory = "$TF";
    command = "${writeShellScript "fake-${name}" script}";
  };
  ok = producer "ok" 5 ''
    printf '# HELP fake_value_celsius A test value.\n# TYPE fake_value_celsius gauge\nfake_value_celsius{port="1"} 21.5'
  '';
  fail = producer "fail" 5 "echo partial_metric 1; exit 3";
  hang = producer "hang" 1 "echo partial_metric 1; exec sleep 30";
in
runCommand "node-exporter-lite-test" { nativeBuildInputs = [ python3 prometheus.cli ]; } ''
  set -euo pipefail
  export TF=$PWD/tf
  mkdir -p tf sys/class/hwmon/hwmon0 sys/class/net/eth0 sys/devices/system/clocksource/clocksource0
  h=sys/class/hwmon/hwmon0
  echo lm75 > $h/name; echo 31500 > $h/temp1_input; echo board_1 > $h/temp1_label
  echo 255 > $h/pwm1; echo 0 > $h/fan1_fault; echo 15100 > $h/fan1_input
  d=sys/class/net/eth0
  echo up > $d/operstate; echo 1 > $d/carrier; echo 1500 > $d/mtu; echo 1000 > $d/speed
  echo 02:00:00:00:00:01 > $d/address; echo 0x1003 > $d/flags; echo 2 > $d/ifindex; echo 1 > $d/type
  echo 3 > $d/carrier_changes; echo full > $d/duplex
  echo timebase > sys/devices/system/clocksource/clocksource0/current_clocksource

  # Producers: stale files from an earlier run must vanish when a run fails.
  echo 'stale_metric 1' > tf/fail.prom
  echo 'stale_metric 1' > tf/hang.prom
  ${ok}
  if ${fail}; then echo "failing producer reported success"; exit 1; fi
  if ${hang}; then echo "hanging producer reported success"; exit 1; fi
  if [ "$(ls -A tf)" != ok.prom ]; then ls -la tf; exit 1; fi
  grep -q '^textfile_producer_last_success_timestamp_seconds{producer="ok"} [0-9]*\.[0-9]\{9\}$' tf/ok.prom
  grep -q '^fake_value_celsius{port="1"} 21.5$' tf/ok.prom
  # A second producer file shares the textfile_producer_* families; one is malformed.
  sed 's/producer="ok"/producer="ok2"/; s/port="1"/port="2"/' tf/ok.prom > tf/ok2.prom
  printf 'broken{label="x" 1\n' > tf/bad.prom

  run() { python3 -s -E ${lite} --path.sysfs=$PWD/sys --collector.textfile.directory=$TF "$@"; }
  run --once --no-collector.systemd --collector.zfs > out.prom 2> err.txt
  cat err.txt
  grep -q "collector 'zfs' not implemented" err.txt
  promtool check metrics --extended --lint=none < out.prom > /dev/null
  if promtool check metrics < out.prom 2>&1 | grep -v camelCase | grep .; then
    echo "promtool lint problems besides node_exporter's camelCase meminfo names"; exit 1
  fi
  for want in \
    'node_hwmon_temp_celsius{chip="lm75",sensor="temp1"} 31.5' \
    'node_hwmon_sensor_label{chip="lm75",label="board_1",sensor="temp1"} 1' \
    'node_hwmon_fan_rpm{chip="lm75",sensor="fan1"} 15100' \
    'node_hwmon_fan_fault{chip="lm75",sensor="fan1"} 0' \
    'node_hwmon_pwm{chip="lm75",sensor="pwm1"} 255' \
    'node_network_up{device="eth0"} 1' \
    'node_network_speed_bytes{device="eth0"} 125000000' \
    'node_network_info{address="02:00:00:00:00:01",adminstate="up",broadcast="",device="eth0",duplex="full",ifalias="",operstate="up"} 1' \
    'node_time_clocksource_current_info{clocksource="timebase",device="0"} 1' \
    'fake_value_celsius{port="1"} 21.5' 'fake_value_celsius{port="2"} 21.5' \
    'node_textfile_scrape_error 1' \
    'node_scrape_collector_success{collector="cpu"} 1' \
    'node_scrape_collector_success{collector="meminfo"} 1' \
    'node_scrape_collector_success{collector="textfile"} 1'; do
    grep -qxF "$want" out.prom || { echo "missing: $want"; cat out.prom; exit 1; }
  done
  grep -q '^node_cpu_seconds_total{cpu="0",mode="idle"} ' out.prom
  grep -q '^node_memory_MemTotal_bytes ' out.prom
  [ "$(grep -c '^# TYPE textfile_producer_last_success_timestamp_seconds ' out.prom)" = 1 ]
  [ "$(grep -c '^textfile_producer_last_success_timestamp_seconds{' out.prom)" = 2 ]
  if grep -q '^broken' out.prom || grep -q 'collector="systemd"' out.prom; then exit 1; fi

  # HTTP: IPv4 and the dual-stack default address, fetched over 127.0.0.1.
  for addr in 127.0.0.1:19100 '[::]:19101'; do
    run --no-collector.systemd --web.listen-address="$addr" & pid=$!
    python3 - "''${addr##*:}" <<'EOF'
  import sys, time, urllib.request
  for _ in range(100):
      try:
          body = urllib.request.urlopen(f"http://127.0.0.1:{sys.argv[1]}/metrics", timeout=5).read().decode()
          break
      except OSError:
          time.sleep(0.1)
  else:
      sys.exit("exporter did not answer")
  assert "node_load1 " in body and "fake_value_celsius" in body, body[:500]
  EOF
    kill $pid
  done

  mkdir $out
  cp out.prom $out/metrics.prom
  echo "node-exporter-lite tests passed" | tee $out/result.txt
''
