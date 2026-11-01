# sonic_platforms independent review (Claude Sonnet 5.5), 2026-10-10

Verdict: ACCEPT with notes, conditional on fixing F1 before import. No other must-fix.

## Checks run
- Manifest: 399 entries, 399 files on disk, 0 missing, 0 extra, 0 sha256 or byte mismatches. 336 upstream-copy plus 63 local-listing (permalink null, flagged).
- Spot-check upstream (5 raw.githubusercontent.com fetches at c431b65): as7816 platform.json, seastone installer.conf, silverstone_v2 platform.json, as5712 Accton-AS5712-54X/port_config.ini, quanta ix9 platform.json. All sha256 equal to saved bytes.
- Curated variants: 63 sources / 63 devices each; sources and devices are identical apart from `snapshot` (data/-relative vs file-dir-relative); every path in both resolves, and each pair is the same file (samefile). curated() resolves snapshot against the input file's parent, so the claim in the handoff about placement is correct.
- Re-derived ALL 63 platforms (not a sample) from the captured files: vendor dir, platform_asic, default_sku (line and HWSKU), CONSOLE_SPEED, console lines, HWSKU list, chassis name, psu/fan_drawer names and counts, fan counts, thermal names, component names, sfps/interfaces counts, port_config.ini row counts. Only discrepancies are F1-F3 below.
- Port count: rows = non-blank, non-`#` lines of port_config.ini (header excluded; no odd/non-Ethernet rows). This counts logical ports, not lanes, which is the right unit, but see F4.
- Boundaries: brand `SONiC platform`, model = platform id, title = brand + model, tier project, 0 affiliations, identifier sonic_platform; coverage text on all 63 disclaims datasheet, vendor support, installed config, existence or variant, and company role of directory names. Chassis names literal (`7816-64X`, `DX010`, `Silverstone_v2`). No platform id is mapped to a hardware identity.
- Owned-model search (CS8200, CS8210, AZ4444, 7760, AS7760, DMC-12/DMC12/DMC_12, AS7772) over path names and file contents of the whole capture: 0 hits (the only "7772" hit is a hex substring in capture-manifest.json). Gap is confirmed for the captured scope.

## Findings

### Must-fix
F1. facts/silent-as-zero: `x86_64-cel_silverstone_v2-r0` (data/raw/hunt-2026-10-10-sonic/*.json) emits `sonic_thermal_names = []`, `sonic_thermal_count = 0` and `sonic_port_count = "0 sfps entries in platform.json"`. Its platform.json chassis has keys name/status_led/components/fans/fan_drawers/psus only: no `thermals` and no `sfps` key. The file is silent, not zero. A buyer would read a 0-port, 0-thermal switch (it has 128x100G HWSKUs). Drop these three facts for that device (or state "no thermals key in platform.json"), regenerate both variants.

### Should-fix
F2. `x86_64-accton_wedge100bf_32qs-r0`: `sonic_platform_json_component_names = []` although its platform.json has no `components` key (silent). The `sonic_component_names` value `{"Wedge100BF-32QS-O-AC-F": []}` is literal (platform_components.json has `"component": {}`), so keep that, drop the former.
F3. handoff.md says "10 platform_components.json files exist"; the capture has 8 (and 8 `sonic_component_names` facts). Correct the count or explain the 2 missing.
F4. `sonic_port_count` from port_config.ini rows is a logical-port/breakout count per HWSKU, not front-panel cages: Accton-AS5712-54X = 72 rows (name suggests 54), Seastone-DX010-10-50 = 112, Minipack 128, Midstone-200i_128x100 = 130. The text says "data rows by HWSKU", which is accurate, but the fact name `sonic_port_count` invites buyer misreading. Rename to `sonic_port_config_row_count` / `sonic_platform_json_sfps_entries` (or at least state "logical rows, includes breakout") before the thin tier is shown to users.
F5. Owned-model gap wording over-reaches. extraction.json/handoff say "SONiC upstream at this commit has no platform dir for any owned model", but only 7 vendor dirs were sparse-checked (and the full path listing was not saved as evidence). Reword to "not present in the 7 captured vendor dirs at this commit". The aside "AS7772 and AZ4444 live elsewhere (ONIE/other vendors)" is unsupported by any capture; remove or mark as unverified speculation.

### Notes
N1. `sonic_installer_conf_console_lines` also carries `ONIE_PLATFORM_EXTRA_CMDLINE_LINUX=...` (kernel cmdline), so the name overstates "console"; literal text is preserved, fine.
N2. `sonic_hwskus` depends on the local subdir-listing (is_hwsku also keys on hwsku.json / sai.profile, not captured). For `x86_64-alphanetworks_snj60d0_320f-r0` the only HWSKU `Alphanetworks-SNJ60D0-320F` cannot be confirmed from captured bytes (no port_config.ini); it is flagged is_hwsku in the local listing only.
N3. Each source's `snapshot` is a single file (platform_asic for 49, platform.json for 14) while facts derive from several; the hashes of the others are only in capture-manifest.json. Acceptable, but integrator should keep the manifest alongside.
N4. 14 platform.json files: 9 with chassis, 5 interfaces-only (counts labeled "no chassis section", no PSU/fan facts emitted: correct). Missing default_sku (3) and installer.conf (1) produce no fact: correct.
N5. Fan count text states chassis-level and in-drawer separately; verified equal in all 9. Good.

## Fix verification (second pass)

Updated verdict: ACCEPT. No remaining must-fix. Two stale-doc should-fixes remain (handoff text only).

Checks re-run:
- Manifest: 399 entries, 399 files, 0 hash/byte mismatches; captures unchanged.
- Single candidate `sonic-platform-facts-2026-10-10.json`: .inplace file gone; all 63 snapshots (`device/<vendor>/<platform>/platform_asic|platform.json`) resolve relative to the file's own directory.
- Full 63-platform re-derivation against the new fact names (sonic_port_config_rows, sonic_sfp_entry_count, sonic_platform_json_interfaces_count, sonic_drawer_fan_count, sonic_chassis_level_fan_count, thermal/psu/drawer/component names and counts, chassis name, asic, default sku, console speed and lines, hwskus): 0 mismatches. Every key-derived fact is present iff its key exists in platform.json / platform_components.json, and no duplicate fact names within a device.
- F1 fixed: silverstone_v2 has no sonic_thermal_* and no sonic_sfp_entry_count; ports shown only as interfaces count 32 and port_config rows {Silverstone_v2: 32, Silverstone_v2_128X100G: 128}. F2 fixed: wedge100bf has no empty platform_json component fact; literal `{"Wedge100BF-32QS-O-AC-F": []}` kept from platform_components.json (file says `"component": {}`). F3 fixed (8). F4 fixed: coverage text and fact value both say "logical/breakout rows ... (not physical port count)" (AS5712-54X 72 rows). F5 fixed in handoff line 32 (scoped to 7 searched vendor dirs).
- Temp-catalog import from scratch (new sqlite in scratchpad, `import_source(c,'curated', <file>)`): returned 63; sources 63, devices 63, records 63, identifiers 63, facts 673 (matches handoff), affiliations 0; integrity_check ok; foreign_key_check empty; all 63 sources carry cache_path + sha256. data/ and git tree untouched by me.

Remaining should-fix (docs only, handoff.md):
- S1. Stale text still describes the old design: lines 16-17 and 43 (`.inplace` file, "which curated file form" decision, though line 55 supersedes), line 29 (`sonic_fan_count`, "port count recorded as N interfaces entries ... (no chassis section)"), line 39 (646 facts, .inplace import). Prune or mark superseded so the integrator is not misled.
- S2. `x86_64-quanta_ix1b_32x-r0` emits `sonic_hwskus = []` (the local listing found no HWSKU subdirs). Literal in the local listing but it is a non-upstream derivation; consider omitting when empty or labelling it. Also note N2 still applies (snj60d0 HWSKU rests on local listing only).
