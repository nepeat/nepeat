# c3a review: immutable coverage manifests (reviewer: Claude Sonnet 5.5, read-only)

Scope: devicehunt/coverage.py, tests/test_coverage.py vs docs/campaign-state-contract.md (coverage paragraph, E1),
docs/campaign-plan.md c3. `python3 -m unittest tests.test_coverage tests.test_campaign -q`: 26 tests OK.
Experiments ran in temp dirs (fixture archives); scripts are in the session scratchpad. Real archive: only
build/validate, read-only; before/after file size+mtime snapshot of data/archive identical.

## What holds (verified by running)
- Real `c2-live-run-1` builds: 12 units, 36 receipts, 1117 artifacts, counts 12/12/12/12, zero-pending. validate with
  roots passes. Every embedded wrapper is byte-identical to `receipts/<id>.json` (sha256(raw)==wrapper_sha256 and
  raw==canonical(wrapper)). run-2 (all `reused`) selects the same 12 bundle receipts as run-1 (different manifest id, as
  intended). Build twice = equal. Run recipes are not consulted, so the c2b code-identity drift does not matter.
- Closure for generated manifests is exactly bundle -> validation -> registration + inputs/outputs of those payloads.
- Mid-unit stop (register+validate done, bundle not): unit is `pending`, no partial receipts frozen; counts equal
  Runner.report for that run. Resume-in-place (same run id, executions span attempts) freezes 12 receipts correctly.
- all-pending / budget_exhausted freeze with zero receipts and artifacts. Explicit zero fixtures have zero_proof.
- build/validate/CLI wrote nothing (tree with locks identical). A missing index/state fails (StateError/ArchiveRecovery).
- publish: temp file removed on validation failure and success; link is exclusive; directory fsynced after link.

## Findings

### S1 should-fix: validate() never compares receipts to the archive; a consistent forgery validates
coverage.py:147, 188-200. `wrapper_sha256` is digest of the embedded wrapper, so it is tautological with the identity.
With archive_root given, only artifact bytes are rechecked; the receipt files/index are never read.
Scenario (verified): take a real manifest, edit a bundle wrapper to outcome=failed, recompute wrapper_sha256, units,
counts and manifest_id -> `validate(..., workspace, archive_root)` returns OK. Likewise edit unit_id, candidate_sha256,
scope, comparison, definition.budgets, run.state, unknowns, evidence_release and recompute only manifest_id: all validate
(identity is a content hash, not a signature; only run/definition/scope are covered against accidental change).
Fix: when archive_root is given, for every receipt require `digest(read_bytes(receipts/<id>.json)) == wrapper_sha256`
and `canonical(wrapper) == raw` (the archive stores exactly canonical(wrapper), so this is the right comparison; also
cross-check index wrapper_sha256 if the DB is open), and cross-check unit fields (unit_id, source, scope, candidate_sha256,
comparison) against the root payload. Say in the docstring/note that without archive_root it is a structural check only.
Verified by running.

### S2 should-fix: validate() does not enforce outcome/coverage invariants or explicit-zero proof
coverage.py:163-176, 118-130. Honesty rules are enforced only by the runner at build time.
Scenario (verified): wrapper+unit outcome=`failed` with coverage.observed=True and validated_complete=True validates, and
`_counts` then reports observed=1 and failed=1 for the same unit. Unknown outcome strings are counted in no bucket.
explicit_zero check (173-175) only tests two booleans; it never checks the candidate's `zero_proof`, and coverage.explicit_zero
may be True with outcome != explicit_zero.
Fix: allow outcome in {validated_nonempty, explicit_zero, failed, blocked, unknown}; require observed==validated_complete==
(outcome in validated_nonempty/explicit_zero) and explicit_zero flag == (outcome=='explicit_zero'); with roots, load the
candidate (units[].candidate_sha256) and require non-null zero_proof for explicit_zero. Also require payload unit_id ==
unit_id, unique unit_ids, no two units sharing a root, and parent kinds (bundle->campaign-validate->campaign-register)
and matching unit_id/unit_revision down the chain (lines 150-154 accept any kind and any unit). Verified by running (C2/C3).

### S3 should-fix: locations are mutable archive state inside the identity; no registration receipts frozen
coverage.py:101-104, 91-93. Locations come from the live `locations` table (all scopes/paths for the sha, including
unrelated registrations), not from the run's closure. Scenario (verified): register a second copy of an input after
freezing; `build(runner,'r1')` now returns a different manifest_id for the same run, so "same run -> same manifest" and
the "already published" check no longer hold, and unrelated registrations leak in. Also the queries at 99-104 run outside the
readiness/generation check used by verified_receipt, so a concurrent publisher races them.
The manifest also has no artifact-registration receipts: byte_count and location are asserted, not proven. A location
list without registration receipts is not sufficient provenance for the stated "required receipt closure"; only the hash is
self-verifying, and workspace-scope bytes (most of the 1117 artifacts on the real run) are not preserved at all, only pinned.
Fix: derive locations from the closure: inputs[].path for saved inputs, objects/<sha[:2]>/<sha> for archive outputs, and
include the matching `registration-<digest(canonical(payload))>` artifact-registration receipts (ids are reconstructible
from path/size/scope) in `receipts`, treating them as roots-by-reference. Then the closure is deterministic and
self-describing. Alternative: drop locations from the identity and keep them as a non-identity advisory block.

### S4 should-fix: validate() pins CLAIMS/UNKNOWNS wording of today's module
coverage.py:25-29, 137. `manifest.get('claims') == CLAIMS` means any later wording edit invalidates every published
immutable manifest. Fix: validate by schema version (a per-schema table of allowed claims text, or check only that it
contains the required disclaimers), and never edit a published schema's constants. Read only.

### S5 should-fix: stopped runs diverge from Runner.report when jobs came from an earlier run
coverage.py:61-67 vs campaign.py:420-423. report() counts a unit as accounted-for when its bundle job (current recipe) is
complete in any run; build() counts it only if this run recorded a bundle execution. Scenario (verified): run rC stopped
after the first register stage while all 12 jobs were complete from rB: report validated_complete=4, manifest 0 and
pending=4, so the later report and the frozen manifest disagree and completed evidence is shown as pending. The per-run
rule is defensible (and is the only correct one for the recipe-drift case), but it is undocumented and the units
carry no hint. Fix: state the rule in the c3a note, and either add a per-unit field such as
`prior_result_available: <bundle receipt id>` (not counted), or take roots from the run's own executions plus an explicit
documented "carried" bucket. Also re-read the run row after selection and fail if state/reason/execution count changed
(build holds no campaign lock, so a concurrent `resume` can interleave). Verified by running.

### N1 note: any pending publication intent in the archive poisons build()
coverage.py:77,89,103 call verified_receipt/resolve -> require_ready, which is global. Verified: an interrupted unrelated
`store_bytes` (intent left at after_intent) makes build raise ArchiveRecoveryError "pending publication requires
recovery"; validate() without roots still passes. Fail-closed is arguably right (index/head not acknowledged) but E1's text
says unrelated pending state must not poison selection; the test only corrupts an unrelated object file. Either document
this as intended (recover first) or add a test pinning it. Real-archive path unaffected today.

### N2 note: publish() hardening
- coverage.py:211-219: mkstemp mode 0600; the published manifest is not chmod 0444 like archive immutables (`_publish`).
  Verified mode 0o100600. Add `os.chmod(temporary, 0o444)` before link.
- 208: `mkdir(parents=True)` entries are not fsynced (artifacts._mkdir does). Directory fsync after link (220-224) is correct.
- 219: a racing publisher gets raw FileExistsError, not CoverageError; the pre-check at 210 is only advisory (link is the
  real exclusion). A SIGKILL between mkstemp and unlink leaves a hidden `.coverage-*.json`; harmless but mention it.
- 210: an existing file with the same name but different bytes is reported as "already published" without comparing.

### N3 note: validate() accepts non-canonical file bytes; CLI details
coverage.py:245-247. The file bytes are not part of the check: a pretty-printed copy, a copy under another filename, and a
file with a duplicate key (last wins) all validate (verified). A NaN token raises ValueError not CoverageError.
Fix: in CLI validate require `raw == canonical(obj)+b'\n'` and filename == manifest_id+'.json'; parse with
object_pairs_hook rejecting duplicates and parse_constant rejecting NaN. Also validate() silently skips the disk check if
only one of workspace/archive_root is given (188; verified): require both or neither. Also `freeze --archive data/archive`
resolves against cwd (Archive uses Path.absolute()) while `validate` resolves against --workspace (inherited from
campaign.py but now inconsistent within this CLI); the CLI test only passes absolute paths.

### N4 note: bundle receipts are not checked against the job row
coverage.py:61-82. payload['recipe_id'] is never compared with jobs[job_id]['recipe'], nor e['result_id'] with
job['result_id']. The definition-sha plus unit_revision check does bind units to the run (same definition => same
revisions; verified two runs, mid-unit stop, resume), so this is defense in depth only.

## Test gaps against E1 and the above
tests/test_coverage.py (all passing) does not cover: an all-failed run (every unit failed; only one failed unit in the
mixed run); a stopped-mid-unit run; a resumed run (executions across attempts); a second run using `reused`/`recovered`
executions; two-run determinism/selection; forged-but-consistent wrappers (the 'bytes' case at line 109 only passes because
wrapper_sha256 went stale, and no test recomputes it); outcome/coverage invariants; location drift; an unrelated pending
publication intent; publish exclusivity/temp-file cleanup/mode; a failing validate in publish (no file, no temp); CLI
validate failure and read-only CLI (the read-only test calls only build()); a missing selected receipt at build time
(only a missing object is tested).

## Answers to the focus questions in one line each
1 Closure exact for generated manifests, but no registration receipts and mutable locations (S3); unrelated corrupt
  object does not poison, unrelated pending publication intent does (N1).
2 Counts honest for build output; validate cannot enforce it (S2); mid-unit stop matches report; report divergence S5.
3 Roots from executions: correct for executed/reused/recovered, resume, two runs; definition sha + revision suffices (N4).
4 Identity deterministic (sorted canonical, no floats in the run block) except S3 drift; wrapper bytes equal the archive
  file bytes for real receipts, but validate never reads them (S1).
5 publish: exclusive, atomic, no leftovers, validated before link, dir fsync right; mode/mkdir notes (N2).
6 Read-only confirmed, including locks, real archive and missing DBs.
7 See test gaps.

No blocker: nothing writes outside the manifest path or yields a dishonest manifest from a real run. S1-S3 should land
before c3b relies on validate() as a gate; the real c2-live-run-1 manifest can be published now, but S3 means its id is
tied to today's location table.

VERDICT: ACCEPT with notes

## fix verification

Re-ran tests.test_coverage + tests.test_campaign: 28 tests OK. Re-ran my earlier experiments on fixture archives, plus new ones
(exp4: forgeries, strict load, CLI from another cwd, later-copy drift). Live archive: build+validate of `c2-live-run-1` and
`c2-live-run-2` read-only, passes with both roots (36 receipts, 1117 artifacts, counts 12/12/12/12, explicit_zero 2), builds
are repeatable, data/archive size+mtime snapshot unchanged. Nothing published into data/.

- S1 fixed (with one limit). A recomputed wrapper+wrapper_sha256+units+counts+id forgery (bundle `occurrences`=77, and the earlier
  outcome=failed edit) is now rejected "embedded receipt differs from the archive" with both roots and with archive_root alone;
  it still validates with no roots (structural only, as the docstring says). Chain kind and unit_id/unit_revision are enforced
  (coverage.py:167-176); a non-campaign receipt kind is refused. Limit: the run, definition, budgets and unknowns blocks cannot be
  verified offline; an edit of them with a recomputed id still validates even with roots (verified). Could be checked against
  the state DB in CLI validate; acceptable to document.
- S2 mostly fixed, one part open. Overclaim (failed with observed=True) is rejected; duplicate unit_id and two units sharing a root
  are rejected; candidate sha must be selected; the explicit-zero proof check reads the candidate when both roots are given
  (coverage.py:215-221, exercised by the new test). Still open (partial): unit `scope`, `source` and `comparison` are copied
  from the receipt payload but not cross-checked; with a recomputed id they can be edited and the manifest validates (verified).
  The payload carries all three (campaign.py:319-339), so a three-line equality check fixes it.
- S3 fixed for determinism, partial for provenance. Locations are derived from the frozen payloads (coverage.py:102-118): after
  registering another copy of an input the rebuilt manifest is identical (verified True; was False), re-publish is refused.
  artifact-registration receipts are still not in the closure; byte_count remains the index value, but bytes are now verified
  against byte_count and sha when both roots are given, so I consider this adequate for c3a and note it for c3b.
- S4 fixed, weak. Prefix check replaces exact wording and evidence_release must be null. The prefix is the only constraint:
  `coverage only: device absent everywhere` validates (verified). A required-disclaimer substring check would be tighter; note.
- S5 fixed as designed. `not_reached` replaces `pending`, so a stopped run no longer asserts completed work is pending; the
  run-level rule is documented in the code comment (coverage.py:76-78). Open (note): build() still takes no snapshot, so a
  concurrent `resume` between the run read and the executions read is not detected (re-read the run row at the end). The label
  also covers a unit that was started but whose bundle never finished; consider saying so. The counts key differs from
  Runner.report's `pending`; fine if c3b reads the manifest key only.
- N1 unchanged (global readiness: an unrelated pending publication intent still makes build() raise, verified); not claimed
  fixed, still an undocumented decision plus untested.
- N2 fixed. Published file mode is 0o444 (verified), only the one file remains in the directory, republish is CoverageError, a
  nested new output directory publishes, the parent of a newly created directory is fsynced. Only the immediate parent is
  fsynced when `parents=True` creates several levels; trivial.
- N3 mostly fixed. `load()` rejects pretty-printed, duplicate-key and trailing-extra-newline files; the CLI always validates
  against both roots; both subcommands resolve `--archive` against the workspace (validate passed from cwd=/ with a relative
  archive; a wrong archive fails). Open (small): `NaN` raises a bare ValueError instead of CoverageError; the CLI does not require
  the filename to equal `<manifest_id>.json` (a renamed copy validates, verified); the library `validate()` still silently skips
  disk checks when only one root is given (candidate and artifact checks need both; receipt compare needs archive_root only).
- N4 unchanged (job-row recipe/result cross-check); defense in depth only.
- New tests: test_validate_checks_archive_bytes_honesty_and_strict_files and the identity/not_reached test pass and cover the
  forged wrapper, overclaim, strict files, later-copy stability and not_reached. Still no test for mid-unit stop, a resumed run,
  all-failed, an unrelated pending intent, scope/source/comparison tampering or publish temp-file cleanup.

Remaining open items are a small unit-field cross-check (S2), a documented offline limit on run/definition blocks, and notes.
None changes the honesty of any manifest built from a real run, and every item I called a should-fix that could mislead a
reader of a generated manifest (S1 forgery, S2 overclaim, S3 drift, S5 pending) is closed.

VERDICT: ACCEPT with notes
