# current handoff

updated 2026-10-12 PDT. coordinator, sole integrator and documentation editor:
`/root` (Codex). independent workers: GPT-6.1 Sol.
[GOAL](GOAL.md), [AGENTS](AGENTS.md), [workflow](docs/agent-workflow.md),
[evidence policy](docs/evidence-policy.md).

Active request: continue search, discovery and open-ended work until
**2026-10-13 00:00 America/Los_Angeles (07:00 UTC)**. Continue bounded
milestones until that cutoff. Do not mark the overall goal complete early.
Assignments and detailed evidence: [discovery milestone](research/discovery-2026-10-12.md).

## accepted catalog

The PSU batch is promoted. [Independent acceptance](research/batches/discovery-2026-10-12/review/psu-integration-review.md),
[promotion](research/batches/discovery-2026-10-12/integration/promotion-result.json),
[export proof](research/batches/discovery-2026-10-12/integration/exports-report.json).
[Completion documentation audit](research/batches/discovery-2026-10-12/review/psu-completion-doc-review.md)
accepted after the historical specification-audit wording was corrected.

| item | verified state |
| --- | --- |
| authority | `data/devices.sqlite`, schema 29; SHA-256 `53a9a0eabaef5656b38ad922f96d963b76839eeda2da3b28a18b22359d857dee` |
| identities / records / sources / facts | 5,115 / 7,236 / 2,087 / 74,171 |
| PDF documents / pages | 66 / 3,779; complete reference bundle has 55 entries |
| owned baseline | 10/10; nine document snapshots + one reviewed 7760 label |
| ByteDance B4020/B4010 expectations | 0/2 qualifying affiliation hits; unresolved |
| manufacturer inventory | 2,147 target/identity pairs / 2,146 identities; Celestica 198 |
| all target associations | 4,591 pairs / 4,448 identities |
| hyperscaler assertions | 22 rows on 18 devices |
| accepted lineage | 146 distinct artifacts / 374 receipts / four reviewed statements; unchanged |
| manifest | `data/import-manifest.json`: 146 jobs / 3,928 verified inputs; SHA-256 `6ab359e22929bdb6a55e47c2a816c2c78fc3e5d0f90c8b252129f8fafea3aac5` |
| full suite | [769 passed](research/batches/campaign-c2-2026-10-05/acceptance/discovery-2026-10-12-poppler-psu-suite-result.json); zero failures/errors/skips/blocked network or process calls; all 111 recorded code/test hashes unchanged |
| clean rebuild | `data/rebuilt-psu-2026-10-12.sqlite`; [all 52 tables/indexes and baseline match](research/batches/discovery-2026-10-12/integration/compare.log), including multiplicities and both search indexes |
| preservation | all previous logical rows preserved; only the expected 50 old reference-source bundle hashes changed; integrity `ok`, no foreign-key violations |
| refreshed exports | catalog HTML, target CSV/JSON, 1,261 specification rows, reference-document CSV, owned validation, and dated PSU specification/document results; database hash unchanged |

Current complete PDF collection:
`data/raw/reference-documents-with-psu-2026-10-12.json`. Keep all 55 entries and
sibling PDF/text dependencies in any later replacement. The superseded bundle
and every original capture remain available.

## completed in this wave

- Three exact PSU identities and 42 source-scoped facts accepted:
  `Bel Fuse Inc. / PFE1100-12-054NA`,
  `Bel Fuse Inc. / PFE1500-12-054NAC`, `FSP / FSP550-29FM`.
  Six source records and five complete PDFs added. No target inventory expansion
  or chassis fitment inference. [Fact review](research/batches/discovery-2026-10-12/review/psu-facts-review.md)
  preserves rated/operating voltage differences, regional/temperature derating,
  separate manufacturer/factory roles and the FSP page-2 model conflict.
- Honest Poppler extraction/import/archive-method support accepted; legacy pypdf
  captures and receipt identities remain compatible. [Code review](research/batches/discovery-2026-10-12/review/poppler-support-review.md),
  [usage](docs/pdf-extraction.md). Revised extraction preserves all 63 pages with
  explicit no-hyphen-removal parameters; page text equals the untouched first
  pass. Nine fact-bearing pages were visually reviewed, not all 63 pages.
- Listing F13 variant leads accepted for separated revision strings on
  `Unattributed accessory` FRU models. Bare `YM-2651Y` yields four leads;
  `YM-2651Y-B01R` yields two spellings. No specifications attach to variant
  leads and identities remain separate. [Review](research/batches/discovery-2026-10-12/review/listing-revisions-review.md).
- Campaign input-shape validation accepted after two independent-review fixes.
  All 12 healthy outputs still match pins. [Review](research/batches/discovery-2026-10-12/review/campaign-input-shape-review.md).
  Ten final shape/gate tests and the full suite pass; the promoted manifest's
  accepted gate test also passes.
- ByteDance discovery and four-query exact-code follow-up found no qualifying
  primary evidence. `BD3300004501`, `BD3300004502`, `8Q-TOR-T4-B4010` remain
  secondary leads only. [Final route review](research/batches/discovery-2026-10-12/review/bytedance-codes-review.md).
  Do not repeat synonymous searches without a new independent source.

## next bounded work

1. Wave 3 captures and both fact candidates have passed independent review;
   **neither candidate is imported**. The two PDF sources used six requests and
   Oracle used three, within the combined cap of ten; no searches or remote
   screenshots. Exact request inputs, redirects and policies are recorded.
   - [SD6200/Wedge candidate](research/batches/discovery-2026-10-12/original-vendor-wave3/psu-edge-facts/handoff.md):
     two sources / two existing identities / 27 facts;
     [review](research/batches/discovery-2026-10-12/review/psu-edge-facts-review.md).
     Printed `Edge-Core / Wedge 100-32X` stays separate from joined
     `WEDGE100-32X`. Six ordering rows are literal facts, not aliases or new
     SKU classes. Generic Accton manufacturing prose remains context only.
   - [Oracle candidate](research/batches/discovery-2026-10-12/original-vendor-wave3/oracle7420-facts/handoff.md):
     two sources / three records / twelve facts on separate printed
     `Sun ZFS Storage 7420` and `Sun ZFS Storage 7420 M2` identities;
     [review](research/batches/discovery-2026-10-12/review/oracle7420-facts-review.md).
     No equivalence to `ZFS STORAGE 7420` or `S7420`; no general-fact transfer
     to M2. Candidate baseline hashes are explicitly historical observations.
2. Curated importer repair is the next engineering task. [Isolated probe](research/batches/discovery-2026-10-12/curated-input-probe/result.json)
   proves that a malformed second source leaves the first replacement committed
   after caller rollback. `Catalog.finish` also rebuilds all search text once per
   source, including 342 times for one existing certification job.
   `/handoff_audit` owns **design only** in `curated-import-design/`; no production
   fix is implemented or authorized by that assignment. Preserve successful
   legacy output, raw values and source ordering. In particular, deferring orphan
   cleanup can change identity title/kind when a model moves between sources;
   investigate keeping cheap cleanup per source while deferring FTS/commit.
   Review the design, assign bounded implementation/tests and independent review,
   then run required full acceptance and clean rebuild before a combined batch.
3. Integrate accepted wave 3 inputs through a new complete PDF bundle and manifest,
   isolated candidate, old-data preservation, baseline/gate checks, clean rebuild
   and independent acceptance. `/root` remains the sole integrator. Do not reuse
   existing output names or rerun completed staging/promotion helpers.
4. Other bounded leads: unused AS8000/SC6100 entries in the
   [next-lead plan](research/batches/discovery-2026-10-12/next-leads/handoff.md),
   244 older Inspur SPEC CPU2017 pages, and buyer-query/repeated-fact presentation.
   SPEC's saved 10-second crawl delay applies; Wi-Fi Alliance API is
   robots-disallowed. Prior denials remain authoritative; no alternate route to
   a denied item. A finished capture set does not authorize an expanding crawl.

## decisions and operating limits

SUN-6000 option A is complete, Kladno-only: five Celestica-brand identities with
certificate-scoped registry-manufacturer evidence; Monterrey remains unmapped
literal context. Oracle/Celestica identities stay separate. The seven-row
Quanta/QCT and Oracle/Sun display-family table is complete; it does not merge
identities or invent aliases. Manifest inclusion and IEIT/Inspur mapping remain
unresolved. RRA cycle 2 is complete with two consecutive no-change cycles;
new cycles/schedules require new scope authorization. Cleanup of the authorized
23 files / 8.83 GB is complete; no further automatic deletions.

Campaign gate remains release `campaign-release-1d45317e…`, coverage
`coverage-b8ee5df9…`, 12 accepted decisions. Current definition/inventory defaults
remain `research/batches/discovery-2026-10-11/sun6000-a/inventory-r2/`.
Changes to campaign sources require queue → independent decisions → release →
manifest gate update. Archive authority is `data/archive/`; initial audit saw
6,976 receipts/head 6,602, distinct from accepted lineage totals. This wave did
not write the live archive. No collector, suite or rebuild process is left
running by the completed PSU batch; check handles before resuming other work.

## runtime and recovery

Use the project direnv `python3` (3.14.7 verified), no installations or system
changes. Workspace is untracked in its parent repository: preserve recovery
files. [Rename note](research/rename-devicehunt-2026-10-10.md),
[whole-workspace relocation](README.md), [compressed history](data/history/README.md).

Current recovery pair: `data/devices-before-psu-2026-10-12.sqlite` and
`data/import-manifest-before-psu-2026-10-12.json`. Promoted candidate files remain
separate and byte-identical to live. Overwritten exports were preserved with
hash provenance in `research/batches/discovery-2026-10-12/integration/exports-before/`.
The initial stage helper's URI-attachment failure and read-only finalizer are
recorded in [integration history](research/batches/discovery-2026-10-12/integration/handoff.md).
The [superseded staging handoff](docs/history/2026-10-12-psu-staging-handoff/PROGRESS.md)
and earlier [pre-discovery handoff](docs/history/2026-10-12-before-midnight-discovery/PROGRESS.md)
retain historical details; their relative links use the original project root.

Full acceptance runner:
`research/batches/campaign-c2-2026-10-05/acceptance/run_offline_suite.py` with a
fresh `FCC_C1_ACCEPTANCE_LABEL`. Rebuild into a NEW path, and compare with
`scripts/compare_rebuild.py` explicitly selecting the candidate database.
Read AGENTS/workflow before integration. Worldwide coverage, current support,
marketplace stock and individual auction-unit configuration remain unproven.
