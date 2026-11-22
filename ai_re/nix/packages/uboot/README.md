# uboot: one patch series on upstream U-Boot 2026.10

`series.nix` holds the ordered list; each patch has a **scope** and each build applies only its
own scopes (`patchesFor` in `default.nix`), so a sibling board can take the generic ones. Numbers
give the apply order inside a build. The series is not one linear branch: `alpine-v2` and
`alpine-v3` add their PCIe driver at the same spot of `drivers/pci/Makefile`, so 0030 does not
apply after 0017 (checked 2026-11-22; every set of scopes that is built applies cleanly, 0001
hunk 2 with fuzz 1).

Evaluation fails when `series.nix` and `patches/` differ, a number is missing or out of order, a
build asks for a scope that does not exist, or nixpkgs moves to another U-Boot release than the
one the series is for (`version` in `default.nix`). Which output takes which patches:
`just uboot-variants` (`variants.nix`).

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
| 0019 | spi-nor-ro | read-only SPI NOR with an optional write window (env), exact opcode allowlist (generic; scope was `alpine` until 2026-11-18, same drvPaths) |
| 0020 | alpine-v2 | SoC init from the DT in place of the vendor U-Boot: MUIO, GPIO, CCU; `alpine` command |
| 0021 | alpine-v2 | report where the env came from (boot line + `alpine state`; never stored in the env) |
| 0022 | dmc12 | Edgecore DMC-12 CPLD sysreset |
| 0023 | dmc12 | the DMC-12 board: U-Boot DT additions + `alpine_v2_dmc12_defconfig` |
| 0024 | alpine-v3 | Alpine V3 target after al-boot stage3 (board/amazon/alpine-v3, the 0014 model) |
| 0025 | alpine-v3 | read-only SPI NOR: optionally pass the 4-byte-address reads (32 MiB flashes) |
| 0026 | cs8210 | the CS8210-32X board: U-Boot DT additions + `alpine_v3_cs8210_defconfig` |
| 0027 | alpine-v3 | write window: up to six ranges, 4-byte-opcode chips (`_ADDR_WIDTH`), `sf erase/write` inside them (`CMD_SF_WRITE_WINDOW`) |
| 0028 | cs8210 | `alpine_v3_cs8210_installer_defconfig`: boot SPI writable only in the scheme A' windows (devices/cs8210-32x/spi-upgrade-plan.md) |
| 0029 | alpine-v3 | SoC init from the DT in place of the vendor U-Boot: MUIO, GPIO, PLL dividers, base MAC; `alpine` command |
| 0030 | alpine-v3 | external PCIe root complex (own HAL copy built for V3, AVG SerDes iCal; ~25 k lines vendor code) |
| 0031 | cs8210 | the CS8210 vendor board-cfg as U-Boot DT (SoC init, PCIe ports, endpoints left alone) |
| 0032 | armada8k | Armada 7040/8040: map all DRAM in the MMU (EFI crashed on 8 GiB boards) |
| 0033 | armada8k | Armada 7040/8040: build with NO_NET and without an MMC env |
| 0034 | as7772 | AS7772-32X on top of the as7760 ones: name, control tree, its factory DDR setup (HSE clear, wrlvl starts) |
| 0035 | fsl-sata | `fsl_sata`: bind a `sata_bootdev` per disk so standard boot finds it (generic; taken by the as7772 builds) |

## Builds

| outputs | where | scopes | notes |
|---|---|---|---|
| `uboot-as7760-{nor,chainload}` | `default.nix` | t208x + as7760 | a board is `{ name, patches, config }`; `norImage` / `chainloadImage` make its two images. devices/edge-core-7760-32x/u-boot-port.md |
| `uboot-as7772-{nor,chainload}` | `default.nix` | t208x + as7760 + as7772 + fsl-sata | devices/edge-core-as7772-32x/u-boot-port.md |
| `uboot-dmc12-*` | `alpine-v2/` | alpine + spi-nor-ro + alpine-v2 + dmc12 | selfinit/stage3app Kconfig (SPI env, write window, SoC init) is in `alpine-v2/default.nix`, not in a patch. devices/dmc-12-48r-as0/u-boot-port.md |
| `uboot-cs8210*` | `alpine-v3/` | alpine + spi-nor-ro + alpine-v3 + cs8210 | devices/cs8210-32x/u-boot-port.md |
| `uboot-as5114-chainload` | `armada8k/` | spi-nor-ro + armada8k | RAM chainload, board control DTS copied in at build time. devices/edge-core-as5114-48x/uboot-chainload.md |

The two Alpine directories share one builder (`lib.mkAlpine` in `default.nix`). Tests: `tests.nix`
(AS7760, AS7772; it also pins the sha256 of the `*-nor` images, which are flashed: a compiler bump
or a patch edit that changes one has to be acknowledged there), `alpine-v2/tests.nix`,
`alpine-v3/tests.nix`; `armada8k` checks its safety config in the build itself.

Reproducibility: `reproducibleVersion` takes the cross prefix out of the compiler banner; the
build date is nixpkgs' `SOURCE_DATE_EPOCH` default, so every banner says "Jan 01 1980" and builds
are told apart by `CONFIG_LOCALVERSION` and their sha256, not by date.

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
