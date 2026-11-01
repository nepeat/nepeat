# nos_platform_code handoff — 2026-10-10 (Claude Sonnet 5.5)

## Outcome
Both pinned captures extracted. 601 files (842K): 158 SONiC python files from `sonic-net/sonic-buildimage` at
`c431b65bf2905ce88d14578a03c68338413b353b` (same commit as the accepted SONiC candidate) and 443 ONL files from
`opencomputeproject/OpenNetworkLinux` master at `28f52e623a5f820598fda549f5c86c670081a48b`. Blobless clones (2 total, sparse, no other network
except one `git ls-remote` on the ONL repo and one `git show HEAD:LICENSE` per clone, all against the same two repos). Curated candidate: 111 sources / 111 devices (20 SONiC-platform + 91 ONL-platform), 483 facts.
Nothing outside the two owned paths was written; data/devices.sqlite, the import manifest and other dirs untouched. Clone/checkout time (both) 2026-10-10T11:01:30Z (blob fetch/checkout; recorded in the manifest as captured_utc).

## Files and hashes
- `data/raw/hunt-2026-10-10-nos/<repo>/<commit>/<path>` — 601 captured files, upstream bytes unchanged.
- `data/raw/hunt-2026-10-10-nos/capture-manifest.json` sha256 `75c7acd37352065523a784887d2186e5a3afdeefd852587b52ec9b517d2faaef` (repos, commits, UTC time, per-file local path/sha256/bytes/github blob permalink at the commit).
- `data/raw/hunt-2026-10-10-nos/nos-platform-code-facts-2026-10-10.json` sha256 `8be73195f332b64a6c65c1b559cf9b4ce5d6d8a87ff7da29928593d439da1fad` (curated (curated; `snapshot` relative to its own directory; tier `project`).
- `research/batches/hunt-2026-10-10/nos/extraction.json` sha256 `c8ea539df3c57f514a347287485008de7e30cb4b57215bd2a01c2195b6b668a5` (all 1976 extracted items with file+line, per-platform summary, skipped platform dirs).
- `research/batches/hunt-2026-10-10/nos/scripts/{capture,extract,build,validate}.py` (stdlib, run `python3 -I`; paths point at the scratchpad; copies for reproducibility).
- Capture scope: SONiC `platform/broadcom/sonic-platform-modules-{accton,cel,quanta,inventec,alphanetworks}` (ingrasys exists but has NO matching python file: only kernel-module .c files) — `sonic_platform/{psu,fan,fan_drawer,thermal,chassis,eeprom}.py`, `plugins/{psuutil,eeprom,fanutil}.py`, `utils/*fan*` .py. ONL `packages/platforms/{accton,celestica,quanta,inventec,ingrasys}/x86-64/*/onlp/builds/*/module/src/{psui,fani,thermali,sysi}.c` and `platform-config/*/src/python/*/__init__.py` (91 platform dirs; armxx/powerpc excluded by the x86-64 path).

## Supported facts (all values carry `(file:line)`, paths relative to the platform dir)
Fact names: nos_repo_commit, nos_source_files, nos_count_constants, nos_model_part_literals, nos_airflow_literals, nos_airflow_symbol_lines, nos_airflow_comments, nos_eeprom_field_literals, nos_declared_literals. A fact exists only if something was extracted (silent, not zero). Examples:
- ONL `x86-64-accton-as7816-64x-r0`: `"YM-2851J" in if (strncmp(info->model, "YM-2851J", ...)` (psui.c:60).
- ONL `x86-64-accton-as7946-30xb-r0`: arrays `ac_models` = "FSH082","FSF019" (psui.c:74); `"B2F"` strncmp on `fandir` (fani.c:116,183). Also asgvolt64 (`ac_models` FSF019, `dc_models` YM-2851J), as7315-27xb (YM-2401J, YM-1401A), as5916-26xb / as7316-26xb (YM-2401U, um400d), as5915-18x (SPAACTN-04).
- ONL `x86-64-accton-as7712-32x-r0`: `NUM_OF_PSU_ON_MAIN_BROAD = 2`, `NUM_OF_CPLD = 3` (sysi.c); airflow symbol lines `case PSU_TYPE_AC_YM2651Y_F2B:` (fani.c:152) — quoted only as the code's symbol lines, not interpreted; `PLATFORM = "x86-64-accton-as7712-32x-r0"`, `MODEL = "AS7712-32X"`.
- ONL `x86-64-inventec-d6356-r0`: FRU text matches `"Model:"` / `"Serial:"` (psui.c:89,94; fani.c:123,128).
- SONiC `x86_64-cel_ds3000-r0`: fan model compare `"FSP550-20FM"`, `"G1251-0550WNA"`, `"FSP550-29FM"`, `"G1251-0550WRA"` (pddf/sonic_platform/fan.py:68,70); airflow `"B2F"`/`"F2B"` vs `airflow` (pddf/sonic_platform/chassis.py:278,280) returning "INTAKE"/"EXHAUST".
- SONiC `x86_64-cel_silverstone-r0`: fan model compare `"TDPS1500AB7C"`, `"TDPS1500AB6E"` (sonic_platform/fan.py:106,108); `IPMI_FRU_MODEL_KEY = "Board Part Number"` (psu.py:19); chassis airflow_type `"FB"`/`"BF"` (chassis.py:422-424).
- SONiC `x86_64-inventec_d6332-r0`: `direction = "FAN_DIRECTION_EXHAUST"/"..._INTAKE"` (fan.py:208,210). SONiC cel eeprom TLV `name = "Vendor Extension"` etc.
- Identity: SONiC devices reuse EXACTLY the accepted candidate's model strings (brand `SONiC platform`); verified by importing both candidates into one scratch catalog: 154 devices; facts 1155 with the SONiC candidate (not 174), `x86_64-accton_as7712_32x-r0` has 2 records. ONL devices: brand `ONL platform`, model = the `PLATFORM` literal (e.g. `x86-64-accton-as7712-32x-r0`; the dir `accton/x86-64/wedge100` has `PLATFORM` literal `x86-64-facebook-wedge100-r0` — kept literal, no company role inferred); 0 affiliations; identifier `onl_platform` / `sonic_platform`.
- Licences (per-source `license` field): SONiC `Apache-2.0` (repo LICENSE), plus a note listing captured files with GPL notice headers where found (8 accton `*_monitor_fan.py` + cel `ds1000_fanctld.py`); ONL `EPL-1.0` as the repo LICENSE states ("Unless otherwise noted, the following license applies"; Debian/Broadcom-derived components keep their own licences; many captured files carry EPL-1.0 headers).
- `AFO` / `AFI`: present only in comments of ONL accton as9716_32d `sysi.c` (lines 146,147,153,179 etc., non-UTF-8 quote bytes shown as \xNN); quoted in `nos_airflow_comments`. No other hits. (The earlier "0 hits" claim was a binary-file grep artefact and is withdrawn.) Other airflow vocabulary: F2B, B2F, FB, BF, INTAKE, EXHAUST, FAN_DIRECTION_*, ONLP_FAN_STATUS_F2B/B2F, PSU_TYPE_*_F2B/B2F.

## Excluded, gaps
- Owned-model check (7760, AS7760, 7772, AZ4444, CS8200, CS8210, DMC-12/DMC_12/DMC12; case-insensitive) over (a) all 601 captured file contents and paths and (b) the full git tree listings of the six SONiC module dirs (1231 paths) and the five ONL vendor platform dirs (5169 paths): ZERO hits. The only raw-substring match is "7772" inside a git blob hash in the SONiC tree listing (not a name). Gap is scoped to those dirs at those commits; ONL dirs of other vendors/arm/powerpc and other SONiC vendor dirs were not searched.
- Ingrasys SONiC python: none exist in the module dir (gap). 9 SONiC module dirs captured but not in the curated file because the platform id could not be matched exactly to an accepted-candidate model and no captured file states it: cel/questone2, cel/seastone2, cel/services (a non-platform `services/platform_api` dir; not attached to a single platform id), quanta/ix7-32x, ix7-bwde-32x, ix8-56x, ix8a-bwde-56x, ix8c-56x, ix9-32x. Their items remain in extraction.json (listed under unmapped_or_skipped_platform_dirs). Name-guessing (questone2 -> questone_2, quanta ix9-32x -> ix9_bwde) was not done.
- Most ONL PSU model matching and fan/PSU counts live in files not in scope (`platform_lib.c/.h`), so 91 ONL records show airflow/PSU symbol lines but often no model strings or counts; 8 platforms yield none of the practical kinds: ONL facebook-wedge100, cel-seastone, quanta ix1b-rglbmc / ix7-rglbmc / ix8-rglbmc / ly4r / ly7-rglbmc; SONiC alphanetworks bes2348t. Silence is not zero.
- No part-number to airflow dict/map exists in any captured SONiC python (0 dict entries of that kind); airflow appears as compares (`"B2F"`, `"FB"`), returns, and ONL symbol lines (`PSU_TYPE_*_F2B`). Quanta eeprom/chassis docstring examples (e.g. 'AG9064') are docstrings, not extracted. 17 py2-syntax `__init__.py` files fell back to line-regex extraction (MODEL/PLATFORM/counts only). Extraction is role-based (compare/array/assign/define) and heuristic; the airflow-token lines are raw source lines.
- Not mapped to hardware identities, SKUs, company roles or installed configurations; directory/vendor names carry no role.

## Checks
- Manifest: 601 files, 0 sha256/byte mismatches, 603 files on disk (+ manifest + curated JSON); all 111 snapshots resolve relative to the JSON's dir.
- Temp catalog (scratch sqlite, Catalog + pipeline.import_source 'curated'): returned 111; sources 111, devices 111, records 111, identifiers 111, facts 483, affiliations 0, integrity_check ok, foreign_key_check empty. Combined with the SONiC candidate: 174 records, 154 devices; facts 1155 with the SONiC candidate.
- No test suite run (no code changed). Network: 2 blobless clones only.

## Decisions needed
1. (Done as specified: one source per repo+platform dir, 111 sources.)
2. Whether the 9 unmapped SONiC dirs should be attached via a later pinned look at their platform ids (needs files outside this scope).
3. Whether `nos_airflow_symbol_lines` (raw source lines, up to ~9KB per fact) is acceptable in the catalog.

## Next (optional, not started)
Capture `platform_lib.c/.h` and `platform.json`-adjacent PSU/fan type tables for ONL accton; SONiC `pddf/*.json` for pddf platforms; other vendors (Dell, Delta, UfiSpace, Mellanox, Arista) in both repos; ONL `packages/base` fan/PSU libs; AS7772/AZ4444 searches in ONIE `machine/` trees.

## Fixes after review (nos-review.md)
- M1: ONL `license` on all 91 sources now states EPL-1.0 per the repo LICENSE text (with Debian/Broadcom exceptions); handoff licence line corrected.
- M2: `nos_count_constants` restricted to constants whose NAME contains PSU/FAN/THERMAL plus NUM/COUNT/TOTAL/MAX, defined at module/class level, in C `#define`s, or as `self.X = <int>` in a class `__init__`; locals, TLV codes, table sizes, `*POWER*` dropped (71 -> 35 devices carry the fact). Name is a code constant, not an installed count.
- M3: AFI/AFO text corrected (above); the literals were already in `nos_airflow_comments` and now decode faithfully.
- S1: non-UTF-8 sources (as9716_32d and one other sysi.c) are decoded byte-faithfully (bytes >0x7f as `\xNN`), with a `nos_non_utf8_files` fact. S2: `"48V"` moved to `nos_psu_name_substring_matched`. S3: ds3000 fan.py:68,70 and silverstone fan.py:106,108 moved to `nos_psu_model_literals_in_fan_direction_code` (read in context: PSU model compared inside the is_psu_fan branch). S4: comments labelled `commented-out code` / `comment naming a different platform` / `comment`. S5: `nos_airflow_symbol_lines` dedups identical lines (locations joined), basename locations; max fact value 5423 chars (was 9104).
- Re-validated: manifest 601 files 0 mismatches (captures and manifest unchanged); 111 snapshots resolve; scratch import of SONiC then NOS candidates: 63 + 111 returned, sources 174, devices 154, records 174, identifiers 174, facts 1155 (672 + 483), affiliations 0, integrity ok, foreign_key_check empty.
