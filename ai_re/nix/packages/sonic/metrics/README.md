# sonic-metrics

SONiC switch state as `sonic_*` Prometheus metrics for the node exporter
textfile collector. Run by [modules/switch/sonic/metrics.nix](../../../modules/switch/sonic/metrics.nix)
(`services.sonic.metrics`, on by default when `services.sonic` and
`monitoring.nodeExporter` are both enabled; producer `sonic`, every 30 s,
timeout 10 s). Dashboard: Grafana `ai-re-sonic` ([tools/grafana/dashboards/sonic.json](../../../../tools/grafana/dashboards/sonic.json)),
linked from the lab overview's row of the job.

Reads redis directly (no SONiC CLI: `show` takes seconds on the e6500):
KEYS + pipelined HGETALL per table, route counts by a Lua script inside
redis; DB ids, separators and the socket from SONiC's `database_config.json`.
Stdlib Python only. Read-only. Without redis it still succeeds with
`sonic_up 0` (conflicts and services are still reported); a table that
fails gives `sonic_collector_success{collector} 0` and the rest is printed.

Port labels: `port` (SONiC name, `Ethernet0`) and `alias` (`Ethernet1/1`).

| family | source | labels |
| --- | --- | --- |
| `sonic_up`, `sonic_collector_success`, `sonic_collect_duration_seconds` | the run | collector |
| `sonic_device_info`, `sonic_config_db_initialized`, `sonic_port_init_done` | CONFIG_DB DEVICE_METADATA, APPL_DB PortInitDone | platform, hwsku, type, bgp_asn |
| `sonic_port_info`, `_admin_up`, `_oper_up`, `_speed_bytes`, `_mtu_bytes` | CONFIG_DB PORT, APPL_DB PORT_TABLE | port, alias (+ index, lanes) |
| `sonic_port_{rx,tx}_{bytes,packets,errors,discards}_total` | COUNTERS_DB (flex counter PORT) | port, alias |
| `sonic_portchannel_{admin_up,oper_up,members,members_selected,min_links}` | CONFIG_DB PORTCHANNEL*, APPL_DB LAG_TABLE | portchannel |
| `sonic_portchannel_member_{selected,enabled}` | STATE_DB LAG_MEMBER_TABLE (tlm_teamd), APPL_DB | portchannel, port, alias |
| `sonic_bfd_session_{state,up,info,tx_interval_seconds,rx_interval_seconds,detect_multiplier}` | STATE_DB BFD_SESSION_TABLE (bfdsyncd); state 0 AdminDown, 1 Down, 2 Init, 3 Up | vrf, interface, alias, peer |
| `sonic_bgp_neighbor_{info,admin_up}` | CONFIG_DB BGP_NEIGHBOR (all VRFs) | vrf, neighbor, name (+ asn, local_addr, bfd) |
| `sonic_bgp_neighbor_{state,established}` | STATE_DB NEIGH_STATE_TABLE (bgpmon); 1 Idle .. 6 Established (RFC 4273), 0 other | vrf, neighbor, name |
| `sonic_lldp_neighbors`, `sonic_lldp_neighbor_info` | APPL_DB LLDP_ENTRY_TABLE (lldp_syncd) | port, alias (+ remote_system, remote_port, remote_port_desc, remote_chassis) |
| `sonic_transceiver_{present,info,temperature_celsius,voltage_volts}` | STATE_DB TRANSCEIVER_INFO / _DOM_SENSOR (xcvrd) | port, alias (+ type, vendor, model) |
| `sonic_acl_rule_{info,active,packets_total,bytes_total}` | CONFIG_DB ACL_RULE, STATE_DB ACL_RULE_TABLE, COUNTERS_DB ACL_COUNTER_RULE_MAP | table, rule (+ action, priority) |
| `sonic_copp_group_{info,cir,cbs,traps}`, `sonic_copp_trap_info`, `sonic_copp_trap_{packets,bytes}_total` | APPL_DB COPP_TABLE (coppmgrd), COUNTERS_TRAP_NAME_MAP | group, trap |
| `sonic_routes`, `sonic_asic_routes` | APPL_DB ROUTE_TABLE, ASIC_DB route entries | vrf, family |
| `sonic_nix_conflicts`, `_by_table`, `sonic_nix_notes`, `sonic_nix_conflicts_updated_timestamp_seconds` | /run/sonic-nix/conflicts.json | table |
| `sonic_service_{active,restarts,start_timestamp_seconds}` | `systemctl show` of sonic-*.service + redis-sonic.service | unit |

Limits: bgpmon reports the default VRF only, so BGP neighbors in other VRFs
have `_info` but no `_state`. ACL counters exist only while `counterpoll
acl enable` is on (off by default); trap counters need flex counter
FLOW_CNT_TRAP, which sai-openbcm does not answer. `sonic_service_restarts`
is systemd's NRestarts (automatic restarts since the last manual start);
restarts cascaded by BindsTo/Upholds show as `changes(sonic_service_start_timestamp_seconds[..])`.
A BFD flap shorter than the interval can be missed (state, not events).

## check

```sh
# on the switch
systemctl start prometheus-textfile-sonic.service; systemctl status prometheus-textfile-sonic.service --no-pager | head -5
curl -s localhost:9100/metrics | grep -E '^sonic_(up|collector_success|collect_duration)|producer="sonic"'
curl -s localhost:9100/metrics | grep -E '^sonic_(port_oper_up\{port="Ethernet(0|124)"|bgp_neighbor_state|bfd_session_state|routes|nix_conflicts )'
sonic-metrics | promtool check metrics   # promtool if installed; else just sonic-metrics | less
```

Tests (native): `nix build -L --impure --expr 'let f = builtins.getFlake "path:'$PWD'/nix"; pkgs = import f.inputs.nixpkgs { system = "x86_64-linux"; }; in pkgs.callPackage ./nix/packages/sonic/metrics/test.nix { }'`
(from ai_re/: real redis-server with a fake SONiC dump, redis missing, a broken
table, 128 ports, promtool) and the VM test
[tests/vm-metrics.nix](../../../modules/switch/sonic/tests/vm-metrics.nix) (live
stack + peer: PortChannel, BGP+BFD, Vrf-test, LLDP, fake optics, ACL, CoPP,
a conflict, a killed unit, redis stopped; ~2 min). Evidence:
[devices/sonic-nixos/sonic-metrics-20261115.txt](../../../../devices/sonic-nixos/sonic-metrics-20261115.txt).
