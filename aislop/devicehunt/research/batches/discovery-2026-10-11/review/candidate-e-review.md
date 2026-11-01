# candidate e review (independent, Claude Sonnet 5.5), 2026-10-11 discovery batch wave 9

Verdict: ACCEPT with notes. No must-fix.

Scope: data/devices-candidate-discovery-2026-10-11-e.sqlite, data/import-manifest-candidate-discovery-2026-10-11-e.json (f1ccce0e...), integration stage-e / rebuild-e / compare-e / pre-promotion-suite-e logs, data/rebuilt-discovery-2026-10-11-e.sqlite. Live data/devices.sqlite (09891954...) and data/import-manifest.json (a2623161...) re-hashed at the end: unchanged. SQLite opened `?mode=ro`; scratch only in the review-candidate-e scratchpad.

## Checks run

1. Manifest vs live. Only `imports`, `files`, `expected` differ; `campaign_gate` identical.
   - imports 143 -> 144; live list is an exact prefix; one appended job: `curated` `raw/discovery-2026-10-11-spec/spec-results-wave9-2026-10-11.json`.
   - files 3605 -> 3756 (+151 = 150 `wave9/cpu2017/*.txt` captures + the supplement). 0 live entries removed or changed. All 151 added hashes recomputed against disk (paths are relative to data/): 0 missing, 0 mismatch. The only wave9 file on disk and not in `files` is `wave9/requests-wave9.jsonl` (request log); same treatment as `wave8/requests-wave8.jsonl`, which is also absent from the manifest.
   - expected: devices 5067 -> 5084, records 6926 -> 7076, `sources` map 1777 -> 1927 (150 new); nothing dropped. Matches stage-e (sources/records/identifiers/affiliations +150, facts +2548, devices +17).
2. Preservation and gates. `live EXCEPT main` on all 62 tables (same table set): 0 live rows missing in the 57 non-FTS tables (the 5 `search_index*` FTS tables are rebuilt and skipped). Added rows: affiliations 150, devices 17, facts 2548, identifiers 150, records 150, sources 150; nothing else grows. `review_queue.gate_inputs('data', candidate manifest)` passes; `gate_projections(candidate, release)` passes. pre-promotion-suite-e: 671 tests OK, 0 failures/errors, 0 blocked network calls.
3. Input. sha256 of `data/raw/discovery-2026-10-11-spec/spec-results-wave9-2026-10-11.json` = `70f37903d88d679c0050a53200c8b9931f5838d9c1d047d6e418c0703eb024b1`, equal to the given sha and to the manifest `files` entry.
4. Attach vs new. 150 records land on 30 devices: 13 existing (NF5180M6 16, NF5180M7 19 (brand IEIT), NF8480M6 5, NF5280M7 4 (IEI), Inspur NF5280A7 1 (IEI), Inspur NF5180A7 1, NF5180A7 4, Inspur NF5280M5 1, NF5466M7 10, NF5688M7 1, NF5280A7 4, NF5280M7 19 (IEIT), NF5180M7 4 (IEI)) and 17 new devices, all kind `server`. Device brand/model/revision/title/kind: 0 of 5067 live devices changed (all five columns compared). New sources: all tier `vendor`, sha256 and cache_path present on 150/150, coverage text states "submitter's printed system/vendor label ... not proof of manufacture or deployment". Identifiers all `spec_system_name`; affiliations all `brand` (test sponsor equals hardware vendor), target None (IEIT 64, Inspur Corporation 74, Inspur Electronic Information Industry Co., Ltd. (IEI) 12).
   - New devices: IEIT NF8480M7, NF3180A7, NF3280A7, NF8260M7, i24A7, i24M7; IEI I24M7, NF5270M7; Inspur Corporation `Inspur NF5466M6`, `NF8260M5`, `NP3020M7`, `NF5280M6`, `NF5280A6`, `NF5280A7`, `NF8260M6`, `NF3280A6`, `NF3180A6`.
5. vendor-inventory, 13 TARGETS x 3 scopes, `--limit -1 --format json`, live vs candidate: all 39 outputs byte-identical (manufacturer 13/13, factory 13/13, all 13/13), including Inspur (all scope 744 KB) and Quanta. Expected: new rows carry target None under brands not in TARGETS aliases (brand-split, see c N3 and d N1); the Inspur target is unchanged.
6. Owned baseline: `validate-inventory` candidate total 10 / hits 10 / gaps 0, rc 0 (document_snapshot 9, label_photo_only 1, unresolved 0); equal to live apart from `generated_at` and `eligible_sources`. ByteDance `validation/vendor-examples.json`: 0 hits / 2 gaps, rc=2 on live and candidate, equal apart from `eligible_sources`.
7. Integrity / rebuild. Candidate: integrity ok, FK empty. Rebuilt DB (devices 5084, records 7076, sources 1927, verified_files 3756): integrity ok, FK empty. Two-way EXCEPT candidate vs rebuilt on devices, affiliations, identifiers, facts, records (id, device, source, key, url, raw_json) and sources (excluding fetched_at/records-timing): 0/0 for every table. compare-e.log: "All 52 tables/indexes and baseline results match.", rc=0. rebuild-e rc=0.

## Lookups (`--db`, `lookup Q --limit -1 --format json`, live -> candidate)

- `NF5280M7`: catalog stored_rows 5067 -> 5084, matches 6 -> 6 (same 6 devices, no new row). Evidence grows on two: IEIT NF5280M7 6 -> 25 SPEC records (all 6 old retained), IEI NF5280M7 14 -> 18 (old 14 retained). `matched_in` now reads `identity; facts[3]: spec_model, spec_system_name, spec_system_title_line` with `matched_in_detail` {identity: title/model/identifier, fact_name_count 3}: same-record fact hits are collapsed by distinct fact name. The entity-manager and OpenBMC rows show the new `cited-path only: N fact names` suffix (7 and 8) separate from the matched names, so path citations are no longer counted as corroboration. Other datasets unchanged.
- `NF5466M7`: matches 1 -> 1; IEIT NF5466M7 evidence 3 -> 13 (old 3 retained); `matched_in` `identity; facts[2]: spec_system_name, spec_system_title_line`.
- `CS8200`: catalog rows identical (5: CS8200-32X plus four RRA CS820x-32X listings); only dataset-level `stored_rows` and `sources` listing change. All other datasets identical.
- Wave 8's d should-fix 1 (matched_in over-listing) is resolved in this build.

## Findings

Must-fix: none.

Should-fix (non-blocking):

1. The lookup `sources` field on the catalog dataset is the full dataset source list (1675 -> 1825 here), so it grows by 150 for every query including ones such as CS8200 that gained no rows. It reads as if the query touched 150 new sources. Consider restricting it to sources cited by the shown rows, or labelling it as dataset-wide. Cosmetic, pre-existing behaviour.

Notes:

N1. No target inventory movement. Inspur and the other 12 targets are byte-identical in all scopes; do not describe wave 9 as adding Inspur target inventory. The 17 new and 13 merged devices sit under non-alias brand strings (`IEIT Systems Co., Ltd.`, `Inspur Corporation`, `Inspur Electronic Information Industry Co., Ltd. (IEI)`).
N2. Brand-split persists: the same product appears under several brand strings (e.g. NF5280A7 as IEIT `NF5280A7`, IEI `Inspur NF5280A7` and Inspur Corporation `Inspur NF5280A7`; I24M7 under IEI and i24M7 under IEIT). These are by design records of the printed SPEC vendor/system label and are not merged; the "Inspur " prefix stays in the model where SPEC prints it, with the printed full name kept as `spec_system_name`.
N3. Evidence is submitter-printed system naming only, not manufacture or deployment proof; coverage text says so on all 150 sources.
N4. `eligible_sources` grows by 150 (all with sha256 + cache_path). 
N5. Promotion note wording: wave 9 adds SPEC CPU2017 vendor labels for Inspur/IEIT servers only; owned baseline stays 10/10, ByteDance stays 0/2.
