# 2026-11-10: Innovium SDK learn-thread wedge (32 GiB malloc, "Learn WB w/o expected SOP" flood)

Read-only investigation, mostly static. On the switch: journal, `/proc`, `lspci -vvv`,
sysfs, and one read of `/proc/<pid>/mem` (a futex word). Nothing was restarted,
signalled or written. A single read-only BAR read of the learn registers was
attempted, and the `mmap` of `resource0` failed with EINVAL, so no ASIC register
was read. Static analysis used `libifcs.so` from the R3181-J0001-01_V0.1.7 SDK
(sha256 `388031e7…0d17`, identical on the switch and in
`analysis/onie-live-2026-11-10/sdk/`). The live evidence was captured before the
restart and is in `analysis/research/ifcs-log-20261110/live/`.

Marks: **V** = verified (log, disassembly, register dump), **I** = inference.

## Summary

The learn ring had never been used (index 0, no traffic: no cables). One read of
the learn ring's hardware producer-index register (BAR0 `+0x4860`) returned a
value that cannot be a ring index, most likely `0xFFFFFFFF` from a failed PCIe
read. The SDK uses that value without any check:

1. The pending-entry count underflows to about 2^32 - 0x7ff.
2. `malloc(count * 8)` asks for 32 GiB and fails. The NULL result is not checked.
3. The processing loop `while (sw_idx != hw_idx)` can never end, because the
   software index only takes the values 0..0x7ff and 0x1000..0x17ff. The register
   is read once per pass, before the loop, so one bad read wedges the thread for
   good.
4. The thread then walks the empty ring forever. It logs
   `Learn WB w/o expected SOP - <idx>` for every slot (about 25 MB/s into
   `/run`) and uses 100% of one CPU.

The same signature appeared in both SDK runs this boot. Nothing external happened
at either time.

**Confidence:**
- High (V): the mechanism once the bad value is in.
- Medium (I): that the bad value was an all-ones PCIe read. A completion timeout
  is latched in the root port's AER status, but it cannot be dated.

## Timeline (local time; the SDK log clock is +8 h)

| time | event | |
|---|---|---|
| 01:51 | boot | V |
| 01:52:37 | SDK (run 1) opens ipd | V |
| **02:36:31** | kernel: `__vm_enough_memory: comm ifcs-learn, bytes: 34359726080` ×2, `34359721984`, `34359857152` (pid 2634). That is 44 min after the start | V |
| 02:39:44 | first ENOSPC on `/run` (logind), then hourly journald ENOSPC until 07:00 | V |
| 07:20:04–07:20:51 | SDK stopped and restarted (run 2, the libMdio trace session). The old log went away and `/run` had space again: no ENOSPC at 08:00–11:00 | V |
| 19:03 SDK (11:03) | timer callbacks run 1.3–1.7 s late. This is routine: the log shows 2–12 such events every few minutes through the whole run, up to 3.0 s, so it is not specific to the crash | V |
| **11:05:10.898** | the same four `__vm_enough_memory` lines, the same byte counts (pid 109380 `ifcs-learn`). That is 3 h 44 min after the start | V |
| 19:05:10.896 SDK | first `Learn WB w/o expected SOP - 0`. The loop starts at index **0** | V |
| 19:05:13–19:06:25 SDK | `PEN read failed` and `Assert in ii_stats_timer_cb:125` every ~6 s, plus `PEN PIC_STATUS read failed [rc = 15]` and port bring-up failures for ports 1..12. After that, nothing but the flood | V |
| ~12:00 | `/run` full again (journald ENOSPC hourly) | V |
| 15:45 | 1.9 GB log, 24.4 M flood lines. After a truncate it grew back at ~25 MB/s | V |

The powerlog at both crash times shows ordinary values: 695–699 W, ASIC
57.8–60.7 °C. `powerlog.sh` only uses I2C (PMBus rails and an ADC) and never
touches PCIe (V, script read). There are no ssh, nix, cron or other journal
events in 02:33–02:37 or 11:02–11:06 apart from the DHCP route churn and the
powerlog (V, `live/journal-around-crashes.txt`).

## Evidence

### The code (V, disassembly of `_ii_pcie_rupt_thread_learn`, libifcs 0x166b4dc–0x166c3b4)

Ring struct (at ctx+0x68b0): `+0` size, `+32` pointer to the WB pidx, `+40` ring
base (8-byte entries), `+48` software index `A`, `+0x40` mutex.

```
lock(ring->mutex)                                 // 0x166b9bc
reg_read(0x4864) ; reg_read(0x485c) ; reg_read(0x4860 -> B)  // B: hw producer index
if ((A >> 12) == (B >> 12))   n = size + B - A;              // 0x166ba2c
else                          n = size - (B & 0xfff) + (A & 0xfff);   // 0x166ba4c
if (n) buf = im_malloc((uint64_t)n << 3);         // 0x166ba88, NULL not checked
do {                                              // 0x166bb30
    e = ring_base[A & 0xfff];
    if (e & WB_ERR)  log("Learn WB error - %x", A);
    if (!(e & SOP))  log("Learn WB w/o expected SOP - %x", A);
    A++; if ((A & 0xfff) == size) A = ~A & 0x1000;   // wrap, flip phase bit 12
} while (A != B);                                 // 0x166bc88: B is NOT re-read
...
unlock(ring->mutex)                               // 0x166c388, never reached
```

- The register reader (`0x1670080` → `node_pci_reg_read32`, libifcs 0x1672484)
  is a plain 32-bit load from the mmapped BAR0. It has no all-ones check, and the
  learn thread ignores its return code (V).
- Ring size 0x800, phase bit 12: the flood indices run 0..0x7ff then
  0x1000..0x17ff and wrap, 4096 distinct values (V, `live/spam-sample.txt.gz`).
  So `A` can never equal a `B` whose low 12 bits are 0x800..0xfff, or whose
  bits 31..13 are set.

### The allocation size pins `A` and `B` (V arithmetic, I for the glibc path)

- With `A = 0` (the flood starts at 0) and `B = 0xFFFFFFFF`, the else branch gives
  `n = 0x800 - 0xfff = 0xFFFFF801`. Then `n*8 = 0x7FFFFC008`, glibc
  `request2size` gives `0x7FFFFC020`, and mmap rounds `nb + 8` up to the page:
  **0x7FFFFD000 = 34359726080**. That is exactly the first two kernel lines
  (thread-arena mmap, then the main-arena retry).
- The other two lines (`0x7FFFFC000`, `0x80001D000` ≈ nb + 128 KiB `top_pad`) fit
  glibc 2.25's brk and mmap-fallback attempts for the same request. That part is
  not derived exactly (I).
- Any `B` with `B >> 12 != 0` and `B & 0xfff` in 0xE01..0xFFF gives the same
  sizes and the same endless loop. All-ones is the natural member of that set.
- Identical sizes in both runs mean the same `A` (0, idle ring) and the same
  kind of `B` each time.

### PCIe: completion timeout latched (V for the registers, I for the dating)

`live/lspci-asic.txt`:
- **Root port 0001:00:00.0:**
  - DevSta: `NonFatalErr+`
  - UESta: **`CmpltTO+ UnxCmplt+`**
  - AER First Error Pointer **0x0e = Completion Timeout**
  - CESta: `BadTLP+ Timeout+ AdvNonFatalErr+`
- **ASIC 0001:01:00.0:**
  - CESta: `RxErr+ BadTLP+ Rollover+ Timeout+`, i.e. link-level replays
- **Both:** DevCtl2 completion timeout is the default range, **50 µs–50 ms**.
- **Link:** now 8 GT/s x4, Mem+ BusMaster+.
- **AER counters:** the kernel's counters (`aer_dev_*`,
  `aer_rootport_total_err_*`) are all **0**. AER interrupts never reach the
  kernel on this platform, so these sticky bits were never reported or cleared.
  They cannot be dated: they could come from either crash, from one of the
  `inno_cleanup`/re-open cycles, or from before the warm reboot at 01:51.

A completion timeout on an MMIO read, answered with all-ones by the root complex,
with the late completion then latched as `UnxCmplt`, is exactly what would put
`0xFFFFFFFF` into `B` without a kernel error. How Alpine's root complex answers
a timed-out read has not been checked (I). No SError or panic happened at either
time (V).

### State of the wedged process (V, `live/innovium-*.txt`, `futex-owner.txt`)

- `ifcs-learn` (tid 109380) is in state R with no kernel stack. It uses ~1 CPU,
  about 1/3 user and 2/3 system (log writes).
- `inno_irq-5-lrn` has 0 interrupts. The learn thread polls; its MSI never
  fired, in either run.
- `inno_irq-6-pic` keeps counting (~230/s). The ASIC still delivers MSIs, so the
  device and the link are alive.
- The other SDK threads sit in normal condvar waits or `inno_rupt_wait`. The main
  thread is in `pthread_join` (futex value = tid 109409). No deadlock is visible
  from outside. Why the PEN/PIC errors stop after 19:06:25 is not explained.

## Alternatives ruled out

| alternative | why not |
|---|---|
| memory pressure or OOM | 7.6 GB RAM, ~4.7 GB available, PSI memory ≈ 0. The 32 GiB request is a garbage length, not real demand (V) |
| ring wrap or overflow from learn traffic | ring idle, `A = 0` at the crash, no cables or traffic. The sizes match `A = 0` exactly (V) |
| `/run` full causing the crash | run 2 crashed at 11:05 with `/run` not full (no ENOSPC 08:00–11:00). In run 1 the first ENOSPC came after the crash (V) |
| ipd `legacy_abi=1` ioctl mismatch | `n` depends only on `A` (SDK memory), `B` (MMIO) and the ring size. No ioctl data is involved (V from the disassembly). Other ipd ioctls (`inno_rupt_wait`) keep working (V) |
| DMA or cache coherency of the WB ring | `B` comes from an MMIO register, not from WB memory. An incoherent empty ring would only make entries look stale, and it cannot produce `B` (V) |
| external trigger (our sessions, powerlog, timers) | nothing in the journal at either time; powerlog is I2C only (V) |
| timer-callback stalls as the trigger | they happen throughout the run, not only before the crash (V) |

Open: the cause of the bad read itself. Candidates: a completion timeout on a
slow ASIC read, link errors (the replays latched in CESta), or something
ASIC-internal around the PEN accesses that also start failing 3 s later. Also
open: why every other SDK thread goes quiet after 19:06:25.

## Proposed fix / next experiments

1. **LD_PRELOAD shim for `node_pci_reg_read32`** (I, plausible). The function is
   exported, and libifcs calls it through its PLT (`bl node_pci_reg_read32@plt`,
   V), so it can be interposed.
   - Log every all-ones read: offset, time, a retry count.
   - Re-read up to N times, at first **only for the learn and ring index
     registers (0x485c–0x4864)**. Other registers may be clear-on-read, so a
     blind retry could lose data.
   - This both prevents the wedge and dates the event, which settles the
     PCIe-read hypothesis.
   - It needs glibc ≤ 2.17 symbols, like libMdio.
2. **Date the AER errors.** After the restart, clear the sticky UESta/CESta bits
   on both devices (setpci, write-1-to-clear) and poll them, e.g. from powerlog
   every 30 s. If `CmpltTO` comes back at the next wedge, the hypothesis is
   confirmed. AER interrupts don't arrive on this platform, so polling is the
   only way.
3. **Completion timeout range.** Consider setting the root port's DevCtl2 to
   range B (65–210 ms) before the SDK starts, if (2) shows timeouts. Trade-off: a
   truly hung read stalls the CPU longer.
4. **Watchdog** (independent of the cause): if `ifcs-learn` stays near 100% CPU
   or the log grows more than X MB/min, restart `cs8210-switch`. Combined with
   the size-capped log on its own tmpfs (main's /run fix), a wedge then costs
   seconds, not RAM.
5. **Link quality.** CESta shows RxErr, BadTLP and replay timeouts at 8 GT/s x4.
   If (2) finds timeouts together with these, look at Gen3 equalization or
   compare against a Gen2 link.

Also note: if an SOP entry ever arrives after the failed malloc, the thread
writes through the NULL buffer (`str x0, [x19]` at 0x166bf28) and crashes the
SDK instead of spinning (V for the code path; not observed).
