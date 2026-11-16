# CS8210 vendor U-Boot: what a replacement must reproduce

Static analysis of `U-Boot 2018.05-alpine_db-v3.65.1.015-HAL (Jun 04 2021) - ONIE 2017.11.015`
from the SPI dump, 2026-11-15. Nothing was run on hardware for this file. Inputs:
`images/spi-2026-11-15/mtd0-uboot.bin` (sha256 `a22ff482...1897f6`, identical to the 11-09 dump),
`mtd1-uboot-env.bin` (`f32a7ee3...5d585`), the boot log `serial-consoled-20261115-boot.txt`
(NixOS path) and `evidence/live-onie-2026-11-09/` (ONIE path).
Evidence: **[code]** read in Ghidra, **[hw]** seen in a boot log or live dump, **[inf]** inferred.
Function addresses are link-time (TEXT_BASE 0x01100000); file offset in mtd0 = addr − 0x01100000 + 0x190048.
Scratch (git-ignored): `analysis/vendor-uboot/` (extracted objects, FDT dumps, Ghidra project `ghidra/vub.gpr`,
decompiles `q-*.txt`, scripts). Alpine V2 prior art: [DMC-12 spi-uboot-replacement.md §4](../dmc-12-48r-as0/spi-uboot-replacement.md),
[uboot-vendor-commands.md](../dmc-12-48r-as0/uboot-vendor-commands.md) (command risk table; it applies here too).

## Where it is
- TOC @0x40000 (alboot.py list): 9 entries, one instance each. **UBOOT** (id 5) @0x190000, slot max 0x1f0000,
  header 0x48, payload 0xe8040 (950,336 B), load = exec = 0x01100000. Stage3 log: "Loading application to 01100000
  (950336 bytes)". **Slot budget:** 0x1f0000 − 0x48 − 0xe8040 ≈ 1.03 MiB free; no BOOT_MODE object, no spare app slot.
- Payload: `b reset`, then `.quad 0x01100000` (TEXT_BASE). `.rela.dyn` = file 0xd8458..0xe8028 of the payload
  (2686 R_AARCH64_RELATIVE), so `__image_copy_end` = 0x011d8458; BSS overlaps the rela table. Ghidra input =
  payload with the relocations applied (`uboot-rel.bin`), raw AArch64 at 0x01100000.
- `dram_init` (0x011031e8) caps `gd->ram_size` at **64 MiB**, hence the relocation near 0x03ed1000 [hw: PROGRESS].
  `CONFIG_SYS_BOOTM_LEN` = 0x4000000 (64 MiB, the literal passed in `do_bootm_states`), which is why the
  71 MB mainline Image needs `booti`.
- **Stale objects in the UBOOT slot slack, not in the TOC** (left from an Annapurna reference image, inert while the
  TOC is unchanged): the original al-boot v3.65.1 U-Boot (Dec 2019) at 0x278088; a UBOOT_SCRIPT object (id 6) at
  0x2ce000 holding uImage `init_script` (0x353 B); an IPXE object (id 0x110, iPXE) at 0x2e0000; a BOOT_APP object
  (id 0x200) at 0x360000 holding uImage `test` (arm64 Linux, 0xef2cc B) that runs past mtd0 into mtd1 (after its
  first 16 KiB, which the env overwrote) and fills mtd2 "unused".
- **Env:** single copy, 16 KiB, CRC32 + data (4-byte header, so no `CONFIG_SYS_REDUNDAND_ENVIRONMENT`), at the
  TOC's UBOOT_ENV 0x380000 (= mtd1). The TOC's UBOOT_ENV_RED points at 0x3c0000 (= mtd2), which holds stale code,
  not an env. `board_late_init` exports `env_offset` from the TOC [code 0x01103cd8].

## The four FDTs
| where (mtd0) | size | what | used by |
| --- | --- | --- | --- |
| 0x1ac9c (inside stage2) | 0x192 | overlay-like stub: `board-cfg/gpio_init/gpio-list-s2` + two TPM nodes under `spi_aux` | stage2 (GPIO 52=0, 53=1, 54=1 so it can reach the TPM) |
| 0x41048 (DT object, id 2) | 0xb50d | `model = "celestica,cs8210"`, `version = "3.65.1.2"`: a full vendor DT plus `/soc/board-cfg` (id "CS8210"), `serdes-avg-complex-{main,sec,hs}` | stage2 (DRAM), stage3 (SerDes, eth params), U-Boot (`dt_read` loads it from the TOC into a 64 KiB malloc buffer = `$fdtaddr`) |
| 0x2587a8 (U-Boot `.dtb`) | 0x11d | "Annapurna Labs Stub Platform": empty memory/chosen/aliases | U-Boot control DT only (prints `Model:`) |
| 0x2abc18 | 0x11d | byte-identical copy inside the stale old U-Boot | nothing |

The kernel DT is a separate file (ONIE FIT, or ours from the SSD); U-Boot only fixes it up (item 22).
Decompiled: `analysis/vendor-uboot/fdt-*.dts`. The DT object differs from `analysis/celestica_cs8210-r0.dts`
(DIAGOS kernel tree, version 3.47.312) mainly by `board-cfg`, the SerDes complexes and the eth-switch stub.

## Init order (code)
`board_init` 0x01103f18 → `power_init_board` = DT read + `dt_based_init` 0x011020e4 → flash/NAND/env →
`pci_init` 0x011714ac (unless env `pcidisable`) → console → `board_late_init` 0x01103cd8 → SCSI scan with
retries 0x01149f28 → net 0x0114fac8 (`board_eth_init` 0x01103560) → TPM init+selftest 0x0114fa78 →
`main_loop`, whose board hook 0x01104600 prints the CPLD fault log, programs the LM75s, sets the ASIC core
voltage and prints the CPLD banner (`checkboard` 0x01104340) before the autoboot prompt.
Stage3, not U-Boot, does SerDes AVG firmware download and lane setup for all three complexes [hw: stage3 log].

## Checklist
"Needed by": L = Linux (mainline, our NixOS), S = vendor Teralynx SDK / ASIC, B = booting NixOS at all.
"Our path" = what happens today when the vendor U-Boot boots our DT with `booti`.

| # | item | vendor U-Boot does (evidence) | needed by | how a replacement / Linux provides it |
| --- | --- | --- | --- | --- |
| 1 | Entry, DRAM banks | stage3 app at 0x01100000 (EL2 per the V2 handoff ABI; not re-checked on V3). `dram_init_banksize` 0x0110321c: 8 GiB → 0+2G, 2G+1G, 0x1_0000_0000+4G, 0x2_0000_0000+1G; `fdt_fixup_memory_banks` rewrites `/memory` [code, hw: runtime DT] | B, L | Same handoff as DMC-12 (TEXT_BASE/PIE work there). Our DTS already hardcodes the four banks; a replacement may keep them static. |
| 2 | **SerDes** (PCIe 0/1/4, SATA, eth 1G/10G) | Nothing new: stage3 downloads AVG FW and configures main lanes 0-3 PCIe0, 4 PCIe4, 8 PCIe1, 15 SATA; sec lane 14 eth_std_1g (156.25 MHz ref), lane 15 eth_std_10g; hs disabled [hw: stage3 log, DT object]. U-Boot only builds lane handles (`board_init`) and per-port lane lists for PCIe (`dt_based_init_pcie_serdes` 0x01101df0) | S, L | **Comes for free** if only the UBOOT object is replaced (stage3 still runs). This is the "SerDes setup used as is" that al_eth port 3 and ASIC devport 129 rely on. |
| 3 | **External PCIe RC + link** | `pci_init_board_external` 0x0110d720: reads `board-cfg/pcie` (`crs-timeout` 30000, `ep-ports` empty), per enabled port HAL `al_init_pcie` 0x01133640 (tag "UBOOT"), up to 4 retrains with gen/width fallback, then registers a hose and scans. Ports 0 Gen3x4, 1 Gen2x1, 4 Gen1x1 [code, hw: "PCIE_n: Link up"] | S, L | **Must be re-implemented** (mainline `pcie-al` assumes link up and a configured controller). Port the V3 HAL PCIe init the way DMC-12 patch 0022 did for V2; V3 HAL 3.65 differs (AVG SerDes lane objects are passed into port init). |
| 4 | PCI device state at handoff | FLR on every device advertising FLR (none of the three external ones do) right before the OS jump (0x0110d628, call at 0x01150548). U-Boot only enables decode on the **ASIC** (COMMAND = 6, item 9); both FPGAs stay COMMAND = 0 [code, hw: PROGRESS 11-09] | L, S | Nothing to copy; Linux enables decode itself (`pci_enable_device`). Keep the "FPGA decode off" assumption in tools. |
| 5 | **ASIC core voltage (AVS)** | main_loop hook 0x01104600: finds PCI **1d98:1b58** (Teralynx), sets COMMAND = 6, reads 4 fuse words at BAR0 + 0x11178, decodes RoV and a 6-char lot ID, picks VDD core (RoV 0 → 800 mV, some lots 780, RoV 2 → 820, 5 → 840, 8 → 780, 10 → 760), then on bus 3 via 0x74:0 → 0x72:2 writes PMBus PAGE (0x00) = 0 and VOUT_COMMAND (0x21) = code (linear16, exp −10: 800 mV = 0x0333) to the regulator at **0x62** [code, hw: "Target VDD Core -> 800 mV, AVS value -> 0x0333"] | **S** | **Must be reproduced before the SDK starts**: in the replacement U-Boot, or a Linux oneshot (PMBus write via i2c-dev, fuse read via the ASIC BAR only after `echo 1 > enable`). Unknown: the regulator's power-on VOUT, i.e. what the ASIC runs at without it. |
| 6 | Board GPIOs | `dt_based_init_gpio` 0x01101a18 → `gpio_board_init` 0x01108f80: for each `board-cfg/gpio_init/gpio-list` triplet, MUIO-allocate the pin as GPIO, request, set dir/value. Inputs: 1, 28, 35, 36, 44-49, 58. Outputs high: 2, 3, 26, 42, 43, 51, 53, 54, 55, 56, 59, 60, 61; low: 4, 27, 29, 40, 41, 50, 52, 63 [code, DT] | L, S [inf] | **Must be reproduced** (one table in the board file). Meaning of most lines is unknown; Linux sees exactly this state today (all eight banks match stock, PROGRESS). |
| 7 | **SPI mux PBS52/PBS54 at OS handoff** | `do_bootm_states` 0x01150060 calls `gpio_set_value(52, 1)` at 0x0115083c and `gpio_set_value(54, 0)` at 0x01150848 just before every OS jump (bootm and booti). Before that, gpio-list leaves 52=0/54=1 (TPM selected), which U-Boot's own TPM use needs [code, hw: gpio6 data 0xbb in Linux] | L | Not needed for mainline: our DT drives the mux (`gpio-mux` + `spi-mux`). A replacement should leave 52=0/54=1 (TPM) or either; Linux sets it per transfer. |
| 8 | Aux SPI controller | DW SSI init for the boot flash (0xfd882000) and the aux bus (0xfd8be100); aux **CS_OVERRIDE (+0xf4) = 0xF** (0x01128b14) [code, hw: PROGRESS reads 0xF] | L | Mainline `amazon,alpine-dw-apb-ssi` handles CS override; no U-Boot action known to be needed. The dead i2cfpga flash is not explained by anything in U-Boot (open question 3). |
| 9 | Pinmux (MUIO) | `dt_based_init_pinctrl`: `board-cfg/pinctrl_init` = if_nand_8, if_nand_cs_0, **if_uart_1**, if_spim_a_ss_1, **if_i2c_gen**, **if_spim_aux**, if_sgpo_clk, if_sgpo_ds_0 [code, DT] | L | **Must be replayed** (DMC-12 patches 0025/0026 do the same for V2). Without if_spim_aux / if_i2c_gen the TPM and i2c-gen lose their pins [inf]. |
| 10 | SB PLL channel | `pll_sb_init` 0x01108e40: SB PLL @0xfd860b00, channel 9 → **25 MHz** (table 0x011a2858) [code] | L? [inf] | Reproduce (one HAL call) until the consumer of channel 9 is known (candidates: 88E1510 or FPGA reference clock). |
| 11 | I2C controllers | i2c-pld 400 kHz, SDA hold 380 ns; i2c-gen (U-Boot bus 9) SDA hold 300 ns from the DT [code] | L | Linux DT already sets both. |
| 12 | I2C bus map / muxes | `CONFIG_SYS_I2C_BUSES` @0x011d0d28: bus 0 = i2c-pld, buses 1-8 = PCA9548 @0x70 ch 0-7, bus 9 = i2c-gen. Board code reaches devices on bus 3 through 0x74/0x71/0x72/0x73 and always writes 0 to each mux afterwards [code] | — | Nothing to keep; Linux owns the tree. Note U-Boot labels 0x74 ch3 = TOPFAN and ch2 = BOTTOMFAN (PROGRESS fan note says the opposite). |
| 13 | Board identity / ONIE vars | `set_platform_env` 0x01103934: reads TLV EEPROM 0x56 (0x74:0 → 0x71:7; types 0x21, 0x27) and sys CPLD reg 0x09 bit 0 (1 or read error → "19" board, 0 → "_12v_21"); sets `onie_platform`, `onie_bootm_args`, `onie_file`, `dtb_file`; **saveenv if `onie_platform` differs** from the stored one [code] | ONIE | Not needed for NixOS. Hazard 2. |
| 14 | **MAC address** | al_eth port 1: base MAC from TLV type 0x24 (0x0115a920, "Net: MAC = ..."); port 3: random (0x0116d008). Stored into EC MAC table entry 0 by `write_hwaddr` (0x0116d084 → `al_eth_mac_addr_store`) at eth init, env `eth1addr`/`eth3addr` via the eth core [code, hw] | L | Linux al_eth reads EC entry 0, which is how enp0s1 keeps the base MAC today [inf: same DHCP lease as ONIE]. Replacement: read TLV 0x24 and store it (DMC-12 patch 0018 does the store for V2), or set `local-mac-address` in the DT. |
| 15 | al_eth board params / halt | stage3 stores per-port board params; U-Boot re-reads them, and on halt (`al_eth_halt_aux` 0x0116d0c4, skipped if env `skip_eth_halt` ≠ 0 or the port was never opened) resets UDMA, re-writes board params and the MAC [code] | L | Stage3 provides the params. Our path never opens U-Boot networking. Keep a halt if the replacement uses the NIC. |
| 16 | 88E1510 PHY (al_eth1, MDIO addr 0) | `board_eth_init` 0x01103560: page 3 reg 16 = 0x1018 (LEDs); page 18 reg 20 = 0x0001 then 0x8001 (mode SGMII-to-copper + reset); page 0. Repeated LED write at PHY start (0x0116e9a8) [code, hw: "Init eth PHY LED/mode"] | L | Mode: mainline `marvell` m88e1510 config_init sets SGMII mode for an SGMII interface. LEDs: cosmetic; DT `leds` or one register write. |
| 17 | Retimers / eth switch | board-cfg declares DS25 retimers (i2c-gen 0x18/0x19) for disabled 25G ports and a disabled mv88e6141 [code, DT] | — | Not needed (ports disabled; no "Configuring retimer failed" in the log). |
| 18 | SATA / AHCI | SerDes lane 15 by stage3. U-Boot: `scsi init` with retries (env `scsi_init_retries`), AHCI 1c36:0031 [hw] | B | Replacement needs AHCI + ext4 to load the kernel. Linux AHCI needs nothing from U-Boot [inf; unverified without U-Boot's scan]. |
| 19 | System LED, console logger | `board_init`: sys CPLD (0x74:0 → 0x71:2, 0x60) reg 0x20 = 0x7d ("blink_green"). `board_late_init` and the baudrate callback: sys CPLD reg 0x40 bit 6 = 1 unless baud is 9600 (console logger rate) [code] | — | Optional; one I2C write each (U-Boot or Linux). |
| 20 | LM75 limits | main_loop hook: 12 LM75s (0x48-0x4f behind 0x74/0x71/0x72/0x73) get THYST = 75 °C, TOS = 80 °C (MSB byte only, regs 2/3) [code, hw] | — [inf] | Same as LM75 power-on defaults, so only matters after Linux changed them and a warm reset; the OS pin wiring is unknown. |
| 21 | CPLD fault log, banner | Reads sys CPLD log (writes address regs 0x31-0x33 and control 0x30, reads 0x35, timestamp 0x58-0x5b); prints CPLD versions [code, hw] | — | Diagnostic only. `fault_log` command for humans. |
| 22 | **Kernel DT fixups** (`ft_board_setup` 0x01102d68) | clock-frequency of `/soc/clocks/{refclk,sbclk,nbclk}` (path) and of compatible `arm,cortex-a15`, `arm,armv8-timer` (= nbclk / (bits 15:8 of 0xf0071000 + 1) = **75 MHz**), `ns16550a` (= sbclk, 500 MHz); `status = "disabled"` on `/soc/pcie-externalN` that are off; `ranges` of `/soc/pcie-externalN` from the PBS address map (`al_addr_map_pasw_get` idx 10-17); env `iocc_force`; if `/soc/ccu` has `io_coherency`: CCU cluster write (item 23) and `dma-coherent` on all Alpine PCIe nodes. Plus generic: memory, chosen/initrd, `fdt_fixup_ethernet` [code, hw: ONIE runtime DT diff: timer 50 → 75 MHz, nbclk 800 MHz → 1.2 GHz, refclk 24 → 100 MHz, dma-coherent, 5 ports disabled] | L | **Our path:** only the compatible-based ones hit (timer 75 MHz, UART 500 MHz, both equal to our DTS values); every path-based one fails ("node not found" in the log), no CCU. A replacement needs none of it as long as the DTS keeps the real clocks. |
| 23 | CCU | Only via item 22: writes 3 to 0xf0200000 + (cluster + 4) × 0x1000 for clusters 0-3 (`al_ccu_cluster_init` 0x01133b60) [code] | — today | Not done on our path, and mainline runs non-coherent without it. Needed only if the DTS ever gets `dma-coherent`. Do not probe these registers from Linux (mistakes list). |
| 24 | TPM | HAL TPM over spi_aux with mux on TPM; selftest at init; measured boot extends PCRs at image load ("TPM extend failed" on our booti path); Ctrl-C at autoboot extends PCRs with junk [code, hw] | — | Not needed (Linux TPM works from the DT). Changes PCR values only. |
| 25 | SGPO | `board-cfg/sgpo_init`: 16x4 groups, SATA active/presence LEDs, group0 init 0xc0 [code, DT] | — | Optional (LEDs). |
| 26 | Fans | None: only fan CPLD 0x66 reg 0 (version) is read; no I2C write to 0x66 in the board code (call-site scan of `i2c_write`) [code] | — | Fans stay at CPLD default until NixOS; a replacement could add the PROGRESS quiet recipe. |
| 27 | Boot flash / env | `sf` probe at 25 MHz (DT `spi-max-frequency`), env 16 KiB at 0x380000 [code, hw] | B | Keep `fw_env.config` (mtd1, 0x0, 0x4000) unchanged if the env stays compatible. |

## Env and boot commands
- `bootcmd` = `boot_diagos; check_boot_reason; nos_bootcmd; onie_bootcmd`; `bootdelay` 3, stop with `123<Enter>`.
  `nos_bootcmd` is ours (PROGRESS 11-09 step 6). `onie_bootcmd` = `sf read` 0x1a00000 bytes from 0x410000 to
  `loadaddr` 0x08000000, `bootm` with FIT config `#celestica_cs8210`. DIAGOS (`onie_boot_reason=diag`):
  `ext2load scsi 0:3` uImage + DTB, `bootm`.
- The rest are Annapurna reference macros (NAND, TFTP, `bootupd*`, `dtupd*`, `eepromupd*`, `delenv*`,
  `uploadonie`) that write SPI/NAND/EEPROM when run. `delenvspi` uses `env_offset` (0x380000) and erases 0x2000.
- Set at every boot by `board_late_init` (RAM only): `dt_location`, `pld_i2c_addr`, `boot_instance_*`, `env_offset`,
  `fwupd`, `dt_is_from_toc`, `dt_filename`, `fdtaddr`; on a recovery boot `bootcmd = echo recovery boot command!`.
- **`init_script` uImage** (stale, see layout): a one-time script guarded by `boot_app_toc` that rewrites
  `bootcmd` to load BOOT_APP from flash and runs `saveenv` (the same reroute as on the DMC-12). U-Boot looks it
  up **only through the TOC** (`power_init_board`: "U-Boot script not found in TOC!"), so it never runs today.

## Vendor commands
The DMC-12 table applies ([uboot-vendor-commands.md](../dmc-12-48r-as0/uboot-vendor-commands.md)). New or
different here: `sbus` (AVG SBUS master rd/wr/int), `serdes` now per AVG complex (main/sec/hs), `ddr_ch_sel`,
`fault_log` (Celestica CPLD log, read-only), `sys_eeprom` (ONIE TlvInfo at 0x56; `write`/`erase` change board
identity and MACs, `protect disable` writes sys CPLD reg 0x05: **never**), `pca953x`, `sspi`, `nvme`,
`bootefi`. `uc_*` exist but this board answers stage2's uC mailbox with timeouts/"secure-mode" errors [hw].
Command table: linker list at 0x011d5c30..0x011d7e20 (`cmds.py`).

## Alpine V2 (DMC-12) list vs this U-Boot
| V2 replacement had to do | CS8210 vendor U-Boot | V3 difference |
| --- | --- | --- |
| PCIe RC init | yes, item 3 | V3 HAL 3.65.1 (DMC-12: 2.6x); PCIe init takes AVG SerDes lane handles; three ports, Gen3 x4 on port 0 |
| MUIO replay | yes, item 9 | different set: uart_1, spim_aux, i2c_gen, NAND, SGPO pins |
| GPIO from `gpio_init` | yes, item 6 (31 lines) | plus stage2's own `gpio-list-s2` and a stage2-embedded FDT stub; plus the bootm-time flip of GPIO 52/54 (item 7) |
| CCU io-coherency before Linux | only if the kernel DT has `/soc/ccu` (item 23) | CCU at 0xf0200000, value 3 per cluster (V2: 0xf0094000 ← 1, 0xf0090004 ← 7) |
| al_eth params + MAC handoff | yes, items 14/15 | MAC from the ONIE TlvInfo EEPROM over I2C, not the uC FRU / EEPROM board data; port 3 random |
| SSI (DW SPI) deselect | aux CS_OVERRIDE = 0xF at init, item 8 | plus the PBS52/54 SPI mux |
| — | **ASIC AVS**, item 5 | new, board-specific, needed by the SDK |
| — | SB PLL channel 9 = 25 MHz, item 10 | new |
| — | DT fixups, item 22 | path-based, so they miss a mainline DT |
| env 16 KiB redundant @0xfd0000/0xfe0000 | 16 KiB single @0x380000 | TOC has an ENV_RED slot whose contents are not an env |
| slot 512 KiB (IPXE), 3.8 KiB spare | UBOOT 0x1f0000, ~1.03 MiB spare; no IPXE/BOOT_MODE | no app slot to stage into; only UBOOT 0 itself |
| TEXT_BASE 0x01100000, 2015.07 | TEXT_BASE 0x01100000, 2018.05, ram_size capped 64 MiB | same load address |

HAL/version strings: U-Boot `2018.05-alpine_db-v3.65.1.015-HAL`; stale old U-Boot
`2018.05-alpine_db-3.65-00006-g946f0a2-HAL-00014-gac54dce` (built from `alpine-v3-al-boot-v3.65.1`); stage2
`stage2_loader v3.65.1`; stage3 3.65.1, commit/CVOS/HAL commit `aa76212` (Nov 20 2020); agent `agent_wakeup
v3.53`; DT object `3.65.1.2`; SerDes FW "rev 1094 build 244d". HAL units present: eth MAC v1/v2, v3, v4,
`al_eth_v4_lm`, serdes_avg, sbus, pcie, sys_fabric_utils_v3, ddr_alpine_v3.

## Hazards
1. **Writing a new UBOOT 0 has no escape path** (no BOOT_MODE/spare slot; the DMC-12 `u`-key trick needs a second
   app). Recovery would be the boot ROM UART boot (enabled in OTP per the 11-15 `otp show`) or a 1.8 V SPI
   programmer. A RAM chainload from the vendor prompt is the safe first step, as on the DMC-12.
2. The vendor U-Boot **writes the env by itself** when `onie_platform` differs from the TLV-derived value
   (item 13). Today they match, so nothing is written.
3. The stale UBOOT_SCRIPT (`init_script`) and BOOT_APP sit in the UBOOT slot slack. Adding them to the TOC
   (e.g. flashing a reference TOC) would reroute `bootcmd` and `saveenv`. A replacement image larger than
   0xe8040 overwrites them, which is harmless.
4. Item 5 sets the ASIC core voltage on every boot. A replacement that skips it runs the ASIC at the regulator's
   power-on value (unknown), with the SDK then on top.
5. `sys_eeprom write/erase/protect`, `otp` (all but `show`), `flash_contents_obj_update`,
   `flash_contents_instance_*`, `eepromupd*`, `bootupd*`, `dtupd*`, `uploadonie`, `delenv*`: never.

## Read-only checks for the main agent (vendor prompt, stop with `123`)
All are reads or the mux writes U-Boot itself does; no flash writes.
1. GPIO bank 6 before handoff: `md.l 0xfd8983fc 1; md.l 0xfd898400 1` → expect data **0x000000eb** (52=0, 54=1;
   bits 0-1 are inputs; Linux later reads 0xbb), dir 0x000000fc.
2. ASIC PCI state: `pci header 01.00.00` (command 0x0006 after the AVS step), `pci header 02.00.00` and
   `03.00.00` (command 0x0000).
3. ASIC core regulator: `i2c dev 3; i2c mw 0x74 0 1; i2c mw 0x72 0 4; i2c md 0x62 0x20 1; i2c md 0x62 0x21.1 2;
   i2c md 0x62 0x8b.1 2; i2c mw 0x72 0 0; i2c mw 0x74 0 0` → VOUT_MODE (expect 0x16, exp −10), VOUT_COMMAND
   `33 03`, READ_VOUT near 0x0333. Optional; skip if uneasy about the ASIC rail.
4. Sys CPLD: `i2c dev 3; i2c mw 0x74 0 1; i2c mw 0x71 0 4; i2c md 0x60 0x09 1; i2c md 0x60 0x20 1;
   i2c md 0x60 0x40 1; i2c mw 0x71 0 0; i2c mw 0x74 0 0` → reg 0x09 bit 0 = 1 (expected from `dtb_file`),
   0x20 = 0x7d, 0x40 bit 6 = 1.
5. SB PLL: `md.l 0xfd860b00 0x40` (for the channel 9 divider; save to `evidence/`).

## Open questions
1. What SB PLL channel 9 (25 MHz) clocks, and whether Linux breaks without it.
2. The meaning of the 31 `gpio-list` lines (resets? enables?), in particular 2/3/26 and 59-61 (high).
3. Why the i2cfpga flash stays dead on mainline: nothing in U-Boot explains it (it only flips 52/54).
4. Whether the AVS regulator's power-on VOUT is safe for the ASIC without U-Boot's write.
5. Whether stage3 on V3 honours the `p`/`e`/`u` keys like V2 (not checked in stage3).
6. Why U-Boot's PCI scan leaves the FPGAs with decode off (BAR assignment path not traced).
