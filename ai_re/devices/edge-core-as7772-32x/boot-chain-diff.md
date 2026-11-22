# AS7772-32X: factory boot chain vs ours (offline RE, 2026-11-22)

Question: does the factory chain (U-Boot 2014.07 1.7.0.12 -> ONIE / diag) do a board-level step (PHY reset, CPLD,
GPIO, clock, power) that our U-Boot 2026.10 + NixOS skip? Nothing was run on the device for this note.

**Answer: no.** The factory U-Boot writes no GPIO, no I2C device and no CPLD at boot; its code is the AS7760
loader's, minus one print. The two factory kernels carry no PHY/CPLD/GPIO driver, and no init script touches
CPLD1-3. The only board writes in the whole chain are the management PHY, the fan CPLD, the I2C muxes and (in the
manufacturing test only) a PHY-rail voltage margin. This agrees with the MDIO-timing result
([mdio-review.md](mdio-review.md), [extphy.md](extphy.md)): the silent PHYs were our MDIO setup, not a missing
board step. Side result: **why the vendor diag's SDK cannot talk to the ASIC under our U-Boot** (section 4).

Evidence marks: **V** verified in the binary/script/capture named, **I** inferred. Addresses are link addresses
(image base 0xeff40000 = file offset 0 of `dumps/mtd7.bin`); GOT base 0xeffbd5a8 (AS7760 image: 0xeffbd6ac).

## 1. Factory U-Boot: AS7772 (1.7.0.12) vs AS7760 (1.7.0.10)
Method: both 768 KiB images as 32-bit words, branch displacements and GOT offsets masked, `diff`; then capstone
disassembly with GOT strings resolved. Text is 0x0-0x65d00.
- **Text differs in one function only** (V). 0xeff4aa14 (`misc_init_r`, name I): AS7772 = `mgmt_phy_cfg(0x1f)`
  only; the AS7760's (0xeff4aa5c there) also calls the board-rev read and prints `ret 0x%x`. Everything else is
  byte-identical after masking, except `li r5,0x82e` vs `0x866` at 0x25bcc/0x25c2c (default-env size) and the
  banner. So the AS7772 loader does **less** than the AS7760 one, and has no PHY code the AS7760 lacks.
- Data differs in strings (names, env defaults, build path) and shifted tables; no I2C/GPIO init table exists
  because no code would consume one (below).
- **GPIO** (V): the image contains exactly one reference to the GPIO block, a **read** of GPIO1 GPDAT
  (0xfe130008) at 0xeff4aa3c: `(GPDAT & 7) == 1` -> "Prob-E", else "Prob-D" (picks the DDR table; callers
  0xeff4ade0, 0xeff72bec). No `lis 0xfe13` / `addis ..,0x13` anywhere else, so GPDIR/GPODR/GPDAT are never
  written. Factory prompt: GPIO1 = `00000000 00000000 0000aa01 0000aa01` (GPDIR 0 = all inputs),
  `factory-uboot-inventory-20261121.txt:457`. GPIO2-4 were not captured.
- **I2C** (V): `i2c_write` (0xeff72f44) has three callers, 0xeff552b8 / 0xeff557a8 / 0xeff55a6c, all inside the
  `i2c` command handlers. Boot-time I2C is read-only: SPD (0xeff6fa84/0xeff6fa9c) and the bus init. No mux
  write, so nothing behind 0x76 or 0x71 is reached at boot.
- **CPLD on IFC** (V): `cpld_read`/`cpld_write` (0xeff4abf0/0xeff4ac0c, base 0xffdf0000) are a T2080RDB
  leftover. Writes come only from the `cpld reset [altbank]` command (0xeff4ac1c/0xeff4ac58 <- 0xeff4ac94);
  `checkboard` (0xeff4a770) only reads 2, 3, 0x11. Nothing answers there on this board (0xff,
  [u-boot-port.md](u-boot-port.md)). The I2C CPLDs 0x60/0x62/0x64 are not referenced at all.
- Board functions read in full (names I, from the upstream t208xrdb layout): checkboard 0xeff4a770,
  board_early_init_r 0xeff4a81c (flash TLB, LIODN, portals), ft_board_setup 0xeff4a8a0, mgmt PHY 0xeff4a920,
  board-rev 0xeff4aa3c, board_eth_init 0xeff4aa84 (MDIO buses, SerDes protocol 0x66/0x6b/0x6c), do_cpld
  0xeff4ac94, pci 0xeff4ad1c/0xeff4ad24, fsl_ddr_board_options 0xeff4ad90, initdram 0xeff4af0c. No delays.
- Factory env (V, `uboot-env-private.txt`): no `preboot`, no `i2c`/`mw`/`mii` command in any variable.
  `bootcmd` only chooses diag / ONIE / NOS.

## 2. Board writes in the factory chain
| who, when | target | write | ours? |
| --- | --- | --- | --- |
| U-Boot `misc_init_r` 0xeff4a920, every boot (V) | mgmt PHY BCM54616S, MDIO 0x1f | reg 9 = 0x0300, reg 4 = 0x01e1, reg 0 = 0x0000, reg 0 = 0x1040 | n as such (generic PHY autoneg in U-Boot/Linux; port works). Not PHY-related |
| U-Boot IFC init 0xeff6f8xx (V) | IFC CS3 | CSPR3 = 0xffdf0085 (the absent RDB CPLD) | unknown, irrelevant (nothing at 0xffdf0000) |
| U-Boot `cpld reset` command only (V) | 0xffdf0010/11 | 0x11 = (r & 0xb8) \| 0x44 or 0x40, 0x10 = 0xff | n, no device there |
| U-Boot LAW6 (V, inventory :420) | PCIe4 memory window | CPU **0x0_d8000000**, 128 MiB | **n: ours opens 0xc_40000000** (section 4) |
| ONIE kernel 3.12.19 (I, strings) | - | board file `as7772_32x.c` is name + compatible only; no accton/CPLD/GPIO driver, no `gpios` in onie.dts | - |
| ONIE `S05bcm-knet.sh` (V) | ASIC + PHYs | would run its own `bcm.user`: `init all`, `extphy init` (`etc/knet/rc-32x100.soc`). **Disabled on this unit**: needs U-Boot env `onie_knet`; console says "SWITCH: Disabled" on every ONIE boot | n (our SDK + re-host do this) |
| ONIE `iic-util --daemon` (I, strings) | I2C | TCP server, writes only on request | n, not needed |
| diag kernel 3.12.19 (V strings, I behaviour) | fan CPLD 0x66 (i2c 4-0066) | built-in `accton_as7772_32x_fanctl` fan control; no driver binds `accton_i2c_cpld` 0x60/62/64 | y (fand) |
| diag `rcS` S80rclocal (V) | - | `insmod linux-kernel-bde.ko maxpayload=128 dmasize=128M debug=2`, user-bde, mount NOR | n/a |
| diag `profile.d/10_wa_i2c_clean_channel.sh` (V) | muxes | 0x76, 0x71-0x75 control = 0; `fan_disable_fanctrl` = 1 | n/a (kernel mux driver) |
| diag `profile.d/11_emi-mgmt-clk125.sh` (V) | mgmt PHY 0x1f | reg 0x1c = 0x9400 (CLK125 off, EMI) | n, cosmetic |
| diag `cpldutil -trans` / `-setfan`, on demand (V, [cpld.md](cpld.md)) | CPLD1 0x04-0x07; fan CPLD 0x33, 0x11 | QSFP reset pulse; fan duty | y/n per cpld.md |
| diag `switch_pre_test` -> `vmon_margin.sh LOW`, mfg scenarios only (V) | IR3595C, I2C4 mux 0x71 ch7, 0x14 | reg 0x6e = 0xfb (PHY_VDD1P0_DVDD -3 %); `switch_post_test` writes 0x6e = 0x00 | n (must stay 0x00) |
| diag `swutil` -> `rc.soc-100G` (V) | ASIC + PHYs | `init all`, `extphy init all`, `extphy cfg`, pre-emphasis | n at boot yet |

No row is a PHY reset, PHY clock or PHY power-enable. No script or binary in either rootfs writes CPLD2/CPLD3.
Device-tree check (V): onie.dts and diag.dts have no `gpios`, `reset-gpios` or `gpio-leds` node; the four
`fsl,qoriq-gpio` controllers have no consumer. Same in our `as7760-vendor.dts` + `as7772-delta.dtsi` and in the
U-Boot tree of patch 0034, so neither side drives a T2080 GPIO.

## 3. Things found on the way
- **I2C4 mux 0x71 ch5, 0x16** answers (ONIE `i2cdetect` bus 9, `onie-inventory-20261121.txt`), and is in no
  vendor I2C map or `vmon` list. `vmon_margin.sh` names it in commented-out lines as **PHY_VDD1P0_PLL**
  (`0x16 reg 0x01 = 0x00 / 0x18`, `0xd6 = 0xfff7` word: PMBus-style OPERATION margin). Part unidentified.
- **"P1014" (mux 0x71 ch4, 0x25) is a Lattice ispPAC-POWR1014(A) sequencer/monitor** (I: name + ADC protocol in
  `vmon` `P1014_read_adc` 0x10001384: write reg 0x09 = channel \| 0x10, poll reg 0x07 bit 0, value =
  (reg 8 << 4 \| reg 7 >> 4) x 2 mV). Rails per `vmon_func_list-G.csv`: VMON1 12 V, 2 5 V, 3 VCC_1V8_CPLD,
  4 3V3, 5 1V8, 6 VDD1P0_ROV, 7 1V2, 8 VDD1P0, 9 1V25, **10 PHY_VDD1P0_DVDD (1.0 V +5/-3 %)**. IR3595C loop 1 =
  PHY_VDD1P0_DVDD, loop 2 = PHY_VDD1P0_TX/RX, both 1.0375 V. Board revision: reg 0x06 bits 3:2 = 11 -> main
  board rev G (`12_vmon.sh`); our dump reads 0xff, so **rev G**. Dump regs 0x00-0x04 = 0xff: by the datasheet
  map (from memory, not checked) all VMON comparators and sequencer outputs high.
- Factory manufacturing test (`mfg_F2B`) runs `swutil extphy ver` on all PHYs; no log of it survives on the SSD
  or NOR (only a board-temperature test).

## 4. Why the vendor diag SDK fails under our U-Boot (SCHAN timeouts, 0xdeadbeef)
- Factory LAW6 maps PCIe4 memory at CPU **0xd8000000** (V); diag.dts and onie.dts say the same
  (`pcie@ffe270000 ranges`), with `mem=1024M` / `mem=1920M` and a `/memreserve/ 0xc0000000 +0x20000000` to keep
  RAM out of it.
- Our U-Boot opens that window at **0xc_40000000** (../edge-core-7760-32x/linux-bringup.md) and our kernel tree
  says so. The diag kernel still uses its own dtb: console `serial-20261121-2125-factory-diag.txt:24223`, "BAR 0:
  assigned [mem 0xd9000000-0xd9007fff]" (V).
- Under our U-Boot, CPU 0xd9000000 is **DDR**, not PCIe. DDR is initialised to 0xdeadbeef (`sdram_data_init`,
  V in the same capture). So the vendor BDE reads RAM: `CMIC_DEV_REV_ID` = 0xdeadbeef, `SCHAN_CTRL` keeps the
  MSG_START it wrote, MDIO returns 0xbeef. Attach works because it uses config space (I for the last sentence;
  the address mismatch itself is V).
- Not a board difference. ONIE's SDK would fail the same way under our U-Boot.

## 5. CPLD registers: what the evidence allows
- **CPLD2/CPLD3 0x20-0x2b: no decode.** No code in the factory U-Boot, either kernel, either rootfs or
  `cpldtst`/`cpldutil` (only CPLD1 has an expected-value table) refers to them. Structure only (I): two 16-bit
  pairs (0x20/21 = 0000, 0x2a/2b = 0000) and four single bytes on even addresses (0x22 = f0, 0x24 = cc, 0x26 =
  40, 0x28 = 00), odd ones unimplemented (ff); 16 bits = the 16 PHYs per CPLD is a guess. They read the same
  under the factory chain (`i2c-dumps-20261121.txt`, taken from ONIE before the U-Boot flash) and under ours, so
  **our chain changes none of them**.
- **CPLD1** `cpldtst` `cpld1_reg_tbl` @0x10013374, 3-byte entries (reg, expected, 0), two lists (V):
  list 1: 01=02 02=3c 08=ff 09=0d 0a=3f 0b=00 0c=ff 0d=ff 14=ff 15=c7 24=00 25=00 40=7c 41=82 42=00 43=05
  44=05 45=05 47=0f 48=70; list 2 (all 32 loopback modules fitted): 01=02 04-07=ff 10-13=ff **20-23=00**
  30-33=00. So 0x20-0x23 is a 32-bit per-cage group next to reset/int/present that reads 0 with and without
  modules (LPMODE or a mask is a guess); 0x01 expected 2, this unit has code version 4. No meaning recovered for
  0x09-0x0b, 0x15, 0x24/25, 0x40, 0x42-0x48.

## 6. Experiments for the live board, ranked
1. **Vendor diag with a corrected PCIe window** (makes the vendor `extphy`/MACsec commands observable). RAM boot
   as before (`init=/bin/sh`, nothing mounted), but patch the dtb before the jump:
   `bootm start ${diag_start}; bootm loados; bootm ramdisk; bootm fdt;
   fdt set /pcie@ffe270000 ranges <0x02000000 0 0xd8000000 0xc 0x40000000 0 0x08000000 0x01000000 0 0 0xf 0xf8030000 0 0x00010000>;
   bootm prep; bootm go`. Expected: BAR 0 at bus 0xd9000000 behind CPU 0xc_40000000, `g CMIC_DEV_REV_ID` sane,
   `init all` passes. Risk: none persistent; a wrong window gives a machine check and a reboot. Unverified: that
   `fdt set` acts on the relocated tree at that point, and that the 3.12 kernel accepts a >4 GiB CPU address
   here (the AS7760 vendor tree was changed the same way for 7.x only).
2. **Rail check, read-only** (health, not a fix). `B` = Linux bus of mux 0x71 channel *n* (`i2cdetect -l`).
   ch7: `i2cget -y $B 0x14 0x6e` and `0x6f` expect 0x00 (no margin left set); ch5: `i2cget -y $B 0x16 0x79 w`
   (STATUS_WORD, expect 0x0000 if the part is PMBus), `0x01` expect 0x00 or 0x80; ch4 P1014 VMON10:
   `i2cset -y $B 0x25 0x09 0x09`, then `i2cget` 0x07 and 0x08, expect about 1.0 V (channel index = VMON number - 1 is assumed, not read out of
   `vmon`'s table). Risk: the P1014 step writes
   its ADC-mux register, as the vendor `vmon` does; the rest are reads.
3. **GPIO1-4 state from our U-Boot prompt, read-only:** `md.l fe130000 4; md.l fe131000 4; md.l fe132000 4;
   md.l fe133000 4`. Expected GPIO1 = `00000000 00000000 0000aa01 ...` as under the factory loader. Risk: none.
4. **CPLD2/3 0x20-0x2b snapshot after PHY firmware is running and links are up**, compared with cold
   (read-only `i2cdump` of 0x62/0x64 rows 0x20). A changed bit would identify interrupt/status bits. Risk: none.
5. Writing CPLD2/3 0x20-0x2b: **not recommended**. No evidence of function, nothing in the factory chain writes
   them, and the PHYs no longer need it.
6. Re-flashing the factory U-Boot for an A/B: **not justified** by anything found here (NOR write, no alt bank).

## Not verified
- Kernel behaviour is from strings and device trees, not disassembly (both kernels are stripped raw images).
- `iic-util` and the diag fan driver were not disassembled.
- U-Boot function names are inferred; the instruction-level facts (single GPIO read, `i2c_write` callers, CPLD
  writers) are verified. Ghidra import was done but the result above rests on the capstone listing.
- Section 4 was not tested on the board.
- What our own U-Boot writes to the management PHY and IFC CS3 was not compared instruction by instruction.
