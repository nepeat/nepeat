# NixOS hosts

Two machines, both on `base.nix`:

| host | what it is | root |
| --- | --- | --- |
| `sea420-desktop` | the big AMD desktop | ZFS-on-LUKS, local SATA SSD, impermanence via `/persist` |
| `chickennugget` | the mini desktop thin client (UM350) | XFS on an iSCSI LUN, attached in stage 1 from iBFT |

`chickennugget`'s console shows a glances dashboard rather than a login prompt
(`services.glancesTty`). It runs on **tty7**, not the megarepo's tty5: logind
spawns `autovt@ttyN` for any VT you switch to while N <= `NAutoVTs` (default 6,
unset here), so a getty would have raced glances for the terminal. Alt+F1 still
reaches a normal login.

```
nixos/
  base.nix                    shared by both
  module/
    agent-secrets.nix         vault-rendered opencode / openviking secrets
    erin.nix                  erin@ + the shared home-manager config
    glances-tty.nix           glances pinned to a VT, switched to at boot
    impermanence.nix          /persist bind mounts + zroot rollback in stage 1
    nvidia.nix                cuda + open kernel module
    iscsi-ibft.nix            attach the root LUN from iBFT (stage-1 systemd unit)
    iscsi-xfs.nix             disko: ESP + XFS root on that LUN
    remote-builders.nix       offload builds to 10g.warc.zip / dreamflasher
    vault-agent.nix           AppRole vault-agent (verbatim from the megarepo)
  machine/<name>/             per-host, `default.nix` is the entrypoint
```

Deploy:

```
nixos-rebuild switch --flake .#sea420-desktop
nixos-rebuild switch --flake .#chickennugget --target-host root@<addr>
```

## home-manager

`mkNixos` loads `home-manager.nixosModules.home-manager` on every host and
passes `isStandalone = false`, which is the same signal the darwin configs use.
`nixos/module/erin.nix` then imports `module/{claude,opencode,openviking}.nix`
at **system** level — each branches on `isStandalone`, and the false branch
registers its home config via `home-manager.sharedModules` and adds its binary
cache to the system's `nix.settings`. erin's own home config is
`base/home.nix` + `machine/nonwork_home.nix`, the same files
`homeConfigurations.erin` uses.

Two files from the standalone config are deliberately left out:
`base/non_nix_home.nix` (its own comment scopes it to non-NixOS hosts, and it
sets `nix.package = pkgs.lix` at home level) and `base/configuration.nix`
(system-level nix settings that `nixos/base.nix` already covers).

## Secrets

`chickennugget` runs vault-agent with AppRole auth, the same shape as the
megarepo fleet. The role_id/secret_id pair never enters the closure — it is
seeded out of band and lives in `/var/lib/vault-agent`:

```
just vault-provision chickennugget 10.36.75.107   # at install time
just vault-rekey     chickennugget 10.36.75.107   # fresh secret_id later
```

`agentSecrets` then renders, via KV v2 mount `secret` (the same mount
`module/openviking/setup-openviking-config.sh` uses). Everything sits under
`infra/` because that is exactly what the `nixos-core` policy already grants
(`secret/data/infra/*`) — no policy widening, and no new AppRole:

| logical path | field | lands as |
| --- | --- | --- |
| `infra/ai/opencode` | `key` | `OPENCODE_GO_KEY` in `/run/secrets/opencode.env` |
| `infra/ai/openrouter` | `key` | `OPENROUTER_KEY` in the same file |
| `infra/ai/opencode` | `key` | `opencode` + `opencode-go` in `~/.local/share/opencode/auth.json` |
| `infra/ai/openrouter` | `key` | `openrouter` in the same `auth.json` |
| `infra/openviking` | `client-api-key` | `api_key` in `/home/erin/.openviking/ovcli.conf` |

`infra/openviking` is a **copy** of `app/openviking`, not a move —
`setup-openviking-config.sh` and an ArgoCD VaultSecret in the megarepo still
read the original, so rotations have to touch both. The AI keys were moved
outright (the old `app/ai/*` paths are soft-deleted and can be `kv undelete`d).

`auth.json` is opencode's own credential store — the file `/connect` writes.
Because vault-agent owns it now, **any provider added interactively there is
overwritten on the next render**; add more to `agentSecrets.opencode.authProviders`
instead — an attrset of provider id to `{ path; field; }`, so providers can come
from different KV paths. `opencode` is OpenCode Zen and `opencode-go` is OpenCode Go: separate
providers, separately billed, but one key from opencode.ai/auth covers both.

The env file is tmpfs-only and sourced from erin's zsh if readable, so a login
shell still works before vault-agent has authenticated. Field names are
options — see `opencode.goKeyField` etc.

Two gotchas baked into the module:

- KV v2 puts the payload one level down, so `app/openviking` is read as
  `secret/data/app/openviking` with values under `.Data.data`.
- Go templates cannot parse a hyphen in a field name (`.Data.data.client-api-key`
  parses as a subtraction), so lookups use `index .Data.data "client-api-key"`.

## Remote builders

All NixOS hosts get `nix.remoteBuilders` (on by default): `10g.warc.zip` for
x86_64-linux and `dreamflasher.skate-gopher.ts.net` for aarch64-linux, matching
what `base/darwin.nix` and `machine/m1laptop.nix` already give the Macs.
`builders-use-substitutes` is on so builders fetch dependencies themselves.

The key at `nix.remoteBuilders.sshKey` (default `/root/.ssh/id_ed25519`) is
**not** managed here — it must exist on the host and be authorized on each
builder. Until then nix just builds locally.

chickennugget's key is generated and its public half is:

```
ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIHN2kQPpCWD+YjvmPl8zzvm+4TFCYONUVjmYENMPh/YM root@chickennugget
```

It is authorized for `erin@10g.warc.zip` (verified: `nix store info` reports
`Trusted: 1`, and a `--max-jobs 0` build really ran there). `dreamflasher` still
needs this key added to `root@`'s authorized_keys. Remember `ssh-keyscan` for
each new builder — nix inherits root's `known_hosts` and an unknown host key
fails the connection.

## Notes

- **`chickennugget` is not host-specific.** Initiator IQN, target, portal and IP
  all come from firmware, so the same closure boots on any host that
  `aislop/ipxe_iscsi_mgr` has assigned a volume to. It was `generic-iscsi` in
  the generalprogramming megarepo.
- **`disko.devices.disk.iscsi0.device = "/dev/sda"`** is safe here because the
  LUN is the only SCSI device — the UM350's local NVMe is `nvme0n1` and disko
  never touches it. Check `lsblk -o NAME,TRAN` for `TRAN=iscsi` if a host ever
  grows a second SCSI disk.
- **erin@ is SSH-key-only** (`hashedPassword = "!"`). No password file needed
  to switch. `passwd erin` will not survive a reboot — impermanence rebuilds
  `/etc/shadow` every boot — so for a console password put the hash on
  `/persist` (`mkpasswd -m yescrypt | sudo tee /persist/passwords/erin`) and
  switch `erin.nix` to the `hashedPasswordFile` line.
- **`sudo` will prompt erin for a password it does not have.** Either give it
  one as above, or set `security.sudo.wheelNeedsPassword = false;`.
- **gitops is off.** The megarepo's comin-based `gitops` module did not come
  across; it was already `enable = false` on the desktop and it reads machine
  IDs out of the megarepo's `vars/machines.nix`. Deploy by hand for now.
- **Three other megarepo modules were left behind on purpose:** `dns`
  (netbox/ipa/consul-specific), `glances-tty` (needs the org's pinned glances
  package), and the org base's `nix-cache` / `ssh-ca` / `nixos-tags` (Attic on
  fmt2 k8s, an SSH CA, and fleet tagging).
- **These hosts are on stock nix, not lix.** The darwin configs in this flake
  use `lix-module`; the NixOS ones deliberately do not.
- `nixpkgs` here tracks `master`, which is looser than the `nixos-unstable`
  these machines used to build against. Expect the occasional broken package.
