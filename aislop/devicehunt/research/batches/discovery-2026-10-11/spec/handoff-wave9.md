# spec_results wave 9 handoff — 2026-10-11 (Claude Sonnet 5.5)

## Outcome
Second supplement to the promoted SPEC candidates (428c78f8…, wave 8 19f14b18…; all their files untouched). Fetched the newest unfetched CPU 2017 text result per distinct (test sponsor, system name) for Inspur (all printed variants) and IEIT, newest first, up to the cap: **150 new requests** (cap 150), all HTTP 200, no 403/429/5xx, no retries, >=10 s spacing, plain UA, no `/cgi-bin/`. No robots re-fetch (last one was wave 8, ~30 min earlier). Combined SPEC total 454 requests. Of 394 remaining unfetched distinct names, the 150 newest were taken: **all 64 IEIT names** (the newest overall) plus 86 Inspur names (74 `Inspur Corporation`, 12 `Inspur Electronic Information Industry Co., Ltd. (IEI)`); **244 older Inspur names remain unfetched** (lead).

## Files and hashes
- Captures `data/raw/discovery-2026-10-11-spec/wave9/cpu2017/*.txt` (150).
- `capture-manifest-wave9.json` sha256 `e3ab63eb80215c59535327a6ce81affe6be1efc840e3374ec214284ca5af3051` (all 150 files re-hashed vs disk: 0 mismatches); log `wave9/requests-wave9.jsonl` sha256 `2e1e484660b9de5e4c154373a87a12d9ac8590a43acddc1ef71b934282b52646`.
- Candidate `spec-results-wave9-2026-10-11.json` sha256 `70f37903d88d679c0050a53200c8b9931f5838d9c1d047d6e418c0703eb024b1` (curated, in place; 150 sources / 150 device rows / 2548 facts; source ids new and disjoint from the promoted and wave 8 candidates, checked).
- Scripts: `research/batches/discovery-2026-10-11/spec/scripts/{fetch9,run9,build9}.py`.

## Identities and rules (revision-3 conventions)
Brand as printed: `Inspur Corporation` (74), `IEIT Systems Co., Ltd.` (64, never mapped to Inspur), `Inspur Electronic Information Industry Co., Ltd. (IEI)` (12); none is a reviewed TARGETS alias, so no canonicalisation. Role `brand` in all 150 (Test Sponsor equals Hardware Vendor). Tier `vendor`, kind `server`. Model = printed name minus trailing CPU/config parenthetical (150 of 150 stripped; no non-CPU parenthetical existed). 30 distinct (brand,model). Full printed name kept as `spec_system_name` identifier and fact; per-line `spec_result_<label>` metric facts; no None; coverage states the rules. No shared-enclosure PSU facts apply (CPU 2017 text). Brand-prefixed models (`Inspur NF5280M5`, etc.) are not normalised (as in review note N2).

## Checks
- Quote check 2398/2398 found in captures, each fact value equal to or inside its quote, no `None`.
- Owned-model check (CS8200, CS8210, AZ4444, AS7760, standalone 7760, DMC-12, AS7772, SV1120/1121/2121/2221, ByteDance, B4020, B4010, Oracle E4-2C/E5-2C/X7-2C/X10-2C) over the 150 texts and the candidate: zero hits.
- Temp import into a scratchpad COPY of the CURRENT data/devices.sqlite (sha 09891954…): returned 150; sources 1777 -> 1927, devices 5067 -> 5084 (**17 added, 13 merged**), device_facts 67503 -> 70051; integrity_check ok, foreign_key_check empty. `vendor-inventory --scope manufacturer` and `--scope all` (`--limit 0`) identical before/after.
- Merges into existing identities (spec records added / records already there): `IEIT Systems Co., Ltd.` NF5180M7 19/5, NF5280M7 19/6, NF5466M7 10/3, NF5180A7 4/2, NF5280A7 4/2, NF5688M7 1/2; `Inspur Electronic ... (IEI)` NF5280M7 4/14, NF5180M7 4/14, `Inspur NF5280A7` 1/1, `Inspur NF5180A7` 1/1; `Inspur Corporation` `Inspur NF5180M6` 16/3, `Inspur NF8480M6` 5/1, `Inspur NF5280M5` 1/8. (Prior records are the earlier SPEC revisions in the same brand strings.)

## Gaps
244 distinct older Inspur names remain unfetched; SPECjbb2015 still has no text results.
