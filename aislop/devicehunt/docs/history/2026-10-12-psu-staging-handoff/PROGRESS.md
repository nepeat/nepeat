# current handoff

updated 2026-10-12. coordinator, sole integrator and documentation editor: `/root`
(Codex). independent workers: GPT-6.1 Sol. [GOAL](GOAL.md), [AGENTS](AGENTS.md),
[workflow](docs/agent-workflow.md), [evidence policy](docs/evidence-policy.md).

Active user request: continue search, discovery and open-ended work until 0000.
Interpretation: **2026-10-13 00:00 America/Los_Angeles** (07:00 UTC). Work in
bounded milestones until that cutoff; do not mark the overall goal complete just
because one wave finishes. Current assignments and results:
[2026-10-12 discovery](research/discovery-2026-10-12.md).

## accepted catalog — rechecked 2026-10-12

The independent [handoff audit](research/batches/discovery-2026-10-12/review/handoff-audit.md)
verified current counts, hashes, all manifest inputs, defaults and saved proofs.
No catalog/manifest import has occurred in the current discovery wave.

| item | verified state |
| --- | --- |
| authority | `data/devices.sqlite`, schema 29; sha256 `63d3ac788833473f08c716836b528531a4ff7f0df60f270035071fe36243081e` |
| identities / records / sources / facts | 5,112 / 7,230 / 2,076 / 74,129 |
| PDF documents / pages | 61 / 3,716 |
| owned baseline | 10/10; nine document snapshots + one reviewed 7760 label |
| ByteDance B4020/B4010 expectations | 0/2 qualifying affiliation hits; unresolved |
| manufacturer inventory | 2,147 target/identity pairs / 2,146 identities; Celestica 198 |
| all target associations | 4,591 pairs / 4,448 identities |
| hyperscaler assertions | 22 rows on 18 devices |
| accepted lineage | 146 distinct artifacts / 374 receipts / four reviewed statements |
| accepted manifest | `data/import-manifest.json`: 145 jobs / 3,910 hashed inputs; all hashes verified; sha256 `14c2a1f177051b6e62c8e04dabbd9621a3d4baf6b759d2db25a722ba8efc9195` |
| latest complete suite proof | [2026-10-12 guarded suite](research/batches/campaign-c2-2026-10-05/acceptance/discovery-2026-10-12-poppler-psu-suite-result.json): 769 passed, zero failures/errors/skips/blocked calls; all recorded production/test hashes unchanged |
| latest accepted rebuild | `data/rebuilt-sun6000-2026-10-11.sqlite`; saved [comparison](research/batches/discovery-2026-10-11/sun6000-a/integration/compare.log) proves all 52 tables/indexes and baseline match; logical equality, not byte equality |
| fresh consistency checks | quick_check `ok`; foreign_key_check empty; baseline and vendor examples rechecked; accepted live integrity was not rerun during the initial read-only audit; staged candidate checks and current suite are reported below |

## current wave

- [PSU research](research/batches/discovery-2026-10-12/psu/handoff.md): primary
  electrical documentation for three exact captured PSU strings; [review](research/batches/discovery-2026-10-12/review/psu-review.md)
  ACCEPT for discovery only. Wave 2 found installed Poppler 26.06.0 outside PATH:
  [runtime](research/batches/discovery-2026-10-12/pdf-runtime/handoff.md).
  [Three targeted visual checks](research/batches/discovery-2026-10-12/review/psu-visual-review.md)
  and [six additional fact pages](research/batches/discovery-2026-10-12/review/psu-fact-pages-review.md)
  are complete. [Poppler support review](research/batches/discovery-2026-10-12/review/poppler-support-review.md)
  ACCEPT with 63 guarded focused tests. [Revised extraction](research/batches/discovery-2026-10-12/psu-extraction-r2/report.json)
  preserves all 63 pages with explicit no-hyphen-removal metadata; page text
  is identical to the untouched first pass. Six sources / three identities /
  42 facts passed independent review and are [staged in an isolated catalog](research/batches/discovery-2026-10-12/integration/stage-report.json).
  Candidate: 5,115 identities / 7,236 records / 2,087 sources / 74,171 facts /
  66 documents / 3,779 pages; 146-job manifest with 3,928 verified inputs.
  All previous logical rows preserved, 10/10 baseline and campaign gate pass.
  Read-only lookup smoke returns 24 practical statements across the three PSUs.
  Clean rebuild is running; authoritative catalog remains the accepted old pair.
  Full guarded suite **769 passed**, zero failures/errors/skips/blocked calls,
  under label `discovery-2026-10-12-poppler-psu`; log in this batch's
  `integration/full-suite.log`. Recorded code/test hashes remain unchanged.
  Keep vendor ratings, low-line/temperature derating and revision conflicts scoped
  to the actual document. Do not infer parent-system fitment.
- [ByteDance search](research/batches/discovery-2026-10-12/bytedance/handoff.md):
  12 searches, no new qualifying primary evidence. Secondary code leads
  `BD3300004501`, `BD3300004502`, `8Q-TOR-T4-B4010` remain discovery-only.
  Known denied resources were not retried. Independent [review](research/batches/discovery-2026-10-12/review/bytedance-review.md)
  ACCEPT for discovery; no import proposed.
  The [four-query exact-code follow-up](research/batches/discovery-2026-10-12/bytedance-codes/handoff.md)
  also found only secondary previews; [review](research/batches/discovery-2026-10-12/review/bytedance-codes-review.md)
  ACCEPT. This search route is exhausted pending a new independent source lead.
- Listing matching F13: coordinator implemented narrow variant leads for spaced
  or comma-separated revision strings on `Unattributed accessory` FRU models.
  Bare `YM-2651Y` yields four live leads; `YM-2651Y-B01R` yields the two B01R
  spellings. No specifications attach to these leads; exact identities remain
  separate. 34 focused tests pass; [checks](research/batches/discovery-2026-10-12/listing-revisions/checks.json)
  and [live smoke](research/batches/discovery-2026-10-12/listing-revisions/live-smoke.json)
  show unchanged database hash. Independent [code review](research/batches/discovery-2026-10-12/review/listing-revisions-review.md)
  ACCEPT; reviewer also passed all 34 tests and checked record/alias scope.
- Documentation: coordinator replaced stale continuation prose, preserving the
  prior [handoff snapshot](docs/history/2026-10-12-before-midnight-discovery/PROGRESS.md)
  with [provenance](docs/history/2026-10-12-before-midnight-discovery/provenance.json).
  Snapshot links retain their original project-root-relative interpretation.
  Independent [documentation review](research/batches/discovery-2026-10-12/review/documentation-review.md)
  ACCEPT with finishing notes; coordinator applied both status/scope corrections.
  Subsequent acceptance-status updates are coordinator-checked.
- Campaign malformed-input issue reproduced in an isolated temporary workspace:
  removing required RRA `form` raises uncaught `KeyError`.
  [Probe result](research/batches/discovery-2026-10-12/campaign-input-probe/result.json).
  Adapter-boundary input-shape fix implemented: 39 affected tests pass and all
  12 healthy source outputs still match pins. Independent review found nested
  shape edge cases; both corrected and [re-review](research/batches/discovery-2026-10-12/review/campaign-input-shape-review.md)
  ACCEPT. Nine dedicated final shape tests pass; full-suite verification pending.

## accepted decisions and operating limits

- **SUN-6000 option A is complete**, Kladno-only: five new Celestica-brand
  identities, with certificate-scoped `registry_manufacturer` evidence; Monterrey
  remains unmapped literal context. Oracle-brand and Celestica-brand identities
  remain separate. The kind is the certificate's literal equipment name, not an
  independently established per-model classification.
  [Candidate review](research/batches/discovery-2026-10-11/review/sun6000-candidate-review.md).
- **Brand-family display table is complete**: seven independently reviewed rows
  for Quanta/QCT and Oracle/Sun. `lookup`, `match-listing` and `model-collisions`
  show grouping without changing identities, ranking or vendor inventory.
  [Review](research/batches/discovery-2026-10-11/review/brand-family-table-review.md).
  Manifest inclusion remains undecided; IEIT/Inspur mapping remains unresolved.
- **RRA cycle 2 is complete**: `2026-10-11a`, seven units `matches_pinned`,
  1,063 unit requests + smoke. Two consecutive no-change cycles. New cycles or a
  recurring schedule require new scope authorization; do not re-ask for cycle 2.
  [c4 notes](research/batches/hunt-2026-10-10/c4/c4-notes.md).
- **Cleanup is complete** for the 23 files / 8.83 GB authorized on 2026-10-11.
  [Ledger](research/batches/discovery-2026-10-11/integration/cleanup-2026-10-11.json)
  records hashes/reasons. All manifests were retained. Further deletions need an
  applicable authorization; no automatic cleanup follows from open-ended work.
- **Gated campaign integration is active**. The manifest selects release
  `campaign-release-1d45317e…` and coverage `coverage-b8ee5df9…`, with 12 accepted
  decisions. Current `DEFAULT_DEFINITION` / `DEFAULT_INVENTORY` point at
  `research/batches/discovery-2026-10-11/sun6000-a/inventory-r2/`.
  Changes to any of the 12 campaign sources require queue → independent decisions
  → release → manifest gate update; run `AcceptedManifestGateTests` afterward.
- Capture authority is `data/archive/`, bound to this workspace. The audit saw
  6,976 receipt JSON files and head sequence 6,602; these operational totals are
  distinct from the 374 accepted lineage receipts. No live collector is asserted
  running by this handoff. Inspect process/job handles before resuming work.

## next bounded steps

1. Complete the staged PSU batch: clean rebuild is still running in exec session
   `53044`, destination `data/rebuilt-psu-2026-10-12.sqlite`, using
   `data/import-manifest-candidate-psu-2026-10-12.json`. Full suite session `60425`
   finished successfully. Read [integration handoff](research/batches/discovery-2026-10-12/integration/handoff.md),
   confirm completion, compare explicitly against
   `data/devices-candidate-psu-2026-10-12.sqlite` using output prefix
   `psu-2026-10-12`, then obtain final independent integration acceptance from
   `/bytedance`. Promotion/export helpers are prepared but not executed.
   Do not rerun the failed stage helper or overwrite its retained outputs.
2. Wave 3 original-vendor discovery completed with nine actual HTTP requests
   (six Celestica/Edge-Core, three Oracle), within the combined cap of ten.
   [SD6200/Wedge review](research/batches/discovery-2026-10-12/review/psu-edge-wave3-review.md)
   and [Oracle 7420 review](research/batches/discovery-2026-10-12/review/oracle7420-wave3-review.md)
   ACCEPT discovery. `/handoff_audit` is finishing a two-source/two-record,
   27-fact candidate in `original-vendor-wave3/psu-edge-facts/`; existing
   `Edge-Core / Wedge 100-32X` retains its printed space and stays separate from
   joined `WEDGE100-32X`. Six ordering rows are literal facts, not aliases.
   `/psu` is preparing `original-vendor-wave3/oracle7420-facts/`: separate printed
   `Sun ZFS Storage 7420` and `Sun ZFS Storage 7420 M2` identities, with no
   equivalence to existing `ZFS STORAGE 7420` or `S7420` and no general-fact
   transfer to M2. Both candidates still need independent acceptance.
3. Fix the newly reproduced curated-import issue after current production code
   acceptance: [isolated probe](research/batches/discovery-2026-10-12/curated-input-probe/result.json)
   shows a malformed second source leaves the first source replacement committed
   after rollback. `Catalog.finish` also rebuilds all search text per source;
   one existing certification job invokes it 342 times. Plan complete input
   validation/transactional replacement and one final index rebuild, preserving
   exact successful output and source semantics. No fix has been implemented.
4. ByteDance exact-code route is exhausted pending a new independent source.
   Remaining bounded leads include the unrequested AS8000/SC6100 items in
   [next-lead plan](research/batches/discovery-2026-10-12/next-leads/handoff.md),
   244 older Inspur SPEC CPU2017 pages and buyer-query/repeated-fact presentation
   issues. SPEC's saved 10-second crawl delay applies. Wi-Fi Alliance API is
   robots-disallowed. Do not repeat prior denials or broaden a finished fetch set.

## runtime, verification and recovery

Use the project's direnv `python3` (3.14.7 verified), from this directory. No
package installs or system changes. This workspace is untracked in the parent
repository; preserve recovery files. The 2026-10-10 rename from
`fcc_shenanigans` is documented in [the rename note](research/rename-devicehunt-2026-10-10.md).
Use the README relocation workflow for a whole-workspace move.

Affected tests for the current change:
`python3 -m unittest tests.test_listing_accessory_revisions tests.test_listings tests.test_listing_specifications tests.test_listing_software_refs`.
Full offline acceptance uses
`research/batches/campaign-c2-2026-10-05/acceptance/run_offline_suite.py` with a
fresh `FCC_C1_ACCEPTANCE_LABEL`; normal suite:
`python3 -m unittest discover -s tests`. Run full acceptance for schema/importer
changes or combined data/code promotion, not to support claims beyond its scope.

Current pre-promotion recovery pair:
`data/devices-before-sun6000-2026-10-11.sqlite` and
`data/import-manifest-before-sun6000-2026-10-11.json`, with previous c3b defaults.
SUN-6000 candidate catalog/manifest are byte-identical to live but remain separate
files. `data/import-manifest-pinbase-sun6000-2026-10-11.json` is a superseded,
gate-less pin base, never an accepted manifest. Older recovery/proof locations
and historical milestone details remain in the archived handoff. Compressed
snapshot restoration uses [the history workflow](data/history/README.md).

Worldwide coverage, current support, marketplace stock, and the provenance or
configuration of individual auction units remain unproven.
