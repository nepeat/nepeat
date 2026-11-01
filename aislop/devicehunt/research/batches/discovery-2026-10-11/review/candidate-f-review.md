# candidate f review (independent, Claude Sonnet 5.5), 2026-10-11 discovery batch wave 10

Verdict: ACCEPT with notes. No must-fix.

Scope: data/devices-candidate-discovery-2026-10-11-f.sqlite, data/import-manifest-candidate-discovery-2026-10-11-f.json, integration stage-f / rebuild-f / compare-f / pre-promotion-suite-f logs, data/rebuilt-discovery-2026-10-11-f.sqlite. Live data/devices.sqlite (d78b0b7e...) and data/import-manifest.json (f1ccce0e...) re-hashed at the end: unchanged. SQLite opened `?mode=ro`; scratch only in the review-candidate-f scratchpad.

## Checks run

1. Manifest vs live. Only `imports`, `files`, `expected` differ; `campaign_gate` identical. imports 144 -> 145, live list is an exact prefix, one appended job: `curated` `raw/discovery-2026-10-11-spec/spec-results-wave10-2026-10-11.json`. files 3756 -> 3906 (+150 = 149 `wave10/jbb2015/*.html` + the supplement): 0 removed, 0 changed; all 150 added hashes recomputed against disk, 0 missing / 0 mismatch. (`wave10/robots/robots.txt` and `wave10/requests-wave10.jsonl` are not in `files`; same treatment as waves 8/9 request logs.) expected: devices 5084 -> 5107, records 7076 -> 7225, sources 1927 -> 2076; every other count identical; nothing dropped. Matches stage-f (+149 sources/records/identifiers/affiliations, facts +4023, devices +23).
2. Preservation and gates. `live EXCEPT main` over all 56 non-FTS tables (same table set as live; the 6 `search_index*` FTS tables skipped): 0 live rows missing. Added rows: sources 149, records 149, affiliations 149, identifiers 149, facts 4023, devices 23; nothing else changes. `review_queue.gate_inputs('data', candidate manifest)` passes (12 release entries); `gate_projections` passes on both the candidate and the rebuilt DB. pre-promotion-suite-f: 671 tests OK, 0 failures/errors/skips, 0 blocked network calls.
3. Input. sha256 of `data/raw/discovery-2026-10-11-spec/spec-results-wave10-2026-10-11.json` = `22514e52de4952268975f4bdfa6acd1482dd25193350b03f9a5972f8bd10452f`, equal to the given sha and to the manifest `files` entry.
4. New vs merged. 149 records land on 60 devices: 37 existing (merged) and 23 new. New sources: all tier `vendor`, sha256 and cache_path 149/149, coverage mentions throughput 149/149. Merged by brand: Oracle 6 (Sun Server X4-2, Oracle Server X5-2L, SPARC T5-2, T7-1, T8-1, SPARC S7-2 with 2 records), Quanta 2 (D51B-2U, D52B-2U), Quanta Cloud Technology 6, Inspur Corporation 8, IEI 7, IEIT 6, Inspur 2. Oracle merges attach to existing `Oracle` brand device ids; no new Oracle device exists (one device per Oracle model, no duplicates). New devices (23, all kind `server`): IEIT I22G7, NF3180G7, NF3280G7, NF5180G7; Inspur NF5180A7, NS5180A7; Inspur Corporation `Inspur NE5260M5`, `NF5266M6`, `NF5280M4`, `NF5280M7`, `NF5288M5`, `NF5468A5`, `NF5484M6`, `NF8480M4`, `NF8480M5`, `TS860G3`, plus `NF3180A6`, `NF5280A6`, `NF5468A5`, `NP3020M7`, `NS3160A6`; Inspur PowerSystem Corporation FP5280G2, FP5290G2. Device brand/model/revision/title/kind: 0 of 5084 live devices changed. Affiliations all role `brand` except Quanta Cloud Technology (19) and one Quanta Computer Inc. row, `reported_system_vendor`; identifiers all `spec_system_name`.
5. vendor-inventory, 13 TARGETS x 3 scopes, `--limit -1 --format json`, live vs candidate. manufacturer scope 13/13 byte-identical; factory 13/13 byte-identical; all scope 10/13 identical, 3 differ as expected (evidence on existing or brand-only identities, no role beyond brand/reported_system_vendor):
   - Oracle all: identities 1281 -> 1281, source_records 1610 -> 1617, sources 99 -> 106, role_assertions 2453 -> 2460 (6 existing identities gain evidence).
   - Quanta all: identities 175 -> 175, source_records 289 -> 291, sources 202 -> 204, assertions 295 -> 297 (D51B-2U, D52B-2U).
   - Inspur all: identities 221 -> 223, source_records 380 -> 384, sources 238 -> 242, assertions 510 -> 514 (new `Inspur` NF5180A7, NS5180A7; existing `Inspur` NF5280A7, TS860G7 gain evidence).
   Manufacturer/factory inventory for Oracle, Quanta and Inspur is unmoved.
6. Owned baseline: `validate-inventory` candidate total 10 / hits 10 / gaps 0, rc 0; equal to live apart from `eligible_sources` (1920 -> 2069, +149). ByteDance `validation/vendor-examples.json`: 0 hits / 2 gaps, rc 2 on live and candidate, equal apart from `eligible_sources`.
7. Integrity / rebuild. Candidate: integrity ok, FK empty. Rebuilt DB: integrity ok, FK empty. Two-way EXCEPT candidate vs rebuilt: devices, affiliations, identifiers, facts, sources (ex fetched_at) 0/0; records identical on id/device/source/raw_json, differing only in `observed_at` (build timing, same treatment as e). compare-f.log: "All 52 tables/indexes and baseline results match.", rc 0. rebuild-f rc 0 (verified_files 3906).

## S1 / S2 fix verification (not previously re-reviewed)

- `spec_jbb_mode` present on 149/149 devices (MultiJVM 125, Composite 23, Distributed 1), each with its own evidence entry (e.g. quote `SPECjbb2015-MultiJVM`, locator page header row 0 cell 1); 149 facts in the DB.
- jOPS values carry the mode label: for all 149, `spec_result_max_jops` / `spec_result_critical_jops` read `N SPECjbb2015-<mode> max-jOPS|critical-jOPS` with `<mode>` equal to `spec_jbb_mode`; 0 mismatches. I also re-checked the quoted number and mode token of every jOPS/mode evidence quote against the saved HTML capture text: 0 misses.
- Coverage: all 149 say max/critical jOPS are "benchmark throughput results for the stated SPECjbb2015 mode ... not hardware specifications", and that modes are not comparable.
- hw_2: `grep hw_2` finds it in exactly one of the 149 captures, jbb2015-20160705-00111 (Sun Server X4-2L, Oracle Corporation); its coverage carries the sentence that hw_2 is a different system, NOT extracted, none of its fields attributed. The device is Oracle `SPARC S7-2`, extracting hw_1 only. Correct.

## Lookups (`--db`, `lookup Q --limit -1 --format json`, live -> candidate)

- `SPARC S7-2`: catalog 44 -> 44 rows, same device ids; Oracle SPARC S7-2 evidence 1 -> 3, `matched_in` now `identity; facts[3]: spec_model, spec_system_name, spec_system_title_line`. SPARC T5-2 (fuzzy neighbour) gains one record. All 29 other datasets identical.
- `Sun Server X4-2`: 6 -> 6 rows; Oracle Sun Server X4-2 evidence 7 -> 8, facts[5] -> facts[8] (+3 SPEC names), cited-path suffix unchanged. Other datasets identical.
- `CS8200`: catalog rows identical (5); only dataset-level `stored_rows` and `sources` change; all other datasets identical.

## Findings

Must-fix: none.

Should-fix (non-blocking):

1. Lookup `sources` on the catalog dataset is dataset-wide (grows for CS8200 though no row changed). Same cosmetic carry-over as candidate e should-fix 1.

Notes:

N1. Wave 10 does not add manufacturer or factory inventory for any target. Do not describe it as Oracle/Quanta/Inspur manufacturer evidence; it is submitter-printed system naming and benchmark throughput, with all-scope association evidence only (Oracle +7 records, Quanta +2, Inspur +4 / +2 identities).
N2. Brand-split persists by design: the same Inspur product appears under `Inspur`, `Inspur Corporation` (with and without the "Inspur " model prefix: e.g. NF5468A5 and `Inspur NF5468A5`, NF3180A6, NF5280A6), IEIT and IEI. Two Inspur PowerSystem Corporation FP5280G2/FP5290G2 devices sit beside the existing `Inspur Power Commercial Systems Co., Ltd.` FP5280G2/FP5290G2. A later cross-brand alias pass could merge them; not required here.
N3. The six oldest Inspur pages were skipped for budget (handoff); 244 Inspur CPU2017 texts remain a lead. Coverage is partial by design.
N4. `eligible_sources` +149, all with sha256 + cache_path. Owned baseline stays 10/10; ByteDance stays 0/2.
