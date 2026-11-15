# Front-panel aliases `Ethernet<cage>/<sub>` vs SONiC's name assumptions (2026-11-15)

Checked in the pinned sonic-utilities, swss, sonic-py-common and
sonic-config-engine (grep for `Ethernet\d`, `startswith("Ethernet")`,
IFNAMSIZ / `isInterfaceNameValid`, natsort, `split('/')`, alias in file
names / redis keys, plus an AST scan of every config command taking a port
argument), then exercised in the VM test (`config ... Ethernet3/1` etc.).
Fixes: `nix/packages/sonic/python/patches/0002-*`, `0003-*`.

| assumption | where | bites? |
| --- | --- | --- |
| SONiC names `Ethernet<N>`, `^Ethernet(\d+)$` (port_util, SNMP index) | sonic-py-common port_util, interface.py | no: names stay `Ethernet<N>`, only the alias has `/` |
| `startswith("Ethernet")` before/after alias conversion | config/main.py (portchannel member, storm control, nat), intfutil, ipintutil | no: aliases also start with `Ethernet`; checks run on converted names |
| alias -> name by exact match on PORT `alias` | utilities_common/cli.py, config/main.py `interface_alias_to_name` | no |
| name -> alias in alias-mode output: regex `(^\|\s)Ethernet1($\|,?\s)` per port | cli.py `run_command_in_alias_mode` | no: the `/` after `Ethernet1` in `Ethernet1/1` stops prefix collisions |
| sub-interface separator `.` | cli.py, config/main.py | no: `Ethernet1/1.10` -> `Ethernet0.10` |
| ranges `Ethernet0-3` | utilities_common/intf_filter.py | **yes**: alias ranges were rejected, and alias filters (comma lists too) reached CONFIG_DB unconverted in `config interface startup/shutdown`. Patched (0003): `Ethernet1/1-4`, `Ethernet1/3-3/1` (missing subports are gaps). `config vlan member` takes one port (its `-m` is for VLAN ids); `show interfaces status <range>` still takes names only |
| IFNAMSIZ (<16) / `isInterfaceNameValid` | sonic-cfggen, fdbsyncd, intfmgrd | no: applied to names (max `Ethernet124`) |
| redis key separators `\|` `:` | everywhere | no: keys hold names, `/` only in the alias field |
| PORT `alias` used as a netdev or file name | orchagent porthlpr (parsed, unused), portmgrd/teammgrd ("alias" = SONiC name) | no |
| natsort order | intfutil, show | no: sorted by name (Ethernet0, 1, 2, 3, 4, 8, ...) |
| `config portchannel member add/del` skip alias conversion | config/main.py | **yes**: wrote `PORTCHANNEL_MEMBER\|PortChannel0001\|Ethernet5/1`. Patched (0002) |
| `run_command(..., return_cmd=True)` in alias mode returns None | utilities_common/cli.py | **yes** (any alias-mode caller that parses output, e.g. reload's service checks). Patched (0002) |
| other commands without alias conversion: sflow, mirror (erspan/span), pfcwd start, buffer pg/queue, storm-control add/del, pc sys-id | config/main.py | yes in alias mode, outside the tested scope; not patched (use SONiC names or `SONIC_CLI_IFACE_MODE=default`) |
| `sonic_sku_create.py` parses `Ethernet<i>/<lane>` itself | scripts | not used here |
| `show interfaces status` first column | intfutil | shows the SONiC name; the alias is in the Alias column (by design) |
