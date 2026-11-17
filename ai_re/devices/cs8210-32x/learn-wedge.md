# CS8210-32X: libifcs's learn-thread wedge and the dead CPU path

**Status (2026-11-17):** the bad read is defused in our libsai (learn-ring guard, mock + chip tested by
injection); both stacks are memory-capped; the watchdog notices a dead chip after the guard (guard / cpu-tx signals),
escalates (restart, ipd reload, alarm) and dates PCIe errors (clear-and-count). ASPM L0s on the ASIC link proven to
cause the link errors (A/B); that those cause the wedge is still inference: a crossover soak decides it (section 7).
Independent review: [reviews/2026-11-17-fable-learn-wedge.md](reviews/2026-11-17-fable-learn-wedge.md). Evidence:
[sai-hw-learnwedge-20261116.txt](sai-hw-learnwedge-20261116.txt). Incident: [oom-20261116.txt](oom-20261116.txt);
first analysis: [reviews/2026-11-10-sdk-learn-crash.md](reviews/2026-11-10-sdk-learn-crash.md).

## 1. The read (V, libifcs.so sha256 388031e7..., disassembly of `_ii_pcie_rupt_thread_learn`)

- The learn thread waits in `inno_rupt_wait` (ioctl 0xc0084904, 1000 ms timeout), then reads BAR0 0x4864, 0x485c
  and, under the ring mutex, 0x4860 (producer index B: slot bits 0..11, phase bit 12; ring 0x800 entries) through
  a wrapper (0x1670080) that calls `node_pci_reg_read32@plt` (JUMP_SLOT 0x187efb0). Return codes are ignored;
  on an error return the value is never written (uninitialised stack).
- Pending count n = size + B - A (same phase) else size - (B & 0xfff) + (A & 0xfff); `im_malloc(n * 8)` (PLT,
  JUMP_SLOT 0x187eae8: malloc + memset); then `while (A != B)` over the ring, B not re-read. B = all-ones gives
  n = 0xFFFFF801 (32 GiB) and a walk that never ends.
- At the end it writes 0x4864 (bits 0..12 = A, the consumer index) and 0x485c (bits 0..20) back over the values
  it read (`node_pci_reg_write32@plt`): an all-ones read of those two would also set every other bit.
- The thread polls about once a second, so on 2026-11-16 0x4860 read correctly ~290 times between the CPU path
  dying (16:00:55) and the bad read (16:05:45).

## 2. The guard (`ai_re/nix/packages/sai-teralynx/src/ifcs_guard.c`, in libsai-teralynx-shim.so)

ifcs_lib.cpp points libifcs's GOT entries for `node_pci_reg_read32`, `node_pci_reg_write32` and `im_malloc` at the
shim (as it already does for memcpy), before `ifcs_init` starts the thread. Only 0x4860 / 0x4864 / 0x485c are
touched: 0x4860 must have bits 13..31 clear and slot < ring size, the other two must not be all-ones. An invalid
value is re-read up to 3 times, 3 ms apart; still invalid: 0x4860 returns the consumer index last written to 0x4864
("nothing new", the poll is skipped), the RMW registers their last valid value (0 before any), and that register is
not re-read again until a read is valid (unanswered reads can stall the SoC; the thread's 1 Hz read is the probe).
All-ones from any other offset is only counted (first 16 offsets by name, one line each). `im_malloc` above 1 GiB
returns NULL instead of malloc + memset. Journal: `sai-teralynx: ifcs guard: ...` (at most 20 lines per 10 min,
written outside its lock); counters in `/run/sai-teralynx/ifcs-guard.prom` (`sai_teralynx_ifcs_guard_*`, invalid
reads also by error return: `invalid_rc_total`). Profile keys and the test
hook: [README](../../nix/packages/sai-teralynx/README.md). Vendor stack: not guarded (innovium.user is the vendor's
binary; an LD_PRELOAD build of the same code would interpose the PLT calls, not written).

## 3. What dies first (2026-11-16, V from VictoriaMetrics, I where marked)

On the self-loop only the CPU sends, so Ethernet0 RX = Ethernet120 TX: "both directions" is one failure, CPU TX.
- ~16:01:00 the ASIC stops sending CPU frames (COUNTERS_DB TX flat on both ports from 16:01:30).
- Linux kept handing frames to ipd (`node_network_transmit_packets_total`) until 16:03:45: 12222 - 12052 = 170
  (Ethernet0) + 12415 - 12244 = 171 (Ethernet120) frames the ASIC never sent = 341 x 3 descriptors = 1023: the TX-3
  ring (0x400 entries) exactly full. That is the 2026-11-10 (s) failure class (the ASIC's TX DMA stops consuming;
  the wmb() doorbell patch did not end it for good).
- From 16:03:45 `inno_enet_tx` returns NETDEV_TX_BUSY on every attempt without stopping the queue, so the qdisc
  requeues in a loop: the softirq on ~2 cores from exactly 16:03:45 (I: mechanism from the source, timing V).
- 16:05:45 one read of 0x4860 is not answered (all-ones), then the 32 GiB im_malloc. The root port read CmpltTO +
  UnxCmplt at 16:19 (undated): a completion that arrived after the 50 ms timeout (I).

## 4. The PCIe link: ASPM L0s (V A/B; causation I)

- Linux 7.3.4 `drivers/pci/pcie/aspm.c` enables L0s and L1 by default on devicetree platforms (DT overrides
  `aspm-no-l0s` / `aspm-no-l1` on the root port or device node). Here: root port 0001:00:00.0 LnkCtl "ASPM L0s
  Enabled" (L1 not: exit latency 64 us > the ASIC's 1 us acceptable), CommClk- on both ends; no other external link
  has ASPM (FPGA links: none, CommClk+). Same with our U-Boot training the link (2026-11-16 19:56: L0s on, CESta
  BadTLP + Timeout after 5 min of uptime).
- A/B 2026-11-16 17:01 (CESta cleared before each 1 s window): L0s on, 5/5 windows latch root port Timeout (replay
  timer) + BadTLP and ASIC Timeout + BadTLP + Rollover (REPLAY_NUM rollover: link retrain); L0s off, 0/5, and 0
  since (soak, root port polled every minute).
- Reading (I): the root port's transmitter leaves L0s too slowly for the ASIC's receiver (separate clocks), TLPs and
  ACKs are lost, replays and retrains run many times a second for as long as the box is up. One late or lost
  completion explains both a halted TX DMA (its descriptor/data fetches are reads of host memory) and an MMIO read
  past the completion timeout. Not proven: the stall itself was not reproduced, and the vendor's own kernel
  (factory DIAGOS) was not checked for ASPM.
- Mitigation: `hardware.cs8210.vendorSdk.asicAspm = false` (default): `cs8210-asic-aspm` (after cs8210-pcie-errors,
  before ipd and either stack; re-run by `cs8210-switch-power on` after its rescan) writes 0 to the ASIC's
  `link/l0s_aspm`/`l1_aspm` and fails if the root port's LnkCtl still has ASPM on. Kernel-side alternatives (need a
  reboot to verify): `pcie_aspm.policy=performance` (7.3.4 source: that policy maps to "no ASPM" for every link,
  DT or not) or `aspm-no-l0s` in a root-port child node of `pcie_external0` in the DTS.
- Not changed: completion timeout (both ends 50 us - 50 ms; the ASIC only supports range B, 65-210 ms; raise it only if
  CmpltTO recurs with L0s off); MSI-X (8 vectors, all on CPU 0, only the NAPI vector busy: not implicated).

## 5. ipd hazards (source of 0.16.2 + our patches; proposals, not changed)

- TX ring full: NETDEV_TX_BUSY without `netif_stop_queue`: the requeue spin above, and no NETDEV WATCHDOG line to
  date it. The round-2 flow-control patch fixes this but stays out (soft lockup, PROGRESS (z)). No MMIO per attempt:
  the `ipd_info` with two BAR reads in that path is compiled behind `ipd_loglevel >= 4` (the box runs 2), so the spin
  is CPU/lock load, not extra PCIe reads (C). ring_stat "Packets" counts frames when queued, "Full" each BUSY.
- RX: the in-loop refresh `posted_cidx = REG32(RXQ_x_DESC_CIDX)` is not range-checked (the writeback value is);
  all-ones gives 0xffff, which `rx->cidx` never equals: the SOP search walks stale descriptors.
- No TX-ring stall detection in the driver: the IFCS watchdog's tx-stall (180 s) restart is the ring reset.

## 6. Containment and metrics

- SONiC: syncd MemoryMax 2 GiB + OOMPolicy=stop; `sonic-cs8210-ifcs-watchdog` (docstring of ifcs-watchdog.py):
  signals kernel / vmsize / rss / learn-cpu / guard (0x4860 substituted 3 polls running) / cpu-tx (TX-3 Full rising,
  nothing queued, 4 polls) / tx-stall (now only with a witness: the host netdev kept sending). CPU-path signals
  escalate: restart, then (still dead within 15 min) an ipd reload in a transient unit, then ALARM and no more action
  until CPU TX moves; whether it moved is logged after every action. Nothing is done under the ASIC lease or a
  SONiC reload. Every poll counts, logs (dated; repeats once per 10 min) and clears the root ports' AER and LnkSta
  LBMS/LABS bits (FPGA ports: UESta/DevSta errors only counted, the cage guard reads them). Counters, the onset
  per signal and the last restart time live in `/run/sonic-cs8210-ifcs-watchdog/state.json` (a deploy keeps the
  15 min limit). Chip: guard signal from injected reads (dry run), clear-and-count with L0s on.
- Vendor stack: cs8210-switch MemoryMax 1536 MiB (`vendorSdk.memoryMaxMiB`; it peaks at 494 MB) + OOMPolicy=stop;
  cs8210-sdk-watchdog restarts it after an oom-kill stop (a second one within 15 min: when they have passed).
- Node exporter producer `cs8210-ifcs` (15 s): `cs8210_ifcs_watchdog_{triggers_total{signal},restarts_total,
  suppressed_total,held_total,escalations_total,recoveries_total,alarm,last_trigger_timestamp_seconds}`,
  `cs8210_pcie_aer_events_total{port,type,error}`, `cs8210_asic_rootport_aer_error{type,error}` (dates the next
  CmpltTO to 15 s), `cs8210_asic_link_{up,speed_gts,width,aspm_enabled}`, `cs8210_ipd_tx_ring_full_total`,
  `cs8210_ipd_ring_packets_total`, and the guard's `sai_teralynx_ifcs_guard_*` while its file is fresh.

## 7. Soak: L0s crossover

Runs so far, all clean (no watchdog trigger, no guard event, root-port AER 0): L0s off 1 h 11 min + 33 min + 19 min,
cut by U-Boot SPI windows. That has no statistical weight (review H1: p = 0.59 even if L0s did not matter), and the
wedge rate under SONiC + our U-Boot is unknown (one event, 42 min after start; a clean 2 h run with L0s on).
Design (from the review): same boot chain (our U-Boot, SPI), the deployed guard build in both arms, blocks of >= 4 h
in the order on, off, off, on (repeated), each started with a syncd restart under the lease; endpoints per block:
guard invalid / substituted, watchdog triggers by signal, TX-3 Full rising, root-port CmpltTO / UnxCmplt / LBMS counts.
Decision: equal on/off time needs >= 5 events on and 0 off (p = 0.03, conditional binomial); 3 on / 0 off is only
p = 0.125 at equal time. If the on arm stays clean too, the rate is just low and L0s is not credited. Controller
`lw-crossover` (transient unit, `/var/lib/lw-soak/`); per-block table: [sai-hw-learnwedge-20261116.txt](sai-hw-learnwedge-20261116.txt).
