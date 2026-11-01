# live_bundle review (independent, Sonnet 5.5)

Verdict: ACCEPT with notes. Tests: `python3 -m unittest tests.test_campaign_rra_live tests.test_rra tests.test_campaign` = 45 OK (115 s). Tests are offline: setUp patches socket/http/urllib/subprocess to raise; fixtures replay saved bytes.

## Checks
1. Carry integrity: OK. `_load_carried_evidence` hashes every file via `documents.dependency` (sibling-name only, resolve-parent check, sha256) in memory BEFORE `mkdir`. Nested `base`/`exhibits` refs are each verified against their parent's declaration; top declaration is copied verbatim; bytes written from the verified in-memory copy with O_EXCL and re-read/re-hashed. Probed offline (scratch): `../x`, nested `../probe.py`, conflicting duplicate-path refs all raise ValueError; tampered base/top files refuse with no dir created (test). Nothing is written outside `root`. No `company_evidence` in the saved bundle, or no `carry_from`: nothing carried.
2. Whole-bundle refusal: OK. `bundle.json` only exists via rename after `rra.validate(candidate)` (which reloads and re-hashes the carried evidence from the new dir). Validation failure returns outcome=failed and leaves `candidate.json` (no `bundle.json`).
3. `first_result_page` omission: correct (validate checks it only if present) and documented in the `bundle` docstring, handoff, and a test assertion. The c4 note follow-up still lists it as to-be-carried; update it.
4. `compare_live`: normalizes only `occurrences[].detail_path` to its file name on both sides; certificate, ordinal, logical_sha256, projected keys, stats and records hash still compared (mutation test shows records-hash mismatch). Real check below.
5. Tests meaningful. Gaps below.

## Findings
1. should-fix: evidence file names are not checked against the names bundle() itself writes. A saved declaration naming `before.csv`, `search-form.html`, `detail-NNN.html` etc. raises FileExistsError (not ValueError) AFTER mkdir, leaving a partial dir; naming `candidate.json` is silently overwritten by the later candidate write; `bundle.json` would be clobbered at rename. Only reachable with a hostile/odd saved bundle (probed `before.csv`: accepted by the loader). Fix: reject reserved/colliding names in `_load_carried_evidence`, before mkdir.
2. should-fix (minor): any failure after `mkdir` (copy re-hash mismatch, OSError, validate fail) leaves a non-valid partial dir (no `bundle.json`), so it is never valid-looking, but the "no dir created" guarantee holds only for the pre-write check. Document it, or cleanup on exception; add a test for a mid-write/validate failure leaving no `bundle.json`.
3. note: duplicate-path refs are de-duplicated by path before their sha is compared; the conflict is still caught later (by the loader and validate), not by the carry itself. Unknown reference keys beyond `base`/`exhibits` are not followed (loud validate failure, as the handoff says).
4. note: no check that the carried evidence belongs to the unit being refreshed (query vs saved bundle query). Driver should pin `carry_from` per unit.
5. note: zero-row units report `location_only=False` with `matches_pinned` (no occurrences to relocate); correct.
6. note: c4-notes follow-ups 1 and 2 now stale/partly done; the Celestica/Accton live bundles on disk predate the fix and need regenerating with `carry_from` before queueing.

## compare_live on real bundles (data/raw/*-live-2026-10-10d vs inventory.json; read-only)
- rra-official-query-78fc6b2e95f6 (edge-core-space, zero): matches_pinned, location_only False (no rows)
- rra-official-query-5827f2fe7092 (edge-core-hyphen): matches_pinned, location_only True
- rra-official-query-33f7d154085d (amazon): matches_pinned, location_only True
- rra-official-query-63cb0b9098c7 (edgecore): matches_pinned, location_only True
- rra-official-query-41e4ca912b33 (celestica): differs_from_pinned [eligible_manufacturer_rows, model_assertions, occurrences] (expected; no carried evidence)
- rra-official-query-e5984834207f (accton): differs_from_pinned, same three keys (expected; same cause). records hash equal for both.
- rra-official-query-73009f906e4e (oracle): no live bundle on disk.

## fix verification
Re-read the code and reran `python3 -m unittest tests.test_campaign_rra_live tests.test_rra tests.test_campaign`: 46 OK.
- Finding 1 fixed: `_check_collisions` refuses any carried name (nested included) matching search-form.html, before.csv, after.csv, detail-NNN.html, empty-result.html, candidate.json or bundle.json, before `mkdir`. Same name with a different hash is refused in the loader.
- Finding 2 fixed: `bundle()` wraps `_write_bundle` and `rmtree`s the exclusively created dir on any BaseException. A validate failure still returns outcome=failed and retains `candidate.json`, with no `bundle.json` (the documented behaviour).
- Finding 4 fixed: carry is refused unless the saved query equals `scope.query` (always on).
- Remaining notes: 3, 5 and 6 are unchanged and are not blockers. The Celestica and Accton live bundles still need regenerating with `carry_from`.
Updated verdict: ACCEPT.
