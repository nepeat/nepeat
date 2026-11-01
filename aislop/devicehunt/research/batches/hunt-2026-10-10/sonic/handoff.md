# sonic_platforms handoff — 2026-10-10

## Outcome
Captured SONiC per-platform definitions for 7 vendor dirs at ONE pinned commit
`c431b65bf2905ce88d14578a03c68338413b353b` (sonic-net/sonic-buildimage master, cloned 2026-10-10T10:37:18Z,
blobless depth-1 sparse). 63 platform dirs: accton 23, alphanetworks 4, celestica 10, ingrasys 9,
inventec 7, quanta 8, wistron 2. All seven requested vendor dirs existed. Extraction and a curated
candidate (63 sources / 63 devices) are done and validated; nothing in data/devices.sqlite or the import manifest was touched.

## Files written and captures/hashes
- `data/raw/hunt-2026-10-10-sonic/device/<vendor>/<platform>/...` — 399 files total (277K) incl. the allowed upstream files
  (platform.json, platform_components.json, platform_asic, default_sku, installer.conf, pmon_daemon_control.json, platform_env.conf where present, each HWSKU port_config.ini)
  plus one locally generated `subdir-listing.json` per platform (subdir names; is_hwsku = has port_config.ini/hwsku.json/sai.profile; NOT an upstream file, flagged in the manifest).
- `data/raw/hunt-2026-10-10-sonic/capture-manifest.json` — repo, commit, clone time, per file path/sha256/bytes/permalink (raw.githubusercontent.com at the commit; null for local listings).
- `data/raw/hunt-2026-10-10-sonic/sonic-platform-facts-2026-10-10.json` — curated candidate as specified (snapshots relative to data/).
- `data/raw/hunt-2026-10-10-sonic/sonic-platform-facts-2026-10-10.json` — the single curated candidate; `snapshot` paths are relative to this file's own directory so `curated()` imports it in place (sha256 48f4ec73136c15539ec1ef1ce9f43fe392f7f6a70fa7492034dddf0ab2ae6c4e).
  (An earlier `.inplace.json` variant was deleted after review.)
- `research/batches/hunt-2026-10-10/sonic/extraction.json` — per-platform extraction (script kept outside data: scratchpad `sonic/scripts/{capture,extract}.py`, run with `python3 -I`).
- Not captured by design: pddf-device*.json, hwsku.json, sai profiles, sonic_platform python, plugins, sensors.conf.

## Supported exact identities and practical facts
One software-reference identity per platform (brand `SONiC platform`, model = platform id literal, no affiliations, identifier `sonic_platform`), kept separate from hardware identities (AS7772 precedent). Examples:
- `x86_64-accton_as7712_32x-r0`: asic broadcom, default HWSKU `Accton-AS7712-32X`, console 115200, 32-row port_config.ini; no platform.json (so no PSU/fan/thermal counts).
- `x86_64-accton_as7816_64x-r0`: chassis.name printed `7816-64X`, 64 sfps entries, asic broadcom.
- `x86_64-accton_wedge100bf_32qs-r0`: chassis `Wedge100BF-32QS-O-AC-F-BF`, asic barefoot, HWSKU `montara`, console 57600, 32 sfps.
- `x86_64-cel_e1031-r0`: chassis `Celestica-E1031-T48S4`, 55 sfps entries, console 9600.
- `x86_64-cel_seastone-r0`: chassis `DX010`, default HWSKU `Seastone-DX010`, 8 HWSKU dirs, no CONSOLE_SPEED line in installer.conf.
- `x86_64-quanta_ix9_bwde-r0`: chassis `T9032-IX9`, 32 sfps. `x86_64-wistron_sw_to3200k-r0`: marvell-teralynx, 6 HWSKUs.
Only 9 of 63 platforms have a `platform.json` with a `chassis` section; 5 more have platform.json with `interfaces` only (recorded as `sonic_platform_json_interfaces_count`; no zero PSU/fan counts are emitted for them). All platforms with port_config.ini carry `sonic_port_config_rows` (logical rows per HWSKU). 8 platform_components.json files exist (component names recorded keyed by the file's chassis key). Facts are literal `sonic_*` names; list values are JSON strings. `sonic_drawer_fan_count` and `sonic_chassis_level_fan_count` are separate because platform.json lists the same fan names in both places.

## Excluded claims, conflicts, gaps
- Owned-model check (case-insensitive over platform ids, chassis names, HWSKU/subdir names, and all paths in the sparse device/ checkout): CS8200, CS8210, AZ4444, 7760, AS7760, DMC-12, AS7772 — ZERO hits. This is a gap: within the 7 searched vendor dirs at this commit there is no platform dir for any owned model (nearest: Accton AS7712/7716/7726/7816, `7816-64X`, but these are not the 7760/7772 identities). Absence is scoped to the 7 searched vendor dirs at that commit only.
- No hardware claims: counts are SONiC declarations, not datasheet/installed config/existence of a variant. Directory names (accton, cel, ingrasys...) are NOT turned into company roles. Quanta/Inventec/Wistron/Celestica platforms are not mapped to catalog hardware identities, SKUs, or ONIE entries. Platform id vs. HWSKU vs. chassis.name differ (e.g. `7816-64X` vs `Accton-AS7816-64X`); all kept literal. Platforms with no default_sku file (questone_2, d6254qs, d6556: default HWSKU absent) and quanta_ix1b_32x has no HWSKU subdirs (`sonic_hwskus` omitted, audited); those are reported as absent, not guessed.
- Other vendors (Dell, Arista, Mellanox, Cisco, Juniper, etc.) in device/ were out of scope.

## Checks run and results
- Manifest: 399 files, all sha256 and byte counts re-verified against the files on disk: 0 mismatches.
- Curated JSON: loaded with `python3 -I`; 63 unique source ids = 63 devices; every snapshot path exists.
- Temp import (scratchpad `sonic/t.sqlite`, in place): returned 63; records 63, facts 672, integrity_check ok, foreign_key_check empty (final, after S2; earlier 646/673 counts are superseded).
- No test suite run (no code changed). Network: one clone only.

## Decisions needed
1. (Resolved) single in-place curated file; no choice needed.
2. Whether a lowercase `sonic-platform-<id>` per-platform source (63 sources) is acceptable vs one aggregated source.
3. Whether the thin tier `project` identities (no PSU/fan data for 54 of 63) are worth importing before richer sources.

## Next bounded step
Optional, not started: ONL (opencomputeproject/OpenNetworkLinux) platform definitions for Accton/Quanta/Inventec/Ingrasys/Celestica; sonic-buildimage `platform/` (pddf, per-vendor sonic_platform modules) for PSU/fan details; a pinned look for AS7772/AZ4444 in other repos (e.g. SONiC `device/` of other vendor dirs, ONIE `machine/`), which this capture did not search beyond the seven vendor dirs.

## Fixes after review (sonic-review.md)
- F1/F2: facts are now emitted only when the key exists in platform.json (psus, fans, fan_drawers, thermals, sfps, components); an automated audit over all 63 devices confirms presence of each fact == presence of its key. silverstone_v2 no longer has thermal/sfp zeros; wedge100bf no longer has empty component names.
- F3: platform_components.json count corrected to 8 (verified).
- F4: `sonic_port_count` replaced by `sonic_port_config_rows` (logical/breakout port_config.ini rows per HWSKU, not physical ports), `sonic_sfp_entry_count` and `sonic_platform_json_interfaces_count` (separately, from platform.json). Coverage text states this. Fan facts split into `sonic_drawer_fan_count` and `sonic_chassis_level_fan_count`.
- F5: owned-model gap softened to the 7 searched vendor dirs; unsupported aside removed.
- Single candidate: `data/raw/hunt-2026-10-10-sonic/sonic-platform-facts-2026-10-10.json` now has snapshots relative to its own directory (importable in place); the .inplace variant is deleted. This supersedes the "two input files" note and Decision 1 above. Re-validated: 399 manifest hashes ok, all snapshots exist, temp import = 63 records / 673 facts, integrity ok, FK check empty. Captured files and manifest unchanged.

- S2: `sonic_hwskus` omitted when empty (only quanta_ix1b_32x); final facts total 672.
