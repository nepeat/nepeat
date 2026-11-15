# Boot ROM, UART boot and stage3's console keys

The boot ROM was disassembled from `dumps/u01/bootrom/bootrom.bin`: 16 KiB, ROM ver 2.3, crc32
`55171005`. It is git-ignored vendor code, so this file holds summaries only.

**Sources.**
- The ROM in Ghidra, ARM/Thumb, base 0xffff0000. Rebuild with
  `tools/ghidra/import-raw.sh bootrom.bin <dir> rom 0xffff0000 ARM:LE:32:v8`.
  Its literals point at 0xffff…, so the ROM is linked at the high-vector alias.
- Stage2 A/B, stage3 and the factory U-Boot, as in [spi-uboot-replacement.md](spi-uboot-replacement.md).
- The Annapurna HAL from the MikroTik GPL drop, called `HAL/` below:
  `devices/cs8210-32x/analysis/research/mikrotik-added/drivers/soc/alpine/HAL/`.
  It also holds `pcie/src/al_hal_pcie.c`.
- Hardware reads in `inventory/u01/stage3-state.txt`, `spi-uboot-regs.txt` and
  `spi-recovery-queries-private.txt`.

Code addresses are ROM (0xffff…), stage2 A (0xf22…), stage3 (0x010…) and U-Boot (0x011…).
*Inferred* means not read from code or hardware.

## Verdict
- **Route A works on u01 (2026-11-15): production stage3, run from a RAM flash image, starts the
  app we name, with zero SPI writes** (§5). It needed four fixes on top of the obvious recipe:
  - an AArch32 warm reset via `RMR_EL3`;
  - clearing the cpu-resume watermark;
  - a CPU0 reset that doesn't wait for WFI;
  - blanking UBOOT_SCRIPT.

  Proven so far:
  - the factory U-Boot from RAM;
  - **the `u` escape with BOOT_MODE ≠ U-Boot** (experiment 3);
  - **our U-Boot as stage3's app**, booting NixOS (experiment 4).
- **UART boot is real in the ROM, and so is the instance fallback.**
  - The ROM tries SPI offsets 0, 0x20000, … 0x1e0000, four times each. It takes the first one that
    passes the checks.
  - If all 16 offsets fail, it falls back to **XMODEM-CRC on UART0 at 115200**, whatever the straps say.
  - Strap value 4 selects that UART boot directly.
  - The ROM has **no UART CLI**: strap values 0, 1 and 6 make it stop with an error.
- **The forced-instance word at 0xfbff4000 (the old rung 3 in spi-recovery.md) is dead.** The ROM rebuilds its
  state block on every boot, and that block is what stage2 reads at 0xfbff4000 (§3).

## 1. Stage3 console keys
- **The poll.** Stage3 checks the UART once (0x0102888c, `LSR & 1`), after the BOOT_MODE/DT/APCEA/
  SERDES lookups and before the app lookup. It keeps reading while bytes are waiting. Stage3 doesn't
  clear the RX FIFO, so keys queued earlier count.
  - **On hardware:** keys sent every 20 ms from stage2's last line ("Executing next at 0x01000000")
    hit every time (`stage3keys.py`).
  - Starting from the stage3 banner as seen in tmux missed the poll 3 times in 4.
- **The keys.**

  | key | effect | message |
  | --- | --- | --- |
  | `p` | `0x0105a3e8 = 1` (skip APCEA), then the tail | "User request for skipping APCEA!" |
  | `e` | `0x0105a48c = 1` (keep early APCEA: no APCEA load), then the tail | "User request for keeping early APCEA!" |
  | `u` | the tail only | — |
  | tail 0x01028964 | if app (`0x0105d268`) ≠ 5: print, set app = 5 | "User request for loading U-Boot!" |

  - `p` and `e` were proven 4/4 on hardware, with their flags read back.
  - `e` passes the same `(c & 0xef) == 0x65` compare as `u`.
  - With BOOT_MODE blank the app is already 5, so `u` is silent.
  - **With BOOT_MODE ≠ 5, `u` proven through Route A (§5, run 3):** the message appears, and the
    app becomes 5.
- **BOOT_MODE data** goes to 0x0105d264, up to 0x40 bytes. Only the word at +4 (the app ID) is
  referenced anywhere in stage3.
- **The flags byte reads `0x82` at the factory prompt.**
  - Bit 1 (`SPI_LOAD_ENFORCE`) is set by stage2.
  - Bit 7 is `UBOOT_USER_BOOT_ABORT`. The factory U-Boot sets it when its autoboot is interrupted
    (0x01141bb4 → 0x0110ed18). In the same step it extends TPM PCR 0/1 with a dummy value: the
    measured-boot "user broke in" marker.

## 2. Boot ROM flow (ROM ver 2.3)
- **Reset handler** (0xffff03e4, every core):
  - CPU index `i = (MPIDR | MPIDR>>6) & 0xf`. Then `r = *(0xf0072028 + i*0x100)`.
  - **If `r & ~1` is nonzero, `mov pc` there in ARM state** with all registers zeroed, MMU and caches
    off, before anything else.
  - Otherwise it enables the I-cache (unless `r` bit 0 is set) and does some PBS/DFX setup, then
    `main` (0xffff1968).
- **main:**
  - Copies its initial state block to 0xf2000000: offset 0, load 0xf2200000, exec 0xf2200020, UART0
    0xfd883000, UART1 0xfd884000. Clears 0xf2000024..+0xc8.
  - Prints the banner only when the debug strap is on (`boot_strap` bit 18 = 0; u01 has 1, so the
    ROM is silent).
  - Reads OTP (0xffff2014) and decodes the straps (0xffff0600).
  - Then the stage2 load loop.
- **Stage2 from SPI or NAND** (0xffff07a8, non-secure path):
  1. Read the 0x20-byte S2 header at the current offset.
  2. Check `sum16(header[0:0x1e]) == u16 @0x1e` and that the size (word 3) is nonzero. **There is no
     magic check.**
  3. Load: if word 4 is `0xffffffff`, to 0xf2200020 with a size limit of 0x67fde. Otherwise to the
     address in word 4, with **no size limit**.
  4. Check the payload with a sum16 against the u16 that follows it.
  5. Jump to the exec address (0xf2200020).
  - The secure path (RSA-2048, PUK hash, min SW version, dbgen serial match) is skipped: OTP secure
    boot is off.
- **Retries** (0xffff19fc, `src/recovery.c`, counters at 0xf20000b8):
  - **Four tries per offset**, then the offset moves on by 0x20000, up to 16 offsets (0..0x1e0000).
  - After that, the boot source is set to UART and debug output moves to UART1.
  - Four tries on UART, then a fatal stop: a GPIO blink loop, or a hang when debug is off.
- **What the offsets hold on u01.** CS0 has valid S2 images at 0 (stage2 A) and 0x40000 (stage2 B).
  0x20000 has an all-zero header: the sum passes but the size is 0, so it fails. Nothing else in the
  first 2 MiB is valid. CS1 has three copies, at 0, 0x20000 and 0x40000.

## 3. Instance handling and 0xfbff4000
- **The state block.** The ROM's state block at 0xf2000000 starts with {current offset, load, exec,
  UART0}. On u01, 0xfbff4000 reads exactly that: `00000000 f2200020 f2200020 fd883000`.
  - So stage2's "stage2 offset" word is the ROM's state block.
  - *Inferred:* 0xf2000000 is the same SRAM as 0xfbff4000 at that point. Experiment 0 checks it.
- **Invalidating instance 0 is a real fallback.** `flash_contents_instance_invalidate 0` zeroes the
  'S' byte, so the header sum fails.
  - After 4 tries at 0 and 4 at 0x20000, the ROM boots stage2 B at 0x40000. 0xfbff4000 then reads
    offset 0x40000, so stages 2 and 3 use **instance 1**.
  - Instance 1 is the generic K2C-AB DT. The hazards are in spi-recovery.md §1.
- **The forced pair (bit 0 = 1) can't survive a reset through the ROM.** The ROM writes the offset
  word on every boot. The 2.44 "force temporary" helper only makes sense for code that skips the ROM.

## 4. UART boot
- **Straps:** `boot_strap` bits 17:15 (PBS 0xfd8a8110; u01 reads 7). In ROM 2.3:

  | value | ROM boot source |
  | --- | --- |
  | 7, 5 | SPI |
  | 2 | NAND |
  | 4 | **UART, XMODEM at 115200** |
  | 0, 1, 6 | error `boot_options.c:92`, then a fatal stop (no CLI, no 1 M/2 M baud) |

  - Bit 18 = 0 turns on the ROM's debug prints. Bits 15 and 16 are the two to pull low for UART.
    Which resistors those are on the K2 card is still unknown.
- **XMODEM receiver** (0xffff12f9, Thumb): `int xmodemReceive(uint8_t *dst, int max)`. It returns
  the byte count or a negative error. It is the classic Menie receiver:
  - **Start:** UART0, 8N1, FIFO on. It sends `C` (CRC mode) up to 60 times, 2 s apart, then NAK
    (8-bit sum mode) 60 times. After that it sends 3× CAN and returns -2.
  - **Packets:** 128-byte `SOH` packets only; `STX` (1K) returns -4. Each is `SOH n ~n data[128]`
    plus a CRC16 (CCITT 0x1021, init 0, big-endian) or an 8-bit sum. There is a 1 s timeout per byte.
  - **Replies:** ACK `06` or NAK `15`. It accepts a duplicate of the previous block. 25 bad packets in
    a row give -3, and `CAN CAN` gives -1. `EOT` gets an ACK and returns the length.
  - **Data:** copied to `dst` and silently truncated at `max`.
- **The ROM's UART load** (strap 4, or after all 16 SPI offsets failed):
  - `xmodemReceive(0xf2200000, 0x68000)`.
  - If the result is ≥ 0, it **jumps to 0xf2200020**. Non-secure, there is no header or payload check.
  - So it expects an S2 image (header, payload, sum16) of at most 0x68000 bytes.
  - `uartboot.py s2` extracts stage2 A's: 0x17247 bytes.
- **What follows.** Both factory stage2 builds see a non-NAND strap and set `SPI_LOAD_ENFORCE`
  (A 0xf220c230, B 0xf221135c). So a UART-loaded factory stage2 continues from SPI.
  - That is a **real unbrick for broken stage2 copies**, as long as TOC/DT/STG3/UBOOT are good. The
    ROM asks over XMODEM by itself once no valid S2 is left.
  - It doesn't help with a broken UBOOT 0. That would need Route B (§5).
- **Stage3 and U-Boot "UART recovery" mode:** `SPI_LOAD_ENFORCE` clear and (strap UART or
  `XMODEM_LOAD_ENFORCE`, flag bit 0).
  - Flash offset X is then read from DRAM 0x01800000 + X.
  - Stage3 prints "UART recovery!", and U-Boot sets `bootcmd=echo recovery boot command!`.
- **The factory U-Boot runs `UBOOT_SCRIPT` in that mode too.** With a default env, the script ends in
  `saveenv`, an SPI write. So `uartboot.py image` always blanks UBOOT_SCRIPT 0/1 in the RAM image.

## 5. Route A: production stage3 → any app, from RAM (`uartboot.py`)
**Recipe** (`uartboot.py image` builds the image; `route-a --run` does the rest at the factory
prompt and checks every CRC and patch):
1. **RAM flash image** at 0x01800000. It is the first 8 MiB of `mtd0`, because stage3 clears DRAM
   from 0x02000000 and so flash 0x800000 and up is gone by app-load time. In it:
   - BOOT_MODE = the app (8 bytes, word +4 = app ID);
   - our U-Boot as IPXE 0 (`--app 0x110 --uboot …`, load 0x08000000);
   - **UBOOT_SCRIPT 0/1 blanked.** Otherwise a factory U-Boot in RAM mode runs it, and with a default
     env the script ends in `saveenv`.
2. **Clean stage3** copied out of the image to 0x01000000, with its .bss zeroed (to 0x0105e000).
3. **Flags** `0xfbff41c1 = (old & ~3) | 1`: RAM load on, SPI enforce off.
4. **Cpu-resume watermark** `0xfbff4120 = 0`. Stage3 then takes its cold-boot path: it enables
   VFP/NEON, re-installs the SRAM agent, and enters main through it.
5. **Hooks** (0x600-byte blob at 0x01060000; 2 patches in the stage3 copy, more with the trace):
   - **`x` at stage3's reset call** (0x010294b4). It writes `cpus_software_reset = 0x00010013`:
     stage3's `0x13` plus WFI_MASK for CPU0. **Without it the reset never fires** (2h).
   - **`P` at stage3's entry**: RMR.AA64 = 1. Not shown to be needed (`--no-rmr-fix` would tell), but
     harmless.
   - The trace (`--no-agent-trace` to drop it) adds the 64-bit agent hooks `AMgrDWSE`, `R` on the
     ROM's AArch32 resume path, and the prints in `P` and `x`.
   - Hooks stay resident; `A` is reached again by a PSCI `cpu_on`. So use the trace for diagnostic
     runs only. NixOS ran with all of it in experiment 4, though.
6. **ROM debug print** `0xfbff4020 = 1`, so an AArch32 exception prints LR/CPSR.
7. **`go 0x01700000`: the trampoline** (EL2, 208 bytes).
   - It puts a 48-byte EL3 stub over the agent's entry (0x1000).
   - It cleans D-cache: 0x01000000–0x02000000, the stub line, and the SRAM lines.
   - It sets the ROM resume address `0xf0072028 = 0x01000000` and resets CPU0 (`0x13`).
   - CPU0 comes back in AArch64 EL3 at the stub. The stub prints `#` and requests `RMR_EL3 = 2`
     (AArch32 warm reset).
   - The ROM's reset handler then jumps to the resume address, so stage3 runs in Secure SVC.
8. **Stage3 then runs unmodified**: "UART recovery!", DT/BOOT_MODE/app from RAM, DRAM clear,
   "Executing application...". The `x` reset brings up the 64-bit agent at 0x1000 (stage3 re-copies
   it), which enters the app at EL2.

**Results (u01; logs in `nixos-build/route-a/`):**

| run | outcome |
| --- | --- |
| 2i, `--app none` | `#P01`, banner, `R`, … `w1 14417c00 00000010 00000000 00000100QAMgrDWSE`, then the factory U-Boot from RAM ending at "recovery boot command!" and `ALPINE_DB>` |
| 3, `--app 0x10000005` | no key: `md.l 0x0105d268 1` = `10000005` (UBOOT 1). `--key u`: "User request for loading U-Boot!" and `00000005`. **The escape works for app ≠ 5** |
| 4, `--app 0x110 --uboot result-uboot-dmc12-selfinit/u-boot.bin` | ours: `Entry: EL2, MMU+caches were off (bootm), x0=0x0`; PCIe kept; `alpcie reinit`; `sf read` CRC `08145ce6`; `bootflow scan -lb` → NixOS, smoke 15/15, 4 CPUs, no dmesg warn/err. **Caveat:** the factory U-Boot had already run in that power-on (PCIe, UART, SerDes up), so this is not our U-Boot's cold init |

**Why each fix (the dead ends, so they aren't retried):**
- **The reset width:** config_aarch64 (0xf0072004) and RVBAR (+0x48) are **write-once**. Config +0x00
  has FORCE_WONCE, and writing 0 left 1 (2a). Stage3 set them on the first boot, so an NB reset always
  comes back in AArch64 (v1, 2).
- **The 32-bit way back:** `RMR_EL3` from an EL3 stub at the agent entry works (2b).
- **VFP:** stage3 enables VFP/NEON only when the cpu-resume struct at 0xfbff4120 is invalid
  (0x01007a18). The trace buffer's first timestamp (0x01000bec, 0x01023d28) is NEON, so leaving the
  struct valid hangs it in the ROM's silent UNDEF vector (2b → v3).
- **Non-secure main:** stage3's main runs Non-secure SVC, because the SRAM agent's exec-via-agent
  switches to NS (struct+4 flags 0). So RMR/secure CP15 are UNDEFINED there (2f). This is also true on a
  real cold boot.
- **The reset never fires (2h):** stage3's `0x13` waits for CPU0's STNDBYWFI, and here every WFI
  returns at once.
  - *Inferred:* that is the `cpus_hold_reset` WFI_MASK semantics in the HAL.
  - The 2i registers give the reason: GICD_CTLR (NS view) = 0x10, only ARE_NS, so Group 1 is not
    enabled. GICR_ISPENDR0 = 0, so no Non-secure-visible SGI/PPI is pending. **ISR = 0x100, so A = 1:
    an asynchronous abort (SError) is pending**, masked by CPSR.A.
  - A pending SError counts as a WFI wake-up event, so WFI never holds. *Inferred:* this is the wake
    source. In 2h WFI woke without our GIC reads, so the abort predates them.
  - Its origin is unknown: some earlier bus error on this path (the first boot, the factory U-Boot,
    or the warm-reset sequence).
- **The factory U-Boot's "al_flash_obj_header_read_and_validate failed!"** (2i, right after "FRU
  initialized") is that blanked UBOOT_SCRIPT: board_init (0x01102334) validates object 6 through
  the RAM reader. The script is then not run, which is intended, and nothing else depends on it.

**Differences from a real SPI boot:**
- the reader is a memcpy instead of the SPI mmap;
- stage2 doesn't run again;
- the factory U-Boot ran first: SerDes, PCIe, GIC, the pending SError;
- the secondaries are powered off from the first boot.

**Risks.** Each run so far only hung the box; a power-cycle recovered it every time, and SPI is
untouched.
- The SRAM agent re-install resets 0xf0072028, unless stage3 dies before that.
- Power-cycle (AC off ≥ 10 s) after every run.
- Never `saveenv` at a factory prompt reached in RAM mode.

**Reference: the CPU0 reset registers** (`HAL/include/sys_fabric/al_hal_nb_regs_v1_v2.h`,
`cpun_config_status[n]` at NB 0xf0072000 + n*0x100; u01 values in `inventory/u01/route-a-regs.txt`):

| off | name | u01 | meaning |
| --- | --- | --- | --- |
| +0x00 | config | 0x24 | bit 2 VINITHI (vectors at 0xffff0000), **bit 5 FORCE_WONCE**: config_aarch64 and RVBAR become write-once |
| +0x04 | config_aarch64 | 1 | AA64nAA32, sampled at CPU power-on reset. Stage3 wrote 1 (0x01028054); **later writes are ignored** (2a) |
| +0x20/+0x24 | power_ctrl / power_status | 0/0 (CPU1: 3/7) | CPU1 is powered off and in WFI |
| +0x28/+0x2c | resume_addr_l/h | 0xfbff423c | the ROM's AArch32 resume address. Stage3's **SRAM cpu-resume agent** sets it for all 4 CPUs (see below) |
| +0x40 | warm_rst_ctl | 0 | an `RMR` warm-reset request is honoured, once the core is in WFI |
| +0x48/+0x4c | rvbar | 0x1000 | AArch64 reset address (write-once), the DRAM agent |

- **The SRAM cpu-resume agent** is the Annapurna `cpu_resume` service (`HAL/services/cpu_resume/`).
  - Stage3's early init (0x0102818c) checks for a resume struct at 0xfbff4120 (watermark
    `0xf0e1d2xx`, xx ≥ 0xc3). If there isn't one, it copies a 0x15fc-byte AArch32 agent
    (stg3 0x010476d0) to **0xfbff4200** and runs it once.
  - That agent fills the struct (flags plus a per-CPU {flags, resume_addr}) and sets every CPU's
    resume_addr to its entry, 0xfbff423c (agent code at stg3 0x01047d70, setter 0x01048224).
  - On a later AArch32 reset, the ROM jumps there and that agent resumes the CPU, optionally switching
    to NS/HYP.
- **The CPU reset recipes.**
  - Stage2 → stage3 is a plain jump: AArch32 EL3, `mov pc` to 0x01000000.
  - Stage3 → agent (AArch32 → AArch64) is `config_aarch64 = 1` + RVBAR 0x1000 + `cpus_software_reset
    = 0x13`. That value is CPU0 at level 1, `cpu_poreset`, which samples AA64nAA32.
  - Nothing on this box ever goes AArch64 → AArch32. The architectural way is `RMR_EL3` {AA64 = 0,
    RR = 1} at EL3 followed by WFI. The SoC turns that WARMRSTREQ into a core warm reset with the new
    register width (HAL note on warm_rst_ctl).
- **Other resets don't help.**
  - A SoC reset (sp805, uC 0x0101, `reset`) re-runs the ROM and stage2. Stage2 re-initialises the shared
    data and sets `SPI_LOAD_ENFORCE`, so the RAM image is ignored. *Inferred:* it also resets the NB
    registers.
  - PSCI `cpu_on` only starts other cores at EL2.

**Route B (strap 4 on a spare, or a CS0 with no valid S2): a real UART unbrick for a bad UBOOT 0.**
- Send stage2 A **patched in RAM**: replace the body of 0xf220c230
  (`flag_set(SPI_LOAD_ENFORCE)`) with `xmodemReceive(0x01800000, 0x800000)` (ROM 0xffff12f9) and
  `flag_set(XMODEM_LOAD_ENFORCE)`.
- Then send the RAM image. Stage2 still reads TOC/DT/STG3 from SPI, then stage3 and U-Boot run from RAM.
- Not built: it needs a code cave in stage2 A, and the ROM's UART0/timer state at 0xf2000000 left
  intact for the call (it is, right after the ROM).
- `uartboot.py xmodem` is the sender. It is pyserial-free, was loopback-tested against a Menie-style
  receiver on a pty, and **needs the tty to itself**:
  - stop tio with `tmux send-keys -t serial-dmc12 C-t q` (the pane exits);
  - run `sudo ./uartboot.py xmodem --tty /dev/serial/by-id/…B0016Y5Q… <file>` (it refuses while any
    process holds the tty);
  - restart tio in tmux with the PROGRESS command and a new `--log-file`.

## 6. Next
- **Route A with `--no-agent-trace`.** It should still boot: `x` and `P` stay. Then once with
  `--no-rmr-fix`, to learn whether the RMR write is needed.
- **Route B / strap 4** on a spare (owner): `uartboot.py s2` + `xmodem`. That is the ROM's UART boot
  end to end.
- **The SPI-write plan** ([spi-uboot-replacement.md](spi-uboot-replacement.md) §7) can now be
  rehearsed in RAM first. The exact IPXE + BOOT_MODE layout boots, and the `u` escape is proven.
  Our U-Boot's cold init (no factory U-Boot before it) is untested: Route A always runs after the
  factory U-Boot.

### Minimal-recipe check (u01, 2026-11-15, main agent)
- `--no-agent-trace`: our U-Boot as stage3's app boots (Entry EL2, x0=0, prompt); only the P + x hooks patched.
- `--no-agent-trace --no-rmr-fix`: also boots. **The RMR write is not needed; the only required fix is the
  reset request without the WFI wait (`cpus_software_reset = 0x00010013`).**
