# 2026-11-17: independent review of the IFCS learn wedge, its hardening, the guard and the L0s soak

Reviewer: Fable (read-only; no hardware access, nothing built or run). Read: oom-20261116.txt, learn-wedge.md,
sai-hw-learnwedge-20261116.txt, reviews/2026-11-10-sdk-learn-crash.md, sai-hw-fpga-stall-20261116.txt, HISTORY (i)/(n)/(y),
`git show ff90015`, the uncommitted tree (ifcs_guard.c, ifcs_lib.cpp, tl_switch.cpp, ifcs-watchdog.py, sonic/ and
vendor-sdk/default.nix), the local raw files (lspci of 11-10 and 11-16, aspm-ab-1701.txt, lw-soak.sh, learn-thread.dis.txt).
Marks: **C** = confirmed by reading code/evidence, **S** = suspected. Paths: `G` = ai_re/nix/packages/sai-teralynx/src/ifcs_guard.c,
`W` = nix/nixos/module/cs8210/sonic/ifcs-watchdog.py, `V` = nix/nixos/module/cs8210/vendor-sdk/default.nix,
`N` = nix/nixos/module/cs8210/sonic/default.nix.

## Verdict

- The mechanism from a bad index to 32 GiB is proven. The containment (MemoryMax, guard) is sound and should ship.
- "L0s causes link errors" is proven (A/B plus two later boots). "Link errors cause the wedge" is still an inference:
  no bad read and no TX stall has ever been dated against an AER bit, and the soak so far (about 1.7 h clean) carries
  no statistical weight (P = 0.59 of seeing that with L0s irrelevant).
- The biggest gaps: after the guard nothing restarts or alarms on a persistently dead chip unless ports were
  transmitting; recovery of the dead CPU TX ring by a plain syncd restart has never been observed under SONiC; the
  soak has no control arm.

## Findings, ranked

### High

**H1. The soak cannot yet support the L0s claim, and has no control arm. (C)**
- Base rate: 3 wedges in ~10 h = 0.30/h, exact 95 % interval for 3 events 0.06 to 0.88/h. Zero events in T hours
  has probability exp(-rate x T) if L0s does not matter: 10 h for p = 0.05 at the point estimate, 15 h for
  p = 0.01, **48 h** at the lower bound. Runs 1 + 2 (71 + 33 min; sai-hw-learnwedge section 6): p = 0.59.
- That base rate is from the vendor stack, the factory U-Boot and 2026-11-10. SONiC + our U-Boot has one event
  (42 min after start) and at least one clean 2 h soak with L0s on (sai-hw-sonic-soak-20261116.txt, 09:41-11:42).
  The rate under the current configuration is unknown, so a clean L0s-off soak proves nothing by itself.
- Time since stack start may matter: 3 of 5 events (incl. the 11-10 SError panic, HISTORY (i)) came 37-53 min
  after a start, the others at 3.7 h and 5.6 h. Not significant (p about 0.09), but a 33 min run is close to
  worthless and reboots that cut a run are not neutral.
- **Fix: a crossover on the same boot chain, now safe because the guard and MemoryMax contain the event.**
  Blocks of L0s on / off (`link/l0s_aspm`), each starting with a syncd restart and lasting >= 4 h, alternated
  (on, off, off, on, ...) so drift (temperature, other agents) cancels. Endpoints per block: TX-ring stalls
  (TX-3 Full rising with no completions), guard invalid reads, watchdog triggers. Decision rule (conditional
  binomial): with off-time = 3 x on-time, 3 events on and 0 off gives p = 0.016; equal time needs 5 on / 0 off
  (p = 0.03). Expect ~10 h of L0s-on time if the old rate holds. If the on arm stays clean for 15 h too, the
  wedge rate under SONiC is simply lower and the question stays open: say so instead of crediting L0s.
- Keep the deployed-guard build in both arms (the guard is also the best event detector, H3).

**H2. After the guard, a dead chip is silent unless tx-stall happens to be armed. (C)**
- Persistent all-ones: G:248-260 substitutes "nothing new" forever, the log is capped at 20 lines / 10 min
  (G:163), and the watchdog has no signal on the guard counters (W:55 SIGNALS; the guard file is only passed
  through in `--metrics`, W:301-314). `vmsize`, `rss`, `learn-cpu` and `kernel` can no longer fire.
- tx-stall needs every transmitting port flat (W:456-457). Confirmed gaps: (a) transit traffic keeps ASIC TX
  counters rising while only the CPU TX ring is dead (the 11-16 failure on anything but the self-loop);
  (b) after a restart that did not revive the chip no port ever reaches `min_rises`, so it never fires again;
  (c) no ports transmitting at all (idle box).
- **Fix:** add two signals. `guard`: `sai_teralynx_ifcs_guard_substituted_total{reg="0x4860"}` rose in 3
  consecutive polls (45 s) -> restart; any new `invalid` -> log root-port status at once (dating). `cpu-tx`:
  ipd TX-3 `full` rose in 4 consecutive polls while its completed-packet count did not move (W:156 already
  parses it; check in the ipd source whether `packets` counts at completion). Alert on
  `sonic-syncd` failed and on `cs8210_ifcs_watchdog_restarts_total` increases.

**H3. No event has been dated; production never clears the sticky bits, so the next one will not be either. (C)**
- All five events rest on CmpltTO/UnxCmplt read minutes to hours later (11-10 review "cannot be dated";
  oom-20261116 section 2 "Not dated"). The metrics read UESta/CESta (W:266-278) but nothing writes them back:
  with L0s on CESta is 0x1040 within seconds forever, and UESta dates only the first CmpltTO per boot.
- **Fix:** in the watchdog, when the root port's UESta/CESta are non-zero: log them with a timestamp and write
  them back (W1C; root-port config is on the SoC). Count clears per bit in `--state`: that turns the sticky bits
  into rates. Same for LnkSta bits 14/15 (LBMS/LABS: a retrain that changed speed/width), which W:127 already
  reads but never reports.
- Then one event answers the main question: guard line (ms timestamp, `rc`, recovered or not) + CmpltTO within
  the same second = a PCIe completion timeout. `rc != 0` with no AER bit = an SDK-internal error path
  (learn-wedge.md section 1: on an error return the value is uninitialised stack; identical sizes on every run fit
  deterministic stack residue as well as all-ones). Secondary status `<MAbort+` = the ASIC answered UR (alive, not
  decoding: reset/internal), not silence. Export `rc` as a counter label; today it is only in the capped log line.

**H4. Recovery of the dead CPU TX ring by a syncd restart is unverified under SONiC. (C for the gap)**
- learn-wedge.md section 5 calls the tx-stall restart "the ring reset". On 11-16 the restarted syncd ran for 15 s
  before another agent stopped sonic.target and did `rmmod ipd` (oom-20261116 16:05:50-16:07:57): the recovery
  that worked included an ipd reload. The 11-10 recoveries were vendor-stack restarts.
- The watchdog restarts once per 15 min and never checks the result or escalates (W:605-618).
- **Fix:** after a watchdog restart, require "some port transmitting again" within ~5 min; if not, one escalation
  step (stop sonic.target, reload ipd, start) and then stop and alarm. Record the first real observation either way.

### Medium

**M1. The guard multiplies timed-out reads on a dead link, with no delay between them. (C code, S impact)**
- G:233-237: 3 immediate re-reads per invalid read, for all three registers: 12 unanswered reads per poll
  instead of 3, 0x4860's under the ring mutex. On this SoC an unanswered read stalled the whole machine in the
  FPGA case (sai-hw-fpga-stall: scrapes, fand's I2C, exec all timed out). Whether ASIC-link timeouts stall the
  same way is not measured.
- Back-to-back re-reads land inside the same link recovery; the chip test only proves the injected case.
- **Fix:** sleep 2-5 ms between re-reads; after a substitution do no re-reads for that register until one read
  is valid again (the thread's own 1 Hz read is the probe).

**M2. `cs8210-asic-aspm` is not ordered after `cs8210-pcie-errors`, reads the ASIC's config space, cannot fail,
and does not survive a PCI rescan. (C)**
- V:344-348: `before` only. V:187 does `setpci -s "$dev"` (a config read across the link) possibly before the
  root port's error response is set to all-ones (S: `pcieReady` exists for exactly this, N:183-186).
- The script always exits 0 (V:172-189: the last command is an `echo`); a failed write is only a stderr line, and
  the unit is `wantedBy`, not required: the stacks start with L0s on and nothing notices except the metric.
- `cs8210-switch-power` removes and rescans the ASIC (vendor-sdk/cs8210-switch-power:71,85): the kernel builds a
  new link state with L0s on, and a oneshot with RemainAfterExit does not run again.
- **Fix:** `wants`/`after` cs8210-pcie-errors; drop the device-side setpci (the root port's LnkCtl is the bit that
  matters); exit non-zero when the root port still has LnkCtl bit 0 set; make it the kernel's job as well
  (`aspm-no-l0s` in the DTS root-port node, or `pcie_aspm.policy=performance` on the command line: verify on
  one boot that 7.3.4 honours it for DT platforms) and alert on `cs8210_asic_link_aspm_enabled{state="L0s"} == 1`.

**M3. Vendor watchdog gives up for good after a second OOM stop within 15 min. (C)**
- V:137-146: `seen_active=0` is set before the 900 s check; the suppressed branch never restarts and the unit
  can never be seen active again, so cs8210-switch stays failed until someone starts it by hand.
- **Fix:** leave `seen_active=1` in the suppressed branch (retry when the 15 min have passed), log once.

**M4. The guard covers three registers; nothing tells us what else reads all-ones. (C gap, S consequences)**
- On 11-10 "PEN read failed", `ii_stats_timer_cb` asserts and PIC_STATUS failures followed within 3 s, and every
  SDK thread went quiet; ipd's RX `posted_cidx` refresh is unchecked (learn-wedge.md section 5). Counter reads that
  return all-ones would make COUNTERS_DB jump, which tx-stall reads as "transmitting".
- **Fix (diagnostic, cheap):** every `node_pci_reg_read32` of libifcs already passes G:209-212. Count all-ones
  results for all offsets there (one compare, no retry, no substitution: some registers are legitimately
  all-ones or clear-on-read) and keep the first ~16 distinct offsets: `..._allones_reads_total` plus one log line
  per new offset. That dates every unanswered read and lists the other victims.

**M5. Root-cause reading: three things the write-ups pass over. (C evidence, S interpretation)**
- *Two failures 4 min 45 s apart on one boot are not independent.* At 0.3/h a second independent hit within
  5 min has p about 0.02. 0x4860 read correctly ~290 times with the TX ring already dead, then failed 2 min into
  the softirq storm. So either the TX stall produces the bad read (does `inno_enet_tx`'s BUSY path read a BAR
  register per attempt? then the storm raises MMIO load by orders of magnitude), or the ASIC degrades in steps
  (DMA engine, then register access: the 11-10 PEN failures fit). Neither needs L0s; L0s may be only the trigger
  of step one. Check the ipd source for MMIO in the ring-full path.
- *UnxCmplt+ means the completion arrived late, not never*: the ASIC answered after the root port's timer
  (default range, 50 us to 50 ms on both ends, lspci 11-10 lines 35/120). That fits a long link recovery (replay
  rollover -> Recovery, re-equalisation at 8 GT/s) and equally a slow ASIC-internal access. LBMS/LABS (H3)
  separates them: set = the link retrained.
- *Completion timeouts also happened on a link without L0s*: the I2C FPGA (0002:01:00.0, ASPM off, CommClk+)
  stopped answering on the same day (sai-hw-fpga-stall). L0s is not the only way this root complex loses a
  device; watch all three root ports with the same clear-and-count.
- For the TX DMA stall the relevant timer is the **ASIC's own** completion timeout (it is the requester of the
  descriptor/data reads). Its UESta read CmpltTO- on 11-10 (lspci line 47) on a boot with wedges, which argues
  against "the ASIC's DMA read timed out" at least for those. Read the ASIC's UESta once after the next TX stall
  (link up, before the restart).
- Supporting the L0s story (C): root port "ASPM L0s Enabled", ASIC "Disabled", CommClk- on 11-10 as well (lspci
  lines 26/108), so every wedge so far ran with L0s on; mainline enables it on DT platforms only since 6.18, so
  the factory kernel most likely never did. Worth one check of the DIAGOS `lspci` if it is ever booted.
- Thermal/power: rail and temperatures nominal at every event (C); VOUT_UV_FAULT_LIMIT = 0 means a dip cannot
  latch (C). Low, and not testable without PMBus writes that are off-limits.
- Not available: L0s/L1 residency (the root port has no vendor-specific extended capability, so no DWC RAS DES
  counters: lspci-asic-link-1659 capability list).

**M6. tx-stall false positives. (C code, S likelihood)**
- A port that was transmitting and goes quiet for 180 s while oper-up restarts syncd when it is the only active
  one (LLDP off on it, transit-only traffic that pauses, or flex-counter interval > 180 s). The self-loop never
  shows this; the house-network cabling can.
- The watchdog ignores the ASIC lease (`/run/asic-lease`, used by every agent test) and `/run/sonic-nix/reloading`
  (recovery.nix:23): it can re-initialise the chip under someone's test.
- **Fix:** require a CPU-originated witness (cpu-tx signal of H2, or Linux netdev tx rising while the ASIC port
  counter is flat) before tx-stall restarts; skip restarts (log only) while the lease or the reload marker exists.

### Low

- **L1 (C).** G:254 returns the all-ones value itself for 0x4864/0x485c when no valid value was seen yet; the
  thread then writes it back with every upper bit set. Return 0 there, or drop the write-back of a register
  whose last read was substituted (the consumer index did not move, nothing is lost).
- **L2 (C).** Ring size has two homes (profile key default 2048, tl_switch.cpp:132, and the init yaml). A wrong
  smaller value makes every valid index "invalid" (G:98): learning dies quietly behind a capped log. Log loudly
  when the first 10 reads of 0x4860 are all invalid with rc 0 and values below 0x2000.
- **L3 (C).** Restart rate limit lives in memory only (W:494-504); a watchdog restart or deploy forgets it.
  Persist the last restart time in `--state` (wall clock + boot id) and seed RestartPolicy from it.
- **L4 (C).** `restart_syncd` has no timeout and no OSError handling (W:527-530); a hung `systemctl` hangs the
  watchdog. `triggers_total` counts polls, not events, while a suppressed condition persists (W:607).
- **L5 (C).** W:114-115 reads all 4096 bytes of the root port's config space every 15 s (metrics) including the
  DesignWare port-logic area at 0x700+. No side effect is known; `pread` only the capability list and the AER block.
- **L6 (C).** Data races in the guard: `next_write` (G:224), `inject_reg` (G:197) read outside `mu`;
  `S[i].good` written plainly at G:243 and atomically at G:222. Benign today; make them atomics or take `mu`.
  stderr under `mu` and the ring mutex (G:166-168) can block the learn thread if journald stalls.
- **L7 (C).** MemoryMax sums exceed RAM: syncd 2 GiB or cs8210-switch 3 GiB, PaperMC 4 GiB, 7.6 GB box. With
  PaperMC near its cap the 3 GiB vendor limit can still end in a global OOM. 1536M covers the measured 494 MB
  peak + the 256M tmpfs. fand has no MemoryMin/mlock (it page-faulted on its own text on 11-16); `MemoryMin=32M`
  on its unit is cheap. `OOMPolicy=stop` is the systemd default: harmless, but it changes nothing.
- **L8 (C).** A valid-looking but wrong index (0 after an ASIC-internal reset, say) passes G:98: the thread walks
  at most 4096 stale entries once (spurious learn events), then resyncs. Bounded; no change needed, but count
  "B went backwards by more than the ring" as a metric.
- **L9 (C).** syncd StartLimitBurst 10 / 300 s (modules/switch/sonic/default.nix:454): a chip that fails
  `ifcs_init` within 30 s ten times leaves the stack down with no retry and no alarm. Correct fail-stop; needs
  the alert of H2.

## What is right (checked, no change needed)

- Interception point: the GOT slots of libifcs only, set before `ifcs_init` (ifcs_lib.cpp:105-134); `*val` preset
  so an error return cannot leak stack (G:218); the thread's `rc`-blindness handled.
- Skipping a poll does not desync: with B = A the pass computes n = ring size, allocates 16 KiB, runs no loop
  iteration (learn-thread.dis.txt 166ba2c-166ba90) and rewrites the same consumer index. Hardware state is untouched.
- `calloc` for `im_malloc` keeps the contract and no longer touches the pages; the 1 GiB limit is below MemoryMax.
- MemoryMax on syncd: a cgroup kill of the one process, `Restart=always`, BindsTo/Upholds bring orchagent and
  the rest back (recovery.nix); the box test with the exact allocation size is convincing.
- The watchdog is not bound to syncd, resets its history on a pid change and skips inactive units (W:561-574),
  so it does not race systemd's own restart; counters flushed at a syncd start are handled (W:449).
- The A/B itself (5/5 vs 0/5 windows, then L0s on again on two later boots with errors back within minutes,
  under both U-Boots) is good evidence that L0s is the source of the replay errors.

## Suggested order

1. Commit the guard with M1 and L1; add the `guard` and `cpu-tx` signals and clear-and-count (H2, H3).
2. Fix M2 and M3 before deploying `cs8210-asic-aspm` and the vendor cap.
3. Run the crossover (H1) with that build; write the per-block table into sai-hw-learnwedge-20261116.txt.
4. At the first event in the L0s-on arm, capture before any restart: guard line, root-port UESta/CESta/LnkSta/
   secondary status, `ring_stat`, the ASIC's UESta (one config read, link up). That settles H3/M5.
