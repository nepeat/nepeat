# artifact archive handoff independent review

date: 2026-10-04. reviewer: `/root/archive_handoff_review`.
scope: documentation review only. outcome: **pass for planning handoff**;
no blocking correctness findings. implementation and migration remain unstarted.

reviewed [the plan](../docs/artifact-archive-plan.md) against project instructions,
[goal](../GOAL.md), [progress](../PROGRESS.md), [workflow](../docs/agent-workflow.md),
[evidence policy](../docs/evidence-policy.md), and
[the assignment](artifact-archive-handoff-batch-2026-10-04.md).

## findings and acceptance

- inspected behavior and proposed behavior are clearly separated. the plan's
  current-code table agrees with `fetch` in [__main__.py](../devicehunt/__main__.py),
  `Catalog.source` and schema in [catalog.py](../devicehunt/catalog.py),
  `dependency`/`read_pages` in [documents.py](../devicehunt/documents.py), and
  `prepare`/`import_bundle` in [reference_documents.py](../devicehunt/reference_documents.py).
  current reuse is filename-based; source time is input mtime; extraction metadata
  is a fixed method string; the reference importer rejects duplicate PDF hashes.
  none of those limitations is presented as already fixed.
- authority is sufficient for the next planning milestone: originals plus versioned
  receipts are durable evidence; the archive registry is an index/acquisition store;
  `data/devices.sqlite` remains the accepted hardware catalog. the precise accepted
  lineage-table split is explicitly deferred to milestone 0. cross-database links
  require validation and accepted releases must be frozen independently of the queue.
- extraction lineage covers ordered multi-input hashes, recipe versus run identity,
  failed/repeated derivations, original versus normalized values, saved model outputs,
  locators and review history. source replacement's record deletion in `Catalog.source`
  makes stable claim keys necessary; the plan states that requirement explicitly.
- evidence boundaries preserve separate company roles and chassis/node identities,
  duplicate rows and conflicting claims. identical bytes do not become independent
  corroboration; text hits and unreviewed extraction do not become hardware assertions.
- migration preserves existing paths, complete sibling bundles and historical files.
  it requires hash/path/symlink checks, avoids mutable hardlinks, and forbids silent
  refetch or partial rebuild. [rebuild.py](../devicehunt/rebuild.py) currently validates
  hashed manifest inputs and refuses an existing destination; the proposed changes
  keep new-path candidate and rebuild acceptance.
- comparison limitations are accurately described: [compare_rebuild.py](../scripts/compare_rebuild.py)
  explicitly lists tables and excludes `observed_at`, `fetched_at`, `cache_path`;
  counts plus `EXCEPT` alone cannot prove arbitrary duplicate multiplicities.
  the plan requires additional duplicate-sensitive comparisons and new-table routing.
- milestones and tests are bounded and meaningful: cache/304/failure distinctions,
  changed bytes/recipes, leases and interrupted writes, missing legacy metadata,
  duplicates/conflicts, read-only behavior, offline equality and restored evidence.
  acquisition tests are mocked; schema/importer changes require the full suite.
- changing catalog counts are delegated to PROGRESS; the plan labels its observed
  pending integration state as an initial-read snapshot and instructs a reread.
  no stale totals are asserted as current within the plan.

## checks actually performed and limits

read the named instructions and plan in full; inspected relevant schema/functions
in the six code files above and the limited collector examples
[fetch_fcc_sitemaps.py](fetch_fcc_sitemaps.py) and
[fetch_fcc_details.py](fetch_fcc_details.py). their cached-hash/failure and
unsafe-general-retry descriptions match the inspected code.

checked every local Markdown target in the plan with the documented bundled Python;
all resolve. verified the documented bundled executable exists by running that check.
no network calls, collection, importer/rebuild commands, tests, SQL queries or live
database verification were run. no external product documentation was revalidated;
the plan treats those as dated recommendations requiring checks before adoption.

only this report was written. the coordinator owns plan/status changes and final
acceptance. next bounded action is milestone 0 after implementation is assigned.
