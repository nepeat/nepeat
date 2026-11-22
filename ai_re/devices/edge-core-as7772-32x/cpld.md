# AS7772-32X CPLD registers

I2C3 mux 0x76: CPLD1 0x60 (ch2, Linux `i2c-6`), CPLD2 0x62 (ch3, `i2c-7`), CPLD3 0x64 (ch4, `i2c-8`), fan CPLD 0x66
(ch0). Dumps: [evidence/i2c-dumps-20261121.txt](evidence/i2c-dumps-20261121.txt) (ONIE); a re-read under NixOS after `init soc`
on 2026-11-21 was identical for CPLD1-3 rows 0x00-0x4f.

Evidence: **D** = disassembly of the diag's `usr/local/sbin/cpldutil` (unstripped ppc32; `do_cpld_transceiver` at
0x10001db4 and the `CPLD_Get*` helpers), **S** = diag script, **V** = matches the hardware, **T** = expected-value
table `cpld1_reg_tbl` in `cpldtst` (0x10013374).

| CPLD | reg | meaning | evidence |
| --- | --- | --- | --- |
| 1 | 0x00 | model id (0x01 = "ES7632BT") | D |
| 1-3, fan | 0x01 | code version (CPLD1 4, CPLD2/3 1, fan 8) | D |
| 1 | 0x02 | PSU status (reads 0x28; the factory table expects 0x3c, PSU2 is not fitted) | D, T |
| 1 | 0x04-0x07 | QSFP reset, cage *n* = bit (*n*-1) mod 8 of reg 0x04 + (*n*-1)/8, **0 = in reset**; `-trans` clears the bit, waits 1 s, writes 0xff | D |
| 1 | 0x10-0x13 | QSFP interrupt, same bit layout, **0 = asserted** (`-trans` expects it low while in reset) | D |
| 1 | 0x30-0x33 | QSFP present, same bit layout, **0 = present**. Reads fc ff ff 3f = cages 1, 2, 31, 32, where the DACs are | D, V |
| 1 | 0x41 | system LEDs: 0x02 LOC blue + DIAG green, 0x01 LOC blue + DIAG red, 0x83 off | S |
| fan | 0x0f | fan module present | D |
| fan | 0x11 | fan duty; `-setfan` and `-trans` first write **0x33 = 0** (meaning unknown) | D |

Not decoded:
- **CPLD2/CPLD3 0x20-0x2b** (both read 00 00 f0 ff cc ff 40 ff 00 ff 00 00; the AS7760 has no such registers).
  No code in the factory U-Boot, either kernel, either rootfs, `cpldutil`/`cpldtst` or the vendor `bcm.user`
  (which has no I2C/CPLD access at all) touches them ([boot-chain-diff.md](boot-chain-diff.md),
  [evora-vendor-re.md](evora-vendor-re.md)). Unchanged from ONIE to NixOS, across cold power cycles and the
  vendor diag's (failed) `init all`; not re-read with links up. Structure only (inferred): two 16-bit pairs
  (0x20/21, 0x2a/2b = 0000), single bytes on even addresses (0x22 = f0, 0x24 = cc, 0x26 = 40, 0x28 = 00), odd
  ones unimplemented (ff). PHY reset/power was a guess and is not needed: all 32 PHYs answer
  ([extphy.md](extphy.md)).
- **CPLD1**, from the factory expected-value table (3-byte entries: reg, expected, 0; two lists, verified):
  list 1: 01=02 02=3c 08=ff 09=0d 0a=3f 0b=00 0c=ff 0d=ff 14=ff 15=c7 24=00 25=00 40=7c 41=82 42=00 43=05
  44=05 45=05 47=0f 48=70; list 2 (all 32 loopback modules fitted): 01=02 04-07=ff 10-13=ff **20-23=00**
  30-33=00. No meaning recovered for 0x09-0x0b, 0x15, 0x24/25, 0x40, 0x42-0x48. 0x20-0x23 is a 32-bit per-cage
  group that reads 0 with and without modules (LPMODE or a mask is a guess). 0x01 expected 2; this unit has
  code version 4. QSFP low-power mode: not found.

Nothing has been written to any CPLD register from NixOS.
