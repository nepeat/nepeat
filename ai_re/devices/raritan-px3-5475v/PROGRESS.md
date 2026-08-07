# Raritan PX3-5475V — RE Progress

Running journal (newest first). Detail lives in sibling files; this is the index + latest status.

- [firmware-analysis.md](firmware-analysis.md) — full firmware teardown & backdoor assessment.
- [cli_main-analysis.md](cli_main-analysis.md) — restricted-CLI escape/injection hunt (result: no escape).

## 2026-07-19 — poked cli_main for a CLI escape (result: nothing exploitable)
Traced the SSH-facing restricted CLI (`dropbear -C "exec /bin/cli_main"`, runs as root) looking for a
command-injection / shell escape. Detail in cli_main-analysis.md. Summary:
- clish CLI; all commands are C++ `builtin=` handlers, **not** raw shell `ACTION`s. cli_main does **not**
  import `system()`. Decent hardening.
- Network diag (`ping`/`traceroute`/`nslookup`) IS a `/bin/sh -c` sink in libisys
  (`exec 2>/dev/null; timeout N ping ... <dest> 2>&1`), but `<dest>` is filtered by anchored regexes
  allowing only `[A-Za-z0-9.-]` (libnetwork `validateHostNameOrAddress`) → **not injectable from CLI.**
- 20+ `escape_chars=""` commands disable escaping, but all just store config data via builtins — no
  shell. Residual (non-cli_main) second-order risk = backend daemons rendering stored strings unsafely.
- Noticed latent path: port-24 `telnet_debug` = `/sbin/telnetd -i` with no forced cli → default
  `/bin/login` (root); off by default, no clean enable found. Pairs with a cracked root hash if flipped.
- **Conclusion:** no cli_main 0-day shell. Root-shell path remains physical serial + crack root hash.


## Device identity
- **Make/model:** Raritan (Legrand) PX3-5475V intelligent rack PDU.
- **Platform:** "Xerus" / PX2-PX3 firmware family. Board id in image: `px2`.
- **SoC:** Microchip/Atmel **SAMA5** ARM (kernel names `Linux-6.6.59-sama5`, alt `-sam9`). ARMv7, little-endian, uClibc, C++ userland.
- **Firmware analysed:** `pdu-px2-040313-52458.bin` (17,196,223 bytes)
  - sha256 `594937b89a4c24e5c458b22486e71e4b4a42ee4080c2475940987edb57175e0f`
  - Version **04.03.13**, build **52458** (per `bin/get_firmware_version.sh`).
  - Built 2026-02-11.

## Firmware container layout (binwalk)
| Offset | Contents |
|---|---|
| 0x000 | Raritan "PP firmware" header (`PP firmware`), then "PP partition" header |
| 0x140 | **SquashFS 4.0 / xz** rootfs, 10,109,842 bytes, 1175 inodes → this is the target |
| 0x9A5568 | uImage `Linux-6.6.59-sam9`, ARM, load/entry 0x20008000 (uncompressed) |
| 0xC3F968 | uImage `Linux-6.6.59-sama5`, ARM, load/entry 0x20008000 (uncompressed) |
| 0xF70C3C | DTB v17 |
| ~0xFF64BA.. | LZ4 blobs (likely additional FIT/ramdisk assets) |
| 0x1065C22 | PEM certificate |

Rootfs extracted to `extract/rootfs/` (`unsquashfs`). Carved squashfs kept at `extract/rootfs.squashfs`.

## Interfaces
- **Serial:** device has a **GPIO serial mux** (`pp::sys::device::hasSerialMux`). Physical RS232/USB serial line is switched between:
  - *normal* CLI line (getty `-l /bin/cli_serial_login` on `/dev/ttyGS0`, 115200 8N1 vt100), and
  - *debug* console (`getty /dev/console 115200 vt100` → **plain `/bin/login`**).
  - Switch via `switch_serial.sh {default|debug}` → `gpioapp set pioD31|pioB16 = high|low` (armv7 → pioD31).
  - **Baud unconfirmed on real HW** — assume 115200 8N1, verify before trusting.
- **Network:** SSH 22 (on), HTTP 80 (on, redirects→443), HTTPS 443 (on). Telnet 23 + "telnet_debug" 24 + Modbus 502 all **off** by default.

## Latest status (2026-07-19)
Firmware pulled apart and audited for backdoors (see firmware-analysis.md). No secret unauthenticated remote backdoor found. Two real risks:
1. **Shared root password hash** baked into `/etc/passwd` (same across every unit on this FW). Reachable as a **direct root shell only via the physical debug serial console** (`/bin/login`). Network logins (SSH) are forced into the restricted CLI, so this is not remote root.
2. Sloppy: two **UID-0 accounts with empty passwords** (`reset`, `reboot`) — but their shell is `/bin/reboot.sh`, so they only reboot.

**Goal set:** get a root shell on the PDU. See "Goal" below.

## Goal — get a shell
Priority order (device is owned; physical access assumed):
1. **Crack the shared root hash** `$5$pIfTHJr9kINrL.fd$…` (sha256crypt). If it falls, physical debug-serial → `/bin/login` root → `/bin/sh`. Tool: `john`/`hashcat`. Quick vendor-default guesses already failed; needs a real wordlist run.
2. **Intended debug path (needs admin creds we own):** set config `debug.console.enabled=true`, connect serial, authenticate at the CLI, run `serialdebug` → flips GPIO mux to the debug console → root `/bin/login`. Confirms mechanism even without cracking the hash.
3. **Boot/U-Boot interception** on the debug UART: interrupt boot, set `bootargs init=/bin/sh` / single-user. Most reliable for owned HW; needs the debug-console UART (mux or direct SoC UART pads).
4. **CLI escape** from `cli_main`/`cli_login` (`factorydefaults`, `#tcpproxy#`, `#bridge#` handlers) — lower priority; the CLI runs as root so any command-injection there = root.

### Next actions
- [ ] Locate the physical serial header on the PDU board; identify normal vs debug UART pads; confirm 3.3V logic before probing.
- [ ] `nix develop` (john now in shell). Run wordlist attack on the root hash (`extract/root.hash`).
- [ ] If we have web-admin creds, try path 2 end-to-end on the real unit.
