# uboot: one patch series on upstream U-Boot 2026.10

`default.nix` holds the ordered list (`series`); each patch has a **scope** and each build applies
only its own scopes (`patchesFor`), so a sibling board can take the generic ones. Platforms touch
disjoint files, so the whole series still applies, in order, to a pristine tree.

Rules for new work: **one goal per patch**, and fold fixes into the patch they fix instead of
stacking follow-ups. That way a U-Boot bump means rebasing each goal once. Prefer new files to
edits of upstream ones. A refactor of the series must be byte-neutral: same `u-boot.bin` sha256
(or same drvPath) for every affected build.

| # | scope | goal |
|---|---|---|
| 0001, 0007 | t208x | chainloading: keep CCSR where it is; restart secondaries from our boot page |
| 0002–0004 | t208x | T208xRDB fixes: CONFIG_VID/PCI, NS16550 clock, i2c0 before relocation |
| 0008, 0009 | t208x | report all DDR; A009942 workaround honours the board CPO |
| 0011, 0012 | t208x | bring-up commands `ddrcmp`, `spltest` |
| 0005, 0006, 0010, 0013 | as7760 | AS7760-32X: SPD test hack, control-tree DT, factory DDR setup, board identity |
| 0014 | alpine-v2 | chainloaded / stage3-app Alpine V2 target (incl. in-place initrd/FDT boot) |
| 0015 | alpine-v2 | al_eth board params + MAC handed to Linux |
| 0016 | alpine-v2 | al_eth driver (Annapurna HAL, ~22 k lines vendor code) |
| 0017 | alpine-v2 | external PCIe root complex (Annapurna HAL, ~19 k lines vendor code) |
| 0018 | alpine | DW SSI: Alpine CS override; deselect before the controller init and each transfer |
| 0019 | alpine | read-only SPI NOR with an optional write window (env), exact opcode allowlist |
| 0020 | alpine-v2 | SoC init from the DT in place of the vendor U-Boot: MUIO, GPIO, CCU; `alpine` command |
| 0021 | alpine-v2 | report where the env came from (boot line + `alpine state`; never stored in the env) |
| 0022 | dmc12 | Edgecore DMC-12 CPLD sysreset |
| 0023 | dmc12 | the DMC-12 board: U-Boot DT additions + `alpine_v2_dmc12_defconfig` |
| 0024 | alpine-v3 | Alpine V3 target after al-boot stage3 (board/amazon/alpine-v3, the 0014 model) |
| 0025 | alpine-v3 | read-only SPI NOR: optionally pass the 4-byte-address reads (32 MiB flashes) |
| 0026 | cs8210 | the CS8210-32X board: U-Boot DT additions + `alpine_v3_cs8210_defconfig` |

Kconfig of the DMC-12 builds (selfinit/stage3app: SPI env, write window, SoC init) is in
`alpine-v2/default.nix`, not in a patch. Tests: `tests.nix` (AS7760), `alpine-v2/tests.nix`, `alpine-v3/tests.nix`. Alpine V3 builds:
`alpine-v3/default.nix` (scopes alpine + alpine-v3 + cs8210; devices/cs8210-32x/u-boot-port.md).

## Old numbers (before the 2026-11-15 squash)
The Alpine/DMC-12 part was 22 patches (0014–0035); it became 10 with the same resulting tree
(git tree 95d1162a, binaries byte-identical: selfinit u-boot.bin 72b8c42b, stage3app .fimg
a0328d81, chainload f4b4b4f3). Device notes dated before then use the old numbers:

| old | new | | old | new |
|---|---|---|---|---|
| 0014, 0017 | 0014 | | 0024, 0029, 0032 | 0019 |
| 0018 | 0015 | | 0025, 0027, 0034 | 0020 |
| 0020 | 0016 | | 0031 | 0021 |
| 0022 | 0017 | | 0015 | 0022 |
| 0023, 0030, 0033 | 0018 | | 0016, 0019, 0021, 0026, 0028, 0035 | 0023 |
