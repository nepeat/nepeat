# ZA-816S-4-W — Wi-Fi Camera RE

Running journal for a cheap "PDD-tier" Wi-Fi camera (Ingenic T31 platform).
Newest entries first.

## Identity

From the label sticker (`photos/IMG_8036.jpg`):

| Field   | Value             |
|---------|-------------------|
| Model   | **ZA-816S-4-W**   |
| QC line | QC16  37LC-X      |
| S/N     | 2306050           |
| UUID    | 406A8EC8F752 → MAC `40:6A:8E:C8:F7:52` |

- Main-board silkscreen: **`JZ31_YT_YH2C_S01`** (`photos/IMG_8031.jpg`).
- **Manufacturer/ODM: Hangzhou Puwell OE Tech Ltd.** (Wi-Fi MAC OUI `40:6A:8E`;
  cloud `ipc365.com`; app **IPC365**). Puwell model `PW879373W-GMY`, FW `5.30.82.04`.
- Class: **dual-lens "gun-ball" (枪球联动) PTZ auto-tracking Wi-Fi camera** with
  spotlight/color night vision — fixed 1080p "gun" (JXF37P) + PTZ 1080p "ball"
  (sc3235), two Ingenic T31 SoCs (see `comms.md`). Whitelabel — same hardware
  ships under many retail brands (SV3C/GENBOLT/ICSee-style, etc.).
- S/N + date codes on chips (2020–2023) suggest ~2023 build.

## Board(s)

**This is a dual-T31 camera — TWO independent Ingenic T31 SoCs, each with its
own SPI flash running its own Linux.** Confirmed by dumping both: all four flash
regions (u-boot/kernel/rootfs/config) differ between the two.

1. **Wi-Fi / app board** (`JZ31_YT_YH2C_S01`) — T31 (lot `260902612B01`), Hi3861
   Wi-Fi, PUYA P25Q64SH @ **U6**, audio, motor drivers. Pads: `SPK`, `MIC`,
   `MOTOR`, `LED`, `5V8`, `1V8`, `0V8`, `3V`. Flash → `dump/p25q64_GOOD.bin`.
2. **Sensor / ISP board** (`JZ31YT_YH0I_ICS_BOX_V1_3`, `photos/IMG_8037-8039`) —
   T31 (lot `260412614B01`), CMOS image sensor @ **U6** (model TBD), PUYA
   P25Q64SH @ **U4**, 8002A amp. **Exposes `R`/`T`/`G` UART pads** (bottom-left,
   `IMG_8037`) = serial console. Flash → `dump/sensorboard_GOOD.bin`.
3. **Secondary small board** (`photos/IMG_8033-8035.jpg`) — JieLi (杰理) SoC +
   LED/USB pads. Button / indicator / chime daughterboard. Role TBD.

Both T31 boards carry the **same `root` shadow hash** (`$6$GupOzpfi$…`) — one
crack unlocks both.

### Architecture: gun-ball auto-tracking camera → see `comms.md`

Full board-to-board comms write-up is in **`comms.md`**. Summary:
- **Ball (master)** = board `YH2C` — PTZ dome, own sensor (sc3235), Hi3881
  Wi-Fi, person-detect AI, cloud/RTSP gateway. `zrt_app` + `Daemon_app`.
- **Gun (slave)** = board `ICS_BOX` — fixed **JXF37P** camera, encodes + pushes
  video, no Wi-Fi. `rndis_server`.
- **Link:** internal **USB → CDC-ECM/RNDIS ethernet**, static `192.168.128.0/24`
  (ball `.15` host ↔ gun `.16` gadget). Gun = TCP **server on 12347 + 12351**,
  ball = client. Video/motion pushed gun→ball; PTZ "shake" control + sensor
  config + firmware-flash pushed ball→gun. No link auth.
- **Vendor:** Puwell / **IPC365** cloud (AWS DynamoDB), FW `5.30.82.04`, model
  `PW879373W-GMY`, hostname `Zeratul`. Supports a 2nd gun on `192.168.127.0/24`.
- Board diff: all four flash regions differ; same firmware family, two role
  builds. Extracted trees + configs live under `dump/analysis/` (gitignored).

## Chip inventory

See `chips.md` for the full ID table with per-photo references and
confidence levels. Headliners:

- **SoC:** Ingenic **T31** (`T31 260902612B01-LC`, i.e. T31L "Lite") — MIPS
  XBurst video/AI camera SoC.
- **Wi-Fi:** HiSilicon **Hi3861** module (`Hi3861 RNIV100 / ATFB1B1V8`) —
  2.4 GHz 802.11 b/g/n + RISC-V MCU.
- **Flash:** PUYA **P25Q64SH** — 64 Mbit (8 MB) SPI NOR. ← **primary dump target**
- **Audio amps:** 2× **8002A** (`CQ1Y1M.1N`, SOP-8) — mono class-AB/D amps.
- **Motor / driver ICs:** BORN `BE2803LV-24` (ULN2803-style Darlington array),
  YX-TEK `SMD1307G`, `AFD2 11F1` (TSSOP-16) — pan/tilt stepper drive. (unconfirmed)
- **Power:** `5030 7418` QFN (buck/PMIC, unconfirmed) + inductors L1/L3.
- **Daughterboard SoC:** JieLi `JL... N032C NF` — BT/audio MCU.

## Power

- **Input: 5 V DC via micro-USB.** Confirmed by: micro-USB female port on the
  board (`photos/IMG_8032.jpg`) + onboard speaker sticker marked `DC 5V`
  (`photos/IMG_8024.jpg`). Internal bucks derive `S2V8`/`S1V8`/`1V8`/`0V8`
  sensor + core rails from the 5 V.
- **Use a 5 V / 2 A supply** (headroom for pan/tilt motors + IR LEDs).
- **⚠️ Never apply 12 V** — T31 cube/PT cams are 5 V, unlike 12 V CCTV bullet
  cams. 12 V will destroy the T31 / Hi3861.

## Flash dump — DONE (2026-07-19)

Both T31 boards dumped, both CRC-verified. See `dump/SHA256SUMS`. LFS-tracked.

- **`dump/p25q64_GOOD.bin`** — Wi-Fi/app board (U6). `sha256 d09c3155…95434`.
- **`dump/sensorboard_GOOD.bin`** — sensor/ISP board (U4). `sha256 c485bd9a…d89378`.
  Same ISVP-Swan layout & offsets; kernel data size 1538097, rootfs 3.81 MB,
  164 files. uImage hdr+data CRC **OK**, squashfs xz-extract **OK**. One flaky
  read differed (try1) but was refuted by the CRC check on the good image.

- All region details below refer to the **first (Wi-Fi/app) board**; the sensor
  board mirrors the layout with different contents.
- Read with **flashrom v1.7 + CH341A**, `-p ch341a_spi` (in-circuit clip).
  **No sudo needed** on this Mac — the CH341A has no kernel driver bound, so
  libusb claims it as a normal user despite the `LIBUSB_ERROR_ACCESS` warning.
- Clip contact is **flaky**: only 1 of ~10 reads completed; failures show as
  `REMS 0 kB` / "no device" (a data pin, likely MISO, losing contact).
- Confidence is high despite the single read — verified independently:
  - uImage **header CRC + data CRC both validate** (Python `zlib.crc32`).
  - SquashFS rootfs **fully xz-extracts** (every block CRC-checked), 194 files.
  - Exact 8388608 bytes, coherent partition map, trailing `0xFF`.
- Reproduce the carve/extract: `dump/dump.sh` + `binwalk`, `unsquashfs`.

### Flash layout (from binwalk)

| Offset    | Size    | Contents |
|-----------|---------|----------|
| 0x000000  | 384 KB  | U-Boot (Ingenic SPL `55aa` + main) |
| 0x060000  | ~1.6 MB | uImage — Linux **3.10.14 `__isvp_swan_1.0__`**, MIPS32, LZMA, 2023-04-07 |
| 0x200000  | 5.37 MB | SquashFS 4.0 (xz) rootfs |
| 0x740000  | 754 KB  | JFFS2 (writable config/data) |
| ~0x7F8000 | —       | erased `0xFF` |

### Rootfs highlights

- Vendor stack: **ZRT / ISVP Swan** T31 firmware. Main app `/usr/bin/zrt_app`,
  supervisor `/usr/bin/Daemon_app`, factory `/usr/bin/Production2Daemon.sh`.
- `root` login = `/bin/sh`; shadow hash `$6$GupOzpfi$…` (SHA-512 crypt).
  **CRACKED → root password = `puwell007`** (john, custom vendor wordlist +
  Jumbo rules, ~30 s). Same hash on BOTH boards → one password for the unit,
  and almost certainly the **vendor default across the Puwell/IPC365 line**.
  Use on the gun's `R/T/G` serial console (115200 8N1) for a root shell.
- **Previous owner's home Wi-Fi is in the ball's config partition in cleartext**
  (SSID + PSK + IPC365 user id). REDACTED here on purpose — the values live only
  in the gitignored `dump/analysis/boardA_wifi/config_fs/cfg.txt` and are
  re-derivable from `p25q64_GOOD.bin`. (Device does not wipe on resale.)

## Interfaces / access — TODO

- [ ] **Crack the root hash** (`hashcat -m 1800`) — likely a weak vendor default.
- [ ] Grep rootfs for cloud endpoints, hardcoded keys, telnet/UART enable flags
      (`zrt_app`, `set_network.sh`, `/etc/init.d`, `Production2Daemon.sh`).
- [ ] (optional) Confirming 2nd full flash read once the clip is reseated — nice
      to have; CRC proof already covers kernel+rootfs.
- [ ] Find UART console. T31 typically exposes 3.3 V UART @ **115200 8N1**.
      Look for a 3–4 pad group near the SoC (labeled U1/U2 rows in `IMG_8025`).
      Cross-ref `check_ttyUSB2_on.sh` from rootfs. **Confirm 3.3 V before probing.**
- [ ] Identify sensor on the ribbon cable (likely SC-series / GC-series CMOS).
- [ ] Map the daughterboard connector + JieLi chip role.

## Log

### 2026-07-19
- Project created from 13 teardown photos (`photos/IMG_8024–8036.jpg`,
  converted from HEIC).
- IDed the SoC (Ingenic T31L), Wi-Fi (Hi3861), SPI flash (PUYA P25Q64SH 8 MB),
  and audio amps (8002A ×2). Logged uncertain motor/power ICs in `chips.md`.
- Read the asset label → model **ZA-816S-4-W**, MAC `40:6A:8E:C8:F7:52`.
- Confirmed **5 V micro-USB** power input.
- **Dumped the SPI flash** (CH341A + flashrom, in-circuit). One clean read of a
  flaky clip; CRC-verified kernel + rootfs. Image `dump/p25q64_GOOD.bin`.
  Firmware = Ingenic ISVP Swan / ZRT stack, Linux 3.10.14, built 2023-04-07.
  Root shadow hash recovered for offline cracking.
- **Diffed both boards + subagent analysis → gun-ball architecture** (`comms.md`):
  USB-RNDIS link, ball master / gun slave, TCP ports 12347+12351.
- **Environment dig:** MTD map from U-Boot bootargs; gun `/config` empty (ball
  pushes config each boot); no static keys (AWS creds are runtime STS from
  IPC365); ball `cfg.txt`.
- **Cracked root password: `puwell007`** (shared across both boards).
- No power-on / live serial attempted yet.
