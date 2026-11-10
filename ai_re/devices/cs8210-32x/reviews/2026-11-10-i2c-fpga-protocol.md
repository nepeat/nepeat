# 2026-11-10 — I2C FPGA (`1d0f:8571`) master transaction protocol

Offline work only. I disassembled the vendor driver, ran the tool against a
simulated register file in session scratch, and touched nothing on the
switch. **None of this has run on hardware yet.**

Source: `analysis/onie-live-2026-11-10/driver/i2c_accel_fpga.ko`, v0.11
(md5 `30972f03…`, vermagic 4.9.124, `srcversion 48E2D00BF10F6B5C0C287CF`),
disassembled with `aarch64-unknown-linux-gnu-objdump -dr` (relocations
applied). I cross-checked it against the SSD copy (`analysis/extracted/root/driver/`,
v0.09, md5 `69e227f5…`): `fpga_i2c_transaction` is byte-for-byte the same and
the transfer constants are the same. Only the probe defaults and the
profile-byte layout differ (see §6). Builds on
[2026-11-10-fpga-roles-and-datapath.md](2026-11-10-fpga-roles-and-datapath.md) §1.1.

Marking: **V** = I read it directly in the disassembly. **I** = I inferred it.
Addresses are `.text` offsets in the v0.11 module.

Deliverables:

- this review
- [`i2c-fpga/cs8210-qsfp.py`](i2c-fpga/cs8210-qsfp.py), a userspace tool
  (standard library only); the same source is listed in §9

## Summary

- **The FPGA is a register-oriented I2C engine, not a raw bit/byte master.**
  Each transaction is one command word (direction, 7-bit address, length)
  plus one start word (start bit, 8-bit register offset). The FPGA generates
  the whole `S addr+W offset [Sr addr+R data… | data…] P` sequence itself.
  Data passes through a 512-byte write window and a 512-byte read window per
  channel. Every access is a 32-bit `readl`/`writel` on BAR0. BAR1 and BAR2
  are never mapped. **V**
- **Completion is polled, not interrupt driven.** The driver polls bit 31 of
  the start register (it self-clears). It makes 50 tries with
  `usleep_range(1000, 2000)` *before* each read, about 50–100 ms in total, then
  returns `-EBUSY`. After that it reads a status register **once, in every
  case**. Bit 1 of status set means `-ENXIO`. No other status bit is decoded.
  The MSI is used only for module presence changes. **V**
- **Per-channel layout** (ch = port−1, 0..31):
  - control block at `0x1000 + ch*0x100`, spanning `0x1000..0x2FFF`
  - write window at `0x4000 + ch*0x200`
  - read window at `0x8000 + ch*0x200`
  - The driver never uses more than the first 0x80 bytes of either window.
    **V**
- **The protocol can only do "register at 8-bit offset" transfers.**
  - It cannot do a plain read with no register write first, 16-bit offsets,
    10-bit addresses, SMBus block (length-prefixed) transfers, or quick
    commands. **V** for the driver's usage, **I** for the hardware.
  - That is exactly what SFF-8636 / CMIS need at `0x50`: byte or block reads
    at an offset, and a byte write to 127 for page select.
- **On mainline the vendor probe has never run**, so the per-channel clock
  and enable registers hold whatever the FPGA powers up with. The probe also
  writes 0 to the module-reset register for all cages. Until that happens,
  modules may be held in reset (unknown). The tool shows these registers and
  has `--init` to apply the vendor probe values.
- **Vendor bug (does not affect the tool):** in `fpga_i2c_master_xfer`, a
  message longer than 128 bytes is split into 128-byte chunks, but every
  chunk is copied to or from the start of `msg->buf`. So `I2C_RDWR` reads of
  more than 128 bytes return corrupted data under the stock driver. SMBus
  paths are capped at 32 and are not affected. **V**

## 1. Register map

Only accesses the v0.11 driver makes are listed. All are 32-bit
(`fpga_read32` = `ldr w; dsb ld`; every store is `dsb st; str w`. I grepped
every store that follows a `dsb st`: all are `str w`, and the `ldrb/ldrh/strb/strh`
in the file all target kernel memory, never MMIO). **V**

### 1.1 Global (BAR0)

| offset | R/W by driver | meaning | evidence | conf. |
| --- | --- | --- | --- | --- |
| `0x000` | R | bits 23:8 = `version`, 7:0 = `board_version` (`0x010b0009` on this unit → 0x0b00 / 0x09) | `cpld_version_show` `aa0 bl fpga_read32(base)`, `ac4 ubfx x2,x19,#8,#16`, `ae0 and w2,w19,#0xff` | V |
| `0x004` | R/W | scratch | `scratch_show a20 add x0,x0,#0x4`; `scratch_store f58/f64` | V |
| `0x140` | R | module present, bit p−1 (1 = present per ONIE `bin/qsfp`, earlier review) | `fpga_bit_show ba4 mov x19,#0x140; bbc bl fpga_read32; bc8 asr; bcc and #1` | V offset / I polarity |
| `0x150` | R, W `0xffffffff` | presence-change status, cleared by writing ones | `fpga_interrupt_handler ec add x0,x0,#0x150; f0 bl fpga_read32; 10c str w23(-1)` | V |
| `0x158` | W only | presence-change IRQ mask: `0xffffffff` on entry, `0` on exit | handler `b0 add #0x158; bc str w23(-1)` … `138 str wzr`; probe `13cc`, `140c` | V |
| `0x160` | R | module interrupt (IntL), bit p−1 | `fpga_bit_show c9c mov x19,#0x160` | V offset / polarity unknown |
| `0x168` | R/W (RMW) | module interrupt mask, bit p−1 | `fpga_bit_store 55c mov x23,#0x168 … 57c read … 5ac str` | V offset / polarity unknown |
| `0x170` | R/W (RMW) | LPMode, bit p−1 (1 = LPMode asserted, **I**) | `fpga_bit_store 630 mov x23,#0x170` | V offset |
| `0x178` | R/W (RMW) | module reset, bit p−1, 1 = ResetL asserted (ONIE `sfp_eeprom` string, earlier review) | `fpga_bit_store 5e8 mov x23,#0x178`; probe `142c str wzr`; handler `dc str wzr` | V offset / I polarity |

### 1.2 Per channel, `cb = 0x1000 + ch*0x100` (ch = port−1)

| offset | R/W by driver | meaning | evidence | conf. |
| --- | --- | --- | --- | --- |
| `cb+0x00` | R/W (RMW) | clock profiles: byte k = SCL code of profile k+1. Codes: `0x18` = 400 kHz, `0x09` = 1 MHz, `0x66` = 100 kHz; anything else is "Unknown clock setting" (`-ENODATA`). Probe value `0x00660918` gives profile1 = 400 k, profile2 = 1 M, profile3 = 100 k | `fpga_i2c_profile_store 3f0 add w21,w21,#0xf; 3fc lsl #8` (= `(port+15)<<8`); `414 lsl w1,w20,#3` (byte = profile−1); `48c #0x18`/`3dc #0x9`/`46c #0x66` ↔ 400000/1000000/100000; show `894-910` the inverse | V |
| `cb+0x08` | R/W (RMW) | bits 1:0 = selected profile − 1 (0 → profile1 = 400 kHz). Sysfs shows 1..3 | `fpga_bit_store 6a4 mov w2,#0x1008; 6ac mov w21,#-4` (mask `~3`), value−1 at `694`; `fpga_bit_show d0c…d44 and #0x3` | V |
| `cb+0x0c` | R/W (RMW) | bit 0 = `i2c_master_reset` (sysfs RMW of bit 0 only). Probe writes `0x80000000` (bit 31 meaning unknown, **I**: enable) | `fpga_bit_store 6dc mov w1,#0x100c; 6e4 mov w21,#-2`; `fpga_bit_show cd4`; probe `11cc mov w0,#0x80000000; 11d0 str` | V write, I meaning |
| `cb+0x50` | **W only** | command: bit 28 = read; bits 15:8 = 7-bit slave address (unshifted); bits 7:0 = byte count. Write = bit 28 clear | `fpga_rtc_read 16b8 mov w0,#0x1050; 16dc orr w24,w22,w24,lsl #8; 16e0 orr w24,w24,#0x10000000; 16e4 str` / `fpga_rtc_write 1600 orr w25,w1,w0,lsl #8; 1604 str` | V layout, I field widths |
| `cb+0x54` | R/W | start: write `0x80000000 \| offset`; bit 31 reads 1 while busy and self-clears when done. Bits 7:0 = register offset. A write **without** bit 31 only stages the offset, and the driver reads it back as "current offset" | `fpga_rtc_read 1710 orr w27,w27,#0x80000000; 1714 str`; `fpga_i2c_transaction 1c0 bl fpga_read32; 1c4 tbnz w0,#31`; `master_xfer 1f88 str w25(buf[0])` (no bit 31), `1fc0 bl fpga_read32` | V |
| `cb+0x60` | **R only** | status: bit 1 = error → `-ENXIO`. Read once after every poll loop | `fpga_i2c_transaction 1d0 mov w0,#0x1060; 1dc bl fpga_read32; 1e0 tst x0,#0x2; 1e4 mov w1,#-6; 1e8 csel` | V bit 1, rest unknown |

### 1.3 Data windows

| range | access | meaning | evidence | conf. |
| --- | --- | --- | --- | --- |
| `0x4000 + ch*0x200 + 4*i`, i < 32 | W only | write data, little-endian bytes in 32-bit words (byte n = bits 8*(n%4)) | `fpga_rtc_write 1584 add w1,w1,#0x20; 158c lsl w1,w1,#9; 157c lsl x5,x19,#2; 15a0 str w26`; byte-data write `1874 str w21` (zero-extended byte) | V |
| `0x8000 + ch*0x200 + 4*i`, i < 32 | R only | read data, same byte order; `ceil(n/4)` words are read, the last one partly copied | `fpga_rtc_read 1750 add w1,w1,#0x40; 1754 lsl #9; 1760 bl fpga_read32; 1778 memcpy(buf, &w, 4 or n&3)` | V |

Maximum length per transaction actually issued by the driver:

- 128 bytes on the `master_xfer` path (`1ee4`/`1fe4 cmp #0x80; csel`)
- 32 on the SMBus I2C-block path (`18cc`/`1ce8 cmp w5,#0x20; b.hi` → clamp 32)

The window is 0x200 bytes, but nothing above 0x7c is ever touched. **V**

Also enforced in `master_xfer`: `offset + len − 1 ≤ 0xff`, otherwise
`-ENOMEM` (`1ecc`, `1fcc cmp #0xff; b.gt 2030`). **V**

Port mapping: adapter `nr = 0x65 + idx` (`12a8 add w1,w26,#0x65`), and
`chan = idx` is stored at adapter+0x408 (`12d0`). So port P = channel P−1 =
`i2c-(100+P)`. **V** (Whether that matches the faceplate order was never
checked with a module.)

## 2. Read transaction (e.g. EEPROM bytes `O..O+n−1` at address `A`)

From `fpga_rtc_read.isra.0` (0x1670) + `fpga_i2c_transaction` (0x160). **V**

Recovered signatures:

- `fpga_rtc_read.isra.0(int *chan, struct device *dev, void __iomem **base, u16 addr, u16 offset, u16 len, u8 *buf)`
- `fpga_rtc_write.isra.0(...)`: the same

They are called with `addr = client addr`, `offset = SMBus command / buf[0]`
and `len = byte count`.

```
1. writel(0x10000000 | A<<8 | n,  cb+0x50)      ; 16e4
2. writel(0x80000000 | O,          cb+0x54)      ; 1714
3. repeat up to 50 times:                         ; 188 mov w19,#50
       usleep_range(1000, 2000)                   ; 1b4
       v = readl(cb+0x54); if !(v & BIT31) break  ; 1c0/1c4
   if all 50 were busy: dev_err "i2c transaction is busy."; ret = -EBUSY   ; 204-214
4. s = readl(cb+0x60)  (always)                   ; 1dc
   if s & BIT1: ret = -ENXIO   (overrides -EBUSY) ; 1e0-1e8
5. if ret == 0: for i in 0..ceil(n/4)-1:          ; 1730 cbnz w0 -> skip
       w = readl(0x8000 + ch*0x200 + 4*i); copy LE bytes   ; 1760/1778
```

Exception: the SMBus `BYTE_DATA` and `WORD_DATA` read paths read word 0 of
the read window **even when the transaction failed**, and store it into
`data->byte`/`word` (`1ca0`, `1ba4` run after the transaction regardless). So
reading the window after an error is also a demonstrated access. **V**

There is no busy check before step 1, no status clear, and no reset or cleanup
after an error. **V** The next transaction just starts. **I:** status bit 1 is
per-transaction and cleared by the next start. Without that, a NACK on an
empty cage would break that channel until reboot, and the stock tools show no
such behaviour.

## 3. Write transaction (e.g. page select: byte `0x7f` = page)

From `fpga_rtc_write.isra.0` (0x1500) and the `BYTE_DATA` write path
(0x1838). **V**

```
1. for each 4 data bytes i: writel(LE word, 0x4000 + ch*0x200 + 4*i)  ; 15a0 / 1874
2. writel(A<<8 | n, cb+0x50)          (bit 28 clear = write)           ; 1604 / 1a88 (18a4 orr #1)
3. writel(0x80000000 | O, cb+0x54)                                     ; 1638 / 1ab8
4. poll + status exactly as in §2 steps 3-4                            ; 164c / 1acc
```

`n` counts data bytes only, not the offset. For a partial last word the upper
bytes are uninitialised stack (`memcpy(&w, src, n&3)`), so the FPGA must
ignore bytes beyond `n` (**I**). On the bus: `S A+W O d0..d(n−1) P` (**I**).

## 4. Algorithm callbacks

`i2c_algorithm` in `.data+0x0` = { `master_xfer` = 0x1e20, `smbus_xfer` =
0x17c8, `functionality` = 0x0 } (from `.rela.data`). `functionality` returns
`0x0c7e0001` = `I2C | SMBUS_BYTE | BYTE_DATA | WORD_DATA | I2C_BLOCK`. That
means no QUICK, PEC, 10-bit, PROC_CALL or SMBus BLOCK. **V**

`fpga_i2c_smbus_xfer` holds the per-adapter mutex (`adap+0x3e0`) for the whole
transfer. **V**

| size | read | write |
| --- | --- | --- |
| `BYTE` (1) | `cmd=0x10000001\|A<<8`, `start=0x80000000` (offset 0, `1990-19cc`); **the data is never read back** | the same as read (the command byte is ignored) |
| `BYTE_DATA` (2) | `cmd=0x10000001\|A<<8`, `start=0x80000000\|command`, word 0 of the read window → `data->byte` (`1c34-1cb0`) | wbuf[0] = `data->byte`, `cmd=1\|A<<8`, `start=0x80000000\|command` (`1840-1acc`) |
| `WORD_DATA` (3) | the same with n = 2; `data->word` = low 16 bits of word 0, so the first byte on the wire is the LSB (`1b38`, `1bb4 strh`) | wbuf[0] = word, n = 2 (`1a20`, `1a84 orr #2`) |
| `I2C_BLOCK_DATA` (8) | n = `block[0]` clamped to 32 → `fpga_rtc_read` (`1ce4-1d10`) | the same → `fpga_rtc_write` (`18c8-18f4`) |
| others | `dev_err "Unsupported transaction %d"`, `-EOPNOTSUPP` (`1928`); the i2c core then falls back to emulation through `master_xfer` | |

`fpga_i2c_master_xfer` (0x1e20) holds the same mutex for the whole message
list. **V**

For each message:

- **Write message, `len == 1`:** `writel(buf[0], cb+0x54)`, with no start bit.
  This only stages the register offset (`1f64-1f88`).
- **Write message, `len > 1`:**
  1. `offset = buf[0]`; return `-ENOMEM` if `offset + len − 1 > 0xff`.
  2. Send `buf[1..]` in chunks of up to 128 bytes through `fpga_rtc_write`,
     advancing `offset` after each chunk (`1ec0-1f1c`).
  3. Bug: the source pointer is reset to `msg->buf+1` for every chunk (`1ee0`).
- **Read message:**
  1. `offset = readl(cb+0x54)`, the staged offset (`1fc0`).
  2. Return `-ENOMEM` if `offset + len − 1 > 0xff`.
  3. Read in chunks of up to 128 bytes through `fpga_rtc_read` into
     `msg->buf`. The same pointer bug applies (`1fe8`).
- **Result:** `num` if the last chunk's return value is ≥ 0, else that error.
  Earlier errors are overwritten, and later messages still run after a
  failure (`1f40-1f44`).

Locking: a global `pcie_lock` mutex (in `.bss`) wraps every *single* MMIO
access. The per-adapter `i2c_lock` wraps a whole transfer. Channels are
therefore driven concurrently and independently. **V**

## 5. Probe-time initialisation (`fpga_probe`, 0xfe0)

Order as executed: **V**

1. `of_find_compatible_node(NULL, NULL, "i2c_accel_fpga")`; `-ENODEV` if absent.
2. `pci_enable_device` (this sets memory decode, which the stock dmesg shows
   as `enabling device (0000 -> 0002)`), `pci_set_master`,
   `pci_request_regions`, `pci_intx(1)`.
3. `pci_ioremap_bar(pdev, 0)`. **BAR0 only.**
4. Register root device `xilinx`, link it as `accel-i2c`, create the sysfs
   group.
5. For idx = 0..31:
   1. `of_get_child_by_name("port%d")` becomes the adapter's `of_node`.
   2. Set up the adapter: name `"i2c_accel_fpga I2C Adapter Port %d"`,
      class `0x100`, retries 1, `nr = 101+idx`.
   3. `i2c_add_numbered_adapter`.
   4. **Then** (with the adapter already live):
      - `writel(0x80000000, cb+0x0c)` (`11d0`)
      - `writel(0, cb+0x08)` (`11f4`)
      - `writel(0x00660918, cb+0x00)` (`1220`)
6. `writel(0xffffffff, 0x158)`, `writel(0xffffffff, 0x150)`,
   `writel(0, 0x158)`, then **`writel(0, 0x178)`: releases every cage from
   reset** (`13cc-142c`).
7. `pci_alloc_irq_vectors(1, 1, PCI_IRQ_MSI)`, then
   `request_threaded_irq(vec0, fpga_interrupt_handler, NULL, IRQF_TRIGGER_RISING, "FPGA MSI", fpga_dev)`.

`0x160`, `0x168` and `0x170` are not touched by the probe.

`fpga_remove` writes no registers.

The IRQ handler (presence change):

1. mask `0x158`, then write 0 to `0x178` (releasing all cages from reset)
2. read `0x150` and write `0xffffffff` back to clear it
3. OR the value into the software `present_changed` latch
4. unmask `0x158`

It never touches the I2C channels. **V**

Other sysfs actions that touch the channels:

- `port<P>_i2c_9_clock` (store only, value ≤ 1, value otherwise ignored;
  `fpga_bit_store 724-7c8`) writes `cb+0x50 = 0x5001`, `cb+0x54 = 0x80000000`
  and runs the poll. That is a one-byte **write** to address `0x50`,
  offset 0, with whatever is in the write window. The name suggests SCL bus
  recovery, but in the driver it is just a dummy write transaction. **V**
  for the sequence; the hardware meaning is unknown. The tool does not
  implement it.
- `port<P>_i2c_master_reset`: RMW of bit 0 of `cb+0x0c`. Bit 31 is preserved.
  **V**

## 6. Timing, errors, speed

| item | value | conf. |
| --- | --- | --- |
| poll interval | `usleep_range(1000, 2000)` **before** each status read, ≥ 1 ms per transaction even when it completes at once | V |
| timeout | 50 polls, about 50–100 ms; `-EBUSY` with `"i2c transaction is busy."` | V |
| error | `cb+0x60` bit 1 → `-ENXIO`. Whether that means address NACK only, data NACK too, or also arbitration loss or a stretch timeout is unknown. No other bits are decoded | V / unknown |
| hardware timeout | unknown. Whether the FPGA ever clears busy on a stuck bus (SDA held low) is unknown | — |
| default speed | profile select 0 → profile1 byte = `0x18` = 400 kHz (release notes 0.0.4: "profile0 (400KHz) as default") | V |
| code formula | **I:** SCL ≈ 40 MHz / (4·(code+1)): 0x18 → 400 kHz, 0x09 → 1 MHz, 0x66 → 97 kHz. This fits all three codes, but I have not confirmed it | I |
| v0.09 difference | probe wrote `profile = 0x00090018`, `select = 1`. Profile bytes are read from a different layout (`lsr #16/#24`). The release notes for 0.0.8 (driver 0.11) say "Realign i2c freq profile select according to the latest spec". FPGA version 0x0b00 pairs with driver 0.11 (**I**), so use the 0.11 encoding | V / I |

Duration at 400 kHz: a 32-byte read is about 0.9 ms of bus time and
128 bytes about 3 ms, comfortably inside the timeout. At 100 kHz, 128 bytes
take about 12 ms (**I**, arithmetic).

## 7. Mainline `i2c_adapter`: design sketch (not built, not tested)

Keep to the shapes the vendor proves:

- **`smbus_xfer`:** implement `BYTE_DATA`, `WORD_DATA` and `I2C_BLOCK_DATA`.
  Optionally implement `BYTE` read as a 1-byte read at offset 0, which is
  what the vendor does (for `i2cdetect -r`).
- **`master_xfer`:** accept only these message shapes; everything else gets
  `-EOPNOTSUPP`:
  - [W(1) offset, R(n)]
  - [W(1+n)]
  - a lone R(n) at the staged offset
- **Chunking:** at most 128 bytes per FPGA transaction, with
  `off + n ≤ 256`. Advance the buffer pointer, which the vendor forgets to do.
- **Functionality:** `I2C_FUNC_SMBUS_BYTE_DATA | SMBUS_WORD_DATA | SMBUS_I2C_BLOCK`,
  plus `I2C_FUNC_I2C` if the restricted `master_xfer` is included (at24 or
  optoe-style EEPROM drivers then work with 128-byte reads).

```c
#define CH(c)          (0x1000 + (c) * 0x100)
#define  CH_PROFILE    0x00   /* 0x00660918: p1 400k, p2 1M, p3 100k */
#define  CH_PROFSEL    0x08   /* bits 1:0 = profile-1 */
#define  CH_MRESET     0x0c   /* probe: 0x80000000; bit0 = master reset */
#define  CH_CMD        0x50   /* W only */
#define   CMD_READ     BIT(28)
#define  CH_START      0x54
#define   START_BUSY   BIT(31)
#define  CH_STATUS     0x60   /* R only */
#define   STATUS_ERR   BIT(1)
#define WBUF(c)        (0x4000 + (c) * 0x200)
#define RBUF(c)        (0x8000 + (c) * 0x200)

static int cs_fpga_xfer(struct cs_chan *c, u8 addr, u8 off, bool rd, u8 *buf, u32 n)
{
	void __iomem *cb = c->bar + CH(c->idx);
	u32 v, i, j;
	int ret;

	if (!n || n > 128 || off + n > 256)
		return -EINVAL;
	if (!rd) {
		for (i = 0; i < n; i += 4) {
			for (v = 0, j = 0; j < 4 && i + j < n; j++)
				v |= (u32)buf[i + j] << (8 * j);
			writel(v, c->bar + WBUF(c->idx) + i);
		}
		writel((addr << 8) | n, cb + CH_CMD);
	} else {
		writel(CMD_READ | (addr << 8) | n, cb + CH_CMD);
	}
	writel(START_BUSY | off, cb + CH_START);
	/* sleep_before_read = true: same as the vendor, first read after >= 1 ms */
	ret = read_poll_timeout(readl, v, !(v & START_BUSY), 1000, 100000, true,
				cb + CH_START);
	v = readl(cb + CH_STATUS);          /* always, as the vendor does */
	if (v & STATUS_ERR)
		return -ENXIO;
	if (ret)
		return -ETIMEDOUT;
	if (rd)
		for (i = 0; i < n; i += 4) {
			v = readl(c->bar + RBUF(c->idx) + i);
			for (j = 0; j < 4 && i + j < n; j++)
				buf[i + j] = v >> (8 * j);
		}
	return 0;
}
```

Probe:

1. `pcim_enable_device`, then `pcim_iomap_regions(BIT(0))`.
2. For each channel, **before** registering its adapter, write the vendor
   values: `CH_MRESET = 0x80000000`, `CH_PROFSEL = 0`,
   `CH_PROFILE = 0x00660918`.
3. Write `0x158 = ~0`, `0x150 = ~0`, `0x158 = 0`.
4. Whether to also write `0x178 = 0` (release every cage from reset) is a
   policy choice. The vendor does it, both at probe and on every presence
   change.
5. Register adapters with `nr = 100 + port` to keep the vendor bus numbers.
6. Use a per-channel mutex. No global lock is needed (**I**; the vendor's
   global lock only serialises single accesses).
7. Presence: either poll `0x140` or take the MSI and replicate the handler.
   The MSI is optional for I2C.
8. On `-ETIMEDOUT`, a bit-0 pulse on `CH_MRESET` is the only recovery knob
   the driver exposes. It is unknown whether that recovers a stuck bus.

## 8. What I could not establish

- **The power-on values** of `cb+0x00/0x08/0x0c` and `0x170/0x178` on a unit
  where the vendor probe never ran (mainline from cold boot). The tool reads
  and prints them first, and refuses transactions unless they match the
  vendor probe values (`--init` writes them; `--force` overrides). If the
  unit was warm-rebooted from ONIE, the FPGA may still hold the vendor state.
- **Status register `cb+0x60`:** only bit 1 is known. I don't know whether
  it is sticky, what the other bits mean, or which failure kinds set bit 1.
- **`cb+0x0c` bit 31** (written 1 by probe) and the polarity of bit 0. The
  inference is 1 = reset, because the probe leaves it 0 and transfers then
  work.
- **Field widths:** the length field (8 bits? is 0 or more than 128 legal?),
  the address field (bits 14:8 or 15:8), and whether any other command bits
  exist, for example a "no offset" or "current address" read. I assume none,
  because the driver never sets any.
- **Bus-level shape:** whether a read is a repeated START or STOP+START
  after the offset, and whether the FPGA waits out clock stretching. This is
  irrelevant for QSFP, but matters for odd devices.
- **The SCL code formula** (only the three codes are known), byte 3 of
  `cb+0x00`, and what `i2c_9_clock` really does in the FPGA.
- **Polarities** of `0x160` (interrupt) and `0x168` (mask), and LPMode = 1
  meaning asserted. These are inferred from the names only.
- **Cage order on the faceplate**, i.e. that channel N is cage N. The driver
  says so; I never checked it with a module.
- **The CPython width assumption.** `memoryview.cast('I')` gives one 4-byte
  `memcpy`, which compiles to a single 32-bit load or store. The same
  approach worked on this unit's mdio-fpga with 16-bit accesses in
  `cs8210-pci-probe`. I did not inspect the machine code of the switch's
  Python.

## 9. Userspace tool: `reviews/i2c-fpga/cs8210-qsfp.py`

What it does:

- Mmaps `/sys/bus/pci/devices/<bdf>/resource0`. It auto-finds `1d0f:8571`;
  this is `0002:01:00.0` under both ONIE and mainline so far.
- Refuses to touch BAR0 unless PCI COMMAND bit 1 (memory decode) is set.
  With `--enable` it does `echo 1 > enable` first, which is what
  `pci_enable_device` does in the vendor probe.
- Refuses if a kernel driver is bound to the device.
- Holds a `flock` so two copies cannot interleave transactions.
- Every access passes an allowlist that is exactly the vendor-proven set:
  - **Global reads:** `0x0`, `0x4`, `0x140`, `0x150`, `0x160`, `0x168`, `0x170`, `0x178`.
  - **Global writes:** `0x168`, `0x170`, `0x178`.
  - **Per-channel reads:** `+0x00`, `+0x08`, `+0x0c`, `+0x54`, `+0x60`.
  - **Per-channel writes:** `+0x00`, `+0x08`, `+0x0c`, `+0x50`, `+0x54`.
  - **Read window:** reads only, first 0x80 bytes.
  - **Write window:** writes only, first 0x80 bytes.
  - Never read: `+0x50`. Never written: `+0x60`.
- Transactions follow §2 and §3 exactly: sleep 1 ms before each poll, 50
  polls, status read once, bit 1 wins.

Before any I2C transaction it checks:

- the module is present
- the module is not held in reset
- the channel registers equal the vendor probe values
- the busy bit is clear

It refuses otherwise (`--force` overrides).

Tested offline against a simulated register file in scratch (status table,
decode-off refusal, `--info` decode, NACK path, empty-cage and reset
refusals, page select, `--init`, `--speed`, `--master-reset`). After every
run the harness checked each logged access against the allowlist and found
none outside it.

Suggested first run on the switch (mainline). Each step is an explicit
decision for the owner:

```sh
python3 cs8210-qsfp.py                     # refuses if decode is off
python3 cs8210-qsfp.py --enable --channels # sideband + per-channel regs (reads only)
# if per-channel regs are not 80000000 / 0 / 00660918:
python3 cs8210-qsfp.py --init all          # vendor probe values (writes)
# if a populated cage shows reset=1:
python3 cs8210-qsfp.py --reset 17 0        # release ResetL on cage 17 (write)
python3 cs8210-qsfp.py --info 17           # lower page + upper page 00h, decoded
python3 cs8210-qsfp.py --read 17 0x50 0 256 --out /tmp/p17.bin
python3 cs8210-qsfp.py --read 17 0x50 128 128 --page 1 --chunk 128   # page 01h, restores page 0
```

Source (identical to the file):

```python
#!/usr/bin/env python3
"""cs8210-qsfp: QSFP-DD cage sideband + module EEPROM access through the
CS8210-32X "i2c_accel_fpga" (PCI 1d0f:8571) under mainline Linux, with no
kernel driver bound.  Standard library only.  Run as root.

Protocol and register map: reviews/2026-11-10-i2c-fpga-protocol.md, recovered
from the vendor driver i2c_accel_fpga.ko v0.11.  Every MMIO access this tool
can make is one that driver performs at the same BAR (BAR0), offset and width
(32-bit readl/writel).  An allowlist in Dev._chk() enforces that; anything
else raises before touching the device.  Unacknowledged MMIO on this SoC is
an asynchronous SError and a kernel panic, so do not widen the allowlist
without new evidence.

Port P (1..32) = FPGA channel P-1 = vendor bus i2c-(100+P) = sideband bit P-1.

Default (no action flags): print sideband state of all 32 cages.  READ ONLY:
  32-bit reads of BAR0 +0x0 (version), +0x140 present, +0x150 presence change,
  +0x160 interrupt, +0x168 interrupt mask, +0x170 lpmode, +0x178 reset.
  --channels  adds per-channel config reads (+0x1000/+0x1008/+0x100c/+0x1054/
              +0x1060 of each channel block).

I2C traffic (writes FPGA *control* registers, not the module):
  --read P A O N   read N bytes at register offset O from 7-bit address A on
                   cage P.  e.g. --read 1 0x50 0 256
  --info P         read bytes 0..255 at 0x50 and decode identifier/vendor/PN/SN

Opt-in WRITES (each flag is the explicit opt-in):
  --enable           PCI: echo 1 > /sys/bus/pci/devices/<bdf>/enable
  --init P|all       per-channel registers exactly as the vendor probe writes
                     them: +0x100c=0x80000000, +0x1008=0, +0x1000=0x00660918
  --speed P 100k|400k|1m   profile select (+0x1008 bits 1:0), needs --init values
  --master-reset P   pulse bit 0 of +0x100c (vendor sysfs port<P>_i2c_master_reset)
  --lpmode P 0|1     +0x170 bit P-1 (read-modify-write, as the vendor sysfs)
  --reset P 0|1      +0x178 bit P-1 (1 = module held in reset)
  --reset-pulse P    reset 1, 300 ms, 0 (what ONIE bin/sfp_eeprom does)
  --write P A O B..  I2C write of bytes B.. at register offset O (module write!)
  --page PG          with --read/--info: write PG to byte 127 first, restore 0
                     afterwards (what sfp_module.ko does)

Access width: registers are accessed through memoryview.cast('I') on the
mmap of resource0, which CPython implements as one fixed-size 4-byte memcpy
(a single 32-bit load/store).  Do NOT replace with struct.unpack or mmap
slicing: those access byte by byte.
"""
import argparse
import fcntl
import mmap
import os
import sys
import time

VENDOR, DEVICE = 0x1D0F, 0x8571
DEFAULT_BDF = "0002:01:00.0"
MAP_LEN = 0x10000          # everything the vendor driver touches is < 0xC000

# ---- global registers (BAR0) -------------------------------------------
R_VERSION = 0x000          # bits 23:8 version, 7:0 board version
R_SCRATCH = 0x004
R_PRESENT = 0x140          # bit p-1: 1 = module present
R_PRES_CHG = 0x150         # presence-change status (IRQ handler reads, W1C 0xffffffff)
R_PRES_CHG_MASK = 0x158    # written only (0xffffffff / 0)
R_INT = 0x160              # module interrupt (IntL), bit p-1
R_INT_MASK = 0x168
R_LPMODE = 0x170
R_RESET = 0x178            # bit p-1: 1 = ResetL asserted

# ---- per-channel block: 0x1000 + ch*0x100 ------------------------------
CH_BASE, CH_STRIDE, NCH = 0x1000, 0x100, 32
C_PROFILE = 0x00           # byte k = clock code of profile k+1 (0x18/0x09/0x66)
C_PROFSEL = 0x08           # bits 1:0 = profile - 1
C_MRESET = 0x0C            # bit 0 = i2c master reset, bit 31 set by probe
C_CMD = 0x50               # bit 28 read, bits 15:8 7-bit addr, bits 7:0 length
C_START = 0x54             # bit 31 start/busy, bits 7:0 register offset
C_STATUS = 0x60            # bit 1 = error (vendor returns -ENXIO)

WBUF, RBUF, BUF_STRIDE = 0x4000, 0x8000, 0x200   # + ch*0x200
BUF_MAX = 0x80             # vendor never uses more than 128 bytes per transfer

CMD_READ = 1 << 28
START_BUSY = 1 << 31
STATUS_ERR = 1 << 1

PROBE_MRESET, PROBE_PROFSEL, PROBE_PROFILE = 0x80000000, 0x0, 0x00660918
CLOCK_CODES = {0x18: 400000, 0x09: 1000000, 0x66: 100000}
SPEED_TO_PROFILE = {"400k": 1, "1m": 2, "100k": 3}   # with PROBE_PROFILE

POLLS = 50                 # vendor: 50 x usleep_range(1000, 2000) then -EBUSY
DEFAULT_CHUNK = 32         # what sfp_module / i2cdump -i use (I2C_BLOCK_DATA)

READ_OK_GLOBAL = {R_VERSION, R_SCRATCH, R_PRESENT, R_PRES_CHG, R_INT,
                  R_INT_MASK, R_LPMODE, R_RESET}
WRITE_OK_GLOBAL = {R_INT_MASK, R_LPMODE, R_RESET}      # all vendor bit_store RMW
READ_OK_CH = {C_PROFILE, C_PROFSEL, C_MRESET, C_START, C_STATUS}
WRITE_OK_CH = {C_PROFILE, C_PROFSEL, C_MRESET, C_CMD, C_START}

VERBOSE = False


def say(msg):
    """Print and flush before the next access, so that if an access does
    fault, the last line on the console names it."""
    print(msg, flush=True)
    try:
        os.fsync(sys.stdout.fileno())
    except OSError:
        pass


class I2CError(Exception):
    pass


class Dev:
    def __init__(self, pci_root, bdf):
        self.bdf = bdf
        self.path = os.path.join(pci_root, bdf)
        self.fd = self.map = self.v32 = None
        self.writable = False

    def _cfg(self):
        with open(os.path.join(self.path, "config"), "rb") as f:
            return f.read(64)

    def check(self, want_enable, force):
        """PCI-level checks.  Returns True only if MMIO decode is on."""
        if not os.path.isdir(self.path):
            say(f"{self.bdf}: no such PCI device")
            return False
        c = self._cfg()
        ven, dev = int.from_bytes(c[0:2], "little"), int.from_bytes(c[2:4], "little")
        cmd = int.from_bytes(c[4:6], "little")
        say(f"{self.bdf}: vendor={ven:04x} device={dev:04x} command={cmd:04x} "
            f"(mem={'on' if cmd & 2 else 'OFF'}, busmaster={'on' if cmd & 4 else 'off'})")
        if (ven, dev) != (VENDOR, DEVICE):
            say(f"{self.bdf}: not {VENDOR:04x}:{DEVICE:04x}; refusing")
            return False
        drv = os.path.join(self.path, "driver")
        if os.path.exists(drv) and not force:
            say(f"{self.bdf}: bound to kernel driver "
                f"{os.path.basename(os.path.realpath(drv))}; refusing (use --force)")
            return False
        with open(os.path.join(self.path, "resource")) as f:
            start, end, _flags = (int(x, 16) for x in f.readline().split())
        if start == 0:
            say(f"{self.bdf}: BAR0 unassigned; refusing")
            return False
        self.bar0_len = end - start + 1
        if self.bar0_len < MAP_LEN:
            say(f"{self.bdf}: BAR0 only {self.bar0_len:#x} bytes; refusing")
            return False
        if not cmd & 2:
            if not want_enable:
                say(f"{self.bdf}: PCI memory decode is OFF.  A BAR access now is "
                    f"not acknowledged (SError panic).  Re-run with --enable to do "
                    f"what the vendor probe does (pci_enable_device).  Refusing.")
                return False
            say(f"{self.bdf}: WRITE: echo 1 > enable (sets PCI_COMMAND_MEMORY)")
            with open(os.path.join(self.path, "enable"), "w") as f:
                f.write("1")
            cmd = int.from_bytes(self._cfg()[4:6], "little")
            say(f"{self.bdf}: command now {cmd:04x}")
            if not cmd & 2:
                say(f"{self.bdf}: memory decode still off; refusing")
                return False
        return True

    def open(self, writable):
        flags = os.O_RDWR if writable else os.O_RDONLY
        prot = mmap.PROT_READ | (mmap.PROT_WRITE if writable else 0)
        self.fd = os.open(os.path.join(self.path, "resource0"), flags | os.O_SYNC)
        self.map = mmap.mmap(self.fd, MAP_LEN, mmap.MAP_SHARED, prot)
        self.v32 = memoryview(self.map).cast("I")
        self.writable = writable

    def close(self):
        if self.v32 is not None:
            self.v32.release()
            self.v32 = None
        if self.map is not None:
            self.map.close()
            self.map = None
        if self.fd is not None:
            os.close(self.fd)
            self.fd = None

    @staticmethod
    def allowed(off, write):
        if off % 4:
            return False
        if off < CH_BASE:
            return off in (WRITE_OK_GLOBAL if write else READ_OK_GLOBAL)
        if off < CH_BASE + NCH * CH_STRIDE:
            r = (off - CH_BASE) % CH_STRIDE
            return r in (WRITE_OK_CH if write else READ_OK_CH)
        base = WBUF if write else RBUF
        if base <= off < base + NCH * BUF_STRIDE:
            return (off - base) % BUF_STRIDE < BUF_MAX
        return False

    def _chk(self, off, write):
        if not self.allowed(off, write):
            raise ValueError(f"BAR0+{off:#x} {'write' if write else 'read'} is not "
                             f"an access the vendor driver makes; refusing")
        if write and not self.writable:
            raise ValueError("mapping is read-only")

    def r32(self, off, what=""):
        self._chk(off, False)
        if VERBOSE:
            say(f"  read32  BAR0+{off:#07x} {what}")
        val = self.v32[off // 4]
        if VERBOSE:
            say(f"          = {val:#010x}")
        return val

    def w32(self, off, val, what=""):
        self._chk(off, True)
        if VERBOSE:
            say(f"  write32 BAR0+{off:#07x} <- {val:#010x} {what}")
        self.v32[off // 4] = val & 0xFFFFFFFF

    def rmw_bit(self, off, bit, value, what):
        old = self.r32(off, what)
        new = (old & ~(1 << bit)) | ((value & 1) << bit)
        say(f"WRITE {what}: BAR0+{off:#x} {old:#010x} -> {new:#010x}")
        self.w32(off, new, what)


# ---- per-channel helpers -----------------------------------------------
def chb(port):
    return CH_BASE + (port - 1) * CH_STRIDE


def channel_regs(d, port):
    b = chb(port)
    return {"profile": d.r32(b + C_PROFILE, "profile"),
            "profsel": d.r32(b + C_PROFSEL, "profile select"),
            "mreset": d.r32(b + C_MRESET, "master reset"),
            "start": d.r32(b + C_START, "start/busy"),
            "status": d.r32(b + C_STATUS, "status")}


def speed_of(regs):
    sel = regs["profsel"] & 3
    if sel == 3:
        return None
    return CLOCK_CODES.get((regs["profile"] >> (8 * sel)) & 0xFF)


def channel_ready(d, port, force):
    """Refuse transactions on a channel that is not in the state the vendor
    probe leaves it in, or that looks busy."""
    r = channel_regs(d, port)
    spd = speed_of(r)
    msg = (f"port {port}: profile={r['profile']:#010x} sel={r['profsel'] & 3} "
           f"mreset={r['mreset']:#010x} start={r['start']:#010x} "
           f"status={r['status']:#010x} speed={spd or 'unknown'}")
    say(msg)
    problems = []
    if r["mreset"] != PROBE_MRESET:
        problems.append("+0x100c is not the vendor probe value 0x80000000 "
                        "(bit 0 set = master in reset?)")
    if spd is None:
        problems.append("selected clock profile code is not one the vendor knows")
    if r["start"] & START_BUSY:
        problems.append("start/busy bit 31 already set (stuck transaction?)")
    if problems:
        for p in problems:
            say(f"port {port}: {p}")
        if not force:
            say(f"port {port}: refusing; run --init {port} (vendor probe values) "
                f"or --master-reset {port}, or pass --force")
            return False
    return True


def xfer(d, port, addr, off, write_data=None, nread=0):
    """One FPGA transaction, exactly as fpga_rtc_read / fpga_rtc_write +
    fpga_i2c_transaction do it."""
    ch = port - 1
    b = chb(port)
    if write_data is not None:
        n = len(write_data)
        if not 1 <= n <= 32:
            raise ValueError("write length must be 1..32")
    else:
        n = nread
        if not 1 <= n <= BUF_MAX:
            raise ValueError(f"read length must be 1..{BUF_MAX}")
    if not 0 <= off <= 0xFF or off + n > 0x100:
        raise ValueError("offset + length must stay within 0x00..0xff")
    if not 0x03 <= addr <= 0x77:
        raise ValueError("7-bit address must be 0x03..0x77")

    if write_data is not None:
        # 1. data words into the channel write buffer (LE, 32-bit stores)
        for i in range(0, n, 4):
            word = int.from_bytes(bytes(write_data[i:i + 4]).ljust(4, b"\0"), "little")
            d.w32(WBUF + ch * BUF_STRIDE + i, word, "write buffer")
        # 2. command: addr<<8 | len, read bit clear
        d.w32(b + C_CMD, (addr << 8) | n, "cmd (write)")
    else:
        # 1. command: read bit | addr<<8 | len
        d.w32(b + C_CMD, CMD_READ | (addr << 8) | n, "cmd (read)")
    # 3. start | register offset
    d.w32(b + C_START, START_BUSY | off, "start")
    # 4. poll: sleep first, then read; 50 tries (vendor)
    busy = True
    for _ in range(POLLS):
        time.sleep(0.001)
        if not d.r32(b + C_START, "poll") & START_BUSY:
            busy = False
            break
    # 5. status is read unconditionally, bit 1 wins over the timeout
    st = d.r32(b + C_STATUS, "status")
    kind = "read" if write_data is None else "write"
    if st & STATUS_ERR:
        raise I2CError(f"port {port} {kind} addr {addr:#04x} off {off:#04x} len {n}: "
                       f"status {st:#010x} bit 1 set (NACK/error; vendor -ENXIO)")
    if busy:
        raise I2CError(f"port {port} {kind}: still busy after {POLLS} polls "
                       f"(vendor: 'i2c transaction is busy.', -EBUSY); status {st:#010x}")
    if write_data is not None:
        return None
    # 6. data from the channel read buffer, 32-bit loads, little-endian bytes
    out = bytearray()
    for i in range(0, n, 4):
        out += d.r32(RBUF + ch * BUF_STRIDE + i, "read buffer").to_bytes(4, "little")
    return bytes(out[:n])


def read_range(d, port, addr, off, n, chunk):
    if off + n > 0x100:
        raise ValueError("offset + length must be <= 256")
    out = bytearray()
    pos = off
    while pos < off + n:
        k = min(chunk, off + n - pos)
        say(f"i2c read  port {port} addr {addr:#04x} off {pos:#04x} len {k}")
        out += xfer(d, port, addr, pos, nread=k)
        pos += k
    return bytes(out)


def write_bytes(d, port, addr, off, data):
    say(f"WRITE i2c port {port} addr {addr:#04x} off {off:#04x} data "
        f"{' '.join(f'{x:02x}' for x in data)}")
    xfer(d, port, addr, off, write_data=data)


# ---- output -------------------------------------------------------------
def hexdump(data, base):
    for i in range(0, len(data), 16):
        row = data[i:i + 16]
        hx = " ".join(f"{x:02x}" for x in row)
        asc = "".join(chr(x) if 0x20 <= x < 0x7F else "." for x in row)
        print(f"{base + i:02x}: {hx:<47}  {asc}")


SFF8024_ID = {0x03: "SFP/SFP+", 0x0C: "QSFP", 0x0D: "QSFP+ (SFF-8636)",
              0x11: "QSFP28 (SFF-8636)", 0x18: "QSFP-DD (CMIS)",
              0x19: "OSFP (CMIS)", 0x1E: "QSFP+ or later (CMIS)"}


def decode_info(m):
    ident = m[0]
    print(f"identifier  {ident:#04x} {SFF8024_ID.get(ident, '?')}")

    def s(a, b):
        return m[a:b].decode("ascii", "replace").strip()

    if ident in (0x18, 0x19, 0x1E):
        t = int.from_bytes(m[14:16], "big", signed=True) / 256
        print(f"CMIS rev    {m[1] >> 4}.{m[1] & 0xf}   current page (byte 127) {m[127]}")
        print(f"temperature {t:.1f} C")
        print(f"vendor      {s(129, 145)!r}  OUI {m[145:148].hex()}")
        print(f"part number {s(148, 164)!r} rev {s(164, 166)!r}")
        print(f"serial      {s(166, 182)!r} date {s(182, 190)!r}")
    elif ident in (0x0C, 0x0D, 0x11):
        t = int.from_bytes(m[22:24], "big", signed=True) / 256
        print(f"rev compl.  {m[1]:#04x}   page select (byte 127) {m[127]}")
        print(f"temperature {t:.1f} C")
        print(f"vendor      {s(148, 164)!r}  OUI {m[165:168].hex()}")
        print(f"part number {s(168, 184)!r} rev {s(184, 186)!r}")
        print(f"serial      {s(196, 212)!r} date {s(212, 220)!r}")
    if m[127] != 0:
        print("note: byte 127 != 0, so bytes 128..255 are not upper page 00h; "
              "re-run with --page 0")


def show_status(d, channels):
    v = d.r32(R_VERSION, "version")
    say(f"i2c_accel_fpga version={(v >> 8) & 0xFFFF:#06x} board={v & 0xFF:#04x} "
        f"(raw {v:#010x})")
    pres = d.r32(R_PRESENT, "present")
    chg = d.r32(R_PRES_CHG, "presence change")
    intr = d.r32(R_INT, "interrupt")
    imask = d.r32(R_INT_MASK, "interrupt mask")
    lpm = d.r32(R_LPMODE, "lpmode")
    rst = d.r32(R_RESET, "reset")
    say(f"raw: present={pres:08x} pres_chg={chg:08x} int={intr:08x} "
        f"int_mask={imask:08x} lpmode={lpm:08x} reset={rst:08x}")
    hdr = "port bus     present reset lpmode int int_mask pres_chg"
    if channels:
        hdr += "   profile    sel mreset     start      status     speed"
    print(hdr)
    for p in range(1, NCH + 1):
        bit = 1 << (p - 1)

        def f(x):
            return "1" if x & bit else "0"
        line = (f"{p:4d} i2c-{100 + p:<3d} {f(pres):>7} {f(rst):>5} {f(lpm):>6} "
                f"{f(intr):>3} {f(imask):>8} {f(chg):>8}")
        if channels:
            r = channel_regs(d, p)
            line += (f"   {r['profile']:08x}  {r['profsel'] & 3}  {r['mreset']:08x}  "
                     f"{r['start']:08x}  {r['status']:08x}  {speed_of(r) or '?'}")
        print(line)
    print("present 1 = module in cage; reset 1 = ResetL asserted (held in reset); "
          "lpmode 1 = LPMode asserted (inferred); int: raw bit, polarity unconfirmed")


# ---- main ---------------------------------------------------------------
def find_bdf(pci_root):
    hits = []
    for name in sorted(os.listdir(pci_root)):
        try:
            with open(os.path.join(pci_root, name, "vendor")) as f:
                ven = int(f.read(), 16)
            with open(os.path.join(pci_root, name, "device")) as f:
                dev = int(f.read(), 16)
        except OSError:
            continue
        if (ven, dev) == (VENDOR, DEVICE):
            hits.append(name)
    return hits


def port_arg(s):
    p = int(s, 0)
    if not 1 <= p <= NCH:
        raise argparse.ArgumentTypeError("port must be 1..32")
    return p


def num(s):
    return int(s, 0)


def main():
    global VERBOSE
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0],
                                 formatter_class=argparse.RawDescriptionHelpFormatter,
                                 epilog=__doc__.split("\n\n", 1)[1])
    ap.add_argument("--bdf", help=f"PCI address (default: scan for "
                    f"{VENDOR:04x}:{DEVICE:04x}, usually {DEFAULT_BDF})")
    ap.add_argument("--pci-root", default="/sys/bus/pci/devices",
                    help="sysfs PCI device directory (for offline testing)")
    ap.add_argument("--lock-file", default="/run/lock/cs8210-qsfp.lock")
    ap.add_argument("--enable", action="store_true",
                    help="WRITE: enable PCI memory decode if it is off")
    ap.add_argument("--force", action="store_true",
                    help="skip the presence / channel-state / bound-driver refusals")
    ap.add_argument("-v", "--verbose", action="store_true", help="log every MMIO access")
    ap.add_argument("--channels", action="store_true",
                    help="with the status table: per-channel config registers")
    ap.add_argument("--read", nargs=4, type=num, metavar=("P", "A", "O", "N"))
    ap.add_argument("--info", type=port_arg, metavar="P")
    ap.add_argument("--chunk", type=num, default=DEFAULT_CHUNK,
                    help=f"bytes per FPGA transaction, 1..{BUF_MAX} (default 32)")
    ap.add_argument("--out", help="also write the bytes read to this file")
    ap.add_argument("--page", type=num, metavar="PG",
                    help="WRITE: select page PG (byte 127) before reading, restore 0 after")
    ap.add_argument("--write", nargs="+", type=num, metavar="X",
                    help="WRITE to module: P A O BYTE [BYTE...]")
    ap.add_argument("--init", metavar="P|all",
                    help="WRITE: per-channel vendor probe values")
    ap.add_argument("--speed", nargs=2, metavar=("P", "100k|400k|1m"),
                    help="WRITE: clock profile select")
    ap.add_argument("--master-reset", type=port_arg, metavar="P",
                    help="WRITE: pulse I2C master reset (bit 0 of +0x100c)")
    ap.add_argument("--lpmode", nargs=2, type=num, metavar=("P", "0|1"))
    ap.add_argument("--reset", nargs=2, type=num, metavar=("P", "0|1"))
    ap.add_argument("--reset-pulse", type=port_arg, metavar="P")
    a = ap.parse_args()
    VERBOSE = a.verbose

    # ---- argument validation before touching anything ----
    for opt in ("lpmode", "reset"):
        v = getattr(a, opt)
        if v and (not 1 <= v[0] <= NCH or v[1] not in (0, 1)):
            ap.error(f"--{opt} P 0|1 with P in 1..32")
    if a.read:
        p, addr, off, n = a.read
        if not 1 <= p <= NCH or not 1 <= n or off < 0 or off + n > 256:
            ap.error("--read P A O N: P 1..32, O+N <= 256")
    if a.write and (len(a.write) < 4 or not 1 <= a.write[0] <= NCH
                    or any(not 0 <= b <= 0xFF for b in a.write[3:])):
        ap.error("--write P A O BYTE [BYTE...]")
    if not 1 <= a.chunk <= BUF_MAX:
        ap.error(f"--chunk must be 1..{BUF_MAX}")
    if a.page is not None and not 0 <= a.page <= 0xFF:
        ap.error("--page 0..255")
    init_ports = []
    if a.init:
        init_ports = list(range(1, NCH + 1)) if a.init == "all" else [port_arg(a.init)]
    if a.speed:
        a.speed = (port_arg(a.speed[0]), a.speed[1].lower())
        if a.speed[1] not in SPEED_TO_PROFILE:
            ap.error("--speed P 100k|400k|1m")

    writes = bool(init_ports or a.speed or a.master_reset or a.lpmode or a.reset
                  or a.reset_pulse or a.write or a.read or a.info)
    status_only = not writes

    if a.bdf:
        bdf = a.bdf
    else:
        hits = find_bdf(a.pci_root)
        if len(hits) != 1:
            say(f"found {len(hits)} {VENDOR:04x}:{DEVICE:04x} devices {hits}; use --bdf")
            return 2
        bdf = hits[0]

    lockdir = os.path.dirname(a.lock_file) or "."
    lockpath = a.lock_file if os.path.isdir(lockdir) else "/tmp/cs8210-qsfp.lock"
    lfd = os.open(lockpath, os.O_RDWR | os.O_CREAT, 0o600)
    try:
        fcntl.flock(lfd, fcntl.LOCK_EX | fcntl.LOCK_NB)
    except OSError:
        say(f"another cs8210-qsfp holds {lockpath}; refusing")
        return 2

    d = Dev(a.pci_root, bdf)
    if not d.check(a.enable, a.force):
        return 1
    d.open(writable=writes)
    rc = 0
    try:
        for p in init_ports:
            b = chb(p)
            say(f"WRITE init port {p}: +{b + C_MRESET:#x}=0x80000000 "
                f"+{b + C_PROFSEL:#x}=0 +{b + C_PROFILE:#x}=0x00660918")
            d.w32(b + C_MRESET, PROBE_MRESET, "master reset reg")
            d.w32(b + C_PROFSEL, PROBE_PROFSEL, "profile select")
            d.w32(b + C_PROFILE, PROBE_PROFILE, "profile clocks")

        if a.speed:
            p, spd = a.speed
            b = chb(p)
            prof = d.r32(b + C_PROFILE, "profile clocks")
            if prof != PROBE_PROFILE:
                say(f"port {p}: profile register {prof:#010x} is not the vendor "
                    f"value; refusing --speed (run --init {p})")
                return 1
            old = d.r32(b + C_PROFSEL, "profile select")
            new = (old & ~3) | (SPEED_TO_PROFILE[spd] - 1)
            say(f"WRITE port {p} profile select {old:#x} -> {new:#x} ({spd})")
            d.w32(b + C_PROFSEL, new, "profile select")

        if a.master_reset:
            p = a.master_reset
            b = chb(p)
            d.rmw_bit(b + C_MRESET, 0, 1, f"port {p} i2c master reset assert")
            time.sleep(0.01)
            d.rmw_bit(b + C_MRESET, 0, 0, f"port {p} i2c master reset release")

        if a.lpmode:
            d.rmw_bit(R_LPMODE, a.lpmode[0] - 1, a.lpmode[1], f"port {a.lpmode[0]} lpmode")
        if a.reset:
            d.rmw_bit(R_RESET, a.reset[0] - 1, a.reset[1], f"port {a.reset[0]} reset")
        if a.reset_pulse:
            p = a.reset_pulse
            d.rmw_bit(R_RESET, p - 1, 1, f"port {p} reset assert")
            time.sleep(0.3)
            d.rmw_bit(R_RESET, p - 1, 0, f"port {p} reset release")

        i2c_port = (a.read[0] if a.read else a.info if a.info
                    else a.write[0] if a.write else None)
        if i2c_port:
            bit = 1 << (i2c_port - 1)
            pres = d.r32(R_PRESENT, "present")
            rst = d.r32(R_RESET, "reset")
            if not pres & bit:
                say(f"port {i2c_port}: no module present (+0x140 bit {i2c_port - 1} = 0)")
                if not a.force:
                    say("refusing (pass --force to try anyway)")
                    return 1
            if rst & bit:
                say(f"port {i2c_port}: module is held in reset (+0x178 bit set); "
                    f"it will not answer.  Release with --reset {i2c_port} 0")
                if not a.force:
                    say("refusing (pass --force to try anyway)")
                    return 1
            if not channel_ready(d, i2c_port, a.force):
                return 1

        if a.write:
            p, addr, off = a.write[:3]
            write_bytes(d, p, addr, off, bytes(a.write[3:]))

        if a.read or a.info:
            if a.read:
                p, addr, off, n = a.read
            else:
                p, addr, off, n = a.info, 0x50, 0, 256
            paged = a.page is not None
            if paged:
                if addr != 0x50:
                    say("--page only makes sense at address 0x50; refusing")
                    return 1
                write_bytes(d, p, addr, 0x7F, bytes([a.page]))
            try:
                data = read_range(d, p, addr, off, n, a.chunk)
            finally:
                if paged and a.page != 0:
                    write_bytes(d, p, addr, 0x7F, b"\0")
            hexdump(data, off)
            if a.out:
                with open(a.out, "wb") as f:
                    f.write(data)
                say(f"wrote {len(data)} bytes to {a.out}")
            if a.info:
                decode_info(data)

        if status_only or a.lpmode or a.reset or a.reset_pulse or a.init or a.channels:
            if not (a.read or a.info or a.write):
                show_status(d, a.channels)
    except I2CError as e:
        say(f"I2C error: {e}")
        rc = 3
    finally:
        d.close()
        os.close(lfd)
    return rc


if __name__ == "__main__":
    sys.exit(main())
```
