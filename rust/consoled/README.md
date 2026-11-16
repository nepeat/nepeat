# consoled: serial console server

Shares a box's serial ports over SSH the way console servers do (Opengear/Digi/Avocent
style, and the DMC-12 factory OS's ser2net-behind-ssh): one daemon owns the ports, users
attach through OpenSSH.

```
line                    menu: ↑↓/jk pick, ⏎ attach, w watch, i info, digits jump, q quit
line list [--json]      the ports: label, settings, device, rx, idle, who is attached
line info PORT          one port in detail
line watch PORT         attach read-only
line attach PORT        attach (write if free); `line PORT` is the same
line log PORT [-f]      print the scrollback and exit (`line log cs8210 | grep panic`);
                        --follow / -f: then keep printing what comes
line send PORT [--take] [--no-newline] TEXT|-
                        type TEXT and Enter (- reads stdin)
line expect PORT PATTERN [--timeout S]
                        wait for new output matching PATTERN (default 10 s)
line run PORT [--take] [--prompt PATTERN] [--timeout S] CMD
                        type CMD, print its output up to the next prompt
line --prefix C-a ...   the attach key prefix (default C-b; also $CONSOLED_PREFIX)
```
Over ssh the same words: `ssh -t console@dmc12` (menu), `ssh -t console@dmc12 watch cs8210`,
`ssh console@dmc12 list`, `ssh console@dmc12 run cs8210 uname -a`. PORT is a name (P12) or
a label (cs8210). `line help` prints all of it.

## Keys while attached
Press the prefix (**C-b**, like tmux; `--prefix C-a` or `CONSOLED_PREFIX=C-a` changes it),
then:

| key | does |
|---|---|
| `d` | detach (back to the menu if you came from it) |
| `?` | list the keys |
| `[` | scrollback: page through the port's RAM scrollback (↑↓ j k, PgUp PgDn, g G); `q` or Esc leaves, output that came meanwhile is shown then |
| `s` `w` | port chooser (the menu); ⏎ attach / `w` watch another port, `q` back to this one |
| `n` `p` | next / previous port in the menu's order (watch stays watch, attach asks for write again) |
| `t` / `r` | take / release write |
| `B` | send a serial break |
| `:` | command: `take`, `release`, `break`, `baud RATE`, `next`, `prev`, `detach`, `help` |
| prefix again | the prefix key itself, to the port |

**^]** stays a prefix with the old meanings too: `^] .` detach, `^] w` take write, `^] b`
break, `^] ^]` a literal ^]. `baud RATE` changes the line for everyone (writer only; it
holds until consoled restarts) and everyone attached is told.

Nested tmux: an outer tmux catches C-b. Press it twice (C-b C-b reaches line as one C-b),
use ^], or pick another prefix: `line --prefix C-a`, `ssh -t console@dmc12 attach cs8210
--prefix C-a` (after the words: ssh would take a leading `--prefix` as its own option), or
`ssh -o SetEnv=CONSOLED_PREFIX=C-a -t console@dmc12`.

## For agents and scripts
`send`, `expect` and `run` attach without a scrollback replay, do one thing and leave.
Exit codes: 0 ok, 1 timeout, 2 refused (someone else has write), 3 other errors.

- `send` and `run` need write: without `--take` they are refused while someone else holds
  it (the message says who); `--take` takes it over and the old writer is told. Write
  acquired for the command is released when it ends.
- `send`: TEXT and Enter (\r); each newline in TEXT or stdin becomes \r. `--no-newline`:
  the bytes as they are. At most 32 KiB.
- `expect`: waits for *new* output; prints what it saw (cleaned, up to the matching line).
- `run`: types CMD and Enter, waits until the prompt shows again, prints the output in
  between: escape sequences (colours, titles, bracketed paste) stripped, carriage returns
  applied, the echoed command line dropped. The prompt is the unfinished last line, and it
  counts once nothing more has come for 200 ms; default `--prompt '[#$>] ?$'` (shells,
  U-Boot `=> `, switch CLIs).
- Options of `send` and `run` go before the text; several words are joined with spaces.

Patterns (`expect`, `--prompt`; `src/pattern.rs`, no regex crate) match one cleaned line:
literal text, `.`, `[a-z]` / `[^...]`, `\d \s \w`, `* + ?` after an atom, `^` at the start,
`$` at the end of a line *or of the output so far* (a prompt has no newline yet); `\` makes
the next character literal. No groups or alternation.

```
$ ssh console@dmc12 run cs8210 --prompt "'# \$'" "'uname -a'"
Linux cs8210 6.12.0 #1-NixOS SMP aarch64 GNU/Linux
$ ssh console@dmc12 'run cs8210 --prompt "# $" "uname -a"'          # same, quoted once
$ ssh console@dmc12 run cs8210 uname -a                               # default prompt
$ ssh console@dmc12 send cs8210 --take reboot
$ ssh console@dmc12 expect cs8210 'login: $' --timeout 300 && echo booted
$ echo 'setenv bootdelay 5' | ssh console@dmc12 send as7760 -
$ ssh console@dmc12 log cs8210 --follow | grep --line-buffered -i error
```
ssh joins its arguments into one string; `line ssh` splits `$SSH_ORIGINAL_COMMAND` like a
shell would (`'...'`, `"..."`, backslash) without expanding or running anything, so quote
anything with spaces for the remote side too.

## Design
- **`consoled`** (daemon): opens every configured port exclusively (TIOCEXCL), raw, line
  settings from the config; one thread, one `poll(2)` loop; device open, close, break and
  runtime baud changes run on a worker thread (a CP2108 open takes about 1 s); ports that
  are missing or go away (USB serial) are retried every 2 s while their clients stay attached.
- **Scrollback in RAM only**: the last N bytes per port (default 1 MiB), replayed on
  attach and sent on request (the `[` view); a silent port uses nothing. Nothing goes to
  disk except attach/detach, takeover, baud and device up/down lines in the journal.
- **One writer, others watch**: the first client asking for write gets it; others are
  read-only (their input is dropped with a hint) until they take it (prefix `t`) or it is
  released or its holder leaves.
- **`line`** (client): talks to the daemon over `/run/consoled/console.sock` (group
  `consoled`); `line ssh` is the sshd ForceCommand (only its own command words, from
  `$SSH_ORIGINAL_COMMAND`, never a shell; no `ssh`, `metrics` or `--socket` there);
  `line metrics` prints Prometheus text for a node-exporter textfile producer. Code:
  `src/session.rs` (attach, keys, menu, scrollback), `src/agent.rs` (send/expect/run),
  `src/cli.rs` (the words), `src/text.rs` (cleaning output), `src/pattern.rs`.
- A client more than 4 MiB behind is dropped rather than slowing the others.
- Access: OpenSSH keys (the NixOS module's `ssh.authorizedKeys`, default root's); any key,
  any port. No network listener of its own: the daemon runs with `PrivateNetwork`.

Protocol (unix socket): frames `[type u8][len u32 BE][payload]`, see `src/proto.rs`. New
fields have serde defaults and old clients ignore new frame types, so either side can be
older.

## Build, test
Its own Cargo workspace (not `../crates/*`: fand's package source is that whole workspace).
`cargo test` runs the daemon and `line` against pseudo-terminals (`tests/daemon.rs`: the
writer rules, baud, scrollback, send/expect/run, the attach keys on a pty). Nix:
`nix/package.nix`, `nix/module.nix` (`services.consoled`); the ai_re root flake passes it in
as `aiReRust.nixosModules.consoled`.
