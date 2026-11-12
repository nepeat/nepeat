# TN48M efuse map (read 2026-11-12)

Read-only, from the factory U-Boot: `fuse read <bank> <word> <cnt>` (Marvell
`drivers/fuse/fuse-mvebu*.c` in u-boot-2018.03-armada-18.12; in this build
`sense` and `override` return -EPERM, and `prog` is the only writer) and
`md.l 0xf06f9000 0x100`. The two methods agree. Raw output:
`efuse-dump.txt`. **Never** run `fuse prog`: it's permanent, and the HD rows
here are already locked.

## banks (factory DT aliases fuse0..4)

| bank | block | where | size |
| --- | --- | --- | --- |
| 0-63 | AP806 HD efuse (`marvell,mvebu-fuse-hd`) | OTP 0xF06F9000, ctrl 0xF06F8008 | 64 rows x 65 bits, 16 B/row: w0 = data[31:0], w1 = data[55:32] + ECC in [31:24], w2 bit0 = lock |
| 64 | AP LD0 "user" (read-only) | OTP 0xF06F8F00 | 256 bits |
| 65 | AP LD1 "proprietary" | same window, ctrl bit6 selects | 256 bits |
| 66 | CP0 LD0 "user" | OTP 0xF2400F00, ctrl 0xF2400008 | 256 bits |
| 67 | CP0 LD1 "proprietary" | same | 256 bits |

## burned HD rows (all locked)

| row | w0 | w1 | meaning |
| --- | --- | --- | --- |
| 0 | `1001b1b5` | `1f03e04c` | factory row (trim/ID?), unknown |
| 25-29 | see below | | **KAK digest**: SHA-256 of the 524-byte KAK field of the boot image's SECURITY extension, 7 bytes per row (w0 LE + low 3 bytes of w1), top byte = ECC |
| 30 | `c0dec0de` | `0a000000` | **flash_id** = header `flash_id` 0xc0dec0de |
| 31 | `de8de800` | `6a000000` | **box_id** = header `box_id` 0xde8de800 |
| 37, 38, 39 | `00000001` | `86000000` | value 1, triplicated: almost certainly the secure-boot enable (inference: no public map, but it matches the BootROM's "Secure boot from SPI NOR") |

KAK digest check (`dumps/mtd0.bin`, KAK field at 0x5c008, 524 bytes):

```
sha256(KAK field) = 624316115d497405d54788939e5b71f3b62054c5497c6754681ce773b552d80b
efuse rows 25-29  = 62431611 5d4974 | 05d54788939e5b | 71f3b62054c549 | 7c6754681ce773 | b552d80b
```

An exact match, so **the SoC trusts exactly the KAK in our flash and nothing else.**
(Hashing just the 294-byte DER, as I first tried in uboot.md, is not what the
BootROM checks; it hashes the whole fixed-size 524-byte field.)

## LD blocks (not decoded)

- AP LD0 (64): `001c2818 3a586f2a 686e2854 000a6a6a 00000000 00000000 00000080 00000000`
- AP LD1 (65): `00000001 8000c000 0...`
- CP LD0 (66): `000340b9 3a584a31 0...`
- CP LD1 (67): `000000d3 00000001 0...`

Probably Marvell production/trim and the AVS work point; the BLE prints
"SVC: Efuse WP: [0]=0x1b". Unknown without Marvell's map.

## what this means

- Secure boot is **fused on and locked**: the KAK digest, box/flash IDs and
  enable rows all have the lock bit set. Any boot image must chain to this KAK
  (Delta/Amazon's private key) through one of the header's CSKs. No public
  BootROM bypass for AP806 is known (see PROGRESS 2026-11-12 (g)).
- The SECURITY extension has `jtag_en=1` with `jtag_delay=20`, so the BootROM
  probably re-opens JTAG after a verified boot. Unexplored. A debugger would
  reach a system we already fully control, so it's low value.
