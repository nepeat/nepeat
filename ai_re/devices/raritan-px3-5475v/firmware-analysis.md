# PX3-5475V firmware teardown & backdoor assessment

Firmware: `pdu-px2-040313-52458.bin`, v04.03.13 build 52458. Rootfs in `extract/rootfs/`.
Analysis: static (binwalk + rootfs inspection + Ghidra on `bin/cli_login`).

## TL;DR — is there a manufacturing backdoor?
**No hidden unauthenticated remote backdoor was found.** SSH forces every session into the
restricted CLI; there are no baked-in SSH host keys or `authorized_keys`; the "debug console"
is auth- and config-gated behind a physical serial mux. There *are* two sloppy findings worth
knowing about (a shared root password hash and empty-password UID-0 accounts), but both are
gated by physical serial access, not remotely exploitable as-is.

## Accounts (`/etc/passwd`, no `/etc/shadow` — hashes inline)
```
root:$5$pIfTHJr9kINrL.fd$rIYvqSOFPGcIjgG3QaH/YV9TjsTzWmyJkGfP5FW/8M9:0:0:root:/root:/bin/sh
reset::0:0:root:/:/bin/reboot.sh
reboot::0:0:root:/:/bin/reboot.sh
luaservice:*:12:12:luaservice::
(bin/daemon/adm/... all locked with '*')
```

### Finding 1 — shared root password hash (MEDIUM, physical)
- `$5$` = **sha256crypt**, salt `pIfTHJr9kINrL.fd`. Baked into the image → **identical on every
  PX3 running this firmware.** Crack it once, it works on all of them.
- **Where it grants a shell:** only the physical **debug serial console**. `inittab` runs
  `getty /dev/console 115200 vt100` → plain **`/bin/login`**, which checks this hash and drops
  to `root`'s shell `/bin/sh` (UID 0). That `/dev/console` UART sits behind the GPIO serial mux
  (see below), so you need either the mux flipped to "debug" or direct access to the SoC debug UART pads.
- **Not remote:** see "Network login paths" — SSH never reaches `/bin/login`.
- Quick guesses tried and failed: raritan, admin, root, password, calvin, default, legrand,
  engineer, diagnostic, changeme, … → needs a real wordlist run (goal item 1).

### Finding 2 — empty-password UID-0 accounts `reset` / `reboot` (LOW)
- Both are UID 0, GID 0, **no password**. Classic red flag, but the login shell is
  `/bin/reboot.sh` (which does a clean shutdown + `reboot -f`), not an interactive shell. So on
  the console they let anyone reboot/factory-context without a shell. Sloppy, not a root shell.
- `reset`'s home/shell is only reachable via the same console getty → `/bin/login`.

## Network login paths (why the root hash isn't remote root)
From `etc/cfgd/default` (service listener config):
- **SSH 22 (enabled):** `//bin/dropbear -i -C "exec /bin/cli_main"`. Dropbear's `-C` **forces**
  that command for *every* client — so an authenticated SSH session is dropped straight into the
  restricted CLI `cli_main`, never `/bin/sh`, regardless of the passwd shell. No `-w`, but the
  forced command neutralises the root-shell path.
- **Telnet 23 (disabled):** `//bin/telnetd -i -l /bin/cli_login` (restricted login).
- **telnet_debug 24 (disabled):** `//sbin/telnetd -i` — a second telnet "debug" listener, off by default.
- **Modbus 502 (disabled).** HTTP 80 → 443 redirect, HTTPS 443, local http 8181.
- Dropbear **host keys are generated per-unit on first boot** (`80-dropbear` sysconf) and stored
  in `/config` — **no shared/baked SSH host key.** No `authorized_keys` shipped anywhere.

## The "serial debug console" mechanism (auth + config + hardware gated)
This is the intended vendor debug path, reconstructed from `bin/cli_login` (Ghidra) + scripts:
- Config key **`debug.console.enabled`** (boolean), schema `etc/cfgd/cfg_debug.cdl`:
  *"allow serialdebug on local console (RS232 / USB)"*. Setting config requires an authenticated admin.
- `bin/cli_login` (restricted CLI gate) authenticates via `pp::aaa::authenticate` (Raritan account
  DB, **not** `/etc/passwd`), then dispatches special command tokens found in its `.rodata`:
  `serialdebug`, `factorydefaults`, `#tcpproxy#`, `#bridge#`.
- `serialdebug` is gated on the `debug.console.enabled` flag; when allowed it calls
  **`switch_serial.sh debug`** → `gpioapp set pioD31=low` (armv7) — flipping a **GPIO hardware mux**
  that routes the physical serial connector to the SoC **debug UART** = `/dev/console` = the root
  `/bin/login` getty. That closes the loop with Finding 1.
- `factorydefaults` → prompts, then runs `/bin/reset_to_defaults`.
- Net: reaching a debug root shell the intended way needs **admin creds** (to set the flag) **+
  physical serial**. Not a secret; documented-style debug feature.

## cli_login (Ghidra notes)
- ARM32 uClibc, C++, stripped, ~19 KB; thin gate — real work in shared libs (`pp::aaa`, `cfg::Config`,
  `pp::runCommand`, `pp::sys::device::hasSerialMux`). Loaded at image base 0x10000; **Ghidra VA =
  file_offset + 0x10000** (auto-analysis missed the ARM literal-pool string xrefs — resolve strings
  by that mapping).
- The serialdebug handler's literal pool (≈0x13058–0x13068) points at `serialdebug` (0x149ac) and
  `debug.console.enabled` (0x149b8); a boolean flag byte (`ldrb r3,[r3,#0x289]`) gates entry.
- No hardcoded magic username/password constant observed in `cli_login`; auth flows through
  `pp::aaa::authenticate`. (A deeper audit of `cli_main`/libpp would be needed to fully rule out
  a lib-level backdoor, but nothing suspicious surfaced.)

## Other observations
- `luaservice` account exists (Lua scripting service, `98-luaserviced`) — attack surface if scripting
  is exposed, but no creds baked in.
- Crestron (`crestrond`), SNMP, Modbus, webcam, card reader, zigbee daemons present — large surface,
  mostly disabled by default.
- No telnet-on-by-default, no rsh/rlogin, no obvious `nc -e` / reverse-shell cron.
