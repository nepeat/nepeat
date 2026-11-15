# SONiC config on NixOS: who owns what (design, 2026-11-15)

Decided with the owner; nothing is implemented yet. Hybrid: NixOS owns the
host and declared switch keys, the SONiC way (`config` CLI, `config save`)
owns the rest.

## layers

| layer | owner | examples |
| --- | --- | --- |
| host | NixOS only; never in CONFIG_DB (the loader drops these tables) | hostname, management port (`MGMT_INTERFACE`), users, SSH, NTP, syslog, fans, platform drivers |
| `enforce` | Nix value wins at boot and at `nixos-rebuild switch` | `DEVICE_METADATA` (platform, hwsku, MAC), the hardware part of `PORT` (lanes, alias, index), anything declared `enforce` |
| `seed` | Nix gives the first value; a saved user value sticks | port speed/FEC/admin state, anything declared `seed` |
| unmanaged | user (`config ...`, `config save`) | VLAN, PORTCHANNEL, INTERFACE, BGP, ... unless declared in Nix |

- Persistent user layer: `config save` writes `/var/lib/sonic/config_db.json`.
- Boot: Nix base + user file merged by the rules above, YANG-validated,
  loaded into CONFIG_DB. `nixos-rebuild switch` writes only changed
  enforced/seeded keys into the running CONFIG_DB (no restart).
- Undeclared front ports stay **admin-down**.
- The Nix side is YANG-validated at build time.

## conflicts (owner's call)

- A runtime or saved change to an `enforce` key is **not reverted live**:
  it is logged as a warning and loses at the next boot / rebuild.
- The pending conflicts (key, Nix value, current value, where it came from)
  are written to `/etc/issue` (and `/run/sonic-nix/conflicts.json`), and
  `sonic-nix-diff` prints the full diff, so whoever logs in sees what will
  go away.

## ports

- hwsku pinned: **AS7760-32X 32x100G** (every cage 1x100G).
- Breakout (one QSFP28 cage = 4 lanes: 1x100G, 2x50G, 4x25G, 4x10G) is a
  per-port Nix override, enforced, because it describes cabling and needs an
  SDK re-init. **The split must be explicit** (owner, 2026-11-15): an
  overridden cage lists every resulting port (name, lanes, speed), e.g. cage
  1 -> Ethernet0 lane 1 25G, Ethernet1 lane 2 25G, ...; no mode shorthand
  that generates them. Build-time checks: each lane of the cage used exactly
  once, speeds valid for the lane count, names unique, no port left over from
  the 1x100G default. Nix generates `config.bcm` (the factory 32x100 config
  matches public sonic-buildimage AS7712 values), so no vendor file is copied
  by hand. The lab loop (port 1 breakout into port 32 via QSA) needs ports
  1 and 32 set to 4x25G (or 4x10G) to link.
