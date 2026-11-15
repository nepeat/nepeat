# SD card identity (probed 2026-11-14, read-only)

**Verdict: a genuine SanDisk SD card at the protocol level** (not eMMC, not
an SD-protocol NAND chip). Whether it sits in a socket or is soldered needs a
physical look; the card-detect pin being wired and asserted points to a real
socket with a detect switch.

| evidence | value | meaning |
| --- | --- | --- |
| kernel init | `mmc0: new high speed SDHC card at address aaaa` | SD init path (ACMD41), RCA 0xaaaa |
| sysfs type | `SD` | eMMC would say `MMC` and expose EXT_CSD instead |
| CID | `035344534130384780b12435c0012400` | MID 0x03 **SanDisk**, OID `SD`, PNM **SA08G**, PRV 8.0, PSN 0xb12435c0, **MDT 04/2018** (board built 11/2018) |
| CSD | structure v2 | SDHC, 15,523,840 sectors = 7.40 GiB |
| SCR | `0235844300000000` | **SD-only register.** SD_SPEC 2 + SPEC3 + SPEC4 + SPECX=1 → Physical Layer **5.x**; 1/4-bit; CMD20/CMD23 |
| SSR | `…0000…04 00 90 0014 05 1a 0a…` | SD_CARD_TYPE 0x0000 = regular R/W card; **Class 10**, AU 4 MiB, **U1**, **V10** |
| OCR | 0x00300000 | 3.2–3.4 V window |
| eSDHC PRSSTAT | 0x0f8d0008: CINS=1 **CDPL=1** WPSPL=1 | card-detect line asserted; write-protect not asserted |
| bus | 4-bit, 50 MHz SD high-speed, 3.3 V | host has no UHS-I 1.8 V switch; measured ~22.6 MiB/s read (U-Boot fatload), ~16 MiB/s write |

SanDisk retail SDHC/microSDHC 8 GB cards use PNM `SA08G`; the V10/U1/C10 grades
fit a 2018 SanDisk Ultra/Industrial-class card. ONIE's factory report also
lists an "SD card" test.

## current contents (2026-11-14)

Original contents (FAT32 with diagnostic logs) are preserved in
`dumps/mmcblk0.img` (SHA256 `71d5a5e6…`, re-verified just before overwrite).
The card now holds the 1-GiB RAM-root NixOS boot image (`#sd-image`): MBR, one
FAT32 `NIXOS_BOOT` partition with uImage, uInitrd (squashfs store), DTB,
boot.scr; the rest of the card is unallocated. Readback SHA256 matched
`203139d2…`.

Linux mounts that FAT **read-only**. Measured writes: 10 sectors on the first
boot (one-time SSH host key), **0** on the next cold boot and after 4 min of
uptime. Journal is volatile, 1 GiB cap. No swap, no fstrim, no Nix store on
the card.
