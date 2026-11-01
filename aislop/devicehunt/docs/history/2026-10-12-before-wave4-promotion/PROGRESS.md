# current handoff

updated 2026-10-12 PDT. `/root` is coordinator, sole integrator and documentation
editor; independent workers use GPT-6.1 Sol. [GOAL](GOAL.md), [AGENTS](AGENTS.md),
[workflow](docs/agent-workflow.md), [evidence policy](docs/evidence-policy.md).

Active request: continue search, discovery and open-ended work until
**2026-10-13 00:00 America/Los_Angeles (07:00 UTC)**. Continue finite milestones;
do not mark the overall goal complete early. Detailed ownership and evidence:
[discovery milestone](research/discovery-2026-10-12.md).

## accepted catalog

Wave 3 is promoted, including the curated importer atomicity repair and literal
model shopping queries. [Final independent acceptance](research/batches/discovery-2026-10-12/review/wave3-integration-review.md),
[promotion](research/batches/discovery-2026-10-12/integration-wave3/promotion-result.json),
[export proof](research/batches/discovery-2026-10-12/integration-wave3/exports-report.json),
[promoted gate check](research/batches/discovery-2026-10-12/integration-wave3/promoted-gate-check.json).
[Completion documentation review](research/batches/discovery-2026-10-12/review/wave3-completion-doc-review.md)
accepted; root owns current documents.

| item | verified state |
| --- | --- |
| authority | `data/devices.sqlite`, schema 29; SHA-256 `4ae2cd0d6a614d582cf6fc47e6ba0e022521ac2b5e73bb268fcfa0dcb87c03ea` |
| identities / records / sources / facts | 5,117 / 7,241 / 2,093 / 74,210 |
| PDF documents / pages | 68 / 3,784; complete reference bundle has 57 entries |
| owned baseline | 10/10; nine document snapshots + one reviewed 7760 label |
| ByteDance B4020/B4010 expectations | 0/2 qualifying affiliation hits; unresolved |
| manufacturer inventory | 2,147 target/identity pairs / 2,146 identities; Celestica 198 |
| all target associations | 4,593 pairs / 4,450 identities |
| hyperscaler assertions | 22 rows on 18 devices |
| accepted lineage | 146 distinct artifacts / 374 receipts / four reviewed statements; unchanged |
| manifest | `data/import-manifest.json`: 148 jobs / 3,941 verified inputs; SHA-256 `9b4d93229e1519770a79f665e02987e9706e74cf86db086599160b905157ffec` |
| full suite | [791 passed](research/batches/campaign-c2-2026-10-05/acceptance/discovery-2026-10-12-curated-wave3-suite-result.json), 783.32 seconds; zero failures/errors/skips/blocked network or process calls |
| clean rebuild | `data/rebuilt-wave3-2026-10-12.sqlite`; [all 52 tables/indexes and baseline match](data/exports/wave3-2026-10-12-comparison.json), including multiplicities and both FTS rowids; new code equals the held old-code candidate |
| preservation | all old logical rows preserved; exactly 55 old reference-source bundle hashes changed; integrity `ok`, no FK violations |
| exports | catalog HTML, target CSV/JSON, 1,285 specification rows, reference-document CSV, owned validation and dated four-model wave3 export refreshed; DB hash unchanged |

Current complete PDF bundle: `data/raw/reference-documents-with-wave3-2026-10-12.json`.
Retain all 57 entries and sibling dependencies in later replacements.

## completed changes

- Wave3 adds 39 source-scoped facts across SD6200, spaced `Wedge 100-32X`,
  `Sun ZFS Storage 7420` and `Sun ZFS Storage 7420 M2`, with two complete PDFs.
  Joined WEDGE spelling and registry ZFS/S7420 identities remain separate;
  ordering rows stay literal facts, and M2 receives only its explicit statement.
  [Integration handoff](research/batches/discovery-2026-10-12/integration-wave3/handoff.md).
- Curated imports preflight every consumed input/snapshot before changes, use one
  savepoint and final FTS rebuild, preserve successful output ordering, and keep
  nested successful imports within the caller's transaction. Standalone imports
  commit. [Code review](research/batches/discovery-2026-10-12/review/curated-import-fix-review.md).
  No schema change; 71 focused checks plus the full suite passed.
- Shopping links add the literal model alongside existing title/explicit IDs;
  existing software-reference classes have no hardware links. Refreshed HTML
  includes this behavior. [Review](research/batches/discovery-2026-10-12/review/shopping-query-fix-review.md):
  15 Python and seven JavaScript tests passed; eight saved proposals matched.
- Earlier PSU/Poppler, listing revision leads and campaign input-shape work remain
  accepted. [PSU checkpoint](research/batches/discovery-2026-10-12/integration/handoff.md)
  and [prior current handoff](docs/history/2026-10-12-before-wave3-promotion/PROGRESS.md)
  retain details. ByteDance exact-code searches found no qualifying primary evidence;
  do not repeat synonymous routes without a new source.

## active bounded work

1. Wave4 r2 and SPEC continuation are **staged, not promoted** in
   `data/devices-candidate-wave4-2026-10-12.sqlite` with candidate manifest
   `data/import-manifest-candidate-wave4-2026-10-12.json`: 5,122 identities /
   7,261 records / 74,602 facts / 2,115 sources / 70 PDFs / 3,798 pages;
   150 jobs / 3,970 pins, full 59-entry bundle. [Stage proof](research/batches/discovery-2026-10-12/integration-wave4/stage-report.json).
   Old rows, baseline10/10 and gates pass. Independent actual-stage review by
   `/psu` pending; new clean rebuild **56138** and query checker **28667** running.
   Root combines these accepted inputs (392 facts):
   - [AS8000/SC6100 r2](research/batches/discovery-2026-10-12/original-vendor-wave4/as8000-sc6100-facts-r2/handoff.md):
     two sources/records, 26 facts, two complete PDFs/14 pages. Existing SC6100,
     new exact Edge-Core/Minipack-AS8000; system configuration/voltage exceptions,
     node fans and optional BBU remain scoped. [Fact review](research/batches/discovery-2026-10-12/review/as8000-sc6100-facts-review.md),
     [title-only r2 review](research/batches/discovery-2026-10-12/review/as8000-sc6100-facts-r2-review.md).
     Six fact pages visually checked; AS8000 pages 5–12 were not. Unused page12
     garbling remains intact.
   - [SPEC candidate](research/batches/discovery-2026-10-12/spec-continuation-facts/handoff.md):
     18 sources/records, 366 facts, 12 identities (eight existing/four new).
     [Independent acceptance](research/batches/discovery-2026-10-12/review/spec-continuation-facts-review.md).
     Raw case/full CPU names remain; established casefold routing retains existing
     lowercase I24 displays. Jbb PSU values describe submitted configurations;
     none of the 12 CPU captures has selected power-hardware facts. Brand role
     does not establish manufacture/deployment. 232 CPU names remain outside scope.
2. `/handoff_audit` owns `original-vendor-wave5/` discovery for three explicit
   saved-href sources: Oracle7420 specifications, AS6700/6701 QSG, AS5710/5712 R09.
   Nine HTTP exchanges, six complete PDF pages and seven local renders captured;
   [independent discovery review](research/batches/discovery-2026-10-12/review/original-vendor-wave5-review.md) accepted.
   `/bytedance` is recommending exact ordering-row identity boundaries. Contradictory
   Oracle weight/power text and ordering-code boundaries must stay explicit.
   No candidate/import. No retries, alternate access or automatic expansion.
3. Wave3 completion documentation review accepted. Root completes the staged
   wave4/SPEC clean rebuild and comparison before independent promotion review. All 209 accepted production/test files remain
   frozen; no code changes are assigned. No proof process remains running from
   wave3 (suite43695/rebuild82624/comparison84628/promotion45892/export24026 done).

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

Use project direnv `python3` (3.14.7), no installs/system changes. Workspace is
untracked in its parent repository; retain recovery files. [Rename note](research/rename-devicehunt-2026-10-10.md),
[relocation](README.md), [compressed history](data/history/README.md).

Latest recovery pair: `data/devices-before-wave3-2026-10-12.sqlite` and
`data/import-manifest-before-wave3-2026-10-12.json`. Prior PSU backups remain.
Promoted candidate pair remains separate, byte-identical to live. Seven replaced
exports and hashes are in `research/batches/discovery-2026-10-12/integration-wave3/exports-before/`.
All earlier captures and dated exports remain unchanged.

Full acceptance runner: `research/batches/campaign-c2-2026-10-05/acceptance/run_offline_suite.py`
with fresh `FCC_C1_ACCEPTANCE_LABEL`. Rebuild to a NEW path and compare with
`scripts/compare_rebuild.py` explicitly selecting the candidate database.
Worldwide coverage, current support, marketplace stock and auction-unit
configuration remain unproven.
