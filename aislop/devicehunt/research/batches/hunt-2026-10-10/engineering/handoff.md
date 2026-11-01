# Handoff: engineering (S5, duplicate_occurrence, decide race)

## Outcome
All three tasks done. `python3 -m unittest tests.test_derivations tests.test_review_queue tests.test_rebuild`: 31 tests OK (frozen projection canary unchanged).

## Files changed
- devicehunt/derivations.py, devicehunt/review_queue.py (conflicts section of build_queue, decide, new `_check_claims` called from build_release, `CLAIMS` const)
- tests/test_derivations.py, tests/test_review_queue.py (new tests appended only)

## Behaviour + design choices
- S5: `code_sha256` in a recipe payload is now `recipe_code_sha256(name)` = digest of {recipe name, explicit `RECIPE_CODE_VERSION[name]`, AST hash of the recipe's own function(s) `_page_text_flatten` / `_xml_locations`, AST hash of external helper `artifacts.canonical`}. AST-based, so comments/whitespace/unrelated edits do not move ids; deterministic across processes (tested). Recipe bodies were extracted verbatim from `run` into those module functions. Bump `RECIPE_CODE_VERSION` for behaviour changes the AST cannot show (e.g. `store_bytes`, stdlib semantics). Existing recipe ids (whole-file hash) do not match the new scheme; they already did not match, so no new loss.
- duplicate_occurrence: `_duplicate_occurrence_conflicts(candidate)`; one conflict per identity group (device_id + identifiers) with count, source_keys and the note. Affects new queue items only.
- decide race: exclusive link-based claim `decisions/.claims/<item_id>.json` (content {item_id, decision_id}) published via `_publish` before the decision. Loser (different decision) gets `ReviewError('item already decided: claimed by another decision ...')`. Identical decision re-run completes an interrupted publish idempotently.
- build_release: marker without its decision raises `interrupted decide: claim without a decision ...; re-run the same decide command`; unknown/foreign/mismatched markers or stray files refused; `.review-*` temp leftovers ignored; decisions with no marker (pre-existing) remain valid.

## Tests run + results
31 OK. New: recipe-id stability/change/determinism, duplicate_occurrence fixture, spawn-based 2-process decide race (3 rounds), interrupted-claim completion, `_check_claims` validation, release refusal/recovery/legacy.

## Risks
- A claim whose intended decision differs from a crashed one can only be cleared by deleting the marker by hand (deliberate; release refuses meanwhile).
- Existing live queue has no markers; its decisions still validate.
- duplicate_occurrence not exercised through the full build_queue on real edgecore data (helper tested directly; wiring is one line).

## Next
Re-queue ised-rel-query-edgecore when the next queue is built to see the new conflict.
