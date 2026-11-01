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
| full suite | [769 passed](research/batches/campaign-c2-2026-10-05/acceptance/discovery-2026-10-12-poppler-psu-suite-result.json); zero failures/errors/skips/blocked network or process calls; accepted before the current importer edits; new-code suite now 791 passed; staged promotion review pending |
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

## active bounded work

1. Wave 3 facts are **staged, not promoted** in
   `data/devices-candidate-wave3-2026-10-12.sqlite`; candidate manifest is
   `data/import-manifest-candidate-wave3-2026-10-12.json` (148 jobs/3,941 inputs).
   [Stage handoff](research/batches/discovery-2026-10-12/integration-wave3/handoff.md),
   [proof](research/batches/discovery-2026-10-12/integration-wave3/stage-report.json).
   Candidate: 5,117 identities / 7,241 records / 74,210 facts / 2,093 sources;
   68 PDFs / 3,784 pages. Complete new reference bundle retains 55 old entries
   and adds two (57 total). Old rows, baseline and campaign gate pass; live pair
   above unchanged. [Independent stage acceptance](research/batches/discovery-2026-10-12/review/wave3-stage-review.md)
   is complete; promotion acceptance remains pending.
   - [SD6200/Wedge candidate](research/batches/discovery-2026-10-12/original-vendor-wave3/psu-edge-facts/handoff.md):
     27 facts on two existing exact identities;
     [accepted review](research/batches/discovery-2026-10-12/review/psu-edge-facts-review.md).
     Spaced `Wedge 100-32X` stays separate from joined `WEDGE100-32X`;
     six ordering rows stay literal facts, no new aliases/SKU classes.
   - [Oracle candidate](research/batches/discovery-2026-10-12/original-vendor-wave3/oracle7420-facts/handoff.md):
     twelve facts, two new printed identities `Sun ZFS Storage 7420` and
     `Sun ZFS Storage 7420 M2`;
     [accepted review](research/batches/discovery-2026-10-12/review/oracle7420-facts-review.md).
     No equivalence to `ZFS STORAGE 7420`/`S7420`, no general-fact transfer to M2.
2. Curated importer atomicity repair is **independently accepted; full suite passed**, owned
   by `/handoff_audit`: `devicehunt/importers.py`, `devicehunt/catalog.py`, new
   `tests/test_curated_import.py`, and batch `curated-import-fix/`.
   [Design](research/batches/discovery-2026-10-12/curated-import-design/design.md)
   profiles all 48 accepted curated inputs and six compatibility probes.
   [Independent design acceptance](research/batches/discovery-2026-10-12/review/curated-import-design-review.md)
   is complete; `/bytedance` remains independent code reviewer.
   [Implementation handoff](research/batches/discovery-2026-10-12/curated-import-fix/handoff.md):
   71 focused tests and final 16 importer tests pass; preparation passes all 48
   accepted and 50 staged curated jobs. Full input/snapshot preflight, one savepoint,
   per-source orphan/count cleanup,
   and one final FTS rebuild preserve successful data ordering/raw semantics.
   Deliberate transaction fix: nested success remains caller-owned/uncommitted;
   standalone success commits. No schema changes or shared catalog writes.
   Wave 3 candidate was built with old code before the implementation freeze
   lifted, so clean rebuild with repaired code must match it across all 52
   tables/indexes. [Independent code review](research/batches/discovery-2026-10-12/review/curated-import-fix-review.md)
   accepted with 71 tests and separate compatibility probes. Full guarded suite
   **43695 completed: 791 passed**, zero failures/errors/skips/blocked calls. Clean rebuild **82624** and comparison **84628**
   finished successfully: new `data/rebuilt-wave3-2026-10-12.sqlite` matches the
   old-code candidate across all 52 tables/indexes and baseline. Logs under
   `integration-wave3/`; proofs in `data/exports/wave3-2026-10-12-{comparison,validation}.json`.
   Frozen 209 files in `code-under-test.json` must stay unchanged. All proof jobs completed; do not restart them. `/psu` reviews helpers
   and complete proofs for final acceptance.
3. Wave 4 discovery completed: AS8000 R02 and SC6100 PDFs, four HTTP exchanges,
   fourteen fully extracted pages, six selected visual pages and 26 observations.
   [Handoff](research/batches/discovery-2026-10-12/original-vendor-wave4/as8000-sc6100/handoff.md),
   [independent discovery acceptance](research/batches/discovery-2026-10-12/review/as8000-sc6100-discovery-review.md).
   [Two-source candidate](research/batches/discovery-2026-10-12/original-vendor-wave4/as8000-sc6100-facts/handoff.md)
   passed [independent review](research/batches/discovery-2026-10-12/review/as8000-sc6100-facts-review.md):
   26 facts, existing Celestica/SC6100 and new exact Edge-Core/Minipack-AS8000.
   [Title-only r2](research/batches/discovery-2026-10-12/original-vendor-wave4/as8000-sc6100-facts-r2/handoff.md) is independently accepted and replaces r1 for integration.
   **Not imported.** Complete fourteen-page PDFs/sidecars stay pinned.
   AS8000 configured weight, conditioned input lines
   and service limits remain system-scoped; SC6100 per-node fans/optional BBU
   remain distinct. AS8000 page-12 extraction garbling stays intact and unused;
   pages 5–12 were not visually reviewed. Exact identity routing precedes facts.
4. `spec-continuation/` capture completed for the eighteen URLs
   in [offline plan](research/batches/discovery-2026-10-12/spec-next-plan/plan.json):
   six remaining older Inspur jbb HTML pages and twelve of the 244 remaining
   full CPU2017 system names. [Handoff](research/batches/discovery-2026-10-12/spec-continuation/handoff.md):
   nineteen HTTP exchanges including fresh robots, all 200, ten-second spacing,
   no searches/screenshots/retries; 354 raw evidence locators.
   [Candidate review](research/batches/discovery-2026-10-12/review/spec-continuation-facts-review.md) accepts 18 sources, 18 records and 366 facts; **not imported**.
   [Independent discovery review](research/batches/discovery-2026-10-12/review/spec-continuation-review.md)
   accepted; 232 CPU names remain. Review additionally checked eighteen jbb
   header rows (372 field/header records total). Historical scripts with
   scratchpad paths are not executable handoffs. `/psu` completed the accepted
   [F17 shopping design](research/batches/discovery-2026-10-12/shopping-query-design/design.md)
   in CLI helper/report template and focused tests, with originals/logs under
   `shopping-query-fix/`. Add literal model queries while retaining title/explicit
   identifiers; suppress hardware links for existing software-reference classes.
   [Independent code review](research/batches/discovery-2026-10-12/review/shopping-query-fix-review.md)
   accepted: 15 Python and seven JavaScript tests pass; eight saved proposals equal
   implementation. Current HTML export is still old pending normal refresh.
   Both code scopes are frozen under the running combined acceptance. No shared export write by workers. Repeated-fact presentation
   remains a later task.
   The finite five-source original-vendor lead plan is captured; do not restart
   it. Wi-Fi Alliance API remains robots-disallowed. Prior denials remain
   authoritative; no alternate access or automatic expanding crawl.

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

Wave 5: `/handoff_audit` owns finite discovery in `original-vendor-wave5/` for three saved-href sources in `next-leads-r2/plan.json`. Nine HTTP exchanges completed; six PDF pages rendered. Source inconsistencies are being preserved in the handoff; no candidate or import.
