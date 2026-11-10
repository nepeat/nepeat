# 2026-11-09 — aux SPI (TPM + i2cfpga flash) reads all-ones on mainline: offline review

Offline only (disassembly + DT reading). Nothing on the switch was touched.
Scratch files (ELF with symbols, disassemblies) are in
`/tmp/claude-1001/-home-erin-nepeat-ai-re/e946fb7a-738a-4d95-8778-a420c2d8b8a7/scratchpad/tpm-agent/`
(`vendor.elf`, `dis.txt`, `dw1.txt`, `ub.dis`, `ub.dts`).

## Result in one paragraph

The TPM and the n25q256a are **not on two controller chip selects**. Both hang
off the aux controller's **SS0**, behind a **board-level mux/gate driven by two
SoC GPIOs, PBS GPIO 52 and PBS GPIO 54** (PL061 bank `gpio6` at `0xfd898000`,
bits 4 and 6). The vendor 4.9.124 `dw_spi_set_cs()` has a Celestica hack that
`strcmp()`s the SPI device's modalias and sets those two GPIOs before every
chip-select change, and `dw_spi_setup()` forces every device's chip select to
0. U-Boot's `do_bootm_states()` sets the pair to the **flash** position
immediately before jumping to any OS (`bootm` and `booti` alike). So mainline
starts with the bus routed to the flash, talks to "the TPM" on SS0 (the flash
is listening, MISO floats high -> `0xFFFF`) and to "the flash" on SS1 (not
routed anywhere -> `ff ff ff`). Mainline's DTS has no PL061 node at all, so
nothing can ever move the mux.

Confidence: **high (~85-90%)** that this is the root cause. The software side
is read directly from disassembly; the hardware meaning of the two GPIOs is
inference (see "not established").

## Evidence

### 1. Vendor kernel `dw_spi_set_cs` (verified, disassembly)

Symbols recovered with `vmlinux-to-elf` from `analysis/kernel-review/vendor-Image`.
`dw_spi_set_cs` is at `0xffff00000875e158`. `x19` = `struct spi_device *`,
`x19+0x2f0` = `spi->modalias`, `w23` = the `enable` argument (4.9 semantics:
true = deassert).

```
875e178: add  x21, x19, #0x2f0          ; spi->modalias
875e184: adrp x1, ...d95000 ; add #0xca0 ; "slb9670"
875e198: bl   strcmp
875e19c: cbz  w0, 875e270                ; -> TPM branch
875e1a0: adrp x1, ...d8f000 ; add #0x90  ; "mt25ql256"
875e1ac: bl   strcmp
875e1b0: cbz  w0, 875e208                ; -> flash branch

flash branch (modalias "mt25ql256"):
875e208: x2="PBS52"  x1=#2 (GPIOF_OUT_INIT_HIGH)  w0=#460  bl gpio_request_one ; bl gpio_free
875e224: x2="PBS54"  x1=#0 (GPIOF_OUT_INIT_LOW)   w0=#462  bl gpio_request_one ; bl gpio_free

TPM branch (modalias "slb9670"):
875e270: x2="PBS52"  x1=#0 (OUT_INIT_LOW)   w0=#460  bl gpio_request_one ; bl gpio_free
875e28c: x2="PBS54"  x1=#2 (OUT_INIT_HIGH)  w0=#462  bl gpio_request_one ; bl gpio_free

common tail:
875e1b4: chip->cs_control(!enable) if set (not set here: no cs-gpios in DT)
875e1cc: cbnz w23 -> SER = 0 ; udelay(1)
875e1d0: ldrb w2,[x19,#724] ; SER = 1 << spi->chip_select ; udelay(1)
```

Strings at `0xffff000008d95ca0..`: `slb9670\0PBS52\0PBS54\0No irq, use poll mode\n`
(file offset `0xd15ca0`), and `mt25ql256` at `0xffff000008d8f090`.

So per chip-select change:

| device (modalias)       | GPIO 460 "PBS52" | GPIO 462 "PBS54" |
|-------------------------|------------------|------------------|
| `slb9670` (TPM)         | output **0**     | output **1**     |
| `mt25ql256` (flash)     | output **1**     | output **0**     |

The GPIOs are written on both assert and deassert, always before SER, and the
level persists after `gpio_free`. The state therefore only changes when the
*other* device is used — it does not move during traffic to one device, which
is likely why sampling "during transfers" showed no GPIO change.

### 2. Vendor `dw_spi_setup` forces chip select 0 (verified)

```
875e370: strb wzr, [x19, #1]
875e378: strb wzr, [x20, #724]     ; spi->chip_select = 0   (x20 = spi_device)
```

`#724` is `spi_device.chip_select` in this build (`master` @712, `max_speed_hz`
@720, `chip_select` @724, `mode` @726, `controller_state` @736, `modalias`
@752 = 0x2f0, `cs_gpio` @784 — all consistent with the other uses in the
function). This is why **SER read 1 for flash traffic** even though the DT
says `reg = <1>`: both devices use SS0.

Nothing else is special in the vendor driver: `dw_spi_transfer_one` is the
stock 4.9 sequence (SSIENR=0, BAUDR, CTRLR0, IMR mask, SSIENR=1, poll loop of
`dw_writer`/`dw_reader`), and no code in `dw_spi_add_host`/`dw_spi_mmio_probe`
writes `+0xf4` or anything in the `0xfd8be000` pre-block. The `al_spi_*` HAL
functions in the image have no callers outside the HAL itself.

### 3. GPIO 460/462 = PBS GPIO 52/54 = PL061 `gpio6` bits 4/6 (strong inference)

- `pl061_probe` (`0xffff0000084d7488`) sets `gc.base = -1`, `ngpio = 8` in the
  DT path; `gpiochip_add_data` allocates downward from `ARCH_NR_GPIOS` = 512
  (`mov w20,#0x200` / `cmp w20,#0x1ff`). The string `baseidx` does not occur in
  the kernel image, so the DT `baseidx` property is not used by Linux.
- dmesg registers `gpio0..gpio7` in order at 2.31 s, nothing earlier:
  gpio0=504, gpio1=496, ..., gpio5=464, **gpio6=456**, gpio7=448.
- 460 = gpio6 bit 4 = PBS GPIO 48+4 = **52**; 462 = gpio6 bit 6 = **54**. The
  request labels are literally `"PBS52"` and `"PBS54"`.

Not run-time verified (no `/sys/kernel/debug/gpio` capture), but the labels
make an off-by-one-chip mistake very unlikely.

### 4. Firmware agrees (verified in the dumps)

- DT embedded in `mtd0-uboot.bin` at offset 266312 (stage-2/U-Boot control DT),
  `/soc/board-cfg/gpio_init`: `gpio-list` contains `<52 out 0> <53 out 1>
  <54 out 1>` and there is a dedicated `gpio-list-s2 = <0x34 1 0  0x35 1 1
  0x36 1 1>` (also in the small stage-2 DTB at offset 109724). That is the
  **TPM position** (52=0, 54=1), set in stage 2 — which needs the TPM for
  measured boot.
- The ONIE/kernel DT (`runtime.dts`) `gpio-list` has `<0x34 1 1> ... <0x36 1 0>`
  = the **flash position**. The vendor kernel does not parse `gpio-list` (no
  such string in the Image), so this only documents intent.
- U-Boot proper, file offset `0x1e0878` in `mtd0-uboot.bin`, in the function
  that walks the `BOOTM_STATE_*` bits (tests bit 6/7/8 = OS_CMDLINE/OS_BD_T/
  OS_PREP with `blr x26` = the OS boot function, then bit 10 = OS_GO):

  ```
  1e0878: mov w1, #1 ; mov w0, #0x34 ; bl 0x1f2a34    ; gpio 52 := 1
  1e0888: mov w1, #0 ; mov w0, #0x36 ; bl 0x1f2a34    ; gpio 54 := 0
  1e0894: tbz w21, #10, ...                           ; BOOTM_STATE_OS_GO
  1e08ac: bl  0x1e0e68                                ; boot_selected_os()
  ```

  `0x1f2a34` range-checks `gpio <= 63`, indexes a per-bank handle array by
  `gpio & ~7` and calls the HAL setter with `gpio & 7` (its sibling just above
  does `al_gpio_dir_set` + the same setter = `gpio_direction_output`). This is
  the only place in U-Boot that references the 52/54 pair.

  => **Every OS handoff (bootm or booti) leaves the mux in the flash position
  (52=1, 54=0).** This also plausibly explains the "red herring"
  `al_tpm_tis20_end: write failed / TPM extend failed` printed on `booti`:
  something still tries the TPM after the mux was flipped (not verified).

### 5. Why mainline reads ones (follows from 1-4)

- `tpm@0` -> SER=1 -> SS0 -> mux in flash position -> the flash sees TIS
  framing, never drives MISO -> `TPM vid 0xFFFF`.
- `flash@1` -> SER=2 -> SS1, which is not a routed pin (only pins 32/33/34/39
  are muxed to SPIM_AUX) -> `ff ff ff`.
- The mainline DTS and `alpine-v3.dtsi` contain no PL061/GPIO nodes, so the
  mux cannot move.

Mainline's DW driver itself looks fine for this IP: `amazon,alpine-dw-apb-ssi`
writes `CS_OVERRIDE=0xF` (same value the vendor register shows), the boot-flash
controller proves multi-transfer messages keep CS asserted, and the irqless
patch gives the same poll path as vendor. RX_SAMPLE_DLY=0 matches vendor aux.
I found no DFS32 / FIFO / mem-ops difference that would matter.

## What I could NOT establish

1. **What the two GPIOs do electrically** (true 2:1 mux with complementary
   selects, two active-low buffer enables, ...). Only the two vendor states
   (0,1)=TPM and (1,0)=flash are known-good. (0,0) and (1,1) occur only as
   brief transients in vendor code, always with CS deasserted. GPIO 53 is also
   driven high by stage 2; purpose unknown — leave it alone.
2. **The failed hand-driven test.** Under the vendor kernel, a hand JEDEC read
   on aux with SER=1 only works if the mux was last left on the flash; if the
   TPM was the last device used it must fail. But if the same hand test was
   also run under *mainline* (mux should be on flash at entry) with SER=1 and
   still gave `ff`, that contradicts this theory and needs explaining first.
   PROGRESS.md does not say which kernel the aux hand test was run under
   besides vendor. Experiment E0/E2 below settles it.
3. Why earlier devmem sampling saw no PL061 change. Expected explanation: the
   bits only change between TPM and flash use, not during one device's traffic;
   or `GPIODATA` was read at an offset other than `+0x3fc` (PL061 data reads
   are address-masked; `+0x000` always reads 0).
4. The exact U-Boot function names (binary has no symbols there); identified
   by structure only.
5. Whether `tpm_tis_spi` behaves well through `spi-mux` (reasoned from source,
   not tested).

## Experiments / fixes, in order

PL061 `gpio6` registers (physical): `GPIODATA` all bits `0xfd8983fc`, bit 4
only `0xfd898040`, bit 6 only `0xfd898100`, bits 4+6 `0xfd898140`, `GPIODIR`
`0xfd898400`. These are implemented PL061 registers (the vendor kernel's
driver reads/writes them), so they should not hit the PBS unimplemented-read
SError — but that is untested on mainline `/dev/mem`; read `0xfd8983fc` first
and stop if it faults.

### E0 — read the mux state (read-only, 1 minute)

On mainline as booted: `devmem 0xfd8983fc 32` and `devmem 0xfd898400 32`.
Prediction: `(data & 0x50) == 0x10` (flash position), `(dir & 0x50) == 0x50`.
Optional cross-checks: at the U-Boot prompt `md.l 0xfd8983fc 1` should give
`(v & 0x50) == 0x40` (TPM position); under ONIE the value should flip between
`0x40` after a TPM access and `0x10` after reading `/dev/mtd5`.

### E1 — flip to the TPM by hand, rebind (no rebuild)

```
devmem 0xfd898040 32 0x00      # PBS52 := 0   (vendor order: 52 first)
devmem 0xfd898100 32 0x40      # PBS54 := 1
# only if E0 showed the direction bits clear:
#   devmem 0xfd898400 32 $(( <dir> | 0x50 ))
echo spi1.0 > /sys/bus/spi/drivers/tpm_tis_spi/bind
dmesg | tail
```

Prediction: `2.0 TPM (vendor-id 0x15D1, device-id 0x1B ...)`. This is the
decisive test of the whole theory.

### E2 — flash by hand in the flash position (optional)

Put back `0xfd898040 := 0x10`, `0xfd898100 := 0x00`, then the existing
hand-driven JEDEC read on `0xfd8be100` with **SER=1**. Prediction: `20 ba 19`.

### F1 — proper DT fix (mirrors the vendor behaviour, no kernel patch)

Needs `CONFIG_GPIO_PL061`, `CONFIG_SPI_MUX`, `CONFIG_MULTIPLEXER`,
`CONFIG_MUX_GPIO` (=y, add to `kernel.nix` next to `TCG_TIS_SPI`). Note the
DTB build only extracts `dt-bindings/interrupt-controller`, so use literal `0`
instead of `GPIO_ACTIVE_HIGH`.

Inside `&{/soc/io-bus@fc000000}`:

```dts
	/* PBS GPIO 48..55. Bits 4/6 (PBS52/PBS54) steer the aux SPI bus. */
	gpio6: gpio@1898000 {
		compatible = "arm,pl061", "arm,primecell";
		reg = <0x1898000 0x1000>;
		interrupts = <GIC_SPI 24 IRQ_TYPE_LEVEL_HIGH>;
		clocks = <&sbclk>;
		clock-names = "apb_pclk";
		gpio-controller;
		#gpio-cells = <2>;
	};

	spi1: spi@18be100 {
		compatible = "amazon,alpine-dw-apb-ssi", "snps,dw-apb-ssi";
		reg = <0x18be100 0x1000>;
		clocks = <&sbclk>;
		num-cs = <4>;
		#address-cells = <1>;
		#size-cells = <0>;

		/* Everything is on SS0; PBS52/PBS54 pick the device. */
		spi-mux@0 {
			compatible = "spi-mux";
			reg = <0>;
			#address-cells = <1>;
			#size-cells = <0>;
			spi-max-frequency = <3000000>;
			mux-controls = <&spi_aux_mux>;

			/* state 1: PBS52=1 PBS54=0 */
			flash@1 {
				compatible = "jedec,spi-nor";
				reg = <1>;
				spi-max-frequency = <3000000>;
				/* partitions { ... i2cfpga, read-only ... } as before */
			};

			/* state 2: PBS52=0 PBS54=1 */
			tpm@2 {
				compatible = "infineon,slb9670", "tcg,tpm_tis-spi";
				reg = <2>;
				spi-max-frequency = <700000>;
			};
		};
	};
```

At the DT root:

```dts
	spi_aux_mux: mux-controller {
		compatible = "gpio-mux";
		#mux-control-cells = <0>;
		/* bit 0 -> PBS52, bit 1 -> PBS54 */
		mux-gpios = <&gpio6 4 0>, <&gpio6 6 0>;
	};
```

Notes on this design:

- gpio-mux state N drives `mux-gpios[i] = (N >> i) & 1`, so state 1 = flash,
  state 2 = TPM; states 0 and 3 are unused (do not add children there).
- **Do not set `idle-state = <1>` or a disconnect idle state.** `tpm_tis_spi`
  holds CS across several SPI messages (`cs_change`), and spi-mux deselects the
  mux after each message; the idle state must be as-is (default) or `<2>`.
- gpio-mux requests the lines `GPIOD_OUT_LOW`, so (0,0) exists from probe
  until the first transfer, with CS deasserted. Vendor code passes through
  (0,0) on every flash->TPM switch, so this should be harmless; `idle-state =
  <2>` shortens it if wanted.
- The interrupt number (GIC SPI 24) is from the vendor DT; drop the property
  if it misbehaves, PL061 works without it.
- Adding the PL061 node does not change pin levels on probe.

### F1-lite — if spi-mux + tpm_tis_spi misbehaves

TPM only, no mux driver: add the `gpio6` node with two hogs and keep `tpm@0`
directly under `spi1` (remove/disable the flash node):

```dts
&gpio6 {
	pbs52-hog { gpio-hog; gpios = <4 0>; output-low;  line-name = "PBS52"; };
	pbs54-hog { gpio-hog; gpios = <6 0>; output-high; line-name = "PBS54"; };
};
```

(Swap the two levels and use `flash@0` instead for a flash-only build.)

### F2 — fallback kernel patch (only if both DT routes fail)

Port the vendor hack as a `set_cs` wrapper for this board in `spi-dw-mmio.c`:
before `dw_spi_set_cs()`, drive two optional GPIOs from the child's `reg`,
and write `SER = BIT(0)` regardless of the child's chip select. Not written
out; F1 should make it unnecessary.

## Side observations

- Vendor dmesg prints `No irq, use poll mode` once per `dw_spi_setup` of a new
  device (allocating its `chip_data`), hence twice on spi1.
- Vendor `dw_spi_set_cs` adds `udelay(1)` after every SER write; mainline does
  not. Not expected to matter at 700 kHz / 3 MHz.
- `gpio5` bit 4 (PBS44) toggling when idle is unrelated to this path.
