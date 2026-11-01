# spec_results wave 8 handoff — 2026-10-11 (Claude Sonnet 5.5)

## Outcome
Supplement to the promoted candidate 428c78f8… (untouched, with its captures, `capture-manifest.json` and `requests.jsonl`). Fetched the remaining matched CPU 2017 text results for Quanta: **59** (all printed vendor `Quanta Cloud Technology`; every `Quanta Computer Inc.` result was already in revision 3). No unfetched Oracle/Sun rows exist (all 6 were captured earlier). **60 new requests** (cap 70): 1 robots re-fetch (byte-identical to the saved copy: Crawl-delay 10, `/cgi-bin/` disallowed) + 59 .txt; 60 x HTTP 200, no 403/429/5xx, no retries, >=10 s spacing, plain UA. Combined SPEC total 304 requests.

## Files and hashes
- Captures: `data/raw/discovery-2026-10-11-spec/wave8/cpu2017/*.txt` (59) and `wave8/robots/robots.txt`.
- `.../capture-manifest-wave8.json` sha256 `d147c225c4cb685bfbe7ba49730f383f2dcc6b23615d6cb2baa675fa99cb294e` (all 60 files re-hashed against disk: 0 mismatches); raw log `.../wave8/requests-wave8.jsonl` sha256 `161a70820c39f9dc34498db61300b3f9476b46691483c0b72e1a6233cc85d37c`.
- Candidate `.../spec-results-wave8-2026-10-11.json` sha256 `19f14b186cc7e7a345e2c2800fa8633b0ffce427a986185d43ab15d88919f88d` (curated; snapshots `wave8/cpu2017/...` relative to the file's directory, so import it in place). 59 sources / 59 device rows / 954 facts; source ids `spec-cpu2017-<stem>` are new (disjoint from the promoted set; checked).
- Scripts: `research/batches/discovery-2026-10-11/spec/scripts/{fetch8,run8,build8}.py` (build8 = revision-3 builder pointed at the wave 8 inputs).

## Identities and rules (identical to revision 3)
Brand `Quanta Cloud Technology` as printed (not a TARGETS alias, not mapped to Quanta); affiliation `[Quanta Cloud Technology, reported_system_vendor]` (Test Sponsor `Quanta Computer Inc.` differs); tier `vendor`; kind `server`; model = printed name minus a trailing CPU/config parenthetical (49 of 59 stripped, same patterns as before, none kept), full name as `spec_system_name` identifier and fact; per-line `spec_result_<metric label>` facts; None/`Not Run`/`--` omitted; coverage text states the rules. 9 distinct models: `QuantaGrid D43K-1U`, `QuantaGrid D44N-1U`, `QuantaGrid D44N-1U AMD EPYC 9755`, `QuantaGrid D53XQ-2U`, `QuantaGrid S43KL-1U`, `QuantaGrid S44NL-1U`, `QuantaGrid S44NL-1U AMD EPYC 9755`, `D54Q-2U`, `D55Q-2U`. No multi-node/shared-enclosure power facts apply (CPU 2017 text has none).

## Checks
- Quote check: 895/895 evidence quotes found in the captures, each fact value equal to or inside its quote, no `None` values.
- Temp import into a scratchpad COPY of the CURRENT data/devices.sqlite (sha baa016d2…): returned 59; sources 1672 -> 1731, device_facts 65930 -> 66884, **devices 5042 -> 5042: 0 added, 9 merged** into the existing `Quanta Cloud Technology` identities from revision 3 (other records already on them: D43K-1U 6, S43KL-1U 1, D53XQ-2U 2, D54Q-2U 2, S44NL-1U 4, D44N-1U 4, D55Q-2U 2, `S44NL-1U AMD EPYC 9755` 1, `D44N-1U AMD EPYC 9755` 1). integrity_check ok, foreign_key_check empty. `vendor-inventory --scope manufacturer` and `--scope all` (--limit 0) identical before/after (reported_system_vendor on a non-alias brand adds no target assertions).
- Owned-model check (case-insensitive CS8200, CS8210, AZ4444, AS7760, standalone 7760, DMC-12, AS7772, SV1120/1121/2121/2221, ByteDance, B4020, B4010, Oracle E4-2C/E5-2C/X7-2C/X10-2C) over the 59 texts and the candidate: zero hits.

## Gaps
Unfetched CPU 2017 rows remain only for Inspur (330 distinct names) and IEIT (64 distinct names); SPECjbb2015 still has no text results.
