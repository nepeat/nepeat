# U118 SPI Flash Analysis — Cisco Meraki MX250 (board "monsters" / x86-gen2)

**Target:** `u118-dump1.bin` (8 MiB, Winbond W25Q64JV read from U118). Verified identical second copy `u118-dump2.bin`.
**Date:** 2026-07-19. All offsets are file offsets; all claims verified against bytes.

## TL;DR verdict

This chip is the **dedicated configuration flash for the platform's secure-boot / Cisco Trust Anchor (TAm) FPGA** — *not* the x86 host BIOS/coreboot flash. It contains a tiny plaintext **directory at offset 0 that points to two byte-identical ~664 KiB encrypted images** (classic FPGA golden+primary dual-boot layout), a second byte-identical ~75 KiB encrypted image pair, and a sector-based writable data store. **Every payload region is encrypted** (near-maximal entropy, no plaintext, no vendor bitstream header, no certificates/keys). This is a **Cisco-custom, hardened TAm image, not a stock/off-the-shelf vendor FPGA image.**

---

## 1. Context confirmation (from `bootlog.txt`)

The boot log firmly ties this device to a Cisco Trust Anchor FPGA:
- `FPGA: v0026`, `SecureBoot: R03...`, `SB Core: F01113R18...`, `Microloader: MA1011R06...`, `----SecureBoot Registers----`
- Kernel: `gpio_meraki_fpga: loading out-of-tree module`, `Starting ciscotamservices`
- `board x86-gen2`, `MERAKI_BOARD=monsters`, `Cisco Monsters/Monsters, BIOS meraki-monsters 08/13/2021`, MX250 `600-56020`.

`ciscotamservices` + `gpio_meraki_fpga` = the userspace/kernel side of the Cisco Trust Anchor module whose FPGA config this flash holds.

## 2. Occupancy / region map (4 KiB granularity)

~80% of the chip is erased (0xFF). Real data = **1,677,577 bytes**. Confirmed non-FF regions and their entropy (entropy computed over the region incl. FF padding):

| Region | Len | nonFF | Entropy | Nature |
|---|---|---|---|---|
| 0x000000–0x014000 | 0x14000 | 74,229 | 7.69 | directory (0x0) + encrypted blob |
| 0x121000–0x1a3000 (sparse) | — | ~100 K | 1.5–3.9* | **writable data store**: small blobs, 1 per 4K sector |
| 0x200000–0x201000 | 0x1000 | 2,472 | 5.75 | lower-entropy header/manifest |
| **0x210000–0x223000** | 0x13000 | 75,022 | 7.92 | **encrypted image (copy A)** |
| **0x280000–0x293000** | 0x13000 | 75,022 | 7.92 | **encrypted image (copy B, identical to A)** |
| **0x301000–0x3a7000** | 0xa6000 | 675,944 | 8.00 | **encrypted image (copy A, 679,936 B)** |
| **0x3cf000–0x475000** | 0xa6000 | 675,944 | 8.00 | **encrypted image (copy B, identical to A)** |

\* Low region-entropy numbers here are an artifact of mostly-FF sectors; the actual data bytes in each sector are high-entropy.

## 3. The offset-0 directory (CONFIRMED)

First 12 bytes: `00 10 30 00 26 00 00 f0 3c 00 26 00`, then 0xFF.
Decoded as **6-byte entries = [3-byte LE address][3-byte LE value]**:

```
entry0: addr=0x301000  val=0x002600     -> BIG image A base
entry1: addr=0x3cf000  val=0x002600     -> BIG image B base
(remaining entries = 0xFFFFFF, empty)
```

Both addresses **exactly match** the two large image bases. This is the standard **FPGA dual-boot / multi-boot flash directory** pattern (a small jump/pointer table at flash address 0 selecting golden vs primary configuration images). The shared `val=0x2600` plausibly encodes a version/tag; note `FPGA: v0026` (0x26) in the boot log — offered as hypothesis, not proven.

## 4. The image payloads are ENCRYPTED (CONFIRMED)

- **0x301000 vs 0x3cf000:** byte-identical over the full 0xa6000 (`==` True). Same for **0x210000 vs 0x280000** (0x13000, `==` True). ⇒ redundant A/B copies, not two different images.
- **Entropy:** big image = **7.998 bits/byte**; excluding 0x00/0xFF the 254 remaining byte values are statistically uniform (per-value counts 2529–2764 around mean 2622, ~±5%). This is indistinguishable from random ⇒ **encrypted** (or encrypted+compressed).
- **No ECB structure:** the only repeated 16-byte ciphertext blocks are all-00 / all-FF padding; no repeated data blocks ⇒ not naive ECB; consistent with CBC/CTR/whitening.
- Both images begin with ~0x30 high-entropy bytes, a 0x120-byte zero gap, then continuous high-entropy payload — i.e. a small (~48 B) header/IV/tag block, padding, then the encrypted body.

## 5. No stock FPGA bitstream signature, no plaintext, no keys (CONFIRMED negatives)

Searched the whole file:
- **No Xilinx** sync `AA 99 55 66` (0), no bus-width `00 00 00 BB 11 22 00 44` (0).
- **No Altera/Intel** RBF pattern.
- **No valid Lattice** ECP5/MachXO header: no `FF FF BD B3` preamble (0), no `LSCC` comment, no `Lattice` ASCII. The 38 stray `BD B3` bytes are inside high-entropy data and statistically consistent with random noise (expect ~25 over 1.6 MB).
- **No X.509/DER:** the 21 `30 82` hits are all junk — declared lengths absurd (e.g. 28,838 / 63,393 bytes) and the following bytes are not `30/A0/02` as a real certificate requires.
- **No ASCII of interest anywhere:** no `Cisco`, `Meraki`, `x86-gen2`, `whitelist`, `SECP384`, `TAM`, `Lattice`, no PEM. `strings` yields only random short printable runs (typical of high-entropy data).

A **stock** vendor flashing (e.g. Lattice/Xilinx via Diamond/Vivado) normally leaves a **plaintext bitstream preamble/comment** at the image start. Its complete absence here, combined with full-entropy payloads and the custom directory format, indicates a **Cisco-proprietary encrypted container**, not a generic vendor image.

## 6. Writable data store — 0x120000–0x1a3000 (structure)

Many small high-entropy blobs (~256–765 bytes), each isolated in its own 4 KiB sector, several with tiny little-endian headers, e.g.:
- 0x181000: `01 00 00 00 01 00 00 00` → (1, 1)
- 0x191000: `20 2C 00 00 16 00 00 00` → (0x2C20=11296, 22)
- 0x194000: `13 2C 00 00 15 00 00 00` → (0x2C13=11283, 21)

The (large counter, small index) shape and per-sector isolation are consistent with a **TAm secure-variable / key-blob / monotonic-counter (anti-rollback) store** that is rewritten sector-by-sector at runtime. Payloads are encrypted/opaque. (Interpretation — hypothesis grounded in the header shapes.)

## 7. Relation to thrangrycat / CVE-2019-1649

CVE-2019-1649 ("thrangrycat") was that the Cisco TAm **FPGA bitstream in SPI flash could be modified** because the stored bitstream was insufficiently protected/authenticated. This image tells a different story:

- The FPGA config is stored **encrypted** (full entropy, no plaintext bitstream), as **dual redundant images** selected by a directory — a **hardened** design.
- This is a **2021** platform (`BIOS meraki-monsters 08/13/2021`), i.e. post-thrangrycat era.

⇒ **Hypothesis:** this is Cisco's remediated/encrypted TAm configuration, materially harder to tamper with than the vulnerable class. Confirming would require the FPGA family and the on-die decryption key, neither of which is present in flash (as expected — the key lives in the FPGA/silicon, not in this SPI part).

## 8. Open questions / what would advance this

- **FPGA family:** unproven. Size (~664 KiB primary image) is in Lattice ECP5 / large-MachXO3 territory, and Cisco TAm is historically Lattice, but there is **no in-flash evidence** of vendor — consistent with encryption stripping the plaintext header. (Hypothesis only.)
- The ~48-byte per-image header (offset 0 of each image) is the best lead for identifying the container/crypto format; worth diffing image A's header against the 75 KiB pair's header.
- The `u15-dump*.bin` (4 MiB) present in this directory is a separate part and was out of scope here; it likely holds a different function (possibly the coreboot/BIOS or a second FPGA), given U118 shows zero x86 firmware strings.

## Evidence appendix (key raw bytes)

```
0x000000: 00 10 30 00 26 00 00 f0 3c 00 26 00 ff ff ff ff   (directory)
0x301000: 2a 63 83 75 e0 80 32 af 76 20 ec 0e 13 10 f4 47   (BIG image A start)
0x3cf000: 2a 63 83 75 e0 80 32 af 76 20 ec 0e 13 10 f4 47   (BIG image B = identical)
0x210000: 57 c4 a1 eb 0a 07 c3 9c bc e5 07 8d 21 f2 10 59   (MID image A start)
0x280000: 57 c4 a1 eb 0a 07 c3 9c bc e5 07 8d 21 f2 10 59   (MID image B = identical)
```
