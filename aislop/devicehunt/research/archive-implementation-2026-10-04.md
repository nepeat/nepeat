# archive implementation and storage consolidation

user assignment: complete archive milestones 1–6 and simplify the many historical
databases. milestone 0 remains pinned. local implementation, verified archival
consolidation and catalog integration are authorized; no network acquisition,
publication or package installation is needed for this finite batch.

coordinator `/root` is sole integrator and documentation editor. owns catalog
schema, lineage/release modules, pipeline/rebuild/CLI/comparison routing, manifests,
accepted database, exports, this note and current documentation. other existing
batch staging is not promoted as part of this archive release.

| worker | exclusive paths | scope and stop |
| --- | --- | --- |
| archive_engine | devicehunt/artifacts.py, devicehunt/derivations.py, tests/test_artifacts.py, tests/test_derivations.py; batch engine.md | immutable objects, receipts, reconstructible registry, saved derivations and one offline recipe; tests and handoff |
| archive_capture | devicehunt/capture.py, tests/test_capture.py, research/fetch_fcc_sitemaps.py; batch capture.md | bounded collector adapter, shared reuse/policy/lease contract, mock tests; no live requests |
| storage_engine | devicehunt/storage.py, tests/test_storage.py; batch storage.md and storage-inventory.json | read-only inventory and lossless compressed chunk archive/restore tooling; no removal of real files |

workers use GPT-6.1 Sol, no further delegation. following the user's effort steer,
remaining reviews were restarted at explicit high effort as `archive_final_review`
(promotion-review.md) and `archive_docs_high` (documentation-review.md); their
predecessors were interrupted and those report paths transferred exclusively.
the durable effort preference is recorded in the workflow.
all existing evidence and shared
data stay read-only to workers. integration and archival consolidation occur only
after independent review and restore checks, under the coordinator. worker slots
rotate into independent review after engineering. each worker uses temporary test
directories, never live data for mutation.

finite scope: the pinned complete 47-document reference collection and its
reconstruction ancestors, a small existing reviewed fact set, the saved FCC
sitemap collection as the second upstream format, and the 124 existing SQLite
files measured at start. all history will remain byte-restorable; no unique
historical evidence is discarded. active catalog and current recovery checkpoint
remain directly usable.

acceptance: milestones 1–6 checks in the plan, full offline tests, candidate and
independent offline rebuild with duplicate-sensitive equality, original projection
preservation, baseline/foreign-key/integrity/FTS checks, frozen lineage release,
archive restoration, measured storage before/after and independent documentation
review. status: milestones 0–6 complete and promoted; storage consolidation
complete. independent high-effort documentation QA passed; this batch is closed.

## accepted results and proof

| milestone | delivered | acceptance |
| --- | --- | --- |
| 0 | complete finite inventory and authority boundary | [pilot](artifact-archive-pilot-2026-10-04.md) |
| 1 | immutable byte objects/receipts, legacy registration, restorable registry | [engine review](batches/archive-implementation-2026-10-04/engine-review.md), [registration](batches/archive-implementation-2026-10-04/reference-registration.json) |
| 2 | saved derivations, version/configuration-bound local recipes, independent consumers | [reuse measurements](batches/archive-implementation-2026-10-04/reuse-metrics.json) |
| 3 | shared capture/reuse/policy/lease contract and offline sitemap adapter | [capture review](batches/archive-implementation-2026-10-04/capture-review.md) |
| 4 | stable reviewed statement/revision mapping, selectors and offline evidence lookup | [lineage review](batches/archive-implementation-2026-10-04/lineage-review.md), [data review](batches/archive-implementation-2026-10-04/data-review.md) |
| 5 | frozen release, schema/importer routing, duplicate-sensitive rebuild equality, promotion/exports | [high-effort acceptance](batches/archive-implementation-2026-10-04/promotion-review.md), [post-promotion proof](batches/archive-implementation-2026-10-04/post-promotion-proof.json) |
| 6 | saved XML second source, measured reuse, lossless database-history simplification | [final storage measurement](batches/archive-implementation-2026-10-04/storage-final.json), [history index](../data/history/README.md) |

the runtime lacked optional PDF packages. the existing complete saved NTRA table
and text captures now reproduce the same target records without new extraction.
[investigation](batches/archive-implementation-2026-10-04/runtime-investigation.md)
and [independent replay review](batches/archive-implementation-2026-10-04/ntra-replay-review.md)
record the evidence; no package installation or catalog-as-source shortcut.

all earlier input pins remain unchanged. catalog rows are preserved except verified
workspace cache-path relocation. historical DB bytes are fully restorable, including
the two old recovery files retired only after the new live/recovery/rebuild were
available. [initial consolidation review](batches/archive-implementation-2026-10-04/consolidation-review.md),
[full archive reconstruction](batches/archive-implementation-2026-10-04/archive-restore-review.md)
and [supplemental restore proof](batches/archive-implementation-2026-10-04/supplemental-storage-proof.json).

limits: no new collection, OCR, general workflow server, hosted model or document UI.
only the four named existing Golduck facts have reviewed claim lineage in this
release. extraction remains candidate evidence; XML discovery creates no hardware
assertions. old SmartNIC/QCT staging was neither promoted nor discarded.

documentation editor `/root`; final [documentation review](batches/archive-implementation-2026-10-04/documentation-review.md)
is owned by `archive_docs_high` at high effort. future steps are in PROGRESS, not
these historical one-shot batch helpers.
