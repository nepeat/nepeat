# onie_machines handoff — 2026-10-11 (Claude Sonnet 5.5)

## Outcome
One blobless depth-1 sparse clone of `https://github.com/opencomputeproject/onie` (default branch `master`), commit
`ce480be7094deef7198ee4d50af8427ea89b93e3` (committed 2026-08-19T10:10:10+03:00), cloned 2026-10-11T09:01:15Z. No other network use.
`machine/` has 38 vendor dirs: accton agema alphanetworks celestica centec cig dell delta dni drivenets facebook imt ingrasys intel inventec
juniper kvm_x86_64 lenovo marvell mellanox nec netberg nxp pegatron plexxi qemu_armv7a qemu_armv8a quanta senao silicom stordis supermicro
ufispace wiwynn wnc.
**Selected (7):** accton, alphanetworks, celestica, ingrasys, inventec, quanta, wiwynn — all are target/related names from the assignment.
**Not present:** edgecore/edge-core, foxconn/hon hai, inspur, hyve, synnex, wistron (no such `machine/` dirs; Wiwynn exists as `wiwynn`).
**Skipped on purpose:** delta (assignment says not a target) and all other vendors (incl. agema, dell, facebook, ufispace...).
172 machine dirs have a `machine.make`: accton 100, alphanetworks 35, quanta 12, celestica 11, inventec 8, ingrasys 5, wiwynn 1. **Revised per coordinator:** the catalog's existing `onie` source (`data/raw/onie-platforms.json`, same commit) already covers the 111 accton+celestica dirs on hardware identities, so the **candidate has only the 61 dirs not in that source**: alphanetworks 35, quanta 12, inventec 8, ingrasys 5, wiwynn 1. All 172 are still captured and extracted (`extraction.json`, flag `in_existing_onie_source`).
Dirs without `machine.make` (not machines, skipped): accton/busybox, accton/kernel, alphanetworks/i2ctools, alphanetworks/kernel, celestica/kernel,
celestica/u-boot, ingrasys/kernel, quanta/busybox. Nothing in data/devices.sqlite or import-manifest.json was touched.

## Files and hashes
- `data/raw/discovery-2026-10-11-onie/opencomputeproject/onie/ce480be.../{machine/<vendor>/<dir>/...,COPYING}` — 363 files, 2.4 MB, all kept; 134 are referenced by the candidate (flag `referenced_by_candidate: true` in the manifest = files of the 61 candidate dirs + the alphanetworks and quanta vendor READMEs); the other 229 (the 111 overlap dirs, COPYING) are captured but unreferenced: every `machine.make`,
  `INSTALL*`, dir-level `README*` (5 incl. `README.secureboot`), the 11 `rootconf/sysroot-lib-onie/gen-config-platform`, vendor-level `README.md`
  (alphanetworks, quanta), plus repo `COPYING`. Kernel/busybox/patches/installer.conf/sysinfo-platform etc. not captured.
- `capture-manifest.json` sha256 `7c00c3a91af12ef399bbc987a2dfd4111c1a60c888608b4eb24f06169b5f36aa` (repo, commit, clone UTC, license, per file `path` relative to the raw dir / sha256 / bytes / github blob permalink at the commit).
- `onie-machine-facts-2026-10-11.json` sha256 `5d4b41edb2920e56f2ead79297ca7a3cd31ceeaae4facd35482614c74e710cea` — curated, 61 sources / 61 devices, `snapshot` = that dir's `machine.make`, relative to the file's own directory (imports in place).
- `research/batches/discovery-2026-10-11/onie/extraction.json` sha256 `bd86845ae0e815fd9308fbc17e3f266c8f6f57f529064fcac8858457435aa068` (per-dir facts); scripts in `scripts/` (stdlib, `python3 -I`; the originals live in the scratchpad `onie/scripts/`).
- Licence: repo COPYING states ONIE is GPLv2; files keep upstream rights (recorded in manifest and per source).

## Identities and practical facts
One software-reference identity per candidate machine dir (61): brand `ONIE machine`, model = dir name literal, identifier `onie_machine`, kind "network switch ONIE machine definition (software reference)",
`affiliations: []`, tier `project`. Not mapped to any hardware identity/SKU. Facts (value ends with `(file:line)`, paths relative to the machine dir; omitted when the file lacks them):
`onie_repo_commit`, `onie_vendor_dir_as_printed`, `onie_machine_dir_as_printed`, `onie_arch`, `onie_switch_asic_vendor`, `onie_vendor_rev`, `onie_machine_rev` (all ifeq-branch values, flagged `[inside conditional]`),
`onie_vendor_id`, `onie_vendor_id_code_comment` (code comment above VENDOR_ID, labelled as such, boilerplate dropped), `onie_vendor_version`, `onie_console_dev/speed/flag`, `onie_uefi_enable`, `onie_pxe_efi64_enable`,
`onie_linux_version`/`_minor_version`, `onie_gcc_version`, `onie_i2ctools_*`, `onie_ipmitool_enable`, `onie_extra_cmdline_linux`, `onie_uboot_*`, `onie_kernel_dtb`, `onie_partition_type` etc.,
`onie_machine_make_header_comments` (+ `_cpu_comments`, `_spdx`), `onie_copyright_company_as_printed` (company string only; `onie_machine_make_copyright_lines` was dropped, no person names/emails anywhere; only `quanta_dnv`: "QUANTA COMPUTER INC."), `onie_install_heading_lines`, `onie_install_build_machine_arg`, `onie_install_platform_strings`,
`onie_install_compatible_machines`, `onie_install_rename_note`, `onie_install_supported_platform_lines` (quanta common boards), `onie_install_cpu_lines` (only lines printing a CPU/processor name, e.g. `CPU: Intel(R) Xeon(R) ...`; 5 dirs), `onie_install_heading_adjacent_lines` (other INSTALL lines naming a CPU family/board, not duplicating the heading fact; 6 dirs; the 5 ingrasys dirs no longer carry a duplicate), `onie_readme_*`,
`onie_vendor_readme_entries` (vendor README table row / quanta heading naming the dir), `onie_gen_config_platform_lines`, `onie_captured_files`. Totals for the 61-device candidate: 1368 facts, max value 526 chars.
Examples (accton_/cel_ examples are from the overlap and are NOT in the candidate; they show the extraction):
- `accton_as7712_32x` (overlap, not imported): arch x86_64 (machine.make:8), asic `bcm` (:9), VENDOR_ID `259` (:24) with comment "Accton Technology Corporation IANA number" (:23), CONSOLE_DEV 1 (:32; no CONSOLE_SPEED), Linux 5.4.86, GCC 8.3.0, INSTALL:2 "Installing ONIE on Accton AS7712_32X", rename note: old name `accton_as7702_32x`, compatible `accton_as7702_32x-r0`/`accton_as7712_32x-r0` (INSTALL:9-10).
- `cel_seastone` (overlap, not imported): header comment "CPU Module: Intel Atom Rangeley (C2000)" (machine.make:2), VENDOR_ID 12244 ("Open Compute Project IANA number"), VENDOR_VERSION .0.0.4, Linux 3.2.69, PARTITION_TYPE gpt; no CONSOLE_SPEED in machine.make (silent, not zero).
- `quanta_common_rangeley`: VENDOR_ID 7244 with code comment "DELL IANA number" (machine.make:18) — printed as-is, a code comment and a caution, not a role; INSTALL lists 7 `(cpu module)` product lines T5032-LY6, T3048-LY8 ... T3048-IX2.
- `alphanetworks_snk60i0_320f`: VENDOR_ID 31874 ("Alpha Networks Inc."), README table rows `x86_64-alphanetworks_snk60i0_320f-r0/-r1` with `VENDOR_REV=0/1`; vendor README row "SNK-60I0-320F | [alphanetworks_snk60i0_320f]".
- `ingrasys_s8900_54xc`: INSTALL:2 "Ingrasys S8900 Series w/ Intel Atom Rangeley (C2538) CPU Module", CONSOLE_SPEED 115200, EXTRA_CMDLINE_LINUX `i2c-ismt.bus_speed=100`.
- `wiwynn_andromeda`: only machine.make + a headerless INSTALL (no product-name line), VENDOR_ID 40092.
Distributions: ONIE_ARCH x86_64 most; powerpc-softfloat and armv7a also appear. SWITCH_ASIC_VENDOR: bcm 157, bft 5, nephos 4, cavium 3, mvl 2, mlnx 1. VENDOR_ID: 259 x100 (accton), 31874 x35, 7244 x12, 12244 x11, 6569 x8, 2468 x5, 40092 x1.

Review fixes (onie-review S1/S2): `onie_install_build_machine_arg` (59 dirs) and `onie_install_supported_platform_lines` (6 dirs) are now the full printed INSTALL line, byte-exact apart from trailing whitespace (double spaces kept, `(cpu module)` as printed, leading ` - ` kept). Line-literal re-check of those four install-line facts: 95 items, 0 mismatches against the cited lines. Raw captures with upstream personal emails stay byte-exact (not in the candidate).

## Excluded claims, conflicts, gaps
- Directory names, IANA/copyright comments, INSTALL "Installing ONIE on <Brand> <Model>" lines and vendor-README model labels are labels printed by the repo; no company role, hardware identity, SKU or manufacture claim was made.
- Dir name != product everywhere: e.g. `accton_as4610_30` heading says `AS4610-30T`, `accton_as5915_16x` INSTALL heading says `AS5915-18X` (printed literally; looks like a copy-forward — left as-is, flagged only as a conflict to review), `alphanetworks_snh60a0_320f` and `..._320fv2` share the model label SNH-60A0-320F in the vendor README, quanta common dirs cover many T-numbers. Quanta README lists the same dir under several headings (all preserved).
- `quanta_come` INSTALL has no "Installing ONIE on" line; wiwynn INSTALL has no product line; both kept silent. CONSOLE_SPEED is present in only 26 machine.make files; other speeds live in installer.conf (not captured).
- Owned-model check (case-insensitive: CS8200, CS8210, AZ4444, 7760/AS7760, DMC-12/DMC_12/DMC12, AS7772, SV1120, SV1121, SV2121, SV2221, ByteDance, B4020, B4010) over all 363 captured paths and contents: ZERO name hits (the only raw-substring hit is "3917760" in a file size in celestica/cls_ds4101/README.secureboot:273). Also over the full git path list of `machine/` for ALL 38 vendor dirs (4292 paths, including the ones not captured; AS7772 token checked as `7772`): ZERO hits. So ONIE `machine/` at this commit has no dir for AS7760/AS7772/AZ4444/CS82xx/DMC-12 — scoped to path names plus the captured files' contents, not to every file in the unselected vendor dirs.
- Not searched: ONIE docs, other branches/tags, the Edgecore-named machines that may live under the `accton` dir (no `edgecore` dir exists).

## Overlap with the existing `onie` source: genuinely new information (lead only, not imported)
Existing facts there: build_console_dev (90), build_onie_arch, build_switch_asic_vendor, build_uefi_enable (71), build_vendor_id (all 111), plus identity_basis/software_evidence/source_title. This capture adds, for the 111 accton+celestica dirs (counts of dirs):
- INSTALL product heading line (`Installing ONIE on <text>`, file:line): 111. Heading vs dir-name disagreements (10), e.g. `accton_as5915_16x` -> "Accton AS5915_18X" (the AS5915-16X/18X conflict: dir says 16x, heading says 18X; sibling `accton_as5915_18x` exists), `accton_as9817` -> "Accton AS9726_32D", `accton_as9737_32db` -> "Accton ASA9737_32DB", `accton_as4610_30` -> "Accton AS4610_30P and Accton AS4610_30T", `cel_e1031` -> "CELESTICA E1031(Haliburton)", `cel_d2030` -> "Celestica D2030(Redstone-TP)", `cls_ds4101` -> "KVM x86_64 Virtual Machine".
- CONSOLE_SPEED: 14 dirs (e.g. accton_as7316_54xs, accton_as7726_32x, accton_as7816_64x, accton_minipack); console_dev is already there.
- VENDOR_REV/MACHINE_REV 111; LINUX_VERSION+minor 104; GCC_VERSION 108; PXE_EFI64_ENABLE 70; I2CTOOLS_SYSEEPROM 31, I2CTOOLS_ENABLE 23, IPMITOOL_ENABLE 23, SKIP_ETHMGMT_MACS 29; U-Boot/DTB (ppc/arm) 12/9; VENDOR_VERSION 5; EXTRA_CMDLINE_LINUX 3.
- Platform strings printed in INSTALL (e.g. `accton_as7712_32x-r0`): 110; rename/compatible notes: 2 (`accton_as7712_32x` old name `accton_as7702_32x`, and one more accton dir).
- Code comments above VENDOR_ID (IANA owner name as printed, e.g. "Accton Technology Corporation IANA number", "Open Compute Project IANA number"): 101; CPU-module comments: 3.

## Cross-reference leads (read-only against data/devices.sqlite; nothing merged)
- The existing `onie` source (111 records: 100 Accton + 11 Celestica) has `onie_platform` identifiers equal to the dir names literally; all 111 overlap dirs match and were excluded from the candidate. The 61 candidate dirs have no existing catalog identifier.
- Dir name -> SONiC platform id by the `x86_64-<dir>-r0` form (not literal equality, derived): 33 of the 63 accepted SONiC platform ids correspond (e.g. `x86_64-accton_as7712_32x-r0`, `x86_64-cel_seastone-r0`, `x86_64-ingrasys_s9100-r0`, `x86_64-inventec_d6356-r0`, `x86_64-alphanetworks_bes2348t-r0`). Dir name -> ONL id by the `x86-64-<dir with dashes>-r0` form: 56 existing `onl_platform` ids correspond. No ONIE dir name equals a SONiC/ONL id verbatim (formats differ); no `devices.model` equals a dir name.
- INSTALL heading last token (with `_`->`-`) exactly equals existing hardware device models for many Accton dirs (e.g. `AS7712-32X`, `AS7816-64X`, `AS9716-32D` for brands Accton and Edge-Core); only a lead — not merged and not used as a fact.
- Notable: `accton_as7816_64x`, `accton_as7712_32x`, `accton_as7726_32x`, `accton_as9716_32d`, `accton_as9726_32d` exist here and as SONiC/ONL platforms; no owned model (see check above).

## Checks run
- Manifest: 363 files, 0 sha256/byte mismatches; 365 files on disk (+ manifest + candidate); 61 snapshots resolve; 61 unique models.
- Temp import (scratchpad `onie/t.sqlite`, `pipeline.import_source(c,'curated',...)`), re-run after revision: returned 61; sources 61, devices 61, records 61, identifiers 61, facts 1368, affiliations 0; integrity_check ok; foreign_key_check empty.
- All assertions in the build: no duplicate fact names per device, no fact over 4 KB. Temporary clone deleted. No code changed, no test suite run.

## Decisions needed
1. Whether to feed the "new information" list above into the existing `onie` source as extra facts (not done).

## Next (optional, not started)
Capture `installer.conf` / `platform.conf` (console speed, kernel cmdline) and other vendors' dirs (agema, ufispace, delta, dell...) if wanted.
