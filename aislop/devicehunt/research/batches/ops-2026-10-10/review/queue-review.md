# queue_baseline review

Verdict: ACCEPT with notes (no must-fix).

Evidence run: `python3 -m unittest tests.test_review_queue` -> 26 tests OK (201s), includes
AcceptedManifestGateTests and test_projection_canary_is_frozen. Read-only probe of data/import-manifest.json:
`gate_inputs` OK (12 entries, all `accepted`, none with `candidate_job`, none excluded) and the new
`accepted_release()` OK on it and returns the same release as `gate_inputs`. Mutation probes ran on a copy in scratchpad.
Note: devicehunt/ is untracked, so there is no git diff; untouched-ness of projection()/_identity()/_content()/raw-row
extraction was checked by reading (the changes are confined to lines 277-302, 305-399 baseline block, 508-521, 547-551, 578-580)
and by the green canary.

## must-fix
none.

## should-fix
1. Excluded-honouring is untested (tests/test_review_queue.py:354-546; code review_queue.py:338-343).
   Verified by mutation: changing line 338 to `... in released and released[unit]['status'] != 'excluded'` (so an
   excluded release entry falls through to the path match) leaves all 8 MovedBundleBaselineTests green. Current code is
   correct (the `if/elif` means an excluded entry yields baseline None/None and stays excluded), and a re-import would in
   any case be refused by gate_inputs (review_queue.py:574-575), but nothing pins it. Add a test: release entry
   `excluded` for RRA, manifest imports its (new) path, expect baseline job/inputs None and release status excluded.
   Same gap for "unit absent from release -> path match" (line 344): no test.
2. The new baseline job/inputs rely on the accepted manifest being internally honest, and the queue never calls
   `gate_inputs` on it (review_queue.py:324). `accepted_release` checks release hash, job in imports, entry inputs == manifest
   files (277-302), which is what the task asks, and the tamper tests cover those (492-525). But it does not check the
   release's campaign/coverage against the current build nor that on-disk bytes of baseline inputs match (rebuild's
   read_manifest does the latter at promotion). Cheap hardening: call `gate_inputs(root, accepted)` when a gate exists
   (needs the coverage manifest hashed; the test fixture sets coverage_manifest=release name, so the fixture would need a fix).

## notes
- Tamper resistance: refusals are real. Release bytes vs `files[name]` (291), release must be hashed (289), canonical id check via
  `check_release`/`load`, job in imports (297), input path+sha vs `files` (300). Tests for each at :492-525. Excluded entries skip the
  job/inputs checks (295), correct since they carry none. A coordinated edit of release+manifest hash is not detectable here by design
  (rebuild/promotion seals it).
- Fallbacks: no gate -> `released is None` -> elif path match, byte-for-byte as before (test :475). Gated manifest, unit in release
  with status excluded -> stays excluded (338-343). Unit absent from release -> path match; gate_inputs requires the release to cover
  every coverage unit (569), so absent only means a unit new since the release; it can match only a path the manifest already
  imports, so it cannot resurrect an excluded/rejected unit. Safe; consistent with the worker's stated choice.
- Behavior difference for gated manifests: baseline inputs now come from the release entry rather than the unit's
  current pinned paths in accepted files (previously `require(all(path in accepted_files))`). Equal for unmoved units, and
  strictly more correct when the input set changed.
- candidate_job: written only when status == baseline and job != item.job (517), so candidate_job == job cannot be produced; check_release
  refuses it (549-551, tested at :546) and refuses it on accepted entries. An accepted entry cannot carry it via build_release (status
  accepted -> job = item.job). gate_inputs refuses importing it (578-580) but only same-source/same-path; the same bytes under another
  path/adapter are not blocked (adapter is forced equal to job.source so the adapter case is covered). Releases frozen without
  the field cannot guard (acknowledged, test :527). Minor: two entries sharing one `job` are not rejected by check_release (only a
  per-entry uniqueness of unit_id/source_id at 535); a candidate_job equal to another unit's accepted job would make the gate refuse
  spuriously. Exotic.
- Extra fix (excluded -> projection_sha256 None, 515): no effect on the promoted release (no excluded entries; all 12 accepted). For
  newly built releases it only removes a case that check_release (542) already rejected. Behavior for a no-baseline unit absent
  from the live catalog is unchanged (projection hash already None).
- Baseline projection still comes from the live catalog and is not cross-checked against the prior release's projection_sha256
  (worker noted it; gate_projections catches it at rebuild).
- Regression power: mutant "baseline job falls back to candidate path" (`baseline_job = dict(job)`) made 5 of 8 new tests fail
  (e.g. test_unreviewed_unit_keeps_the_old_job_and_inputs at :427), so the main behavior is pinned. Only the excluded/absent cases survive (should-fix 1).
- c4 operational: queue must be run with the gated `--accepted-manifest data/import-manifest.json`, and the next promoted manifest must import the
  OLD bundle for baseline units (per handoff); otherwise the ungated path match drops baseline as before.

## re-review

Verdict: ACCEPT (no must-fix).

- `python3 -m unittest tests.test_review_queue`: 28 tests OK (272s), canary and AcceptedManifestGateTests included.
- Promoted data/import-manifest.json read-only: `gate_inputs` and `accepted_release` OK, 12 accepted entries, unique jobs, so the new
  shared-job rule (review_queue.py:554-555) does not trip it.
- Excluded fall-through mutant (line 338 skips excluded release entries) now FAILS exactly
  `test_excluded_release_entry_stays_excluded_and_absent_unit_uses_path_match` (the other 9 pass). should-fix 1 resolved. The test
  also covers the absent-unit path match via an older coverage manifest, and asserts the RRA unit keeps its old job there.
- should-fix 2 resolved: `accepted_release` calls `gate_inputs` on the accepted manifest (294); the earlier local checks stay.
- Altered tamper tests (tests/test_review_queue.py:553-585): the same tampering is still injected and still refused. Release-bytes and
  unhashed-release cases are unchanged. The removed-import case now expects 'exactly the released job' (gate_inputs, 577) and is not
  weaker (same refusal, earlier). The input-hash case uses the alternation 'input differs from the reviewed release|differs from the
  accepted manifest' (line 573); both alternatives are real refusals, but pin it to the gate message for strictness (note only).
- Shared-job rule: considers non-excluded entries only, so excluded entries sharing a path stay legal; reasonable.
