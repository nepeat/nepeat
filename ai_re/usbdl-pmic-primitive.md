
## `pmic_config_interface` internals — prep for the probe

`VA 0x2223f0`, signature `(addr, val, mask, shift)`. It is a **read-modify-write**,
not a blind write:

```
0x00222404  bl #0x221ac          ; pmic_read(addr, &cur)
0x0022240c  cbz r0 -> continue   ; read failure -> log and return
0x0022241a  lsl.w r7, r7, sb     ; mask <<= shift
0x0022241e  lsl.w r5, r5, sb     ; val  <<= shift
0x00222422  bic.w r1, r1, r7     ; cur &= ~mask
0x00222428  orrs r5, r1          ; cur |= val
0x0022242c  bl #0x221ba          ; pmic_write(addr, cur)
```

`usbdl_pwr_write16` passes `mask = 0xffff, shift = 0`, so a USBDL write replaces
the full 16-bit register.

Two things this establishes for the staged plan:

* **`pmic_read` (`0x221ac`) exists as a separate primitive**, and is what cmd
  `0xC6` (`PWR_READ16`) drives — so step 1 really is a pure read with no
  write-back.
* Because the write path is read-modify-write, a step-2 "write current value
  back" is a genuine no-op at the hardware level.

**Still needed before step 1:** a known-safe MT6358 register with a documented
expected value (the chip/SW ID). The preloader logs
`shutdown_pmic_time … hw_id 0x%x sw_id 0x%x` (`0x2f422`), but the register
addresses are computed rather than immediates — only one `pmic_read` call site
uses a literal (`VA 0x00223c96`, `r0 = 1`). Resolving the ID register properly
means either tracing that table or taking the address from MT6358 documentation.
**Do not guess an address for the first probe.**
