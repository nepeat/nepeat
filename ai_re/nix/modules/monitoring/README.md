# monitoring/node-exporter

`monitoring.nodeExporter` (export: `nixosModules.monitoring-node-exporter`):
Prometheus node metrics on **:9100** (all addresses, firewall opened by
default) plus a textfile directory fed by timer-driven producers.

```nix
imports = [ aiRe.nixosModules.monitoring-node-exporter ];
monitoring.nodeExporter.enable = true;
monitoring.nodeExporter.textfile.producers.mybox = {
  command = "${pkgs.my-tool}/bin/my-metrics --flag";   # prints Prometheus text, no timestamps
  interval = 30; timeout = 20;                        # seconds
};
```

| option | default | |
| --- | --- | --- |
| `implementation` | `upstream`, or `lite` on pre-POWER8 PowerPC | see below |
| `port` / `listenAddress` / `openFirewall` | `9100` / `[::]` / `true` | |
| `enabledCollectors` | `[ "systemd" ]` | on top of node_exporter's defaults |
| `disabledCollectors` | bcache btrfs dmi fibrechannel infiniband ipvs mdadm nfs nfsd nvme rapl selinux tapestats xfs zfs | nothing to see on a control plane |
| `textfile.directory` | `/run/prometheus-textfile` | tmpfs, created by tmpfiles |
| `textfile.producers.<name>` | | `command`, `interval` (30), `timeout` (20), `staleAfter` (3 x interval + timeout), `user` (root), `path`, `serviceConfig` |
| `textfile.unmanagedMaxAge` | `3600` | delete `*.prom` no producer owns after this long |

Producers run as `prometheus-textfile-<name>.service` (oneshot, timer every
`interval`). The wrapper ([textfile-producer.nix](textfile-producer.nix))
runs the command under `timeout`, writes `.<name>.prom.tmp`, appends
`textfile_producer_last_success_timestamp_seconds{producer}` and
`textfile_producer_duration_seconds{producer}`, then renames to
`<name>.prom`. On a non-zero exit, empty output or timeout it **deletes**
`<name>.prom`, so no stale values are served; the unit shows as failed.
`prometheus-textfile-cleanup.timer` (every minute) deletes a producer's file
once it is older than `staleAfter` (e.g. the timer was stopped). Each producer
gets `/run/prometheus-textfile-<name>/` (kept between runs) for caches.

## board producers

| board module | producer | families |
| --- | --- | --- |
| `boards/as7760/monitoring.nix` | [as7760-metrics](../../packages/as7760-metrics/README.md) | `switch_*`: optics, fans, board LM75s (board-neutral names) |
| `boards/dmc12/monitoring.nix` | [dmc12-metrics](../../packages/dmc12-metrics/README.md) | `dmc12_*`: CPLD fans/PSUs/alarms/WDT flag, labelled temperatures, P1..P48 |
| `switch/sonic/metrics.nix` (any board with `services.sonic`) | [sonic-metrics](../../packages/sonic/metrics/README.md) | `sonic_*`: ports, counters, PortChannels, BGP, BFD, LLDP, optics, ACL/CoPP, routes, conflicts, unit restarts (from redis) |

Producers are read-only and cheap (one run per interval, no daemon); a
board's hardware owner (fan control, interrupt monitor) keeps every write.

## implementation: upstream vs lite

Go's `ppc64` port needs POWER8 (ISA 2.07): there is no `GOPPC64` below
`power8`. nixpkgs' node_exporter **cross-builds** for
`powerpc64-unknown-linux-gnuabielfv2`, but on an e6500 it dies at runtime
start-up with SIGILL (checked with `qemu-ppc64 -cpu e6500`: the faulting
word is `7c070166`, `mtvsrd`, a POWER8 VSX move; `-cpu power8` runs it).
gccgo's libgo is far behind the Go version node_exporter needs. So on
PowerPC that is 32-bit or big-endian below POWER8 the default is
[node-exporter-lite](../../packages/node-exporter-lite): stdlib Python
(already in the appliance closure), the same flags (`--collector.X`,
`--no-collector.X`, `--collector.textfile.directory`, `--web.listen-address`),
and node_exporter's metric names and labels for: cpu, stat, loadavg, meminfo,
netdev, netclass, filesystem, diskstats, hwmon, thermal_zone, pressure,
uname, time, os, edac, filefd, conntrack, vmstat, textfile, systemd (via
`systemctl`). Others are ignored with a warning. Every scrape includes
`node_scrape_collector_success{collector}`, and `node_exporter_build_info`
has `version="lite-…"`. Standard "Node Exporter" dashboards work for these
families; netstat/sockstat/softnet/timex/... panels stay empty.

## query

```sh
curl -s http://<switch>:9100/metrics | grep -E '^(node_load1|node_hwmon_temp_celsius|switch_)'
curl -s http://<switch>:9100/metrics | promtool check metrics --extended --lint=none
```

Tests (native, offline): `nix build path:.#node-exporter-lite-test`
(fake sysfs, the sandbox's /proc, succeeding/failing/hanging producers, a
malformed file, HTTP on IPv4 and `[::]`, promtool).
