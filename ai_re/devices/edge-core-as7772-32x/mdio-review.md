# AS7772-32X: is our MDIO towards the BCM82391 PHYs wrong? (review, 2026-11-22)

Offline review (SDK source, vendor binary, recorded traces); I ran nothing on the device. Live facts quoted here are
the coordinator's (`mdio-delay-sweep-20261122.txt`, `mdio-rate-20261122-*.txt`, and the cold-boot result reported
to me: first access at delay 8, all 32 ids answer). SDK = `/nix/store/lgykiyx22bsz6vjh9dap980nwlmpigbz-openbcm-sdk-6.5.27-patched`.

## Answer
**Yes. `init soc` never programs the external MDIO timing.** On Tomahawk the divisor and the output delay are
written by `_soc_tomahawk_misc_init` (`init misc`, or `init all`), not by `soc_tomahawk_chip_reset` (`init soc`).
Every scan and the whole re-host bridge (`evora-rehost/mdio.py:71`, `init=('init soc',)`) therefore ran with the
CMIC reset values: `CMIC_RATE_ADJUST_EXT_MDIO` = 0x00010018 (divisor 24) and `CMIC_MIIM_CONFIG.MDIO_OUT_DELAY` = 3.
The SDK's own value for the delay is **10**, and the vendor binary uses the same 10. The property names are right;
they were not ignored, they were never read.

## 1. SDK 6.5.27 (verified in source)
- **Where the properties are read:** `src/soc/esw/tomahawk.c:7927-7968`, inside `_soc_tomahawk_misc_init` (starts
  :7200, registered as the chip's `soc_misc_init` at :14388). `rate_ext_mdio_divisor` (default (freq+3)/4 = 213
  at 850 MHz, i.e. 2 MHz) and `rate_ext_mdio_dividend` (1) go to `CMIC_RATE_ADJUSTr` (:7935-7942; diag name
  `CMIC_RATE_ADJUST_EXT_MDIO`, offset 0x11000); `rate_int_mdio_divisor/dividend` to `CMIC_RATE_ADJUST_INT_MDIO`
  (:7953-7960); **`mdio_output_delay` (default 10, written only if 1..15)** to `CMIC_MIIM_CONFIG.MDIO_OUT_DELAY`
  (:7962-7968, offset 0x11184, 4-bit field). Comment formula: MDC = core clock x dividend / divisor / 2
  (850 MHz from :953): 24 -> 17.7 MHz, 53 -> 8.0 MHz, 213 -> 2.0 MHz. (Formula from the comment, not measured.)
- **Nothing else writes them:** no other `CMIC_RATE_ADJUSTr` / `CMIC_MIIM_CONFIGr` write in `src/soc/esw/tomahawk.c`,
  `src/soc/esw/drv.c`, `src/soc/common/`, `src/bcm/esw/` (grep). `soc_tomahawk_chip_reset` (:2076-2671) does not
  touch them. So nothing is hardcoded; `init soc` leaves the hardware reset values.
- **Reset values** (`src/soc/mcm/allregs_c.i`): `CMIC_RATE_ADJUST_BCM56960_A0` 0x00010018 (:1330665),
  `CMIC_MIIM_CONFIG_BCM56960_A0` 0x3 (:1240430). Both match the live read-backs (divisor 0x18, delay 3).
- **Init commands** (`src/appl/diag/shell.c:6217-6316`): `init soc` = `sh_do_init` (reset only); `init misc` =
  `soc_misc_init`; `init all` = `system_init` = reset, misc, mmu, bcm (`src/appl/diag/system.c:2191-2199`).
- **`phy raw c45 <id> <devad> <reg>`** (`src/appl/diag/phy.c:8947`) -> `soc_miimc45_read/write`
  (`src/soc/common/miim.c:2565`, :2191). CMICm path: `CMIC_CMC0_MIIM_ADDRESS` = devad + reg, `CMIC_CMC0_MIIM_PARAM` =
  MIIM_CYCLE 0 (auto: address frame then read/write), C45_SEL 1, INTERNAL_SEL = id bit 7, BUS_ID =
  `PHY_ID_BUS_NUM(id)`, PHY_ID = id & 0x1f, PHY_DATA on writes; then `CMIC_CMC0_MIIM_CTRL` = 2 (read) or 1 (write),
  poll `MIIM_STAT.OPN_DONE`, clear CTRL (miim.c:2609-2819). One operation at a time under `MIIM_LOCK`.
- **Id encoding is right:** `include/soc/phyctrl.h:1081-1091`: bus = bits 9:8 (as bus bits 3:2) | bits 6:5, bit 7 =
  internal. BUS_ID is 3 bits (`fields_c.i:150048`), so buses 0-7; 0x110 = bus 4 addr 0x10, 0x17f = bus 7 addr 0x1f.
  Which package pins carry buses 4-7 is not in the source; that all 8 are wired is shown by the PHYs answering.
- **Other keys:** `miim_intr_enable` (`src/soc/esw/drv.c:22516`, default 0 = polled; completion method only, no
  effect on the wire). `miim_timeout_usec` (:22490). `mdio_io_voltage` is read only in the BCM56340 and Triumph3 reset
  code (`drv.c:16267`, `triumph3.c:7456`): no effect on Tomahawk. There is no "phy_clause45" key; C45 is per call.
- **Default external PHY probe addresses** on Tomahawk are bus 0-3, addr 2-9 (`src/soc/phy/phyident.c:3694-3704`),
  which overlap Evora ids 0x02, 0x03, 0x24-0x27, 0x48, 0x49. Whether `init all` actually sends probe frames there
  when no `port_phy_addr` is set (`src/soc/esw/portctrl.c:202`) I did not trace. The vendor's `init all` is the same code path.
- Read-back: `g CMIC_RATE_ADJUST_EXT_MDIO`, `g CMIC_MIIM_CONFIG`, `g CMIC_RATE_ADJUST_INT_MDIO`.
- Every transcript starts with `sal_config_refresh: format error in config.bcm on line 3 (ignored)`: one line of the
  config used on the switch is not parsed. Not the cause here, but worth a look.

## 2. Vendor (SDK 6.5.9 `bcm.user`, configs)
- `rc.soc-100G`/`-25G`/`-MFG`/`-dbg` all start with **`init all`**, so misc init runs before `extphy init all`.
- Config keys: only `rate_ext_mdio_divisor=53`, `rate_ext_mdio_dividend=1` (config.bcm-100G:17-18). No
  `mdio_output_delay`, no other MDIO/PHY key; no script writes a CMIC register.
- Binary (disassembly, script in the session scratchpad, not kept): the function that reads `rate_ext_mdio_divisor`
  with default (freq+3)>>2 (0x111820b8-0x111820d0) and `rate_int_mdio_divisor` with default (freq+23)/24
  (0x11183d6c-0x11183d94) reads **`mdio_output_delay` with default 10** (0x11183e20-0x11183e30, `li r5,0xa`), range
  check 1..15. The other three `mdio_output_delay` readers (0x11197278, 0x111be88c, 0x111f5e28) default to -1 and
  belong to other chip families. Identification as the Tomahawk routine is by these default formulas (inference).
- So the factory state is **divisor 53 (8 MHz), MDIO_OUT_DELAY 10**. No different id encoding (see
  [evora-vendor-re.md](evora-vendor-re.md)), no per-bus setup, no extra probe pass found.

## 3. What the sweep shows (coordinator's files, my reading)
- Results depend only on the delay, not on the divisor (24, 53, 255 give the same table at every delay): the delay is
  an absolute hold time, so a slower MDC cannot compensate. Delay 0-2: reads shifted by one bit (0x4723 =
  0x2391 << 1 | 1) or none; 3: 20/32; 4-15: 26/32 in that power cycle (the other five already hung).
- Writes with delay above 15 were rejected by the field check, so "SET x 31..255" rows repeat delay 15.
- Inference: at delay <= 3 the PHY samples MDIO at the wrong MDC edge, so frames (also reads) can be decoded as
  other frames, including writes of garbage. That fits "PHY hangs with no vendor code involved" and the clause-22
  suspicion in extphy.md; it is not proven. **Sweeping delays 0-3 on a populated bus is itself risky.**

## 4. Id 0x02 in the traces (`dumps/x/evora-trace/`)
- `b0-init.trace.txt` vs the good `init-cfg-0x24-0x27.trace.txt` (ids mapped 0x00-0x03 -> 0x24-0x27): **lines 2-1242
  are identical**, every read value included (ID, reset, both LMI sides, broadcast enable). Id 0x02's last normal
  access is line 1241-1242 (`R 1.8a4c 0024`, `W 1.8a4c 0a24`; the good group does the same on 0x26).
- Download (lines 1243-50084, all addressed to id 0x03): same handshake sequence as the good group; the only
  difference is 98 more idle polls of 1.8221 = 0x0000 (780 vs 682). The master was fine.
- **First differing read: line 50137, `R 002 1 8251 ff9c`** (good PHYs: 0x0000 then 0x000d). 0xff9c is already the
  hung constant (line 66218, 1.826c, the same). So id 0x02 died somewhere in lines 1243-50136 while it was only
  listening; the trace cannot say at which word because nothing reads it in between.
- The first night's failure of the same id was at a different place (system-side LMI read time-out before any
  download, hang after an LMI write in the end stage; [evora-rehost.md](evora-rehost.md) "Bus 0").
- Judgment: both runs used delay 3 (bridge = `init soc`). Two different failure points with identical preceding
  reads look like corrupted frames, not a deterministic step the chip cannot do. It is not a lost firmware word in
  the usual sense (a bad image fails the 0x600d check; it does not turn every register into a constant). A slower
  MDC or `fw_load=1` would not have helped, since the hold time does not scale with the rate. A chip-side fault is
  **not excluded** until 0x02 passes an init with delay >= 8 from a cold start.
- Emulator write pipelining: each `phy raw` is a complete CMIC operation that waits for OPN_DONE before the next,
  so frames cannot overlap; spacing is at least the vendor's. I see no way it matters. (Not tested.)

## 5. Fix
No SDK patch is needed for correctness: **run `init misc` after `init soc`** (or `init all`) before the first
external MDIO access, with `rate_ext_mdio_divisor=53`, `rate_ext_mdio_dividend=1` and, to pin it,
`mdio_output_delay=10` in config.bcm. Places that use `init soc` only: `evora-rehost/mdio.py:71`,
`mdio-id-8b00.soc`, `mdio-scan.soc`, `mdio-silent.soc`, `mdio-probe.soc`. `init misc` standalone after `init soc`
is the order `system_init` uses; I have not run it alone on this board.

Optional patch so that `init soc` alone is safe (proposal, **not built or tested**; would be
`nix/packages/openbcm/patches/0003-tomahawk-program-external-MDIO-timing-in-chip-reset.patch`). It repeats the
misc-init logic at the end of `soc_tomahawk_chip_reset` (`si->frequency` is already valid there, used at :2216):
```diff
--- a/src/soc/esw/tomahawk.c
+++ b/src/soc/esw/tomahawk.c
@@ soc_tomahawk_chip_reset(int unit)   (before the final "return SOC_E_NONE;" at :2670, after the "#endif" at :2668)
+    /* External MDIO rate and output delay: also set in misc init, but
+     * "init soc" users (raw MDIO to external PHYs) need them too. */
+    if (soc_feature(unit, soc_feature_cmicm)) {
+        int mdio_div, mdio_dvd, mdio_dly;
+        mdio_div = (SOC_INFO(unit).frequency + 3) / 4;
+        mdio_div = soc_property_get(unit, spn_RATE_EXT_MDIO_DIVISOR, mdio_div);
+        mdio_dvd = soc_property_get(unit, spn_RATE_EXT_MDIO_DIVIDEND, 1);
+        rval = 0;
+        soc_reg_field_set(unit, CMIC_RATE_ADJUSTr, &rval, DIVISORf, mdio_div);
+        soc_reg_field_set(unit, CMIC_RATE_ADJUSTr, &rval, DIVIDENDf, mdio_dvd);
+        WRITE_CMIC_RATE_ADJUSTr(unit, rval);
+        mdio_dly = soc_property_get(unit, spn_MDIO_OUTPUT_DELAY, 10);
+        if (mdio_dly >= 1 && mdio_dly <= 15) {
+            READ_CMIC_MIIM_CONFIGr(unit, &rval);
+            soc_reg_field_set(unit, CMIC_MIIM_CONFIGr, &rval, MDIO_OUT_DELAYf, mdio_dly);
+            WRITE_CMIC_MIIM_CONFIGr(unit, rval);
+        }
+    }
```
(`rval` is declared at :2089; the insertion point is outside the `#ifdef BCM_IPROC_SUPPORT` block.)

## 6. Live experiments, ranked
1. **Prove the config path.** config.bcm with `rate_ext_mdio_divisor=53`, `mdio_output_delay=10`; then
   `init soc`, `init misc`, `g CMIC_RATE_ADJUST_EXT_MDIO` (expect DIVISOR=0x35), `g CMIC_MIIM_CONFIG`
   (expect MDIO_OUT_DELAY=0xa). Change the delay key to 8 and repeat to prove the property is read.
2. **(a) silent ids, confirmation.** Cold power cycle, the sequence of 1 as the very first commands, then
   `mdio-id-8b00.soc` without its `init soc` line: expect 32 x 0x2391. Repeat on 2-3 power cycles (0x17d used to
   come and go).
3. **(b) id 0x02.** Same cold start, bridge changed to `init=('init soc','init misc')`, then
   `rehost.py --backend ssh 'init 0x00 4'`. Pass = MDIO cause for 0x02. If it fails again at delay 10:
   `init 0x02 1 fw_load=1` after another cold start, and compare the new trace with section 4; a failure there
   points at the chip or its system side.
4. **Is `init all` safe before the PHY init?** After a cold start run `init all` (delay 10) and then the id scan.
   If ids 0x02, 0x03, 0x24-0x27, 0x48, 0x49 (the SDK's default probe addresses) are affected and others not, the
   port probe is sending frames to them: then set `port_phy_addr` away from the Evora ids or keep
   `init soc` + `init misc` for the PHY phase.
5. **Does the divisor change MDC at all?** `m CMIC_RATE_ADJUST_EXT_MDIO DIVISOR=0xffff DIVIDEND=1` (about 6.5 kHz
   by the formula), then time 100 x `phy raw c45 0x24 1 0x8b00`: expect about 20 ms each instead of well under
   1 ms. Restore 53 afterwards. Low value; only if the 8 MHz figure matters.
6. Do **not** repeat delays 0-3 with PHYs that are wanted afterwards.

## Not verified
- Anything live; the MDC formula; that soc reset (CPS reset) restores the CMIC reset values rather than leaving a
  previous session's (the read-backs 0x18 / 3 are consistent with either a reset or never-written registers).
- The unit of MDIO_OUT_DELAY and why 3 is marginal on this board.
- That the vendor routine at 0x11182xxx-0x11183xxx is the Tomahawk one (matched by default formulas only).
- Whether bcm port init probes the default external addresses; the proposed patch.
