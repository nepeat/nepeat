# c2 runner review (Claude Sonnet 5.5, independent, read-only)

Scope: devicehunt/campaign.py, campaign_sources.py, campaign_state.py (schema 3, job API,
upgrade_campaign_jobs), the artifacts.py/capture.py changes, tests. All experiments used temp
dirs under the scratchpad; no network, nothing under data/ written (the live
capture-leases.sqlite was only copied, to read its schema/lease rows).
Ran: tests.test_campaign (13 ok), tests.test_campaign_state (34 ok), tests.test_artifacts (8 ok),
the new test_capture test (ok). Not run: full suite.
Verified = ran something; read = by reading only.

## blockers

### B1. Bundle job is reused over a changed validation (stale coverage, stale comparison)
campaign.py:119,205-215,291-317. Job identity is (unit revision, stage, recipe). The bundle
recipe (campaign.py:107-108) hashes only campaign.py, and does not depend on the validate recipe
or the validation result id. If the validator code changes (rra.py, ised.py, campaign_sources.py
compare, catalog.py) the validate job gets a new recipe/job and re-executes, but the bundle job id
is unchanged, is `complete`, and is reused with `_check_result` (checks only kind, unit_revision,
recipe_id, outcome).
Scenario (verified, exp1): run-1, then change only the `devicehunt/rra.py` hash. run-2 reports
{'reused': 2, 'executed': 1}; the bundle receipt's `validation_id` is the OLD validation
(b4c8a78c...) while the current validate job result is 56525d24.... The report shows
`change_from_previous_run: unchanged` and coverage/comparison from the old parser. This directly
breaks the plan's "a changed parser yields new jobs over the same bytes" and the C1 "compatible
results only" rule, and the report would over-claim validated coverage.
The same hole applies to `unit.expectations` (not in `Unit.revision`, campaign.py:93-95): an
edited pin set with unchanged files yields a reused bundle with a stale `comparison`.
Fix: bind the bundle (and validate) job to its upstream: put the upstream result id and a hash of
`unit.expectations` into the bundle recipe/job identity (or make `_check_result` compare
`validation_id` to the current expected id and re-run when it differs, using a new result id that
includes the validation id). Add a regression test (the exp1 scenario).

## should-fix

### S1. Recipe code hashes omit real dependencies and the runner itself for stages 1-2
campaign.py:24,107-111; campaign_sources.py:40,80. Validate recipes hash rra.py,
rra_company_names.py, ised.py, catalog.py, campaign_sources.py but not documents.py (`dependency`),
fcc_index.py, importers.py (`Page`), oracle_evidence.py (`plain`), rra_celestica_historical.py -
all imported by the validators and all listed in pin_inventory.py REFERENCES / the definition's
pinned_adapter_recipe_policy. Neither the validate nor the register recipe hashes campaign.py,
although `Runner._validate` (occurrence-id formula, output schema) lives there; the bundle recipe
omits campaign_sources.py (`compare`). Editing any of these keeps old job ids, so incompatible
resume/reuse is not refused (read; B1 shows the consequence). Fix: derive the module list from the
pinned recipe-policy paths and add campaign.py to register/validate/bundle, campaign_sources.py to
bundle.

### S2. new_bytes cap accounting undercounts bytes after a crash (verified, exp2)
campaign.py:220-221,223,228-229,336. `before` is read only for a stage that actually executes,
and a recovered job is recorded with new_bytes=0. Bytes written by an interrupted or recovered
attempt (object file, registration receipts, the result receipt) are never charged. Verified: hard
crash during the validate publication, `Archive.recover`, resume: executions sum to 13,608 B while
the archive grew by 18,591 B. A repeated crash/resume loop can drift the real growth past
`new_metadata_and_candidates_bytes_cap` while `_capacity` keeps seeing a smaller `spent`.
It is reachable without a crash: `_capacity` fires inside the `_register` loop (campaign.py:252)
after earlier registration receipts are already durable; the CapacityStop leaves no execution row,
and the resumed attempt resets `before` (`_footprint`), so those receipts are never charged.
Fix: charge the recovered result (receipt file + archive-scope objects reachable from its outputs)
on the `recovered` execution, and take the `before` sequence at stage start from the previous
execution rather than per attempt (or compute spent from receipts/objects linked to the campaign).

### S3. RECEIPT_ALLOWANCE is not an upper bound; index growth is not counted
campaign.py:29,225,252,284,312. 4,096 B per receipt is used as the worst case, but the ised-rel-25869
bundle receipt is already 4,114 B (verified), and the register and validate receipts list every
input: the oracle RRA unit has 520 inputs, about 80 KB of `inputs` JSON in each (computed from the
pinned definition; read for the receipt sizes). So `_capacity` can pass and the write exceeds the
cap by about 80 KB x 2. Cap is 64 MiB so no practical harm now, but the gate is not a bound. Also
`_growth` (336-347) sums receipt files plus archive-scope objects only: index.sqlite grew 57,344 B
while new_bytes was 36,012 B (verified, exp5). Fix: use the exact canonical size of the payload
(`len(canonical(wrapper))` with margin) before publishing and record index growth separately in the
report as "physical storage growth" (the plan requires that field and it is absent).

### S4. `observed` counts failed units
campaign.py:296,380; test_campaign.py:237 pins `observed == 4` with three failed units. The
contract calls for "successfully observed units" and the plan says failed/blocked work is
accounted for, not observed coverage. A failed unit (saved HTML error page, malformed CSV) is
added to `observed`. validated_complete/explicit_zero are correct (verified in the failure test).
Fix: set `observed` only for validated_nonempty/explicit_zero and add an explicit `failed` bucket
check (or rename to `registered_inputs`); update the test.

### S5. Cap and floor are optional; job/path caps unenforced
campaign.py:75-77,321-325. `load_definition` validates only the network budgets. A definition
without `new_metadata_and_candidates_bytes_cap` runs uncapped (`cap is None` skips the check) and a
missing `free_space_floor_bytes` becomes 0. `max_source_jobs`/`max_referenced_source_paths` are
never compared with len(units)/files. Fix: require integer values for the cap and floor and check
the two counts at load. (read)

### S6. Interrupted 2->3 upgrade needs manual file surgery
campaign_state.py:396. A crash after the O_EXCL create but before the backup finished leaves a
0-byte/partial `capture-leases.before-c2.sqlite`; the retry is refused forever ("backup already
exists"), as is a retry after any fault before commit (the test encodes it by renaming the file).
`upgrade()` (c1b) instead verifies an existing backup equals the current snapshot and continues.
Fix: if the backup exists and its snapshot equals the live snapshot, proceed; otherwise refuse
(read; refusal verified by test). The snapshot is also taken before `BEGIN IMMEDIATE` (line 403),
so a non-publication-lock writer could slip between snapshot and begin; low risk while quiesced.

## notes

- N1 identity binding. `run()` never compares `definition.sha256` with the campaign's earlier
  runs, and `unit.revision` binds definition_version, not the definition hash (campaign.py:93,160).
  `spent` is keyed by campaign_id, so a raised cap in an edited definition (same id/version)
  applies on the next fresh run. `resume` correctly refuses (definition sha, inventory sha, every
  recipe id incl. Python version; verified by test_incompatible_checkpoint) - only for the recipe
  case; definition-sha and Python-version refusals are untested.
- N1b `run()` (campaign.py:159-164) never checks for an existing `running` run of the campaign. After
  a hard crash a user who calls `run` instead of `resume` completes the pending jobs under the new run,
  but the crashed run stays `running` forever in status/report (read). Suggested: under `_exclusive`
  refuse `run()` while any campaign run is `running`, pointing at `resume`. Also a `budget_exhausted`
  run is effectively unresumable (raising the cap changes the definition sha, which `resume` refuses),
  so the only forward path is a fresh `run()`, where the N1 "spent keyed by campaign_id" point applies.
- N2 crash windows. Hard crash before complete_job: verified recovered once (test). Crash inside
  `archive.receipt` (after_intent): `_execute`'s `require_ready()` refuses with ArchiveRecoveryError
  until `Archive.recover(...)`; after recovery resume finishes (verified exp2: reused + executed,
  no duplicate jobs, no duplicate receipts). Run stays resumable, but untested and undocumented.
  No wall-clock/unordered data reaches result payloads (`recipe`, hashes, validator `error.message`);
  recompute never happens once the receipt exists (`_existing` first), so the receipt-id conflict
  path cannot be reached by one runner (read). IDs are full sha256: no collision concern.
- N3 `_existing` (campaign.py:231-235) catches KeyError from the whole dependency walk, not only
  "receipt absent"; a missing referenced receipt is retried as fresh work and then re-raised as a
  bare KeyError from `archive.receipt`. Also complete-job verification KeyErrors (missing receipt)
  surface as raw KeyError, not StateError.
- N4 explicit zero is not re-proved by the runner: `_bundle` (campaign.py:296-298) trusts the
  adapter's outcome and never checks `zero_proof is not None`. Both validators do require proof today
  (rra.validate empty_result; ISED v2 total==0 page), so this is a hardening gap only.
- N5 report. `change_from_previous_run` is vacuous in practice (identical jobs are reused, so the
  result id is the same) and `previous` is ordered by wall clock/id (campaign.py:360,
  campaign_state.py runs ORDER BY start_wall); a prior run stopped early gives 'no prior run' for
  units it never reached. `report(run_id)` for an old run mixes that run's actions with current
  job state. Missing vs plan: reused-vs-new bytes split, physical growth, unresolved leads.
  `units_accounted_for` is a set of counts, not an accounted-for total; there is no `partial`.
  upstream totals stay unknown (correct); budget_exhausted/pending units never count toward
  validated coverage (verified by test).
- N6 `validated_complete` is independent of `pinned_comparison`: a unit with `differs_from_pinned`
  still counts as validated complete. Acceptable (pins are expectations) but it should be visible
  in the counts.
- N7 `INPUT_ERRORS` includes TypeError/KeyError/IndexError, so an adapter code bug becomes a durable
  `failed` source outcome (reused until the recipe changes - see B1). All validator failures share one
  `failed` outcome with free text, so zero/missing-detail/changed-total are not distinct outcomes
  (A1/A2 wording is "keep distinct outcomes").
- N8 occurrences keep selector ordinals and `logical_sha256`, but not the source file/artifact hash
  or the manifest job ordinal, and the occurrence id binds the logical hash, not the artifact
  (contract lines 27-30,259-260). Unit revision pins the files, so it is recoverable; c3 may need it.

## focus-question answers (short)

3/4 coverage and capacity: see S2-S5, N4-N6 (only `observed` over-claims; `_growth` only SELECTs
  on archive.db, safe, but also charges other publishers' receipts: over-count).
5 read-only: verified (exp3). plan/status/report change no file or mtime, create no capture-locks
  (directory deleted first), no DB; missing state DB -> StateError; missing archive -> error; missing
  index: plan/status succeed (status "complete" without archive proof), report raises. The test's
  tree() skips capture-locks, so it cannot see lock creation (add it back).
6 upgrade: 2->3 atomic (rollback verified, version stays 2); refusals: not quiesced/no decision, wrong
  version/archive/marker, any lease row, existing backup. Untested: lease-row and marker/version
  refusals. Live store: version 2, 0 lease rows, so the lease guard will not block. 1->3 `upgrade`
  verified: same tables/indexes as a fresh `initialize` (only operational_meta/reservations column
  order differs, pre-existing from the c1b ALTERs); start_run works afterwards.
7 adapters: verified for all 12 pinned units (real data, read-only): compare() = matches_pinned,
  and ordinal/certificate/hvin/exact_csv_ordinals/target flag/matches_html_identity/logical hashes
  show zero diffs versus pin_inventory's expectations (ISED v1 vs v2, csv_matches included in the
  row hash exactly as replay does; eligible-manufacturer and model-assertion counts equal). Gap:
  compare() only checks keys present in `pinned_values`; imported_target_assertions, selected_pages,
  saved_response_count, saved_html_files, raw_registry_rows and the ordinal-flag fields are not
  compared, and v1 `matches_html_identity` is True by construction.

## test gaps

Offline-testable now (missing):
1. B1/S1 regression: validator-code-only recipe change must not reuse the bundle.
2. S2: crash (os._exit) at each of register/validate/bundle publication points, including inside
   `archive.receipt` (Archive fault hooks) then Archive.recover + resume; assert the byte accounting.
3. Definition-sha change and Python-version change refuse resume; `run` binding (N1).
4. A1-style fixtures: RRA `exact_rows_ignore_order` reorder passes and a changed cell fails; missing
   detail; each failure keeps a distinguishable error class. ISED unit with unmatched CSV rows
   (accton/edgecore/oracle) and non-target rows; test only uses 4 small units. A 12-unit adapter-only
   compare run takes under 1 s.
5. Read-only: include capture-locks in tree(), CLI status/report paths, missing state DB/index.
6. Upgrade refusals (lease rows, marker/version mismatch, partial backup) and a post-1->3 check.
7. Cap-boundary: a receipt larger than the allowance; explicit-zero-without-proof guard if added.
8. N1 gate: the guard patches only socket.connect/create_connection/OpenerDirector.open/subprocess;
   add socket.getaddrinfo and http.client, and a real-source/accepted-state hash assertion (currently
   only the coordinator's protected recheck).
Live-only, list as remaining: A1 live before/after separate HTTP attempts and cache non-reuse;
blocked/throttled transport outcomes (402/403/429/503) and stale ISED tokens; budget-account sharing
across runs for nonzero quotas (Q/R, c1a surface); live ISED session contract.

## fix verification

Read the new campaign.py/campaign_sources.py/campaign_state.py and tests. Ran (temp dirs only):
`tests.test_campaign tests.test_campaign_state` = 53 ok; my earlier experiments re-run against the new code
(parser-only change, crash+recover accounting, read-only CLI sweep, a cap sweep, a malformed-manifest case).

- B1 FIXED. `Runner.recipe` chains register -> validate(upstream) -> bundle(upstream + expectations hash);
  `_check_result` also compares `registration_id`/`validation_id` with the current expected ids. Re-ran my
  exp1 (rra.py hash changed only): all three stages re-execute, no stale reuse; the new tests
  (parser change, changed expectations -> only the bundle re-runs, comparison `differs_from_pinned`) pass.
- S1 FIXED. `module_closure` (AST walk, incl. lazy imports) now covers documents, fcc_index, importers,
  oracle_evidence, rra_celestica_historical, validation, and campaign.py in every stage. Side effect (note):
  campaign.py imports everything, so all three stage recipes share one closure (artifacts.py,
  campaign_state.py, validators). Any edit to any of them re-executes every stage and refuses resume. Safe
  direction, but expect unrelated edits to invalidate completed jobs.
- S2 FIXED. Growth is measured from the campaign baseline at each stage start, so interrupted and recovered
  attempts are charged. Re-ran the crash-during-publication/recover/resume case: no duplicate jobs, report
  `campaign_growth_bytes == new_bytes`, and `test_byte_accounting_includes_interrupted_attempts` ties
  receipts+objects to the real file delta.
- S3 MOSTLY FIXED. Exact measured growth plus 3x(payload+512) projections, and `physical_growth_since_run_start`
  and `campaign_growth_bytes` are in the report (index growth included). Swept the cap over 3,000..200,000 B
  (about 25 values): 24 stopped at or under the cap; one (cap 60,000) ended at 62,361 B (+3.9%), probably
  index page granularity. So the cap is a tight estimate, not a hard bound; fine for 64 MiB, but do not
  describe it as strict.
- S4 FIXED. `observed` is `validated_complete` only; `inputs_registered` is separate; the failure test
  was updated. Failed units show `comparison.status == 'failed'`.
- S5 FIXED. The four budget keys are required nonnegative ints; unit/path counts are enforced at load;
  the real definition loads (12 units).
- S6 FIXED for the stated cases. An exactly matching backup is reused, an empty one is replaced, a conflicting
  backup and active leases are refused (new test). Residual note: a partially written non-empty backup is
  still refused as "conflicts"/unreadable and needs manual removal (very small window for a ~100 KB DB).
- N1b FIXED. `run()` refuses while any campaign run is `running` (test). `resume` is unchanged.
- N3 FIXED. `_published` is an index point lookup; damaged dependencies now raise from
  `verified_receipt` instead of reading as "absent".
- N4 FIXED. explicit_zero without `zero_proof` becomes `failed`.
- N7 FIXED as written, with a trade-off to be aware of (new note): with INPUT_ERRORS = (ValueError,
  csv.Error), a saved manifest that is structurally malformed (e.g. missing `form` key) now raises a bare
  KeyError and aborts the run (run `stopped: interrupted: KeyError`; I reproduced it) instead of a
  checkpointed `failed` unit; resume will hit it again. HTML-error/malformed-CSV/changed-total cases are
  still ValueErrors and are checkpointed. Acceptable since inputs are hash-pinned; say so in the docs.
- Tests: tree(locks=True) option exists; definition-change resume refusal, broader socket guard
  (getaddrinfo, http.client) present. Read-only re-check: plan/status/report change no bytes/mtimes and
  create no lock files; missing state DB / index behave as before.

Still open (notes only, not blockers):
- N1 `run()` still does not compare the definition hash with the campaign's earlier runs; spent is per
  campaign_id (only the chained recipes now stop stale reuse).
- N5/N6/N8 unchanged: `change_from_previous_run` is near-vacuous; counts do not surface
  `differs_from_pinned`; occurrences lack artifact hash/manifest ordinal; `compare()` key coverage.
- New: `report(run_id)` for an older run measures growth with no upper bound, so it includes later runs; the
  baseline is the campaign's first run, so unrelated archive publications (e.g. the coming SmartNIC/QCT
  integration into the same archive) consume this cap.
- Remaining untested/live-only list from the original report stands (A1 live, blocked/throttled outcomes).

VERDICT: ACCEPT with notes
