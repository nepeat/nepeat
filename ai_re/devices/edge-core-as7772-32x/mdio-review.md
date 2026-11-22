# AS7772-32X: MDIO towards the BCM82391 PHYs, source-level review (2026-11-22)

Offline review of SDK source, vendor binary and recorded traces. The rule and the live proof are in
[extphy.md](extphy.md). SDK = OpenBCM 6.5.27 (`/nix/store/lgykiyx22bsz6vjh9dap980nwlmpigbz-openbcm-sdk-6.5.27-patched`).

**Answer: `init soc` never programs the external MDIO timing.** On Tomahawk the divisor and the output delay are
written by `_soc_tomahawk_misc_init` (`init misc` / `init all`), not by `soc_tomahawk_chip_reset` (`init soc`).
Our scans and the first re-host bridge ran at the CMIC reset values: `CMIC_RATE_ADJUST_EXT_MDIO` = 0x00010018
(divisor 24), `CMIC_MIIM_CONFIG.MDIO_OUT_DELAY` = 3. SDK default and vendor value for the delay: **10**. The
config keys were right; they were never read.

## SDK 6.5.27 (verified in source)
| what | where |
| --- | --- |
| properties read | `src/soc/esw/tomahawk.c:7927-7968` in `_soc_tomahawk_misc_init` (:7200, registered :14388): `rate_ext_mdio_divisor` (default (freq+3)/4 = 213), `rate_ext_mdio_dividend` (1) -> `CMIC_RATE_ADJUSTr` (diag name `CMIC_RATE_ADJUST_EXT_MDIO`, 0x11000); `mdio_output_delay` (default 10, written only if 1..15) -> `CMIC_MIIM_CONFIG.MDIO_OUT_DELAY` (0x11184, 4 bits) |
| MDC | comment formula: core clock (850 MHz, :953) x dividend / divisor / 2: 24 -> 17.7 MHz, 53 -> 8.0 MHz, 213 -> 2.0 MHz (**not measured**) |
| nothing else writes them | no other write in `tomahawk.c`, `drv.c`, `src/soc/common/`, `src/bcm/esw/` (grep); `soc_tomahawk_chip_reset` (:2076-2671) does not touch them |
| reset values | `src/soc/mcm/allregs_c.i`: `CMIC_RATE_ADJUST_BCM56960_A0` 0x00010018 (:1330665), `CMIC_MIIM_CONFIG_BCM56960_A0` 0x3 (:1240430); both match the live read-backs |
| init commands | `src/appl/diag/shell.c:6217-6316`: `init soc` = reset only; `init misc` = `soc_misc_init`; `init all` = reset, misc, mmu, bcm (`system.c:2191-2199`) |
| `phy raw c45` | `src/appl/diag/phy.c:8947` -> `soc_miimc45_read/write` (`src/soc/common/miim.c:2565`, :2191): one complete CMIC operation at a time under `MIIM_LOCK`, waits for `OPN_DONE` |
| id encoding | `include/soc/phyctrl.h:1081-1091`: bus = bits 9:8 (as bus bits 3:2) and bits 6:5, bit 7 = internal; BUS_ID is 3 bits, so buses 0-7 (0x110 = bus 4 addr 0x10, 0x17f = bus 7 addr 0x1f) |
| other keys | `miim_intr_enable` (`drv.c:22516`, default 0 = polled; no effect on the wire), `miim_timeout_usec` (:22490); `mdio_io_voltage` is read only for BCM56340 / Triumph3: no effect here; there is no "phy_clause45" key |
| default external PHY probe | bus 0-3, addr 2-9 (`src/soc/phy/phyident.c:3694-3704`): overlaps Evora ids 0x02, 0x03, 0x24-0x27, 0x48, 0x49. Whether `init all` sends probe frames there without `port_phy_addr` (`portctrl.c:202`) was **not traced** |

Read-back: `g CMIC_RATE_ADJUST_EXT_MDIO`, `g CMIC_MIIM_CONFIG`, `g CMIC_RATE_ADJUST_INT_MDIO`.

## Vendor (SDK 6.5.9 `bcm.user`, configs)
- Every `rc.soc-*` starts with **`init all`**, so misc init runs before `extphy init all`.
- Config keys: only `rate_ext_mdio_divisor=53`, `rate_ext_mdio_dividend=1` (config.bcm-100G:17-18); no script
  writes a CMIC register.
- Binary: the routine that reads `rate_ext_mdio_divisor` (default (freq+3)>>2, 0x111820b8-0x111820d0) reads
  **`mdio_output_delay` with default 10** (0x11183e20-0x11183e30, `li r5,0xa`), range 1..15. Identified as the
  Tomahawk routine by its default formulas only (inference). Three other readers (0x11197278, 0x111be88c,
  0x111f5e28) default to -1: other chip families.
- Factory state: **divisor 53 (8 MHz), delay 10**. No other id encoding, per-bus setup or probe pass found.

## Reading of the sweep (`evidence/mdio-delay-sweep-20261122.txt`, `evidence/mdio-rate-20261122-*.txt`)
- Results depend only on the delay, not the divisor (24, 53, 255 give the same table): the delay is an absolute
  hold time, a slower MDC cannot compensate. Delay 0-2: reads shifted by one bit (0x4723 = 0x2391 << 1 | 1) or
  none; 3: 20/32; 4-15: 26/32 in that power cycle (the others were already hung).
- Delays above 15 are rejected by the field check, so the "SET x 31..255" rows repeat delay 15.
- Inference, not proven: at delay <= 3 the PHY samples at the wrong MDC edge, so frames (also reads) can be
  decoded as other frames, including garbage writes. That would explain hangs with no vendor code involved.

## Id 0x02 in the first-night traces (`dumps/x/evora-trace/`)
- `b0-init.trace.txt` vs the good `init-cfg-0x24-0x27.trace.txt` (ids mapped): lines 2-1242 identical, every
  read value included. The download (lines 1243-50084, addressed to id 0x03) has the same handshake sequence,
  98 more idle polls. **First differing read: line 50137, `R 002 1 8251 ff9c`**, already the hung constant: id
  0x02 died while only listening, the trace cannot say at which word.
- The same id failed at another place the night before (system-side LMI read time-out before any download).
  Two failure points with identical preceding reads = corrupted frames, not a step the chip cannot do. Confirmed
  on 2026-11-22: id 0x02 passes the init from a cold start with `init misc`.

## Fix
- Run `init misc` after `init soc` (or `init all`) before the first external MDIO access;
  `rate_ext_mdio_divisor=53`, `mdio_output_delay=10` in config.bcm. Done ([extphy.md](extphy.md)).
- Not done, not needed: an SDK patch repeating the misc-init MDIO block at the end of `soc_tomahawk_chip_reset`
  (before the final return at :2670; `rval` at :2089) so that `init soc` alone is safe. The proposed diff is in
  this file's history (commit `b7ce68b`); never built or tested.

## Not verified
- The MDC formula, the unit of `MDIO_OUT_DELAY`, why 3 is marginal on this board.
- That soc reset restores the CMIC reset values rather than leaving a previous session's (the read-backs 0x18 /
  3 fit either).
- That the vendor routine at 0x11182xxx-0x11183xxx is the Tomahawk one.
- Whether bcm port init probes the default external addresses.
- Whether the divisor changes MDC at all (test: `m CMIC_RATE_ADJUST_EXT_MDIO DIVISOR=0xffff DIVIDEND=1`, time
  100 reads, expect about 20 ms each; restore 53). Low value.
- Every transcript starts with `sal_config_refresh: format error in config.bcm on line 3 (ignored)`: one line of
  the config used on the switch is not parsed. Not looked at.
