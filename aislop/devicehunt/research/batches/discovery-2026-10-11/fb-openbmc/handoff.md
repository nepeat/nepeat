# fb_openbmc handoff — 2026-10-11 (Claude Sonnet 5.5)

## Outcome
`facebook/openbmc` default-branch HEAD `94594682123991e330709fd814b256f023f2ddcf`, one blobless depth-1 sparse clone, cloned 2026-10-11T09:09:34Z. No 403/429, no other network. Only `meta-facebook/` carries BMC platforms (other top-level dirs: meta-aspeed, meta-delta, meta-evb, meta-nuvoton, meta-zephyr, not scanned: out of assignment).
- 57 `meta-facebook/meta-*` dirs: 53 platform layers + 4 non-platform dirs (`meta-backports`, `meta-python3`, `meta-qemux86`, `meta-qemux86-git`, excluded). Table below.
- **15 layers selected** (tier a or b), 19 machine identities (one per non-`*-upstream-overrides` conf stem; brand `Meta OpenBMC platform`, identifier `fb_openbmc_platform`, `affiliations: []`, tier `project`): acctonbmc, bletchley, bletchley15, catalina, clemente, celesticabmc, clearcreek, cmm, elbert, emeraldpools, fby2, fbgp2, northdome, fuji, minipack, wedge, wedge100, wedge400, yosemite. Candidate: 19 sources (snapshot = the machine conf) / 19 devices / 281 facts, max fact 3956 chars (review revision 2026-10-11: S1-S3 applied).
- Honest disclosure: to grep locally I widened the sparse set to all of `/meta-facebook/` (6513 files, 6.6 MB) after the first narrow checkout, then copied out only the 76 used files (+ `meta-facebook/COPYING`, repo `LICENSE`) and deleted the clone. An early `git ls-tree -l` on the blobless clone triggered lazy blob fetches (killed); not a per-file network pattern beyond the clone itself.

## Files and hashes
- `data/raw/discovery-2026-10-11-fb-openbmc/capture-manifest.json` sha256 `6544a89abc16658d216d217f74607438fb88b928a97dd03aaaa99247e6b8f3e8` (79 entries: repo, commit, clone UTC, per file path/sha256/bytes/permalink at the commit; includes the locally generated `layer-survey.json`, flagged).
- `.../fb-openbmc-platform-facts-2026-10-11.json` (curated candidate; `snapshot` relative to its own dir) sha256 `1040857147f35f921b27684bc4677de9d29a9cea2ba2d39ea09637a79fce8c97`.
- `.../layer-survey.json` (locally generated from checkout/tree objects, not upstream) sha256 `9e6ae029d6dc2999265035c3a4aeb8ecbab5f0b575347fcc04520fd35e5b1d1b`.
- Captures under `.../openbmc/<commit>/meta-facebook/<upstream relpath>` (spec files at `meta-facebook/spec/...`), plus `openbmc/<commit>/LICENSE` and `meta-facebook/COPYING`.
- Scratchpad `fb-openbmc/{build,cand,finalize,leads}.py`, `owned-check.json`, `leads.json`, `allpaths.txt` (full tree listing, 11197 paths), `t.sqlite` (temp import).

## Selection rule and tiers
Scan of every text file <=200 KB in `meta-facebook/` excluding `.patch/.diff/.bb/.bbappend/.inc/.cfg/dts/defconfig/layer.conf` and the generic JEDEC vendor table `meta-yosemite4/.../libcxl.cpp` (a manufacturer-id list, not platform identity). Tiers: **a** target vendor string (Accton, Edgecore/Edge-Core, Celestica, Quanta/QCT, Wiwynn, Wistron, Inventec, Ingrasys, Foxconn/Hon Hai/Fii, Inspur, Hyve) on a line that is not an attribution line; **b** literal PSU part/device string (`ECD########`, `PS-####-#*`, `PFE####*`/`pfe####`, `D1U54*`, `DD-####-*`, incl. hex-byte arrays); **c** PSU/fan maker word only (Delta, Liteon, Murata, Bel) - recorded as `fbobmc_psu_maker_word_as_printed`, not a selection reason; **d** attribution only (Copyright/MODULE_AUTHOR/@vendor email) - not selected, not captured. Per selected layer: machine confs (incl. nested), README, mapped `spec/<name>/OVERVIEW.md|*.yaml`, hit files, `board-utils.sh`/`eeprom.json`/`fsc_board.py`/ipmi-lite `config.json`, cap 15 (cap never reached). Evidence scope is layer-level (`fbobmc_evidence_scope`); a hit file is not proven machine-specific for each conf in a multi-conf layer.

## Layer table (generated from the scan)
| layer | confs | files | tier | decision |
|---|---|---|---|---|
| acctonbmc | 1 (acctonbmc) | 51 | a | selected, tier a |
| anacapa | 2 (anacapa-upstream-overrides, anacapa) | 70 | none | not selected: no vendor/part string in scanned text |
| aristabmc | 1 (aristabmc) | 76 | none | not selected: no vendor/part string in scanned text |
| backports | 0 () | 135 | none | non-platform dir (excluded) |
| bletchley | 2 (bletchley, bletchley15) | 25 | a | selected, tier a |
| catalina | 3 (catalina, clemente-upstream-overrides, clemente) | 89 | a | selected, tier a |
| celesticabmc | 1 (celesticabmc) | 54 | a | selected, tier a |
| churchillmono | 1 (churchillmono) | 28 | none | not selected: no vendor/part string in scanned text |
| clearcreek | 1 (clearcreek) | 87 | a | selected, tier a |
| cloudripper | 1 (cloudripper) | 134 | c | not selected: maker word only (Delta/Liteon etc.) |
| cmm | 1 (cmm) | 53 | b | selected, tier b |
| elbert | 1 (elbert) | 180 | b | selected, tier b |
| fbal | 1 (angelslanding) | 135 | none | not selected: no vendor/part string in scanned text |
| fbdarwin | 1 (fbdarwin) | 64 | none | not selected: no vendor/part string in scanned text |
| fbep | 1 (emeraldpools) | 117 | a | selected, tier a |
| fboss-lite | 0 () | 77 | none | not selected: no vendor/part string in scanned text |
| fbtp | 1 (fbtp) | 131 | none | not selected: no vendor/part string in scanned text |
| fbttn | 1 (fbttn) | 164 | none | not selected: no vendor/part string in scanned text |
| fby2 | 3 (fby2, fbgp2, northdome) | 312 | a | selected, tier a |
| fby3 | 1 (fby3) | 244 | none | not selected: no vendor/part string in scanned text |
| fby35 | 4 (fby35, greatlakes, halfdome, javaisland) | 338 | c | not selected: maker word only (Delta/Liteon etc.) |
| fuji | 1 (fuji) | 163 | b | selected, tier b |
| galaxy100 | 1 (galaxy100) | 95 | none | not selected: no vendor/part string in scanned text |
| grandcanyon | 2 (grandcanyon, grandcanyon2) | 268 | c | not selected: maker word only (Delta/Liteon etc.) |
| grandteton | 3 (grandteton, gtartemis, inspirationpoint) | 386 | none | not selected: no vendor/part string in scanned text |
| harma | 2 (harma-upstream-overrides, harma) | 58 | none | not selected: no vendor/part string in scanned text |
| janga | 1 (janga) | 42 | none | not selected: no vendor/part string in scanned text |
| lfdarwin | 2 (fbdarwin-upstream-overrides, fbdarwin) | 42 | none | not selected: no vendor/part string in scanned text |
| lightning | 1 (lightning) | 113 | none | not selected: no vendor/part string in scanned text |
| meru | 1 (meru) | 73 | none | not selected: no vendor/part string in scanned text |
| minerva | 1 (minerva) | 19 | none | not selected: no vendor/part string in scanned text |
| minilaketb | 1 (minilaketb) | 137 | none | not selected: no vendor/part string in scanned text |
| minipack | 1 (minipack) | 179 | b | selected, tier b |
| minipack3n | 1 (minipack3n) | 33 | d | not selected: attribution lines only (MODULE_AUTHOR/Copyright), not captured |
| montblanc | 1 (montblanc) | 57 | d | not selected: attribution lines only (MODULE_AUTHOR/Copyright), not captured |
| morgan800cc | 1 (morgan800cc) | 42 | none | not selected: no vendor/part string in scanned text |
| netlakemtp | 1 (netlakemtp) | 113 | none | not selected: no vendor/part string in scanned text |
| netlakenext | 1 (netlakenext) | 152 | none | not selected: no vendor/part string in scanned text |
| nexthopbmc | 1 (nexthopbmc) | 39 | none | not selected: no vendor/part string in scanned text |
| python3 | 0 () | 13 | none | non-platform dir (excluded) |
| qemux86 | 1 (qemux86) | 17 | none | non-platform dir (excluded) |
| qemux86-git | 1 (qemux86-git) | 6 | none | non-platform dir (excluded) |
| rainiera7 | 4 (rainiera7-upstream-overrides, rainiera7, rainiera6-upstream-overrides, rainiera6) | 49 | none | not selected: no vendor/part string in scanned text |
| sandia | 1 (sandia) | 25 | none | not selected: no vendor/part string in scanned text |
| sanmiguel | 2 (sanmiguel-upstream-overrides, sanmiguel) | 65 | none | not selected: no vendor/part string in scanned text |
| santabarbara | 2 (santabarbara-upstream-overrides, santabarbara) | 57 | none | not selected: no vendor/part string in scanned text |
| tahan | 1 (tahan) | 42 | none | not selected: no vendor/part string in scanned text |
| ventura | 2 (ventura-upstream-overrides, ventura) | 42 | none | not selected: no vendor/part string in scanned text |
| ventura2 | 4 (ventura2-upstream-overrides, ventura2, ventura2a7-upstream-overrides, ventura2a7) | 55 | none | not selected: no vendor/part string in scanned text |
| waimeacanyon | 1 (waimeacanyon) | 71 | none | not selected: no vendor/part string in scanned text |
| wedge | 1 (wedge) | 92 | a+b | selected, tier a+b |
| wedge100 | 1 (wedge100) | 114 | b | selected, tier b |
| wedge400 | 1 (wedge400) | 212 | b | selected, tier b |
| yamp | 1 (yamp) | 119 | none | not selected: no vendor/part string in scanned text |
| yosemite | 1 (yosemite) | 115 | a | selected, tier a |
| yosemite4 | 2 (yosemite4-upstream-overrides, yosemite4) | 161 | none | not selected: no vendor/part string in scanned text |
| yosemite5 | 4 (yosemite5-upstream-overrides, yosemite5, yosemite5a7-upstream-overrides, yosemite5a7) | 79 | none | not selected: no vendor/part string in scanned text |

Notes: `*-upstream-overrides.conf` fragments are captured where in a selected layer (catalina/clemente) but get no identity. `fbdarwin` is a conf stem in both meta-fbdarwin and meta-lfdarwin (neither selected, no collision in output). Layers with no conf (`fboss-lite`) list 0.

## Identities and practical facts (examples)
Facts are `fbobmc_*` with `path:line: text` as printed: `fbobmc_machine_conf`, `fbobmc_machine_conf_lines` (non-license lines), `fbobmc_conf_name/description/require`, `fbobmc_uboot_machine`, `fbobmc_kernel_devicetree`, `fbobmc_vendor_string_as_printed`, `fbobmc_psu_part_string_as_printed`, `fbobmc_psu_id_hex_bytes_as_printed` (+ separate `fbobmc_observation_hex_ascii_decode`), `fbobmc_psu_maker_word_as_printed`, `fbobmc_board_type_string_as_printed`, `fbobmc_eeprom_platform_line_as_printed`, `fbobmc_spec_overview_lines`, `fbobmc_captured_files`.
- `acctonbmc` / `celesticabmc`: conf lines `#@DESCRIPTION: Machine configuration for Facebook acctonbmc` (conf:20) and `... Facebook Celestica-BMC` (celesticabmc.conf:20); `"platform" : "acctonbmc"` in eeprom.json:2. The vendor token here is part of the layer/machine name: a label, no role.
- `catalina`: `spec/catalina/OVERVIEW.md:22: ODM partners are Quanta and Ingrasys.` (printed claim in a Meta spec, recorded as a printed string, not a role fact); OVERVIEW:3 "Catalina is Meta's NVIDIA GB200 NVL72 compute tray".
- `wedge400`: board-utils.sh:52-53 `DELTA_48V="0x45 0x43 ..."` / `LITEON_48V="0x44 0x44 ..."` (observation, not fact: ASCII decode = `ECD25010015`, `DD-2152-2L`); board-type echoes `WEDGE400_MP_RESPIN`, `WEDGE400-C_EVT` (:81,:122). `fuji` has the same hex strings (:82-83) and `echo "FUJI"` (:191).
- `minipack`: psu_driver.c:130-142 `/* PSU model name: ECD55020006 */`, `PS-2152-5L`, `PFE1100-12-054NA`, `PFE1500-12-054NACS457`, `D1U54P-W-1500-12-HC4TC-AF`; minipack-psu.h:50,62,65,68 `DELTA_MODEL`/`LITEON_MODEL`/`BEL_MODEL`/`MURATA_MODEL`.
- `elbert`: elbert-psu.h:46 `DELTA_MODEL "ECD15020056"`, :63 `LITEON_MODEL "PS-2242-9A"`; `meta-elbert/recipes-utils/openbmc-utils/files/psu_show_tech.py:238-241` maps these to `PWR-2421-HV-RED`, `PWR-2411-AC-RED`, `PWR-2422-HV-RED` (and `PWR-00591` at :241).
- `wedge`/`wedge100`: `i2c_device_add 7 0x59 pfe1100 # wedge power supply` (wedge setup_i2c.sh:47); wedge100 setup_i2c.sh:32 "2 PFE1100 power", psumuxmon.py:312 `"pwr_type": "pfe1100"`. `cmm`: `chip "pfe3000-*"` (cmm.conf:346), `pfe3000` psu1-4 (setup_i2c.sh:184-187). `wedge` also `ipmid.c:215: // OEM Command Codes (Quanta/FB defined commands)`.
- Quanta comment `/* Quanta BMC correction table */` (clearcreek pal_calibration.h:6, fbep:5, yosemite pal.c:225); fby2 bic.c:588 `// WiWynn IANA`; bletchley spec/FEATURES.md:167,222 `facebookexternal/openbmc.quanta#...` (TODO repo refs). All are strings in files, not claims about who built what.
- **Shared-layer PSU strings (not attached to a platform; captured, no identity):** `meta-facebook/recipes-kernel/i2c-dev-psu-mod/files/psu.c:149-168` (`ECD55020006`, `ECD15020060`, `PS-2152-5L`, `PS-2202-6L`, `PFE1100-12-054NA`, `PFE1500-12-054NAC`, `D1U54P-W-1500-12-HC4TC`, `D1U54T-W-2000-12-HC4TC-FB`, `DD-2152-2L`, `ECD25010015`); `recipes-utils/pem/files/pem.h:43 DELTA_MODEL "ECD24020003"`.

## Review fixes applied (fb-openbmc-review.md S1-S3)
- S1: acctonbmc/celesticabmc name-token hits are now `fbobmc_layer_name_token_as_printed` (each item prefixed "layer/machine name token, not a vendor statement"); their `fbobmc_selection_tier` reads "selected on a name token only".
- S2: layer-level facts (vendor/PSU/board-type/eeprom strings, captured file list) are attached once to the primary conf (bletchley, catalina, fby2; single-conf layers unchanged). Sibling confs (bletchley15, clemente, fbgp2, northdome) carry conf facts plus one `fbobmc_layer_shared_evidence` note naming the primary; clemente carries only its own `spec/clemente/OVERVIEW.md` lines, catalina only `spec/catalina/`. The fby2 `bic.c:588` WiWynn comment sits in a branch (`TYPE_SPB_YV250`); it is on `fby2` only.
- S3: the cmm Celestica copyright line (fand.cpp:4) and the injected `[attribution context]` suffix are removed; attribution lines are no longer recorded in any fact (cmm is selected on PSU strings, tier b).
- N3: elbert path fixed above. N1: the manifest `method` also should note the killed `git ls-tree -l` lazy blob fetches (stated here and in Outcome). N2: **worker-reported, not re-derivable from the raw dir** (the widened checkout was deleted): the 11197-path listing, tier scan over 6513 files, the layer table's non-selected rows and tiers c/d, the whole-checkout owned-model scan (8 hex coincidences), and the `wedge100bf` search. Candidate facts do not depend on them. Owned-model check over the raw dir itself: zero.
- Re-check: line-literal check over all cited items in all facts: 693/693 match the captured bytes at the cited line; no `[attribution context]` text left.

## Excluded claims, conflicts, gaps
- Vendor strings are labels; no company roles, no `affiliations`, no mapping of Meta platform names to catalog hardware, SKUs or other repos' names. Delta/Liteon/Murata/Bel/Artesyn appear only as printed PSU-maker words/constants; PSU/fan population per platform is not claimed (code branches list models the software recognizes).
- Near-miss (maker word only, not selected): cloudripper (`_DELTA_PSU_PLATFORM_H_` include guard), fby35 (`Delta module` comments, pal_sensors.c/h), grandcanyon (`use DELTA fan table`, `FSC_GC*_DELTA_v1_config.json` fan tables, not captured). Attribution-only (not captured): minipack3n, montblanc (celestica.com in MODULE_AUTHOR), plus acctonbmc/fuji/cmm/shared ltc4282.c, max6615.c; patch-header @quantatw/@wiwynn/@fii-foxconn/@accton emails across many layers are ignored by rule (kernel patches are out of scope). So not all Quanta/Wiwynn-authored layers (anacapa, harma, catalina patches, netlakenext...) are selected.
- No vendor string at all (and no machine-hardware identity in text) for wedge-family layers beyond what is listed; silence is not zero. `meta-fboss-lite` has no conf. Spec dirs mapped to layers by name: anacapa, bletchley, catalina(+clemente), harma, minerva, santabarbara, yosemite4, yosemite5, greatlakes/javaisland -> fby35, inspirationpoint/grandteton -> grandteton; only those in selected layers are captured (bletchley, catalina/clemente).
- Not scanned: other top-level dirs of the repo; `.patch`/binary files; contents of non-captured files beyond the scan above.
- **Owned-model check** (CS8200, CS8210, AZ4444, 7760/AS7760, DMC-12/DMC_12/DMC12, AS7772, SV1120/SV1121/SV2121/SV2221, ByteDance, B4020, B4010; case-insensitive): captured files contents+paths (81 files): zero. Full repo path listing (11197 paths from tree objects, offline): zero. Contents of the whole `meta-facebook/` checkout (6513 files, before narrowing): 8 hits, all hex-digit coincidences (`7760`/`b4020` inside sha256 strings in `satellite-relay-crates.inc`, `Cargo.lock`, an aspeed commit URL, and a number table in max6615.c): zero real. Scope: that checkout only.

## Cross-reference leads (read-only `data/devices.sqlite`; exact case-insensitive match on model/title, identifier value, fact value; leads only, no merge)
- `Minipack` equals device models `Minipack` (brand Accton, id 5ba9e67c80250f7776d5, switch) and `Minipack` (brand Edge-Core, id 21b54a03177aac1299d0).
- `Wedge400` equals identifier `model_search` `Wedge400` (record 3a56d5cdc992d713368782ba, device model `Wedge 400`, brand Celestica): spacing differs from `wedge400` conf stem; name coincidence only.
- `Catalina` equals device model `Catalina` (brand Amazon Data Services, id 0cfba60cca3eecb44113): name coincidence, different context (Meta spec says GB200 compute tray).
- `Wedge100BF-32QS` (Accton id 5ae8fcaf59976fb0ce4c, Edge-Core eebf9147ff0cfecfcb59) exists in the catalog but does NOT appear in this repo (searched all 6513 meta-facebook files, case-insensitive `wedge100bf`: zero). Wedge100 here is `wedge100` (no BF suffix); no link made.
- Vocabulary overlaps (not identity leads): `Accton`, `Celestica`, `Quanta`, `Ingrasys` equal existing `registry_brand`/`subsidiary_name_as_printed` fact values.
- No catalog match: wedge100, wedge, fuji, elbert, yamp, cloudripper, bletchley, clemente, cmm, clearcreek, yosemite, fby2, PFE1100, PFE3000, every ECD*/PS-*/PFE*/D1U54*/DD-* string, `Celestica-BMC`, `acctonbmc`.

## Checks run
- Manifest: 79 entries, 0 sha256/byte mismatches; 81 files on disk = 79 manifest entries + the candidate + the manifest itself; all 19 snapshot paths exist.
- Temp import (scratchpad `fb-openbmc/t.sqlite`, `pipeline.import_source(c,'curated',...)` in place): returned 19; sources 19, devices 19, records 19, identifiers 19, facts 281, affiliations 0; integrity_check ok; foreign_key_check empty. `data/devices.sqlite` and `data/import-manifest.json` untouched. No test suite run (no code changed).

## Decisions needed
1. Resolved per review: per-conf identities, layer-level facts on the primary conf only.
2. Whether near-miss tier-c layers (fby35, grandcanyon, cloudripper) deserve a later bounded capture, and whether the shared PSU part-number table should become a software-reference identity of its own.
