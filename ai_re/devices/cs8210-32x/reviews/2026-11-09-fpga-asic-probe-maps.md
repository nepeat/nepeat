# 2026-11-09 — register maps for a safe first probe of the two FPGAs and the Innovium ASIC

Offline work only: vendor 4.9.124 modules were disassembled; nothing on the
switch was touched. Nothing here has been tried on hardware yet.

## Summary

| device | safe identifying access | expected | confidence |
| --- | --- | --- | --- |
| mdio-fpga `1d0f:ec30` | BAR0 `+0x0`, 32-bit read | `0x00001008` (stock kernel logged it on this unit) | high |
| i2c_accel_fpga `1d0f:8571` | BAR0 `+0x0`, 32-bit read; version = bits 23:8, board version = bits 7:0 | value never recorded | high for offset/width; not proven on this unit |
| Innovium `1d98:1b58` | none in BAR0. `ipd.ko` identifies the chip from PCI config space only (vendor `0x00`, device `0x02`, revision `0x08`) | `1d98:1b58` rev `0x10` | high |
| Credo CMS50216 PHYs | clause 45, MMD 1, reg `0x00cc` (ID high) and `0x00cb` (ID low), one PHY per MDIO master, PHY address = master number + 1 | `0x2161` / `0x00a?` | high for the map; **needs FPGA register writes** |

Three things the main session should know before running anything:

1. **Check PCI memory decode first.** The stock kernel logs
   `enabling device (0000 -> 0002)` for both FPGAs, so they come out of U-Boot
   with `PCI_COMMAND_MEMORY` clear. With no driver bound on mainline it is
   probably still clear. Mapping `resource0` works regardless, but a read with
   decode off is exactly the unacknowledged access that panics this SoC. The
   script below refuses to read unless bit 1 of the command register is set,
   and only sets it (`echo 1 > .../enable`, a config-space write) with `--enable`.
2. **Access width in Python.** `struct.unpack('<I', ...)` and mmap slicing read
   byte by byte. The script uses `memoryview.cast('I'/'H')`, which CPython
   implements as a fixed-size `memcpy` (one 32-bit or 16-bit load). I did not
   verify the generated machine code of the Python on the switch; if that
   matters, a few lines of C with `volatile uint32_t *` removes the doubt.
3. **Reading a PHY ID is not read-only.** It takes 16-bit writes to the MDIO
   master registers. No PHY register is written.

## Which binaries were analysed

The modules that actually ran on this unit are not the ones on the SSD. The
ONIE image in SPI flash (`images/spi-2026-11-09/mtd4-onie.bin`) carries its own
`root/driver/driver.tar.xz`, `root/sdk/..._V0.1.7_Briggs_SDK.tar.xz` and
`root/diag/diag.tar.xz` inside the initramfs (driver release 0.0.8, 2021-06-04).
The live dmesg line `I2C Accelerator FPGA device driver v0.11` matches that
set, not the SSD's 0.09.

| module | SSD (`analysis/extracted/root/driver`) | flash ONIE | difference |
| --- | --- | --- | --- |
| `mdio-fpga.ko` | 1.6 | 1.7 | code identical; ID table gains `1d0f:ec30` (1.6 only matches `1204:ec30`) |
| `mdio-lattice.ko` | 1.5 | 1.5 | same srcversion |
| `mdio-lattice-legacy.ko` | 1.4 | 1.4 | same srcversion |
| `cms50216.ko` | 1.4 | 1.4 | same srcversion |
| `i2c_accel_fpga.ko` | 0.09 | 0.11 | 0.11 is what ran; used below |
| `ipd.ko` | SDK 0.1.0 / 0.1.2 | SDK 0.1.7 | byte-identical |

Extraction is reproducible from the flash dump: carve the xz stream at
`0x6b3ebc`, it is a cpio archive. Working copies and disassembly are in
`/tmp/claude-1001/-home-erin-nepeat-ai-re/e946fb7a-738a-4d95-8778-a420c2d8b8a7/scratchpad/probe-agent/`
(session scratch, not kept).

## 1. mdio-fpga (`1d0f:ec30`, mainline `0004:01:00.0`)

Two drivers share BAR0. `mdio-fpga.ko` is the PCI driver; it maps BAR0
(`pci_ioremap_bar(pdev, 0)`), owns the version/scratch/"phy misc" registers,
and registers one MFD cell `lattice-mdio` with 18 resources. `mdio-lattice.ko`
(or `-legacy`) is the platform driver for that cell and owns the MDIO masters.
There are not 16+8 MFD cells: the eight "phy misc-N" resources are used by
`mdio-fpga.ko` itself.

### BAR0 map

| BAR0 offset | width | name | source |
| --- | --- | --- | --- |
| `0x000000` | 32 | version | `mdio_fpga_probe`, `version_show` |
| `0x000004` | 32 | scratch (read/write) | `scratch_show`, `scratch_store` |
| `0x040000`–`0x0400ff` | 16 | "mdio interrupt" block: `+0x0` done status, bit n = master n+1; `+0x4` acknowledge | `mdio_res[1]`, `lattice_mdio_interrupt_handler` |
| `0x080000 + 0x20*(N-1)` | 32 | "phy misc-N", N = 1..8 | `phy_res[]`, `phy_update_device` |
| `0x0c0000 + 0x100*(N-1)` | 16 | "mdio master-N", N = 1..16 | `mdio_res[2..17]` |

Cross-check on this unit: the stock kernel printed
`mdio-1 mapping memory region 0xe02c0000 - 0xe02c00ff` … `mdio-16 … 0xe02c0f00`
with BAR0 at `0xe0200000`.

Evidence for the version register (`mdio_fpga_probe`, identical in 1.6 and 1.7):

```
 c1c:  mov  w1, #0x0
 c20:  bl   pci_ioremap_bar          ; BAR 0
 c24:  str  x0, [x22]                ; priv->base
 ...
 cf8:  ldr  x0, [x22], #24           ; x0 = base
 d14:  mov  w1, #0x0
 d18:  bl   iomem_read32             ; ldr w0, [x0, w1, uxtw]  -> 32-bit load
 d1c:  ... "Version: %08x\n"
```

Live result under the stock kernel: `mdio-fpga 0003:01:00.0: Version: 00001008`.

### "phy misc-N" block (32-bit registers)

`phy_update_device` reads, in this order, `+0x0`, `+0xc`, `+0x4`, `+0x8`:

| offset | meaning | evidence |
| --- | --- | --- |
| `+0x0` | bit 0 = PHY reset released. sysfs `phyN_reset` shows `!bit0`; writing `reset=1` clears the bit | `get_phy` (`mvn; and #1`), `set_phy` |
| `+0x4` | interrupt status, bit 0 = odd port, bit 1 = even port (ports 2N-1, 2N); cleared by writing 1 | `get_port`, `set_port` |
| `+0x8` | interrupt mask, same bit layout | `get_port`, `set_port` |
| `+0xc` | `gpio0..7` in bits 7:0 | `get_phy`, `set_phy` |

These reads happen only when someone reads the sysfs attributes, not at probe,
so unlike the version register there is no log proving they were exercised on
this unit.

### MDIO master block (16-bit registers), from `mdio-lattice.ko` 1.5

| offset | meaning |
| --- | --- |
| `+0x0` | bits 4:0 device address (MMD), bits 9:5 PHY address, bit 15 set only by the clause-22 write path |
| `+0x4` | register address |
| `+0x8` | write data / read result |
| `+0xc` | control: bit 0 = enable (0 holds the master in reset), bit 2 = start, bit 3 = read, bits 15:8 = MDC divider. Status: bit 5 = transaction error |
| `+0x10` | legacy driver only: reads 1 when the transaction is done |

`lattice_mdio_read(bus, phy, regnum)`:

```
 3d8:  tbz  w20, #30, 524            ; MII_ADDR_C45?
 3dc:  ubfx x23, x20, #16, #5        ; devad
 3e0:  and  w26, w20, #0xffff        ; reg
 408:  orr  w25, w25, w22, lsl #5    ; devad | phy<<5
 40c:  strh w25, [x27]               ; -> +0x0
 42c:  strh w26, [x25]               ; -> +0x4   (x25 = base+4)
 438:  ldr  w25, [x24, #16]          ; priv->mdc_div (default 0x7d)
 43c:  mov  w1, #0xd
 448:  orr  w25, w1, w25, lsl #8
 45c:  strh w25, [x26]               ; -> +0xc   (0x7d0d)
 46c:  bl   lattice_mdio_wait4ready  ; wait_for_completion_timeout(.., 500)
 480:  bl   iomem_read16 (+0xc)      ; ldrh
 484:  tbnz w0, #5, "read operation error!!"
 490:  bl   iomem_read16 (+0x8)      ; ldrh -> result
```

Everything on the wire is **clause 45**. A clause-22 style call is translated:

- register 0 is not a PHY register at all, it is the cached MDC divider
  (`cms50216.ko` uses it as its `mdc_freq` knob: `0x7c` = 1 MHz, `0xc` = 10 MHz,
  `6` = 20 MHz, `4` = 30 MHz);
- register 2 (PHYSID1) becomes MMD 1 reg `0x00cc`, register 3 (PHYSID2) becomes
  MMD 1 reg `0x00cb`;
- any other register n becomes MMD 1 reg n.

Completion is interrupt driven. `lattice_mdio_interrupt_handler` (1.5) gets the
`0x40000` block as `dev_id`, does `ldrh [dev_id]`, completes every master whose
bit is set, then `strh` the handled bits to `[dev_id, #4]`. The driver never
writes an interrupt-enable register. The legacy driver instead reads the
master's own `+0x10` and completes when it equals 1.

Bus bring-up, `lattice_mdio_reset` (called by `mdiobus_register`): `strh 0` to
`+0xc`, `udelay(100)`, `strh 1` to `+0xc`. The bus scan that follows runs at
divider `0x7d`.

### Where the PHYs are and what identifies them

Sixteen MDIO masters, one PHY address each, two per chip:

| master | PHY addr | chip (diag `mdio.yaml`) | misc block |
| --- | --- | --- | --- |
| 1, 2 | `0x02`, `0x03` | U80 | phy misc-1 |
| 3, 4 | `0x04`, `0x05` | U4 | 2 |
| 5, 6 | `0x06`, `0x07` | U6 | 3 |
| 7, 8 | `0x08`, `0x09` | U9 | 4 |
| 9, 10 | `0x0a`, `0x0b` | U11 | 5 |
| 11, 12 | `0x0c`, `0x0d` | U12 | 6 |
| 13, 14 | `0x0e`, `0x0f` | U13 | 7 |
| 15, 16 | `0x10`, `0x11` | U14 | 8 |

Sources: stock dmesg on this unit (`phy[0x2]: device mdio-1:02, driver Credo
CMS50216` … `mdio-16:11`), flash-ONIE `diag/configs/mdio.yaml` (reg `0x00cc`,
mask `0xffff`, default `0x2161` for all 16), and `get_port` in `mdio-fpga.ko`
(port → misc block = (port+1)/2). The chip-to-misc-block column follows from
that pairing and is not independently confirmed.

Identifier: `cms50216_driver.phy_id = 0x216100a0`; the MDIO alias table uses
mask `0xfffffff0`. So MMD 1 `0x00cc` = `0x2161` and MMD 1 `0x00cb` = `0x00a0`
with the low nibble as revision. The driver's own `phy_id_mask` field is zero,
so the dmesg lines only prove that each address returned something other than
all-ones, not that the ID matched. `cms50216.ko` has no ID check of its own;
its raw-access sysfs/ioctl path always uses `MII_ADDR_C45 | (1 << 16) | reg`,
i.e. MMD 1 with a 16-bit register address. Standard 1.2/1.3 are not used by
any vendor code.

### Resets and enables

- PHY resets are bit 0 of each misc block's `+0x0`. Nothing in `install.sh`,
  `mdio-fpga.ko` probe or `mdio-lattice.ko` probe writes them, and the PHYs
  answered the bus scan milliseconds after `mdio-fpga` loaded, so the FPGA
  default is "released". The `--misc` step reads the bits to confirm.
- System CPLD (`0x60`, vendor bus 8), from `sys_cpld.ko` `cpld_get_data`:
  `mdiofpga_reset` = `!(reg 0x06 bit 2)` (diag default for `0x06` is `0xff`,
  not in reset); `macsec_present` = `!(reg 0xa4 bit 6)`; `macsec_lm75_interrupt`
  = `!(reg 0xa4 bit 5)`. `0xa5` "MacSec DC VRM" expects `0x3f` under mask
  `0x3f`, `0xa6` "MacSec 0.9V" expects `0x3f`; the YAML names suggest
  power-good bits but no driver decodes them. The vendor init does not write
  any of these. They are I2C reads and safe to do at any time; since the FPGA
  already enumerates on mainline, `0x06` bit 2 must be set.
- What the vendor does write to each PHY at probe (`cms50216_probe`): divider
  `0x7c`, then MMD 1 reg `0xb73a` = `0x40` (MDIO pad open-drain → push-pull,
  per release notes needed above a few MHz), then divider `6` (20 MHz). None of
  that is needed to read the ID: the bus scan that found the PHYs runs before
  it, at divider `0x7d`.

### New or legacy handshake on this unit: not established

`install.sh` loads `mdio-lattice.ko` when system CPLD reg `0x01` has a non-zero
high nibble (board revision), else the legacy module. The board EEPROM says
R0B-2 and the FPGA reports `0x1008`, later than the `mdio_fpga_v1001_0210.vme`
update shipped next to the February 2021 driver that introduced the new
handshake, so the new one is expected. The captured dmesg cannot tell the two
modules apart. One `i2cget` of CPLD `0x01` settles it. If the guess is wrong
the symptom is a timeout: both `0x40000` (new) and master `+0x10` (legacy) are
registers a vendor driver reads, but I cannot show that each exists in both
FPGA builds.

## 2. i2c_accel_fpga (`1d0f:8571`, mainline `0002:01:00.0`)

Driver 0.11 maps BAR0 only (`pci_ioremap_bar(pdev, 0)` at `fpga_probe+0x130`);
BAR1/BAR2 are requested but never mapped.

| BAR0 offset | width | meaning | source |
| --- | --- | --- | --- |
| `0x0` | 32 | bits 23:8 `version`, bits 7:0 `board_version` | `cpld_version_show` |
| `0x4` | 32 | scratch | `scratch_show` |

```
 a98:  add  x0, x22, #0x8, lsl #12
 a9c:  ldr  x0, [x0, #552]           ; fpga_dev->base
 aa0:  bl   fpga_read32              ; ldr w19, [x19]
 ab4:  strcmp(attr->name, "version")        -> ubfx x2, x19, #8, #16
 acc:  strcmp(attr->name, "board_version")  -> and  w2, w19, #0xff
```

The probe itself reads nothing; it only writes three per-port registers for
each of the 32 I2C masters. The version read happens when
`/sys/devices/xilinx/accel-i2c/version` is read, which ONIE's
`/usr/bin/get_versions` does. No capture of that output exists, so there is no
expected value. The update file name `i2c_fpga_v01.02.0E_20200604.bin` hints
at the format only.

## 3. Innovium ASIC (`1d98:1b58`, mainline `0001:01:00.0`)

`inno_probe` does not read BAR0 at all. In order: `pci_set_master`, three
config-space reads, `pci_find_ext_capability(3)` (serial number),
`pci_request_region(0)`, `pci_ioremap_bar(0)`, DMA mask setup, coherent
allocations, `pci_enable_device`.

```
 5af4:  mov w2, #0x0 ; bl pci_bus_read_config_word   -> dev+0x8014  vendor_id
 5b10:  mov w2, #0x2 ; bl pci_bus_read_config_word   -> dev+0x8016  device_id
 5b2c:  mov w2, #0x8 ; bl pci_bus_read_config_byte   -> dev+0x8018  rev_id
```

Those three values are what `ipd_info_r` prints (`VendorId/DeviceId/RevId`) and
match the `inno_dev_info_t` layout (vendor, device, revision) recovered earlier in
`analysis/research/ipd-abi.json`; I did not trace which ioctl returns it.
`libifcs.so` contains `device_id: 0x1b58`, `Unknown device, node %d device_id
0x%x` and `Unknown Teralynx Revision, node %d rev_id 0x%x`, consistent with the
SDK identifying the chip from the PCI IDs. The stock `lspci` on this unit shows
`1d98:1b58 (rev 10)`. So the read-only identification is the config header,
which mainline already shows.

No reset or endianness setup precedes identification. The first BAR0 traffic
is in `inno_open` and starts with writes:

| step | BAR0 offset | access |
| --- | --- | --- |
| 1 | `0x400c`, `0x4010`, `0x4014`, `0x4018`, `0x401c` | write 0 |
| 2 | `0x13804`, `0x137d4`, `0x137e0`, `0x137ec`, `0x137f8`, `0x13810` | write `0xffffffff` |
| 3 | `0x13808` | 32-bit read, logged as `Swap status` (DWARF name `CSI_PCIE_SWAP_STATUS1`) |

`0x13804` is therefore the endian-swap control (all-ones reads the same in
either byte order) and `0x13808` its status. Chip-internal registers are not
directly mapped; `isn_read_pen` reaches them through an indirect window:
read `0x58` (status, busy = bits 2:1), write address to `0x68`, block selector
to `0x6c`, command to `0x54`, poll `0x58`, data at `0x100+`.

For a BAR0 liveness check the two reads the driver performs are `0x13808` and
`0x58`, both 32-bit. The driver only ever reads `0x13808` after the step-2
writes; nothing suggests the read depends on them, but that is an inference.
They are opt-in in the script (`--asic-bar`) and tell you nothing about chip
identity. I did not reverse `libifcs.so` (stripped, 37 MB), so whether the SDK
also reads an ID register through the indirect window is unknown.

## What I could not establish

- Whether memory decode is currently on for the three devices under mainline.
- Which MDIO completion handshake this FPGA build uses (see above).
- The i2c FPGA version value, and whether its `+0x0` was ever read on this unit.
- Whether the done bit at `0x40000` latches when the FPGA's interrupt cannot be
  delivered (no driver, MSI off). If it does not, the script times out.
- The meaning of individual bits in system CPLD `0xa5`/`0xa6`.
- Low nibble of the PHY ID low word, and what the two MDIO slaves per CMS50216
  correspond to inside the chip.
- That CPython on the switch really emits a single load per register read.

## Probe script

Tested only against a mock sysfs tree made of ordinary files (logic, offsets
and written values checked); never run on the switch. Suggested order:

```
python3 cs8210_pci_probe.py                 # config space + version reads (if decode is on)
python3 cs8210_pci_probe.py --enable        # only if it reported "memory decode is OFF"
python3 cs8210_pci_probe.py --misc          # phy reset bits, gpio, irq state
python3 cs8210_pci_probe.py --phy-id        # WRITES; master 1 only
python3 cs8210_pci_probe.py --phy-id --masters 1-16
```

```python
#!/usr/bin/env python3
"""CS8210-32X: minimal identification probe for the three driverless PCIe
endpoints under mainline Linux.  Standard library only.  Run as root.

Every MMIO access below is one the vendor 4.9.124 modules perform at the same
BAR, offset and width (see reviews/2026-11-09-fpga-asic-probe-maps.md).

Default run (no flags) touches NO device register except:
  * PCI config space through sysfs (vendor/device/revision/command)
  * mdio-fpga  BAR0+0x000000, 32-bit read   (version; vendor reads it in probe)
  * i2c fpga   BAR0+0x000000, 32-bit read   (version; vendor sysfs "version")

Opt-in read-only extras:   --misc   --asic-bar
Opt-in steps that WRITE:   --enable (PCI command register, via sysfs "enable")
                           --phy-id (MDIO master registers; see WRITE SECTION)

Why the width handling looks fussy: an unacknowledged or odd-sized MMIO access
on this SoC ends in an asynchronous SError and a panic.  Registers are read
through memoryview.cast('I'/'H') on the mmap, which CPython implements as a
fixed-size memcpy (one 32-bit or one 16-bit load).  Do NOT replace this with
struct.unpack('<I', ...) or with mmap slicing: both read byte by byte.
"""
import argparse
import mmap
import os
import sys
import time

MDIO_FPGA = ("0004:01:00.0", 0x1D0F, 0xEC30)
I2C_FPGA = ("0002:01:00.0", 0x1D0F, 0x8571)
ASIC = ("0001:01:00.0", 0x1D98, 0x1B58)

# mdio-fpga BAR0 layout (mdio-fpga.ko resource tables + mdio-lattice.ko)
MDIO_VERSION = 0x000000          # u32
MDIO_SCRATCH = 0x000004          # u32
MDIO_IRQ_STATUS = 0x040000       # u16, bit n = master n+1 transaction done
MDIO_IRQ_ACK = 0x040004          # u16, write the bits being acknowledged
MDIO_MISC_BASE = 0x080000        # u32 regs, 8 blocks of 0x20 ("phy misc-N")
MDIO_MASTER_BASE = 0x0C0000      # u16 regs, 16 blocks of 0x100 ("mdio master-N")
M_ADDR, M_REG, M_DATA, M_CTRL, M_LEGACY_DONE = 0x0, 0x4, 0x8, 0xC, 0x10
CTRL_ENABLE, CTRL_START, CTRL_READ, CTRL_ERROR = 0x01, 0x04, 0x08, 0x20
MDC_DIV_DEFAULT = 0x7D           # what mdio-lattice uses for its bus scan


def say(msg):
    """Print and push to the console before the next access, so that if an
    access does fault the last line names it."""
    print(msg, flush=True)
    try:
        os.fsync(sys.stdout.fileno())
    except OSError:
        pass


class Dev:
    def __init__(self, root, bdf, vendor, device):
        self.bdf, self.vendor, self.device = bdf, vendor, device
        self.path = os.path.join(root, bdf)
        self.fd = self.map = self.v32 = self.v16 = None

    def cfg(self):
        with open(os.path.join(self.path, "config"), "rb") as f:
            return f.read(64)

    def check(self, want_enable):
        """PCI-level checks.  Returns True only if MMIO decode is on."""
        if not os.path.isdir(self.path):
            say(f"{self.bdf}: not present, skipping")
            return False
        c = self.cfg()
        ven, dev = int.from_bytes(c[0:2], "little"), int.from_bytes(c[2:4], "little")
        cmd, rev = int.from_bytes(c[4:6], "little"), c[8]
        say(f"{self.bdf}: config vendor={ven:04x} device={dev:04x} rev={rev:02x} "
            f"command={cmd:04x} (mem={'on' if cmd & 2 else 'OFF'}, "
            f"busmaster={'on' if cmd & 4 else 'off'})")
        if (ven, dev) != (self.vendor, self.device):
            say(f"{self.bdf}: unexpected ID, expected "
                f"{self.vendor:04x}:{self.device:04x}; skipping")
            return False
        with open(os.path.join(self.path, "resource")) as f:
            start, end, _flags = (int(x, 16) for x in f.readline().split())
        if start == 0:
            say(f"{self.bdf}: BAR0 is unassigned; skipping")
            return False
        self.bar0_len = end - start + 1
        say(f"{self.bdf}: BAR0 {start:#x}-{end:#x} ({self.bar0_len:#x} bytes)")
        if not cmd & 2:
            if not want_enable:
                say(f"{self.bdf}: memory decode is OFF.  A BAR read now would not "
                    f"be acknowledged (SError).  Re-run with --enable to do what "
                    f"the vendor probe does (pci_enable_device).  Skipping.")
                return False
            say(f"{self.bdf}: WRITE: echo 1 > enable (sets PCI_COMMAND_MEMORY)")
            with open(os.path.join(self.path, "enable"), "w") as f:
                f.write("1")
            cmd = int.from_bytes(self.cfg()[4:6], "little")
            say(f"{self.bdf}: command now {cmd:04x}")
            if not cmd & 2:
                say(f"{self.bdf}: memory decode still off; skipping")
                return False
        return True

    def open(self, writable=False):
        flags = os.O_RDWR if writable else os.O_RDONLY
        prot = mmap.PROT_READ | (mmap.PROT_WRITE if writable else 0)
        self.fd = os.open(os.path.join(self.path, "resource0"), flags | os.O_SYNC)
        self.map = mmap.mmap(self.fd, self.bar0_len, mmap.MAP_SHARED, prot)
        mv = memoryview(self.map)
        self.v32, self.v16 = mv.cast("I"), mv.cast("H")

    def close(self):
        for v in (self.v32, self.v16):
            if v is not None:
                v.release()
        self.v32 = self.v16 = None
        if self.map is not None:
            self.map.close()
        if self.fd is not None:
            os.close(self.fd)
        self.map = self.fd = None

    def _chk(self, off, width):
        if off % width or off + width > self.bar0_len:
            raise ValueError(f"bad offset {off:#x} for width {width}")

    def r32(self, off, what):
        self._chk(off, 4)
        say(f"{self.bdf}: read32  BAR0+{off:#08x}  ({what}) ...")
        val = self.v32[off // 4]
        say(f"{self.bdf}:   = {val:#010x}")
        return val

    def r16(self, off, what, quiet=False):
        self._chk(off, 2)
        if not quiet:
            say(f"{self.bdf}: read16  BAR0+{off:#08x}  ({what}) ...")
        val = self.v16[off // 2]
        if not quiet:
            say(f"{self.bdf}:   = {val:#06x}")
        return val

    def w16(self, off, val, what):
        self._chk(off, 2)
        say(f"{self.bdf}: WRITE16 BAR0+{off:#08x} <- {val:#06x}  ({what})")
        self.v16[off // 2] = val


# --------------------------------------------------------------------------
# READ-ONLY steps
# --------------------------------------------------------------------------
def mdio_fpga_readonly(d, misc):
    ver = d.r32(MDIO_VERSION, "mdio-fpga version; stock kernel logged 00001008")
    say(f"mdio-fpga: Version: {ver:08x}")
    if not misc:
        return
    d.r32(MDIO_SCRATCH, "scratch")
    for n in range(8):
        base = MDIO_MISC_BASE + 0x20 * n
        # same order as phy_update_device(): 0x0, 0xc, 0x4, 0x8
        rst = d.r32(base + 0x0, f"phy misc-{n + 1} reset reg")
        gpio = d.r32(base + 0xC, f"phy misc-{n + 1} gpio")
        irq = d.r32(base + 0x4, f"phy misc-{n + 1} interrupt")
        msk = d.r32(base + 0x8, f"phy misc-{n + 1} interrupt mask")
        say(f"mdio-fpga: phy{n + 1}: reset={'ASSERTED' if not rst & 1 else 'released'} "
            f"gpio[7:0]={gpio & 0xff:#04x} irq={irq & 3:#x} mask={msk & 3:#x}")


def i2c_fpga_readonly(d):
    v = d.r32(0x0, "i2c_accel_fpga version/board word")
    say(f"i2c-fpga: version={(v >> 8) & 0xffff:#X} board_version={v & 0xff:#X}")


def asic_bar_readonly(d):
    d.r32(0x13808, "Innovium CSI_PCIE_SWAP_STATUS1 (ipd.ko inno_open)")
    d.r32(0x00058, "Innovium IAC status (ipd.ko isn_read_pen, first access)")


# --------------------------------------------------------------------------
# WRITE SECTION -- disabled unless --phy-id is given.
#
# Reading a PHY register is not a read-only operation on the FPGA: the MDIO
# master has to be released from reset and given an address and a command.
# This replays exactly what mdio-lattice.ko does (lattice_mdio_reset, then
# lattice_mdio_read for clause-45 1.0x00cc and 1.0x00cb).  It writes only
# FPGA registers; it does not write any PHY register.  Risks:
#   * it changes MDIO-master state the vendor OS would otherwise set itself
#     (harmless; the stock driver redoes the reset in its own probe);
#   * the FPGA raises its "done" interrupt.  With no driver bound MSI is off,
#     so it may assert INTx on PCIe port 4 if DisINTx is clear;
#   * which completion handshake this FPGA build uses is inferred, not
#     observed (see the review).  A wrong guess shows up as a timeout, not as
#     a bus error: every register polled here is one a vendor driver reads.
# --------------------------------------------------------------------------
def mdio_c45_read(d, master, phy, devad, reg, legacy, div):
    m = MDIO_MASTER_BASE + 0x100 * (master - 1)
    bit = 1 << (master - 1)
    d.w16(m + M_ADDR, (phy << 5) | devad, f"master-{master} phy={phy:#x} devad={devad}")
    d.w16(m + M_REG, reg, "register address")
    d.w16(m + M_CTRL, (div << 8) | CTRL_READ | CTRL_START | CTRL_ENABLE, "start read")
    deadline = time.monotonic() + 0.5
    done = False
    while time.monotonic() < deadline:
        if legacy:
            done = d.r16(m + M_LEGACY_DONE, "", quiet=True) == 1
        else:
            done = bool(d.r16(MDIO_IRQ_STATUS, "", quiet=True) & bit)
        if done:
            break
        time.sleep(0.0005)
    if not done:
        say(f"mdio-fpga: master-{master}: no completion within 500 ms")
    elif not legacy:
        d.w16(MDIO_IRQ_ACK, bit, "acknowledge done bit")
    ctrl = d.r16(m + M_CTRL, "control/status")
    data = d.r16(m + M_DATA, "read data")
    if not done:
        return None
    if ctrl & CTRL_ERROR:
        say(f"mdio-fpga: master-{master}: FPGA reports read error (ctrl bit 5)")
        return None
    return data


def mdio_phy_ids(d, masters, legacy, div):
    if not legacy:
        st = d.r16(MDIO_IRQ_STATUS, "pending done bits before starting")
        if st:
            d.w16(MDIO_IRQ_ACK, st, "clear stale done bits")
    for n in masters:
        m = MDIO_MASTER_BASE + 0x100 * (n - 1)
        phy = n + 1
        d.w16(m + M_CTRL, 0x0000, f"master-{n} reset assert")
        time.sleep(0.001)                       # vendor: udelay(100)
        d.w16(m + M_CTRL, CTRL_ENABLE, f"master-{n} reset release")
        time.sleep(0.001)
        id1 = mdio_c45_read(d, n, phy, 1, 0x00CC, legacy, div)
        id2 = mdio_c45_read(d, n, phy, 1, 0x00CB, legacy, div)
        f = lambda v: "----" if v is None else f"{v:04x}"
        verdict = ""
        if id1 is not None and id2 is not None:
            ok = id1 == 0x2161 and (id2 & 0xFFF0) == 0x00A0
            verdict = "Credo CMS50216" if ok else "UNEXPECTED (want 2161/00ax)"
        say(f"mdio-fpga: mdio-{n}:{phy:02x}  1.00cc={f(id1)} 1.00cb={f(id2)}  {verdict}")


def parse_masters(s):
    out = []
    for part in s.split(","):
        lo, _, hi = part.partition("-")
        out.extend(range(int(lo), int(hi or lo) + 1))
    if not out or min(out) < 1 or max(out) > 16:
        raise argparse.ArgumentTypeError("masters must be within 1-16")
    return out


def main():
    ap = argparse.ArgumentParser(description=__doc__,
                                 formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--sysfs", default="/sys/bus/pci/devices")
    ap.add_argument("--mdio-bdf", default=MDIO_FPGA[0])
    ap.add_argument("--i2c-bdf", default=I2C_FPGA[0])
    ap.add_argument("--asic-bdf", default=ASIC[0])
    ap.add_argument("--enable", action="store_true",
                    help="WRITE: turn on PCI memory decode if it is off")
    ap.add_argument("--misc", action="store_true",
                    help="also read mdio-fpga scratch and the 8 phy-misc blocks")
    ap.add_argument("--asic-bar", action="store_true",
                    help="also read two Innovium BAR0 registers (see review)")
    ap.add_argument("--phy-id", action="store_true",
                    help="WRITE: run MDIO transactions to read the PHY IDs")
    ap.add_argument("--masters", type=parse_masters, default=[1],
                    help="MDIO masters for --phy-id, e.g. 1 or 1-16 (default 1)")
    ap.add_argument("--legacy", action="store_true",
                    help="use the R0A completion handshake (master+0x10)")
    ap.add_argument("--mdc-div", type=lambda x: int(x, 0), default=MDC_DIV_DEFAULT)
    a = ap.parse_args()

    say("== mdio-fpga (MACsec MDIO) ==")
    d = Dev(a.sysfs, a.mdio_bdf, *MDIO_FPGA[1:])
    if d.check(a.enable):
        d.open(writable=a.phy_id)
        try:
            mdio_fpga_readonly(d, a.misc)
            if a.phy_id:
                say("-- WRITE SECTION: MDIO transactions --")
                mdio_phy_ids(d, a.masters, a.legacy, a.mdc_div & 0xFF)
        finally:
            d.close()

    say("== i2c_accel_fpga ==")
    d = Dev(a.sysfs, a.i2c_bdf, *I2C_FPGA[1:])
    if d.check(a.enable):
        d.open()
        try:
            i2c_fpga_readonly(d)
        finally:
            d.close()

    say("== Innovium ASIC ==")
    d = Dev(a.sysfs, a.asic_bdf, *ASIC[1:])
    ok = d.check(a.enable and a.asic_bar)
    say("asic: identification is the PCI config line above "
        "(ipd.ko reads nothing else to identify the chip)")
    if a.asic_bar and ok:
        d.open()
        try:
            asic_bar_readonly(d)
        finally:
            d.close()


if __name__ == "__main__":
    main()
```
