# I2C FPGA on hardware: first live tests (2026-11-10, empty cages)

All 32 cages empty (BAR0 `0x140` = 0). Run from sea420-desktop over ssh with
`cs8210-qsfp` (allowlisted 32-bit BAR0 accesses only) and
`fpga-nack-timing.py` (uses the same `Dev` class and allowlist). The serial
console showed no kernel errors during the runs. Raw output is in
`2026-11-10-live-{1..4}.txt`.

| # | what | result | vs. layer 7 / simulation |
|---|---|---|---|
| 1 | `--enable --channels`, read only apart from PCI `enable` | version `0x010b0009`; present 0, reset `0xffffffff`, lpmode 0, int 0, int_mask `0xffffffff`; every channel `+0x00` = `0x00081766`, `+0x08` = 0, `+0x0C` = 0, `+0x54` = 0, `+0x60` = 0 | power-up codes 0x66/0x17/0x08 **match** the layer-7 prediction |
| 2 | `--init 1` (vendor probe values) | `+0x1000` reads back `0x00660918`; `+0x100c` reads back **0** after writing `0x80000000` | bit 31 self-clearing soft reset **matches** layer 7. The old `cs8210-qsfp` check expected 0x80000000 and refused; fixed |
| 3 | `--force --read 1 0x50 0 1` | busy clear at the first 1 ms poll, status `0x00000002` → tool reports NACK (-ENXIO) | **matches** simulation (bit 1 = slot 0 NACK) |
| 4a | init all 32, 1-byte read at 0x50 on each port | 32/32: status `0x00000002`, busy clear | every engine answers at its own page. The crosstalk check is weak: statuses stay 2 from earlier runs and `0x60` is not writable in the allowlist |
| 4b | port 1 busy time from the start write until bit 31 clears, 300 runs per profile | sel 0 (0x18): 36.7 µs, sel 1 (0x09): 19.6 µs, sel 2 (0x66): 125.3 µs (medians; one poll read ≈ 2.9 µs) | linear in the per-bit time. With the layer-7 bit model 6(code+1)+13 clocks at 62.5 MHz it fits **11.8 bit-times + 5.8 µs overhead** (residuals ≤ 0.03 µs), about START + 8 address bits + ACK + STOP. The naive 4(code+1) model would need 17.8 bit-times, too many for an address NACK. So hardware supports 386 kHz / 868 kHz / 99 kHz (I: SCL itself was not probed) |

Not tested: anything with a module present (no cables yet), clock
stretching, data transfers, presence interrupt/MSI.
