# board-dmc12 — DMC-12-48R-AS0 (Annapurna Alpine v2, 48-port console server)

**Verbatim import** of [codeberg dma/dmc12-nixos](https://codeberg.org/dma/dmc12-nixos)
at `5d02d1b3de04082aea3960cd74db0c24f5b3ed32` (2026-07-25, upstream HEAD on
2026-11-15). Every tracked file except the standalone `flake.nix`/`flake.lock`
and the unrelated `.gitignore` was copied unchanged (sha256 checked). Later
commits change it in place, so `git log -p -- .` shows each change against
upstream.

Exported as `nixosModules.board-dmc12`. It evaluates for aarch64 when added to
a bare system. It has no `configs/` entry yet and has not been built or booted
from this tree.

Device notes, dumps and the bring-up checklist: `devices/dmc-12-48r-as0/`
(not readable from here by rule).

## Known gaps vs the shared-tree rules (to fix, not yet done)

- The kernel, DTS and `al_eth` are built inside the board module. They belong
  in `packages/` (e.g. `linux-alpine`, `al-eth`) and the SoC parts belong in a
  `platforms/alpine` module. The monorepo's `nix/nixos/module/cs8210` (Alpine v3)
  carries a modified copy of the same `al_eth` and bootmenu builder. Merge the
  two instead of keeping both.
- Doesn't import `profile-appliance`. It tracks `linuxPackages_latest`
  (unpinned), and pulls files in with `readFile`/`./path` rather than
  `builtins.path`.
- Hardware assumptions (I2C bus 4 = CPLD, EEPROM at `7-0051`, sensors at
  0x48/0x49/0x4a, the USB hub topology behind P1–P48) come from the factory
  image. This tree hasn't re-checked them against a live board.
- `kernel/dts/amazon/alpine-v2-dmc12.dts` is a trimmed mainline rewrite. The
  factory kernel's live DT (dumped 2026-11-15, kept with the device) also has
  the SPI NOR controller (`snps,dw-apb-ssi`), an `arm,sp805` watchdog,
  `arm,pl061` GPIO, `gpio-leds`, thermal, SGPO, and a second TPM on SPI
  (`infineon,slb9670`, besides the I2C one). Each needs a decision: describe it
  or leave it out on purpose.
- I2C bus numbers (CPLD on 4, EEPROM `7-0051`, lm75 `5-004a`, tmp75 `7-0048/49`)
  match the *factory* kernel's mux enumeration (pca9548 @0-0070, pca9546
  @4-0071). Mainline numbering can differ, so use names or aliases, not
  hard-coded bus numbers.
- The ported factory scripts (fan floor at step 9, `setphyspeed -q`) match
  factory OS **≥1.0.526**. The second unit (2026-11-15) boots 1.0.457, which
  has neither. Our module enforces both itself, so this is no blocker, but
  don't treat "factory behaviour" as a single version.
