# wave4 and SPEC continuation — promoted

Root is the sole integrator and current-document editor. The authoritative
pair is `data/devices.sqlite` SHA `fc88dca7e8b6f23aa8078134be767c22fed902303c2880ee923ad1d55952e3bf`
and `data/import-manifest.json` SHA `43e9fdf88703d979b13eef475410d6f138be90833b1fe67de72bf99abe741562`.
Counts: 5,122 identities, 7,261 records, 74,602 facts, 2,115 sources, 70 PDFs
and 3,798 pages. The manifest contains 150 jobs and 3,970 pinned inputs;
the complete 59-entry reference bundle retains every previous 57 entry.

[Final independent acceptance](../review/wave4-integration-review.md) has SHA
`fe5982907b13c76f9279af616fecd742860dc2b71d65964e98280ffaa16d72df`.
All 50 ordinary tables preserve old rows and multiplicities; exactly 57 previous
reference-source hashes changed to the new complete-bundle hash. The fresh
rebuild matches all 52 logical tables and search indexes, including FTS rowids.
Owned baseline 10/10, integrity, foreign keys and campaign gates pass. All 209
code files match the accepted 791-test freeze; this data-only follow-up has its
own complete rebuild comparison. All 20 new raw records, 392 facts, roles and
identifiers were verified against accepted inputs.

`promotion-acceptance.json` binds the proof set. Promotion session 37927 and
export session 75988 finished successfully. The wave4 recovery pair preserves
the previous wave3 bytes; journal and result files bind old and new pairs.
Eight refreshed exports verified, with seven earlier outputs preserved.
Normal curated specification exports contain 1,304 rows. The dated six-model
export includes source facts with explicit origins, producing 11/11/3/3/3/0
rows. NF5488M5's zero means no selected practical fact names, not absent hardware
evidence. Document-search hits remain leads with provenance. Exports left the
database unchanged; `promoted-gate-check.json` passes.

Manufacturer inventory remains 2,147 pairs / 2,146 identities. All associations
contain 4,594 pairs / 4,451 identities. Hyperscaler assertions remain 22 rows on
18 devices; accepted lineage remains unchanged. ByteDance 0/2 is distinct from
the owned 10/10 baseline. `inventory-check.json` binds these observations.

All wave4 processes finished. The first comparator invocation had an invalid
output-prefix argument and stopped before database access; its log is retained
as `compare-prefix-error.log`. Corrected session 65231 passed. Helper review
fixed fresh backup/pending-path guards and the dated source-fact export; prior
bytes remain in `.before-review-fixes` files. Current documents were updated and
[completion review](../review/wave4-completion-doc-review.md) accepted. This
handoff's later readability-only edit preserves those results; its earlier
version remains in `handoff-before-readability.md`. Subsequent candidates are
not part of this accepted integration.
