# curated import atomicity — independent code review

reviewer `/root/bytedance`, 2026-10-13 UTC. **ACCEPT** the bounded implementation. no material correctness or compatibility blocker found. this is code acceptance; coordinator-owned full guarded tests, clean rebuild, preservation and promotion checks remain pending.

Read finalized [handoff](../curated-import-fix/handoff.md), [author checks](../curated-import-fix/checks.json), implementation diff, both preserved original production files, [accepted design](../curated-import-design/design.md), [design review](curated-import-design-review.md), and current source/test files. Original and author-log/report hashes verified. Schema literal is exactly unchanged. Only the two assigned production files and new test module appear in the implementation diff.

Frozen SHA-256, checked before and after independent verification:

| file | sha256 |
| --- | --- |
| devicehunt/importers.py | `56dd6d4c6d97d7b1764321c5ce075a5fe42a4f0c91ad6f024ed55a0d1f5c88ab` |
| devicehunt/catalog.py | `1e162a142420e16806850baec3b024199d62db8e61bd3cef6d0aee849871e6e6` |
| tests/test_curated_import.py | `98a019a8279d68fd38c193088ebd498d2274b0c006462874336b1f9a054f7b78` |

## implementation findings

`prepare_curated` loads JSON once, prepares every declared source occurrence and matching row before any catalog write, and reads/stats every consumed snapshot before entering the savepoint. Source selectors retain equality-based matching; unreferenced row contents remain ignored, while missing selectors fail as consumed input. Empty sources stay a no-op even without devices or inside a caller transaction. Invalid consumed shapes produce diagnostic ValueError; I/O and execution/programming errors retain their types.

Required scalar operations and iterable pair/triple unpacking preserve previously successful SQLite coercions. The validator retains skipped tuple fields, original dictionaries/order, ignored source extras and row revision, duplicates and raw JSON. Facts retain `value is not None and str(value)`; identifiers retain truthiness before `str`; false affiliation companies remain skipped. Null fields allowed through INSERT OR IGNORE preserve its existing skip behavior rather than adding a metadata policy. No URL/role/uniqueness/content/finite-number/path restriction was introduced.

Prepared snapshot metadata comes from local bytes/stat, not payload hashes, and source execution requires its internal metadata type and matching stored path. Ordinary `Catalog.source` behavior remains unchanged. End-to-end independent instrumentation confirms one snapshot byte read; execution does not reopen it. The existing immutable-capture contract and coordinator hash checks still apply; preparation is not a concurrent-file-change guarantee.

One savepoint encloses source mutation, add calls, ordered per-source count/orphan cleanup, final FTS rebuilding and release. Source/row order and occurrence counts stay intact, including first identity metadata and last duplicate record/raw data. Per-source orphan deletion preserves the A-to-B identity-move behavior that final-only cleanup would change. One final device FTS rebuild replaces repeated rebuilding; document FTS is untouched. Other callers retain source/finish defaults.

Standalone release commits the bundle. Nested success deliberately remains uncommitted and caller-owned, as the coordinator accepted. Execution, SQL/programming/index errors, release failures and interruptions propagate the original exception after savepoint rollback. Cleanup handles BaseException, never releases after failed rollback, and cannot mask the original exception through failing diagnostic hooks. A cleanup failure leaves connection recovery/discard to its owner and is explicitly reported; no successful rollback is falsely claimed.

## independent verification

[Focused test log](curated-fix-independent-focused.log): **71 tests passed in 49.512 seconds**, including the final 16 curated tests and affected catalog/rebuild/listing/specification/source-fact/accepted-manifest tests. Log SHA-256 `45d2dffc3678928aa3dbff65f47b23c9b61a919d1a7e8d21817b64a880dc4503`. Existing affected modules emitted unclosed-database ResourceWarnings; no test failed. The new curated module guards network/external processes and covers late shape/snapshot preflight failures, full logical/FTS rollback, original exception identity, release/interrupt/cleanup failures, standalone/nested visibility and caller rollback, per-source metadata moves, duplicates/raw/skip/coercion behavior, snapshot reuse and unchanged defaults.

[Reviewer probe](curated-fix-independent-probes.py) and [result](curated-fix-independent-probes.json) independently:

- verified pinned JSON hashes and pure preparation for all48 accepted curated jobs: 1,853 source occurrences / 2,201 rows; all50 wave3 staged jobs: 1,857 / 2,206. Both held manifests remained byte-identical.
- compared the actual preserved original importer/catalog with current code in two tiny temporary catalogs. Null identity fields under INSERT OR IGNORE, skipped null/false tuple fields, ignored raw revision/unreferenced row contents and final logical/FTS output matched.
- instrumented the entire snapshot-backed import and observed exactly one byte read; saved source hash came from trusted bytes despite an untrusted payload hash.

The probe forbids network/external processes and opens no shared catalog. All three frozen source/test hashes remain unchanged after tests/probes. No production, live database, manifest, export, archive or shared-document edit, network, install, delegation, broad benchmark or full-suite run was performed by this reviewer.

Stop condition met. No further code correction is required for this milestone; coordinator may freeze this accepted code and proceed with full-suite and new-path clean-rebuild comparison to the old-code wave3 candidate before promotion. No runtime speed claim or integration acceptance is implied here.
