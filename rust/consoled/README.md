# consoled: serial console server

Shares a box's serial ports over SSH the way console servers do (Opengear/Digi/Avocent
style, and the DMC-12 factory OS's ser2net-behind-ssh): one daemon owns the ports, users
attach through OpenSSH.

```
ssh -t console@dmc12 P12        # attach (write if free, else watch)
ssh -t console@dmc12 watch P12  # watch only (read-only)
ssh console@dmc12               # list ports
console P12 / console           # the same, on the box
```
Escape: `^]` then `.` quit, `w` take write, `r` release write, `b` send break, `?` help,
`^]` a literal `^]`.

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
- **`console`** (client): talks to the daemon over `/run/consoled/console.sock` (group
  `consoled`); `--ssh` is the sshd ForceCommand (port from `$SSH_ORIGINAL_COMMAND`);
  `--metrics` prints Prometheus text for a node-exporter textfile producer.
- A client more than 4 MiB behind is dropped rather than slowing the others.
- Access: OpenSSH keys (the NixOS module's `ssh.authorizedKeys`, default root's); any key,
  any port. No network listener of its own: the daemon runs with `PrivateNetwork`.

Protocol (unix socket): frames `[type u8][len u32 BE][payload]`, see `src/proto.rs`.

## Build, test
Its own Cargo workspace (not `../crates/*`: fand's package source is that whole workspace).
`cargo test` runs the daemon against pseudo-terminals (`tests/daemon.rs`). Nix:
`nix/package.nix`, `nix/module.nix` (`services.consoled`); the ai_re root flake passes it in
as `aiReRust.nixosModules.consoled`.
