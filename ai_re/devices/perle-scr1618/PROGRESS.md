# Perle IOLAN SCR1618 — firmware compatibility

Goal: determine which of two SCR-series firmware files is compatible with the **Perle IOLAN SCR1618** console server.

## Verdict (2026-07-19)
**Compatible = `scr-4.5.G4.img`.  Do NOT flash `scr-v2-8.2.G1.emg` — it's for a different product (IOLAN SCR *v2*).**

The higher version number (8.2 > 4.5) is a **trap**: 8.2.G1 is not a newer firmware for the SCR1618, it's the
firmware for a *separate hardware line* (the "IOLAN SCR v2"). Flashing wrong-platform firmware risks bricking.

## Evidence (two independent confirmations)
1. **Perle's official IOLAN SCR download page** (`perle.com/downloads/iolan-scr.shtml`): two distinct firmware
   sections —
   - **"Firmware IOLAN SCR1618" → Version `4.5.G4`**  ← our device
   - **"Firmware IOLAN SCR (v2)" → Version `8.2.G1`**  ← a different product
2. **The `.img`'s own embedded FIT description** literally reads:
   `IOLAN SCR Series Console Server/4.5.G4/0xc/0x0/0x010000000e/` — self-identifies as SCR-series 4.5.G4. (The
   trailing `0xc/0x0/0x010000000e` looks like version + a hardware-compat mask; not fully decoded.)

## Reverse engineering (2026-07-19) — `scr-4.5.G4.img` internals
Decompiled the FIT with `dtc -I dtb -O dts`:
- **kernel@1**: "Router Linux kernel", **arch=arm64**, os=linux, compression=none, load/entry `0x80000`, md5.
- **fdt@5**: "Router Device Tree blob - 1 port", flat_dt, arm64.
- **ramdisk@1**: "Squashfs Image" (type=ramdisk) → **the root filesystem** (squashfs).
- default config: "Boot Linux kernel, FDT for **Amazon**, rootfs in ramdisk".

**Platform (CORRECTED — not Annapurna/Alpine):** the DTB says `model="Perle Amazon Board"`,
`compatible="marvell,armada3700"`, dual Cortex-A53 → SoC is a **Marvell Armada 3700 (Armada-3720)**; "Amazon" is
just Perle's board codename. Confirmed by the DTB + an embedded Armada-3720 U-Boot at `lib/firmware/router-uboot.bin`.

## Rootfs analysis — DONE (2026-07-19) — see `rootfs-analysis.md`
Subagent extracted (all three FIT md5 hashes verify) and analyzed the squashfs rootfs (SquashFS 4.0/xz, built
2021-08-11). Highlights:
- **OS**: **VyOS 1.2.0-rolling** (Debian 8 "jessie" base), **kernel 4.14.235 aarch64**, glibc 2.33. Perle
  "device_server" layer v4.5.G4. Gobi WWAN (LTE OOB) modules present.
- **Proprietary stack**: Perle **CLP** CLI over VyOS (`clpd` + 39 `/usr/lib/clp/*.so` plugins; `initdb`/`reqhandlerd`
  → `/product`). Console-server core = **`iol_*` suite** (`iol_telnetd/sshd/portmgr/sredird/smodbusd/vc`, `portctl`)
  = reverse serial, RFC2217, Modbus, vmodem. REST API `perle-api-server` (Pistache/OpenAPI). Web UI = aarch64 CGI on
  Perle Apache. MIBs are a rebranded **EtherWAN** OEM tree (hardware is EtherWAN-shared).
- **Firmware update/verify**: upload writes plaintext `fit.itb` straight to eMMC `/media/image/{current,backup}` +
  reboot. **No signature check on upload; boot verifies FIT md5 only; no verified-boot / dm-verity / IMA** → an
  **unsigned modified image will flash and boot**. Big deal for custom firmware.

## Security findings (from `rootfs-analysis.md`)
- **Shared static HTTPS key on every unit**: `etc/default_ssl_rsa_cert.pem` (cert + 2048-bit RSA key,
  `O=Perle Systems, CN=PerleRouter`) → impersonation/MITM until re-provisioned.
- **Factory default `vyos` / `vyos`** (confirmed by recomputing the `$6$` hash in `config.boot.default`). Two extra
  Perle `/etc/shadow` seeds (root, vyos) — not stock, uncracked, superseded at boot. SSH host keys generated at first
  boot (not shared — good); root SSH off by default.
- Stock Apache **sample CGIs** (`printenv`, `test-cgi`), **world-writable `/product`**, a config-driven `bypass_login`
  path (needs further RE), EOL software stack.
- **`.emg` decryption NOT recoverable from this image**: zero `.emg`/GPG/passphrase material in the rootfs (this IS
  the already-decrypted payload). The `.emg` symmetric passphrase lives in Perle's build/release tooling or another
  partition, not here.

## `bypass_login` — DONE (2026-07-19) — VERDICT: legit provisioning/recovery, NOT a backdoor (`bypass-login-analysis.md`)
- Only `usr/www/root/auth.cgi` uses the web flag. `config_get_bypass_login()` returns dataset var
  `DS.System.express_setup_mode`, computed as:
  `express_setup_mode = (HTTP.Host == "192.168.0.1") ? !file_exists("/product/nvram/startup-config") : 0`.
  → bypass active **only** when the unit has **no saved config** AND is reached on the **factory-default IP 192.168.0.1**.
- When active it skips `pam_authenticate`, but does **NOT** drop a root shell — it swaps the login page for the
  **fast-setup wizard** (create first admin) or the **password-recovery** flow (gated by on-flash flag file
  `/product/password-recovery`). Scoped to provisioning/recovery UX.
- Reaching that state on a deployed unit needs a **virgin box or a physical factory reset** (RESET button polled by
  `sbin/prodinit` at boot → wipes config). **Not default-enabled, not remotely reachable**, no hardcoded cred/magic
  value. Abuse = same trust boundary as any factory reset (physical). (libglobals' `Bypass-Login` strings are an
  unrelated operator-configured console-line CLI keyword.)

## Shell-access vectors — DONE (2026-07-19) — see `shell-access-vectors.md`
Ranked easiest→hardest (root amplifier across ALL of them: `vyos`, `www-data`, `radius_priv_user` are in
`%sudo ALL=NOPASSWD:ALL` per `/etc/sudoers.d/vyatta`, so ANY code-exec as those = instant root):
1. **U-Boot serial → `init=/bin/sh`** (physical, no auth, ~100%). `router-uboot.bin`: `bootdelay=2`, "Hit any key to
   stop autoboot", **no U-Boot console password**. Interrupt @115200 8N1, set `bootargs ... init=/bin/sh`, boot → root.
   (`bootcmdnfs`/`nfsargs` also allow netbooting an attacker rootfs.) **Top pick for physical access.**
2. **Web UI → `clpd` command injection → root** (network, needs web-admin login `admin`/default `superuser`). CGIs run
   as `www-data` (sudo NOPASSWD). CGI layer is hardened (auth.cgi = real PAM, manage.so = sanitizing `system_safe` +
   `execvp`, ping/traceroute CLP-mediated), so the sink is downstream in `clpd`'s `system("sudo …%s…")` builders.
   Medium confidence, needs more RE.
3. **`vyos`/`vyos` via SSH/console → `vbash` → `sudo su -`** (default cred + confirmed escalation).
4. **`operator` + `sudo tcpdump -z` → root** (needs operator foothold).
5. **REST `perle-api-server`** firmware/software-upload endpoints (needs API auth). Combined with unsigned firmware =
   remote persistent root via a crafted image.
6. CLP CLI escape / offline `$6$` hash cracking (harder).
- **Default-on & network-reachable**: web UI + REST + SSH (vendor default, config-gated). **Physical**: U-Boot + serial CLP.
- `root` is blocked from the serial console by `pam_securetty` (`ttyMV0` not in `/etc/securetty`) — hence go via U-Boot.
- **Flagged**: a `Config.DebugEnabled` / `Config.DebugPassword` debug-auth path in `auth.cgi` — possible factory
  backdoor; overlaps the running bypass_login investigation.
- Note: subagent's Ghidra MCP calls weren't invocable from its client → findings are radare2/rabin2-based; a
  main-loop Ghidra pass can go deeper on `clpd`/`auth.cgi` if wanted.

## The two files
| File | Size | Format | Target | For SCR1618? |
|------|------|--------|--------|--------------|
| `scr-4.5.G4.img` | 231,348,508 B (~231 MB) | **U-Boot FIT image** (plaintext DTB; ARM; "Router Linux kernel") | IOLAN SCR1618 | ✅ **YES** |
| `scr-v2-8.2.G1.emg` | 545,959,412 B (~546 MB) | **GPG symmetric AES256-CFB** encrypted (S2K iterated+salted, MDC) — Perle's modern encrypted format | IOLAN SCR **v2** | ❌ no |

Notes:
- The format difference is itself a signal: Perle moved from plaintext `.img` (4.5 line) to encrypted `.emg` (8.x/v2
  line). The `.emg` contents are sealed (passphrase-protected), so it can't be inspected without Perle's key.
- The `.img` is a standard FIT; `mkimage -l` would list it on Linux (ubootTools is Linux-only on nix-darwin). Its
  metadata was read directly from the header with `strings`.

## Before flashing (recommended checks)
- Confirm the device label/CLI reports the **SCR1618** model (not "SCR v2") and note its **current running firmware**.
- Perle units can hold multiple firmware versions and roll back — keep the current image as a backup first.
- Verify the `.img` against Perle's published checksum/signature if available before flashing.

## Files
- `scr-4.5.G4.img` — compatible firmware (kept locally; large — see repo note re: git-lfs).
- `scr-v2-8.2.G1.emg` — wrong-platform (SCR v2), encrypted; kept for reference only.
