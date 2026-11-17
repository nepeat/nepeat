# CS8210-32X: our U-Boot into the boot SPI (scheme A'): flasher, safety case, plan

Status 2026-11-16: **built, verified offline, rehearsed; nothing written to SPI.** The first write
waits for the parent's go (gates below). Scheme and evidence of the boot chain:
[boot-chain.md](boot-chain.md) §5/§7; our U-Boot: [u-boot-port.md](u-boot-port.md); escape and
recovery routes: [uart-recovery.md](uart-recovery.md). DMC-12 original of this design:
[../dmc-12-48r-as0/spi-upgrade-plan.md](../dmc-12-48r-as0/spi-upgrade-plan.md).

## Layout (what is ever written)
| region | bytes | content | written by |
| --- | --- | --- | --- |
| 0x020000 +0x1000 | 0x198 | **TOC2**: the 9 factory entries byte for byte (incl. STG2 @0, which stage3 uses for the instance), + BOOT_MODE, IPXE 0, IPXE 1 | install (last); rollback erases it |
| 0x030000 +0x1000 | 0x8c | **BOOT_MODE** object: 0x40 B data, word +4 = app (0x110 = slot A, 0x10000110 = slot B) | install, update/switch (flip) |
| 0x038000 +0x8000 | — | our U-Boot's redundant env (0x38000/0x3c000, standalone build): erased by install (default env on first boot) and `env-reset`; never compared | install, env-reset |
| 0x0f0000 +0xa0000 | ≤ 640 KiB | **slot A** = IPXE 0: our U-Boot as an al-boot object, id 0x110, load = exec 0x08000000 | install, update |
| 0x280000 +0x100000 | ≤ 1 MiB | **slot B** = IPXE 1 (inactive until an update writes it) | update |

Untouched: stage2, the factory TOC at 0x40000 (TOC2 shadows it), DT/SerDes FW, STG3, **UBOOT 0 at
0x190000..0x27808c** (the `u`-key escape and the only U-Boot with full `sf`), the factory env
0x380000, ONIE. The payload must fit slot A (640 KiB; today's `#uboot-cs8210-stage3app` is 0x66a5c B).
- **Why IPXE 0/1:** stage3 looks up `BOOT_MODE word | stage2-instance << 28` (instance 0 here), so the
  instance bits in the word pick the slot; the TOC lookup compares the full id (HAL
  `al_flash_toc_find_id_with_fallback`, fallback = the same id). No automatic A/B fallback: a bad
  slot hangs until `u` (below). IPXE is otherwise unused (stage3 reads its header for a version only).
- **Slot B over 0x280000-0x380000** (parent's question, 2026-11-16): the old 412 KiB hole at
  0x2f9000 left 1444 B for the standalone build. The new range starts past UBOOT 0's data end
  (0x27808c; 0x7f74 B gap) and ends at the factory env (0x380000); it covers only orphans of the
  reference image that no TOC lists (kept in images/spi-2026-11-15). It is inside UBOOT 0's
  declared TOC max (0x1f0000): only a factory `flash_contents_obj_update UBOOT` would erase it
  (never used). `check_layout()` refuses any window that overlaps UBOOT 0's data, a factory object,
  0x380000+ or another window; the installer carries the same table (tests.nix compares them).
- Linux is neither a writer nor a rollback path: mtd0 flags 0x800 (no MTD_WRITEABLE).

## Tools
- Builder: [../../nix/packages/alboot/alboot3.py](../../nix/packages/alboot/alboot3.py) (V3 TOC, object
  and BOOT_MODE builders, every-TOC lister: the boot-chain.md §10 gaps; alboot.py itself is unchanged
  because the DMC-12 drvPaths hash it). CS8210 layout and plans: [scripts/spi_upgrade.py](scripts/spi_upgrade.py)
  `build` (exact region bytes + mtd0-after.bin into images/spi-upgrade/<plan>/, git-ignored). A
  `.fimg` payload is used as is after checks; wrapping its u-boot.bin here gives identical bytes.
- Writer: **`#uboot-cs8210-installer`** (patches 0027 + 0028): the board build cut down to console,
  SPI, loadx, crc32, cmp; prompt `cs8210-installer> `, no autoboot; SPI read-only except the five
  windows above (MTD hooks and the spi-mem opcode filter, 4-byte opcodes only); `sf erase/write`.
  Started like milestone 2: vendor prompt → `loadx 0x08000000` → `booti 0x08000000 - ${fdtaddr}`.
- Emulator: [scripts/bootchain_emu.py](scripts/bootchain_emu.py) runs the factory stage2 (C entry, real
  strap word, TOC search/validation, DT and STG3 load, measured boot) and stage3 (board init + app
  loader: BOOT_MODE, key poll through a modelled UART, app lookup, header/data sums, load) in Unicorn
  on a 32 MiB image; hardware calls are stubbed, each with its reason in the file. Cross-checks: the
  factory image gives the boot log's messages and loads UBOOT 0 (950336 B) to 0x01100000; `p` gives
  "User request for skipping APCEA!" as on the box (11-15); the measured-boot digests of STG3 and the
  env equal sha256 of the dumped bytes.

## Gates (`spi-upgrade-gates.json`; `--write` refuses while one is open; overrides are journaled)
1. `review`: two independent reviews of tool + plan, findings fixed (PROGRESS entry).
2. `payload`: the payload is the build the owner wants in SPI, RAM-tested as stage3 would start it.
3. `recovery`: a 1.8 V programmer + clip plan is at hand, or the owner accepts that a state no key
   escapes (e.g. a damaged stage2/STG3/UBOOT 0, never written here) needs one.
4. `owner_go`: the parent's go for the first write.
5. automatic: a passing `verify` record of the exact plan sha (verify plans from the model's current state:
   install, or with TOC2 live the update/switch/rollback) and a passing sandbox `rehearse` of the
   payload ([spi-upgrade-journal.jsonl](spi-upgrade-journal.jsonl)). Gate values must be {date, by, evidence};
   a `start` record (plan, payload, installer sha256, overrides) is journaled before the first write.
   The installer's `.config` windows must equal the tool's table.

## Install sequence (`sudo spi_upgrade.py install --payload F --installer DIR --ssh 'sudo -n ssh' --write`)
1. ASIC lease; NixOS `/dev/mtd0ro` == model (the verified dump a22ff482… + journaled writes, our env excluded).
2. `systemctl reboot` (a power cycle when NixOS does not answer; then no lease and no mtd0ro check);
   `u` every 20 ms from stage2's banner until stage3's poll is past (always: harmless with app 5);
   `123` at the vendor autoboot; `version` = 2018.05. The consoled attach replays its scrollback: nothing before
   our take's answer counts, and the boot must show a fresh `stage2_loader`.
3. loadx + crc32 the installer; booti; banner `Entry: EL2`; `version` = `2026.10-cs8210-installer`.
4. `sf probe 0:0` (mx25u25635f, 32 MiB); `sf read` 0..0x380000; crc32 of every piece == model; **stop on any difference**.
5. One XMODEM upload of the objects (4 KiB-aligned from 0x04400000), crc32 per object.
6. Per region, in order **env-erase, app, BOOT_MODE, TOC2** (regions already holding their target
   bytes are skipped, so a crashed install resumes; our env is always erased): journal intent →
   [**kill**, if the model shows an acceptable header or the range is unknown: program the first
   0x100 B to 0 and read them back, so no partial erase can leave a valid header over damaged data]
   → `sf erase` → read-back blank → `sf write` body (data + sum, or the TOC entries + sum) → read-back
   (crc32 + `cmp.b`) → `sf write` header bytes 4.. (magic still blank) → read-back → `sf write` the
   4-byte **magic last** → read-back (crc32 + `cmp.b`) → journal done. A header can only validate once
   its magic is complete over bytes already read back: a torn header write cannot validate.
   Every destructive line is typed, its echo compared character for character, then Enter (else
   Ctrl-C). Never a bare Enter at the installer (U-Boot repeats the last command): idle checks are `echo <tag>`.
7. Full re-read == the new model; power cycle; expect `Loading application to 08000000` and our banner.
8. Then: `u` escape test (power cycle, `u` from stage2's banner → factory U-Boot), NixOS boot.

Dry run (no `--write`): steps 1-5 and the printed commands, then a power cycle back to the normal
boot. `update` = the inactive slot, then the BOOT_MODE flip; `switch` = the flip only;
`rollback` = erase 0x20000 (gate-free; with our app live the vendor prompt comes through `u`);
`env-reset` = erase our env.

## What a power cut or a failure leaves (emulated, evidence below)
- Until the TOC2 header is complete, stage2/stage3 use the factory TOC: **factory boot**; TOC2
  complete → our app. Every cut state of install, update and rollback boots factory or ours, never
  hangs; with `u` every install state boots the factory U-Boot.
- Traps the order avoids (emulated to show why): BOOT_MODE with a valid header and bad data hangs
  **even with `u`** (stage3 skips its key poll); an app with a valid header and bad data, or BOOT_MODE
  pointing at a never-written slot, hangs but `u` gets out; BOOT_MODE data > 0x40 B → factory.
- A BOOT_MODE or TOC2 step that fails after one of its commands went out is killed and erased
  again at once (any exception, unless the console stopped answering: then nothing more is typed,
  since keys could abort a running erase). After a failed destructive step the tool does **not**
  power-cycle: the installer stays at its prompt; a power cycle happens only after clean runs or
  failures before the first destructive command (and the plug is switched back on even after an
  interrupt). consoled notices are dropped from the matched text; losing write stops the run.
- Torn headers (review B): byte sums let a tear that leaves e.g. one size bit and one sum bit
  unprogrammed still validate (BOOT_MODE size 0x80000040, TOC2 0x0100000c entries); stage2's
  measured boot then loads/reads by that size before stage3's key poll: a hang no key can escape.
  Hence the magic-last order above (a torn magic never equals 0x000b9ec7/0x070c070c) and the kill
  page before erasing a live header.
- Our app hangs at cold boot: power cycle with `u` → factory U-Boot → `rollback --write`.
- Not recoverable by this tool: a broken stage2/STG3/UBOOT 0 (never written here) → ROM UART
  (stage2 only) or a 1.8 V programmer (uart-recovery.md).

## Evidence (2026-11-16)
- `verify` with `#uboot-cs8210-stage3app` (install plan d2dd7fce51c49bfa): 213/213 flash states as
  expected (install, update, switch back, rollback, re-install: every cut state incl. kill-page and
  partial-erase cuts, with and without `u`; the traps; SerDes FW loads under TOC2 = the factory's):
  [spi-upgrade-verify-20261116.txt](spi-upgrade-verify-20261116.txt). From an installed model: 125/125.
- `rehearse` on the sandbox twin with the real 32 MiB dump as its flash file: the dry run leaves the
  file unchanged; install (real XMODEM), update, switch back and rollback each read back == model,
  file == the plan's image, emulated boot = ours A / ours B / ours A / factory; 11 out-of-window or
  forbidden commands refused, file unchanged; killed after each of the 32 destructive commands of
  those four runs: every state boots factory or ours, `u` → factory:
  [spi-upgrade-rehearse-20261116.txt](spi-upgrade-rehearse-20261116.txt).
- `#uboot-alpine-v3-tests` installer-window: installer and sandbox configs carry the same windows;
  36 sandbox erase/program/raw-op attempts; random flash bytes change only inside the windows.
- Hardware, read-only: NixOS /dev/mtd0ro == model (sha256 a22ff482…, 17:04). Final-code dry run 18:38-18:41
  (serial-spiupgrade-dryrun3-20261116.txt): same path with `u` from stage2's banner (factory app 5: no
  "User request", vendor prompt reached), installer sha256 8128d2b1…, flash == model, upload ok, 13 writes
  printed only, power cycle, NixOS + SONiC (BGP 2/2, BFD 2/2), lease free. First dry run 17:07-17:10 (plan d2dd7fce…, lease spi-flasher): ssh reboot → `123` → vendor `loadx` of the installer (crc ok) → `booti` → `cs8210-installer>` at EL2 → `sf probe` mx25u25635f 32 MiB → 0..0x380000 == model → upload 0x68198 B (3332 blocks, 0 retries, 46.8 s, crc32 per object ok) → the 10 writes printed only → power cycle → NixOS, cage guard ok, BGP up (journal record 17:27; transcript serial-spiupgrade-dryrun2-20261116.txt, git-ignored). Found on the way and fixed: the attach scrollback replay matched an old `Type 123` (now: nothing counts before the take answer, and a fresh `stage2_loader` is required); a killed `sudo ssh` left the attach holding write (now: C-b d + process-group kill); `lab-power` takes `--entity`.
- Not proven: our app as stage3's app on hardware (gate `payload`); `u` with an app ≠ 5 on hardware
  (`p` proven, same code path); erase/program by the DW SSI on the real chip (sandbox only).

## Reviews (2026-11-16, two fresh agents, not told the conclusions)
- A (host logic): erase of a live header sector could leave "valid header, bad data" → **kill page
  before erase**; power cycle after a failed write → **none after a destructive step**, recovery on
  any exception; rollback needed NixOS → optional; rollback after a crash in TOC2 → always `u`;
  install not resumable / update unverifiable → skip-equal regions, verify plans the current state,
  rehearse keyed by payload; torn journal line → repaired on append; consoled notices → filtered,
  losing write stops; loose gates → {date, by, evidence}, `start` record, installer window check;
  typing into a running erase → `Hung`, nothing more typed; small ones (xmodem read-ahead, lease
  release, docstring). All fixed.
- B (boot chain): **torn header write can validate** (byte sums) with a huge size and hang stage2's
  measured boot → **magic written last**; rollback refused after an interrupted update → exempt;
  verify after install → current-state planning; stubbed SerDes init reads TOC2 → its FW lookups and
  loads now run for real (identical under TOC2); `mw` unchecked → echo-checked; a `recovery`
  gate added. Open from B: the emulator forces stage2's measurement path (the real TPM flow could
  skip it: no effect on the verdicts, which only get safer); `--expect-banner` cannot tell slot A
  from B (`slots`/crc check of the running copy not ported from the DMC-12 tool).

## Notes for the owner
- Under our U-Boot the ASIC's PCI COMMAND is 0 at handoff (the vendor's 6 came from its AVS step);
  the AVS oneshot enables the device via sysfs first (standalone agent; not yet run on the chip).
- Measured-boot PCR 0 changes (two more objects measured); nothing seals to it on NixOS.
- Never `saveenv`/`env default` at the vendor prompt; never type `sf` at `ALPINE_DB>`.
