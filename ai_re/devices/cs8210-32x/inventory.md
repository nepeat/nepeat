# CS8210 firmware and flash inventory

What every programmable part runs, where a copy is, how it updates, and what is still out of reach.
Built 2026-11-15 from the factory images (SSD DIAGOS 2026-10-17, ONIE in SPI, the SSD's ONIE updater,
the SDK chroot) and read-only live state on NixOS. No I2C, no `/dev/mem`, no SDK run.
Tags: **[hw]** live read, **[img]** found in an image, **[inf]** inferred.

- Copies are local only (`analysis/`, `images/` are git-ignored); hashes for all of them:
  `analysis/inventory/SHA256SUMS`. Live captures: `analysis/inventory/live-20261115/` (TPM, lspci, dmesg, mtd, sda, SDK file list).
- Paths below: `fw/` = `analysis/extracted/root/fw/` (SSD `/root/fw`, the same files as the ONIE updater's
  payload, `analysis/research/onie/update/`); `sdk7/` = `analysis/onie-live-2026-11-10/sdk/R3181-J0001-01_V0.1.7_Briggs_SDK/`
  (byte-identical to the switch's `/var/lib/cs8210-sdk/root` for every file named here [hw]).
- Version sources: vendor `get_versions` / `install-platform` / `fw.cfg` (ONIE updater, SSD `/root/fw`).

## Programmable parts

| component | running | best copy (sha256) | updates via | status |
| --- | --- | --- | --- | --- |
| Boot chain (stage2/3, U-Boot, Alpine SerDes + SBus FW, DT) | al-boot v3.65.1, U-Boot …015 (Jun 2021) | `images/spi-2026-11-15/mtd0-uboot.bin` `a22ff482…` ([boot-chain.md](boot-chain.md)) | SPI flash | **have**. SSD `celestica_cs8210-boot.img` `6ba51e2a…` is older (ONIE 2017.11.011, Apr 2021) |
| ONIE | 2017.11.015 (FIT Jun 4 2021) | `mtd4-onie.bin` `63c70b99…` | `onie-self-update` | **have**. SSD FIT `610b0fa4…` / updater `ee486e57…` are 2017.11.011 |
| K2X uC (SoC-card microcontroller) | bootloader 2.28, app 2.1.14 [hw, U-Boot `uc_get_version`] | **none** | `al_tool -s local uc -c update\|bootloader_update -i FILE` (= `onie-fw-update -t uc-app\|uc-bl`); U-Boot README: update `uc-app.bin` before U-Boot | **missing**, see below |
| System CPLD "SYSFPGA" U51 | 0x15 [hw] | `fw/briggs_sysfpga_v13_0330.vme` `684ab604…` (v0x13, 2021-03-30) | `vmetool_arm -s` | **running is newer than any copy** (v15 missing) |
| Switch LED CPLD U27 | 2 [hw] | `fw/SW_LED_CPLD_TOP_v01.vme` `091e9f5e…` (v1, 2020-08-21) | `vmetool_arm -l` | **running newer** (v2 missing) |
| MACsec LED CPLD U26 | 1 [hw] | `fw/MACsec_LED_CPLD_TOP_v01.vme` `a2658a23…` | `vmetool_arm -ml` | version matches; byte equality unproven |
| Fan CPLD U9, top + bottom | 3 / 3 [hw] | `fw/FENGHUANG_CPLD_FAN_impl1_V03.vme` `dfefbf9f…` (2019-04-22), one file for both | `vmetool_arm -f1` / `-f2` | version matches; byte equality unproven |
| MDIO FPGA U1 (`1d0f:ec30`, subsys `1204:3010` Lattice) | 0x1008 [hw, stock dmesg + mainline BAR0] | `fw/mdio_fpga_v1001_0210.vme` `4abb2f85…` (0x1001, 2021-02-10); `fw/mdio_fpga_top_v07.vme` `0057645b…` (R0A boards) | `vmetool_arm -md` | **running newer** (0x1008 missing) |
| I2C FPGA (`1d0f:8571`, XC7A75T, IDCODE 0x03632093) | update image at 0x400000 of its flash; version reg 0x010b0009 [hw] | `images/spi-2026-11-09/mtd5-i2cfpga.bin` `fa559a02…` (golden + update) | `flashcp` to `/dev/mtd5` | **have** ([fpga/](fpga/README.md)). Shipped `fw/i2c_fpga_v01.02.0E_20200604.bin` `3b8c6c73…` is older |
| Lattice ASC10 x4 (0x60..0x63, rail monitors) | unknown: CRC read needs I2C | `fw/Phoenix_ASC{0,1,2,2_R0A,3}.hex` (258 B each); `fw.cfg` target CRCs 016f / 5372 / – / bef6 | `asc_fwupd_arm -w` (I2C, into the ASC EEPROM) | copies have; live match unknown |
| TPM Infineon SLB9670 | FW **7.85.4555.0** (`FIRMWARE_VERSION_1/2` 0x00070055 / 0x0011CB00), TPM 2.0 rev 1.38, FIPS mode [hw] | firmware: none (not readable by design). EK certs: `live-20261115/tpm/ek-{rsa,ecc}.der` `5b88dc6a…` / `9726ab3f…` | Infineon field upgrade (vendor command; no image anywhere) | **identity complete**, see below |
| Credo CMS50216 MACsec PHY x16 | 01.92.02 (date 0x4914, CRC 0xa0b3) [hw, SDK log] | `sdk7/c-phy/B52b0.lz4.1.92.2.bin` `203537de…` | RAM only, loaded over MDIO by `phy_server` every boot | **have**. Unused alternates: `c-phy/release_B52b0.lz4.fw.1.92.1.bin`, `phy/{B52,B52b0,Owla0,Owlb0}.fw.*.bin` |
| Teralynx 7 SerDes | eth FW `0x10A5_208D_003` (32765 words, "RING-A FW size"), SBus master `0x1024_2001` (rev 0x10240001) [hw, ifcs log 11-10] | embedded in `sdk7/libifcs.so` `388031e7…` as `eth_serdes_*_hex_code` / `sbm_serdes_*_hex_code` (10 eth + 2 SBM revisions; same file in SDK 0.1.2 and the chroot) | RAM only, `node_serdes_*_fw_download` at SDK start | **have**. The ASIC has no flash of its own: none in DT, diag YAML or SDK |
| SSD WD Ultrastar SA210 | M6102100 [hw, udev `ID_REVISION`] | none | ATA DOWNLOAD MICROCODE (supported) | **unobtainable**: vendor image only, no ATA readback |
| PSU x4 slots (`celestica,dps_1500` @0x59, driver `psu_dps800`), PSU1 fitted | unknown | none | none in any image | PMBus `MFR_REVISION` read needs I2C; MCU FW unobtainable |
| Regulators TPS53688 x7 (0x62/0x66/0x68 switch board; MACsec card 0x68/0x77/0x71/0x72) | NVM config | none | PMBus (U-Boot writes ASIC VOUT, [vendor-uboot.md](vendor-uboot.md) item 5) | config readable over PMBus only |
| Clocks TI LMK03328 @0x54, LMK05318 @0x64 (MACsec card) | on-chip EEPROM config | none | I2C | readable over I2C only |
| Retimers | none fitted that matter: DT `retimer@0/1` "ds25" at root bus 0x18/0x19 serve only the disabled al_eth ports 0/2/4/5 (reference-board leftover); no driver bound | – | – | presence unverified; DS250-class parts have no firmware [inf]. The Credo PHYs act as the retimers (MACsec bypass) |

Part identities from the `.vme` IDCODE checks (SIR 0xE0, TDO bit-reversed), names from openFPGALoader's
`part.hpp`: sys CPLD (and `xfr.vme`) 0x612BD043 LCMXO3LF-6900C; SWLED and MACLED 0xE12BC043
LCMXO3LF-2100C; fan 0xE12BB043 LCMXO3LF-1300C. All Diamond Deployment Tool 3.10/3.11 VME 12.1, compressed.
The MDIO FPGA `.vme` has no IDCODE check: it sends IR 0x3A + DR 0x68FE (Lattice background-SPI access,
the non-ECP3 path in openFPGALoader) and then SPI flash traffic, so U1 is an ECP5-class part [inf: Gen1 x1
PCIe hard IP, likely LFE5UM] whose bitstream sits in a SPI flash behind it, on no SoC bus.

## Flash / EEPROM devices on the board

| device | where | contents |
| --- | --- | --- |
| SPI NOR mx25u25635f 32 MiB | spi0 CS0 (boot) | dumped twice (`images/spi-2026-11-*`); env differs only |
| SPI NOR n25q256a (DT: mt25ql256) 32 MiB, I2C FPGA config | spi_aux CS1, behind the PBS52/54 mux | dumped from ONIE 2026-11-09 (= mtd5). Reads all ones on mainline ("aux SPI flash" in PROGRESS is this chip) |
| MDIO FPGA config SPI flash | behind U1 only | not reachable from the SoC; JTAG via the FPGA |
| MachXO3LF internal flash x5 (CFG + UFM) | sys, SWLED, MACLED, 2x fan | older `.vme` copies only for sys/SWLED |
| ASC10 internal EEPROM x4 | I2C | `.hex` copies; live CRC unread |
| uC internal flash (bootloader, app, FRU) | K2X SoC card | nothing; FRU content once seen as `root/diag/fru.log` (K2X-N, B0) |
| TPM NV | SLB9670 | only 0x1C00002 (RSA EK cert, 1171 B) and 0x1C0000A (ECC EK cert, 775 B), `platformcreate`, `ppwrite`, `ownerread`, `authread`, `no_da`; no persistent handles; owner/endorsement/lockout auth unset; lockout counter 0 [hw] |
| I2C EEPROMs 24c64 | root 0x57 (K2X card, blank), switch board 0x56 (ONIE TlvInfo, printed by U-Boot/ONIE), I2C-FPGA card 0x56, busbar 0x54, MACsec card 0x56, 2 fan boards 0x56, 14 fan modules 0x50, riser 0x50 | **not dumped** (I2C); locations in `analysis/celestica_cs8210-r0.dts`, [board-control.md](board-control.md) |
| I2C EEPROMs 24c02 | PSU FRU 0x51 x4, DDR SPD 0x52/0x53 | not dumped (I2C) |
| PMBus/clock NVM | TPS53688 x7, LMK03328, LMK05318 | not read |
| SATA SSD 120 GB | ata16 | imaged 2026-10-17 |
| NAND | DT node disabled, U-Boot `NAND: 0 MiB` | none |

## Out of reach, and why

- **uC firmware.** No `uc-app.bin`/bootloader file in the SSD (all 41k p3 files), the SSD's ONIE updater, the
  ONIE initramfs in flash, the SDK chroot or the SPI dumps. `al_tool` (ONIE copy `1fea61fb…` stripped; SSD
  `/usr/bin/al_tool` `8f706bb1…`, same BuildID, **unstripped with debug info**, Annapurna job
  `k2_v3_fw_and_tools_release_k2x`) only writes images (`update`, `bootloader_update`, `spi_preloader_burn`);
  no command reads the app back. Needs the vendor release package or the uC's own debug port (MCU type unknown:
  no board photo of the SoC card).
- **CPLD/FPGA images newer than our copies** (sys v15, SWLED v2, MDIO 0x1008): the Jun 2021 ONIE that put
  them there left no payload (its initramfs carries tools only). A JTAG readback is possible in principle from
  the SoC itself (`vme` bit-bangs `/dev/gpiochip3` after an I2C write of sys CPLD 0x7e: 0x00 sys, 0x10 SWLED,
  0x12 MDIO FPGA + MACLED (one chain, MDIO first: the MACLED file has a 1-device header), 0x04/0x0c fan, 0x20 idle),
  but that is a chip read and needs I2C writes: not done. Read-protect state unknown.
- **PSU, regulator, clock, ASC, FRU EEPROM contents**: all behind the I2C tree the fan service drives; parked.
- **SSD and TPM firmware**: not readable by design; only versions.

## Risks found

- **Never run the SSD's ONIE updater** (`onie-updater-arm64-celestica_cs8210-r0`, `installer/install-platform`
  `cel_update_fw`): it reflashes whatever differs from `fw.cfg`, which here means **downgrading** sys CPLD 15→13,
  SWLED 2→1, MDIO FPGA 0x1008→0x1001, U-Boot and ONIE .015→.011, rewriting mtd5 if its version string differs,
  and ASC EEPROMs if their CRCs differ, then `xfr.vme` (IDCODE + LSC_REFRESH 0x79) reloads the sys CPLD live.
- U-Boot `uc_sdr_set_fw_update` writes uC state (DMC-12 table: high risk, never).

## Leads

- Sys CPLD 0xa1 "I2C FPGA Misc IO" (diag default 0xd3, mask 0xf0): the updater writes 0xd7 after `flashcp`
  to mtd5 with the comment "switch spi mux back to fpga". So bit 2 likely gives the n25q256a to the FPGA
  [inf]. Nothing in `install.sh` or the U-Boot env writes it, so it probably does not explain the
  stock/mainline difference, but one read settles whether the flash is SoC-side when mainline sees all ones.
- V2 vs V3: DMC-12 uC 1.13/1.35.0 vs K2X 2.28/2.1.14 (both NOT-SECURE); DMC-12 CPLD update is `cpldupd` on one
  GPIO line, CS8210 is a JTAG mux in the sys CPLD plus a GPIO-chip bit-banger ([DMC-12 vendor-tooling.md](../dmc-12-48r-as0/vendor-tooling.md)).
