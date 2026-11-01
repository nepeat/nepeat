# nos_platform_code independent review (Claude Sonnet 5.5), 2026-10-10

Verdict: ACCEPT with notes, conditional on fixing M1 and M2 (data text) before import. M3 is handoff text.

## Checks run
- Manifest: 601 entries, 601 files on disk, 0 extra, 0 sha256/byte mismatches. Upstream spot-check (1 raw.githubusercontent fetch, ONL as9716_32d sysi.c at 28f52e6): sha256 equals saved bytes. Fetched ONL and SONiC LICENSE at pinned commits (2 more).
- Locations: parsed every `(file:line[-line])` in all 517 facts (1532 locations). All files exist; every quoted literal appears in the cited line(s). 74 apparent misses are all `#define NAME<ws>VALUE` rendered `NAME = VALUE` (whitespace only), so literal. Covers far more than 30 facts, both repos, accton/cel/inventec/ingrasys/quanta-era vendors. Hand-read context for ds3000 fan.py:68-70, silverstone chassis.py:415-425, as7946 psui.c:74, as7712 fani.c:152, as7712 psu.py:46, ds1000_fanctld.py:282, silverstone ONL __init__.py:19.
- Identity: 20 SONiC models all exactly equal accepted-candidate models (brand `SONiC platform`); 91 ONL models unique, brand `ONL platform`, model = PLATFORM literal (wedge100 dir keeps `x86-64-facebook-wedge100-r0`); 0 affiliations; no hardware-identity merge; coverage text disclaims vendor spec / installed config / company role.
- Excluded 9 SONiC dirs: justified and recorded (handoff line 33, extraction.json unmapped_or_skipped_platform_dirs). None of the 9 captured dirs contains an `x86_64-...-r0` id string, so mapping would be name-guessing (e.g. quanta ix7-32x vs accepted ix7_bwde/ix7_rglbmc). Correct to exclude.
- Owned-model gap: own grep (7760, 7772, AZ4444, CS8200, CS8210, DMC-12/12/_12) over all captured contents and filenames: only 2 hex-substring hits inside capture-manifest.json hashes. Gap confirmed for captured scope. Note N3 on unsaved tree listings.
- Snapshots: all 111 resolve relative to the JSON's directory.
- Scratch catalog (Catalog + import_source 'curated', SONiC candidate then NOS candidate): returns 63 then 111; sources 174, devices 154 (20 SONiC overlaps merge), records 174, identifiers 174, facts 1189 (672 SONiC + 517 NOS), affiliations 0; integrity_check ok; foreign_key_check empty; every source has cache_path+sha256; no duplicate (brand,model). data/ and git tree untouched.
- Licences: SONiC Apache-2.0 correct (LICENSE says Apache 2.0). 9 GPL-notice files named exactly correct (the 9 files with "GNU General Public License"; no others in the SONiC capture). ONL: see M1.

## Findings

### Must-fix
M1. licence text (all 91 ONL sources, `license` field): says "the repository LICENSE file only points to Debian/Broadcom licence pages and states no single SPDX licence". Upstream LICENSE at 28f52e6 opens with Debian/Broadcom notes but then states "Unless otherwise noted, the following license applies to the files with in the Open Network Linux Repositories: Licensed under the Eclipse Public License, Version 1.0" and reproduces EPL-1.0. Record as upstream states (EPL-1.0 by default per repo LICENSE, with Debian/Broadcom-derived components under their own licences; 268 of 443 captured files carry an EPL-1.0 header). The handoff line repeating the claim must change too.
M2. `nos_count_constants` contains items that are not counts or are not constants (name overstates):
  - `num_good_fans = 0` (x86_64-cel_ds1000, utils/ds1000_fanctld.py:282): a loop-local runtime accumulator reset each iteration; reads as "0 good fans".
  - `count = 0` (x86-64-cel-belgite, platform-config __init__.py:36): helper retry-loop counter, `actual_port_num = 1` (x86-64-cel-silverstone, __init__.py:19 and :39): local variable.
  - `TLV_CODE_MANUF_COUNTRY = 0x2C` (csp9250 sysi.c:122, d10056 sysi.c:65, d5254 sysi.c:62): an ONIE TLV type code, not a count.
  - `ITEM_NUM`/`SYSI_ONIE_TYPE_SUPPORT_NUM`/`SYSI_PLATFORM_INFO_NUM` (d10056 sysi.c:44-49, d5254 :44-46): table sizes of ONIE TLV code arrays, not ports/fans/PSUs.
  Drop these, or rename the fact to something neutral ("numeric constants whose names contain NUM/COUNT"). Also name-only note: `NUM_PSU`, `NUM_OF_PSU_ON_MAIN_BROAD`, `NUM_FAN` etc. are code constants; value text is fine since it is "NAME = N (file:line)", but the fact name `nos_count_constants` should not be read as installed PSU/fan count.
M3. handoff claim "`AFO` / `AFI`: 0 hits in any captured file" is false. ONL accton as9716_32d sysi.c:146,147,153 contains AFI/AFO (the facts themselves quote them in `nos_airflow_comments`). Correct the handoff.

### Should-fix
S1. `nos_airflow_comments` for x86-64-accton-as9716-32d-r0 (sysi.c:146-147): source bytes are 0xA1 0xA7 / 0xA1 0xA8 (non-UTF-8 quotes); the fact stores U+FFFD replacement characters ("��AFI-Back to Front��"). Not literal. Either decode losslessly (latin-1/gb) or note "non-UTF-8 bytes replaced".
S2. `nos_model_part_literals` on x86_64-accton_as7712_32x-r0: `"48V" compared with prod_name` (psu.py:46) is a substring test on the system-EEPROM product name to set PSU type DC, not a model/part literal. Move to another kind or drop.
S3. `nos_model_part_literals` on x86_64-cel_ds3000-r0 (fan.py:68,70): the compared `model` is `psu.get_model()` inside the `is_psu_fan` branch, i.e. PSU models, and the lines map them to EXHAUST/INTAKE. Value does not say whose model it is; add "PSU model (via psu.get_model())" or keep with a note. Same family: silverstone fan.py:106,108 (not re-read for PSU vs fan).
S4. `nos_airflow_comments` carries copy-paste/commented-out text: SONiC accton as7326/as7726/as9716 fan.py:18 says "Since AS4630 psu_fan airflow ... fixed as 'F2B' or 'intake'" (comment names the wrong platform); silverstone chassis.py:116 is commented-out code (`# airflow = self.__get_air_flow()`). They are labeled `comment:` so not overstated, but a consumer should not read them as airflow facts.
S5. Decision 3: `nos_airflow_symbol_lines` up to 9 KB per fact (max fact value 9104 chars). Acceptable but heavy; recommend the integrator keep only the first N lines or accept as is.

### Notes
N1. 8 platforms with only commit/source_files facts: silence, not zero (handoff already states).
N2. Overlap facts: `NUM_PSU = 2` etc. are code constants (chassis.py:24, chassis.py:30); fine as worded.
N3. Owned-model tree-listing check (1231 SONiC + 5169 ONL paths) rests on listings that are not saved; claim is plausible and the saved-capture grep supports a narrower claim. Scope wording ("those dirs at those commits") is correct.
N4. SONiC candidate imports 672 facts into the catalog versus 673 claimed in sonic-review (one fact collapses on insert, e.g. duplicate name+value); not a NOS issue.
N5. Combined import: both candidates share no source ids; the 20 SONiC overlaps get extra `nos_*` facts under the same device row with a separate source, as intended.

## Fix verification (second pass)

Updated verdict: ACCEPT. M1-M3 and S1-S4 closed; no remaining must-fix.

- Hashes: curated sha256 8be73195..., extraction.json c8ea539d... as stated; manifest 601 files, 0 mismatches (captures unchanged).
- M1: all 91 ONL licence fields now read EPL-1.0 per the repo LICENSE ("Unless otherwise noted, the following license applies"), Debian/Broadcom components keep their own licences. Matches upstream text I fetched.
- M2: `nos_count_constants` now 56 items on 35 devices, names only fan/psu/thermal-count (fan_num, NUM_OF_FAN_ON_MAIN_BROAD, NUM_PSU, __num_of_thermals, ...). 16 random items spot-checked across accton/inventec/cel/alphanetworks, ONL and SONiC; all are #define or class-level / `self.X` in `__init__` (e.g. accton monitor_fan.py:70 `self.fan_num = 6`, inventec psu.py:22). No loop-locals, TLV codes or table sizes remain. Note: `__num_of_fans = 1` in inventec psu.py is a PSU class constant; fine as a code constant. `NUM_OF_SENSOR_PER_PSU` is a sensors-per-PSU constant, arguably not a PSU count; acceptable as named.
- M3: as9716_32d AFI/AFO lines are quoted with `\xa1\xa7`/`\xa1\xa8` escapes; `nos_non_utf8_files` present on as9716_32d and as5916_54xm (S1). S2: `nos_psu_name_substring_matched` holds "48V". S3: `nos_psu_model_literals_in_fan_direction_code` on ds3000 and silverstone, labelled PSU model in the is_psu_fan branch (silverstone fan.py:96-108 read: confirmed). S4: comment labels present; commented-out code and wrong-platform (AS4630) comments labelled.
- Re-verification of all 1390 locations (path forms now relative or basename): 1388 literal matches; the other 2 are my matcher tripping on the "comment naming a different platform (AS4630):" label, text is on the line (as7326 / as7726 fan.py:18). Far above 15 random facts.
- 111 snapshots resolve; 91 unique ONL models; identity unchanged.
- Scratch import, SONiC then NOS: 63 + 111 returned; sources 174, devices 154, records 174, identifiers 174, facts 1155 (672 + 483), affiliations 0, integrity ok, foreign_key_check empty.
- Remaining note: S5 reduced (max fact value 5423 chars); N3/N4 unchanged.
