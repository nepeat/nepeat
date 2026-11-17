# 2026-11-17: why only the root port shows L0s on the ASIC link, and what the soak's "on" arm tests

Reviewer: Fable. Box access read-only (lspci -vvv/-xxxx of 0001:00:00.0 and 0001:01:00.0, sysfs reads, journal), during
crossover block 1 (arm on), 2026-11-17 00:18 PST. Nothing written to the box, no tracked file changed.
Marks: **V** = read from the box / source / a capture, **I** = inferred.
Live capture: [lspci-asic-link-l0s-on-20261117-0018.txt](../analysis/research/learn-wedge-20261116/lspci-asic-link-l0s-on-20261117-0018.txt)
(sha256 71df890f4e157069..., local raw dir). Kernel source: linux-7.3.4 tarball from the nix store, `drivers/pci/pcie/aspm.c`.

## Findings

1. **The asymmetry is Linux's per-direction L0s latency check, nothing else. (V)** Both ends advertise L0s and L1
   (LnkCap ASPM support = 3 on both). `pcie_aspm_check_latency` (aspm.c:608-675) compares each direction's exit latency
   with the endpoint's acceptable latency (ASIC DevCap: L0s 1024 ns):
   - `L0S_UP` (ASIC transmitter idles; the root port's receiver must wake): root port LnkCap exit class 6 = 4096 ns
     > 1024 ns: removed from `aspm_capable`. This is the bit that would go into the ASIC's LnkCtl (aspm.c:1072-1073).
   - `L0S_DW` (root port transmitter idles; the ASIC's receiver must wake): ASIC LnkCap exit class 2 = 256 ns
     <= 1024 ns: kept. This is the bit in the root port's LnkCtl (aspm.c:1074-1075).
   - L1: max(64 us, 8 us) > 1 us acceptable: removed. Cross-check: `link/` on the ASIC holds only `l0s_aspm`
     (visibility is by `aspm_capable`, aspm.c:1816-1838), and dmesg has "ASPM: default states L0s L1" (the DT default
     before the latency check).
   - Not the cause: the soak controller (`echo 1 > link/l0s_aspm` only clears `aspm_disable`; the kernel then applies
     default AND capable = `L0S_DW`, aspm.c:1717-1745, 1052); `cs8210-asic-aspm` (ran once at 21:18:43, "LnkCtl=0c00",
     writes only 0); the ASIC's LnkCap.
2. **Physically: only the SoC-to-ASIC direction enters L0s.** The root port's transmitter goes to electrical idle when
   it has nothing to send and wakes with fast training sequences; the ASIC's receiver has to relock on them. The
   ASIC-to-SoC direction never leaves L0. One sample of the DesignWare debug register caught it: PORT_DEBUG0 (0x728)
   = 0x03000092, LTSSM field 0x12 = `DW_PCIE_LTSSM_L0S` (V, single read).
3. **The on arm is register-identical to the pre-fix box. (V)** Root port LnkCtl 0x0c01 / ASIC 0x0000, CommClk- on
   both, same LnkCap/DevCap, in every capture with our kernel: 11-10 (wedge day, vendor stack), 11-15, 11-16 16:59 and
   17:01 (A/B), vendor and our U-Boot, and now. So the arm reproduces the condition the wedges happened under. The
   word "L0s on" should read "root-port-transmit L0s (kernel default)"; the ASIC's own transmitter was never in L0s.
4. **New: the factory ONIE kernel (4.9.124) leaves ASPM off on this link** (LnkCtl "ASPM Disabled" both ends, CESta and
   UESta all clear on both; evidence/live-onie-2026-11-09/lspci-vvnn.txt lines 363, 427, 384-387, 440-443) (V). That
   closes half of learn-wedge.md section 4's "vendor kernel not checked" (DIAGOS itself still is not).
5. **The correctable stream fits one-direction L0s directly (I, on top of the V A/B).** After an L0s exit the ASIC's
   receiver is not locked when the first TLP or ACK arrives: a TLP it misses entirely gives the root port's replay
   timer timeout (CESta 0x1000); one it sees damaged gives the ASIC's BadTLP; lost ACK DLLPs (ACKs for the ASIC's
   DMA traffic travel on the idling direction) give the ASIC's own Timeout and, four in a row, Rollover = retrain.
   The root port's BadTLP (about half the polls) is on the direction without L0s and is not explained directly:
   most likely TLPs cut by those retrains (I, weakest link). An honest L0s exit cannot time out a replay timer (it
   starts when the TLP has been sent, and the ACK path is awake), so the timeouts mean lost TLPs, not slow exits.
6. **The ASIC very likely asks for too few FTS (I).** The root port needs N_FTS = 220 for its own receiver
   (PORT_AFR 0x70c = 0x1bdcdc00: N_FTS 0xdc, common-clock N_FTS 0xdc; 220 x 16.25 ns = 3.6 us = its LnkCap "&lt;4us").
   The ASIC claims 128-256 ns, at most ~15 FTS at 8 GT/s, while reporting SlotClk- (its own reference clock). The
   N_FTS it actually sent in training is not readable from either config space.
7. **Common Clock is not the lever.** CommClk- is Linux's decision from the ASIC's LnkSta SlotClk- (root port SlotClk+;
   `pcie_aspm_configure_common_clock`, aspm.c:409-430) (V); ONIE has it clear too. CommClk- makes a port advertise
   its larger, separate-clock N_FTS, so it is the conservative setting and the link fails anyway. Setting it would make
   the ASIC ask for the same or fewer FTS (the root port's two values are equal), needs a retrain, and is wrong if the
   clocks really are separate: predicted no change or worse (I).
8. **CmpltTO/UnxCmplt: plausible as the rare tail of the retrains, and now dated against an event for the first time.**
   - Timeouts (V): root port DevCap2 0x141f (ranges ABCD), DevCtl2 0x0000 = default 50 us - 50 ms; ASIC DevCap2
     0x00140812 (range B only), DevCtl2 0x0000 = default. Actual values inside that range are not readable.
   - A single replay or rollover retrain is far shorter than tens of ms; a completion later than the timeout needs
     a chain of them or one slow Recovery. LnkSta stayed 0x3043 (no LBMS/LABS) in every poll, which a same-speed
     retrain does not set. So: 1 UE against ~700 polls with CE fits "rare tail", but the long outage itself is
     not observed (I).
   - Block 1 timeline (V, journal): 23:42:41 guard, 0x4864 read all-ones, good on re-read 2; 23:42:55 root port
     UESta 0x00014000 (CmpltTO + UnxCmplt), CESta 0x3040, in the same 15 s poll; 23:45:56 tx-stall (a 180 s signal,
     so CPU TX was flat from about 23:42:56). One bad MMIO read, the root port's completion timeout and a dead TX
     ring within ~15 s of each other.
   - **The ASIC's own AER now reads UESta 0x00014000 (CmpltTO + UnxCmplt), First Error Pointer 14 = CmpltTO, CESta
     0x3140 (V).** As requester the ASIC only reads host memory (descriptor/data DMA), so a DMA read of its own timed
     out and the completion came late: the mechanism learn-wedge.md section 3/4 proposes for the TX stall, and the
     answer to review M5 ("ASIC CmpltTO- on 11-10 argues against"). Undated: clear at 11-16 19:31
     (uboot-compare/ours-fix-20261117/lspci.txt:132), set by 11-17 00:18; nothing clears or samples the ASIC's AER.
   - The first stall (23:20:05) had no root-port UE and no guard event: consistent with only the ASIC's read
     timing out, which today leaves no dated trace (I).
9. **Soak verdict: the on arm is a valid test of the question that matters** (kernel-default ASPM vs off, same
   registers as the incident). It does not by itself separate "L0s exit errors" from other effects of L0s; the
   10-second experiment below does.

## Register values (2026-11-17 00:18, V)

| | root port 0001:00:00.0 | ASIC 0001:01:00.0 |
|---|---|---|
| LnkCap | 0x00736c43: ASPM L0s+L1, L0s exit class 6 (&lt;4 us), L1 class 6 (&lt;64 us) | 0x0041ac43: ASPM L0s+L1, L0s exit class 2 (&lt;256 ns), L1 class 3 (&lt;8 us) |
| DevCap acceptable | (root port: n/a) | 0x00008102: L0s class 4 (1 us), L1 class 0 (1 us) |
| LnkCtl | 0x0c01: L0s enabled, CommClk-, ExtSynch- | 0x0000: ASPM off, CommClk-, ExtSynch- |
| LnkSta | 0x3043: 8 GT/s x4, SlotClk+, DLActive+ | 0x0043: 8 GT/s x4, SlotClk- |
| DevCap2 / DevCtl2 | 0x0000141f / 0x0000 | 0x00140812 / 0x0000 |
| AER UESta / CESta | 0 / 0x1000 (cleared every 15 s by the watchdog) | 0x00014000 / 0x00003140, FEP 14 |
| sysfs | no `link/` attributes | `link/l0s_aspm` = 1 (only file); policy [default] |
| DWC port logic | 0x700 = 0x006d0024, 0x70c = 0x1bdcdc00 (L0s entrance field 3), 0x718 = 0x00020000, 0x728 = 0x03000092 | - |

Journal counts, block 1 up to 00:18 (state.json): root port Timeout 707 polls, BadTLP 589, CmpltTO 1, UnxCmplt 1;
tx-stall 2 (23:20:05, 23:45:56), both recovered by the syncd restart; guard invalid 1.

## Proposed (nothing done)

1. **Extended Synch A/B (10 s, between blocks, under the lease).** With L0s on, set LnkCtl bit 7 on the root port only
   (on-SoC config write; aspm.c never touches that bit): the transmitter then sends 4096 FTS on every L0s exit instead
   of the count the ASIC asked for. Same method as the 17:01 A/B (clear CESta on both ends, 5 x 1 s windows each):
   L0s on / L0s on + ExtSynch / off. Errors gone with ExtSynch = the ASIC's receiver needs more FTS than it
   advertises (finding 6 proven); errors unchanged = its L0s receive path is broken beyond relock time. Either way
   L0s stays off in production; this only settles the mechanism.
2. **Sample the ASIC's AER in the soak.** At each block start (under the lease) read and W1C-clear the ASIC's UESta /
   CESta, and read UESta once at every tx-stall trigger before the restart (one config read, link up). That dates the
   ASIC-side CmpltTO per stall, which finding 8 shows is the missing witness.
3. Keep the arm definition; rename it in the docs (finding 3). No Common Clock experiment (finding 7). An ASIC-transmit
   L0s arm is not worth running: Linux will never enable it here and it violates the ASIC's own acceptable latency.
4. If `aspm-no-l0s` goes into the DTS: `aspm_support` loses L0s (aspm.c:895-899), `link/l0s_aspm` disappears, and the
   crossover controller can no longer switch arms through sysfs. Finish the soak first, or keep the DT change out of
   the soak build.
5. Doc fixes for learn-wedge.md section 4 when the soak is written up: "the root port's transmitter leaves L0s too
   slowly" -> the ASIC's receiver does not relock in the FTS it asked for (I); add the ONIE fact (finding 4) and the
   ASIC-side CmpltTO (finding 8).

## Not verified

- The N_FTS the ASIC advertises, the real replay-timer and completion-timeout values, and whether the board feeds
  both ends from one reference clock (SlotClk- is the ASIC's claim only).
- DWC field meanings beyond those named in mainline `pcie-designware.h` (0x700, 0x718, the L0s entrance encoding).
- That 4096 FTS applies unchanged at 8 GT/s on this DesignWare build (spec behaviour, not tested here).
- When the ASIC's CmpltTO latched, and that the long outage behind a completion timeout is a retrain chain.
