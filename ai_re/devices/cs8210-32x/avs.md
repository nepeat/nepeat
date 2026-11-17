# CS8210-32X: ASIC core voltage (AVS)

The vendor U-Boot sets the Teralynx 7 core rail from the ASIC's fuses on every boot
([vendor-uboot.md](vendor-uboot.md) item 5). Since 2026-11-16 the same step exists as a Linux oneshot,
`cs8210-asic-avs.service` (`nepeat/nix` `nixos/module/cs8210/avs`, option `hardware.cs8210.asicAvs`,
default on), so a U-Boot that skips it (ours) still gives the SDK the right voltage.
Chip test: [avs-chip-20261116.txt](avs-chip-20261116.txt) (sha256 `b273260f…8225b`).

## What the vendor does (main_loop hook 0x01104600, from the disassembly)

1. Finds PCI 1d98:1b58, COMMAND = 6 (memory + bus master; checks both bits), maps BAR0.
2. Reads four 32-bit words at **BAR0 + 0x11178** (prints `vid[0..3]`). 128-bit value, word 0 = bits 0-31.
3. **Lot ID**: bits 0..41, bit i -> bit (6 - i%7) of character i/7: six 7-bit ASCII characters.
   **RoV** = (word 2 >> 13) & 0xf.
4. **Target** (jump table 0x011a47d4): RoV 2 -> 820 mV, 5 -> 840, 8 -> 780, 10 -> 760, RoV 0 -> 780 if the lot is one of
   PNHB06..PNHB11, PNKW27, PPHN57 (strncmp 6, strings at 0x011aad98), else 800; every other RoV -> 800.
5. **Closest code**, table at 0x011a0670 (code, mV): (0x030a, 760) (0x031f, 780) (0x0333, 800) (0x0348, 820)
   (0x035c, 840) = linear16 exponent -10, code = mV x 1.024. For a target between two entries the lower one wins
   unless the upper is strictly closer (790 -> 780). The targets above are always exact entries.
6. Bus 3, mux 0x74 = ch0, 0x72 = ch2; PMBus to 0x62: write PAGE (0x00) = 0, udelay(5), write VOUT_COMMAND (0x21)
   = code (word, little endian). No VOUT_MODE check, no read-back, no retry; failures only print.

**This unit** (every vendor boot log, e.g. serial-20261115-boot-7.3.4.txt): `64bdb285 10602a70 00200644 10200000`
-> RoV 0, lot **PS7R09**, **800 mV, 0x0333**. Our BAR read on 2026-11-16 gave the same four words.

## The regulator (0x62 behind 0x70:2 -> 0x74:0 -> 0x72:2; vendor DT: `ti,tps53688` "vddcore")

Read 2026-11-16 under SONiC (PAGE 0): IC_DEVICE_ID `TISh 80 00`, PMBUS_REVISION 0x33, VOUT_MODE 0x16
(linear, exp -10), VOUT_COMMAND 0x0333 (799.8 mV), READ_VOUT 0x0333-0x0334, VOUT_MAX 0x03cd (950 mV),
VOUT_OV_FAULT_LIMIT 0x04dc (1215 mV), VOUT_UV_FAULT_LIMIT 0, TRANSITION_RATE 10 mV/us, ON_OFF_CONFIG 0x17 (enable
pin only; OPERATION reads 0x00 and is ignored), READ_IOUT ~146 A, READ_TEMPERATURE_1 ~42.5 C, READ_VIN 12.0 V,
READ_POUT ~117 W, STATUS_WORD 0x1003. VOUT_CAL_OFFSET (0x23) NACKs. Linux has no driver bound (no DT node).

**Power-on default: not readable without a cold boot.** Standard PMBus only exposes the operating
VOUT_COMMAND (already rewritten by U-Boot before its prompt); VOUT_MAX / limits say nothing about the boot value;
the TPS53688 NVM/VBOOT registers are in TI's non-public register map, and RESTORE_USER_ALL would apply the NVM value
(a write: not done). The vendor U-Boot prints only the target. Ways to learn it: a power cycle with the rail read
before the vendor U-Boot's main_loop (e.g. our U-Boot chainloaded from the boot ROM UART path, or stage3), or TI's
register map. Hazard 4 of vendor-uboot.md (a replacement without AVS runs at this unknown value) stays open.

## Our oneshot (`cs8210-asic-avs apply`)

- Order at boot: `cs8210-pcie-errors` -> **cs8210-asic-avs** -> `modprobe ipd` (its ExecStartPost) -> syncd /
  cs8210-switch (both `Requires=` + `After=` it: a failed AVS keeps both from starting, and ipd stays unloaded).
  ipd is blacklisted for udev's alias autoload: with `IO_STRICT_DEVMEM` a sysfs `resource0` mmap fails (EINVAL) while
  ipd holds BAR0 (checked 2026-11-16). ipd's probe does not touch the chip beyond config space and its BAR map; the
  SDK's `IPD_HW_INIT` ioctl does the chip init.
- Fuse read: root port must have the AMBA error response off (0x8d0 = 0) and the link up; COMMAND memory bit (sysfs
  `enable` if off; never /dev/mem); one 32-bit load per word through a 4 KiB mmap of resource0 at 0x11000; any
  all-ones word = refuse. Words are kept in `/var/lib/cs8210-asic-avs/fuses` (warning if a later read differs).
- With ipd already bound (deploy onto a running box, a manual rerun) the words come from `/run/cs8210-avs.done` or that
  file instead; with neither it refuses.
- Decode exactly as above; refuse outside 760..840 mV or a code not in the table.
- PMBus (i2c-dev on the mux's `channel-2` link, mux found by its DT node; plain I2C_SLAVE, so a kernel driver on 0x62
  = EBUSY = refuse): IC_DEVICE_ID must be `TISh 80 00`; PAGE written to 0 only if it is not 0; VOUT_MODE must be 0x16;
  code must be <= VOUT_MAX; **VOUT_COMMAND written only if it differs**, then read back, READ_VOUT within 16 codes
  (~16 mV) within 1 s, STATUS_WORD VOUT bit clear. Only whitelisted commands reach the bus (no STORE/RESTORE/
  CLEAR_FAULTS/MFR). Old/new values in the journal and in `/run/cs8210-avs.done` (key=value).
- `cs8210-asic-avs status` (readings + state), `metrics` (node exporter producer `cs8210-asic-avs`, 30 s:
  `cs8210_asic_avs_{applied,target_volts,rov,wrote,info,pmbus_ok,vout_command_volts,vout_command_code,read_vout_volts,
  read_iout_amperes,read_temperature_celsius,read_vin_volts,read_pout_watts,status_word}`), `decode W0 W1 W2 W3`.
  `apply --dry-run` writes nothing; `--rewrite` writes VOUT_COMMAND even when equal (same value, write-path test).
- Unit tests at build (`avs/test_avs.py`): this unit's vector, a literal transliteration of the vendor bit loop
  against 2000 random words, every RoV x lot branch, the closest-code rounding, the limits, linear11/16, the whitelist.
- `systemctl restart cs8210-asic-avs` also restarts syncd / cs8210-switch (Requires=); deploys leave it alone
  (`restartIfChanged = false`).

## Chip test 2026-11-16 ([evidence](avs-chip-20261116.txt))

- With SONiC up (ipd bound): late path without records refused as designed; `--dry-run --fuse-words` (the logged
  words) decided "already correct".
- Boot path by hand (sonic.target stopped, `rmmod ipd`, COMMAND 0x0002 so no enable write needed): BAR read
  = the U-Boot words, RoV 0 / PS7R09 / 800 mV, **"already correct; no write"**; `--rewrite` wrote 0x0333 over 0x0333,
  read back 0x0333, READ_VOUT 0x0333, STATUS_WORD 0x1003 unchanged; `modprobe ipd`; late-path dry run from the state
  file OK; `systemctl start sonic.target`: both VRF BGP sessions and BFD up within 90 s, no failed units.
- Seeded for the deploy: `/run/cs8210-avs.done` and `/var/lib/cs8210-asic-avs/fuses` on the box.
- Not exercised on the chip: the sysfs `enable` write (decode was on), a VOUT_COMMAND change (forbidden).
- STATUS_CML reads 0x21 (bit 5 PEC failed, bit 0 other memory/logic) with STATUS_WORD 0x1003 (CML, MFR); most likely
  latched by the first survey (a NACKed 0x23 read, block reads past the count). Left latched (CLEAR_FAULTS not sent);
  regulation unaffected (READ_VOUT steady).
- Before the boot-path test, syncd was OOM-killed at 16:05:48 (5.5 GB RSS, 35 GB VM; the loop's BFD/LACP had expired
  at 16:00:56) during read-only work only (PMBus reads, root-port config reads, one refused resource0 mmap at
  15:58); no causal link found. Same shape as the vendor stack's learn-thread 32 GiB allocation but without its kernel
  line, so the ifcs watchdog did not fire. PROGRESS entry 2026-11-16 (AVS).
