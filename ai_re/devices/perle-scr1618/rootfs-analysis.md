# Perle IOLAN SCR1618 — Firmware `scr-4.5.G4.img` rootfs analysis

Analysis date: 2026-07-19. Target (never modified):
`/Users/nep/nocloud/git/nepeat/ai_re/devices/perle-scr1618/scr-4.5.G4.img` (231,348,508 B).
Everything extracted under `devices/perle-scr1618/extracted/` (gitignored).

---

## 0. Extraction & integrity (confirmed)

The `.img` is a **U-Boot FIT image** (flat DTB, `d00dfeed`, v17). Parsed the FDT structure
directly (dtc chokes on the multi-hundred-MB inline `data` blobs) to recover each sub-image's
byte offset, length, and stored MD5. **All three MD5 hashes verify against the carved blobs:**

| FIT node | description | type / arch / comp | file offset | length | MD5 (stored == computed) |
|---|---|---|---|---|---|
| `/images/kernel@1`  | "Router Linux kernel"            | kernel / arm64 / none  | 244        | 16,294,400  | `9b93fcc85418dfc230f94e33d1ef72e6` ✅ |
| `/images/fdt@5`     | "Router Device Tree blob - 1 port" | flat_dt / arm64 / none | 16,294,892 | 12,042      | `60afea8370a53fcd5befcabbb9f16124` ✅ |
| `/images/ramdisk@1` | "Squashfs Image"                 | ramdisk / arm64 / none | 16,307,116 | 215,040,000 | `2efaae8ffe85f45d80b31a599fc5dcd5` ✅ |

- kernel → `extracted/kernel.bin`, fdt → `extracted/fdt.bin` (+ `extracted/fdt.dts`), squashfs → `extracted/rootfs.sqfs`.
- **rootfs:** SquashFS **4.0, xz-compressed**, 128 KiB blocks, 47,010 inodes, exportable(NFS),
  created **Wed Aug 11 23:27:12 2021**. Unpacked to `extracted/rootfs/` (885 MB, 4,499 symlinks).
- **FIT signature model:** the image nodes carry **only `hash@1` = md5** — there is **no
  `signature` node, no RSA/sha256 signing** in the FIT. So the FIT provides integrity (md5) but
  **not authenticity**. Whether the on-device U-Boot enforces a signature is out of scope of this
  image (bootloader not included), but nothing in the FIT itself is signed.

### Platform correction (evidence-based)
The FIT config codename "Amazon" is **Perle's board codename, not the SoC**. The device tree
(`extracted/fdt.dts`) says:
- `model = "Perle Amazon Board"`, `compatible = "marvell,armada3700"`.
- CPUs: 2× `arm,cortex-a53` / `arm,armv8`.
- Peripherals: `marvell,armada-3700-{spi,i2c,uart,uart-ext}`, `marvell,armada-3700-periph-clock-*`,
  a `perle,rtc-samproxy` RTC on i2c@0x68, and **two Marvell DSA switches** (`switch0@1`,
  `switch1@2`) exposing ports **`PerleEth1`..`PerleEth16`** (+ `PerleEth17/18` DSA/CPU uplinks).

**The SoC is a Marvell Armada 3700 (88F37xx), dual Cortex-A53 — NOT Annapurna Labs Alpine.**
Update the earlier PROGRESS.md hypothesis accordingly.

---

## 1. OS / platform (confirmed)

- **Base distro:** Debian GNU/Linux 8 (jessie), `ID=debian VERSION_ID=8` (`etc/os-release`),
  `etc/debian_version` = `8.11`.
- **It is a VyOS build:** `usr/share/vyos/version.json` → `1.2.0-rolling+201807030337`
  (built `autobuild@vyos.net`, 2018-07-03). `etc/issue` = "Welcome to VyOS". Admin user
  `vyos … /bin/vbash` in `etc/passwd`. VyOS 1.2 ("crux") is Debian-jessie based — consistent.
- **Perle product layer:** `lib/image-version` →
  `Router, Firmware version: 4.5.G4, Build date: Thu Aug 12 02:19:24 EDT 2021`,
  `Build directory: /eng_dev/router/v4_5_maintenance/device_server, Build user: root`.
  So: stock VyOS 1.2 (Jun–Jul 2018) + a Perle "device_server" 4.5.G4 layer built Aug 2021.
- **Kernel:** `Linux version 4.14.235 (builduser@dev2k18) (gcc 10.2.0, crosstool-NG 1.24.0.293)
  #1 SMP PREEMPT Thu Aug 12 02:16:35 EDT 2021`, aarch64. Modules in `lib/modules/4.14.235/`
  include **`GobiNet.ko` / `GobiSerial.ko`** (Qualcomm/Sierra cellular WWAN — the LTE OOB option).
- **Userland:** full GNU userland (not busybox-only): 249 real ELF files + 67 symlinks across
  `bin`+`sbin`; `bin/bash`, `bin/busybox` (1.x, as a helper), perl, python2/3. libc is
  **glibc 2.33** (`lib/libc-2.33.so`) — newer than jessie's stock 2.19 (Perle rebuilt the
  toolchain; some jessie-era libs like `libcidn-2.19` linger). Dynamic loader
  `/lib/ld-linux-aarch64.so.1`.

---

## 2. Perle console-server application & CLI (confirmed)

Architecture is a **hybrid**: stock VyOS provides the routing/config *backend*; a proprietary
Perle **"CLP" (Command Line Processor)** + an `iol_*` (IOLAN) daemon suite provide the actual
product management surface. The admin sees a Cisco-style Perle CLI, not raw `vbash`.

**VyOS backend (stock, unmodified):** `/bin/vbash`, `/bin/cli-shell-api` →
`/opt/vyatta/sbin/my_cli_shell_api`; config binary `/opt/vyatta/sbin/my_cli_bin` (+ `my_set`,
`my_delete`, `my_commit`, …). `/opt/vyatta/{bin,sbin}` are stock `vyatta-*`/`vyos-*` scripts (Jun 2018).

**Perle CLP front-end (the real CLI):**
- `/usr/bin/clpd` (2.8 MB aarch64 ELF) — CLP daemon, run by `perle-cli.service`
  (`Type=notify`, `Restart=always`). It **wraps VyOS**: contains literal config templates like
  `$SET interfaces ethernet %s description '%s'`, `$DELETE interfaces bridge %s description`, and
  `cfg2clitext`/`cfg2cli_*` translators. Debug facility `PERLE_FAC_CLPD`; Cisco-ish semantics
  (`EXEC is disabled on the console`, `configure`, `CHECK_TOKEN_HOSTNAME/IPv4/IPv6`).
- **39 CLI feature plugins** `/usr/lib/clp/*.so` (e.g. `iolan_`, `router_`, `ip_host_`,
  `crypto_`, `dot1x_`, `lte_`, `gnss_`, `wan_`, `vrrp_`, `sp_tree_`, `hotspot_`,
  `ip_passthrough_`, `power_mgmnt_`), entry symbol `_clp_recur_command`.
- **Config DB / request handler:** `/usr/bin/reqhandlerd` (`perle-reqhandlerd.service`; holds the
  `4.5.G4` version string) and `/usr/bin/initdb` (`perle-init.service`; `initdb -load_startup`
  bridges Perle nvram config ↔ VyOS active config). Persistent config lives under **`/product/`**
  (nvram/flash), not VyOS `/config`.

**Serial / console-server port handling** (no `conserver`/`ser2net` — Perle's own suite):
- Per-port controller `/usr/bin/portctl` via templated unit `console_portctl@.service`
  (`ExecStart=/usr/bin/portctl %i nopasswd 4`); references `/dev/ttyS*`, `/dev/ttyMV0/1`,
  reverse sessions started by TELNETD/SSHD, `/etc/console_ready`.
- `iol_*` daemons in `/usr/bin`: port mgmt (`iol_portmgr`, `iol_spcd`), reverse access
  (`iol_telnetd`, `iol_sshd`, `iol_rlogin`, `iol_rshd`, `iol_sredird` = RFC2217 com-port
  redirector, `iol_rsessmgr`, `iol_multihost`), buffering/logging (`iol_lpbfr`, `iol_rpbfr`,
  `iol_logind`, `iol_lldatalog`, `pty_gs0`), and serial protocols (`iol_smodbusd/p`,
  `iol_mmodbusp` Modbus gw, `iol_rawd`, `iol_udpd`, `iol_pppd`+`iol_chat`, `iol_vc`/`iol_vcqd`
  vmodem). Web templates `/usr/www/templates/iolan_*.html` enumerate serial profiles: tcp, udp,
  trueport, vmodem, ppp, slip, modbus, serial_tun, printer, terminal, console.
- **REST API:** `/usr/bin/perle-api-server` (8.5 MB, `perle-api-server.service`) + `-ssl` variant
  — C++ **Pistache** HTTP framework with **OpenAPI-generated** handlers
  (`org::openapitools::server::api::NetworkApi…`, `get_ipv4_dhcp_leases_handler`,
  `getRESTApiSSLEnabled`).
- Platform managers (`perle-*.service`): `iol_perleinit`, `perle-ledmgr`, `perle-alarmmgr`,
  `perle-trapmgr`, `perle-drmgrd`, `perle-rtcmgr`, `perle-ipsecmgr`, `perle-emailnd`,
  `perle-lte-firmware-sync`, `wanifmgr`, etc.
- Proprietary libs in `/lib`: `libiolan.so`, `libpamperle.so`/`libiol_pamperle.so` (PAM),
  `libsslperle.so`, `libipsecperle.so`, `libperle_snmplib.so`, `libperle_libnetsnmpmibs.so`,
  `libperle_glibc.so`.

**OEM lineage (strong hypothesis):** `/usr/share/snmp/mibs/product/` ships **two parallel MIB
trees — `perle-*.MIB` and identically-named `etherwan-*.MIB`** (e.g. `perle-IOLAN-SDS.MIB` vs
`etherwan-IOLAN-SDS.MIB`) → the console-server/router stack appears **OEM-shared with EtherWAN**,
rebranded as Perle IOLAN. Model IDs: `SCR1618` / entity `SCR1618RDAC`
(`perle-ENTITY-VENDORTYPEOID.MIB`), family `IOLAN SCR` (`perle-PRODUCTS.MIB`).

---

## 3. Network services & management (confirmed)

- **SSH (OpenSSH `/usr/sbin/sshd`):** no `sshd_config` and **no `ssh_host_*_key` baked in** — host
  keys generated at first boot (`/opt/vyatta/bin/ssh-server-key`, `dpkg-reconfigure
  openssh-server`). `sshd_config` generated at runtime by
  `/usr/libexec/vyos/conf_mode/ssh.py`; **`PermitRootLogin` defaults to `no`**
  (only `yes` if `service ssh allow-root` set). Good: no shared host keys, root SSH off by default.
- **HTTPS management = Perle Apache** `httpd.service` ("Apache PERLE HTTP Web Server",
  `/usr/apache/bin/httpd`), `DocumentRoot /usr/www/root`, TLS vhosts on `${_SSL_PORT}` bound to
  `169.254.0.1` (link-local recovery) and `_default_`. **Web UI = compiled aarch64 CGI +
  ClearSilver templates:** `/usr/www/root/auth.cgi` (256 KB ELF, login), `manage.cgi` +
  `manage.so` (1.7 MB, main app), templates in `/usr/www/templates/*.html`.
  `/usr/apache/cgi-bin/` also still contains **stock Apache sample CGIs** (`printenv`, `test-cgi`,
  `printenv.vbs/.wsf`) — classic info-leak samples that shouldn't ship.
- **Legacy VyOS lighttpd GUI** also present (`service https` template, docroot
  `/var/www/html/Vyatta`); generates its own per-device self-signed cert at commit
  (`openssl req … /C=US/CN=Vyatta Web GUI`).
- **SNMP:** `snmpd` present but `etc/snmp/snmpd.conf` etc. are **0 bytes** (generated from config);
  VyOS `service snmp community` is `[REQUIRED]` — **no default/hardcoded community** (no "public")
  in the image; `snmpd` is stopped by default (`K02` in `rc2.d`).
- **Telnet:** `/usr/sbin/telnetd` binary exists but **no init/xinetd/systemd unit enables it**
  (`etc/xinetd.d` empty). Off by default. No dropbear, no nginx.
- **Boot-enabled units** (`multi-user.target.wants`): `atd, cron, perle-cp-startup-cfg,
  perle-cli, perle-init, perle-reqhandlerd, perleTemp, pppd-dns, rsyslogd0, ssh-session-cleanup,
  vyatta-router`. `httpd`/`ssh` are started on demand by the Perle/VyOS config layer.

---

## 4. Credentials & secrets (HIGH PRIORITY)

### 4.1 `/etc/shadow` (baked into this image)
Two SHA-512 crypt (`$6$`) hashes are present; the rest are locked (`*`/`!`):
- `root : $6$u/1V/kCY$rhV91K7Gg9X6YCsUalcOxgDausuJDRy3gbYh9kW88PqqJsLVV0IpPEoahK8qcNtSkr4omn6cZsMRYcxtJsDux.`
- `vyos : $6$l5f.uVN6$J8DlFFtBgaEFXZoJSL7j7CzJmKx3Sl6wF17UAopN6VzmLABzSUHSwxZR4EWox5NcpaI3EysYPrpNI/RmqZONa/`

These are **Perle-set** and are **NOT** the stock `vyos`/`vyos` hash (verified: `vyos` does not
match either salt; a quick list of perle/iolan/scr/admin/etc. also failed). A rockyou run
(`john`, sha512crypt) did not recover them in the available time — **uncracked**. They matter less
than they look: these are *build-time seeds* in the squashfs, and on a factory boot VyOS applies
`config.boot.default`, which sets the login password from the field in §4.2, i.e. the effective
credential becomes **`vyos`/`vyos`** regardless of these seed hashes.

### 4.2 Factory default account = `vyos` / `vyos` (baked into config, confirmed cracked)
`/opt/vyatta/etc/config.boot.default` (+`.default-orig`) ships system-login user **`vyos`**, level
`admin`, with
`encrypted-password $6$QxPS.uk6mfo$9QBSo8u1FkH16gMyAVhus6fU3LOzvLR9Z9.82m3tiHFAxTtIkhaZSWssSgzt4v4dGAL8rhVQxTg0oAG9/q11h/`.
This is the well-known VyOS default hash; **verified by recompute with `openssl passwd -6 -salt
QxPS.uk6mfo vyos` → exact match. Factory web/SSH/console login = `vyos` / `vyos`.** VyOS applies
`config.boot` at boot and sets the system password from this field, so on a factory device the
effective credential is `vyos`/`vyos` regardless of the (different) squashfs `/etc/shadow` seed.

### 4.3 Baked-in **shared TLS private key** (top secrets finding)
`/etc/default_ssl_rsa_cert.pem` — a combined **self-signed cert + 2048-bit RSA PRIVATE KEY in one
PEM (3022 B)**, shipped identically in every unit:
- Subject/Issuer `C=CA, ST=Ontario, L=Markham, O=Perle Systems Limited, CN=PerleRouter`,
  valid `Jun 12 2020 → Jun 10 2030`.
- Cert SHA-256 FP `AA:A8:1F:57:AE:E3:2A:15:68:15:6A:ED:06:F2:74:4F:9C:15:48:18:54:4A:2A:73:70:66:99:2C:C2:FC:F7:39`;
  file MD5 `0151ecc5edc2bf55c8225f4973e03b7b`; SPKI SHA-256 `4656aa65ee0262fa72a6ac172000ad9cce3e0b40155e6df29688678bb14849ee`.
- Apache serves TLS via `/usr/apache/conf/server.{crt,key}` → `/etc/ssl_rsa_cert.pem` (the active
  copy, created at first boot from this default). Until an operator regenerates the cert, **every
  device serves HTTPS with this identical private key → trivial impersonation / passive MITM.**

### 4.4 Other key material
- **SSH host keys: NOT in image** (generated first boot) — good, not shared.
- `/etc/lighttpd/server.pem` (VyOS GUI) — not in image, generated per-device.
- `usr/bin/managed-devices*.yaml` — OpenAPI **example** certs only (O=CompanyABC, "Easy-RSA
  Generated"), no private keys, not real secrets.
- Public trust anchors only: `usr/share/dns/root.key`, `lib/crda/pubkeys/*.pem`,
  `etc/ssl/certs/*`, CA bundle — benign.
- No other hardcoded passwords/API keys/tokens found in `/etc` or `/opt/vyatta/etc` (only PAM /
  strongSwan / xl2tpd template placeholders like `<put password here>`).
- Recovery: `/opt/vyatta/sbin/standalone_root_pw_reset` (console root pw reset — needs physical access).

---

## 5. Firmware update & verification (confirmed)

### 5.1 On-device format is a plaintext FIT `fit.itb` — NOT `.emg`
The device installs/boots an unencrypted U-Boot **FIT image** (`fit.itb`), identical in structure
to the `scr-4.5.G4.img` analysed here. The extracted rootfs is the squashfs *inside* such a FIT,
so by the time it runs, any outer `.emg` wrapper has already been removed.

**Update triggers:**
- Web: `usr/www/templates/software_management.html` "Update Software" forms POST to
  `manage.cgi/file_transfer?file_type=firmware&protocol=…`.
- CLI: stock Vyatta `add system image` →
  `opt/vyatta/share/vyatta-op/templates/add/system/image/node.tag/node.def` runs
  `sudo /opt/vyatta/sbin/install-image "$4"` (legacy ISO installer, not the primary path).

**Backend & storage:** `usr/www/root/manage.cgi` (thin, `execl`s `auth.cgi`) →
`usr/www/root/auth.cgi` (256 KB) → `usr/www/root/manage.so` (1.7 MB real handler) and
`usr/bin/reqhandlerd`. Images live on eMMC:
`/media/image/{tmp,current,backup}/fit.itb` (+ `description.txt`), staged as
`fit.itb.new` on `mmcblk0p2` (`lib/libiol_globals.so`). Flow: upload bytes → write
`tmp/fit.itb` → build description → swap current↔backup → reboot. **No decrypt step in this path.**

**Boot chain:** bootloader present at `lib/firmware/router-uboot.bin` (a Marvell **Armada-3720 /
EspressoBIN** U-Boot — independent confirmation of the SoC). U-Boot:
`bootcmdmmc=… init=/sbin/prodinit; run bootmmc_current; run bootmmc_backup; run bootmmc_error`;
`bootmmc_current` loads `/current/fit.itb` then `boot_fit → bootm ${loadaddr}#ramdisk_conf@${hw_type}`,
falling back to `/backup`. `sbin/prodinit` mounts the FIT squashfs from eMMC as the overlay root.

### 5.2 Verification / signature model — integrity only, no enforced authenticity
- **Upload path performs no signature/hash authenticity check** — bytes are written straight to
  `fit.itb` and swapped; no detached-sig, no `sha256sum` manifest, no `openssl dgst` in
  `manage.so`/`reqhandlerd`.
- **Boot-time = FIT *hash* integrity only.** U-Boot verifies FIT md5/CRC hashes ("Verifying Hash
  Integrity", "Bad hash in FIT image!"). FIT *signature* verification code is compiled into U-Boot
  (`key-name-hint`, "Bad Signature") but **there is no evidence verified boot is enforced** — no
  "Verifying signature", no active rsa key hint, no required-key gating; `bootcmd` selects an image
  purely on successful load + hash. Consistent with the FIT itself (§0) carrying only md5 hashes
  and no signature node, and with the kernel having **no dm-verity / IMA / EVM / module-sig
  enforcement** (verified in `extracted/kernel.bin`).
- **Net:** a structurally-valid FIT with correct hashes will flash and boot — nothing
  cryptographic prevents an unsigned/modified image.

### 5.3 The `.emg` decryptor is NOT in this rootfs (confirmed negative)
Whole-tree binary-safe search found **zero** occurrences of `.emg` and **no** GPG-symmetric decrypt
logic, passphrase, or firmware keyring:
- `grep -rlaF '.emg'` → 0 files. No Perle binary/script invokes `gpg`/`gpgv` for firmware; only
  stock apt `usr/bin/{gpg,gpgv}` (unused by the update path).
- The `aes-256-cfb` strings in `lib/libiol_globals.so` are a **red herring** — the full OpenSSL EVP
  cipher-name table (all `aes-*-cbc/ofb/cfb`), not a decrypt routine.
- `lib/libsslperle.so` = password-strength + SSL cert/key only; `manage.so` `passphrase` strings are
  SSL cert-import fields. `libcrypto.so.1.0.2` used only for SSL / one SSH host-key conversion.
- Only keyrings on disk are Debian apt (`usr/share/keyrings/debian-archive-keyring.gpg`,
  `etc/apt/trusted.gpg.d/*`). No `secring`/`pubring`/`*.asc`/`~/.gnupg`; `/product/keys` absent.

**Conclusion:** the Perle `.emg` GPG-symmetric AES-256 wrapper is decrypted **outside** this running
userspace — either by a separate recovery/bootstrap updater or a flash partition not in this image,
or (more likely) only ever by Perle's build/release tooling, with the device receiving/booting the
already-decrypted `fit.itb`. **The passphrase that would open `scr-v2-8.2.G1.emg` is not recoverable
from this firmware.** To pursue it you'd need other eMMC partitions (`mmcblk0p2`, a recovery image)
or the factory bootstrap loader — none of which are contained here.

---

## 6. Security findings (summary)

1. **Shared/static HTTPS private key** `/etc/default_ssl_rsa_cert.pem` (CN=PerleRouter) baked into
   every unit → impersonation + passive HTTPS decrypt on any device not re-provisioned. (§4.3)
2. **Factory default admin `vyos`/`vyos`** (confirmed-cracked, `config.boot.default`). (§4.2)
3. **FIT image is md5-hashed but not cryptographically signed** — no authenticity at the FIT
   layer. (§0)
4. **Stock Apache sample CGIs** left in `/usr/apache/cgi-bin` (`printenv`, `test-cgi`) — info leak. (§3)
5. **World-writable `/product` and `/product/nvram`** (`drwxr-xrwx`) — the persistent config/nvram
   store; local-tamper concern.
6. **`bypass_login` / `password_recovery_mode`** config paths referenced in `auth.cgi`
   (`config_get_bypass_login`, `DS.Auth.bypass_login`, `DS.System.password_recovery_mode`,
   two-factor `tflogin`) — a config-driven login-bypass/kiosk mode; needs binary RE to confirm
   whether it can be triggered unauthenticated (potential auth bypass).
7. Old software base (Debian 8 jessie EOL, VyOS 1.2 rolling 2018, kernel 4.14 — all long EOL).
