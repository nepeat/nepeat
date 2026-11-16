# consoled: serial console server

Shares a box's serial ports over SSH the way console servers do (Opengear/Digi/Avocent
style, and the DMC-12 factory OS's ser2net-behind-ssh): one daemon owns the ports, users
attach through OpenSSH.

```
line                  menu: ↑↓/jk pick, ⏎ attach, w watch, i info, digits jump, q quit
line list [--json]    the ports: label, settings, device, rx, idle, who is attached
line info PORT        one port in detail
line watch PORT       attach read-only
line attach PORT      attach (write if free); `line PORT` is the same
line log PORT         print the scrollback and exit (`line log cs8210 | grep panic`)
```
Over ssh the same words: `ssh -t console@dmc12` (menu), `ssh -t console@dmc12 watch cs8210`,
`ssh console@dmc12 list`. PORT is a name (P12) or a label (cs8210).
While attached: `^]` then `.` detach, `w` take write, `r` release write, `b` send break,
`?` help, `^]` a literal `^]`. After `^] .` from the menu you are back in the menu.

## Design
- **`consoled`** (daemon): opens every configured port exclusively (TIOCEXCL), raw, line
  settings from the config; one thread, one `poll(2)` loop; ports that are missing or go
  away (USB serial) are retried every 2 s while their clients stay attached.
- **Scrollback in RAM only**: the last N bytes per port (default 1 MiB), replayed on
  attach; a silent port uses nothing. Nothing goes to disk except attach/detach,
  takeover and device up/down lines in the journal.
- **One writer, others watch**: the first client asking for write gets it; others are
  read-only (their input is dropped with a hint) until they take it (`^] w`) or it is
  released or its holder leaves.
- **`line`** (client, `src/client.rs`): talks to the daemon over `/run/consoled/console.sock`
  (group `consoled`); `line ssh` is the sshd ForceCommand (words from `$SSH_ORIGINAL_COMMAND`,
  never a shell); `line metrics` prints Prometheus text for a node-exporter textfile producer.
- A client more than 4 MiB behind is dropped rather than slowing the others.
- Access: OpenSSH keys (the NixOS module's `ssh.authorizedKeys`, default root's); any key,
  any port. No network listener of its own: the daemon runs with `PrivateNetwork`.

Protocol (unix socket): frames `[type u8][len u32 BE][payload]`, see `src/proto.rs`.

## Build, test
Its own Cargo workspace (not `../crates/*`: fand's package source is that whole workspace).
`cargo test` runs the daemon against pseudo-terminals (`tests/daemon.rs`). Nix:
`nix/package.nix`, `nix/module.nix` (`services.consoled`); the ai_re root flake passes it in
as `aiReRust.nixosModules.consoled`.
