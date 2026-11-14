# PaperMC on CS8210

Standalone deployment at `root@10.60.2.190:/root/papermc`.
Pinned nixpkgs supplies Paper 26.3-223 and headless Java 25 for ARM64.
The flake also supports x86_64-linux. The lock file pins the package and jar hash.

## Resources and gameplay

- 4 GiB systemd MemoryMax, 3.5 GiB MemoryHigh, no swap.
- Java heap 1 GiB initial / 2560 MiB maximum; four JVM-visible processors.
- CPUWeight=25 and Nice=10 favor other switch work during contention.
- Survival, normal difficulty, online authentication, six player slots.
- View distance 6; simulation distance 4.
- Binds 10.60.2.190:25565; RCON on 25575 is firewall-blocked externally.
- Whitelist disabled; authenticated accounts may join. cuttlefishCuller is op.
- Service adds/removes one labeled IPv4 INPUT rule for that address and port.
  No router forwarding is configured.

## Human-friendly controls

In /root/papermc, run `just` for the menu. Available commands:
`just start`, `just stop`, `just restart`, `just status`, `just logs`,
`just recent`, `just memory`, `just join`, and `just boot-status`.
`just build` rebuilds the pinned server without restarting;
`just deploy` builds and restarts. Neither updates the lock file.

The flake exports `packages.just` and a devshell with just and Python:
`nix develop path:/root/papermc`.
The switch has just installed in root's Nix profile. On a fresh host use
`nix profile add path:/root/papermc#just`.

## Minecraft command console

Run `just console` in /root/papermc for an interactive Minecraft admin
prompt (commands without a leading slash; type `Q` to disconnect).
For a single command: `just command "list"` or
`just command "whitelist add PlayerName"`.
Use `just logs` separately for the live server log; RCON shows command replies.
This is a Minecraft console; SSH provides the Linux shell.

The mcrcon package is in the flake/devshell and root's Nix profile.
A random RCON password lives only in the root-readable live server.properties;
console.sh passes it via environment, not command arguments. No RCON firewall
opening was added. configure-console.sh initializes this on a fresh install
and restarts the service; it also disables the whitelist.
Online authentication remains enabled. No internet router forwarding is configured.

## Operations (on the switch)

```sh
cd /root/papermc
nix build path:/root/papermc
systemctl restart papermc
journalctl -u papermc -f
systemctl stop papermc
```

Worlds, plugins, configuration, and logs live in /root/papermc.
Stop the service before copying worlds for a consistent backup.
The result symlink roots the runtime against Nix garbage collection.
First startup downloads Mojang's base jar and applies Paper patches.

Persistent unit and boot dependency symlinks live in
/etc/systemd/system.attached, a confirmed systemd search path on this NixOS
host. This avoids editing the concurrent NixOS hardware work.
The installer records Minecraft EULA acceptance for the requested deployment.
No reboot was performed; boot dependency is checked through systemd.
Because this uses the separate attached directory, `systemctl is-enabled`
reports disabled; `systemctl show multi-user.target -p Wants` confirms the
persistent boot dependency.
The service waits up to 120 seconds for its DHCP address before starting.
If the management IP changes, update server.properties, firewall.sh,
wait-address.sh, and the justfile join recipe.

To update deliberately: back up the worlds, stop the service, run
`nix flake update` and `nix build path:/root/papermc`, then restart.
Do not downgrade an existing world without restoring a compatible backup.

To remove: stop papermc, remove only its two symlinks under
/etc/systemd/system.attached, and run systemctl daemon-reload.
Keep /root/papermc until its worlds have been backed up.

Local read-only network check:
`nix develop -c python3 devices/cs8210-32x/papermc/status.py`.

## WorldEdit

WorldEdit 7.4.6 (Bukkit/Paper, supports Minecraft 26.3) is pinned with SHA-256
in flake.nix as packages.worldedit. Upstream metadata and download checksums
are recorded in worldedit.json; the download was checked against Modrinth SHA-512.

Reproduce installation with the server stopped:

```sh
cd /root/papermc
nix build path:/root/papermc#worldedit --out-link worldedit-package
ln -sfn ../worldedit-package plugins/worldedit.jar
just start
just command "worldedit version"
```

The worldedit-package out-link keeps the plugin in the Nix store.
WorldEdit configuration is in plugins/WorldEdit. Operators can use WorldEdit;
in game, start with //wand. No world edits were made during verification.
