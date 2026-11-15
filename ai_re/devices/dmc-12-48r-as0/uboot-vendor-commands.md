# Factory U-Boot: vendor commands and what the boot path relies on

Factory `U-Boot 2015.07-alpine_db-2.60.4-00030-g1e9d506-HAL-00010-g3eb5ee1` on u01. This
covers what each non-standard command does, and what our upstream U-Boot (RAM chainload,
`u-boot-port.md`) needs from it. Static analysis only (2026-11-15). Nothing was run on
hardware for this file.

**Method.** The image is `dumps/u01/spi-cs0/uboot.0.bin` from offset 0x48 (al-boot
header first), linked at 0x01100000. The command linker list maps every name to its
handler: `inventory/u01/uboot-cmd-table.txt`. Ghidra project with the handlers named
`do_<cmd>`: `ghidra/dmc12.gpr` (git-ignored, holds firmware). Rebuild it with
`tools/ghidra/import-raw.sh <img> devices/dmc-12-48r-as0/ghidra dmc12 0x01100000`. Usage
text: `inventory/u01/uboot-help.txt`. Claims marked *inferred* come from code shape and
were not observed.

**Verdicts** are for our port: **needed** (the board can't boot or run our way without an
equivalent), **useful** (bring-up or maintenance), **not needed**, **never** (dangerous or
one-way: keep the vendor U-Boot for it). Short version: the RAM chainload needs **no**
vendor command inside upstream U-Boot. It does need some side effects of the factory
boot path (next section).

## What the factory boot path does that Linux relies on

The factory U-Boot always runs before ours (chainload). So everything in its init is
already done when upstream U-Boot starts. The items marked **gap** are the ones our boot
path skips today.

1. **Board init, before the prompt** (`board_init` / `power_init_board` / `misc_init_r`):
   - Reads the straps and inits the SB PLL.
   - Allocates the MUIO pinmux. The DT asks for `uart_1`, `spim_a_ss_1`, `spim_a_ss_2`.
     Mainline Linux has no Alpine pinctrl, so it inherits this.
   - Drives GPIO 22 high (`gpio_init`, purpose unknown) and sets up SGPO.
   - Powers up the SoC thermal sensor (0xfd860a00).
   - Creates SerDes handles only. The SerDes groups were already set up by al-boot
     (group0 `pcie_g3x4`, group1 `sata`).
   - Copies the uC FRU from shared memory (MACs).
   - Loads the DT object from the SPI TOC into the buffer that becomes `${fdtaddr}`.
   - **Trains the external PCIe link** (`pci_init_board_external`, the same code as
     `al_pcie_ext`): `PCIE_0: Link up 2.5GT/s x1` to the Pericom bridge, and so to the
     48 USB serial ports. Mainline `pcie-al` assumes firmware did this.
   - Runs the SPI `UBOOT_SCRIPT` object on **every** boot (see hazards).

   Upstream U-Boot needs none of this while it is chainloaded. Replacing the factory
   U-Boot in SPI would mean porting all of it.
2. **`bootm` itself** (also runs for our uImage):
   - `ft_board_setup` patches clocks into the DT (refclk/sbclk/nbclk, CPU, arch-timer
     = nbclk/div, ns16550 = sbclk) and disables unused `pcie-external` nodes.
   - It also **programs the CCI**: CCU at 0xf0090000, +0x4000 ← 1 (snoop enable,
     cluster ACE port), +0x4 ← 7. It does this because the vendor DT has
     `/soc/ccu io_coherency = <1>`. *Inferred: this is the CCI-400 snoop and speculation
     control.* It also programs the PCIe-external `dma-ranges` from the PBS address map.
   - It then FLRs every enumerated PCI function that has FLR. The u01 log prints none.

   Our chainload `bootm … - ${fdtaddr}` passes the vendor DT, so the CCI is set before
   upstream starts. Our Linux DTS marks nothing `dma-coherent`. That is correct, just
   slower. Don't add `dma-coherent` unless the snoop state is known.
3. **`dmc12_boot_preamble`** (env macro; interrupting autoboot skips it). Both on-disk
   loaders now run the whole macro before anything else (uboot-menu: `/boot.scr`;
   chainload: the factory `nixos_boot`, `u-boot-port.md`), so the gaps below are closed
   on paper, not yet on hardware:

   | step | what | our path |
   | --- | --- | --- |
   | `check_pci_for_sata_voltage` | Compares the external-PCIe device ID at 0xfb600000 with 104c:8241 (TI xHCI = a "DN0" box). If it matches, runs `serdes wr 1 p0 pma 21 2 0 5`, the SATA lane TX setting. u01 has 12d8:400c, so it is skipped there. | via the preamble |
   | `clear_wdt_flag`, `fan_speed_70` | CPLD 0x2a←0, 0x50←0x2a (`vendor-tooling.md`) | via the preamble |
   | `status_led_yellow_blink` | CPLD 0x17←0x02 | cosmetic; NixOS sets the LED |
   | `set_uplink_speed` | Uplink PHY sync over I2C 0x40 from CPLD 0x18 | NixOS module does it at boot |
   | `bootp` | DHCP. Its **side effect** matters, see 4. | via the preamble (needed) |
   | `dmc12_load_dt`, `scsi init` | copy the DTB to 0x04078000; AHCI | upstream does its own |
4. **Ethernet handoff (needed).** The vendor and our Linux `al_eth`
   (`nix/packages/al-eth`, `al_eth_board_params_init`) read the port's **board params
   from MAC scratch registers** (mac_base +0x4, +0xc, +0x404, with a magic). Without them,
   probe fails with "board info not available". They also read the MAC from EC forwarding
   entry 0 (EC +0x868/+0x86c); without it, Linux picks a random MAC.
   - The factory U-Boot's `al_eth_halt` (end of every network command when
     `skip_eth_halt` is unset or `0`) reads the params back, FLRs the function, restores
     them and writes the MAC (`al_eth_write_hwaddr`). The `eth_*` commands also write
     the params. Correction (2026-11-15, Ghidra on `uboot-2015-alpine.bin`): its
     `al_eth_register` (PCI probe, `FUN_01167568`) *reads* the params and fails with
     "board_params_get failed" if they're absent. So something writes them before
     the first network command: **stage3** does (`al_eth_board_params_store` in its
     DT-based eth init; found 2026-11-15, `spi-recovery.md` work), which matches the
     params being present at the prompt right after power-up.
   - So the TFTP chainload (it runs `dhcp`) is fine, and both on-disk paths run the
     preamble's `bootp` (factory env has `autoload=n`). Upstream U-Boot now also writes
     missing params and the MAC itself before Linux (`u-boot-port.md`, network).
   - Port1 (RGMII, 1G, autoneg off, `allow-random-mac`) is the only enabled port in
     board-cfg.
5. **uC watchdog.** The factory U-Boot never talks to the uC except inside the `uc_*`
   commands (every caller of the mailbox is a command handler). So there is no kick to
   copy. Nothing needed.
6. **`reset`** is vendor code: sp805 at 0xfd88c000, LOAD←0 then CTRL←3, then spin. The
   same as our sysreset fallback. It never touches the CPLD.
7. **Firmware memory.** The 0x02000000 +1 MiB reserved range holds the al-boot trace
   buffer (0x02004000–0x0203f000, from the stage3 log; `trace_dump` prints it). Keep
   reserving it.

### Hazards found on the way
- **`UBOOT_SCRIPT` runs on every boot** (from `misc_init_r`), not once. When
  `boot_app_toc` ≠ 10, it sets `bootcmd=al_pcie_ext; …; bootm BOOT_APP`, sets
  `bootdelay 0` and runs `saveenv`. The compiled-in default env (NAND bootcmd, delay 2)
  has no `boot_app_toc`. So any env reset (`env default -a; saveenv`, `run delenv`, a
  blank env sector) makes the **next boot rewrite the env and boot the K2 `BOOT_APP`
  firmware**, with all `dmc12_*` macros gone. Never reset the env. `reimage.md`'s
  edits keep `boot_app_toc`.
- `otp`'s "password" is a fixed constant compiled into the HAL. It is the same for
  every board and is no protection. Every `otp` subcommand except `show` burns a fuse.
- `uc_reset`'s delay argument can't be given: maxargs 2 makes the argument count too
  small. It always sends 3 s. Per the HAL string, the uC then **resets the Alpine SoC**,
  not only itself.

## uC command channel (all `uc_*`)

Every `uc_*` handler first runs `al_uc_cmd_handle_init`. A command works like this:
1. Write {cmd, arg} into a 4-word **mailbox at 0xfd8a4170**, and clear the status and
   result words.
2. Pull **GPIO 37** (PL061 @0xfd88b000, line 5) low.
3. Poll the status word every 1 ms. The default timeout is 2 s, the update timeout up to
   300 s.
4. Release GPIO 37. Success is status `0xcaca00aa` (updates: `0xcaca00dd`). The result is
   word 3.

Command IDs are `0xbabaNNNN`. Data commands (FRU burn, updates) pass a **DRAM address**
as arg, so the uC reads SoC memory itself. *Inferred: over the SoC's I2C-slave/debug
path, which also explains the PBS-window mailbox.*

The uC also publishes **shared data at 0xfd8a7f00**:
- FRU, 0x60 bytes, magic 0x1c361c36, holding the MACs. The factory U-Boot copies it at
  boot ("FRU initialized").
- A voltage/temperature record at +0x100, magic 0x4747.

The `uc_*` I2C faces (LM75A 0x4A, mgmt 0x46, FRU 0x30) are in `k2-uc.md`. Linux must
not claim GPIO 37: toggling it sends a command. A Linux-side reader (devmem + gpiod)
would be the right home for the read-only commands. Upstream U-Boot needs none.

## Command table

`uC 0x0707` means command `0xbaba0707`. "R" = read-only, "W" = writes. Risk: low /
med / **high** / **one-way**.

| command | what it does (HAL, hardware) | R/W | risk | verdict |
| --- | --- | --- | --- | --- |
| `uc_get_version` | uC 0xdede (bootloader) + 0xdddd (app) versions | R | low | useful |
| `uc_get_flash_booted_from` | prints "Unsupported command for dev id 1"; sends nothing | R | low | not needed |
| `uc_get_secure` / `uc_get_pp` / `uc_get_nitro_mode` | uC 0x2121 / 0x2222 / 0x2a2a+0x2931 | R | low | not needed |
| `uc_get_secure_log` | uC 0x2323 | R | low | not needed |
| `uc_get_recovery` | uC GPIO read 0x0909 **port 2 bit 2**. The message says "SPI flash": *inferred* SPI flash select (CS0 vs recovery CS1?) | R | low | useful (answers "what selects mtd1") |
| `uc_set_recovery` | uC GPIO write 0x0a0a port 2 bit 2 = 0/1 ("Spi set recovery") | W uC GPIO | **high** (boot flash choice) | never |
| `uc_watchdog_get_status` | uC 0x0f0f: enabled, timeout, **reset count** | R | low | useful |
| `uc_watchdog_set_enabled` | uC 0x0e0e (enable, timeout s) / 0x0d0d (disable) | W uC | med (10 s reset risk) | never |
| `uc_reset` | uC 0x0101, arg = delay (always 3 s, see hazards); the uC resets the SoC | W | med (resets board) | not needed (CPLD reset works) |
| `uc_thermal` | uC 0x0404 status; enable 0x0202 / disable 0x0303 of the LM75A face, addr 0x48–0x4f | R, or W with args | med (Linux `5-004a` sensor moves or vanishes) | not needed |
| `uc_mgmt` | uC 0x0707 status; 0x0505 / 0x0606 enable/disable the mgmt I2C slave (default 0x46) | R, or W with args | med | not needed |
| `uc_fru` | uC 0x1717 status; 0x1313 / 0x1414 FRU I2C face (0x30) | R, or W with args | med | not needed |
| `uc_sdr` | uC 0x1a1a status; 0x1818 / 0x1919 SDR face | R, or W with args | med | not needed |
| `uc_fru_print` | uC 0x1616 word by word into `$loadaddr`, then prints (holds identifiers) | R (RAM) | low | not needed |
| `uc_fru_valid_get` | uC 0x3838 | R | low | not needed |
| `uc_fru_burn` | uC 0x1515, arg = DRAM addr; ≤1 KiB, byte-sum checksum; writes the length to addr−4 | W uC FRU | **high** (MACs/ID) | never |
| `uc_fru_erase` / `uc_fru_unlock` | uC 0x3030 / 0x3737 | W | **high** | never |
| `uc_sdr_print` / `uc_sdr_get_version` | uC 0x1b1b (16-bit reads) / 0x1d1d | R | low | not needed |
| `uc_sdr_reset` | uC 0x2020 | W | med | never |
| `uc_sdr_set_fw_update` | uC 0x1c1c, value 1, 2 or 4 | W | **high** | never |
| `uc_sdr_set_mask_reg` / `uc_sdr_clear_mask_reg` | uC 0x2d2d / 0x3d3d ("V3 only") | W | med | never |
| `uc_set_secure` | uC 0x1e1e enable/disable secure mode | W | **one-way?** (unknown) | never |
| `uc_set_pp` | uC 0x1f1f physical-presence flag | W | **high** | never |
| `uc_clear_secure_log` | uC 0x2424 | W | med (destroys evidence) | never |
| `uc_set_nitro_mode` | uC 0x2828 disable / 0x2929 full / 0x2930 MVP+threshold / 0x2b2b IPR+PERST threshold (AWS Nitro card modes) | W | **high** | never |
| `uc_set_spi_buffer` | uC **0xcaca3737** enable/disable ("uC config"). Stage2 sends the same command as `uc_enable_tpm20_access` before talking to the SPI TPM on CS2, so it gates **TPM** SPI access, not the boot flash (corrected 2026-11-15, `spi-recovery.md`) | W | **high** | never |
| `uc_thermal_shutdown` / `_set_temp` | uC 0x2525 / 0x2626 on/off; 0x2727 max temp 51–115 °C (the help text is wrong) | W | **high** (cooks or trips the board) | never |
| `uc_gpio_read` / `uc_gpio_get_dir` | uC 0x0909 / 0x0b0b [port, bit] | R | low | not needed |
| `uc_gpio_write` / `uc_gpio_set_dir` | uC 0x0a0a / 0x0c0c | W uC GPIO | **high** (unknown wiring; port 2.2 = flash select) | never |
| `uc_update` | `app` 0xcccc (sizes 0x3000, 0xac00, 0xad00, 0xb000, 0x30000, 0x30100; CRC16-CCITT trailer), `bootloader` 0xbbbb (0x4f00/0xf000), `auth_info` 0xeeee (0x200); arg = DRAM address | W uC flash | **one-way-ish** (brick the uC) | never |
| `al_pcie_ext` | `pci_init_board_external`: SerDes ICAL, port init, link train with gen/width fallback, from `board-cfg/pcie`. `--no-teardown`, `--reinit`, `--skip-serdes-ical` | W PCIe/SerDes | med | not needed (done at factory init; needed only if the factory U-Boot is replaced) |
| `eth_1g_params_set` | port 0–3: sgmii/1000basex, AN, speed, duplex into **board params (MAC scratch regs)**; Linux al_eth reads them | W eth regs | low | **needed** as a mechanism (see 4), not as a command |
| `eth_freeze_serdes_settings` | board-params `dont_override_serdes` (the DT already says `enable` for ports 0 and 2) | W eth regs | low | not needed |
| `eth_link_training_enable` / `eth_lm_debug_enable` | board-params KR LT flag / link-management debug | W eth regs | low | not needed |
| `eth_mac_mode_set` | board-params MAC mode (RGMII, 10G, SGMII, 1000BASE-X, auto) | W eth regs | low | not needed |
| `eth_retimer_config` | board-params retimer exist/bus/addr/channel (DT: DS25 at I2C bus 2 / 0x18 for ports 0 and 2, both disabled) | W eth regs | low | not needed |
| `serdes` | `al_serdes_*` reg rd/wr, PMA resets, power modes, loopback, BIST/PRBS, eye, TX/RX EQ on groups 0–4 (0xfd8c0000 + grp×0x400; grp 4 @0xfd8c2000) | R (rd, eye) / W | **high** for writes | needed **on DN0 only**: the one SATA write, done from the factory side; never port |
| `flash_contents_toc_print` / `_obj_info_print` | TOC in the first 128 KiB of SPI CS0 (16-byte steps); object IDs in `k2-uc.md` | R | low | not needed (offline on the dumps) |
| `flash_contents_obj_read` / `_obj_read_mem` / `_obj_validate` | object → `$loadaddr`; parse an object already in RAM (the `dt.img` path); CRC check | R | low | not needed |
| `flash_contents_set_dev` | backend `boot` / `spi` / `nand`. SPI is bus 0 **CS0 only** | R | low | not needed |
| `flash_contents_obj_update` | SPI erase + write of an object instance (`app_updspi` uses it) | W SPI | **high** | never |
| `flash_contents_instance_invalidate` | Zeroes the first byte of that instance's PRE_BOOT_V2 / PRE_BOOT / STG2 object: the boot-ROM `S2` header at 0x0 / 0x40000. One 4 KiB erase + rewrite. The boot ROM then skips the instance. | W SPI | **high** | never |
| `flash_contents_instance_revalidate` | writes `S` (0x53) back | W SPI | **high** | never |
| `flash_contents_boot_mode_default_app_set` / `_show` | BOOT_MODE object (ID 0, 0x8c bytes): which app the pre-boot code loads | W SPI / R | **high** / low | never / not needed |
| `fru` | `show`: 4 MACs from the uC FRU copy (falls back to the per-device EEPROM) | R (identifiers) | low | useful idea: upstream net can take MACs from 0xfd8a7f00 instead of the env |
| `eeprom_per_device` | `show`; `mac_addr_set` (RAM); `save`: I2C EEPROM at `${pld_i2c_addr}` (the boot-ROM **I2C preloader** EEPROM) offset 0x600, record magic 0xcc | R / W EEPROM | **high** on save (that EEPROM feeds the boot ROM) | never |
| `otp` | `al_otp` @0xfd896000 + PBS: `show direct/shadow`; everything else sets OTP bits (secure boot, JTAG/debug/boot-source disables, serial, PUK hashes, min SW ver, TZ) | R / **burns fuses** | **one-way** | never |
| `dev_info` | bootstrap straps: PLL freqs, I2C/SPI preload, SPI master mode, boot device | R | low | not needed (run once per unit for `units.md`) |
| `is_nand_boot` | bootstrap: boot device is NAND | R | low | not needed |
| `cpu_set_speed` | CPU PLL @0xfd860d00 channel divider, live | W PLL | **high** | never |
| `cpu_aborts_enable_set` | HCR_EL2.AMO / DAIF.A (SError routing/masking) | W sysreg | med | not needed |
| `ddr_ecc_stats` | DDR ctrl 0xf0070000 / MC 0xf0080000 / PHY 0xf0088000 ECC status; `clear` resets counters | R (W with `clear`) | low | useful (memory health during bring-up; run it in the vendor U-Boot) |
| `ddr_ecc_poison` | injects a correctable or uncorrectable ECC error at an address | W DDR ctrl | **high** | never |
| `ddr_training_results` | prints the stored DDR training | R | low | not needed |
| `dram_margins` | copies a 4280-byte blob to on-chip SRAM 0xfbff4010 and runs it (RDQS/WDQS shmoo with the DDR BIST) | W SRAM + DRAM | **high** (clobbers SRAM boot info and DRAM; may hang) | never |
| `iodma_init` / `_memcpy` / `_memset` / `_terminate` | RAID/memory UDMA (PCI 1c36:0022) | W RAM | med | not needed |
| `thermal_init` / `thermal_get` | SoC thermal sensor 0xfd860a00 power-up / read (board init already powers it) | R / W | low | not needed |
| `sgpo` | SGPO @0xfd8b4000 set/clear/user/hw (DT leftovers: "ETH 0/1" LEDs) | W | low | not needed |
| `muio_mux` | alloc / free / falloc of MUIO pin groups (PBS 0xfd8a8000) | W pinmux | med (can steal UART/SPI pins) | not needed |
| `lcd_print` | serial-LCD escape bytes out of **UART1** (0xfd884000); called by many env macros | W UART1 | low | not needed |
| `trace_dump` | dumps the al-boot trace ring (0x02004000) | R | low | not needed |
| `wfi` | CPU `wfi` | – | low | not needed |
| `confirm_msg` / `scripth` | y/n prompt into `$?`; prints the script help (bootupd, delenv, …) | – | low | not needed |
| `setenvmem` / `incenv` / `showvar` / `mem_pattern_set` | env from a memory word / add to an env var / hush locals / alternating 64-bit fill | RAM | low | not needed (upstream has `setexpr`) |

## Hardware questions (read-only; for the main agent)
Run at an idle `ALPINE_DB>` after `./powercycle_to_uboot.py`:
1. **Ethernet handoff.** First `pci header 0.1.0` to get the BARs. Then `md.l` the MAC
   BAR +0x4, +0xc, +0x404 and the EC BAR +0x868, +0x86c. Do this before, then after,
   `setenv autoload no; dhcp`. Do the params appear only after `dhcp`? That settles
   item 4. The EC words are the MAC: don't paste them into tracked files.
2. **Flash select.** `uc_get_recovery`, `uc_gpio_read 2 2` and `uc_gpio_get_dir 2 2`
   (read the SPI-select line).
3. `uc_watchdog_get_status`: the reset count, to see whether it ever fired.
4. `dev_info`: the straps (I2C preload on or off, boot device); record it per unit.
5. `md.l 0xfd8a4170 4` (mailbox idle state) and `md.l 0xfd8a7f00 1` (expect 0x1c361c36;
   only the first word, the rest holds MACs).
6. From the **upstream** `=>` after a chainload: `md.l 0xf0090004 1` (expect 7) and
   `md.l 0xf0094000 1` (expect 1). This confirms the CCI state that Linux inherits.
7. Per new unit: `md.l 0xfb600000 1` (after the factory PCIe init). 0x8241104c means a
   DN0 box, which needs the SATA SerDes write.
