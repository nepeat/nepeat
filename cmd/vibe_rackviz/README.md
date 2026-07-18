# vibe_rackviz

Terminal rack visualizer for NetBox. Three panes: rack list, rack elevation,
info + actions. Reads the API token from 1Password (`op read`), never writes
to NetBox.

```
cd cmd/vibe_rackviz
go run .                # TUI
go run . --list         # print racks and exit
go run . --rack MDF     # jump straight to a rack
go run . --dry-run      # power actions log only
```

`NETBOX_TOKEN` env overrides the 1Password lookup (useful when the calling
context can't pop the op authorization prompt).

## Proxies

- Standard `HTTP_PROXY` / `HTTPS_PROXY` / `NO_PROXY` are honored for NetBox
  HTTP calls.
- `PROXY=<url>` overrides them for HTTP, and — when it is SOCKS5
  (`socks5://`, `socks5h://`, `socks://`) — is **also used for SNMP**: the
  PDU driver relays its UDP datagrams through a SOCKS5 UDP ASSOCIATE, so the
  mgmt VLAN is reachable through e.g. a proxy on the jump host. An invalid
  `PROXY` value is a startup error, never a silent direct connection.
- Caveat: `ssh -D` is TCP-only (fine for NetBox HTTP, no UDP ASSOCIATE), so
  SNMP needs a proxy that supports UDP — dante, sing-box, xray, etc. An HTTP
  `PROXY` applies to HTTP only; SNMP then connects directly.

```
ssh -D 1080 -N root@100.123.83.112 &        # HTTP via jump host
PROXY=socks5://127.0.0.1:1080 go run .      # + SNMP too if the proxy does UDP
```

## Keys

| key | action |
|-----|--------|
| tab / shift+tab | cycle pane focus (wraps) |
| ← / → (or h/l) | move pane focus left/right (clamped) |
| j/k, ↑/↓ | move cursor |
| enter | rack pane: open rack · device row: open action submenu |
| f | front/rear face toggle (face is always shown in the middle pane title) |
| r | refresh current rack (drops cache) |
| q | quit |

Pane titles read `RACKS | <rack_name> <FACE> | <hostname>`. Power actions
(on/off/cycle) live in the enter submenu and lead into the confirmation
modal. Set `RACKVIZ_DEBUG=/path/to/log` to trace key/message handling.

The info panel shows role/type/serial/IP, connected switch ports (from NetBox
`connected_endpoints`), and the feeding PDU outlet(s) with **live per-outlet
draw** (`└ 45.2 W · 0.38 A`) plus a summed Total line for the device.
Selecting a PDU itself shows per-leg draw + inlet totals, polled every 30s.

Bay children (NetBox parent/child devices, e.g. shelf residents) are listed
in a "bays" section as `parent/bay  name` with power dots, the parent block
shows a `└ child, child` summary row, and the child's info pane shows its
bay. Bare 0U devices keep their own section.

Elevation block backgrounds encode **live power state**, swept from every
configured PDU when a rack loads (and re-swept after power actions / `r`):
green = powered on, red = powered off, gray = no power info (not fed by a
configured PDU, or the PDU is unreachable). 0U shelf entries get a ●
indicator instead.

0U devices (no rack position in NetBox) are listed in a "0U / shelf" section
under the elevation with their description, which by house convention holds
the shelf location.

## PDU control

Configure PDUs in `~/.config/vibe_rackviz/config.toml` (see
`config.example.toml`). Two Raritan PX3 drivers, both with outlet switching,
per-leg current/power, and inlet totals:

- **`raritan-json`** (recommended) — the Xerus JSON-RPC API over HTTPS with
  basic auth. Pure TCP, so `PROXY=socks5://…` from a plain `ssh -D` tunnel
  covers it. Self-signed PDU certs are accepted by default
  (`tls_verify = true` to enforce).
- **`raritan-snmp`** — PDU2-MIB over SNMP v2c (UDP; needs direct reachability
  or a SOCKS proxy with UDP ASSOCIATE).

Without config, PDUs get the `none` driver and all power features stay hidden.

Safety: actions are only reachable through the enter submenu and its
confirmation modal; power-off/cycle require typing the outlet number in the modal;
`--dry-run` forces log-only; every action is appended to
`~/.local/state/vibe_rackviz/actions.log`.

Before first live use, validate read-only from a host (or tunnel) that can
reach the management VLAN.

JSON-RPC (method names follow the Raritan PX3 JSON-RPC SDK — verify once
against real firmware):

```
curl -k -u admin:PASS -H 'Content-Type: application/json' \
  -d '{"jsonrpc":"2.0","method":"getState","params":{},"id":1}' \
  https://10.20.10.110/model/pdu/0/outlet/5        # outlet 6 (rids are 0-based)
curl -k -u admin:PASS -H 'Content-Type: application/json' \
  -d '{"jsonrpc":"2.0","method":"getSensors","params":{},"id":1}' \
  https://10.20.10.110/model/pdu/0/inlet/0
```

SNMP:

```
snmpwalk -v2c -c <community> 10.20.10.110 .1.3.6.1.4.1.13742.6.5.2.4.1.4.1.1   # inlet pole values
snmpget  -v2c -c <community> 10.20.10.110 .1.3.6.1.4.1.13742.6.5.4.3.1.3.1.6.14 # outlet 6 on/off state
```

then try one `power_cycle` on a sacrificial outlet with `--dry-run` off.
