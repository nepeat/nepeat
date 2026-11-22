# upstream U-Boot on the AS7772-32X

**State (2026-11-21): upstream U-Boot 2026.10 (`#uboot-as7772-nor`, sha256 `21718a87…`, crc32 `8883619c`) is in
boot NOR (mtd7).** It is the AS7760 port ([../edge-core-7760-32x/u-boot-port.md](../edge-core-7760-32x/u-boot-port.md))
plus two patches. Build: `nix build path:./nix#uboot-as7772-nor` / `#uboot-as7772-chainload`; series and scopes in
[nix/packages/uboot/README.md](../../nix/packages/uboot/README.md).

## what differs from the AS7760
- **0034** (scope `as7772`): board name, own control tree (`as7772-32x.dts` = the AS7760 tree + model), and the DDR
  setup of this board's factory loader ("Prob-E" in its banner; the AS7760 prints "Prob-D"): `SDRAM_CFG[HSE]` clear
  (live `0xe7048000`, AS7760 `0xe7048008`), `wrlvl_cntl_2/3 = 0x0608060e / 0x0a0c0b09`
  (AS7760 `0x0608060d / 0x090c0a09`).
- **0035** (scope `fsl-sata`): `fsl_sata` binds a `sata_bootdev`, so `bootflow scan` finds the SSD. Generic; the
  AS7760 builds don't take it (no disk there).
- Kconfig: SATA on (`FSL_SATA_V2`, the AS7760 turns it off), `EFI_PARTITION` on. USB is still off.
- Unchanged from the AS7760: no RDB CPLD (`md.b ffdf0000` = 0xff, so **no alt bank**), VID off, 100 MHz SYSCLK,
  SPD 0x52/0x53, mgmt = DTSEC1 SGMII + BCM54616S at MDIO 0x1f, FMan ucode from NOR mtd5, env in NOR mtd6
  (factory format, read unchanged).
- The as7760 builds are untouched: same drvPaths before and after (`hdcx4faq…` nor, `6zk9kpff…` chainload).

## evidence
- Factory prompt inventory (bdinfo, flinfo, I2C probes, DDR/LAW/IFC/SerDes/SFP registers):
  [factory-uboot-inventory-20261121.txt](factory-uboot-inventory-20261121.txt). Secure boot: SFP OSPR = 0 (ITS
  clear), SRK hash all zero, SVR `0x85300011` (non-E part). Of 332 registers also captured on the AS7760, only
  `sdram_cfg` and DDR training results differ (plus two GPIO data bits).
- RAM chainload from the factory prompt (`tftpboot 0x200000`, `go`): banner, 16 GiB sized, FMan ucode, ping,
  SATA disk + GPT, and **`ddrcmp`: 50 computed registers, 0 differ from the live factory setup**. Then the factory
  env's `onie_bootcmd` booted ONIE through it (8 CPUs, both `14e4:b930` functions, no machine checks).
- Flash, from the factory prompt, the AS7760 sequence: new image and the verified `dumps/mtd7.bin` both TFTPed and
  CRC-checked in RAM, `cmp.b eff40000` against the backup (proves the address map), then
  `protect off; erase eff40000 efffffff; cp.b; cmp.b; crc32; protect on`. Transcript:
  [flash-uboot-20261121.txt](flash-uboot-20261121.txt).
- After the flash: cold power cycle boots it (DDR trained from SPD by our build), `ddrcmp` 0 differences,
  2 x 16 MiB pattern fill/compare at 0x10000000 and 0x60000000 clean, warm `reset` and unattended autoboot reach
  ONIE through the factory `bootcmd`. Console: `serial-20261121-*-factory-uboot.txt` (git-ignored).
- `nix build path:./nix#uboot-tests` covers both as7772 images (ELF/e6500, entry, NOR size + reset vector, config).

## recovery
- A working prompt: `tftpboot 0x1000000 <mtd7.bin>; protect off eff40000 efffffff; erase eff40000 efffffff;
  cp.b 1000000 eff40000 c0000; cmp.b ...; protect on eff40000 efffffff` restores the factory loader
  (`dumps/mtd7.bin`, sha256 `34478cb2…`, crc32 `f15731c5`). Stop autoboot with any key (factory loader: `123`).
- No prompt: there is no alternate bank. The NOR is parallel CFI (0x89/0x227E2801, TSOP-56 class); write mtd7 at
  NOR offset 0x7F40000 by JTAG/COP or desolder. No JTAG header has been looked for on this unit.

## open
- USB (off, unidentified), `cpld`/board reset path untested beyond `reset`, TLV MAC from `hw-info` (the MAC comes
  from the factory env today), cold boot repeated only twice so far.
