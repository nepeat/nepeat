# engineering review (Sonnet 5.5, independent)

Verdict: ACCEPT with notes. No must-fix.

Verified: `python3 -m unittest tests.test_derivations tests.test_review_queue tests.test_rebuild` = 31 OK (canary included). Live check: `build_release` on data/campaign-reviews/coverage-d2bc5c40.../ (no markers) into a scratch dir gave release_id campaign-release-2a5b2f0c..., byte-identical to data/campaign-releases/. No git baseline exists for this dir, so "frozen untouched" rests on reading: projection/_identity/_content/diff/no_changes/check_release/gate are unchanged in behaviour, the canary passes, and duplicate_occurrence only appends to item `conflicts` (not part of any projection hash).

## S5 (derivations.py)
1. note, derivations.py:14-34: ids identical under PYTHONHASHSEED 0/1/2 (-I runs); comment/unrelated-function edits keep the id, an edit inside a recipe body (e.g. 'loc'->'locx') changes it. Recipe dependencies are only stdlib (json, ET) plus `canonical`, which is hashed; no module constants or private helpers are used, so nothing is missed today.
2. note (design limit), derivations.py:15-16: tracking is manual. A future helper/constant called by a recipe, `store_bytes`, or the `run()` dispatch is NOT hashed; the author must add it to RECIPE_FUNCTIONS/RECIPE_EXTERNAL_HELPERS or bump RECIPE_CODE_VERSION. Documented in the handoff; consider a test that fails when a recipe function references a module-level name outside an allowlist.
3. note: old receipts are safe. Old recipe payloads carry a whole-file code_sha256, so `run(old_id)` raises the existing mismatch error and re-registration yields new ids. There is no false reuse (reuse is keyed on recipe_id plus an equal-payload check).

## decide race (review_queue.py:371-399, 418-434)
4. note: correctness holds. Claim is link-based exclusive (`_publish`); a different decision loses with ReviewError, an identical one passes the byte check. Crash after marker and before decision: release refuses with an explicit message, and the same decide completes it. Decision without marker (legacy): valid, as the live run shows. `.review-*` temp files are ignored. The concurrent decide + build_release interleaving fails closed.
5. should-fix (minor), review_queue.py:381-384: the advisory scan still rejects a re-run of an already COMPLETE identical decision ("item already decided"), the same as before. The handoff's "idempotent re-run" holds only for the interrupted case. Either say so in the docs or let an identical decision_id pass.
6. note, review_queue.py:431: `claim['item_id'] in by_id` raises TypeError (not ReviewError) if a hand-edited marker has an unhashable item_id. Fail-closed anyway.
7. note: a marker whose intended decision differs from a crashed one needs a manual marker delete (accepted risk, release refuses meanwhile).

## duplicate_occurrence (review_queue.py:265-274, 329)
8. note: grouping by `_identity` = canonical([device_id, identifiers]), matching the frozen diff identity; one conflict per group of 2+; sort keys are unique strings so there is no comparison error. Item bodies only. Caveat: rebuilding a queue for a manifest with duplicates yields new item_ids, so existing decisions will not bind (expected, "new queue items only"). The live queue is not rebuilt. Not run end-to-end on real data (helper tested directly).
