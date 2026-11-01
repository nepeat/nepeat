# bundle_sha handoff

## what candidate_sha256 / bundle_receipt actually are
`candidate_sha256` hashes the archived **candidate-bundle JSON** (campaign.py `_bundle`): outcome, scope, zero_proof,
occurrence_ids, occurrences_sha256, record_sets, coverage. It is derived by the adapter from the unit's saved manifest;
it is NOT one of `unit.files` and not a hash of the manifest or any input. So comparing it to a file hash would compare
unrelated things. `bundle_receipt` is the `campaign-bundle` receipt id whose payload carries the same sha (and lists it as
its `candidate-bundle` output). Artifact bytes live in the archive (`objects/xx/<sha>`, scope `archive`).

## check implemented (devicehunt/review_queue.py `check_candidate_bundle`, called from `build_queue`)
All offline, no network. Refuses (ReviewError) unless:
1. the bundle receipt is a selected `campaign-bundle` receipt with intact `wrapper_sha256`, for this unit id + revision,
   whose payload `candidate_sha256` and `candidate-bundle` output both equal the entry's sha;
2. (reviewable units) the sha is a selected artifact, its archive bytes verify (size + sha, via `coverage._read_artifact`);
3. the adapter re-derives (outcome, scope, zero_proof, occurrence_ids, occurrences_sha256, record_sets) from the very
   manifest `candidate_projection` imports (pinned unit.files hashes already verified just before) and they equal the bundle.
Non-reviewable outcomes (failed/not_reached/...) may have no receipt and sha=None; with a receipt they must still agree with
it; nothing is projected or re-derived. A reviewable unit with no receipt/sha is refused.
Archive root: new optional `archive_root` kwarg / `queue --archive` (default `data/archive`; falls back to `<workspace>/archive`
so existing fixtures work; none found => refused). Items gain no field, so ITEM_SCHEMA/item ids are unchanged by this check.
projection()/_identity()/_content()/raw extraction/projection_sha256 untouched.

## live verification (read-only)
`python3 -m devicehunt.review_queue queue data/coverage/rra-ised-saved-2026-10-04/coverage-d2bc5c40...json --output <scratchpad>`
(coverage manifest from data/import-manifest.json campaign_gate) succeeded: 12 items, all checks passed (bundle receipt,
archive bytes, re-derivation), nothing written under data/. Item ids differ from data/campaign-reviews/coverage-d2bc5c40...
only because of earlier work (baseline.job from queue_baseline; duplicate_occurrence conflicts on accton/edgecore), not this change.

## tests (tests/test_review_queue.py, class CandidateBundleShaTests, 5 new)
positive (all 12 fixture units), tampered archive bundle bytes refused by build_queue, sha naming another unit's bundle refused,
bundle vs manifest-derived mismatch refused, non-reviewable None accepted / reviewable None refused / receipt disagreement refused.
`python3 -m unittest tests.test_review_queue`: Ran 33 tests, OK (28 pre-existing + 5 new; run before a cosmetic test edit,
after which the new class + AcceptedManifestGateTests + ProjectionDiffTests re-ran OK, 10 tests).

## limits
- The re-derivation proves the bundle matches the manifest bytes now on disk; the archive/receipt chain proves what was
  observed at run time. A coherent attacker able to rewrite archive objects AND coverage manifest is out of scope.
- Residual TOCTOU between the pin check and the importer reading files is unchanged.

## review follow-up (bundle-sha-review.md: S1-S3, S5, S4)
- S1: receipt payload outcome must equal entry outcome (all units), and the re-derived outcome must equal the entry outcome and the bundle's.
- S2: `build_queue` calls `coverage.validate(manifest)` once (no roots needed; offline) and wraps CoverageError as ReviewError
  `coverage manifest invalid`. Passes on the live manifest (12 items rebuilt).
- S3: bundle JSON that is invalid or not an object raises ReviewError `not a JSON object`; payload must be a dict.
- S4 (optional, done): `unit.files` are re-hashed after `candidate_projection` ('pinned input changed during projection').
- S5: tests pin receipt `unit_revision` and `unit_id` binding (consistent wrapper digest, only the binding can fail).
- Comment added on the data/archive-first archive root preference. `--archive` relative paths resolve against `--workspace` (unchanged, noted).
- New tests: outcome mismatch, revision/unit binding, non-object bundle, invalid coverage manifest, plus an updated non-reviewable test.
- `python3 -m unittest tests.test_review_queue`: Ran 37 tests, OK (33 + 4 new). Live read-only queue build: 12 items, all checks pass.
