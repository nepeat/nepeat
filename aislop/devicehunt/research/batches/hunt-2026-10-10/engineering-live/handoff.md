# engineering-live handoff (live_bundle)

## Outcome
`RraRefresh.bundle(captured, output, *, carry_from=None)` now carries the saved unit's reviewed `company_evidence`;
`compare_live(output, expectations)` is a location-insensitive pinned comparison. Both implemented and tested offline.

## Files
- devicehunt/campaign_rra_live.py (bundle carry, `_load_carried_evidence`, `compare_live`)
- tests/test_campaign_rra_live.py (appended `RraLiveBundleCarryTests`, 3 tests)

## Design choices
- Evidence is verified (sha256 via `documents.dependency`) BEFORE any write; mismatch raises ValueError and the output dir is never created.
- Celestica's declaration is a tree (historical-companies -> `base` + 2 `exhibits`). Those sibling files are carried too, each verified against the hash its parent declares; the top-level declaration is copied identically. Copies are byte-identical, re-hashed after writing. Carried files are written before `rra.validate`, which loads them.
- No `carry_from`, or a saved bundle without `company_evidence`: nothing carried, nothing invented.
- `first_result_page` is NOT carried: it is an old-session observation and `rra.validate` treats it as optional (checked only `if 'first_result_page' in data`), so no query shape requires it. Copying would pass old bytes off as live.
- `compare_live` compares `detail_path` by file name on both sides; everything else as `RraSavedAdapter.compare`. `location_only=True` means the adapter comparison differed but only by location. Returns `not_pinned` for empty expectations.

## Tests + results
Celestica replay: without carry 48 eligible rows; with carry eligible rows and model assertions equal the pinned c0 values; `compare_live` matches (location_only). Tampered base/top evidence refuses and creates no dir. Byte-identical replay of Edge-Core at a new directory: adapter differs, `compare_live` matches. Mutated detail cell: still differs (records hash).
`python3 -m unittest tests.test_campaign_rra_live tests.test_rra tests.test_campaign`: 45 tests OK.

## Risks
- Evidence loaders that add new reference keys beyond `base`/`exhibits` would not be followed (the load would then fail validation loudly, not silently).
- The saved bundle's evidence is trusted as reviewed only as far as its hashes; the live bundle is only as good as that review.
- Carried evidence changes the live bundle's file set vs a bare capture; queue/import code should key on bundle.json.

## Next
Driver should pass `carry_from=<pinned bundle.json>` and use `compare_live` instead of `RraSavedAdapter.compare` for live units; c3b queue baseline/candidate path separation (c4 follow-up 3) remains open.

## Fixes after review
1. Carried evidence names (incl. nested base/exhibits) that match a generated bundle file (search-form.html, before.csv, after.csv, detail-NNN.html, empty-result.html, candidate.json, bundle.json), or reuse a name with a different hash, are refused before mkdir.
2. Any exception after mkdir removes the new output dir (created exclusively by this call). A validate failure that returns outcome=failed still retains candidate.json as before.
3. Carry is refused unless the saved bundle's `query` equals the live scope query (always enforced, no opt-in param).
Tests: test_collisions_wrong_scope_and_midwrite_failure_leave_nothing. Suite: 46 tests OK.
