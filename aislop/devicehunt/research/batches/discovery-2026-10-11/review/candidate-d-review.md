# candidate d review (independent, Claude Sonnet 5.5), 2026-10-11 discovery batch wave 8

Verdict: ACCEPT with notes. No must-fix.

Scope: data/devices-candidate-discovery-2026-10-11-d.sqlite (sha256 09891954...), data/import-manifest-candidate-discovery-2026-10-11-d.json (a2623161...), integration stage-d / rebuild-d / compare-d / pre-promotion-suite-d logs, data/rebuilt-discovery-2026-10-11-d.sqlite. Live data/devices.sqlite (baa016d2...) and data/import-manifest.json (d5799672...) re-hashed at the end: unchanged. SQLite opened `?mode=ro`; scratch only in the review-candidate-d scratchpad.

## Checks run

1. Manifest vs live. Only `imports`, `files`, `expected` differ; `campaign_gate` identical.
   - imports 140 -> 143; live list is an exact prefix; appended in order, all `curated`: redhat-cert wave8 detail supplement, nos2 platform facts, spec wave8 results.
   - files 3497 -> 3605 (+108: redhat 22 = 21 `detail-wave8/*.html` + supplement; nos2 26 = 15 Stratum chassis_config + 10 DENT yml + facts; spec 60 = 59 result captures + supplement). 0 live entries removed or changed; all 108 added hashes recomputed against disk: 0 missing, 0 mismatch.
   - expected: devices 5042 -> 5067, records 6821 -> 6926 (+105), 105 new `sources` entries (each count 1); nothing dropped. Matches stage-d (sources/records/identifiers +105, affiliations +80, facts +1573, devices +25).
   - `dent-overlap-not-imported.json`, `capture-manifest.json` and any NOT-FOR-IMPORT file are in neither `imports` nor `files` (dent-overlap and capture-manifest sit on disk under raw/discovery-2026-10-11-nos2 only).
2. Preservation and gates. `live EXCEPT main` on all 62 tables (same table set): 0 live rows missing in the 57 non-FTS tables; only the 5 `search_index*` FTS tables differ (rebuilt). Growth only in sources, devices, records, affiliations, identifiers, facts. `gate_inputs('data', candidate manifest)` passes; `gate_projections(candidate, release)` passes. integrity_check ok, foreign_key_check empty. pre-promotion-suite-d: 669 tests OK, 0 failures/errors, 0 blocked network calls.
3. Inputs. sha256 on disk: Red Hat supplement 32d077a6dc13..., Stratum/DENT facts be679965c0c3..., SPEC supplement 19f14b186cc7...; all equal the given shas.
4. Attach vs new.
   - Red Hat wave 8: 21 records on 21 existing devices (all `Inspur Power Commercial Systems Co., Ltd.`: K1 Power S914/S922/S924/E950/E980/M16/K8820G2/K8828G2/K8870G2/K8880G2/FP8260G2/FP8410G2/FP8480G2, FP5180G2/5290G2/5295G2/5466G2/5468G2/5280G2, CP5466G2/CP5280G2), 0 new devices. Identifier `redhat_certified_product`, role `listing_holder`, target None. Detail facts only.
   - SPEC wave 8: 59 records on 9 existing `Quanta Cloud Technology` devices (D55Q-2U, D54Q-2U, D43K-1U, D53XQ-2U, D44N-1U, D44N-1U AMD EPYC 9755, S44NL-1U, S44NL-1U AMD EPYC 9755, S43KL-1U), 0 new devices, kind `server` unchanged. Identifier `spec_system_name`, role `reported_system_vendor`, target None, same shape as the 21 prior SPEC records.
   - Stratum/DENT: 25 records -> 25 new devices (15 `Stratum platform`, kind `network switch software platform config`; 10 `DENT platform`, kind `network switch software platform definition`). 0 affiliations (no company role implied), identifiers `stratum_platform` / `dent_platform`. All 25 sources: tier `project`, sha256 and cache_path present, coverage states "not a hardware datasheet ... proof of a model's existence". Placeholder printed values (`asdf`, `Stratum device device:s204`, as6712-32x) are flagged in fact text. nos2 re-review M1 (DENT URLs include `packages/platforms`; as4610-30/-54 point to armel with armhf noted) and M2 (DENT licence text not an SPDX claim) are both applied in the DB.
   - Device brand/model/revision/title/kind: 0 of 5042 live devices changed (all five columns compared).
5. vendor-inventory, 13 targets x 3 scopes, `--limit -1 --format json`, live vs candidate: all 39 outputs byte-identical, including manufacturer (13/13) and factory (13/13). The all-scope also does not move (the brief anticipated deltas; there are none). That is consistent with the data: Inspur Power and Quanta Cloud Technology are not TARGETS aliases (target None, matches wave-8 review N1 and spec review), and Stratum/DENT carry no affiliations.
6. Owned baseline. `validate-inventory` candidate = total 10 / hits 10 / gaps 0, equal to live apart from `eligible_sources`. ByteDance `validation/vendor-examples.json`: 0 hits / 2 gaps, rc=2 on both live and candidate, equal apart from `eligible_sources`.
7. Integrity / rebuild. Rebuilt DB: integrity ok, FK empty (devices 5067, records 6926, sources 1777, verified_files 3605). Two-way EXCEPT candidate vs rebuilt on devices, affiliations, identifiers, facts, records (id, device, source, key, url, raw_json) and sources (excluding fetched_at): 0/0 for every table. compare-d.log: "All 52 tables/indexes and baseline results match", rc=0. rebuild rc=0.

## Lookups (`--db`, `lookup Q --limit -1 --format json`, live -> candidate)

- `wedge100bf-32qs`: catalog 5 -> 6; new row `Stratum platform x86-64-accton-wedge100bf-32qs-r0` (kind software platform config, matched_in identity + stratum facts). This is the one Stratum id with no ONL twin (no lead fact). rra unchanged.
- `as4610`: catalog 23 -> 25; new `DENT platform arm-accton-as4610-30-r0` and `-54-r0`. rra 13, documents 2, ntra 4, tec 2, anatel 7, bahrain 1 identical.
- `K8880G2`: catalog 1 -> 1 (existing `Inspur Power Commercial Systems Co., Ltd. K1 Power K8880G2`, gains detail facts `redhat_cert_model_field_as_printed...`); inspur_support identical.
- `QuantaGrid D52BQ-2U`: catalog 3, documents 4, identical. SPEC wave 8 did not touch it.
- `CS8200`: all datasets (catalog 5, rra 2, documents 1, tec 2, anatel 2, bahrain 1) identical.

## Findings

Must-fix: none.

Should-fix (non-blocking):

1. `matched_in` over-listing persists for the new rows (same as candidate c should-fix 1): `DENT platform arm-accton-as4610-30-r0` reports `facts[7]` because the platform id appears in many fact values. Not wrong, but reads as 7 corroborations. Collapse same-record fact hits in a later lookup revision.

Notes:

N1. The brief expected all-scope deltas; there are none. Do not describe wave 8 as adding Inspur or Quanta target inventory. The 21 K1 Power / FP / CP devices and 9 QCT SPEC devices stay outside the Inspur and Quanta targets by brand-alias design (brand-split continues, see candidate c N3).
N2. Stratum/DENT identities are software-reference rows, labelled as such in kind, brand, and coverage. `lookup as4610` / `wedge100bf-32qs` now return them beside real hardware (RRA, NTRA, Anatel) rows; consumers must read kind. 14/15 Stratum ids equal an existing `onl_platform` identifier and are recorded only as a lead fact, no merge. 46 overlapping DENT ids are not imported.
N3. `stratum_files_not_captured_over_50KB` is recorded for as7712-32x (`base_bcm_chassis_map.pb.txt` not captured); facts for that device are partial by design and say so.
N4. `eligible_sources` grows by 105 (all new sources carry sha256 + cache_path).
N5. Wording for any promotion note: Stratum/DENT rows are not evidence for owned CS8200 or ByteDance B4020/B4010; ByteDance remains 0/2.
