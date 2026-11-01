# lookup batch optimization handoff

outcome: complete, frozen for independent review. no network, installs, imports,
shared export refreshes, database changes or manifest changes performed.

one bounded improvement: `lookup.run_many(db, [(query, limit), ...])` streams each
registry once in its existing SQL order, decodes cached cells/casefolds searchable
text once per row, and matches all requested literal strings. only matching rows
within requested limits are retained; unlimited requests retain their requested
outputs. catalog SQL/evidence/FTS membership stays unchanged. source counts and
metadata are loaded once per registry per call. no persistent cache or schema change.

all inputs validate before SQL. all results share one read snapshot. caller-owned
transactions survive success and failure; an internally owned snapshot rolls back
on exit. independent calls read current rows again. single-query public `run` is
unchanged. refresh now batches eligible dictionary reports, guarding legacy JSON
arrays/scalars with `isinstance(previous, dict)`; normal export defaults remain.

## measurements and checks

- before: 176.171200208 seconds, 98 calls to preserved original `run`.
- after: 13.811420083 seconds, same 98 query/limit pairs in one batch.
- improvement: 12.75547x faster, about 92.16 percent less lookup time.
- 98 results equal both original implementation results and saved current exports,
  including row ordering, all metadata, every dataset, query and limits.
- peak process RSS: 177,684,480 bytes (169.45 MiB), including both complete old/new
  result lists retained for comparison. registry input is streamed, never retained
  as a full-row snapshot. memory scales with requests and requested output.
- cProfile: one old query invokes `contains` 959,464 times, `cells_text` 118,397
  times and `str.casefold` 2,032,207 times. the repeated scan/decode cost is removed
  across registry queries; catalog scans remain per request.
- affected suite: 14 tests pass (0.777 seconds).
- full offline suite: 433 tests pass (22.650 seconds).
- live database SHA remains
  `a098c390431efd77d25f162acb039d2ddd938ff5effeffd7e93fa9b0bc34ff16`.

new fixture regressions compare batch against ordinary run for all datasets,
Unicode casefold, literal punctuation/whitespace, limits 0/1/20/-1, duplicate and
NULL raw cells, SQL ties across source IDs, missing tables versus loaded zero,
stale/duplicate/missing FTS identities, no cache across changed calls, validation
before SQL and transaction ownership on success/error. existing raw date/role
fields remain included in complete equality comparisons.

## artifact paths

all paths below are relative to this engineering directory:

- `before/lookup.py`, `before/refresh_exports.py`: exact preserved code.
- `code-hashes.json`: before/final code/test SHA-256 values.
- `lookup.py.diff`, `refresh_exports.py.diff`: reviewable unified changes.
- `benchmark.py`, `benchmark.log`, `benchmark.json`: old/new timing and equality.
- `profile.log`: before bottleneck evidence.
- `golden-manifest.json`: all 98 names, queries, limits and original export hashes.
- `affected-tests.log`, `full-tests.log`: actual terminal evidence.
- `isolatedreports/`, `isolated-proof.log`, `equivalence.json`: proposed serialized
  reports and per-report SHA/byte-equality proof, generated into this private path.

written application paths: `devicehunt/lookup.py`, `scripts/refresh_exports.py`,
`tests/test_lookup_batch.py`. existing `tests/test_lookup.py` remains unchanged.

next bounded step: independent code review; accept before the designated integrator
uses this code for a shared export refresh. timings measure lookup computation,
not the entire export script. order among exact SQL ordering ties is checked for
this frozen DB's full 98 reports and fixture cases; no additional SQL tie-breaker
was introduced.
