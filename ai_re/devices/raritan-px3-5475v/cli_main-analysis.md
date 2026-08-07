# cli_main — restricted-CLI escape hunt

Target: `bin/cli_main` (the binary dropbear force-execs for every SSH session,
`dropbear -C "exec /bin/cli_main"`; runs as root). Goal: find a command injection / shell escape
that turns the restricted CLI into a root shell. Method: clish XML command defs → cli_main builtins
→ backend libs (libisys, libnetwork) in Ghidra.

## Verdict
**No CLI escape found.** cli_main is a **clish** CLI and is reasonably hardened: every command runs a
C++ `builtin=` handler (arguments passed as data), not a raw shell `ACTION`, and cli_main does **not**
import libc `system()`. The one real shell sink reachable from the CLI (network diagnostics) is
guarded by a strict input regex. Realistic root-shell path stays: physical serial + crack the shared
root hash (see firmware-analysis.md), not a cli_main 0-day.

## What was examined

### 1. Command execution model
- Commands are defined in `etc/clish/*.xml`. Actions are `<ACTION builtin="name">${param}</ACTION>` —
  the builtin is a registered C++ function in cli_main; params arrive as argv-style data, **not**
  interpolated into `/bin/sh`. cli_main imports `execl` and `pp::runCommand` (argv/execv, no shell),
  **not** `system`. (`system` appeared only as a substring false-positive.)

### 2. Network diagnostics — the one real shell sink (GUARDED)
- `ping` / `netstat` / `nslookup` / `traceroute` (`etc/clish/netdiag.xml`) → builtins →
  `isys::net::diag::{ping,traceRoute,...}` in **libisys.so**.
- libisys builds a **shell command line** and runs it via `pp::BackgroundCommand::open` → `/bin/sh -c`:
  ```
  exec 2>/dev/null; timeout <N> ping -c <M> ... <dest> 2>&1
  exec 2>/dev/null; timeout <N> traceroute[6] [-I] <dest> 2>&1
  ```
  (`isys::net::diag::traceRoute` @ libisys `0x49d58`; templates at file off `0x3e423`–`0x3e45d`;
  `/bin/sh` `-c` at `0x3bb04`.) The destination is **concatenated into the shell string** — textbook
  injection sink shape.
- **Why it's safe:** `dest` uses ptype `HOSTNAME_OR_ADDR` (`method="custom" pattern="hostname_or_addr"`)
  → `network::validateHostNameOrAddress` (libnetwork), which validates against anchored regexes that
  permit **only `[0-9A-Za-z]`, `-`, `.`**:
  ```
  ^[0-9A-Za-z](?:[0-9A-Za-z-]{0,61}[0-9A-Za-z])?$
  ^(?:[0-9A-Za-z](?:[0-9A-Za-z-]{0,61}[0-9A-Za-z])?\.)*[A-Za-z][0-9A-Za-z-]{0,61}[0-9A-Za-z]\.?$
  ```
  No space / `;` / `` ` `` / `$` / `|` / `&` / `(` / quote can survive → nothing to break out of the
  shell word. libisys additionally re-parses `dest` into an address type and rejects unparseable input.
  **Defense-in-depth nit for the vendor:** should use argv (`pp::runCommand`) not `sh -c`; today the
  regex is the only thing standing between this and RCE.

### 3. `escape_chars=""` commands (escaping disabled) — checked, not injectable in cli_main
- Commands that turn clish's default metachar-escaping **off**: `config encrypt` (hidden), `user modify`,
  `role modify`, `authentication ldap add/modify`, `inlet`/`outlet`/`outletgroup`/`pdu`/`pmc`/`src`,
  `network wireless`, `network services snmp`, `externalsensor`/`actuator`, asset-strip cmds.
- All are `builtin=` handlers that **store the value into config** (names, SSID/PSK, LDAP DN, encrypted
  value) — none is a shell `ACTION`. Escaping is off precisely because the value is data, not shell.
  No direct cli_main injection. *Residual second-order risk (not cli_main):* a backend daemon that later
  renders one of these stored strings into a shell/config unsafely (e.g. networkd writing wireless
  SSID/PSK into wpa_supplicant). Not audited — would be the next place to look for injection.

### 4. Latent path noticed: "Debug Telnet" (port 24 `telnet_debug`)
- cli_main has strings `Debug Telnet` / `TELNET_DEBUG`. In `etc/cfgd/default` the `telnet_debug`
  listener (port 24) runs `//sbin/telnetd -i` with **no `-l /bin/cli_login`** — so, unlike normal
  telnet (23, forced into cli_login), its sessions drop to the default **`/bin/login`** = a real root
  shell (gated by the shared `/etc/passwd` root hash). Disabled by default; **no standard user CLI
  command enables it** — appears to be a factory/debug-build feature tied to the debug console. If it
  can be flipped on, it's a network→`/bin/login` path that pairs with a cracked root hash.

## Binaries imported to Ghidra (project "ghidra", folder /raritan)
`cli_login`, `cli_main`, `libisys.so.1.0`, `libnetwork.so.1.0`. Note: VA = file_offset + 0x10000 for
the executables; auto-analysis misses ARM literal-pool string xrefs (resolve strings by offset).
