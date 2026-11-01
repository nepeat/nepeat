# queue_baseline handoff

## what changed (devicehunt/review_queue.py)
- `accepted_release()` (line ~277): new. When the accepted manifest declares `campaign_gate`, resolves
  `gate['release']` with `rebuild.local_path(manifest_dir, name)` (same as `read_manifest`; lazy import, no cycle),
  requires it hashed in `files` and `file_digest == files[name]`, loads it strictly and runs `check_release`.
  For every non-excluded entry it also requires the released job is in the manifest's `imports` and every entry
  input path/sha equals the manifest's `files` hash.
- `build_queue` (~337): unit baseline comes from the release entry when the gate exists and lists the unit
  (status accepted/baseline: `baseline.job` + `baseline.inputs` copied from the entry; status excluded: none).
  Units absent from the release, and manifests with no gate, use the old (source, path) match unchanged
  (`baseline.job` = the unit's own job there). Items gain `baseline.job`.
- `build_release` (~510): `job = item.job` if accepted or excluded, else `baseline.job` (falls back to `item.job` for
  items without the field). New optional entry field `candidate_job` (= `item.job`) is written only for a baseline
  entry whose job differs from the candidate. Excluded entries now always carry `projection_sha256 = None`
  (previously a unit lacking a manifest baseline but present in the live catalog produced an entry that failed
  `check_release`; only reachable in the moved-path situation).
- `check_release` (~547): if `candidate_job` is present it must be `{source,path}`, same adapter, differ from `job`,
  and the entry must not be accepted. Absent is fine. "job is one of its inputs" unchanged.
- `gate_inputs` (~578): a non-excluded entry with `candidate_job` requires that job be imported 0 times.
- Constants `RELEASE_SCHEMA`/`ITEM_SCHEMA` unchanged. `projection`, `_identity`, `_content`, `projection_sha256`,
  CLI untouched (the `queue` invocation is the same; `--accepted-manifest` already existed).

## refusals
accepted release not hashed in manifest files; hash mismatch; non-canonical/invalid release; released job not in
manifest imports; entry input sha/path differing from manifest files; release adapter differing from unit source;
`candidate_job` double import at the gate.

## compatibility
The frozen release has no `candidate_job`; `check_release` and `gate_inputs` only look at it when present, and
no required key was added. Its entries have job == current unit path so a rebuilt queue would reproduce the same
job. `AcceptedManifestGateTests` passes unchanged; `test_projection_canary_is_frozen` untouched and green.
Note: a release frozen WITHOUT `candidate_job` cannot forbid a double import (test g documents this); only newly
built baseline-with-moved-job entries carry the guard.

## tests (tests/test_review_queue.py, class MovedBundleBaselineTests, 8 tests)
moved path unreviewed -> baseline/old job/old inputs/candidate_job; accepted -> new job, no candidate_job;
rejected -> baseline/old; gate old-only passes, new-only and old+new refused; accepted unit gate wants new;
no-gate fallback incl. moved path -> excluded as before; tampered release/hash/input/import refusals;
frozen pre-field release still passes `check_release` and `gate_inputs`, bad `candidate_job` refused.

## suite results
`python3 -m unittest tests.test_review_queue`: 26 tests (18 pre-existing + 8 new); the only failure in the first run was fixed by the excluded-projection change, new class re-run OK, and the module is exercised again inside the full suite.
Full `python3 -m unittest discover -s tests`: Ran 638 tests in 846s, OK (including the concurrent campaign_state edits).

## open notes for the coordinator
- rebuild.py needs no change (it only calls `gate_inputs`).
- The baseline projection in the item still comes from the live catalog; it is not cross-checked against the old
  release entry's `projection_sha256` (the rebuild gate does that). Easy follow-up if wanted.
- Units absent from an accepted release fall back to the path match (keeps data rather than dropping it); say so
  if you would rather they be baseline-less.
- c4: the next queue must be built with `--accepted-manifest data/import-manifest.json` (the gated one) so the
  old job is carried; the next promoted manifest must import the OLD bundle for baseline units.

## review follow-up (queue-review.md should-fixes)
- `accepted_release` now runs `gate_inputs(manifest_dir, accepted)` and uses its release (so release/coverage pairing,
  imported jobs and input hashes are checked; the pairing is the accepted release's own coverage manifest, not the new
  queue's). The fixture now hashes a real coverage manifest as `coverage_manifest`.
- `check_release` rejects two non-excluded entries sharing one (source, path) job. The promoted release has 12 unique
  jobs, so it still validates (probed read-only).
- New tests: `test_excluded_release_entry_stays_excluded_and_absent_unit_uses_path_match` (fails under the reviewer's
  mutant; verified) and `test_release_with_two_entries_sharing_a_job_is_refused`. An "absent" unit is only reachable when
  the release's coverage manifest predates the unit, so the test crafts an older coverage manifest without it.
- Tamper test messages now come from gate_inputs for input/import mismatches. `tests.test_review_queue`: 28 tests OK.
