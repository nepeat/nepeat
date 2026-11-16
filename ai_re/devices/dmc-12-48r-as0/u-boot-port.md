# upstream U-Boot on the DMC-12: RAM chainload (2026-11-15)

**Goal of this stage:** the factory U-Boot 2015.07 (`ALPINE_DB>`) loads upstream
U-Boot 2026.10 into RAM and jumps to it. The new U-Boot boots Linux the modern way:
raw arm64 `Image` + `booti`, extlinux via `bootflow scan`, from the SATA SSD's ext4.
Nothing is flashed. SPI NOR, the env and the SSD are only ever read.

**Status (2026-11-15, u01): RAM chainload works on hardware.** The factory `bootm`
entered at EL2 with MMU+caches off. Console at 500 MHz, `DRAM: 3 GiB (total 4 GiB)`,
relocated to 0xaff6a000. I2C through both muxes to the CPLD (0x01 = 0x88), internal
PCIe ECAM (4 SoC devices), AHCI + SSD, GPT read. `bootflow scan -l` finds none (the
factory SSD has no extlinux). `reset` → CPLD → stage2 in 2 s. Not run: `--go`,
`--boot`, booting Linux. Not in this build: the `dm` command (dropped from the harness
since), external PCIe (the USB bridge: no U-Boot driver for `al-alpine-v2-pcie`, not
needed). SCSI capacity reads one sector high (62533297 vs Linux 62533296; QEMU shows the
same +1), harmless for partition reads. Transcript:
`serial-chainload-20261115-042317.txt` (git-ignored).

**Ethernet (2026-11-15): built and offline-tested, not yet run on hardware.** The al_eth
driver (`dhcp`, `tftpboot`, `ping`, HTTP `wget`) and the board-params/MAC handoff to
Linux, see [network](#network-al_eth-built-not-yet-run-on-hardware). Staged in
`/srv/tftpboot/dmc12-uboot/eth-b/` (driver + handoff) and `eth-a/` (handoff only, no
network stack); result links `nixos-build/result-uboot-dmc12-eth-{a,b}`.

## what exists

- `nix build path:./nix#uboot-dmc12-chainload` → `u-boot.uimage` (for `bootm`),
  `u-boot.bin` (for `go`/`booti`), `u-boot`, `.map`, `.dtb`, `.config`.
  Current (with Ethernet, = `eth-b`): `/nix/store/5ivzlvicrzwnhyf7vpgja1zrk0s0kcva-…-2026.10`,
  `u-boot.uimage` sha256 `062428a9…`, `u-boot.bin` `f4b4b4f3…`. `eth-a` (0018+0019
  only, built at commit 7e438d0; the later 0018 change only matters with the driver):
  `/nix/store/2j3xfn9znz0wavzxg3b3j80k4dmbh102-…`, uimage `6e16f48b…`. The
  hardware-tested build (no network): `/nix/store/7w61vis50vjz9p5vjm4vs4czbn0wsjba-…`,
  uimage `7634a36c…`.
- Patches: shared series [ai_re/nix/packages/uboot/default.nix](../../nix/packages/uboot/default.nix):
  - 0014 `alpine-v2` (any Alpine V2 board): new target `TARGET_ALPINE_V2`, with no low-level init.
  - 0015 `dmc12`: CPLD sysreset driver.
  - 0016 `dmc12`: `-u-boot.dtsi` and `alpine_v2_dmc12_defconfig`.
  - 0017 `alpine-v2`: initrd and FDT used in place (`fdt_high`/`initrd_high`).
  - 0018 `alpine-v2`: al_eth board params + MAC handoff before Linux, `aleth` command.
  - 0019 `dmc12`: the port's DT node (00:01.0, rgmii fixed-link 1000, EEPROM MAC).
  - 0020 `alpine-v2`: the al_eth network driver (HAL copied verbatim from `packages/al-eth/src`).
  - 0021 `dmc12`: network on in the defconfig, no ethernet bootdev.
  - 0022–0028: the [self-init build](#self-init-build-pcie-muio-ccu-read-only-spi-ram-tested-on-u01)
    (external PCIe RC, Alpine SSI CS override, read-only SPI NOR, MUIO/CCU, DT bits).
    All default off: `#uboot-dmc12-chainload` is byte-identical with them applied
    (u-boot.bin `f4b4b4f3…`, uimage `062428a9…`, same DTB; `.config` gains only
    "is not set" lines).

  The T208x builds take only their own scopes, and their `.drvPath`s are unchanged.
  The board DTS is the Linux one, `nix/packages/linux-alpine/boards/dmc12/alpine-v2-dmc12.dts`,
  copied in at build time. A DTS edit there rebuilds U-Boot.
- Tests: `nix build path:./nix#uboot-alpine-v2-tests` (no board needed, all five pass;
  `selfinit` and `pcie-init` are described in the self-init section).
  - **static:** AArch64 ELF linked at 0x08000000, Linux Image header, uImage header,
    RAM footprint, no `smc`/`hvc` instruction anywhere. The config is checked to have
    no SPI flash, env storage, MMC, EFI, PSCI reset, ethernet bootdev or EEPROM-writing
    command, the sp805 not auto-started, and al_eth + dhcp/tftp/ping/wget present. The
    DT is checked for the memory banks, firmware ranges, CPLD and sp805 nodes, the
    500 MHz UART bound before relocation, and the al_eth port node with its EEPROM phandle.
  - **eth-params:** host build of the handoff encoders (`include/alpine_v2_eth.h` out of
    the series) against the HAL in `packages/al-eth/src`: register offsets, enums, the
    DMC-12 params `60000001 00000004 00000000`, 20000 random param sets through
    `al_eth_board_params_set/get`, MACs through `al_eth_mac_addr_store/read`, and the
    ONIE TLV parser (the CRC convention was also checked by hand on the u01 EEPROM
    dump). Also: the 23 HAL files inside patch 0020 are byte-identical to `packages/al-eth/src`.
  - **qemu-virt:** the same patches and board code on `virt,highmem=off,virtualization=on`
    (Cortex-A57, EL2), with a test DT ([qemu-virt.dts](../../nix/packages/uboot/alpine-v2/qemu-virt.dts)).
    1. Entered like the vendor `bootm`: MMU off, x0 = DTB.
    2. Relocates below a reserved top range.
    3. `bootflow scan -lb` goes PCI ECAM → AHCI → GPT → ext4 → extlinux, then `booti`s a
       probe "kernel". The probe sees EL2, a valid FDT, and the reserved ranges in its
       memreserve map, even though its DTB had no `/reserved-memory`.
    4. From that U-Boot's prompt (MMU and caches on), `go` to a second copy reaches a
       prompt that reports the MMU-on entry.

    QEMU doesn't model caches, so this proves the code path, not the cache maintenance.
    Also checked: the al_eth handoff runs before "Starting kernel" and reports no NIC.
    QEMU has no al_eth model, so the network driver is tested on hardware only.

## self-init build: PCIe, MUIO, CCU, read-only SPI (RAM-tested on u01)

What the factory U-Boot does for us today, done by ours, so it can one day be al-boot
stage3's application ([spi-uboot-replacement.md](spi-uboot-replacement.md) §4). Built
2026-11-15; passes the RAM test on u01 (run 3 below); nothing flashed.

- **u01 RAM run 1 (2026-11-15 12:17, `serial-chainload-20261115-121717.txt`): abort at
  PCIe probe** (esr 0x96000004, far 0xfffffffffd8a815c, in `al_pbs_dev_id_get` called
  from `al_pcie_port_handle_init`). Cause: the glue read the PBS base with
  `dev_read_u32_default()`, which returns `int`, so 0xfd8a8000 was sign-extended. Fixed in
  0022 (u32 + `map_sysmem`); the HAL was not at fault. Banner, EL2, DRAM and the MUIO
  replay (`4 pins from the DT, 0 changed`) were fine. Factory prompt reads:
  `0xfd8108d0` (DW AMBA error response) = 1, `0xfd810900` (iATU viewport) = 0, as the
  driver assumes.
- **u01 RAM run 2 (12:22, `serial-chainload-20261115-122243.txt`):** probe, kept link and
  enumeration fine (`02.00.00 12d8:400c`, `03.03.00 12d8:400c`, `04.00.00/01 12d8:400e`,
  `04.00.02 12d8:400f`; SErrors drained 0). `alpcie reinit` **trained the link with our
  init** (`our init after teardown, LTSSM 0x11`), then an SError ("Error" handler, esr
  0xbf000000) in the rescan. Two bugs, both fixed in 0022:
  - `alpcie reinit` left the old child bridges bound; `pci_auto` numbers a bridge's
    secondary bus as the highest bound seq + 1, so the root port got secondary 3, not 1,
    and the scan hit buses nothing answered. Reinit now unbinds the children.
  - U-Boot's start.S routes SErrors to EL2 and unmasks them (the factory U-Boot keeps
    them masked), so an error response is taken at once, before the drain. The
    downstream accessors now mask PSTATE.A around the access and drain before unmasking.
  - `PCI: Failed autoconfig bar 10/14` (both runs, also at boot) is the root port's own
    BAR0/1 not fitting the window: harmless, Linux reassigns; `alpcie status` now
    prints BAR0/1 and the bus numbers.
- **u01 RAM run 3 (12:31, `serial-chainload-20261115-123112.txt` + a manual continuation
  by the main agent): all checks pass.** Firmware link kept; both chips' `crc32` match
  the backups (CS0 below env `08145ce6`, CS1 `3d086ac6` and full `db9e4937`); MUIO pins
  as the DT wants; CCU `c0000000 -> c0000001`, `0 -> 7`; `alpcie reinit` → init ours,
  x1, full tree (400c, 400c, 400e ×2, 400f), 0 SErrors; then `bootflow scan -lb` booted
  NixOS on our link: smoke 15/15, 48 ports, Linux `PCIe Gen.1 x1 link up`, no PCIe errors.
  - `sf read` of 16 MiB took ~260 s: the DW SSI caps transfers at the controller's
    `spi-max-frequency`, 500 kHz when absent. 0026 now sets 25 MHz on the controller
    (as Linux reads them); not yet run. The harness allows 600 s per `sf read` and
    prints the rate.
  - AXI `+0x30` (0x3ff now, 0x1ff or 0xff at other times) is `ob_ctrl.cfg_target_bus`:
    mask 0xff + the bus number of the last downstream config access, which the driver
    (and Linux pcie-al) rewrites per access. Not a configuration difference. `+0x34` =
    `cfg_control` sec 1 / sub 0xff, as pcie-al sets it.

- `#uboot-dmc12-selfinit` (`/nix/store/8jzhzl8wviki8p9szn87784scys8idh1-…-2026.10`,
  u-boot.bin `f8edf702…`, 520328 B, uimage `b275b066…`; run 3 was `vq5f68zs…`, which
  lacks the SSI frequency and GPIO 22): the chainload build plus
  - **0022 `alpine-v2`, external PCIe RC** (`pcie_alpine_v2`, on the mainline
    `amazon,al-alpine-v2-pcie` node). Annapurna PCIe HAL 3.4 copied verbatim from the
    MikroTik GPL drop (`devices/cs8210-32x/analysis/research/mikrotik-added/drivers/soc/alpine/HAL`,
    reference copy [al-pcie-hal/](../../nix/packages/uboot/alpine-v2/al-pcie-hal)).
    `al_init_pcie_rc.c` runs the RC path of the vendor `al_init_pcie()` with the factory
    U-Boot's parameters (Ghidra: template 0x011b3180, port params 0x011b3598, Gen3
    0x011b3220; snoop on, AXI slave error response on, the factory EQ table, gen/lanes
    from DT = gen1 x1). No SerDes iCal (the factory passes none for external ports).
    Probe: a link already up is kept ("firmware init, kept"); a down link is
    initialised. `alpcie reinit` removes the bus, tears the port down (link stop,
    memory shutdown, port disable), runs the full init and re-enumerates. Downstream
    config errors are SErrors on this SoC: drained like the factory does
    (HCR_EL2.AMO + a returning vector), CRS retried 30 s. iATU region 0 = memory
    window 1:1 (as Linux dwc).
  - **0023 `alpine-v2`:** DW SSI `amazon,alpine-dw-apb-ssi` writes CS override 0xf
    (+0xf4, as Linux and stage3 do).
  - **0024 `alpine-v2` (generic code):** `SPI_FLASH_READ_ONLY`: no erase/write/lock/
    quad-enable code linked, no SR write at probe, `sf` = probe + read only, and
    `SPI_MEM_READ_ONLY` refuses every SPI op that isn't a read with data-in. The
    factory `sf probe` writes SR 0x00 (spi-recovery.md §5); ours writes nothing.
  - **0025 `alpine-v2`:** MUIO replay from a DT node at board_init (mux_sel 4 bit/pin,
    8 pins/reg; RMW of the listed pins only), CCU (CCI SI4 snoop |= 1, speculation 7)
    in board_prep_linux, `alpine state|ccu|muio`.
  - **0026 `dmc12`:** DT under `#ifdef`: MUIO pins 0, 1 (SPI CS1/CS2), 20, 21 (UART1)
    fn 2 = the factory's `pinctrl_init` result (u01 dump: mux_sel_0 0x22, mux_sel_2
    0x00220000); PCIe port 0 gen1 x1.
  - **0027 `alpine-v2` / 0028 `dmc12`, SoC GPIO 22 high** (not yet run on hardware). The
    factory `board_init` runs `dt_based_init_gpio` (0x01101d50) right after the pinctrl
    and before SGPO and PCIe: for each dt.0 `gpio_init gpio-list` triplet (only
    `<0x16 0x01 0x01>`: GPIO 22, out, 1) `gpio_board_init` (0x01107f50) gives MUIO pin 22
    the GPIO function (iface 0x3a, fn 0), then `gpio_direction_output` (0x0115d998)
    sets PL061 DIR, then DATA. Ours: MUIO list gets `<22 0>`, and an
    `amazon,alpine-v2-gpio-init` node (same triplet, the 6 PL061 banks by phandle) is
    applied in board_init after the MUIO, before PCI, value first, then direction (no
    glitch to the stale latch). Boot line `GPIO:  22 out 1 (was out 1)` after the factory;
    `alpine state` shows `GPIO 22: out 1 (DT wants out 1)`. GPIO 22 is PL061 @0xfd889000
    line 6 = `CPLD_JTAG_EN`, the line `cpldupd` uses around MachXO3 JTAG updates;
    the factory-initialised state is `dir 40 data cf` (line 6 out, high).
  - Not done: SGPO (the DMC-12 board-cfg has no `sgpo_init`, so the factory never
    writes it; `alpine state` only reads it), USB in U-Boot, the fan/WDT CPLD preamble.
- `#uboot-dmc12-stage3app` (`/nix/store/30jizg0ps4098axaxif9vyddxksqyadx-uboot-dmc12-stage3app`):
  that u-boot.bin, byte for byte, wrapped by [alboot.py](../../nix/packages/alboot/alboot.py)
  (moved into nix so the build can use it; the device copy is a symlink) as IPXE
  objects (id 0x110), load = exec = 0x08000000 (`u-boot-ipxe-08000000.fimg`, sha256
  `2c00d920…`) and 0x01100000 (`…-01100000.fimg`, `be4995ac…`). 0x7f0d4 B each:
  3884 B left in the 512 KiB IPXE slot, so growth goes to UBOOT 1 (0xe0000) or needs
  trimming (MTD core, `mii`, LZMA are candidates). Same binary as the RAM test: the
  probe policy (keep / init) and the idempotent MUIO/CCU make it entry-agnostic.
- Offline tests (`#uboot-alpine-v2-tests`):
  - **pcie-init:** `al_init_pcie_rc.c` out of patch 0022 + the verbatim HAL, host build
    against a register model (memory-backed unit + PBS; LTSSM L0 gen1 x1 while the
    port is enabled and LTSSM_EN set). Cold (all zero): link up, sequence
    enable/disable/enable/start, and the AXI words the init owns equal the factory's
    u01 dump (`+0x00 +0x14 +0x18 +0x20 +0x30 +0xc8 +0xe0 +0xe8 +0xec`, incl. IB credits
    `02844c3c` from the HAL's own table). Warm (the dump state, link up): stop, disable,
    enable, disable, enable, start, and those words end bit-identical. Stuck link:
    error at "link up wait" after 1 s, port left stopped/shut down/disabled. Also: the
    21 HAL files in patch 0022 are identical to `al-pcie-hal/`.
  - **selfinit:** config (PCIe/MUIO/CCU/read-only SF on; no SF lock/unlock/SFDP/soft
    reset, no `sspi`/`mtd`, env nowhere); symbols present (drivers, commands, init,
    SError drain, -EROFS stubs) and absent (`spi_nor_erase`, `spi_nor_write`,
    `spi_nor_write_data`, `write_sr`, `macronix_quad_enable`, `stm_lock`,
    `spi_flash_update`, the `sf` erase/protect/test handlers); no `sf write|erase|
    update|protect` help text; DT (MUIO, PCIe props, SSI compatible, 25 MHz flashes;
    the chainload DT has none of it); both `.fimg` carry u-boot.bin verbatim and fit.
  - Not covered offline: the DM PCI glue, the SError drain, the SSI and the CCI —
    QEMU models none of them. `write_enable` is still linked (octal-DTR/4-byte paths
    this chip never takes; SPI_MEM_READ_ONLY refuses the opcode anyway).

### RAM test (main agent; nothing written to flash)

From an idle `ALPINE_DB>` (`./powercycle_to_uboot.py`):
`./chainload_uboot.py nixos-build/result-uboot-dmc12-selfinit --selfinit` (add `--boot`
for a disk boot). By hand, after the usual factory-side `dhcp; tftpboot 0x10000000
dmc12-uboot/u-boot.uimage; bootm 0x10000000 - ${fdtaddr}`. The harness runs the
read-only, SPI and CCU steps first and `alpcie reinit` last (the table's order differs):

| `=>` command | expect |
| --- | --- |
| (boot log) | `U-Boot 2026.10-dmc12-selfinit`, `MUIO:  5 pins from the DT, 0 changed`, `GPIO:  22 out 1 (was out 1)`, `PCIe 0: link up 2.5GT/s x1 (firmware init, kept)` |
| `alpcie status` | `port enabled, link up 2.5GT/s x1, LTSSM 0x11`, link partner `12d8:400c` |
| `alpcie reinit` | `PCIe 0: link up 2.5GT/s x1 (our init after teardown, LTSSM 0x11)` and the status block; exit 0 |
| `pci 2` | `02.00.00 0x12d8 0x400c Bridge device` |
| `pci 3` | the OHCI/EHCI functions behind the bridge |
| `alpcie status` | `SErrors drained: N` (N > 0 = CRS/UR seen and absorbed; note it) |
| `sf probe 0:0` | `SF: Detected mx25u12835f with page size 256 Bytes, erase size 4 KiB, total 16 MiB` |
| `sf read 0x40000000 0 0xfd0000` then `crc32 0x40000000 0xfd0000` | `Read: OK`, `==> 08145ce6` (mtd0 below the env pair) |
| `sf probe 0:1`, `sf read 0x40000000 0 0x1000000`, `crc32 0x40000000 0xfd0000` | `==> 3d086ac6`; full 16 MiB `db9e4937` if CS1's env is untouched |
| `alpine state` | `SI4 snoop c0000001`, `spec 00000007` (factory bootm); pins 0/1/20/21 `function 2 (DT wants 2)` |
| `mw.l 0xf0090004 0; mw.l 0xf0094000 0xc0000000`, `alpine ccu` | `snoop c0000000 -> c0000001, speculation 0 -> 7` |
| `bootflow scan -lb` (or `--boot`) | `CCU:   snoop c0000001 -> c0000001, …` right before `Starting kernel`; then in Linux `./smoke.py` (48 ttyUSB = the re-initialised link works for Linux) |

Stop on the first miss. If `alpcie reinit` fails, the port is left disabled (vendor
teardown) and the USB ports are gone until a power-cycle. A `spi-mem: … refused` line
means some code tried a non-read SPI command; capture it. Useful extra reads at the
factory prompt for comparison: `md.l 0xfd8108d0 1` (DW AMBA error response),
`md.l 0xfd810900 1` (iATU viewport).

Risks: the factory-state model is our reading of the HAL and the Ghidra template, not a
register dump of the DBI space (none was taken); after a PBS port disable every core
register the HAL doesn't write is back at reset value, and that is exactly what the
reinit test exercises. The SError drain swaps VBAR_EL2 for a few instructions. The Gen3
EQ table has 16 entries as in the factory, which writes past the x8 lane registers (as
the factory does). The HAL's Amazon 2017 files carry no licence header; they ship in
MikroTik's GPL kernel source (licence call for the owner before anything is published
upstream).

## persistent environment in SPI (built 2026-11-15, not yet run on hardware)

`#uboot-dmc12-selfinit` keeps its environment in SPI CS0, so `saveenv` at `=>` persists.

- **Where:** redundant pair, 0x10000 B each, at **0xf90000** and **0xfa0000** (ENV_SIZE =
  ENV_SECT_SIZE = 0x10000, bus 0 CS 0, 25 MHz). That is the blank tail of the BOOT_APP 1
  slot: its object ends at 0xb53a11, flash is 0xff from 0xb5f000 to the slot end at
  0xfb0000. The factory env pair at 0xfd0000/0xfe0000 is never touched; the factory
  U-Boot (stage3 `u` escape) keeps using it.
- **Write window (patch 0029 `alpine-v2`, generic code; the dmc12 values are in the
  selfinit config):** `SPI_FLASH_WRITE_WINDOW` = bus 0, CS 0, 0xf90000 + 0x20000. Two
  independent checks share `include/spi_flash_window.h`:
  - spi-nor: erase/program run only if the whole request is on that chip and inside the
    window. Otherwise `SF: erase|write … outside the write window, refused`, -EROFS, no
    command sent.
  - spi-mem: on that chip only, WREN/WRDI, page program inside the window, and 4K/32K/64K
    erase whose aligned block is inside it. Everything else that isn't a read is refused
    on every chip: SR/CR writes, chip erase, reset, lock, OTP, 4-byte mode.
  - Still absent: lock/unlock, quad enable, SR write at probe, BAR, soft reset. `sf` stays
    probe + read: only the env driver writes.
- **Boot with a blank or corrupt env:** both copies fail the CRC, so you get `*** Warning -
  bad CRC, using default environment`. The compiled-in default (`bootcmd=bootflow scan -lb`
  etc.) loads and boots as before. No `ENV_IS_NOWHERE` fallback: with it, `saveenv` would
  target "nowhere" after a bad-CRC load.
- **Caveat:** once an env is saved, changes to the compiled-in defaults in a later build
  (e.g. `bootcmd`) do not apply until `env default -a; saveenv`.
- **Recovery:** at `=>` run `env default -a; saveenv`. From the factory prompt (stage3 `u`
  escape) run `sf probe 0:0; sf erase 0xf90000 0x20000`. The factory `sf probe` writes SR 0
  (spi-recovery.md §5), and the window is blank in the backup, so this is a clean reset to
  defaults.
- **Offline proof (`#uboot-alpine-v2-tests`):**
  - `spi-window`: the header on the host. Every boundary of the window, the factory env and
    UBOOT ranges, SR/chip-erase/lock/reset opcodes, and 2M random requests against a
    128-bit reference model.
  - `selfinit`: the env config, both copies inside the window and clear of 0xfd0000,
    the window inside the blank BOOT_APP 1 tail, and the refusal string.
  - In this build `spi_nor_erase`/`spi_nor_write` are inlined into the window hooks (no
    symbols); if they ever stop being inlined, the test requires that only those hooks
    call them and that no relocation takes their address.
  - Not proven offline: the DW SSI write path (WREN/PP/erase on the wire), erase timing,
    and the env load/save itself on hardware.

### RAM test (main agent)

Read and CRC the window first and keep the result: the 0x20000 B are blank in the backup
(`crc32` of 0x20000 × 0xff = `154803cc`). Then:

```
=> sf probe 0:0; sf read 0x40000000 0 0x1000000
=> crc32 0x40000000 0xf90000; crc32 0x40fb0000 0x50000     # outside the window: keep these
=> crc32 0x40f90000 0x20000                                 # expect 154803cc (blank)
   (boot log of the new build: "Loading Environment from SPIFlash... *** Warning - bad CRC, using default environment")
=> printenv bootcmd                                         # bootflow scan -lb
=> setenv foo bar; saveenv                                  # "Saving Environment to SPIFlash... Erasing ... Writing ... OK"
=> sf read 0x40000000 0 0x1000000; crc32 0x40000000 0xf90000; crc32 0x40fb0000 0x50000   # unchanged
=> saveenv                                                  # the second copy (0xfa0000)
   (re-run the new build: RAM chainload, or the SPI one after the update)
=> printenv foo                                             # foo=bar, no bad-CRC warning
=> env default -a; saveenv                                  # back to defaults; then boot: bootflow scan -lb
```

There is no prompt command that asks for a write outside the window: that path exists only
for code. If an on-hardware refusal check is wanted, a RAM-only test build with
`sf erase` enabled could try `sf erase 0xf8f000 0x1000` and `sf erase 0xfb0000 0x1000`,
both blank on u01 (harmless even if the check failed), expecting `outside the write window,
refused`. Not built.

## design (why it is shaped like this)

- **Mainline U-Boot has no Alpine support** (checked: the 2026.10 tree has no
  `mach-alpine`, no board, no driver; "alpine" only in mach-types and docs). Annapurna's
  `alpine_db` U-Boot 2015.07 is GPL, but a quick search found no published source.
  Places to look: QNAP GPL drops for AL-314/AL-324 NAS (TS-x31X/x32X), Netgear R9000 /
  ReadyNAS, Synology DS416, or a request to Edgecore. Nothing here depends on it.
- **No low-level init.** al-boot stage2/3 has trained DRAM, set up SerDes and the PCIe
  unit adapters, and installed the PSCI v0.1 monitor. Secondaries stay with the firmware,
  and Linux starts them with PSCI `cpu_on` as before. U-Boot itself makes no SMC.
- **Entry.** `bootm` of a *kernel-type* uImage (`-A arm64 -O linux -T kernel`, load =
  entry = 0x08000000) makes the factory loader clean up as it does for Linux: caches and
  MMU off, entry at EL2 with x0 = its DTB. `-T standalone` would not, and `go` doesn't
  either. For `go`, `save_boot_params` (patch 0014) cleans the D-cache by set/way and
  turns MMU/caches off before start.S touches memory. The banner line
  `Entry: EL<n>, MMU+caches were on (go)/off (bootm), x0=…` says which path ran. It also
  answers "**at which EL does the factory U-Boot run**". We expect EL2: the vendor kernel
  logs "All CPU(s) started at EL2", and the 2015 armv8 bootm only drops EL3→EL2, never
  further. The string `got EL3 abort!` sits in al-boot stage3, so EL3 is probably the
  firmware's.
- **Position independent**, linked at 0x08000000 (no fixups when loaded there). It also
  carries a Linux Image header, so a later build can be `booti`'d from this one.
- **Memory.** Banks come from `/memory` in `alpine-v2-dmc12-u-boot.dtsi`: 0–3 GiB and
  8–9 GiB, as the vendor's DT has them. The MMU map is DRAM = Normal plus every hole
  below 4 GiB = Device. The firmware ranges come from the vendor loader's `/memreserve/`
  list (0x0 +1 MiB, 0x02000000 +1 MiB, 0xb0000000 +256 MiB; contents unknown). They are
  `/reserved-memory` in the U-Boot DT: lmb won't load over them, U-Boot relocates below
  0xb0000000, and `ft_board_setup()` adds them to the reserve map of **whatever DTB
  Linux gets**. The kernel-tree DTS has none of them. The vendor's other two memreserves
  are only its own DTB and initrd.
- **Reset** (`reset`, panic): sysreset tries the board CPLD first (i2c-4 / 0x68: 0x29←1,
  then 0x28←1, as the vendor kernel does). If the board is still alive after 1 s, it
  falls through to the SoC sp805 (`wdt-reboot`, load 0). There's no PSCI SYSTEM_RESET
  (the firmware has none). The sp805 is never started otherwise, so Linux doesn't
  inherit a running watchdog. Untested on hardware.
- **Network:** see [network](#network-al_eth-built-not-yet-run-on-hardware). The default
  boot stays SATA only: there is no ethernet bootdev, so `bootflow scan` never DHCPs.

## load addresses

| range | owner |
| --- | --- |
| 0x0 +1 MiB, 0x02000000 +1 MiB | firmware (reserved) |
| ~0x03B00000–0x04000000 | factory U-Boot (stack 0x03B04CD0, `fdtaddr` 0x03B06010, relocaddr 0x03F0A000) |
| 0x04078000 +28 KiB | factory DTB copy (`loadaddr_dt`) |
| **0x08000000–0x080E3CB0** | **this image** (code + BSS + 512 KiB early stack; dead after relocation; with Ethernet) |
| 0x0f000000 / 0x0f100000 / 0x0f200000 / 0x0f300000 | `fdt_addr_r` / `fdtoverlay_addr_r` / `scriptaddr` / `pxefile_addr_r` |
| 0x0fd00000 | factory initrd staging (not used by us) |
| 0x10000000 | uImage TFTP staging (factory side), then `kernel_addr_r` |
| 0x20000000 +64 MiB | `kernel_comp_addr_r` (Image.gz) |
| 0x30000000 | `ramdisk_addr_r` |
| just below 0xb0000000 | relocated U-Boot, malloc, its FDT |

`fdt_high`/`initrd_high` = `0xffffffffffffffff` (patch 0017): `booti` uses the FDT and
initrd where they were loaded, no copy. The value must be 64-bit; `0xffffffff` would be
read as a 4 GiB limit.
| 0xb0000000 +256 MiB | firmware (reserved) |

## network: al_eth (built, not yet run on hardware)

The port is integrated Ethernet port 1: PCI 00:01.0 `1c36:0001` rev 2, RGMII at a fixed
1000/full to the CPLD-managed uplink PHY pair. It has no MDIO PHY and no SerDes.
BARs: 0 = UDMA (128 KiB), 2 = MAC (4 KiB), 4 = EC (16 KiB). U-Boot assigns them inside
0xfe000000 +16 MiB, and Linux moves them again.

**Handoff to Linux (patch 0018, also in builds without a network stack).** Linux
al_eth reads three "board params" scratch registers (MAC BAR +0x4, +0x404, +0xc) and the
MAC address (EC forwarding entry 0, +0x868/+0x86c). Without the params, probe fails
("board info not available"). Without the MAC, it picks a random one. An FLR clears
both. In `board_prep_linux` (just before "Starting kernel"), for every 1c36:0001
function:
- **Params** already there are kept; a difference from the DT is printed, not acted on.
  Missing params are written from the function's DT node (`phy-mode` + `fixed-link`),
  encoded as the HAL's `al_eth_board_params_set()` does. On the DMC-12 that is
  `60000001 00000004 00000000`: rgmii, no PHY, autoneg off, 1000, full, ref-clk
  500 MHz. ref-clk is inferred; only the MDIO divider uses it.
- **MAC**, first that exists: env `ethaddr`, what the EC already holds, the board
  EEPROM's ONIE base MAC (TLV 0x24, CRC checked; it equals the factory env's `eth1addr`).
- `aleth info` shows BARs, params and MACs (read-only). `aleth handoff` runs the
  handoff by hand.

**Driver (patch 0020).** The device is `al_eth1`, named like the vendor U-Boot's. It runs
on Annapurna's HAL, copied verbatim; only the platform shim is new, and its `al_assert`
reports instead of halting. The order follows Linux al_eth:
- **start:** first quiesce whatever ran before: MAC stop, all 8 queues reset, UDMA
  disable, FLR keeping params + MAC. Then adapter init, MAC config from the board
  params, link config 1000/full, one TX and one RX queue, RX forwarding (our unicast and
  broadcast, the rest dropped), 63 RX buffers of 1536 B, MAC start.
- **stop and remove:** both quiesce the same way. remove runs before the OS
  (`DM_FLAG_OS_PREPARE`), so Linux finds the function idle with params and MAC in place.
- **probe:** maps the BARs, runs the handoff, allocates. Nothing touches the data path
  before a network command, so a driver fault can't break a disk boot.
- **DMA:** cached memory with explicit flush/invalidate. TX goes through an aligned
  bounce buffer padded to 60 B. The MAC's link bit is printed at start but not trusted:
  Linux turns the carrier on unconditionally for this port.

Commands: `dhcp`, `tftpboot`, `ping`, `wget`. `wget` is the legacy stack's: HTTP only,
the server given as an IP, the port from env `httpdstp` (default 80). There is no env
storage, so nothing persists.

### hardware test plan (main agent; RAM only, nothing written)

Run each step only after the one before it passed. Start from an idle `ALPINE_DB>`
(`./powercycle_to_uboot.py`).

1. **Factory side, before any chainload** (read-only; answers question 1 in
   `uboot-vendor-commands.md`):
   1. `pci header 0.1.0` for the BARs.
   2. `md.l` MAC BAR +0x4, +0x404, +0xc and EC BAR +0x868, +0x86c.
   3. Repeat 2 after `setenv autoload no; dhcp`.

   Do the first reading straight after the power-cycle, before any `dhcp`. Expect params
   at or near `60000001 00000004 00000000`. Record the first word: it settles ref-clk.
   Don't paste the EC words (the MAC) into tracked files.
2. **Handoff only:** `./chainload_uboot.py nixos-build/result-uboot-dmc12-eth-a --cmd
   'aleth info::board params'`. Expect:
   - `board params (present): …` with the factory values;
   - a valid `EC MAC:` (the chainload ran `dhcp`), equal to the `EEPROM MAC:` line.

   Then `aleth handoff` must report "kept" for both.
3. **Driver probe:** the same with `…-eth-b`. Expect the banner, an `al_eth 00:01.0:`
   line and `Net:   al_eth1`, with no "random MAC" or "No valid MAC" warning.
   `aleth info` as in step 2.
4. **Data path:**
   - `setenv autoload no; dhcp`: expect `al_eth1: RGMII 1000M full, link bit …` and a
     lease in 10.60.2.0/24.
   - `ping 10.60.2.208`: expect "is alive".

   On failure, note what the link bit said and capture `aleth info`.
5. **TFTP:** `setenv serverip 10.60.2.208; tftpboot ${kernel_addr_r}
   dmc12-uboot/eth-b/u-boot.bin; crc32 ${kernel_addr_r} ${filesize}`. Compare with the
   host: `python3 -c 'import zlib,sys;print(hex(zlib.crc32(open(sys.argv[1],"rb").read())))' <file>`.
   Then repeat with a large file, `dmc12/uInitrd` (~300 MB), and note the rate U-Boot prints.
6. **HTTP:**
   1. On the host: `python3 -m http.server 8000 --bind 10.60.2.208 --directory
      /srv/tftpboot`. The port must be open in the host firewall (8000 and 8080 are; 8608 was not, 2026-11-15). Check the
      host firewall.
   2. At `=>`: `setenv httpdstp 8000; wget ${kernel_addr_r}
      10.60.2.208:/dmc12-uboot/eth-b/u-boot.bin`, then `crc32` as in step 5.
   3. Then the uInitrd, for the rate.
7. **Linux after U-Boot networking (the handoff end to end):** netboot NixOS as below.
   In Linux, expect:
   - `al_eth … Board info: phy exist No … media 1`;
   - the board MAC on eth0, not a random one;
   - eth0 up at 1000 with DHCP.

   Then `./smoke.py`.
8. **Disk boot unaffected:** `bootflow scan -l` lists only SCSI bootflows (no
   ethernet, no DHCP).

If step 4 gets no RX at all, the likeliest causes, in order:
1. RX forwarding: compare MAC entry 0 (`md.l <EC BAR>+0x868 2`) with the MAC.
2. RGMII clocking/delays: the factory U-Boot has `al_cmos_rgmii_dll_*` code that this
   HAL lacks (see risks).
3. Cache maintenance.

### netboot NixOS through it

The existing TFTP bundle boots unchanged from upstream U-Boot, which has `bootm` and
`source`. The bundle is `dmc12-netboot` in `/srv/tftpboot/dmc12/`: legacy uImage,
uInitrd, dmc12.dtb, boot.scr.

```
=> setenv autoload no; dhcp; setenv serverip 10.60.2.208; setenv dmc12dir dmc12
=> tftpboot 0x07000000 dmc12/boot.scr; source 0x07000000
```

Over HTTP, use the server from step 6 with the same files and the same addresses as
boot.scr. Take the bootargs from the bundle's `bootargs.txt`; the console buffer is
1024 B, so typing them works when paced (console.py).

```
=> setenv autoload no; dhcp; setenv httpdstp 8000
=> wget 0x10000000 10.60.2.208:/dmc12/uImage
=> wget 0x20000000 10.60.2.208:/dmc12/uInitrd
=> wget 0x04078000 10.60.2.208:/dmc12/dmc12.dtb
=> setenv bootargs '<contents of bootargs.txt>'
=> bootm 0x10000000 0x20000000 0x04078000
```

`bootm` moves the kernel to 0x04200000, the uImage load address, clear of everything
this U-Boot still uses. The natural next step for `nix/packages/dmc12-images` is a
raw-`Image` + `booti` bundle (kernel at `kernel_addr_r`, initrd at `ramdisk_addr_r`,
FDT at `fdt_addr_r`). It isn't built yet.

## first hardware test (main agent runs it; RAM only)

Artefacts are staged in `/srv/tftpboot/dmc12-uboot/` (TFTP served, sha256 checked).
The box must be at an idle `ALPINE_DB>` (`./powercycle_to_uboot.py`). Then either the
harness, which writes `serial-chainload-<date>.txt` and exits 0 only on PASS:

```sh
./chainload_uboot.py nixos-build/result-uboot-dmc12-chainload           # bootm path, read-only checks
./chainload_uboot.py nixos-build/result-uboot-dmc12-chainload --go      # MMU-on path
./chainload_uboot.py … --reset     # then also: reset via the CPLD, wait for stage2
./chainload_uboot.py … --boot      # then also: bootflow scan -lb (needs extlinux on sda1)
```

or by hand:

```
ALPINE_DB> setenv autoload no; dhcp
ALPINE_DB> setenv serverip 10.60.2.208
ALPINE_DB> tftpboot 0x10000000 dmc12-uboot/u-boot.uimage
ALPINE_DB> iminfo 0x10000000
ALPINE_DB> bootm 0x10000000 - ${fdtaddr}
#  "go" variant:  tftpboot 0x08000000 dmc12-uboot/u-boot.bin; go 0x08000000
```

The `- ${fdtaddr}` (the factory's live DTB) is only there because the 2015 arm64 bootm has
no ATAGS fallback and stops without an FDT (from the 2015 source, not tried). The new
U-Boot ignores it and uses its own DT. Expect `U-Boot 2026.10-dmc12-chainload`, the `Entry:` line,
`DRAM: 4 GiB`, a 3 s countdown (any key), then `=>`. Useful read-only checks: `bdinfo`
(relocaddr < 0xb0000000), `i2c dev 4; i2c md 0x68 0x01 1` (CPLD version 0x88),
`pci` (1c36:0031 AHCI), `scsi scan`, `part list scsi 0`, `ls scsi 0:1 /boot`,
`bootflow scan -l`.

Interrupting the factory autoboot skips its preamble, so the fans stay at full speed
(safe, loud). `run clear_wdt_flag; run fan_speed_70` at the factory prompt (both are
I2C writes to the CPLD) quiets them first, if wanted.

## booting Linux from it

- Standard boot: `bootflow scan -lb` (the default `bootcmd`) finds
  `/boot/extlinux/extlinux.conf` or `/extlinux/extlinux.conf` on any SCSI partition. It
  loads `KERNEL` (a raw `Image`), `INITRD` and `FDT`, or `FDTDIR` +
  `fdtfile=amazon/alpine-v2-dmc12.dtb`. With no FDT line it uses its own DT, which is the
  Linux board DTS plus the U-Boot additions. It fixes up `/memory` and the reserve map,
  then `booti`s at EL2.
- By hand: `ext4load scsi 0:1 ${kernel_addr_r} /boot/Image; ext4load scsi 0:1
  ${ramdisk_addr_r} /boot/initrd; setenv rd_size ${filesize}; ext4load scsi 0:1
  ${fdt_addr_r} /boot/alpine-v2-dmc12.dtb; setenv bootargs console=ttyS0,115200
  earlycon=uart,mmio32,0xfd883000 …; booti ${kernel_addr_r} ${ramdisk_addr_r}:${rd_size}
  ${fdt_addr_r}`.

## on-disk boot (built, not yet run on hardware)

`hardware.dmc12.bootLoader = "chainload"`
([boot-chainload.nix](../../nix/modules/boards/dmc12/boot-chainload.nix),
[disk.nix](../../nix/modules/boards/dmc12/disk.nix)); output
`path:./nix#dmc12-disk-image-chainload`. The default image stays `uboot-menu`.
The factory U-Boot never reads ext4 in this mode (its ext4 is flaky, TFTP is slow).

| part | LBA / size | content |
| --- | --- | --- |
| sda1 | 2048, 8 MiB, MBR type `0xda` | raw: U-Boot uImage slot A at LBA 0x800, slot B at 0x2800 (4 MiB each) |
| sda2 | 18432, 512 MiB, bootable | ext4 `DMC12BOOT` (default features) = /boot: `extlinux/extlinux.conf`, `nixos/*` |
| sda3 | after it | ext4 `DMC12ROOT` = /, grown on first boot |

1. **Factory U-Boot** env (reimage.py writes it from the image's `factory-env.txt`):
   `bootcmd=run nixos_boot; run dmc12_bootdisk` and
   `nixos_boot=run dmc12_boot_preamble; scsi init; scsi read 0x10000000 0x800 0x2000 && iminfo 0x10000000 && bootm 0x10000000 - ${fdtaddr}; <same for 0x2800>`.
   The preamble is what the factory boot runs (DN0 SATA SerDes fix, WDT flag, fans 70%,
   LED, uplink sync, `bootp` with `autoload=n`): `bootp`'s al_eth halt leaves the board
   params + MAC that Linux al_eth needs (`uboot-vendor-commands.md`). `scsi read` takes
   hex address, start block, count. Both slots bad → falls through to `dmc12_bootdisk`.
   The `setenv` line is 233 chars typed; an eval assertion keeps it under 254.
2. **`bootm 0x10000000 - ${fdtaddr}`**: the same entry as the hardware-tested TFTP path.
3. **Upstream U-Boot**: `bootflow scan -lb` → sda2 (bootable flag) →
   `/extlinux/extlinux.conf` from nixpkgs' extlinux builder: raw `Image` at
   `kernel_addr_r` 0x10000000, initrd at 0x30000000, `FDT ../nixos/…-dtbs/amazon/alpine-v2-dmc12.dtb`,
   3 s menu, 10 generations → `booti` at EL2.

Updates: the installer (activation) rewrites /boot via the extlinux builder, then, if
the U-Boot image changed, copies slot A to B and writes the new image to A. It refuses
unless sda1 is exactly this partition (start, size, type `0xda`). The disk image has
the same image in both slots.

- **Checked offline** (2026-11-15): see PROGRESS entry; partition table, both slot
  headers (`dumpimage -l`), extlinux.conf, root closure.
- **Not yet run:** factory `scsi read` of our slot; Linux via `booti` from disk;
  al_eth after the preamble's `bootp` on this path.
- **Recovery:** the factory countdown (5 s) still comes before `bootcmd`; from that
  prompt `run dmc12_bootdisk` or the TFTP chainload above. A U-Boot that hangs after
  `bootm` can't fall back by itself: power-cycle and interrupt.

## risks / unverified

- What has run on u01 is in the status at the top (console, I2C/CPLD, PCI, AHCI, CPLD
  reset via `bootm`). Everything else here is build- or QEMU-tested only.
- Arch timer: U-Boot uses `CNTFRQ_EL0` as the firmware left it. The vendor DT also
  carries `clock-frequency = 66.66 MHz`. If CNTFRQ were 0, delays and the autoboot
  countdown would break. In that case: hang after `DRAM:`, or a countdown that never ends.
- `go` path: set/way clean on the A57 cluster is the standard method but unproven here.
  Prefer `bootm`.
- The DM serial re-inits UART0 from the DT clock: 500 MHz, divisor 271, 115313 baud
  (+0.1%). A wrong clock shows as garbage after the banner. The debug UART (no re-init)
  prints `<debug_uart>` first.
- PCI: U-Boot re-assigns BARs on the internal ECAM bus inside 0xfe000000 +16 MiB. Linux
  does this again anyway (`pci=realloc`). External PCIe (USB hubs) is not touched.
- 0x02000000 +1 MiB holds the al-boot trace buffer (`uboot-vendor-commands.md`). The
  other ranges' contents are unknown, and so is whether other Alpine units or
  al-boot versions use the same ones. They stay board-level (dmc12 dtsi) until then.
- Old kernels without Image-header flag bit 3 would be relocated by `booti` to DRAM base
  0x0, which is firmware. Every current kernel sets the bit.
- If the SSD holds a `boot.scr`, `bootflow scan -lb` runs it. The first iteration's
  script calls `bootmenu`, which isn't compiled in, so it fails without effect. u01's
  sda1 is not known to have one.
- The uC watchdog (10 s, "enabled") never fired in the factory U-Boot, which never
  services it either (disassembly). Watch for an unexpected reset while sitting at `=>`.
- **On-disk boot:** the DN0 SATA SerDes write comes from `dmc12_boot_preamble`, which
  `nixos_boot` runs first (`uboot-vendor-commands.md`). al_eth's board params and MAC
  come from it too, and now also from the upstream handoff (patch 0018) whenever they
  are missing. Unverified on hardware. Without a DHCP answer, `bootp` times out first,
  which only slows the boot.
- **Network (patches 0018–0021), none of it run on hardware:**
  - RGMII timing. The factory U-Boot contains `al_cmos_rgmii_dll_start` /
    `_delay_set`, which this HAL (and Linux al_eth) lacks. Linux works after the
    factory's init, and so should this, if the DLL state survives the factory's FLR the
    way it does for Linux. A board whose NIC the factory never touched is untested.
  - DMA coherency is not assumed. Rings and buffers get explicit cache maintenance; a
    bug there shows as RX garbage or TX timeouts, not a hang.
  - ref-clk in written params is inferred (500 MHz). Firmware values win when present.
  - Legacy `wget` is HTTP only, and nobody has measured whether it is fast enough for a
    300 MB initrd. TFTP uses blocksize 1468, window 1.
  - The control DT now has a `/soc/pci@fbc00000/ethernet@1,0` node. A Linux booted
    with U-Boot's own DT sees it; the out-of-tree al_eth ignores DT.
