# Netvue NI-3421 — Progress

Pan/tilt Wi-Fi IP camera (2MP, H.264). RE journal — newest first.

## FULL DUMP CAPTURED + APPFS ANALYZED (2026-07-19)
- **`fulldump_full.bin`** sha256 `7f52b9460f8f0ade1fa7e5be162881991d7cabd8311d5a8c86c1e13cbb9a1cfd` — VERIFIED
  by 6 byte-identical complete reads (majority vote via `dump_vote.sh`; corrected 3 single-bit errors
  that were in the earlier single `fulldump.bin`). Region reads are garbage on this rig — full reads only.
- appfs (mtdblock3, `/mnt/mtd`) extracted → `extract/appfs_out/`. Build tag leaks:
  `gitlab-runner/.../nv-camera-iii/nv_v4_package` (Netvue "nv_v4" app).

### Q&A (the four questions)
1. **Bluetooth?** NO — no BT radio (RTL8188FU wifi-only + T21), no BT driver (only `8188fu.ko`/`mt7601`),
   no BT binaries. Cannot listen on BT. Pairing is Wi-Fi (SmartLink/AP).
2. **On Wi-Fi it:** joins as STA (`8188fu.ko`+`wpa_supplicant`, PSK plaintext in wpa_supplicant.conf),
   DHCP (udhcpc/dhcptool), fallback DNS 8.8.8.8/8.8.4.4/9.9.9.9/208.67.220.220, NTP `0.ntp.nvts.co`.
   Outbound cloud: **MQTT** control (mTLS client cert `config/v4/client_priv.key`+`client_cert.crt`+
   `rootCA.crt`), HTTPS `localweb.nvts.co` (default) `/v1/deviceconfig`, AI `ai.nvts.co/human-detection`,
   media→`s3.*.amazonaws.com`, public-IP via `checkip.amazonaws.com`, rebrand host
   `localweb-prod.onelinkbell-firstalert.com` (First Alert OneLink). Live video = **WebRTC (AWS KVS)** +
   STUN/**TURN**. LAN: **UDP broadcast auto-discovery** responder + local TCP stream/"speed" servers +
   **UPnP AddPortMapping** (asks router to forward a port).
3. **Services:** getty on serial console (root/NO password), mdev, wpa_supplicant, and `loaderd` which
   supervises logger/watchdog/network/upgrade/upgrade_ca/**main** (the camera app). `firmware.sh` runs
   `pkill -9 telnetd` at boot → **telnet killed**. NO telnetd/ssh/dropbear/httpd/inetd exposed. Listeners
   are main's local UDP discovery + TCP streaming servers. cron: daily `upgrade_check.sh`.
4. **Port-knock?** NONE. No knockd/iptables/magic-packet/backdoor. Network-facing behavior = passive
   UDP LAN discovery responder + active UPnP port-forward request; local TCP servers open at startup
   (not knock-gated).

### Security notes
- **Passwordless root via UART** (empty shadow + console getty).
- Wi-Fi PSK stored plaintext; **cloud mTLS private key on flash** (`config/v4/client_priv.key`) →
  extractable, could impersonate device to Netvue cloud.
- telnetd binary present (busybox) but actively killed at boot.

## Status (2026-07-19, latest)
- **rootfs fully captured & extracted; appfs (the app) still truncated by clip brownout.**
- `fulldump.bin` (sha256 `d8ca697e…59bd7`): boot+kernel+**rootfs complete**, appfs real data only to
  **0x5abc94** (~1.7 MB of 12 MB) then brownout → all-0xFF. rootfs jffs2 extracted clean (CRC-valid).
- **KEY FINDINGS (from rootfs):**
  - **`root` has NO password** (`shadow: root::…`) + **getty on serial console @115200** (inittab)
    → **UART = instant root shell, no password.** The 4-pin header is the way in.
  - `telnetd` is **commented out** in `rcS` (not started by default).
  - **No Bluetooth** anywhere (no BT radio in HW; zero bt/hci binaries or modules in rootfs). Confirmed.
  - App architecture: `rcS` → mounts **mtdblock3 (appfs) at `/mnt/mtd`** → runs `/mnt/mtd/boot.sh`
    → `/mnt/mtd/netvue/firmware.sh &`. PATH/LD add `/mnt/mtd/netvue/firmware/{bin,lib}`.
    cron: daily `/mnt/mtd/netvue/firmware/bin/upgrade_check.sh` (OTA).
  - Camera app is **WebRTC-based** (`/etc/webrtc_profile.ini` stub → appfs). hostname `netvue-jz-ipc`.
    Kernel modules dir tag `isvp_monkey_1.0` (uImage tag was `isvp_turkey_1.0` — minor SDK skew).
  - `/etc/Wireless`, `/etc/sensor`, `webrtc_profile.ini` all symlink/stub into appfs → **Wi-Fi/service/
    port behavior is ALL in the appfs**, which we don't have yet.
- **Blocking:** getting the full appfs. Two dead ends found:
  1. **Full sequential reads brown out** (~2–5.7 MB in) — CH341A back-powers BOTH boards via flash VCC,
     rail sags. Pre-brownout data IS valid (fulldump.bin 0..0x5abc94 verified). But tail never reached.
  2. **flashrom region reads (`-l layout -i cNN`) return GARBAGE** on this ch341a build — a deterministic
     repeating `40 18 c8` pattern for every region, not real data. Double-read validation was fooled by
     the determinism. `dump_chunks.sh` output (`fulldump_full.BAD_regionreads.bin`) is INVALID — quarantined.
- **Only full sequential reads give real data here.** To capture the appfs tail, must kill the brownout:
  - **(try first, non-destructive)** unplug the sensor board from the main board so CH341A only powers the
    small sensor board (flash lives on sensor board) → far less load → a full read should complete.
  - **(surest)** desolder the SOIC-8 and read standalone.
  - or hold the SoC in reset / inject external 3.3 V during a full read.
- **Fastest route to the actual Q's:** the **passwordless root UART shell** — just log in and run
  `netstat -tulnp` / `ps` / `iptables -L` / read `/mnt/mtd/netvue/firmware.sh` on the live device.
- NOTE: `/etc/resolv.conf` in the extract is a symlink that reads the HOST's file — ignore it.

## (superseded) earlier status
- **`dump1.bin` is PARTIAL — only boot+kernel valid. Rootfs+appfs (upper 14 MB) read back all-0xFF.**
- SoC confirmed = **Ingenic XBurst T21**. Boot+kernel analysis solid; userland NOT captured yet.
- Cause: flaky in-circuit clip **brown-out** — CH341A back-powers both whole boards through the flash
  VCC, rail sags after ~2 MB, chip stops responding, rest reads as pull-up 0xFF. Later reads died with
  `device not responding`; programmer now needs a USB re-plug (`Couldn't open device 1a86:5512`).
- **TODO: clean full re-dump.** Best fix = **desolder the SOIC-8 and read off-board** (kills back-power
  + SoC bus contention). Or hold SoC in reset / inject stable 3.3 V. `dump1.bin` boot+kernel = good.

### Verified from partial dump
- uImage @0x48000: LZMA kernel, **type=Kernel, 1.60 MB, NO embedded ramdisk** → separate rootfs required
  on mtdblock2 (jffs2). That partition + appfs are blank in the dump = the missing 14 MB.
- Hardware fact (no dump needed): board has **RTL8188FTV = Wi-Fi-only (2.4 GHz 802.11n), no Bluetooth
  radio**, and T21 has no BT. → **The camera has no Bluetooth at all; it cannot listen on BT.**

## Correction (2026-07-19)
Earlier photo-ID guessed the main SoC as "Grain Media" from a cursive **"S"** logo — **WRONG**.
The firmware proves it's an **Ingenic XBurst T21** (`jz_sfc` controller, `Board: ISVP (Ingenic
XBurst T21 SoC)`, kernel `Linux-3.10.14 __isvp_turkey_1.0__`). ISVP = Ingenic Smart Video Platform.

---

## 2026-07-19 — Flash dump via CH341A

- Programmer: **CH341A** (WCH `1a86:5512`, macOS shows it as "USB UART-LPT"), `flashrom -p ch341a_spi`.
  - macOS driver-detach warnings are benign; no sudo needed once the device is free. Only run **one**
    flashrom at a time — back-to-back reads hit `LIBUSB_ERROR_ACCESS` / "opened for exclusive access".
- JEDEC ID matched **GigaDevice GD25Q128** family (16384 kB) → confirms `MD25Q128SIG` is a GD25Q128
  clone. Read with `-c GD25Q128C` (read path identical across the matching defs).
- **`dump1.bin`** — full 16 MB, sha256 `30371c16…a4b219`. Internally consistent: uImage magic lands at
  exactly `0x48000` (the boot→kernel boundary), so the read is aligned/clean.

### Recovered layout (from bootargs `mtdparts=jz_sfc:...`)
```
console=ttyS1,115200n8 mem=40M@0x0 rmem=24M@0x2800000 init=/linuxrc
rootfstype=jffs2 root=/dev/mtdblock2 rw
```
| mtd | name   | size   | offset      | contents |
|-----|--------|--------|-------------|----------|
| 0   | boot   | 288k   | 0x000000    | U-Boot SPL + **U-Boot 2013.07** (Mar 01 2020) |
| 1   | kernel | 1760k  | 0x048000    | uImage **Linux-3.10.14 __isvp_turkey_1.0__** (MIPS) |
| 2   | root   | 2048k  | 0x200000    | **jffs2** rootfs, mounted rw as `/` (mtdblock2) |
| 3   | appfs  | ~12 MB | 0x400000    | app filesystem (bulk) |

Console/UART = **ttyS1 @ 115200 8N1**.

## 2026-07-19 — Chip identification from teardown photos

Two-board stack, joined by a board-to-board / FPC interconnect:
- **Main board** (square) — SoC, Wi-Fi module, motor driver, power.
- **Sensor board** `C240-M11-00` dated `2019.10.14` (round-ish, has lens + IR) — image sensor, **SPI flash**, audio amp.
Photos in `photos/` (converted from the source HEIC).

### Main board (`photos/IMG_8047-8049`)
| Ref / marking | Part | Function |
|---|---|---|
| Big QFP w/ cursive **"S"** logo (no PN printed) | **Ingenic XBurst T21** (MIPS) — confirmed by firmware, NOT Grain Media | Main SoC / ISP / H.264 encoder |
| Green shielded module, `REALTEK 8188FTV J91G631` + `M40.00` xtal | **Realtek RTL8188FTV** | 2.4 GHz 802.11n Wi-Fi (SDIO/USB) |
| SOP-18, `ULN2803A 1930LH` | **ULN2803A** | 8-ch Darlington array — drives pan/tilt stepper motors |
| Metal-can module, label `2008TG21-A / 002468 / H264 200万` | LAN magnetics / RJ45 transformer (2MP H.264 product label on it) | Ethernet magnetics (if wired RJ45 present) |
| `4R7` inductor, misc SOT regulators | power | 5V→3.3V/1.x rails |

### Sensor board `C240-M11-00` (`photos/IMG_8043-8046`)
| Ref / marking | Part | Function |
|---|---|---|
| SOIC-8, **`MD25Q128SIG`** (top mark `MD AJ1947`, lot `E88389`) | **25-series SPI NOR flash, 128 Mbit = 16 MB**, 3.3V. JEDEC-compatible 25Q128 (W25Q128/GD25Q128 clone; "MD" house brand). | **← THE FLASH. Firmware/boot NOR.** |
| SOP-8, `8002B LX1931HF` | **8002B** class-AB ~3W mono audio amp | Speaker driver |
| Center under lens | 1/2.7" 2MP CMOS sensor (H.264 200万) | Image sensor |
| `AACAP`, `2R2`, `1R0`, `PT14`, IR-CUT labels | DC-DC + IR-cut driver | Sensor power + day/night IR-cut switch |

## Where is the flash?
**`MD25Q128SIG`, SOIC-8, on the sensor board `C240-M11-00`** — top-left corner next to the
board-to-board connector and a **4-pad gold header** (see IMG_8044/8045). 16 MB SPI NOR at 3.3V.
The SoC on the *other* board boots from it over the interconnect (no separate NOR visible on the
main board; possible NAND could hide under the SoC — unconfirmed).

## Next steps / candidates to probe (non-destructive first)
- The **4-pad gold header** by the flash on the sensor board is a prime **UART candidate** — scope for
  a 115200 boot log (3.3V logic — confirm before connecting).
- In-circuit **SPI dump of the MD25Q128** with a clip (SOIC-8, easy) — CH341A/flashrom; JEDEC ID will
  confirm exact part. Hold SoC in reset or power sensor board alone.
- Confirm Grain Media SoC PN via boot log (GM8135/GM8136 print their model at boot).
