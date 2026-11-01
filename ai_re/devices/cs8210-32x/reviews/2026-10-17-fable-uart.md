# fable focused review: early console

Requested by the user, completed through Claude Code with `claude-fable-5-1`.
Review-only: source excerpts supplied; no tools or project edits. Original
response follows. Full prompt and result are retained in ignored
`analysis/research/fable-uart-review-{brief.txt,result.json}`.

**Verdict: hypothesis confirmed, with one refinement.**

**Code trace.** The kernel splits bootargs on spaces, so the earlycon handler receives `uart8250,mmio32,0xfd883000,115200n8`. In `parse_options` (earlycon.c:125-127), baud becomes 115200 and `strchr` finds no further comma, so `uartclk` stays 0. `register_earlycon` (line 148-149) then forces `BASE_BAUD * 16` = 1843200. Nothing on this path touches the device tree. `init_port` (8250_early.c:143-149) computes divisor 1 and writes it. With a real 500 MHz reference that is 31.25 Mbaud. Garbage output follows immediately.

**Mistake in the reasoning.** It frames the loss as "a hang before the real driver takes over." The more likely failure is misdiagnosis: the board boots fine but the console shows garbage or silence until `8250_of` reprograms the divisor, and the reviewer reads that gap as a hang. Also, `keep_bootcon` keeps the broken early console registered, so both consoles write to the same port.

**Severity.** High for a first boot. No crash or corruption, but the one window where diagnostics matter most produces nothing readable.

**Preferred correction.** Drop the baud suffix: `earlycon=uart8250,mmio32,0xfd883000`. With `baud == 0`, `early_serial8250_setup` only masks interrupts and leaves the firmware divisor intact. This does not depend on the 500 MHz figure being the true UART reference, which is still unverified on hardware.

Appending `,500000000` works per line 127 (divisor 271, 0.1% error) but bets on the clock value. Use it only after a readable boot confirms the clock.

## coordinator clarification

The regular serial driver can restore the hardware divisor. `keep_bootcon` does
not itself preserve the wrong divisor after that restoration; it can leave
duplicate console output. The confirmed defect is the initial explicit-baud
earlycon setup, which is enough to lose first-boot diagnostics.

The reviewed candidate has not been changed by this review. The recommended
correction remains outstanding.
