# SPI U-Boot upgrade: spec for `spi-upgrade.py` (written 2026-11-15, not run against flash yet)

Goal: put our U-Boot in the boot SPI flash of a DMC-12 so stage3 boots it directly, keeping the
factory U-Boot as a key-press fallback. One command per unit, safe for a human at a terminal.
Background and evidence: `spi-uboot-replacement.md` (format, flashing concept §7), `spi-recovery.md`
(ladder), `uart-boot.md` (Route A, stage3 keys), `u-boot-port.md` (our U-Boot), `board-notes.md`.

## Design (what gets written, and what never)
- Write only **IPXE instance N** (our U-Boot, object 0x110, load = exec 0x08000000, 512 KiB slot at
  0x2c0000 / 0x580000) and **BOOT_MODE** (first 4 KiB sector of its 64 KiB slot at 0x340000, data
  word +4 = app ID: 0x110 = IPXE 0, 0x10000110 = IPXE 1). Never STG2, STG3, DT, TOC, UBOOT, the env sectors or EEPROM objects.
- Payload: `#uboot-dmc12-stage3app` `u-boot-ipxe-08000000.fimg`, already wrapped and `alboot.py check`ed.
  BOOT_MODE: `uartboot.obj()` with **0x40 B** of data (word +4 = app, rest 0), i.e. the factory's
  0x8c-byte object; stage3 copies up to 0x40 B and reads word +4 only (`uart-boot.md`). Route A ran
  the 8-byte form; the rehearsal runs the 0x40 form, so the exact bytes are tested.
- **Writes use plain `sf erase` + `sf write`** (whole target slot / sector), not
  `flash_contents_obj_update`: that command writes the bytes as given with no verify anyway
  (`spi-uboot-replacement.md` §7), and sf semantics are known. `flash_contents_obj_validate` still has
  to accept each object (UBOOT 0 is run first as a control).
- **Host model:** expected CS0 0..0xfd0000 = verified `mtd0.bin` + every write journaled in
  `inventory/<unit>/spi-upgrade.jsonl` (bytes kept in `dumps/<unit>/spi-writes/`). Each full read must
  equal it, so a torn or foreign write stops the script.
- Escape: holding `u` during stage3 boots the untouched factory UBOOT 0 [hw: Route A exp 3]. From there
  the factory env still chainloads our U-Boot from raw sda1 (`reimage.md`), so a `u` boot still reaches
  NixOS. Rollback = erase the BOOT_MODE sector → stage3 boots factory UBOOT again.
- Updates: write the *inactive* IPXE instance, validate, then flip BOOT_MODE. The active copy is never
  overwritten in place.

## Preconditions (script refuses without them)
1. Unit known: serial in `units-private.txt` matches the board (`printenv bootp_dci`); original state
   archived and verified (`dumps/<unit>/manifest.jsonl`: sda, mtd0, mtd1) — as `reimage.py`.
2. **External recovery proven for this hardware revision**: one clip read of CS0 with the box
   unpowered equals the backup (record in `units.md`: date, programmer, 1.8 V, package). Not per unit.
3. Unit state checks (read-only, factory prompt): OTP secure boot disabled (`otp show direct`), a
   fresh CS0 read equals the model (so the model's TOC, checked for unknown object IDs, is the
   flash's), BOOT_MODE blank (install) or selecting IPXE (update).
4. **Dress rehearsal on this unit, zero writes:** Route A (`uartboot.py route-a`) with a RAM image built
   from *the exact payload bytes* boots our U-Boot to its prompt, and `stage3keys.py --key u` (or Route A
   `--key u`) shows the escape. Results logged.
5. Our U-Boot gaps closed or accepted by the owner: GPIO 22 drive (only the factory U-Boot sets it), a
   PCIe start with no factory init at all (untested; `u` covers it).
6. Independent review of the patch series + this script done (AS7760 rule).

Gates 2, 5, 6 live in `spi-upgrade-gates.json` (null = open; `--override-gate NAME=REASON` is
recorded); gate 4 is a `rehearsal` record of the same payload + BOOT_MODE hashes in the unit's journal.

## Steps (each prints what it does; destructive ones ask for the unit id unless `--yes`)
| # | step | where | proven by |
|---|---|---|---|
| 1 | power-cycle, stop factory autoboot (`powercycle_to_uboot.py`) | factory | [hw] |
| 2 | identify + env backup (`printenv` → `inventory/<unit>/…-private.txt`) | factory | [hw] reimage.py |
| 3 | `sf probe 0`; `sf read` all CS0; `crc32` == current-state record (first time: == mtd0 backup); **stop on any difference** | factory | [hw] reads; note `sf probe` rewrites SR=0 (already 0) |
| 4 | TFTP the `.fimg` and the BOOT_MODE object to RAM; `crc32` each vs host | factory | [hw] TFTP |
| 5 | `sf erase` the whole IPXE n slot, `sf write` data+sum then header; `sf read` the slot, `crc32` == model; `flash_contents_obj_validate IPXE n` | factory | [code] |
| 6 | `sf erase 0x340000 0x1000`, `sf write` data+sum then header; read back + `crc32` (no factory validate: BOOT_MODE load = 0) | factory | [code] |
| 7 | re-read all CS0; diff vs step 3 must be **only** the IPXE slot + BOOT_MODE sector | factory | – |
| 8 | power-cycle, no keys; expect `U-Boot 2026.10-dmc12-…` and `Entry: EL2 … x0=0x0` | stage3 → ours | [hw] Route A exp 4 |
| 9 | boot NixOS (`bootflow scan -lb`), wait for 48 ports, `smoke.py` | ours | [hw] |
| 10 | rehearse the escape: power-cycle with `u` (`stage3keys.py`), expect factory `U-Boot 2015.07`; then boot normally again | factory | [hw] |
| 11 | record `inventory/<unit>/spi-upgrade.jsonl` (hashes, slot, BOOT_MODE app, timings) | host | – |

`--rollback`: at the factory prompt (via `u` if ours is broken) erase the BOOT_MODE sector (`sf erase
0x340000 0x1000`), verify it reads 0xff, power-cycle, expect the factory U-Boot. `update`: steps 1–7 on
the inactive instance, flip BOOT_MODE, 8–11. Without `--write` (default) every read-only step runs and
the write commands are printed; `status` and `rehearse` never write SPI.

## Failure handling
- Any CRC/validate mismatch before step 6: stop; nothing boots IPXE yet, the write is harmless.
- **A good BOOT_MODE header with bad data hangs stage3 after its key poll** ("al_flash_obj_data_load
  failed (boot mode)!" → the TOC-failed path; review 2026-11-15, stage3 disassembly): `u` can't help, only
  the clip. Hence data first, header last (a torn write leaves a bad header → app 5), and on a BOOT_MODE
  read-back mismatch the script re-erases the sector at once, still at the factory prompt.
- Every destructive command is typed, its echo compared exactly, then sent (Ctrl-C otherwise); only the
  three regions IPXE 0/1 and the BOOT_MODE sector are accepted. Read-back mismatch: **don't power off**;
  the running factory U-Boot can restore from the backup (`spi-recovery.md`, `sf update`).
- Journal: `intent` (with the region's expected bytes) before, `done` after a matching read-back. An
  intent without done is an unknown range: left out of comparisons, so `rollback` (gate-free) still runs.
- Step 8 no banner / hang: power-cycle with `u` → factory → `--rollback`.
- `u` not working either (stage3 itself broken: impossible from these writes, but): clip + backup
  (`spi-recovery.md` ladder).
- Never `saveenv`, `env default`, `delenv` at any prompt (BOOT_APP reroute script, `board-notes.md`).

- After install, IPXE 1 is blank: if stage2 instance 1 ever boots (ladder rung 4), stage3 looks for
  IPXE 1 → bad object; `u` still reaches the factory U-Boot.

## Reuse
Console + prompt handling: `console.py`; power: `tools/lab_power.py`; stage3 keys: `stage3keys.py`
logic; payload/BOOT_MODE builder: `uartboot.py` `obj()` + `alboot.py`; smoke: `smoke.py`. Same shape as
`reimage.py` (steps, `--from`, `--json`, `--yes`, gates), so the two can share a module later.
