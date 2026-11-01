# bundle_sha review

**Verdict: ACCEPT with notes.** No must-fix. The check binds what it claims; gaps are defense-in-depth only, because the
coverage manifest is identity-hashed and `build_queue` does not otherwise run `coverage.validate`.

Evidence legend: [V] verified by running something, [R] read.

## Answers to the brief
1. **What candidate_sha256 hashes** [R] `campaign.py:330-337,340`: `candidate` dict (schema_version, unit_id, unit_revision, outcome,
   scope, zero_proof, coverage, occurrence_ids, occurrences_sha256, record_sets, error, review) is `canonical()`-serialised and
   stored; `candidate_sha256` = the stored object's sha, `outputs=[candidate-bundle, sha]`. Handoff claim is correct. It is
   not a hash of any `unit.files` entry.
2. **Binding** [R] `review_queue.py:335-369`. Receipt must be selected, `receipt['kind']=='campaign-bundle'`, wrapper digest intact,
   payload `unit_id`, `unit_revision`, `candidate_sha256` and the `candidate-bundle` output all equal the entry/unit. Reviewable: sha in
   manifest artifacts; `_read_artifact` verifies size and sha against the archive; bundle fields (unit_id, unit_revision, outcome, scope,
   zero_proof, occurrence_ids, occurrences_sha256, record_sets) must equal the re-derivation. A stale or foreign bundle fails at the
   receipt sha/unit/revision step or at re-derivation. See S1-S3 for what is not bound.
3. **Re-derivation / TOCTOU** [R] 357 uses `ADAPTERS[unit.source].records(workspace/unit.manifest, ...)`; `candidate_projection`
   (425) imports the same `workspace/unit.manifest`. Order is pin check (423), then bundle check (424), then projection (425).
   Within `check_candidate_bundle` there is no re-hash, and the manifest and detail files are read twice more afterwards
   (records(), then import_source). See S4. Not a regression: the handoff already lists the window.
4. **archive_root** [R] 308-314: explicit wins; else `data/archive`, else `<ws>/archive`; none => refuse (349). With both present
   `data/archive` is chosen silently, but `_read_artifact` (coverage.py:253-262) checks size and sha, so a wrong archive can only
   produce a refusal, never wrong bytes accepted. Refusing when absent is correct. [V] The tampered-object test refuses.
5. **Read-only** [V] Copied `data/archive` to scratchpad, hashed all files plus mode/mtime before and after, ran the live
   `queue ... --archive <copy>` (12 items): hashes identical. [R] `_read_artifact` only does `read_bytes`; no receipts/registry/lock
   opened. The repo `data/` tree is untouched (the CLI writes only `--output`).
6. **Schemas / canary** [V] `ITEM_SCHEMA='campaign-review-item/1'` etc unchanged (review_queue.py:31-33); canary test
   (`test_projection_canary_is_frozen`, literal 63fe658f...) and `AcceptedManifestGateTests` pass. [V] Live items rebuilt vs
   `data/campaign-reviews/coverage-d2bc5c40.../items`: `candidate`, `changes`, `candidate_bundle_sha256` etc identical for all 12;
   only `baseline` (all 12) and `conflicts` (accton, edgecore) plus the derived `item_id` differ, consistent with the handoff's
   attribution to earlier work (I could not verify that attribution: no git history, devicehunt is untracked). The new check cannot
   produce those differences (it returns None and never touches item fields).
7. **Mutants** (copies in scratchpad `m1..m4`; `CandidateBundleShaTests`)
   - skip re-derivation comparison: KILLED (`test_bundle_derived_from_other_bytes...`) [V]
   - accept missing receipt for reviewable: KILLED (`test_non_reviewable_outcomes...`) [V]
   - drop `digest(stored)==sha` (line ~353): SURVIVES [V], but redundant because `_read_artifact` already hashes. Equivalent mutant, fine.
   - drop `unit_revision` payload check (340): SURVIVES [V] -> S5.
8. **Test run** [V] `python3 -m unittest tests.test_review_queue` in the repo: Ran 33, OK (209 s). (An initial run of a copy with
   symlinked data failed one test with "symlink forbidden", a harness artifact of my copy, not the code.)

## Must-fix
None.

## Should-fix
- **S1 entry outcome not bound to bundle/receipt outcome.** `reviewable` comes from `entry['outcome']` (build_queue:~418) but only
  the *bundle* outcome is compared to the re-derivation; `entry['outcome']` vs `payload['outcome']`/bundle is never compared. [V] probe
  (copy): swapping entry outcome validated_nonempty<->explicit_zero passes `check`. Both are REVIEWABLE so the effect is a wrong
  `outcome` label on the item, and a non-reviewable entry label skips projection, not a stale bundle. Add
  `bundle['outcome']==entry['outcome']` (and payload outcome) at review_queue.py:340/368.
- **S2 receipt wrapper kind/id not checked.** Check uses top-level `receipt['kind']` and the wrapper digest but never
  `wrapper['kind']==receipt['kind']` / `wrapper['id']==name` (coverage.validate does, coverage.py:172). [V] probe: a receipt whose wrapper
  kind is `campaign-validate` and id `x` with recomputed digests passes. Needs an attacker who can recompute manifest identity, so
  low severity, but cheap: call `coverage.validate(manifest)` (offline, no roots) once in build_queue, or add those two checks.
- **S3 `json.loads(stored)` result unchecked.** A bundle that is valid JSON but not an object raises AttributeError at
  `bundle.get` (368) rather than ReviewError; same for a malformed `payload`/`outputs`. Cosmetic, still fails closed.

## Notes
- S4: TOCTOU unchanged and now two extra reads (records() at 357 and import_source at 425) after the single pin check at 423; the derivation
  proves bundle==current bytes, not bytes==pin at projection time. A re-hash of `unit.files` after `candidate_projection` would close it.
- S5: untested: payload `unit_revision` and `unit_id` binding (mutant m4 survives). Add a test with a receipt of another revision.
- Not compared: bundle `coverage`, `error`, `schema_version` (coverage is derived from outcome; `error` is not produced by records()).
- `--archive` relative paths resolve against `--workspace`, while `--catalog/--accepted-manifest/coverage_manifest` resolve against cwd.
  Equal in normal use (workspace defaults to cwd); document or make consistent.
- Fallback to `<ws>/archive` exists only for test fixtures; production has no such directory (`ls archive` fails). Fine, but a
  silent preference for `data/archive` when both exist is worth a one-line comment.
- Handoff test count (28+5) is correct for the final tree: 33 OK.

## re-review

**Verdict: ACCEPT.** No must-fix. S1-S5 are resolved and verified.

- S1 [V]: swapped entry outcome now refused (`bundle receipt outcome differs from the unit outcome`); `review_queue.py` checks payload outcome vs entry for all units and re-derived outcome vs entry/bundle for reviewable ones. Mutant dropping the payload-outcome check (n2) is KILLED (2 tests fail).
- S2 [V]: `build_queue` calls `coverage.validate(manifest)` (line ~393), wrapped as `coverage manifest invalid`. Probe: wrapper kind changed with recomputed wrapper digest and manifest_id, written as a canonical file, run through `build_queue` -> `coverage manifest invalid: receipt wrapper mismatch`. (`check_candidate_bundle` called directly still lacks the wrapper kind/id check; fine since the only production caller validates first.)
- S3 [R]: non-object or invalid bundle JSON -> `candidate bundle is not a JSON object`; payload must be a dict. Covered by a new test.
- S4 [R]: pinned inputs re-hashed after `candidate_projection` (`pinned input changed during projection`). Not exercised by a test (note, not blocking).
- S5 [V]: mutant dropping the `unit_revision` binding (n1) is now KILLED (`test_receipt_of_another_revision_or_unit_is_refused`).
- Changed existing test [R]: the receipt-bearing `outcome='failed'` entry now fails with `outcome differs` instead of `does not name the recorded candidate sha`. It is still refused for the same disagreement (entry vs receipt), now caught earlier; the sha-disagreement case is kept as the added `other` sub-case.
- Read-only [V]: live queue build (12 items) against a fresh scratchpad copy of `data/archive`: tree hash (contents, mode, mtime) identical before and after.
- Suite [V]: `python3 -m unittest tests.test_review_queue` in the repo: Ran 37, OK (422 s).
- Note: the added `coverage.validate` runs only structure/chain checks (no roots); per-unit bytes remain checked by `check_candidate_bundle`. Schemas and the canary test are still green within the 37.
