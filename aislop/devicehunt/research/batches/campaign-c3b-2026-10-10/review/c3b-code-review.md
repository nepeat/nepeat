# c3b code review (independent, Claude Sonnet 5.5)

Verdict: ACCEPT with notes, conditional on fixing finding 1 (must-fix) before the live release is frozen.

Scope: devicehunt/review_queue.py, the campaign_gate hook in devicehunt/rebuild.py, tests/test_review_queue.py.
`python3 -m unittest tests.test_review_queue tests.test_rebuild` passes (14 tests, ~46 s, offline).
Experiments ran in a scratch workspace under the session scratchpad only (script `review-c3b/exp.py`); nothing under data/ was touched.

## Findings

1. must-fix - release is not bound to the coverage manifest's unit set (review_queue.py:372-413, 436-450).
   `build_release` freezes whatever `queue/items/review-item-*.json` files exist. Nothing compares them to the units of the
   coverage manifest/definition (build_queue checks this at :277, build_release never re-checks), and `gate_inputs`/
   `gate_projections` only guard units that appear in `release['entries']`.
   Repro (E1): amend the Amazon CSV timestamp, build the queue, delete the Amazon item file, `build_release` -> release has
   the remaining entries, no error; a gated manifest over the amended bytes rebuilds successfully. The unreviewed amended
   candidate entered the catalog, which is exactly what the c3 gate must prevent. Same effect if a queue is only partially built.
   Fix: have the release record the full expected unit list (e.g. require the coverage manifest or the pinned inventory when
   releasing and fail on any missing item/unit; store `unit_ids` or the definition's unit set in the release and have
   `check_release` require every entry for it). Add a test for a missing item.

2. should-fix - the gate is opt-in and its removal or downgrade is undetectable (rebuild.py:54-56, 120).
   A manifest with no `campaign_gate`, or one naming an older/smaller release, rebuilds without any review. Not a bug in the
   hook, but nothing (test or promotion check) asserts that data/import-manifest.json carries a gate covering all 12 units.
   data/import-manifest.json has no campaign_gate today. Fix: a promotion check/test asserting presence and full coverage;
   with finding 1 fixed the release itself can state the required unit set.

3. should-fix - one rejected, failed or unreviewed NEW unit blocks the whole release (review_queue.py:398-400).
   A unit whose source is not in the live catalog or whose job is not in the accepted manifest has `baseline.inputs` None.
   If it is rejected/failed/unreviewed, `build_release` raises "neither an accepted candidate nor a baseline" for the entire
   queue (repro E3). c3 says unrelated pending inputs must not prevent freezing a valid closure. Fix: allow an entry
   `status: excluded` (no job, no inputs) for such units; the gate then forbids the job in the manifest (jobs.count == 0) and
   the source from existing. Not hit by the current 12 units (all exist in baseline), hence not must-fix.

4. should-fix - decision documents are weakly validated at release (review_queue.py:379-406).
   `build_release` does not check `decision in DECISIONS`, `unit_id == item.unit_id`, or reviewer/reason presence; an unknown
   decision value silently becomes status baseline, reason `rejected`. An `accepted` decision for a non-reviewable item
   (hand-crafted; ids are plain hashes) reaches `item['candidate']['projection_sha256']` and raises TypeError instead of
   ReviewError. Decisions are also unauthenticated (free-text reviewer), so "independent reviewer" is procedural only. Cheap
   to harden: validate the schema, require `item['reviewable']` for accepted.

5. should-fix - `decide` is check-then-write without a lock (review_queue.py:357-366).
   Two concurrent decisions with different reason text get different decision_ids, both files are published, and
   `build_release` then fails closed with "two decisions for one item". Safe (fails closed) but needs a manual file deletion
   of a read-only, content-addressed file. Prefer an exclusive per-item marker (e.g. `decisions/<item_id>.lock` via O_EXCL)
   or make the decision filename the item_id so the exclusive link is the lock. Also `_publish` (:84-96): two simultaneous
   identical publishes raise FileExistsError from `os.link`; treat as success if bytes match.

6. should-fix - diff()/no_changes can report no_changes while the projection hash differs (review_queue.py:196-219).
   `rows()` drops `row_number`, and reordering is only flagged when the multiset is equal and the sequence differs. A
   row_number-only change (gap or renumbering with unchanged order) is invisible: repro E4 gives `no_changes True` while the
   two projection hashes differ. Real data change = none, but an item with `no_changes: true` and a different candidate sha
   should never exist. Fix: add `candidate.projection_sha256 != baseline.projection_sha256` into `no_changes`, or add a
   catch-all `projection_differs` flag, and include row_number differences in the raw-row comparison.
   I found no case where an actual record/field change is hidden: every record lands in exactly one of
   unchanged/moved/changed/added/removed, greedy duplicate pairing can only mislabel moved vs changed, not drop a change.
   The in-order pairing in `zip(b, c)` can pair unrelated duplicates and report a "changed" on the wrong key; harmless for
   gating because acceptance is of the whole projection hash.

7. note - the real backstop is the post-import projection hash; the input-hash precheck is only early-fail.
   Path spelling is compared as plain strings (rebuild.py:50, review_queue.py:445-448): the manifest can import the same
   file twice as `raw/x/m.json` and `./raw/x/m.json` (repro E5: accepted when both are the released bytes). Any divergent
   second spelling imports different bytes, but then `gate_projections` fails, so there is no bypass. Still, normalizing via
   `local_path()` in `read_manifest` duplicate detection would make this clean. Hijack via another job (`curated`) is covered
   by the test at test_review_queue.py:158. Shared dependency paths: none among the 12 units today (checked); if a later
   unit shares a file with a rejected/baseline unit, accepted-vs-baseline hashes for that single path become unsatisfiable
   and the whole gate fails. Fail-closed, worth knowing.

8. note - projection completeness is adequate for rra/ised; exclusions are justified.
   Verified against rra.py/ised.py/catalog.py: these importers write only sources, records and children
   (affiliations, identifiers, facts, hyperscaler_evidence) plus rra_equipment / ised_equipment (`SELECT *` minus source_id,
   so every column incl. cells_json, source_url, detail_sha256 is in). Excluded: sources.fetched_at/cache_path (mtime, path),
   records.id (hash of source+key), observed_at (now()), and devices brand/model/title/kind (first-writer-owned via INSERT OR
   IGNORE; device_id is a hash of normalized brand/model/revision so identity is still covered, and the printed literal is in
   raw_json). The `search_index` FTS rows are not part of projection; they are device-level and cross-source, rebuilt by
   `Catalog.finish`, derived from included data plus excluded first-writer device fields, so cannot carry a source-specific
   leak. FTS equality remains the job of compare_rebuild, not the gate. `device` rows themselves (count) are covered by the
   rebuild expected counts. Nondeterminism: none found; sorted children, ORDER BY source_key/row_number, canonical JSON.
   Minor: `ORDER BY source_key` sorts text, fine for determinism.

9. note - candidate projection is derived by re-running the importer on the pinned bytes, not cross-checked against the
   campaign's candidate bundle (`candidate_bundle_sha256`, `bundle_receipt` are recorded but not verified; build_queue
   :296-298). Equivalent only because pinned file digests are checked first and the importer is the same code the gate
   runs later. Only unit.files are digest-checked, not any file the importer reads that is outside the pinned dependency set
   (the projection gate backstops this at rebuild time).

10. note - malformed releases raise KeyError/TypeError rather than ReviewError (review_queue.py:421-430, e.g. missing
   `job`, `inputs` not a list, non-hex projection_sha256, duplicate input paths inside an entry). Fail-closed, but give a
   confusing message. `check_release` accepts any `claims` equal to the constant only; good.

11. note - queue idempotence: re-running `queue` after the live catalog or accepted manifest changes writes new items
   beside old ones for the same unit; `build_release` then fails closed with "duplicate unit in queue" (items are immutable,
   no archive/supersede mechanism). Use a fresh output dir per queue run; document it.

## Checked and found sound

- Gate ordering: `gate_inputs` runs in `read_manifest` before any import and again in `rebuild`; `gate_projections` runs after
  every job, before count checks and before publication; `read_manifest` re-run at the end re-verifies the release file.
  Release file must be a hashed manifest input; strict canonical loader refuses duplicate keys, NaN, reformatting.
- Zero-result handling: explicit zero is reviewable with a real source row (projection not None); unavailable/failed/blocked
  units have no candidate and cannot be accepted; they revert to baseline inputs and baseline sha.
- Baseline inputs come from the accepted manifest, not the pinned candidate hashes; a unit missing a baseline hash for any
  pinned path is refused at queue time (:289).
- Item/decision/release ids are content hashes with verification on read; decision binds the item id and the candidate sha;
  release re-verifies that binding; one decision per item enforced at decide and at release.
- Live check claimed by the author (12 units project identically) is consistent with the importer code: the importers only
  touch their own source, and the one global-looking statement (rra.py:221 UPDATE affiliations) is scoped to the source's records.

## c3 acceptance coverage by the tests

Covered: unchanged real units yield no_changes and rebuild passes with archive and queue moved away; explicit zero
(ised celestica); repeated certificate with distinct devices (Amazon, asserted); amended source needs acceptance and is
refused when unreviewed; failed unit cannot be accepted and keeps baseline; rejected unit keeps baseline (release-level);
reorder = moved not changed; duplicate identities preserved; synthetic add/amend/remove in diff; hijack by another job;
edited item/release refused; lineage-dangling on moved key (synthetic).

Not covered (add tests): omitted/partial queue (finding 1); gated manifest absent; rejected or failed candidate inputs
refused by the actual rebuild gate (only the release status is asserted); cross_query_certificate_overlap flag and
`independent_corroboration: False` (no assertion anywhere); `html_csv_discrepancy` conflict surfacing; conflicting roles on
one certificate; accepted addition/removal flowing through queue -> release -> rebuild (only amendment does); RRA unit in the
gated rebuild with an amendment; real PDF/lineage statements with the gate (only a synthetic dangling test); new unit not in
the accepted manifest or live catalog; two-decision / concurrent decide; path-spelling duplicate; decision with bad fields.
Unrelated-damaged-pending-object and missing-dependency-before-replacement belong to c3a but the gate-level missing-file
case (gated input absent from disk) has no test.

## Fix verification (second pass)

Verdict: ACCEPT. No remaining must-fix.

- `python3 -m unittest tests.test_review_queue tests.test_rebuild`: 17 tests pass (~59 s).
- Finding 1: the missing-item repro now fails with "review queue must hold exactly one item per coverage unit"
  (`build_release` requires the coverage manifest, checks identity and per-item coverage id). `gate_inputs` requires
  `campaign_gate = {release, coverage_manifest}`, both hashed, release id/campaign match, and entries equal to the coverage
  unit set. Fixed.
- Finding 2: opt-in gate is acceptable for rebuild; not blocking provided the promotion check asserts the gate and
  it is documented. Residual: still only a procedure, not enforced by code.
- Finding 3: `excluded` status verified in `build_release`/`check_release`/`gate_inputs` (job must be absent) and
  `gate_projections` (projection must be None, so the source must not exist). Fixed.
- Finding 4: `_check_decision` validates keys, decision value, unit, reviewer/reason, accepted -> reviewable + candidate sha,
  rejected -> null sha; runs for every decision at release. Fixed.
- Finding 5: `_publish` handles FileExistsError with a bytes check. The decide check-then-write race remains but fails
  closed (note only).
- Finding 6: `raw_row_numbers_changed` now makes `no_changes` false (my E4 repro now returns False).
- Not re-reviewed in depth: the new overlap logic over raw-row certificate fields (author's dry run reports SUN-6000 only).
- Still open as notes: findings 7-11 (path spelling, unauthenticated reviewers, KeyError on malformed releases).
