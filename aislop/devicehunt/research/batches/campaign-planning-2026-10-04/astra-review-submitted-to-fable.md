# astra review — campaign design

Reviewer: coordinating `/root`, fulfilling the user's requested Astra/self-review.
This is a separate review of Fable's proposal and the implementation evidence,
not a claim that a second independent GPT service was invoked. Planning only.

## independent findings before reading fable's proposal

1. **start with durable state and acquisition correctness, not a scheduler daemon.**
   `devicehunt/capture.py` holds budget in `Collector.remaining`. A restart or
   another collector gets another budget. A campaign reservation must be durable
   before HTTP starts and uncertain interrupted attempts must not be refunded
   automatically. Exactly-once HTTP cannot be promised after a crash.
2. **optimize the index without losing receipt authority.** Writable `Archive`
   construction performs `reindex()` and an audit. `receipts(kind)` reads every
   receipt file; capture repeatedly invokes it. Removing these scans requires
   an explicit publication/recovery protocol so a committed denial cannot be
   invisible to the next request. Benchmark finite synthetic input and test
   crash boundaries; do not substitute a faster but stale cache.
3. **do not call an FCC mirror a first-party regulator feed.** The pilot collector
   reads saved `fccid.co` sitemaps. It is useful discovery coverage of that mirror,
   not complete FCC application coverage or independently corroborated evidence.
4. **ISED is the right heterogeneous test, with a prerequisite.** Its existing
   collector has in-memory cookies, POST form state, refresh steps and export
   links. A URL/body hash alone does not make a state-changing workflow step
   eligible for cross-session reuse. Preserve responses, but distinguish replay
   from execution. A new session after interruption may require budgeted replay
   of setup steps; never reuse old execution tokens as current session state.
   Session cookies must remain in memory and out of new receipt header fields.
5. **source-local row ordinals are not stable claim identities across refreshes.**
   Existing ISED records use positional source keys. Keep every raw row and
   duplicate occurrence, and separate snapshot row IDs, upstream IDs (where
   present), candidate entity links and durable claim IDs. Reordering must not
   attach yesterday's reviewed statement to another product.
6. **incremental discovery is not permission to partially replace a source.**
   Existing importers expect complete validated source snapshots. Build complete
   candidate bundles, or add and test explicit delta semantics later. A missing
   row in an incomplete observation is not a deletion or retraction.
7. **release selection needs engineering.** `devicehunt/archive.py:freeze` takes
   all known archive artifacts/receipts. Growing acquisition must not force all
   pending or unrelated broken objects into an accepted release. Select explicit
   roots, validate the complete transitive closure, freeze it, and preserve prior
   accepted releases. A hash-shaped string is not automatically an artifact ref.
8. **trade feasibility is a branch, not a promised adapter.** Saved notes already
   distinguish snippets from bills of lading and record denied trade pages.
   Require a qualifying free/public source and mapped fields before implementing
   shipment entities. Aggregate statistics cannot establish an exact model,
   importer/exporter, manufacturer or datacenter deployment.
9. **keep usefulness measurable.** A campaign must expose exact-identifier leads,
   practical-document gaps, candidate review status and coverage gaps alongside
   request/byte counts. Synthetic changes demonstrate mechanics; they do not
   count as new hardware evidence. A valid no-change run is successful.
10. **storage discipline must be designed in.** Use one operational state database
    plus the reconstructible archive index and accepted catalog; do not create a
    database per source or run. Register saved bytes in place, avoid a full
    database for each collection tick, and create candidate/rebuild copies only
    for a named integration. Preserve history with the verified existing tools.

Evidence inspected: `devicehunt/{artifacts,capture,archive,derivations,lineage,
pipeline,ised}.py`, `research/{fetch_fcc_sitemaps,capture_ised}.py`,
`tests/{test_capture,test_ised_capture,test_ised_paged}.py`, current project docs,
`research/ised-radio-equipment.md` and `research/bytedance-inspur-2026-10-04.md`.
Historical source notes describe past observations; their current availability
has not been reverified and their old catalog counts do not override PROGRESS.

## cross-review disposition

Fable's first consultation completed with actual CLI model `claude-fable-5-1`.
The coordinator then inspected `research/capture_rra_query.py`, `devicehunt/rra.py`,
the RRA source notes and current import manifest to check its recommendations.

- **accepted:** RRA + ISED replaces my initial FCC-mirror + ISED pilot. RRA is
  a better official, date-bounded source with an existing stability validator.
  The mirror remains useful regression/discovery material, explicitly labelled.
- **accepted:** harden capture/indexing before a runner; no daemon, mass lineage
  migration, automatic entity matching or assumed trade source. Make unresolved
  attempts visible, and keep ISED live work conditional on session support.
- **revised:** use one transactional operational store by evolving the existing
  lease store. Fable proposed counting attempt receipts after host lease capture;
  this alone permits two different hosts to exceed a shared campaign ceiling.
  Append-only receipts remain evidence, with transactional reservation state for
  admission and a tested crash/recovery boundary.
- **rejected shortcut:** receipt-file count equals indexed-row count does not
  establish consistency, and computing the count is still a scan. Require an
  explicit publication/recovery protocol, single writer initially, and indexed
  point lookups. Receipt sharding is deferred until measured need; ID prefixes
  such as `capture-` are not an even distribution and legacy paths must survive.
- **revised:** release roots must support selected source-unit/coverage receipts,
  including zero/unavailable outcomes, not only statements. Otherwise a useful
  no-change/no-claim campaign cannot have a complete frozen coverage record.
- **corrected counts:** current manifest contains seven exact `rra` importer jobs
  and five `ised` jobs. Fable's nine RRA/ten ISED phrase is not used as an input
  contract. c0 must derive its finite set from the actual pinned manifest.
- **corrected stability:** RRA supports byte-identical exports OR explicit
  `exact_rows_ignore_order` comparison of certificate-keyed cells. The after CSV
  must be a distinct observation when live, not a cache hit on the before CSV.
  The old curl helper follows redirects and bypasses shared capture policy;
  the plan requires a transport adaptation, not running that helper unchanged.
- **qualified incremental dates:** query windows are not a modification feed.
  Non-overlapping new-date windows alone miss amended or late-published older
  approvals. Add separately scoped revalidation and keep cross-spelling duplicate
  certificate observations distinct; never call them independent corroboration.
- **accepted with ordering fix:** current lineage validation requires the fact
  already projected into an isolated candidate. Candidate import precedes lineage
  validation, but live acceptance waits for review and clean rebuild. Existing
  importers must not bypass candidate decisions through automatic whole-source
  projection; accept a complete reviewed projection or implement tested filtering.
- **coverage correction:** terminal job accounting includes blocked/unavailable;
  successful observation coverage must be a different measure. Empty stable
  results are valid observations with their explicit source proof.

The synthesized `docs/campaign-plan.md` incorporates these dispositions and is
being returned to Fable for challenge before final acceptance.
