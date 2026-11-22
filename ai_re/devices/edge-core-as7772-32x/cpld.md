# AS7772-32X CPLD registers

I2C3 mux 0x76: CPLD1 0x60 (ch2, Linux `i2c-6`), CPLD2 0x62 (ch3, `i2c-7`), CPLD3 0x64 (ch4, `i2c-8`), fan CPLD 0x66
(ch0). Dumps: [i2c-dumps-20261121.txt](i2c-dumps-20261121.txt) (ONIE); a re-read under NixOS after `init soc`
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
  No diag tool or script touches them and the vendor `bcm.user` has no I2C/CPLD access at all
  ([evora-vendor-re.md](evora-vendor-re.md)), so there is no vendor evidence for what they do. PHY reset/power is
  a guess that is no longer needed: all 32 PHYs answer once MDIO timing is right ([extphy.md](extphy.md)).
  Unchanged across cold power cycles and the vendor diag's (failed) `init all`.
- CPLD1 0x09-0x0b, 0x15, 0x20-0x25, 0x40, 0x42-0x48: the factory table gives their expected values (equal to
  what we read) but no meaning. QSFP low-power mode: not found.

Nothing has been written to any CPLD register from NixOS.
