# 2026-11-10 — what the two FPGAs do, the port map, and the stock bring-up sequence

Offline work only, from vendor files. Nothing on the switch was touched and
nothing here has been tried on hardware. Builds on
[2026-11-09-fpga-asic-probe-maps.md](2026-11-09-fpga-asic-probe-maps.md)
(register maps are not repeated). `ipd.ko` and the SDK runtime internals are a
different agent's job and are only touched where a script names an interface.

Marking used below: **V** = read directly in a file / disassembly / log,
**I** = inference from those.

## Sources and versions

| label | what | where |
| --- | --- | --- |
| flash 0.1.7 | ONIE initramfs in SPI flash: drivers 0.0.8 (2021-06-04), SDK `R3181-J0001-01_V0.1.7_Briggs_SDK` (2021-05-31), diag 0.1.3 | `images/spi-2026-11-09/mtd4-onie.bin`; unpacked for this session in `.../scratchpad/probe-agent/flash-onie/ex/{drv,sdk,diag}` and the ONIE rootfs in `.../scratchpad/datapath-agent/onie-rootfs/` (session scratch, not kept; re-extract with the recipe in the 11-09 review) |
| SSD 0.1.2 | DIAGOS SSD: drivers, SDK 0.1.0 / 0.1.2, diag | `analysis/extracted/root/{driver,sdk,diag,fw}` |
| factory log | a real SDK run on this SSD, 2021-05-03 | `analysis/extracted/root/sdk/R3181-J0001-01_V0.1.2_Briggs_SDK/ifcsshell.log`, `c-phy/dynamic_config.txt`, `cel_configs/config.yaml` |
| live | this unit under stock ONIE, 2026-11-09 | `evidence/live-onie-2026-11-09/`, `serial-20261109-182401.txt` |

Everything below is from flash 0.1.7 unless it says otherwise.

## Short answers

- **`1d0f:8571` "i2c_accel_fpga" (Xilinx, on its own "i2cfpga-card")** is the
  QSFP-DD management plane: 32 independent I2C masters (Linux buses 101..132,
  one per cage) plus the five cage sideband signals for all 32 ports (present,
  reset, low-power, interrupt, interrupt mask) and a presence-change
  interrupt. Nothing else is exposed by the driver: no LEDs, SPI, GPIO, PTP or
  UART. It carries no traffic.
- **`1d0f:ec30` "mdio-fpga" (Lattice, on the MACsec card)** is the Credo PHY
  management plane: 16 MDIO masters (one per PHY "slice" = one front-panel
  port), and per chip a reset bit, eight GPIO bits and two interrupt/mask bits.
  Nothing else. It carries no traffic.
- **Only ports 1-16 go through the Credo CMS50216 MACsec PHYs.** Ports 17-32
  are wired from the Innovium ASIC straight to the cages. Eight chips x two
  400G "slices" each = 16 ports.
- **Neither FPGA is needed for packets to flow on ports 17-32.** For ports
  1-16 the PHYs must be given firmware and a port configuration over MDIO
  first; that is done entirely from userspace by Credo's `phy_server`.
- **The stock SDK never ran on this unit while we watched.** `onie_fpp` is not
  set in the U-Boot env, so ONIE printed `onie fpp default to none, not bring
  up SDK` (serial capture line 1055) and `ipd.ko` was never loaded. The only
  record of a successful run is the factory log on the SSD.

## 1. Role of each FPGA

### 1.1 i2c_accel_fpga, `1d0f:8571` (driver 0.11)

Sysfs root is `/sys/devices/xilinx/accel-i2c/` (a root device named `xilinx`
plus a symlink `accel-i2c` to the PCI device) **V** (`fpga_probe`:
`__root_device_register("xilinx")`, `sysfs_create_link`).

Attributes, complete list from the module's symbol table **V**:

| attribute | BAR0 register | notes |
| --- | --- | --- |
| `version`, `board_version`, `scratch`, `name` | `0x0`, `0x4` | see 11-09 review |
| `raw_access_addr`, `raw_access_data` | any | debug peek/poke |
| `port[1-32]_module_present` | `0x140`, bit N-1 | read-only; 1 = module present per `bin/qsfp` |
| `port[1-32]_module_present_changed` | none (software latch) | set by the IRQ handler from `0x150` |
| `port[1-32]_module_interrupt` | `0x160`, bit N-1 | |
| `port[1-32]_module_interrupt_mask` | `0x168`, bit N-1 | |
| `port[1-32]_module_lpmod` | `0x170`, bit N-1 | |
| `port[1-32]_module_reset` | `0x178`, bit N-1 | writing 1 asserts ResetL per `bin/sfp_eeprom` ("Fail to set ResetL to 0") |
| `port[1-32]_i2c_master_reset` | `0x100c + 0x100*(N-1)` | |
| `port[1-32]_i2c_profile_select` | `0x1008 + 0x100*(N-1)`, bits 1:0 | 0 = 400 kHz default (release notes 0.0.4) |
| `port[1-32]_i2c_profile[1-3]_speed`, `port[1-32]_i2c_9_clock` | per-channel block | |

Evidence for the sideband offsets, `fpga_bit_show` (0.11):

```
 b9c: strcmp(attr, "module_present")        -> mov x19, #0x140
 c0c: strcmp(attr, "module_reset")          -> mov x19, #0x178
 c30: strncmp(attr, "module_interrupt")     -> mov x19, #0x160
 c48: strncmp(attr, "module_interrupt_mask")-> mov x19, #0x168
 c5c: strcmp(attr, "module_lpmod")          -> mov x19, #0x170
 bbc: bl fpga_read32 ; asr w19, w19, w23 ; and w19, w19, #1     (w23 = port-1)
 cd4: "i2c_master_reset"   -> 0x100c + (chan << 8)
 d0c: "i2c_profile_select" -> 0x1008 + (chan << 8), and #0x3
```

Interrupt handler (`fpga_interrupt_handler`, MSI "FPGA MSI") **V**: writes
`0xffffffff` to `0x158` (mask), **writes 0 to `0x178`** (the module-reset
register, all ports), reads `0x150` (presence-change status), writes
`0xffffffff` back to `0x150` to clear it, ORs the value into the software
`present_changed` latch, writes 0 to `0x158`. So `0x150`/`0x158` are the
presence-change status/mask pair. Why the handler clears the reset register on
every presence change is not explained by the code (**I**: release any module
from reset when something is plugged).

I2C masters **V**: `i2c_add_numbered_adapter` with `nr = 0x65 + index`, i.e.
buses 101..132, adapter name `"%s I2C Adapter Port %d"`. Probe writes three
registers per channel c = 0..31: `0x100c + 0x100*c = 0x80000000`,
`0x1008 + 0x100*c = 0`, `0x1000 + 0x100*c = 0x00660918`. The transfer path
(`fpga_rtc_read` / `fpga_rtc_write` / `fpga_i2c_transaction`) uses
`0x1050 + 0x100*c` (on read: `0x10000000 | arg3<<8 | arg5`; which argument is
the slave address and which the length was not traced),
`0x1054 + 0x100*c` (`0x80000000 | arg4`), a write buffer at
`(c + 0x20) << 9` = `0x4000 + 0x200*c` and a read buffer at
`(c + 0x40) << 9` = `0x8000 + 0x200*c`, 32-bit words. I did **not** decode
the status/polling half (`fpga_i2c_transaction`), so this is not yet enough
to write a mainline adapter driver.

Port-to-channel mapping: driver 0.11 has no `of_property_read*` call at all;
it only attaches the DT child `port%d` as the adapter's `of_node`. Release
notes 0.0.8: "Remove SW I2C mapping". The `bus`/`port` permutation in the SSD
device tree's `qsfp-portmap` node (`analysis/celestica_cs8210-r0.dts:3034`)
belongs to the older driver/FPGA image (**I**: 0.09 has `bus` and `port`
property-name strings, 0.11 does not; I did not disassemble 0.09); the DT that the flash ONIE boots has
an **empty** `qsfp-portmap` node (`evidence/live-onie-2026-11-09/runtime.dts:3052`).
So on this unit **port N = FPGA channel N = `i2c-(100+N)` = sideband bit N-1** **V**
for the driver, **I** that the FPGA image on this unit (version `0x0b00`)
is the one that pairs with it (the driver calls itself 0.11 and loaded
without complaint; cage order was never checked with a module).

Who uses it:

- `driver/install.sh` instantiates `sfpN` at `0x50` on bus `100+N` for N = 1..32
  (`echo sfpN 0x50 > /sys/bus/i2c/devices/i2c-1NN/new_device`); `sfp_module.ko`
  ("I2C driver for QSFP-DD module EEPROM" 1.0.3) binds and exposes hwmon-style
  attributes (`vendor_name`, `module_type`, `laneN_rx_power_input`,
  `laneN_tx_disable`, `temp1_input`, ...). Live dmesg: `qsfp-dd 101-0050: Port
  0 detected` ... for all 32.
- ONIE `bin/qsfp`, `bin/sfp_detect_tool`: read `portN_module_present`, then
  `i2cget -y -f 1NN 0x50 ...` (identifier, CMIS/SFF-8636 fields).
  `sfp_detect_tool` prints one line per port (`P7: Copper_400G`,
  `P8: NONE_READ`, ...) which `integrator_mode -m sfp_detect` turns into a
  port-mode string.
- ONIE `bin/sfp_eeprom` and diag `cel-sfp-test` (`diag/configs/sfp.yaml`:
  `dev_path: "/sys/devices/xilinx/accel-i2c/"`, `present/reset/interrupt/
  lpmod/profile_sel/profile_spd`): reset pulse (1, 300 ms, 0), LPMode on/off,
  raw EEPROM page access via `i2cdump`/`i2cset`.
- `get_versions` reads `/sys/devices/xilinx/accel-i2c/version`.
- The SDK scripts do **not** touch it, except through `sfp_detect_tool`.

Other facts **V**: the card has its own LM75 (`0x48`), FRU EEPROM (`0x56`) and
the FPGA also answers as an I2C slave at `0x6a` (`xilinx,i2cfpga`; diag
`i2cs.yaml` only checks that it ACKs, no driver binds). Its bitstream is the
aux SPI flash (`fw/README`: "I2CFPGA: `flash cp -v image.bin /dev/mtd5`").
The system CPLD has `i2cfpga_present`, `i2cfpga_lm75_interrupt`,
`i2cfpga_eeprom_write_protect` and `qsfp_mux[1-4]_reset` attributes, and
`sfp.yaml` says "Only for machine with i2c accel card" (**I**: the card is an
option; boards without it reach the cages through PCA9548 muxes. Not relevant
to this unit, which has the card).

### 1.2 mdio-fpga, `1d0f:ec30` (driver 1.7 + mdio-lattice 1.5)

Sysfs root `/sys/devices/lattice/mdio-fpga/`. Complete attribute list **V**:
`version`, `scratch`, `name`, `raw_access_addr`, `raw_access_data`,
`phy[1-8]_reset`, `phy[1-8]_gpio[0-7]`, `port[1-16]_interrupt`,
`port[1-16]_interrupt_mask`. Register offsets are in the 11-09 review. There
is nothing else in the resource tables: no I2C, LED or clock function.

- PHY reset: `phyN_reset` per chip. No stock script asserts it. The only
  reference is `credo_helper.py: credo_double_reset()` (`echo 1/0 >
  /sys/devices/lattice/mdio-fpga/phy{1..8}_reset`), whose one call site is
  commented out.
- PHY interrupts: `portN_interrupt{,_mask}`; no stock script or module
  consumes them (`cms50216.ko` registers no IRQ).
- `phyN_gpio0..7`: no user anywhere in the stock files; meaning unknown.
- The MDIO buses are named `mdio-1` .. `mdio-16`, one Credo slice each at
  address N+1 (`mdio-1:02` .. `mdio-16:11`).
- It also answers as an I2C slave at `0x67` on vendor bus 30 (diag
  `i2cs.yaml` group_26, presence check only). Reprogrammed with `vmetool_arm
  -md image.vme` (JTAG via CPLD, `fw/README`).
- System CPLD: `mdiofpga_reset`, `macsec_present`, `macsec_lm75_interrupt`.

`cms50216.ko` 1.4 is a thin phylib driver whose only job is to give userspace
register access to each slice **V**: sysfs `raw_access_addr`,
`raw_access_data`, `mdc_freq` under `/sys/bus/mdio_bus/devices/mdio-N:XX/`,
and a char device `/dev/mdio-N:XX` with ioctl 0 = read, 1 = write
(`struct { u32 reg; u32 val; }`, release notes "ioctl access method for high
speed accessing"). It does no PHY configuration beyond the pad-mode write
described in the 11-09 review.

### 1.3 The Amazon PCI IDs

What the files say **V**:

- Older modules match FPGA-vendor IDs: `i2c_accel_fpga` 0.09 has only
  `pci:v000010EEd00007021` (Xilinx); `mdio-fpga` 1.6 only `1204:ec30`
  (Lattice).
- Driver release 0.0.7, 2021-04-26: "[mdio-fpga] Version 1.7. Support new PCIe
  vendor/device id(1d0f:ec30). [i2c_accel_fpga] Version 0.10. Support new PCIe
  vendor/device id(1d0f:8571)." 0.11 matches both `10ee:7021` and `1d0f:8571`.
- Diag release notes 0.1.1 / 0.1.3: "Update pcie vendor id for MDIO FPGA" /
  "... for I2C FPGA"; `pcis.yaml` expects `1d0f:8571` (5GT x1) and `1d0f:ec30`
  (2.5GT x1).

So the IDs were changed in the FPGA images in spring 2021 from the FPGA
vendors' defaults to vendor `1d0f`. No file says why.

## 2. Datapath and port map

Evidence that ports 1-16 are behind the PHYs and 17-32 are not, all **V**:

- `diagtest_sdk.py:8`: `gNoneMACSecPortIndex = 17`.
- `integrator_mode`: "force enable host LT on innovium side(port 1-16)";
  `dynamic_config.sh`: `linePortStart=16` (ports below it get `ipg: "48"`
  and the "Host" pre-emphasis files, ports above get the "line"/Optics/DAC
  files); `inv_helper.py`: `host_devports = 1..16*n`, `line_devports` the rest.
- `c-phy/auto_load_user.sh`: `sliceN=16`, default `1-16:optics_1x400G`, any
  range above 16 is clipped.
- `cel_configs/isg.yaml`: ASIC polarity is all zero for the 16 ISGs of ports
  1-16 (polarity is done in the Credo `swap_setting.cmd` instead) and non-zero
  for ports 17-32.
- `diag/configs/leds.yaml`: port LEDs "sfp1-16" are on `mac_led_cpld`
  (23-0063), "sfp17-32" on `sw_led_cpld` (11-0064).
- Factory log: BER tests run Innovium PRBS on devports 17-32 against
  `phy_shell ... lane_mask=0xff000` (Credo line side) through loopback
  modules, and separately "Host BER" for 1-16.

Inside a Credo slice (`c-phy/auto_load_user.sh`, factory log) **V**: host
(ASIC-facing) lanes are 0..7, line (cage-facing) lanes are 12..19
(`lane_mask=0xff0ff`, `line_start=12+port*lanes`, `host_start=port*lanes`,
log: `Host start_lane/no_of_lanes/fec_type (0/8/3)`, `Line ... (12/8/3)`).
`allocate_device ... device_id=CREDO_OWL_800`: each MDIO slave is one 400G
"slice"; a CMS50216 package holds two (this also answers the 11-09 open
question about the two MDIO slaves per chip). Per-slice lane polarity is in
`c-phy/swap_setting.cmd`; no lane *swap* is configured on the Credo side.
Slice N -> PHY address N+1 -> `mdio-N` is a fixed table in `libMdio.so`
(`phy_addr_list` = `02 03 ... 11`).

ASIC side **V**: SKU file `configs/sku/innovium.77701_A` ("sku: innovium.77700_A",
`device_id: 0x1b58`, 33 ISGs, `max_serdes_speed: 50`). Each front-panel port
is one 8-lane SerDes group ("ISG"); the map is the same in
`dynamic_config.sh` (`isgMap`), `devport_PAM4_400G_32.yaml` and the factory
`ifcs show devport` output. In 32x400G mode devport = sysport = port number.
With breakout, devports are numbered consecutively in port order
(`dynamic_config.sh`: `lport` increments per sub-port), e.g. `128x100`:
port P sub-port s (1..4) = devport `4*(P-1)+s`, lanes `2*(s-1):2`.

Fixed extra devports **V** (factory log): 0 = CPU (sysport 1000);
129 = "mgmt 0"/AUX, ISG 32 ("MSER0"), 1 lane, 10G, `DOWN / DISABLED`;
513-518 = recirculation. **I**: devport 129 is the ASIC end of the CPU card's
second al_eth NIC (`00:03.0`, 10G, PHY-less), since PROGRESS records that NIC
as 10GBASE-SR "towards the switch board". Nothing in the files states it.

| port | QSFP-DD I2C bus | path | Credo chip / slice / MDIO | ASIC ISG | devport = sysport (32x400) | ASIC lane_swap | ASIC tx_pol / rx_pol | port LED CPLD |
|---|---|---|---|---|---|---|---|---|
| 1 | i2c-101 | ASIC -> Credo -> cage | U80 / slice 1 / `mdio-1:02` | 26 | 1 | 21043567 | 00000000 / 00000000 | MACsec LED CPLD (23-0063) |
| 2 | i2c-102 | ASIC -> Credo -> cage | U80 / slice 2 / `mdio-2:03` | 27 | 2 | 01364257 | 00000000 / 00000000 | MACsec LED CPLD (23-0063) |
| 3 | i2c-103 | ASIC -> Credo -> cage | U4 / slice 3 / `mdio-3:04` | 24 | 3 | 75634012 | 00000000 / 00000000 | MACsec LED CPLD (23-0063) |
| 4 | i2c-104 | ASIC -> Credo -> cage | U4 / slice 4 / `mdio-4:05` | 25 | 4 | 01234576 | 00000000 / 00000000 | MACsec LED CPLD (23-0063) |
| 5 | i2c-105 | ASIC -> Credo -> cage | U6 / slice 5 / `mdio-5:06` | 18 | 5 | 75643102 | 00000000 / 00000000 | MACsec LED CPLD (23-0063) |
| 6 | i2c-106 | ASIC -> Credo -> cage | U6 / slice 6 / `mdio-6:07` | 19 | 6 | 47625310 | 00000000 / 00000000 | MACsec LED CPLD (23-0063) |
| 7 | i2c-107 | ASIC -> Credo -> cage | U9 / slice 7 / `mdio-7:08` | 13 | 7 | 67541302 | 00000000 / 00000000 | MACsec LED CPLD (23-0063) |
| 8 | i2c-108 | ASIC -> Credo -> cage | U9 / slice 8 / `mdio-8:09` | 14 | 8 | 67543201 | 00000000 / 00000000 | MACsec LED CPLD (23-0063) |
| 9 | i2c-109 | ASIC -> Credo -> cage | U11 / slice 9 / `mdio-9:0a` | 6 | 9 | 04761532 | 00000000 / 00000000 | MACsec LED CPLD (23-0063) |
| 10 | i2c-110 | ASIC -> Credo -> cage | U11 / slice 10 / `mdio-10:0b` | 7 | 10 | 75634021 | 00000000 / 00000000 | MACsec LED CPLD (23-0063) |
| 11 | i2c-111 | ASIC -> Credo -> cage | U12 / slice 11 / `mdio-11:0c` | 1 | 11 | 41270653 | 00000000 / 00000000 | MACsec LED CPLD (23-0063) |
| 12 | i2c-112 | ASIC -> Credo -> cage | U12 / slice 12 / `mdio-12:0d` | 2 | 12 | 32154760 | 00000000 / 00000000 | MACsec LED CPLD (23-0063) |
| 13 | i2c-113 | ASIC -> Credo -> cage | U13 / slice 13 / `mdio-13:0e` | 30 | 13 | 76532410 | 00000000 / 00000000 | MACsec LED CPLD (23-0063) |
| 14 | i2c-114 | ASIC -> Credo -> cage | U13 / slice 14 / `mdio-14:0f` | 23 | 14 | 57364012 | 00000000 / 00000000 | MACsec LED CPLD (23-0063) |
| 15 | i2c-115 | ASIC -> Credo -> cage | U14 / slice 15 / `mdio-15:10` | 20 | 15 | 47653210 | 00000000 / 00000000 | MACsec LED CPLD (23-0063) |
| 16 | i2c-116 | ASIC -> Credo -> cage | U14 / slice 16 / `mdio-16:11` | 10 | 16 | 67543102 | 00000000 / 00000000 | MACsec LED CPLD (23-0063) |
| 17 | i2c-117 | ASIC -> cage (direct) | none | 15 | 17 | 20134567 | 00000010 / 11101010 | switch LED CPLD (11-0064) |
| 18 | i2c-118 | ASIC -> cage (direct) | none | 5 | 18 | 01234567 | 00000000 / 01101001 | switch LED CPLD (11-0064) |
| 19 | i2c-119 | ASIC -> cage (direct) | none | 28 | 19 | 75106243 | 00000110 / 10000100 | switch LED CPLD (11-0064) |
| 20 | i2c-120 | ASIC -> cage (direct) | none | 29 | 20 | 74032165 | 10001001 / 11100111 | switch LED CPLD (11-0064) |
| 21 | i2c-121 | ASIC -> cage (direct) | none | 21 | 21 | 35420671 | 10000010 / 00010101 | switch LED CPLD (11-0064) |
| 22 | i2c-122 | ASIC -> cage (direct) | none | 22 | 22 | 73651024 | 01101000 / 10111010 | switch LED CPLD (11-0064) |
| 23 | i2c-123 | ASIC -> cage (direct) | none | 16 | 23 | 03542761 | 10110110 / 10000000 | switch LED CPLD (11-0064) |
| 24 | i2c-124 | ASIC -> cage (direct) | none | 17 | 24 | 37541026 | 00010001 / 11100110 | switch LED CPLD (11-0064) |
| 25 | i2c-125 | ASIC -> cage (direct) | none | 31 | 25 | 02134756 | 10110100 / 11010111 | switch LED CPLD (11-0064) |
| 26 | i2c-126 | ASIC -> cage (direct) | none | 0 | 26 | 12465307 | 00010100 / 01001000 | switch LED CPLD (11-0064) |
| 27 | i2c-127 | ASIC -> cage (direct) | none | 11 | 27 | 50621374 | 10000111 / 11011111 | switch LED CPLD (11-0064) |
| 28 | i2c-128 | ASIC -> cage (direct) | none | 12 | 28 | 45360127 | 00001001 / 01110000 | switch LED CPLD (11-0064) |
| 29 | i2c-129 | ASIC -> cage (direct) | none | 8 | 29 | 03642571 | 10100011 / 01000001 | switch LED CPLD (11-0064) |
| 30 | i2c-130 | ASIC -> cage (direct) | none | 9 | 30 | 32541076 | 00000000 / 01110011 | switch LED CPLD (11-0064) |
| 31 | i2c-131 | ASIC -> cage (direct) | none | 3 | 31 | 05231674 | 11110001 / 01111111 | switch LED CPLD (11-0064) |
| 32 | i2c-132 | ASIC -> cage (direct) | none | 4 | 32 | 21540367 | 11100010 / 00110110 | switch LED CPLD (11-0064) |

Table columns: Credo chip names are from diag `mdio.yaml`; "ASIC ISG",
`lane_swap` and polarities from `cel_configs/isg.yaml` (board version other
than `0x2`; `isg_R01.yaml`, used when sys CPLD `board_version` reads `0x2`,
differs only in ISG 3 / port 31 `rx_polarity: 10111111`). The table was
generated by script and cross-checked against `devport_PAM4_400G_32.yaml`.
The physical position of port N on the faceplate, and which Credo line lane
goes to which cage pin, are not in the files.

## 3. Stock bring-up sequence

As ONIE does it at boot (`lib/onie/init-platform`, `init_platform_pre_arch`).
"Prints" are the literal strings in the scripts/binaries; lines marked
*(factory log)* were actually produced by a unit, the rest were not observed.

1. **Unpack and load platform drivers.** `cd /root/driver; tar xJf
   driver.tar.xz; ./install.sh`.
   - `insmod` in this order: `asc10 tps53679 psu_dps800 ltc4282 cls-i2c-client
     sys_cpld fan_cpld i2c_accel_fpga sfp_module cms50216 mdio-fpga`, then
     `mdio-lattice.ko` if `i2cget -f -y 8 0x60 0x01 >> 4` is non-zero, else
     `mdio-lattice-legacy.ko`.
   - `echo sfpN 0x50 > /sys/bus/i2c/devices/i2c-1NN/new_device` for N = 1..32.
   - fan watchdog on (`31-0066`, `40-0066` `fan_watchdog_enable`), system
     watchdog off, `i2cset -f -y 8 0x60 0x25 0x02` (power-cycle on reboot),
     picks `-21` diag YAMLs when CPLD `0x09` bit 0 is 0, clears
     `fan_max_speed`, starts `cel-fan-test --test -t fan_ctrl` in the
     background (only under ONIE), installs `setuptools`/`netaddr` for python2.
   - Success on this unit (live dmesg): `I2C Accelerator FPGA device driver
     v0.11`, `mdio-fpga ... Version: 00001008`, 16 x `phy[0x..]: device
     mdio-N:XX, driver Credo CMS50216`, 32 x `qsfp-dd 1NN-0050: Port N-1
     detected`.
2. **Unpack the SDK and run `onie_sdk_init.sh`** (skipped when the kernel
   command line contains `boot_reason`). It symlinks `integrator_mode` and
   `knet-l2-show` into `/usr/bin`, reads `onie_fpp` from the U-Boot env
   (`onie-env-get onie_fpp`, lower-cased; empty -> `NONE`), reads the base MAC
   from `onie-syseeprom -g 0x24` and adds 2 to its **first** byte
   (`0C:48:C6:7A:55:CE` -> `0E:48:C6:7A:55:CE`), then runs
   `./integrator_mode -m $oniePortMode -a $mac -k -d`, sleeps 5 and prints
   `onie switch driver initialization finished.`
   - On this unit: `## Error: "onie_fpp" not defined` -> `onie fpp default to
     none, not bring up SDK` (serial capture lines 1054-1057). Steps 3-7 did
     not run.
3. **`integrator_mode`** (bash). Kills a running daemon (`cls_shell exit`),
   translates the mode, then:
   `./auto_load_user.sh -m $innvMode -k -a MAC -d` (step 4), then
   `c-phy/auto_load_user.sh -m $credoMode -l` (step 6). Without `-k` it then
   restarts the SDK once more with `-s` ("singleVlan": all ports in one L2
   domain, VLAN 130, no netdevs).
   - Valid `-m` values: `none` (do nothing), `sfp_detect` (the script default:
     per-port result of `sfp_detect_tool`; `1-32:Optics_4x100G` if the tool is
     missing; an unreadable/empty cage becomes `4x100G`), `32x400`, `32x200`,
     `64x100`, `64x100-1`, `128x100`, `32x100`, `32x40`, `128x25`, `128x10`,
     or a raw SDK string `"<first>-<last>:<copper|optics|elb|lt>_<1|2|4|8>x<10|25|40|50|100|200|400>G[:fec-def|fec-en|fec-dis][:down-pN|down-pall];..."`.
     Translation: `32x400 -> 1-32:optics_1x400G`, `32x200 -> copper_1x200G`,
     `64x100 -> copper_2x100G`, `64x100-1 -> optics_2x100G:down-p2`,
     `128x100 -> optics_4x100G`, `32x100 -> optics_1x100G`, `32x40 ->
     optics_1x40G`, `128x25 -> optics_4x25G`, `128x10 -> optics_4x10G`.
   - For the ASIC it rewrites ports 1-16 to `copper_...` regardless of the
     media ("force enable host LT"), because the ASIC's link partner there is
     the Credo host side.
   - `getopt` string is `hdkm:a:`; the `-o` and `-s` options in the usage text
     are not accepted.
4. **`auto_load_user.sh`** (ASIC). In order:
   - if no `cel_configs/devport_<mode>.yaml` exists (always, for the strings
     above), `dynamic_config.sh <mode>` generates
     `cel_configs/dynamic_config.yaml` (devport list),
     `cel_cmds/dynamic_misc.cmd`, `cel_cmds/dynamic_preemphasis.cmd`, and the
     dump command files; prints `dynamic config. mode is ...`;
   - `lspci | grep '1d98:'`, `lspci -d 1d98: -vvv | grep '8GT/s, Width x4'`;
   - `insmod ./ipd.ko` if not loaded;
   - builds `cel_configs/config.yaml` = `common_PAM4_400G_32_nd.yaml` (with
     `-k`; has `netdev: - auto_create: "yes"` and `netdev: "true"` on TX rings
     1-3 and all four RX rings; SKU `configs/sku/innovium.77701_A`. The
     sibling `common_nd.yaml`, SKU `77701_B` with ISG 15/31 split 4:4, is only
     used with a prebuilt `devport_<name>.yaml` other than `PAM4_400G_32`,
     which no `integrator_mode` path selects) + devport list +
     `isg.yaml` or `isg_R01.yaml` chosen by
     `/sys/bus/i2c/devices/8-0060/board_version` (needs `sys_cpld.ko`);
   - builds `cel_cmds/misc_config.cmd` (`node create 0`, `ifcs set devport N
     rx_eq_fine_tune 1`, five `pci write led_*` lines, the per-lane
     pre-emphasis commands, `ifcs set devport N link_training 1` for copper
     PAM4 ports, `port enable|disable N`);
   - symlinks `phy_server`, `phy_shell`, `cel_swshell`,
     `/root/{en,dis}ablePacketForwarding.sh`; `ulimit -s unlimited`;
     `ifconfig lo up`;
   - daemon mode (`-d`): first `python2.7 ifcsshell.py -e switch -c config.yaml
     -i misc_config.cmd` in the foreground with `exit` appended, then
     `innovium.user config.yaml > /dev/null &`, `sleep 10`, prints `Enter
     daemon mode.` / `misc config.`, strips the `node` and `exit` lines,
     appends `diagtest knet config -a MAC` (with `-k`) and/or `diagtest
     integrator_mode config -v` (with `-s`), and replays the file with
     `cls_shell source misc_config.cmd`; prints `misc config end.`
   - Kernel interfaces: `ipd.ko` and its `/dev/ipd` (string in `libifcs.so`),
     the sys CPLD sysfs attribute, loopback TCP port 7105 (CLI server inside
     `innovium.user`, `IFCS_INNO_CLI_PORT`; client is `cls_shell` ->
     `ifcsrshell.py -p 7105`).
   - Success *(factory log)*: `Innovium Standalone Command Shell.`, `Node 0
     created and enabled.`, `=== PCI: LED_REFRESH_PRECLIFF (0x13538) ===
     Writing 00013538: 0x0018eec2`, and later `ifcs show devport` listing
     devports 1-32 `400G UP LINK_UP KP` (with loopback modules fitted).
5. **`cls_shell <command>`**: one-shot CLI client to the daemon (python2.7
   `ifcsrshell.py -e switch -p 7105 -s "<cmd>"`); `cls_shell exit` also
   `kill -9`s `innovium.user`. `/usr/bin/cel_swshell` is a symlink to it.
6. **`c-phy/auto_load_user.sh -m <mode> -l`** (Credo PHYs, ports 1-16 only).
   Starts `phy_server` (daemonises, pid in `/var/run/phy_server.pid`, commands
   arrive on FIFO `/tmp/cls_phy_fifo`, replies via `/tmp/cls_phy_echo` /
   `/tmp/cls_phy_cmd_done`), writes `c-phy/dynamic_config.txt` and runs
   `phy_shell load cfgfile=...`. The generated file, in order:
   - `allocate_device slice_id=all device_id=CREDO_OWL_800`;
   - MDC to 1 MHz on all 16 (`echo 1000000 > /sys/bus/mdio_bus/devices/
     mdio-N:XX/mdc_freq`), `mdio_reg_wr slice_id=all addr=0xb73a val=0x40`
     (pads push-pull), MDC to 20 MHz;
   - `slice_init slice_id=N init_type=SLICE_FULL_INIT firmware=.../c-phy/
     B52b0.lz4.1.92.2.bin` for N = 1..16: **this is the firmware download**,
     register by register over MDIO;
   - `set_tx_msb` / `set_rx_msb ... lane_mask=0xff0ff`, then
     `swap_setting.cmd` (per-slice `set_lane_config ... rx_pol_mask /
     tx_pol_mask`);
   - per port `firmware_configure_port slice_id=S lport=P line_start=12+..
     host_start=.. line_num=L host_num=L line_fec=.. host_fec=.. speed=..
     mode=MacsecBypass connect=Retimer flag=..` and `double_CRC ... enable=1`.
     `-l` adds `PORT_SYS_SIDE_LT` (host-side link training) for
     1x400/4x100/8x50; copper adds `PORT_LINE_SIDE_ANLT+PORT_AUTONEG_DISABLE`;
   - host- and line-side TX pre-emphasis from the `preemphasis_*.cmd` tables;
   - **MACsec rules** (see below);
   - `set_rx_input_mode slice_id=all lane_mask=0xff v=0`.
   - MDIO path **V** (`libMdio.so`): `PHY_mdio_init` tries
     `open("/dev/mdio-1:02")` + ioctl read of `0xcc`; if that returns `0x2161`
     it prints `mdio access change to ioctl type` and uses `/dev/mdio-N:XX`
     for everything, otherwise `mdio access change to sysfs type` and uses
     `raw_access_addr` / `raw_access_data` under
     `/sys/bus/mdio_bus/devices/mdio-N:XX`. MDC speed is always set through
     the `mdc_freq` sysfs file (`system("echo ...")`). No `/dev/mem`, no
     direct FPGA access.
   - Success *(factory log, SDK 0.1.2)*: `Chip version: 0020`, `[Firmware
     load] Version: 01.92.02 Date Code: 0x4914, Hash Code: 0x2e4303, CRC Code:
     0xa0b3`, `[Firmware] TOP PLL calibration done after ... usecs`,
     `firmware_load for slice mask 0x1fffe, SUCCESSFULLY`, `slice id 1 config
     port : flag is 0x202`, `[owl_fw_config_macsec] ... connection_mode: 0,
     port_mode: 1`, `allocate ingress SA node for slice 1 OK`, `add ingress
     vport rule(slice_id=1,sa_id=1,rule_id=1) successfully`, `credo config
     done!`.
   - `c-phy/load_fw_init.sh` is the older two-step variant
     (`SLICE_INIT_NO_FIRMWARE` then `firmware_load`, prints `FW load done!`).
     Nothing in 0.1.7 calls it; the factory 0.1.2 flow ran it from the SDK
     shell (`shell .../c-phy/load_fw_init.sh -m`).
7. **Front-panel netdevs** appear when the replayed `diagtest knet config -a
   MAC` runs (section 4).

Optional, by hand:

- `enablePacketForwarding.sh` (symlinked into `/root`): `cls_shell "diagtest
  integrator_mode config -v"` (one VLAN 130 containing every front port, one
  STP instance forwarding, PVID 130, untagged egress) and starts
  `polling_tx_bpdu.sh`, which every 10 s injects `BPDU.bin` on every port
  with `ifcs_hostif_send_packet(..., PIPELINE_BYPASS)` until `/tmp/tx_stop`
  exists. `disablePacketForwarding.sh` touches that file, waits 20 s and runs
  `diagtest integrator_mode unconfig`.
- `dynamic_config.sh "<mode string>"`: only generates files (step 4).
- `diagtest_sdk.py --case init|ber|pre-screen|screen|ess --port_mode
  1x400|2x200|4x100|2x100|8x50|1x100 --phy_mode serdes|macsecbypass|
  GCM-AES-128|...` with `inv_helper.py` (wraps `auto_load_user.sh` +
  `cls_shell`) and `credo_helper.py` (wraps `c-phy/auto_load_user.sh -e
  <phy_mode>` + `phy_shell`). These are the manufacturing test (snake
  traffic, PRBS BER). They use Python 3 syntax; the ONIE rootfs only has
  python2.7, so they belong to the DIAGOS side (**I**).

### PHY mode actually configured

- Every generated port line says `mode=MacsecBypass connect=Retimer`. The
  alternatives the tool knows are `mode=Macsec|MacsecBypass` and, with
  `-e serdes`, `mode=serdes` (no MACsec datapath at all).
- **But the script's default `macsec="GCM-AES-128"` is never overridden by
  `integrator_mode`** (it passes only `-m` and `-l`), so `create_macsec_rule`
  also emits, per slice: `macsec_datapath_init`, `macsec_enable slice_id=S
  port=P`, one ingress and one egress SA with a **fixed key that is printed
  in the script** (`sak=0xad,0x7a,0x2b,...`, `sci=0x81,0x5e,...`), and a
  catch-all vport rule (`da={0,0,0,0,0,0} num_tags=all
  pkt_type=CFYE_RULE_PKT_TYPE_OTHER`). The factory log shows those commands
  succeeding.
- **I**, not established: whether that leaves ports 1-16 sending
  MACsec-protected frames with the well-known test key, or whether
  `mode=MacsecBypass` keeps the crypto engine out of the path despite the SA
  being installed. The files do not settle it and it matters for
  interoperability of ports 1-16; it needs a capture on the wire or a read of
  Credo's `owl_fw_config_macsec` in `libowl.so`. `-e macsecbypass` makes the
  script emit `macsec_disable` instead, and `-e serdes` skips MACsec
  entirely; either is the safer choice for a first bring-up.

## 4. What "knet" is here

Not Broadcom KNET: there is no such module here. The name is Celestica's label
for "front-panel ports as kernel netdevs" (`auto_load_user.sh` still carries a
`#cel_bcmshell` comment). `knet-l2-show` only runs `cls_shell ifcs show
l2_entry`, and in 0.1.7 it still points at another product's directory
(`R1165-J0003-01_V2.1.14_Fenghuang64_SDK`), so **it is broken as shipped**.

- `-k` selects the `_nd` YAML: `netdev: - auto_create: "yes"`. `ipd.ko`
  contains `inno_netdev_create/delete/set/get` and creates one netdev per
  devport named `inno_0_NNNN` (node 0, 4-digit devport) **V** for the names
  (`knet.py` renames exactly those), **I** that creation happens at node
  creation because of `auto_create`.
- `diagtest knet config -a MAC` (`cmds/knet.py`) then, through `libifcs`:
  finds every devport's sysport; creates one STP instance (all forwarding);
  for each front port i a private VLAN `i+1` (L2VNI + flood group) whose only
  members are the CPU port and port i, PVID `i+1`, untagged egress. Each port
  is thus an isolated port-to-CPU pipe: **no switching between front ports**.
  Then for each i: `ifconfig inno_0_%04d down`, `ip link set inno_0_%04d
  name fppN` (or `fppN-M` for breakout sub-port M), `ifconfig fppN up|down`
  (per the `down-pN` option), `ifconfig fppN hw ether <MAC+i>`.
- Result: `fpp1..fpp32` in 32-port modes, `fpp1-1..fpp32-4` in 128-port
  modes, MACs counting up from the converted base MAC + 1. They are created
  by `ipd.ko`, renamed by the python command. ONIE can then DHCP/install over
  a front-panel port.
- `integrator_mode -dk`: `-d` SDK as a daemon with the remote shell, `-k`
  the above. Without `-k`: no netdevs, one flat VLAN (130) across all ports,
  i.e. a dumb L2 switch ("integrator mode"; BPDU keep-alives are the separate
  `enablePacketForwarding.sh`).

## 5. What the stock userspace needs from the kernel besides `ipd.ko`

To run the stock scripts unmodified:

| consumer | needs | provided by (vendor) |
| --- | --- | --- |
| `c-phy/*` (`phy_server`, `libMdio.so`) | `/sys/bus/mdio_bus/devices/mdio-N:XX/` with N = 1..16, XX = N+1 in the exact `mdio-%d:%02x` spelling; attribute `mdc_freq` (Hz, decimal; mandatory, written with `echo`); either char devices `/dev/mdio-N:XX` (ioctl 0/1, `{u32 reg; u32 val}`) or attributes `raw_access_addr` / `raw_access_data` (`0x%x` text). Register access is MMD 1 with a 16-bit address | `mdio-fpga.ko` + `mdio-lattice.ko` + `cms50216.ko` |
| `credo_helper.py` (unused path) | `/sys/devices/lattice/mdio-fpga/phyN_reset` | `mdio-fpga.ko` |
| `auto_load_user.sh` | `/sys/bus/i2c/devices/8-0060/board_version` (text `0x2` selects `isg_R01.yaml`); i.e. the vendor I2C bus numbering too | `sys_cpld.ko` |
| `integrator_mode -m sfp_detect`, `qsfp`, `sfp_eeprom`, `cel-sfp-test` | `/sys/devices/xilinx/accel-i2c/portN_module_{present,reset,lpmod,interrupt}`, I2C buses numbered exactly 101..132 with the module at `0x50` (via `i2cget -f`, so no client driver needed for detection); `sfp_module.ko` attributes only for the diag tool and `qsfp`'s fast path | `i2c_accel_fpga.ko`, `sfp_module.ko` |
| `onie_sdk_init.sh` | `onie-env-get`, `onie-syseeprom` (ONIE userland, not kernel) | ONIE |
| `get_versions`, diag | CPLD sysfs at `8-0060`, `11-0064`, `23-0063`, `31-0066`, `40-0066`, the two FPGA `version` files | `sys_cpld.ko`, `fan_cpld.ko`, FPGA drivers |
| SDK userland | glibc aarch64, python2.7 with `libpython2.7.so.1.0`, `libjansson.so.4`, `libyaml-0.so.2`, `script`, `lspci`, `ifconfig`, `ip` | ONIE rootfs |

Not needed by the SDK itself: `sfp_module.ko`, the I2C FPGA (unless
`sfp_detect`), port LEDs CPLD drivers, the PHY reset/GPIO/interrupt bits.
The vendor modules are 4.9.124 binaries and cannot be loaded on 7.3.3, so
"unmodified userspace" means re-implementing those sysfs/char-device ABIs in
new drivers (or a small shim). The cheaper route for the Credo half is to
replace `libMdio.so` (five exported functions: `PHY_mdio_init`,
`PHY_mdio_read`, `PHY_mdio_write`, `PHY_mdc_freq_set`, plus the sysfs/ioctl
helpers; 15 kB, not stripped) with one that talks to whatever mainline MDIO
interface exists; `phy_server` and the Credo libraries only reach hardware
through it (**V** for the import lists: `libcredoCmd.so` imports
`PHY_mdio_*`; **I** that nothing else in `libcredo.so`/`libowl.so` opens a
device, I only checked strings).

## 6. Course suggested by the above (inference, for the owner to weigh)

1. Ports 17-32 need only the ASIC: `ipd.ko`-equivalent + SDK + the YAML/ISG
   tables above. No FPGA driver is on the critical path. Start there.
2. Ports 1-16 additionally need MDIO access for `phy_server` (firmware
   `B52b0.lz4.1.92.2.bin`, about 58 kB per slice, 16 slices, at 20 MHz MDC)
   and the ASIC configured with host-side link training on those ports.
   Use `-e serdes` or `-e macsecbypass` first.
3. The I2C FPGA is only for optics management (presence, reset, LPMode,
   EEPROM/DOM). Cages come out of reset by FPGA default as far as the stock
   flow shows (no script deasserts `module_reset` at boot; **I**), but
   high-power optics will need LPMode handling, so a mainline I2C-adapter +
   GPIO driver for it is still wanted for real use.

## What I could not establish

- Whether MACsec encryption is really active on ports 1-16 after the default
  stock flow (see "PHY mode actually configured").
- The Credo lane-to-cage-pin order and any lane swap on the PHY line side;
  only polarity masks are in the files.
- The mapping from ISG number to physical ASIC SerDes/ball, and the meaning
  of the digits in `lane_swap` (logical-to-physical or the reverse).
- Faceplate position of port N, and that FPGA channel N really is cage N on
  this unit (driver says so; never checked with a module).
- The rest of the I2C FPGA transaction protocol (`fpga_i2c_transaction`
  status/poll bits, `i2c_9_clock`, profile speed encoding `0x00660918`).
- What `phyN_gpio0..7` on the MDIO FPGA are wired to, and whether PHY
  interrupts are used by anything.
- Why the I2C FPGA interrupt handler zeroes the module-reset register.
- That devport 129 / ISG 32 is the link to the CPU's second NIC.
- Why the PCI IDs became Amazon's; and what the I2C-slave side of each FPGA
  (`0x6a`, `0x67`) offers.
- Whether `innovium.user` needs anything from the kernel beyond `/dev/ipd`
  (other agent's scope), and what `ipd.ko` names the netdevs before the
  rename other than what `knet.py` implies.
- The 0.1.7 flow was never observed running; all "success" output is from
  the 0.1.2 factory log (2021-05-03) or read out of the scripts.
- SDK 0.1.0/0.1.2 on the SSD were only spot-checked against 0.1.7
  (`integrator_mode` differs; `onie_sdk_init.sh` there calls
  `integrator_mode -dk -m ...`); I did not diff them line by line.
