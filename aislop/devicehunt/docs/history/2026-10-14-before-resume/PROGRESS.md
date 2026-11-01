# current handoff

updated 2026-10-12 PDT. `/root` is coordinator, sole integrator and documentation
editor; independent workers use GPT-6.1 Sol. [GOAL](GOAL.md), [AGENTS](AGENTS.md),
[workflow](docs/agent-workflow.md), [evidence policy](docs/evidence-policy.md).

**Stopped at the user's request on 2026-10-12 PDT:** “Spin down gracefully now.
Quota reset is done.” This supersedes the earlier midnight work cutoff. The goal
is paused; do not restart collection, tests, rebuilds or integration without a
new user request. Current production code and the accepted live catalog remain
unchanged. Detailed ownership/history: [discovery milestone](research/discovery-2026-10-12.md).

[Independent shutdown documentation review](research/batches/discovery-2026-10-12/review/spin-down-doc-review.md)
accepted: 35 local links resolve, staged and private work remain clearly marked,
and no project command processes were observed running. All workers are stopped.

## accepted catalog

Wave 4 and SPEC continuation are promoted. Previously accepted importer
atomicity and shopping-query code remains unchanged. [Final independent acceptance](research/batches/discovery-2026-10-12/review/wave4-integration-review.md),
[promotion](research/batches/discovery-2026-10-12/integration-wave4/promotion-result.json),
[export proof](research/batches/discovery-2026-10-12/integration-wave4/exports-report.json),
[promoted gate check](research/batches/discovery-2026-10-12/integration-wave4/promoted-gate-check.json).
[Wave4 completion review](research/batches/discovery-2026-10-12/review/wave4-completion-doc-review.md)
accepted; prior wave3 audit remains accepted history.

| item | verified state |
| --- | --- |
| authority | `data/devices.sqlite`, schema 29; SHA-256 `fc88dca7e8b6f23aa8078134be767c22fed902303c2880ee923ad1d55952e3bf` |
| identities / records / sources / facts | 5,122 / 7,261 / 2,115 / 74,602 |
| PDF documents / pages | 70 / 3,798; complete reference bundle has 59 entries |
| owned baseline | 10/10; nine document snapshots + one reviewed 7760 label |
| ByteDance B4020/B4010 expectations | 0/2 qualifying affiliation hits; unresolved |
| manufacturer inventory | 2,147 target/identity pairs / 2,146 identities; Celestica 198 |
| all target associations | 4,594 pairs / 4,451 identities |
| hyperscaler assertions | 22 rows on 18 devices |
| accepted lineage | 146 distinct artifacts / 374 receipts / four reviewed statements; unchanged |
| manifest | `data/import-manifest.json`: 150 jobs / 3,970 verified inputs; SHA-256 `43e9fdf88703d979b13eef475410d6f138be90833b1fe67de72bf99abe741562` |
| full suite | [791 passed](research/batches/campaign-c2-2026-10-05/acceptance/discovery-2026-10-12-curated-wave3-suite-result.json), 783.32 seconds; zero failures/errors/skips/blocked network or process calls |
| clean rebuild | `data/rebuilt-wave4-2026-10-12.sqlite`; [all 52 tables/indexes and baseline match](data/exports/wave4-2026-10-12-comparison.json), including multiplicities and both FTS rowids; unchanged accepted code, fresh complete data rebuild |
| preservation | all old logical rows preserved; exactly 57 old reference-source bundle hashes changed; integrity `ok`, no FK violations |
| exports | catalog HTML, target CSV/JSON, 1,304 specification rows, reference-document CSV, owned validation and dated six-model wave4 export (with selected source facts) refreshed; DB hash unchanged |

Current complete PDF bundle: `data/raw/reference-documents-with-wave4-2026-10-12.json`.
Retain all 59 entries and sibling dependencies in later replacements.

## completed changes

- Wave4 adds392 facts across AS8000/SC6100 and eighteen SPEC captures, five new
  identities and two complete PDFs/14pages. [Integration handoff](research/batches/discovery-2026-10-12/integration-wave4/handoff.md).
  AS8000 configured weight/input exceptions and SC6100 node fans/optionalBBU
  remain scoped. SPEC's twelve identities include eight existing/four new;
  case-only I24 displays follow the established rule while raw printed case stays.
  PSU values describe submitted configurations; CPU pages lack selected power
  facts. No manufacturer/deployment claim. 232 CPU names remain outside scope.
  Six fact pages visually checked; AS8000 pages5–12 were not; unused garbled text
  is preserved. Normal categorized exports and opt-in source facts stay distinct.
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

## paused work and exact resumption point

1. **Wave5 + QCT is staged, not promoted.** Stage session48379 exited0 before the
   stop request. [Handoff](research/batches/discovery-2026-10-12/integration-wave5/handoff.md),
   [stage proof](research/batches/discovery-2026-10-12/integration-wave5/stage-report.json).
   Candidate `data/devices-candidate-wave5-2026-10-12.sqlite` SHA
   `0879a77f5d2cdf4edfdce3a88ab03034504c02f99b4645537f43af5db5c32853`;
   candidate manifest `data/import-manifest-candidate-wave5-2026-10-12.json` SHA
   `86f6179ff8dd490739936d5ab416e39e606380acf547ac009b1dc5b269dca81b`.
   It contains5,157 identities /7,302 records /74,837 facts /2,121 sources /
   72 PDFs /3,804 pages;152 jobs /3,984 pins and full61-entry PDF bundle.
   Root stage checks pass preservation, integrity/FK, baseline10/10 and gates.
   Actual-stage independent acceptance, query/inventory checkers, clean rebuild,
   all52-table comparison and final promotion review **have not run**. No wave5
   rebuild path or promotion result exists. Do not rerun completed stage.py.
2. Inputs are independently accepted:
   [wave5 facts](research/batches/discovery-2026-10-12/review/original-vendor-wave5-facts-review.md)
   (three sources/40records/227facts;35new exact hardware codes) and
   [QCT facts](research/batches/discovery-2026-10-12/review/qct-compatibility-facts-review.md)
   (one existing QCT parent/eight literal PSU compatibility rows).
   [Stage-helper review](research/batches/discovery-2026-10-12/review/wave5-stage-helper-review.md)
   accepts the helper, not future stage/rebuild/promotion outcomes. QCT auxiliary
   parent HTML is explicitly copied/pinned. Source contradictions, full regional
   codes and row scopes survive; no disputed fan/service identity, Unattributed
   accessory merge or component-manufacturer inference.
3. Specification grouping is **private-overlay work only** under
   `research/batches/discovery-2026-10-12/spec-grouping-implementation/`.
   [Design](research/batches/discovery-2026-10-12/spec-grouping-design/handoff.md)
   and [design review](research/batches/discovery-2026-10-12/review/spec-grouping-design-review.md)
   accept an optional complete-evidence grouped view with unchanged defaults.
   Author reports48 focused tests (11new+37existing), four bounded live cases,
   zero failures/errors/skips/blocked calls for that focused suite. A separate
   CLI help-text assertion failed; its log is preserved and resolution is pending.
   [Frozen implementation handoff](research/batches/discovery-2026-10-12/spec-grouping-implementation/handoff.md)
   records patch/provenance and limitations. No production application or
   independent code acceptance. All209 real production/test files still match the791-tested freeze.
   Author sessions49301/97157/18964 finished; no implementation process remains.
   Finish data acceptance before considering a reviewed code application.
4. [Next-leads-r3](research/batches/discovery-2026-10-12/next-leads-r3/handoff.md)
   is an unexecuted offline plan: SD4100's exact Download route (type unknown),
   Oracle ZS3-2 and ZS3-4 internal-components pages. Six saved sources checked;
   proposed future cap8HTTP. No requests authorized by this paused checkpoint.
5. Hyve Orion/Rugged attempt stopped at CDN robots HTTP403 after one request;
   neither PDF was requested. [Failure handoff](research/batches/discovery-2026-10-12/hyve-orion-rugged-discovery/handoff.md).
   No retries, alternate routes or source expansion. QCT CCL is already captured.

On explicit resume, verify live/staged hashes and owner status first. Keep one
integrator. Complete the staged wave5 proof sequence using fresh rebuild/output
paths and independent review before promotion; preserve all historical files.
Real code remains frozen until that data milestone is accepted. No collection or
proof job should be restarted merely because an older handoff called it active.

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

Latest recovery pair: `data/devices-before-wave4-2026-10-12.sqlite` and
`data/import-manifest-before-wave4-2026-10-12.json`. Prior wave3/PSU backups remain.
Promoted candidate pair remains separate, byte-identical to live. Seven replaced
exports and hashes are in `research/batches/discovery-2026-10-12/integration-wave4/exports-before/`.
All earlier captures and dated exports remain unchanged.

Full acceptance runner: `research/batches/campaign-c2-2026-10-05/acceptance/run_offline_suite.py`
with fresh `FCC_C1_ACCEPTANCE_LABEL`. Rebuild to a NEW path and compare with
`scripts/compare_rebuild.py` explicitly selecting the candidate database.
Worldwide coverage, current support, marketplace stock and auction-unit
configuration remain unproven.
