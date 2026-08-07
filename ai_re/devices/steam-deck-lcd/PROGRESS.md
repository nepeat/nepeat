# Steam Deck LCD (Jupiter)

## Identity
- Make / model: Valve Steam Deck LCD (original, "Jupiter"), NOT the OLED ("Galileo") — chip/board differs between revisions
- Part numbers: TBD (record board rev once visually confirmed)
- Chip markings (MCU, flash, PMIC, radio): SPI BIOS flash believed to be **Winbond W25Q128JW**, 128Mbit / 16MB, SOIC-8 208-mil package — per iFixit "Steam Deck Chip ID" guide, corroborated by stanto.com and Quarkslab writeup. Confirm by reading the actual chip marking before clipping.
- Photos / teardown refs: iFixit "Steam Deck Chip ID" (https://www.ifixit.com/Guide/Steam+Deck+Chip+ID/147811)

## Power
- Input voltage: N/A for this repair (board fully unpowered / battery disconnected during clip flash)
- Logic level: **1.8V (VCC 1.7–1.95V)** per Winbond W25Q128JW datasheet Rev G — ⚠️ NOT 3.3V. A stock CH341A clip kit supplies 3.3V by default and will over-volt this chip. Need a 1.8V adapter/level-shifter board (commonly sold alongside CH341A kits for this exact class of chip — same issue seen on some AM4 BIOS chips).

## Interfaces
- Serial: not applicable to this task
- JTAG / SWD: not applicable to this task
- SPI / flash chip: W25Q128JW, SOIC-8, 1.8V — accessible via SOIC-8 pogo/spring clip without full board disassembly per community sources (exact siting not yet visually confirmed on this unit)
- USB: Steam Deck has a built-in Insyde "Crisis Mode" recovery path (USB + renamed `.fd` + h2offt) that's software-only and worth trying FIRST if the board still responds at all — cheaper/safer than clip flashing. Only fall back to the hardware clip if Crisis Mode doesn't recover it.
  - USB drive prep: FAT32, MBR partition table, exactly ONE file at the drive root (no subfolders, no other files/drives attached)
  - File: download `F7A0133_sign.fd` from evlaV GitLab mirror (`gitlab.com/evlaV/jupiter-hw-support` → `usr/share/jupiter_bios/`), rename to exactly `F7ARecovery.fd` (rename required — distributed filename won't trigger crisis mode). OLED equivalent is `F7GRecovery.fd`, not applicable here.
  - Trigger: Deck fully powered off, plugged into mains/dock power (not battery alone), USB inserted. Hold **Volume Down + "···" (Quick Access) button**, then press **Power** once. Different from the normal Volume Down + Power boot-to-BIOS-update combo — the extra "···" invokes true crisis mode. Power LED flashes continuously while working; don't unplug power or USB until it stops on its own.
  - ⚠️ Some newer LCD BIOS versions reportedly drop into battery-storage mode (powers off) on first trigger attempt — if nothing happens, retry with power connected.
  - Sources: stanto.com Crisis Mode guide, SteamDeck-BIOS-Manager GitHub script (confirms rename target), evlaV GitLab live directory listing (confirms current filename).
- Other: AMD Van Gogh APU — uses AMD's Embedded Firmware Structure / PSP directory table, NOT an Intel Flash Descriptor. Do not use flashrom `--ifd` region flags (Intel-specific); do plain full-chip read/write.

## Firmware / dumps
- Location of dumps (kept in this subdir): none yet — **dump the corrupted chip FIRST before writing anything new**
- Sizes / hashes: expect full dump to be exactly 16,777,216 bytes (16MB) if chip is confirmed W25Q128JW — verify actual chip ID/size via flashrom probe before trusting this
- Known community image source: evlaV GitLab mirror (gitlab.com/evlaV/jupiter-hw-support/-/tree/master/usr/share/jupiter_bios) hosts signed `.fd` BIOS files (e.g. `F7A0115_sign.fd`) sourced from SteamOS's own package repo — considered reputable (long-standing, widely cited), though not an official Valve-branded host. `SteamDeck-BIOS-Manager` (github.com/ryanrudolfoba/SteamDeck-BIOS-Manager) automates fetching from there.
- ⚠️ Unconfirmed whether these `.fd` files are full raw 16MB chip images or a BIOS-region-only payload meant only for `h2offt` at a specific offset — verify file size before writing wholesale via flashrom.
- ⚠️ **DMI transplant gotcha**: the corrupted chip's dump contains unit-specific DMI data (serial number, controller ID). Writing a generic community image without transplanting this from the original dump (via hex editing) will lose device-specific identity data. This is why dumping the bad chip first is mandatory even though it's "bad."

## Log (dated entries — newest first)
- 2026-07-19: Incident — failed BIOS/firmware update bricked the board (no boot). Model confirmed LCD/Jupiter. No existing BIOS backup on hand. Researched chip identity, voltage, recovery procedure, and image sourcing (see above). Plan: (1) try Insyde Crisis Mode via USB first if board responds at all, (2) if not, dump chip in-circuit via SOIC-8 clip + CH341A **with 1.8V adapter** before any write, (3) verify dump size/chip ID, (4) source community stock image from evlaV mirror, (5) hex-transplant DMI block from original dump into clean image, (6) write back, (7) verify by reading back and diffing. Not yet attempted — waiting on clip connection from user.
