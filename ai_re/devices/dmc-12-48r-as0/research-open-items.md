# DMC-12 u01: open research items (2026-11-15)

Worked from the dumps, the factory OS (initramfs 1.0.457/1.0.526 and sda1, extracted to scratch), the
vendor HAL (`../cs8210-32x/analysis/research/mikrotik-added/drivers/soc/alpine/HAL/`, "HAL" below) and
read-only reads on u01. No writes to the CPLD, uC, SPI or any I2C device. Captures:
`inventory/u01/i2c-scan-20261115.txt`, `inventory/u01/research-reads-20261115.txt`.
One slip: the scan probed the CPLD (4-0068) through child buses 5–8, four SMBus receive-byte reads
and no writes. CPLD and fan metrics stayed the same afterwards. Details are in the scan file.

Factory binaries cited: `cpldupd` sha256 `fb12e166…` (the same in 457 and 526), aarch64, static, not
stripped; addresses are its ELF addresses. Factory U-Boot = `ghidra/dmc12.gpr`, stage2 =
`ghidra/stg-rec` (`stg2a.bin`).

## 1. What selects CS1 (mtd1)
**Finding:** the K2 uC does, through its GPIO **port 2 bit 2**. The HAL calls it
`UC_SPI_TOGGLE_GPIO_{PORT_V1_V2=2, BIT_V2=2}` (`services/uc_shared/al_uc_cmds.h:24`; V3 uses bit 3). The
factory `uc_get/set_recovery` read and write exactly that bit. The HAL calls the two chips the **"main"
and "recovery" flash**. It also has later uC commands that this firmware doesn't take on this card:
`SPI_FLASH_SET/GET` 0xbaba6b6b/7b7b, `SPI_BOOTED_FROM` 0x8b8b (the factory U-Boot refuses it, "dev id 1"),
and an **EP/RC auto-detect** (0x9c9c/0x9d9d) that picks main or recovery from the **PCIE_WAKE** signal
(`al_uc_cmds.h:1245`). The straps can't do it (`pbs/include/al_hal_bootstrap.h`: `boot_device` has no
chip-select field). Nor can the CPLD: it sits on the main board, and no factory tool touches a related
register.
- CS1's content fits "recovery": stock Annapurna K2C-AB al-boot 2.44 with an **endpoint** DT
  (`ep-ports <0>`) and the PCIe-EP app APCEA. That is the image of a plain K2C-AB NIC card.
- u01 state (already in `spi-recovery.md`): "recovery disabled", GPIO 2.2 output 0.
- **Confidence:** high that the uC owns the select (HAL names it, and U-Boot's code uses it). Unknown:
  whether it swaps the two chips or only re-points SS0, whether it survives a power cycle, and whether
  uC 1.35.0 runs the PCIE_WAKE auto-detect (if it does, the boot chip could change with no SoC command).
- **Next:** owner photos of the K2 card around both flashes (look for an analog switch or mux on the CS
  lines). The volatility test (`uc_set_recovery 1`, then power-cycle) belongs on a second unit with the
  1.8 V clip ready, never on u01 alone.

## 2. CPLD (Lattice MachXO3)
**Part:** cpldupd expects IDCODE **0x612BB043** (`.data` 0x4960b4, the `IDCODE` config item). That is
**LCMXO3LF-2100C** in openFPGALoader's part table (`src/part.hpp`): flash-based with UFM, C = 2.5/3.3 V
core. It isn't confirmed on silicon until the IDCODE is read.

**Hardened config I2C port: reachable, i2c-4 address 0x10.** In `cpldupd`, `dmc12_CPLD1_open` (0x400658)
does `I2C_request(4)` and probes `I2C_Read_8(0x10, 0)`; `dmc12_CPLD1_msg` (0x4006ac) sends every
sysCONFIG command as `I2C_nWrite/I2C_nRead(0x10, …)`. The scan sees ACKs at 4-0010, 4-0011 and 4-0013.
0x11 and 0x13 are unidentified (other EFB or fabric slaves?). The user register file stays at 0x68.
**So `cpldupd` updates the CPLD over I2C, not over JTAG.** Its flow uses the standard opcodes (IDCODE 0xE0,
transparent 0x74, offline 0xC6, erase 0x0E, program, verify, `PROGRAM_DONE` 0x5E, `REFRESH` 0x79,
`BYPASS` 0xFF; 3-byte commands for 0x74/0x26/0x79 as I2C requires). Around an update, the
"heatless" (hitless) control drives **SoC GPIO 22** (sysfs 494) **low** (strings: `- Enable heatless`,
`echo 0 > /sys/class/gpio/gpio494/value`). So GPIO 22 is the CPLD's hitless-update hold, active low,
pulled up when idle. That is why both DTs drive it high.

**Register map:** the factory tools (show, setled, setphyspeed, fancontrol, interruptmon, S99cpld_hook,
the reboot driver) use exactly the registers in `vendor-tooling.md`; nothing new is in 457 or 526. Not
explained (`inventory/u01/cpld-regs.txt`): 0x00=0x05, 0x10=0x3f, 0x12=0x07, 0x13=0x00, 0x14=0x1f,
0x15=0xff, 0x16=0x03. 0xcc is the fill of unimplemented addresses. Never sampled: 0x30–0x4f, 0x51–0x5e,
0x63–0xff. No CPLD image ships anywhere (both initramfs and sda1 searched).

**Readback plan (no JTAG needed).** It runs from u01 over `i2ctransfer` on i2c-4 0x10, with a strict
allowlist:
`0xE0` IDCODE, `0xC0` USERCODE, `0x3C` READ_STATUS (security bit), `0xFB`/`0xE7` FEABITS/feature row,
`0x74 08 00` transparent mode (user logic keeps running), `0x46` INIT_ADDRESS (CFG) / `0x47` (UFM),
`0x73` READ_INCR_NV (16 B per page; MachXO3LF-2100 = 3198 CFG + 639 UFM pages, *from the family
datasheet, check*), then `0x26` ISC_DISABLE and `0xFF`.
- **Never send:** 0x0E erase, 0x70 program, 0x5E done, 0x79 refresh (reloads the CPLD that runs fans,
  PSU and reset), **0xC6 offline mode (stops the user logic)**, 0xC2/0xE4/0xF8.
- If the security bit is set, the flash reads back blank. That is still worth knowing.
- It needs the **owner's OK**: the commands are writes to the config port. Run it as a reviewed script
  whose dry run prints the bytes first.
- **Tigard alternative (JTAG):** use it only if a header exists. No photo or note in the repo shows one.
  The MachXO3 JTAG pins sit on bank 0, likely 3.3 V: measure before setting Tigard's VTGT switch.
  - **openFPGALoader** (`-c tigard`): detect, IDCODE, status and feature row. It has no MachXO3 flash dump
    (only `Verify` against a file).
  - **openocd** (ftdi, Tigard channel B; `jtag newtap … -irlen 8 -expected-id 0x612bb043`): scripted
    `irscan`/`drscan` with the same opcode sequence.
  - **python pyftdi:** for a custom JTAG or I2C reader.
  - ecpprog does not do MachXO3.
  - None of these is added to `flake.nix`.
- **Confidence:** high for I2C 0x10 and the GPIO 22 role (code and scan), medium for the part number.

## 3. K2 uC firmware
- **Not on the SSD or in the OS:**
  - No updater and no image in either initramfs or sda1 (searched by name and for the HAL app-header
    magic 0xABCDDCBA, in either byte order; mtd0/mtd1 searched too).
  - The factory kernel only links the HAL `al_uc_shared_*` readers (VT-sensor data at 0xfd8a7f00) and
    never uses the mailbox.
  - The only update path is factory U-Boot `uc_update`, which needs an image in DRAM that nothing ships.
- **No software readback:**
  - The HAL opcode list (`al_uc_cmds.h:31–163`) has no uC-flash read.
  - 0xbaba9999 `SPI_PRELOADER_READ` reads only a "SPI preloader" blob kept in uC flash, up to
    `AL_UC_SPI_PRELOADER_MAX_SIZE`; not attempted.
  - The uC keeps two app images (`APP_IDX_GET/SET` 0x3e3e/0x3f3f, applied after an AC power cycle), and
    its images carry a 256-byte signature plus 512-byte auth info (`AL_UC_SIGNATURE_SIZE`,
    `SET_BL_AUTH_INFO`). Whether bootloader 1.13 enforces the signature is unknown.
- **Corrections from the HAL:** 0xbaba3535 = `FRU_SRAM_READY` and 0xbabaa9a9 = `ALPINE_I2C_CLEAN_FIFO`
  (`spi-recovery.md` §3 calls them "I2C / TPM access"). 0xcaca3737 `SET_SPI_BUFFER` switches the uC's SPI
  buffer between **"MOBO" (true) and "TPM" (false)** (`al_uc_cmds.h:998`).
- Possible further uC I2C faces, from the scan: 5-0036 (next to its LM75 face at 5-004a) is
  unidentified. 0x30 and 0x46 were left unprobed.
- **Next:** owner photo to identify the uC part and any SWD/debug pads. A Tigard SWD read only if that
  part's readout protection allows it; check its datasheet first.

## 4. SPI `APCEA` / `APCEA_CFG`
- **Where** (`inventory/u01/alboot-objects.txt`, factory dumps):
  - **APCEA (0x100):** blank in both CS0 slots (0x110000 and 0x3d0000, 512 KiB each). On CS1, v1.15.10,
    0x3f2dc B, load/exec 0xb8000000, data sha256 `b5c67b1e…` in both instances.
  - **APCEA_CFG (0x101, 0x7600 B):** CS0 v2.7.1 `9c7a74e2…`, CS1 v1.15.10 `f72308ec…`, identical across
    instances.
- **Layout (new):** APCEA_CFG is a 0x100-byte header plus **three 0x2500-byte records**. The uC has
  `SET/GET_APCEA_CFG_FLAVOUR` (0xbaba9090/9191), so these are most likely selectable flavours.
  - CS0 and CS1 differ in 69 bytes, at the same spots in each record: a small capability-style block at
    record +0x700, and the VPD keyword table at +0x2700 (`V1`…`V5`, each pointing into the EEPROM board
    data).
  - The difference in the table is a rotation: the 8-byte type-3 field is `V1` on CS0 and `V5` on CS1.
  - The PF header and VPD templates are in `boot-chain-details.md` §4.
- **Who reads them:** stage3 loads APCEA instance n after the DT and starts it at 0xb8000000 next to the
  app (`p` skips it, `e` keeps an early one). APCEA reads APCEA_CFG and the EEPROM. In stage2, stage3 and
  U-Boot the names appear only in the TOC name tables. On u01 **nothing consumes CS0's APCEA_CFG**: there
  is no APCEA, the stage2 is the k2 flavour and the DT has no EP port. Our U-Boot ignores both objects.
- **Confidence:** high for location and readers; medium for the "flavour" reading of the three records.
- **Next:** none needed. Keep CS0's APCEA slots blank (see the hazards).

## 5. `show power`: the INA219 reads 0
- **Device:** INA219 at **7-0044** (mux 0x71 ch2: factory `show` writes 0x04 to 0x71 by hand). It is in
  no DT, factory or ours, and is unbound. It ACKs in the scan.
- **Shunt = 1 mΩ:** factory `show` (526, sha256 `c9bf5b02…`) writes config `0x9f01` and calibration
  `0x00a0` as SMBus words, i.e. **config 0x019F** (16 V range, PGA /1 ±40 mV, 12-bit, continuous) and
  **calibration 0xA000 = 40960**. It then scales current by 1 mA/LSB and power by 20 mW/LSB.
  Cal = 0.04096 / (I_LSB × R) gives R = 0.04096 / (40960 × 0.001 A) = **0.001 Ω**.
- **Live reads** (`research-reads-20261115.txt`, word reads only):
  - config 0x399F (power-on default); calibration, power and current all **0**;
  - shunt ≈ 1.7 mV, bus ≈ 11.9 V. So about **1.7 A and 20 W** at R = 1 mΩ, which is plausible for this box.
- **Why it reads 0, two causes:**
  1. On the chip: calibration is 0 from power-up, so power and current stay 0 until someone writes it.
     Bus and shunt voltage need no calibration.
  2. **What `show power` prints is 0 because of our port, for every field, voltage included.** The NixOS
     `show` hands `0x…` strings to **gawk**, and gawk turns them into 0 (`awk -v v=0x5cf2 'BEGIN{print v+0}'`
     prints 0 on u01). The factory awk is busybox 1.29.3, which parses hex.
- **Calibration value, if one is ever written:** **0xA000** (wire order for `i2cset … w`: `0x00a0`) with
  config 0x019F, I_LSB = 1 mA, P_LSB = 20 mW. Not written here.
- **Kernel way (`ina2xx`, `CONFIG_SENSORS_INA2XX=m` in our kernel):** a node under `mux71_2`:
  `power-sensor@44 { compatible = "ti,ina219"; reg = <0x44>; shunt-resistor = <1000>; };` (µΩ).
  - At probe the driver writes config 0x399F and calibration **4096** (0x1000). From 1000 µΩ it derives
    I_LSB = 10 mA and P_LSB = 200 mW (`drivers/hwmon/ina2xx.c`, `ina2xx_set_shunt`/`ina2xx_init`).
  - **Without `shunt-resistor` it assumes 10 mΩ and reads 10× low.**
  - With the node in place, `show power` must stop writing raw registers: it would fight the driver.
- **Confidence:** high (the reads and the gawk test). R = 1 mΩ is medium-high: it comes from the factory
  constants and fits the measured power. No schematic.
- **Next (safe software step, needs a deploy):** in `dmc12-tools/show`, convert with
  `$((v))`/`strtonum`, or better, compute read-only: I = reg1 × 10 µV / 1 mΩ = reg1 × 10 mA,
  P = V_bus × I, and drop the two `i2cset`s. Or add the DT node and read hwmon, which also gives
  `dmc12-metrics` an input-power series.

## 6. TPM "could not request locality"
**Resolved 2026-11-16:** SPI node added (516bf72); `tpm_tis_spi spi0.2: 2.0 TPM (vendor-id 0x15D1,
device-id 0x1B)`, PCR reads work, every mtd partition's sha256 unchanged across the first probe.
- **Error:** `tpm_i2c_infineon 3-0020: could not request locality` on every boot.
  - It happens under the factory 4.19 kernel too: 8 burn-in boots in `factory-os/dmc12_diag_log/bi_dmesg.log`
    (2019), the operator syslog on sda1 (2022) and the initial capture.
  - Under our 7.3.4 the node `slb9645tt` stays unbound, there is no `/sys/class/tpm`, and IMA prints
    "No TPM chip found".
- **Cause:** **no I2C TPM answers.** The read-byte scan of i2c-3 (mux 0x70 ch2) gets no ACK at 0x20 or
  any other address. The probe's TPM_ACCESS reads fail, and the driver reports that as the locality
  error. The node is a K2C-AB reference-DT leftover (dt.0 and dt.1 both carry it).
- **The real TPM is very likely the SPI SLB9670 (TPM 2.0) on CS2:**
  - The factory U-Boot `tpm_dt_based_probe` (0x0110e7e8) tries `/soc/spi_aux`, then **`/soc/spi`
    (`infineon,slb9670`)**, and only then `/soc/i2c-pld`. It returns −6 silently if none answers.
  - The caller prints `TPM: u-boot interrupted, extending TPM PCRs!` only when init **and** both PCR 0/1
    extends returned 0 (0x01141bb4 → 0x0110ed18). u01 printed it (`serial-20261115-025824.txt` line 262).
  - Since I2C can't answer, the TPM that worked must be the SPI one.
  - Stage2 runs the same probe order (0xf220cb78), with an optional `uc_enable_tpm_access` DT property
    (absent from dt.0).
- **Who uses the TPM today:** the factory kernel never did (`CONFIG_TCG_TIS_SPI` unset; I2C only). Our DT
  has no SPI TPM node; our kernel has `TCG_TIS_SPI=m`.
- **Confidence:** high that there is no I2C TPM; medium-high that the SPI TPM exists and works (code path
  plus one log line; no direct SPI read).
- **Next (safe software step):** drop `tpm@20` from `alpine-v2-dmc12.dts` (it only produces the error).
  To use the TPM (owner decision):
  - add `tpm@2 { compatible = "infineon,slb9670", "tcg,tpm_tis-spi"; reg = <2>; spi-max-frequency =
    <1000000>; }` to the SPI node;
  - check first that SS2 is muxed (vendor pinctrl `if_spim_a_ss_2`; check that our U-Boot's MUIO list
    covers it);
  - check that dw-apb-ssi keeps CS asserted across TIS wait states (it may need `cs-gpios`).
  - PCRs then show the factory chain's measurements.

## Corrections for the existing docs
- `vendor-tooling.md` (cpldupd row) and `spi-recovery.md` §6: the CPLD is updated over **I2C 4-0x10**, not
  by bit-banged JTAG. GPIO 494 = SoC GPIO 22 is the hitless hold, not JTAG. `board-notes.md` "GPIO 22 is an
  input" at cold start: it is that pulled-up hold line.
- `spi-recovery.md` §3: uC 0x3535 / 0xa9a9 are FRU-SRAM-ready / I2C-FIFO-clean (HAL), not TPM access.
  `uboot-vendor-commands.md` `uc_set_spi_buffer`: the HAL says the uC SPI buffer switches between MOBO
  and TPM.
- `board-notes.md` TPM line and `vendor-tooling.md` leftovers: there is no I2C TPM on the bus; the SPI
  SLB9670 answered the factory U-Boot.
- `vendor-tooling.md` power row "gap: none": our `show power` prints 0 (the gawk hex bug).

## Hazards found
- **Any write to i2c-4 0x10 is a CPLD configuration command.** A stray write can erase, refresh or
  offline the CPLD that runs the fans, PSU sensing and board reset. Never run `sensors-detect` or any
  write-probing scan on bus 4. The factory `cpldupd` must never run here.
- **CS0's APCEA slots must stay blank** (0x110000, 0x3d0000). A valid header there makes stage3 start that
  code at 0xb8000000 on every boot. Don't use them as spare space.
- The uC can pick the boot flash by itself if its firmware runs the PCIE_WAKE auto-detect (item 1). u01
  has always booted CS0.
- Our `show power` writes the INA219 with `-f`. That is harmless today, but it would fight an `ina2xx`
  driver.
