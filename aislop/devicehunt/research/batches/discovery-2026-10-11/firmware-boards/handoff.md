# firmware_boards handoff — 2026-10-11 (Claude Sonnet 5.5)

## Outcome
Three blobless depth-1 sparse clones (one per repo, no per-file fetches, no 403/429), one curated candidate: 8 sources / 8 devices (review S2: U-Boot yosemite4 dropped).
- coreboot, cloned from `https://review.coreboot.org/coreboot.git` (cloned cleanly; GitHub mirror not used; server printed "filtering not recognized", so the clone was effectively depth-1 sparse only) @ `23115693cfdd3182633c829397f8bc5685122636`, 2026-10-11T11:35:27Z. 8 boards in 5 vendor dirs -> 8 devices (brand `coreboot mainboard`, model = dir path e.g. `ocp/tiogapass`, identifier `coreboot_mainboard`).
- U-Boot `https://github.com/u-boot/u-boot` @ `5e3b01c8466dbe45cd6a06299f8f4c7e8c1c2363`, 2026-10-11T11:35:56Z: ZERO `board/<target vendor>/` dirs. One name-token-only hit, U-Boot's own `arch/arm/dts/nuvoton-npcm845-yosemite4.dts` (model `"Nuvoton npcm845 yosemite4"` dts:11, compatible `"nuvoton,npcm845"` :12; Makefile has 1153 lines, no yosemite rule). Raw capture retained but NOT imported (marked in capture-manifest.json): no candidate device.
- EDK2 `https://github.com/tianocore/edk2-platforms` @ `013fca79e55c7ac4f3d0eaf23799a9a73ed43dba`, 2026-10-11T11:35:56Z: ZERO target/related dirs; nothing captured.
Nothing in data/devices.sqlite or the import manifest touched. Clones deleted.

## Files and hashes
`data/raw/discovery-2026-10-11-firmware-boards/`: `capture-manifest.json` (f14bf67eaf9d...; 44 files + listing), `firmware-boards-facts-2026-10-11.json` (63c1291df6b6...; the candidate, snapshots relative to its dir), `tree-listings.json` (44e72f68da1b...; locally generated from tree objects, not upstream), captures under `coreboot/<commit>/src/mainboard/...` (5 vendor `Kconfig`+`Kconfig.name`; per board `board_info.txt`, `Kconfig`, `Kconfig.name`, `devicetree.cb`, all < 7 KB), `u-boot/<commit>/arch/arm/dts/{nuvoton-npcm845-yosemite4.dts,Makefile}`. Scripts: scratchpad `firmware-boards/scripts/{build,validate}.py` (`python3 -I`), `owned-check.json`, `t.sqlite`.

## Vendor dirs listed and selection
- coreboot `src/mainboard/` (65 entries incl. Kconfig; full list in tree-listings.json): selected `bytedance`, `facebook`, `foxconn`, `inventec`, `ocp` (target names or the OCP label asked for). Present but not selected: `mitaccomputing` (3 boards; not in target registry), `ibm`, `raptor-cs`, `supermicro`, `google`, `intel`, `dell`, etc. Absent: `quanta`, `wiwynn`, `inspur`, `amazon`, `accton`, `celestica`, `ingrasys`, `wistron`, `hyve`.
- Selected boards: `bytedance/bd_egs`, `facebook/fbg1701`, `facebook/monolith`, `foxconn/d41s`, `foxconn/g41s-k`, `inventec/transformers`, `ocp/deltalake`, `ocp/tiogapass`. (`foxconn/g41s-k` Kconfig.name also defines a second board `G41M/G41M-S/G41M-V`, variants dir not captured.)
- U-Boot `board/` (213 dirs; list in tree-listings.json): none of accton/celestica/quanta/inventec/ingrasys/wistron/wiwynn/inspur/bytedance/foxconn/hyve/facebook/ocp/amazon exist; `git ls-tree` over the full repo for those tokens in any path returned only upstream-synced `dts/upstream/src/arm/aspeed/aspeed-bmc-{bytedance,facebook}-*.dts` (kernel mirror, already covered by the Linux BMC dts worker; not captured here) . `board/nuvoton` (arbel_evb, poleg_evb) and `board/aspeed` are not target dirs; not captured.
- EDK2-platforms `Platform/`: 96Boards ADLINK AMD ARM ASRockRack Ampere Bosc Intel Loongson Marvell MinPlatformPkg NXP Phytium Qemu Qualcomm RISC-V RaspberryPi SiFive SolidRun Sophgo StandaloneMm; `Silicon/`: AMD ARM Ampere Atmel Bosc Broadcom Intel Marvell Maxim NXP Openmoko Phytium Qemu Qualcomm RISC-V SiFive Sophgo Synopsys; `Features/`: Ext4Pkg Intel. Zero target vendors (searched at the directory-name level only).

## Identities and practical facts (every value carries file:line)
Facts: `coreboot_vendor_kconfig_label` (e.g. `bool "Open Compute Project"` ocp/Kconfig.name:4), `coreboot_board_info_<key>` (every `Key: value` line of board_info.txt), `coreboot_kconfig_name_line`, `coreboot_kconfig_romsize_select` (the literal `select BOARD_ROMSIZE_KB_n` line; not a printed ROM size), `coreboot_kconfig_silicon_select` (SOC/CPU/NORTHBRIDGE/SOUTHBRIDGE/SUPERIO/EC), `coreboot_kconfig_mainboard_{dir,part_number,family}`, `coreboot_devicetree_chip_lines` (count + first 12 `chip` lines). Vendor/dir names are labels, no roles; `affiliations: []`.
- `ocp/tiogapass`: Board name `TiogaPass` (board_info.txt:1), Category server, Release year 2018 (:5), `select BOARD_ROMSIZE_KB_32768` (Kconfig:7), SOC_INTEL_SKYLAKE_SP (:14).
- `ocp/deltalake`: `DeltaLake`, server, 2020, `BOARD_ROMSIZE_KB_65536` select, SOC_INTEL_COOPERLAKE_SP.
- `bytedance/bd_egs`: board_info `Vendor name: ByteDance` (:1), Board name `bd_egs`, Category eval, `BOARD_ROMSIZE_KB_65536` select, SOC_INTEL_SAPPHIRERAPIDS_SP, MAINBOARD_PART_NUMBER `ByteDance bd_egs` (Kconfig:21).
- `inventec/transformers`: `Vendor name: Inventec`, `Transformers`, eval, `BOARD_ROMSIZE_KB_65536` select, SOC_INTEL_SAPPHIRERAPIDS_SP.
- `facebook/fbg1701`: `Vendor name: Facebook`, `FBG-1701`, sbc, `BOARD_ROMSIZE_KB_8192` select, SOC_INTEL_BRASWELL. `facebook/monolith`: `Monolith System`, misc, `BOARD_ROMSIZE_KB_16384` select, SOC_INTEL_KABYLAKE.
- `foxconn/d41s`: Kconfig.name label `D41S, D42S, D51S, D52S`, category mini, ROM package DIP-8, `BOARD_ROMSIZE_KB_1024` select, no Release year, Board URL printed. `foxconn/g41s-k`: `G41S-K`, desktop, 2009, `BOARD_ROMSIZE_KB_1024` select, Board URL printed.

## Excluded claims, conflicts, gaps
- board_info.txt has no ROM size line anywhere; no ROM size is printed; only the Kconfig select exists (a coreboot build option, not a statement of installed flash). Vendor name line exists only in bytedance, facebook, inventec boards (ocp, foxconn omit it; not inferred from dir). Release year only for tiogapass, deltalake, g41s-k.
- No company roles from dir names (OCP/Facebook/Foxconn/Inventec/ByteDance are labels); no mapping of `transformers`, `tiogapass` etc. to catalog hardware or OpenBMC/SONiC records. Not captured: board source (.c/.asl), SPD, blobs, variants/, fmd files. U-Boot yosemite4 dts and Linux/OpenBMC `yosemite4` are not linked here (lead only).
- Owned-model check (case-insensitive over all captured paths and values, listings, candidate and manifest; CS8200, CS8210, AZ4444, 7760/AS7760, DMC-12/DMC_12, AS7772, SV1120/SV1121/SV2121/SV2221, B4020, B4010): ZERO hits. `ByteDance` token: hits only as labels (coreboot `bytedance/` dir, `Vendor name: ByteDance` bytedance/bd_egs/board_info.txt:1, Kconfig labels); `bd_egs` is not B4020/B4010 and no link is asserted. Scoped to what was captured plus the listed directory names in tree-listings.json (including all vendor dir names; the full-repo path grep for U-Boot above only used target tokens, not the owned-model strings).

## Cross-reference leads (read-only data/devices.sqlite, exact lower-case)
- Name coincidence only: exact model `FBG-1701` exists on a Quanta record (id 7940faaced53b39e61a0, kind "Transceptor de Radiação Restrita"): same string as coreboot `facebook/fbg1701` board_info Board name `FBG-1701`; exact-string agreement only; different kinds, no shared source; needs separate evidence; no merge, no role.
- `Transformers`/`transformers`: OpenBMC machine identity `transformers` (id 42160a5174f13a817a8b, from the bmc worker) equals coreboot board name `Transformers` (inventec/transformers); name coincidence only (OpenBMC identity is BMC firmware, coreboot one is host firmware for an Inventec-labelled dir; both Inventec by label only); needs separate evidence, no merge.
- No match: bd_egs, TiogaPass, DeltaLake, Monolith, D41S, G41S-K, G41M, yosemite4 (searched as model/title and identifier value, exact lower-case).

## Checks run
- Manifest: 45 entries, all sha256/bytes verified, 0 mismatches; all snapshots resolve.
- Temp import into a scratchpad copy of the CURRENT data/devices.sqlite: import_source('curated') returned 8 (after review fixes); sources 1477->1485, devices 5015->5023, records 6626->6634, identifiers 6175->6183, device_facts 63703->63860 (+157), affiliations unchanged (9991); integrity_check ok; foreign_key_check empty. Longest fact under 240 chars.
- No tests run (no code changed). Network: three clones only.

## Decisions needed
1. (Resolved) U-Boot yosemite4 dropped from the candidate.
2. Whether to add coreboot `mitaccomputing` (3 boards) or other non-target vendors later.
3. Whether to import these thin project-tier identities.
