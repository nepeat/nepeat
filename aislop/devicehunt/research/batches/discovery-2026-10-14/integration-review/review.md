# integration helper review — changes required

Reviewer `/root/odm_bulk_full`; source reviewed before any accepted-input plan
existed. **Do not run this revision.** Reviewed helper SHA-256:

`04c9449ebcaf3da1b3852f57d083c018dc4fbdad5ebad15561d250c5a1f36fb5`.

[Reviewed source snapshot](stage-reviewed.py), [isolated probes](probe_helper.py),
[probe results](probes.json). No `stage.main()` invocation, accepted-input plan
creation, staging import, network request or shared-state write occurred. Only
this `integration-review/` directory was written. Prior source reviews unchanged.

## blocking fixes

1. **Preserve all old columns when comparing live → candidate.** Lines 71–73 drop
   `observed_at`, `fetched_at` and `cache_path`. Those exclusions are appropriate
   for rebuilt-versus-candidate comparisons, but this is a new-source-only import
   into a live backup. Every old value should remain byte-for-byte/logically exact.
   The isolated SQLite probe changes an old row only in these three fields: the
   helper reports zero missing rows although full comparison finds one changed row.
   Include every old column in this preservation check and retain multiplicity
   checks. Continue excluding/rebuilding only FTS tables as intended.

2. **Carry accepted hashes through copying and final acceptance.** Lines 37/51/53
   verify candidates/snapshots/dependencies before copying; `clone` then compares
   the destination to the source's *current* bytes, not the accepted hash. If an
   input changes between these steps, the manifest can pin the changed bytes and
   `prepare_curated` does not compare `original_sha256`. The isolated probe confirms
   that `clone` accepts changed, unreviewed bytes after a caller's acceptance check.
   Require each copy to match its specific accepted hash after copying and again
   before success, including each staged candidate itself. Require every optional
   `text_sidecar` to have an explicit accepted hash (or a matching hashed
   `evidence_dependencies` entry), rather than introducing an arbitrary current
   sidecar hash at line 64. Pin the parsed `accepted-inputs.json` hash at startup
   and require it unchanged at success; do the same for the review files. The
   final plan hash currently may name bytes different from those actually parsed.

3. **Constrain group destination names before any writes.** Line 43 interpolates
   `group['name']` directly into a path without `checked_path`, a slug constraint
   or uniqueness validation. Candidate/review/dependency paths correctly reject
   parent traversal; destination names should receive the same treatment. Require
   unique nonempty names matching a simple slug such as `[a-z0-9][a-z0-9-]*` and
   confirm each resolved destination is under `DATA/raw`. Validate all groups and
   destinations before creating the first destination directory.

The acceptance-plan review should also check the exact finite group/source/record
counts and bind every candidate hash to its specific independent source review.
A top-level `accepted: true` plus a review file hash does not mechanically prove
that the review accepted that candidate. This can be supplied explicitly in the
new plan and checked before running; no plan exists yet, so no plan acceptance is
being offered here.

## mechanisms that are appropriate

- Live DB and manifest are read/hash-only, catalog work targets a new backup,
  and all candidate sources must have IDs absent from live and the other groups.
  The helper checks orphan source references and duplicate source IDs before import.
- Curated preparation, original capture hash checks and dependency path confinement
  are present. Byte-identical copying retains relative sibling/nested snapshot
  dependencies, so existing `prepare_curated` and offline rebuild paths can work.
- Existing manifest job order, file pins and campaign gate remain intact. Three
  new curated jobs can be appended without changing the reference-document job;
  the complete 61-entry PDF bundle and its existing siblings remain referenced.
  Document/page counts are explicitly unchanged. No partial replacement PDF
  bundle is created.
- Record/fact/source deltas, database integrity, foreign keys, owned baseline,
  campaign input/projection gates and the production code freeze are checked.
  Candidate source counts plus device/record/document counts are written in the
  schema expected by the unchanged rebuild loader; other expected counts remain
  inherited. Facts are checked directly because the rebuild schema has no expected
  fact-count field. Full fresh rebuild and explicit candidate comparison remain
  necessary after staging.
- Curated imports may commit per group, but only the candidate DB is affected.
  An interrupted/error run can leave copied inputs, a partial candidate or an
  incomplete candidate manifest; no live promotion occurs here. Treat absence of
  a complete verified `stage-report.json` as incomplete staging. Preflight the
  report path too, and document recovery using new run paths while preserving
  partial artifacts; do not silently reuse, overwrite or delete them.

This is helper review only. After fixes, return the new helper hash for recheck;
then independently review the concrete accepted-input plan and the eventual
staging/rebuild results before promotion. No production code change or new full
suite is required merely to fix this private helper, but meaningful isolated
checks should demonstrate the corrected hash/preservation/path behavior.
