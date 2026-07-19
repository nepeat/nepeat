# MS320-48 — hardware flash access (SPI NOR)

Photos: `IMG_8011.HEIC` (whole board) / `IMG_8012.HEIC` (close-up of the NOR). Board PN 630-21060, CISCO 560-021050,
MAC 00:18:0A:CC:52:D0 (matches our unit).

## The chip to clip: U10 — Macronix MX25L128 SPI NOR (16 MiB boot flash)
- Silkscreen ref **`U10`**, marking `MX25L128 45E...` (Macronix MXIC). = the `mx25l12805d`/`m25p80` the bootlog reports.
- This is the **16 MiB SPI NOR** holding `loader1/boot1/loader2/boot2/...` — the target for reading/patching the boot
  chain. (The Micron NAND in `IMG_8011` bottom-right is the 128 MiB rootfs store — different chip, not this one.)
- flashrom detects it by JEDEC id `C2 2018` (family MX25L12805D/33F/35F/45E). `flashrom -p ch341a_spi` lists it.

## ⚠️ Package correction: it's 16-SOP, NOT SOIC-8
Earlier notes assumed SOIC-8. The photo clearly shows a **16-lead SOP (SO16, 300 mil)** — pins 1–8 left, 16–9 right.
**You need a SOP-16 / SOIC-16 test clip** (e.g. Pomona 5252, or a cheap 16-pin SOIC clip), not the SOIC-8 clip. The
CH341A's 8-pin ZIF socket won't seat a 16-pin part — wire the clip's flying leads to the CH341A SPI signals.

## Verified 16-SOP pinout (from Macronix MX25L12845G datasheet v1.8)
| Pin | Signal | Pin | Signal |
|----:|--------|----:|--------|
| 1 | NC/SIO3 | 16 | SCLK |
| 2 | **VCC (3.3V)** | 15 | SI/SIO0 (MOSI) |
| 3 | RESET#/SIO3 | 14 | NC |
| 4 | NC | 13 | NC |
| 5 | NC | 12 | NC |
| 6 | NC | 11 | NC |
| 7 | **CS#** | 10 | **GND** |
| 8 | SO/SIO1 (MISO) | 9 | WP#/SIO2 |

## CH341A ↔ clip wiring (8 wires)
| CH341A pin | → MX25L pin | note |
|---|---|---|
| VCC (**3.3V**, not 5V) | 2 | most CH341A boards are 5V by default — must mod/verify 3.3V or you kill the chip |
| GND | 10 | |
| CS  | 7 | |
| CLK | 16 | |
| MOSI (DI) | 15 | |
| MISO (DO) | 8 | |
| (tie high to VCC) | 9  (WP#) | pull high so writes aren't blocked |
| (tie high to VCC) | 3  (RESET#) | datasheet: RESET#/SIO3 must be held high if not driven, else chip stays in reset |

## Gotchas before we read
1. **CH341A must run at 3.3V.** Stock CH341A black boards output 5V on the SPI header — that overvolts a 3V flash.
   Use a 3.3V-modded CH341A, a level shifter, or a 3.3V programmer.
2. **In-circuit contention.** The SoC (VCore-III) shares this SPI bus. Powering only the flash via the clip may
   back-feed the board's 3.3V rail and/or the SoC may fight the bus → bad reads. Try in-circuit first with the board
   unpowered; if reads are unstable/inconsistent, **hold the SoC in reset** or **desolder the NOR** (hot air, SOP-16).
3. **Always take a full read backup first** (`flashrom -r nor-backup.bin`, read twice and diff) before any `-w`.

## Read/write commands (once wired)
```
flashrom -p ch341a_spi -r ms320-nor-backup-1.bin      # backup
flashrom -p ch341a_spi -r ms320-nor-backup-2.bin      # second read
cmp ms320-nor-backup-1.bin ms320-nor-backup-2.bin     # must match → good contact
# ... patch boot1 offline (carve initramfs, swap bootsh→busybox, fix CRC32) ...
flashrom -p ch341a_spi -w ms320-nor-patched.bin
```
