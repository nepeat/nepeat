# Perle IOLAN SCR1618 — Root/Shell Access Vectors

Target: Perle IOLAN SCR1618 console server. Marvell Armada 3700 (arm64), OS = VyOS 1.2.0-rolling / Debian jessie base, kernel 4.14.235, proprietary Perle **CLP** CLI (`clpd`) + `iol_*` console-server daemons + Apache CGI web UI (`/usr/www/root`) + `perle-api-server` REST API.

Rootfs analysed: `/Users/nep/nocloud/git/nepeat/ai_re/devices/perle-scr1618/extracted/rootfs`
Also: `extracted/kernel.bin`, `extracted/fdt.dts`, bootloader `rootfs/lib/firmware/router-uboot.bin`.

Read-only static analysis. "Confirmed" = proven from files in the image. "Hypothesis" = strongly indicated but needs live device / deeper disassembly to prove. The `bypass_login` path is owned by a separate agent and only noted briefly.

**Tooling note:** `manage.so` and `auth.cgi` were imported into the shared Ghidra project (folder `/perle-shell`, auto-analyzed). The Ghidra MCP *analysis* tools (decompile/list-functions) were not invocable from this client session (only the debugger/management tools were exposed), so the binary-level findings below come from **radare2 / rabin2 / objdump / strings** run locally on the same ELFs. Findings are still evidence-based (symbol tables, imports, `.rodata` string/xref tables); function-body decompilation of `clpd`'s `system()` builders remains the one open item that would need working decompiler access.

---

## TL;DR — Ranked shell vectors (easiest → hardest)

| # | Vector | Access | Auth needed | Default-enabled? | Confidence |
|---|--------|--------|-------------|------------------|-----------|
| 1 | **U-Boot autoboot interrupt → `init=/bin/sh`** | Physical (serial) | None | Yes (bootloader) | Confirmed |
| 2 | **Web UI → `clpd` cmd-injection → `www-data`/clpd `sudo` (NOPASSWD:ALL) → root** | Network (HTTP/HTTPS) | Web admin login (`admin`/default) | Web mgmt on by vendor default | Hypothesis, medium — CGI layer is hardened (sanitizing `system_safe`, CLP-mediated); real sink is `clpd` |
| 3 | **`vyos` default creds via SSH/console → `vbash` → `sudo su -`** | Network/physical | `vyos`/`vyos` (factory) | SSH: config-dependent | Confirmed creds path; SSH state hypothesis |
| 4 | **`operator`/sudoers `tcpdump -z` (PCAPTURE) escalation → root** | Needs any operator shell first | operator login | Yes (sudoers.d/vyatta) | Confirmed sudoers, needs foothold |
| 5 | **`perle-api-server` REST — software/firmware upload or injection** | Network (REST) | API token/Basic | API config-dependent | Hypothesis |
| 6 | **Serial CLP CLI escape to shell** | Physical (serial) | CLI admin | Yes (serial = CLP) | Hypothesis (no escape found yet) |
| 7 | **Crack production `root`/`vyos` `$6$` shadow hashes** | Offline | — | — | Confirmed hashes, unknown plaintext |
| 8 | `bypass_login` path | — | — | — | Deferred to other agent |

**Single best bet:** with physical access, **#1 (U-Boot serial interrupt → `init=/bin/sh`)** — unauthenticated, no password on the bootloader, ~100% reliable. For network-only, **#2 (web UI → `www-data` → `sudo`)** is the strongest, amplified by the `%sudo NOPASSWD:ALL` misconfig.

---

## 1. Credentials (Confirmed)

### `/etc/passwd` — login-capable accounts
```
root:x:0:0:root:/root:/bin/bash
vyos:x:1000:100:VyOS Administrator,,,:/home/vyos:/bin/vbash
radius_user:x:1001:100:...:/home/radius_user:/sbin/radius_shell
radius_priv_user:x:1002:108:...:/home/radius_priv_user:/sbin/radius_shell
```
All other accounts use `/usr/sbin/nologin` or `/bin/false`.

### `/etc/shadow` — production seeds (unknown plaintext)
```
root:$6$u/1V/kCY$rhV91K7Gg9X6YCsUalcOxgDausuJDRy3gbYh9kW88Pqq...:17702:...
vyos:$6$l5f.uVN6$J8DlFFtBgaEFXZoJSL7j7CzJmKx3Sl6wF17UAopN6Vz...:17702:...
radius_user:!  radius_priv_user:!   (locked)
```
SHA-512 crypt. These are the **shipped** passwords (Perle set them); plaintext not recoverable from the image — offline cracking only (vector #7).

### `/opt/vyatta/etc/config.boot.default` — factory-reset config
```
user vyos { authentication {
    encrypted-password $6$QxPS.uk6mfo$9QBSo8u1FkH16gMyAVhus6fU3LOzvLR9Z9...
} level admin }
console { device ttyMV0 { speed 115200 } }
```
This hash is the **well-known VyOS default `vyos` / `vyos`**. After a factory reset the `vyos` admin password reverts to `vyos`. (Note the shipped `/etc/shadow` `vyos` hash differs from this default — Perle changed it in the golden image, but factory-reset restores the `vyos`/`vyos` default.)

### Group membership — the privilege amplifier (Confirmed, `/etc/group`)
```
sudo:x:27:radius_priv_user,www-data,vyos
adm:x:4:radius_user,radius_priv_user,www-data,vyos
vyattacfg:x:108:radius_priv_user,www-data,vyos
operator:x:37:radius_user
```
`vyos`, **`www-data`**, and `radius_priv_user` are all in the `sudo` group. Combined with sudoers `%sudo ALL=NOPASSWD: ALL` (below), **code execution as any of these three = instant root with no password.**

### `/etc/securetty` (Confirmed)
Lists `console`, `ttyS0`, `ttyS1`, `ttyUSB0-2`, `tty1-63` — but **not `ttyMV0`** (the Armada UART device node). `login`'s `pam_securetty.so` therefore blocks direct `root` login on the Perle serial console. `root` password login is only possible on a tty in the list. This pushes attackers toward `vyos` + `sudo` rather than direct root login.

**Who can log in where:** `root` (console-restricted, shipped password unknown); `vyos` (admin, `vbash`, sudo-group — the golden path if you have its password or can factory-reset to `vyos`/`vyos`); `radius_*` (only if RADIUS server configured, `radius_priv_user` is sudo-group).

---

## 2. Serial console (Confirmed hardware; login flow = Hypothesis)

- `extracted/fdt.dts` → `chosen { stdout-path = "serial0:115200n8"; }`. Kernel `console=`/`init=` are **not** in the FDT — they come from U-Boot (see §3). Serial is 115200 8N1.
- No `/etc/inittab` (systemd system). `serial-getty@.service` exists (`ExecStart=-/sbin/agetty --keep-baud 115200,38400,9600 %I`) but **no `serial-getty@ttyMV0` symlink is enabled** and `getty.target.wants` is empty of serial gettys.
- Default-enabled units (`/etc/systemd/system/multi-user.target.wants`): `perle-cli` (`/usr/bin/clpd`), `perle-init` (`/usr/bin/initdb`, `TTYPath=/dev/ttyMV0`), `perle-reqhandlerd`, `vyatta-router`, `cron`, `atd`, `rsyslogd0`, `ssh-session-cleanup`.
- `clpd` owns the console line: strings `cfgdb_set_line_console`, `get_console_user`, `is_console_exec_active`, `set_console_to_load`.

**Conclusion:** the serial port presents the **Perle CLP CLI (`clpd`)**, not a raw Linux `login:`. So the serial port by itself is a CLI-credential surface (vector #6), *unless* you interrupt U-Boot first (vector #1) — which uses the same physical serial line and needs no credentials. `console_portctl@.service` (`/usr/bin/portctl %i nopasswd 4`) is the pass-through handler for the **managed** device ports, not the box's admin shell.

---

## 3. U-Boot — the easiest root path (Confirmed)

`rootfs/lib/firmware/router-uboot.bin` (`strings`):
```
bootdelay=2
Hit any key to stop autoboot: %2d
bootcmd=run bootcmdmmc
baudrate=115200
preboot=
bootcmdmmc=run isolate_switch_ports;...;setenv bootargs $console root=/dev/ram0 \
   rootfstype=squashfs rw rootwait net.ifnames=0 biosdevname=0 init=/sbin/prodinit; \
   run bootmmc_current; run bootmmc_backup; run bootmmc_error
bootcmdnfs=run isolate_switch_ports_nfs;run load_product_uenv;run uenvcmd
nfsargs=setenv bootargs console=ttyMV0,115200 earlycon=ar3700_uart,0xd0012000 \
   root=/dev/nfs nfsroot=${nfsserverip}:${rootfspath}... rw ... init=/sbin/prodinit
```
- **`bootdelay=2`** and a plain **`Hit any key to stop autoboot`** prompt → autoboot is interruptible over serial.
- **No console-password protection.** No `CONFIG_AUTOBOOT_KEYED`, `bootstopkey`, `bootstopkeysha256`, `menupassword`, or any `password`/`Enter password` string in the image. The only "authenticate" string is `ERROR: Failed to authenticate BL32`, which is ARM Trusted Firmware (secure world / TrustZone) verifying BL32 — **it does not gate the U-Boot console**.
- Rootfs is a read-only squashfs in RAM (`root=/dev/ram0 rootfstype=squashfs`), normal init is `/sbin/prodinit`.

**Exploit:** connect serial (115200 8N1), power-cycle, press a key within 2 s to reach the `=>` prompt, then:
```
setenv bootargs console=ttyMV0,115200 root=/dev/ram0 rootfstype=squashfs rw init=/bin/sh
run bootmmc_current      # or reconstruct the boot with the edited bootargs, or: boot
```
Root shell with no authentication. Alternatively use the built-in `bootcmdnfs`/`nfsargs` to **netboot an attacker-controlled rootfs** (TFTP/NFS recovery). This is the classic unsigned-boot break and the most reliable vector against a device you can physically touch. **Confirmed / default-enabled / physical / no auth / ~100% reliable.**

---

## 4. SSH / Telnet (Confirmed binaries; default state = Hypothesis)

- Stock OpenSSH `sshd` present: `/usr/sbin/sshd`; units `ssh.service`, `ssh.socket`, `ssh@.service`. **Not** in the default `multi-user.target.wants` — Perle enables it from the CLP/VyOS config, not a static systemd symlink. Vendor factory default on IOLAN SCR is typically management-on, but this cannot be *confirmed* from the config files in the image (the live Perle default config lives in `clpd`'s config DB / `initdb`, not a readable file). **Hypothesis: likely reachable.**
- `iol_sshd` and `iol_telnetd` (`/usr/bin/`) are Perle's own SSH/Telnet front-ends for **serial-port pass-through / CLP access**, launched on demand by the Perle daemons (no systemd unit; controlled by config). Telnet default state not determinable from files.
- If SSH is up and you authenticate as **`vyos`** (default `vyos`/`vyos` after reset, or shipped password if cracked), you land in `/bin/vbash` (VyOS shell) → §5 → `sudo su -` → root. Remote `root` SSH login depends on `sshd_config PermitRootLogin` (the file `/etc/ssh/sshd_config` had no non-comment overrides in this image; VyOS generates the effective config at runtime).

---

## 5. CLI escape (VyOS `vbash` = Confirmed root escape; CLP escape = Hypothesis)

### VyOS `vbash` → root (Confirmed)
`vyos`'s shell is `/bin/vbash`. Once in a VyOS operational shell, `vyos` is in the `sudo` group and:
```
/etc/sudoers.d/vyatta:  %sudo ALL=NOPASSWD: ALL
/etc/sudoers        :  root ALL=(ALL) ALL
```
So any `vbash` session for `vyos` (or `www-data`/`radius_priv_user`) can run:
```
sudo su -            # or: sudo bash, sudo vbash, run add system image, etc.
```
→ **root, no password.** This is the standard VyOS operational→root escalation, left fully intact. VyOS also historically exposes `add system image`, config `script`/`system` nodes, and `run` sub-shells — all reachable once you have any admin `vbash`.

### `radius_shell` (`/sbin/radius_shell`, Confirmed)
Setuid-capability wrapper (`cap_setuid`, `setresuid`, `setfsuid`) that execs `/bin/<name>` or `/opt/vyatta/bin/<name>` and drops into `vbash`/`restricted-shell` (strings: `-vbash`, `-restricted-shell`, `Exec of shell %s failed`). Used for RADIUS-authenticated logins; `radius_priv_user` is in `sudo` → same `sudo su` escalation. Requires a configured RADIUS server, so situational.

### Perle CLP CLI (`clpd`) → shell (Hypothesis — no confirmed escape)
`/usr/bin/clpd` (2.8 MB) drives the box via `system()`/`sudo`. Notable strings:
```
clp_shell_launch_system_cmd
"For starting an exec (shell)" / "Enable EXEC CLI session"
"Run exec commands while in config mode"
%s() @ %s:%d (pid=%d): system(%s)
sudo iw dev %s set 4addr on
sudo ifconfig %s down; sudo ifconfig %s up
sudo /opt/vyatta/sbin/%s-init %s %s
sudo perl /opt/vyatta/sbin/wireless-hostapd.pl %s
```
- **No `/bin/sh` / `/bin/bash` / `vbash` menu-escape string** was found in `clpd`, so there is no obvious built-in "shell" command from the CLP CLI (unlike vbash). The CLP "exec" is a CLI session, not a Unix shell.
- However `clpd` builds many `system("sudo …%s…")` command lines from parameters (interface names, wireless args). If any of those `%s` fields are attacker-controllable from the CLI **and not sanitized**, classic argument/shell-metacharacter injection (`; …`, `$( )`) yields command execution — and because these run `sudo`, they run as **root**. This needs disassembly of the specific command builders to confirm which fields reach `system()` unsanitized. **Hypothesis, high value.**

---

## 6. Web UI / REST (surface Confirmed; injection/bypass = Hypothesis)

### Apache + Perle CGI web UI
- Web server: `httpd.service` → `/usr/apache/bin/httpd -DFOREGROUND`, **`DocumentRoot "/usr/www/root"`**, **`User www-data` / `Group www-data`** (`/usr/apache/conf/httpd.conf`). `www-data` is in the `sudo` group → **any RCE in a CGI = root via `sudo`.**
- Perle CGIs (`/usr/www/root/`, all ARM64 ELF): **`auth.cgi`**, **`manage.cgi`**, shared logic **`manage.so`**.
  - **`auth.cgi` — real PAM auth, no hard-coded backdoor string (Confirmed via rabin2).** Imports `pam_authenticate`, `pam_acct_mgmt`, `pam_set_item`, `htmlui_glob_send_pam_acct_start/stop`, `pamutil_destroy_auth_env`; session via `clpd_api_open_session`, `htmlui_glob_*_session`, `get_session_cookie`, `SimpleTokens`, `two_factor_login`, `is_password_expired`. So the web front door is a genuine PAM login, not a `strcmp("admin","admin")` shortcut. The web/CLP admin account is **`admin`** (`.rodata`: `DS.Auth.USR_ADMIN`, `admin`, `SERVER_ADMIN`) — this is the Perle CLP/web account stored in `clpd`'s config DB, **separate** from the Linux `vyos`/`root` accounts in `/etc/shadow` (Perle IOLAN factory web/CLI default is typically `admin` / `superuser`).
  - **Debug-mode surface in `auth.cgi` (Hypothesis / note):** `.rodata` contains `Config.DebugEnabled`, `Config.DebugPassword`, `Query.debug`, `Query.debug_pause`, `DS.Generic.factory.ip`. There is a **debug-enabled + debug-password** code path — a candidate factory/debug auth path worth live testing, and it may be the same mechanism the separate `bypass_login` agent is analysing (noted only, not deep-dived here).
  - **`manage.so` diagnostics are CLP-mediated, and the sink is a *sanitizing* wrapper (Confirmed via rabin2/r2 — this tempers the naive-injection theory):** the exec helpers are `imp.system_safe` (imported from a Perle lib — a **sanitizing** command runner, per its name) and `imp.execvp` (**argv exec, no shell → no metacharacter interpretation**). Crucially, the web ping/traceroute do **not** build `system("ping "+host)`; they emit **CLP CLI** commands: `.rodata` `"test %d target %s type ping response-timeout %d"` and `"test %d target %s type traceroute limit %d"`. So the user-supplied `target` is passed as a **CLP CLI argument to `clpd`**, not to a shell. A `/bin/sh` string exists in `manage.so` but no direct xref resolved to a `sh -c $USERDATA` sink.
  - **Revised assessment:** the web CGI layer is **more defensive than a typical embedded box** (real PAM, sanitizing `system_safe`, shell-less `execvp`, CLI-mediated diagnostics). Command injection is therefore **not** a slam-dunk — the exploitable sink most likely lives **downstream in `clpd`** (the `system("sudo …%s…")` builders, §5), reached via the web `cmdline*`/`console`/`test` endpoints. Template set still exposes the interesting endpoints: **`cmdline.html`, `cmdline-full.html`, `console.html`, `debugging.html`, `ping.html`, `traceroute.html`, `software_management.html`, `flash_transfer.html`, `recovery_disk.html`, `config_replace.html`**. Because the CGIs run as `www-data` (sudo NOPASSWD:ALL), *any* injection that does land = root.
  - **Practical path (Hypothesis, revised confidence — medium):** authenticate to the web UI (`admin`/default), drive a `test`/`cmdline` endpoint, and get injection through `clpd`'s argument-to-`system()` path → executes as root (clpd `sudo`) or as `www-data`→`sudo`. Needs `clpd` decompilation to pin the exact unsanitized field.
- The `webgui-wrap` / `/usr/lib/cgi-bin/webgui` and stock Apache samples (`/usr/apache/cgi-bin/printenv`, `test-cgi`) are the **VyOS/Vyatta** GUI leftovers; the samples are mode `0664` (not executable) so `mod_cgi` won't run them — low value. The active UI is `/usr/www/root`.

### `perle-api-server` REST (Confirmed present; RCE = Hypothesis)
- `/usr/bin/perle-api-server` + `-ssl` variant (C++ Pistache/OpenAPI, 8.5 MB). Not in default systemd wants — enabled via config.
- Auth: HTTP **Basic** (`Pistache::Http::Header::Authorization::getBasicUser/getBasicPassword`) plus API tokens (`lib_database_add_user_token`, `cfgdb_*_restapi_token_entry`). Requires credentials/token.
- Exposes `AdministrationSoftwareManagementApi` (software update / image install), `AdministrationConfigurationApi`, archive/reload endpoints. **Firmware/software-install and config-replace endpoints are prime post-auth RCE / persistence candidates** (upload an image or config that runs code). Needs API creds and endpoint disassembly. **Hypothesis.**

---

## 7. Local priv-esc / persistence surfaces (Confirmed)

- **sudoers (the big one):** `/etc/sudoers.d/vyatta` → `%sudo ALL=NOPASSWD: ALL`. Members: `vyos`, `www-data`, `radius_priv_user`. Any shell as these = passwordless root. Also `%operator ALL=NOPASSWD: … PCAPTURE(/usr/bin/tcpdump) …` and `%users ALL=NOPASSWD: /opt/vyatta/bin/sudo-users/`.
  - **`tcpdump -z` escalation:** operators (`radius_user` ∈ operator) get `NOPASSWD` `tcpdump`. `sudo tcpdump -w /dev/null -z /path/script -G1` runs the `-z` post-rotate command as **root** — classic GTFOBins escalation from an operator-only shell.
- **Setuid/setgid:** `find -perm -4000/-2000` over the extracted tree returned **nothing** — but the rootfs was extracted from squashfs on macOS, which does **not** preserve the setuid/setgid bits, so this is inconclusive. `radius_shell`/`vbash` rely on **file capabilities** (`cap_setuid`) rather than the setuid bit; live `getcap` on-device is needed to enumerate the real capability set. *(Caveat, not a clean bill of health.)*
- **PAM:** `/etc/pam.d/login` uses `pam_securetty` (blocks root off `ttyMV0`, §1), `pam_faildelay`, standard `common-auth`. No backdoor PAM module observed.
- **World-writable sensitive files:** none found under `/etc` or `/product`. Note `/product` (perms `drwxr-xr-xrwx`, i.e. group/other-writable dir) contains `nvram/` — the persistent config store; group/other-writable **directory** permission is a persistence hook worth checking live (can add/replace files there).
- **cron:** only stock Debian jobs (`/etc/cron.daily/*`, `/etc/cron.d/mdadm`) — no custom Perle cron RCE. A writable cron dir would be a persistence sink post-root.
- **`inside-chroot`** (top-level, 0 bytes) and **`overlay/`** (empty dir): `inside-chroot` is an empty **marker file** — a flag some Perle startup script tests to detect it is running inside the product chroot/overlay (build-time/runtime sentinel), not an exploit surface. `overlay/` is the empty mountpoint for the writable overlay layered over the read-only squashfs root at boot. Neither is directly exploitable but both confirm the RAM-squashfs + overlay architecture (relevant to why `init=/bin/sh` in §1 works and why on-disk changes are volatile unless written to `/product/nvram`).

---

## 8. Recovery / factory / debug modes

- **U-Boot NFS/TFTP netboot** (`bootcmdnfs`, `nfsargs`, `load_product_uenv`, `uenvcmd`): full network-recovery boot path — point it at an attacker NFS/TFTP server to boot a controlled rootfs (physical serial + network). Confirmed in U-Boot env; §3.
- **Web recovery/factory:** templates `recovery_disk.html`, `config_replace.html`, `config_merge.html`, `software_management.html`, `flash_transfer.html`, `boot.html`, `reset_success.html` → factory-reset (restores `vyos`/`vyos`, §1) and image/config replacement via the web UI (post-auth).
- **`initdb` / `perle-init`** rebuilds the config DB on boot (oneshot on `ttyMV0`); factory-reset likely re-runs it against `config.boot.default`.
- **Debug password path (Confirmed strings, behaviour = Hypothesis):** `auth.cgi` `.rodata` contains `Config.DebugEnabled`, `Config.DebugPassword`, `Query.debug`, `Query.debug_pause`, `DS.Generic.factory.ip`. This is a vendor **debug-mode** auth path — if `DebugEnabled` is set (factory/support mode) a `DebugPassword` may grant elevated web access. Prime live-test target and a likely factory/support backdoor; may coincide with the separate agent's `bypass_login` analysis.
- **Debug web endpoints:** `debugging.html`, `test_messages.html`, `cmdline*`/`console` templates; `clpd` has extensive debug/`system()` paths. No open debug daemon/port found enabled by default in the systemd unit set. JTAG/UART test points are hardware — the FDT confirms `serial0` @115200 is the exposed UART (the one used for §1/§3); no JTAG evidence in the image.

---

## Final ranking & recommendation

1. **U-Boot serial interrupt → `init=/bin/sh`** — physical, unauthenticated, no bootloader password, confirmed, ~100% reliable. *Do this if you can touch the device.*
2. **Web UI cmd-injection (`manage.so system_safe`/`execvp`) → `www-data` → `sudo su -`** — best network vector; amplified by `%sudo NOPASSWD:ALL`; needs web admin login and confirmation of the injectable parameter.
3. **`vyos`/`vyos` (factory default) via SSH/console → `vbash` → `sudo su -`** — trivial if SSH is enabled or after a factory reset; confirmed credential+escalation chain.
4. **`operator` + `sudo tcpdump -z` → root** — confirmed sudoers path, needs an operator foothold first.
5. **REST API software/config upload → RCE/persistence** — needs API creds + endpoint analysis.
6. **CLP CLI `system("sudo …%s…")` argument injection** — high value if a reachable CLI field is unsanitized; needs `clpd` disassembly.
7. **Offline cracking of the shipped `$6$` `root`/`vyos` hashes** — fallback.
8. **`bypass_login`** — deferred to the other agent.

**Overall recommendation:** if physical/serial access is available, **U-Boot interrupt → `init=/bin/sh` (#1)** is the fastest, most reliable root. Network-only, pursue **web UI → `www-data` → `sudo` (#2)**, checking `vyos`/`vyos` and default web-admin creds first, and confirm the exact injection sink in `manage.so`.
