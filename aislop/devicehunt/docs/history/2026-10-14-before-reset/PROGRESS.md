# current handoff

updated 2026-10-14 PDT (2026-10-15 UTC). `/root` coordinates, integrates and edits
current documentation; workers use GPT-6.1 Sol/high. [GOAL](GOAL.md),
[AGENTS](AGENTS.md), [workflow](docs/agent-workflow.md),
[evidence policy](docs/evidence-policy.md).

**Active user request:** prioritize broad vendor/ODM datasource expansion and reusable
collectors; user corrected single-device hyperfocus. Continue discovery and validation in
“yolo mode” until quota reset. User explicitly resumed after a UI pause. At
approximately 05:39 UTC the reset was expected in the next 1–2 hours; no exact
reset notification has arrived. [Current assignments and evidence](research/discovery-2026-10-14.md).
The [previous shutdown handoff](docs/history/2026-10-14-before-resume/PROGRESS.md)
is historical and does not stop this resumed run.

## accepted catalog

**Wave5 + QCT promoted on resumption.** [Independent final acceptance](research/batches/discovery-2026-10-12/review/wave5-integration-review.md),
[root acceptance receipt](research/batches/discovery-2026-10-12/integration-wave5/promotion-acceptance.json),
[promotion result](research/batches/discovery-2026-10-12/integration-wave5/promotion-result.json).
These newly created proofs retain the original pending wave5 filenames so the
reviewed helpers remain unchanged; their execution date is 2026-10-14 PDT.

| item | verified state |
| --- | --- |
| authority | `data/devices.sqlite`, schema 29; SHA-256 `0879a77f5d2cdf4edfdce3a88ab03034504c02f99b4645537f43af5db5c32853` |
| identities / records / sources / facts | 5,157 / 7,302 / 2,121 / 74,837 |
| PDF documents / pages | 72 / 3,804; complete reference bundle has 61 entries / 3,730 pages |
| owned baseline | 10/10; nine document snapshots + one reviewed 7760 label |
| ByteDance B4020/B4010 expectations | 0/2 qualifying affiliation hits; unresolved |
| manufacturer inventory | 2,147 target/identity pairs / 2,146 identities; unchanged |
| all target associations | 4,629 pairs / 4,486 identities |
| hyperscaler assertions | 22 rows on 18 devices; unchanged |
| accepted lineage | 146 distinct artifacts / 374 receipts / four reviewed statements; unchanged |
| manifest | `data/import-manifest.json`: 152 jobs / 3,984 verified inputs; SHA-256 `86f6179ff8dd490739936d5ab416e39e606380acf547ac009b1dc5b269dca81b` |
| code validation | all 209 production/test/dependency files match the accepted 791-test freeze; no new full-suite run for this data-only milestone |
| fresh offline rebuild | `data/rebuilt-wave5-2026-10-12.sqlite`; [all 52 tables/indexes and baseline match](data/exports/wave5-2026-10-12-comparison.json), including duplicate multiplicities and both FTS rowids |
| preservation | all 1,060,700 old logical rows preserved across 50 non-FTS tables; exactly 59 expected reference-bundle source hashes change; candidate/rebuild integrity `ok`, FK violations zero |

Complete PDF bundle: `data/raw/reference-documents-with-wave5-2026-10-12.json`.
Retain all 61 entries and sibling dependencies in later replacements.

Wave5 adds **35 exact hardware identities, 41 records and 235 source-scoped
facts**: original-vendor ordering/accessory facts plus eight literal QCT PSU
compatibility rows. Documentation-vendor roles do not imply manufacture or
auction-unit fitment. Full regional codes, Oracle plain-7420 scope and source
contradictions remain separate; disputed fan/service entries remain excluded.
[Actual stage review](research/batches/discovery-2026-10-12/review/wave5-stage-review.md)
and [query proof](research/batches/discovery-2026-10-12/integration-wave5/model-query-check.json).

Prior wave4/SPEC, wave3, curated-import atomicity and shopping-query changes
remain accepted. Their detailed proof links and limits are preserved in the
[pre-resumption handoff](docs/history/2026-10-14-before-resume/PROGRESS.md).

## in progress and next steps

1. **Wave5 export refresh completed**, session81779 exited0. Eight outputs were
   hash-verified against [the export receipt](research/batches/discovery-2026-10-12/integration-wave5/exports-report.json);
   promoted pair and campaign gates pass [post-promotion verification](research/batches/discovery-2026-10-12/integration-wave5/promoted-gate-check.json).
   Independent final documentation review remains pending.
2. **New practical sources** are captured under
   `research/batches/discovery-2026-10-14/practical-sources/`: SD4100 Download
   redirected to a complete two-page PDF; Oracle ZS3-2 and ZS3-4 internal-component
   pages succeeded. Six actual HTTP responses including host policies. One
   sandbox DNS failure preceded approved execution; no remote retry/denial.
   Both PDF pages were visually inspected by author and root. Candidate has 3 sources / 4 records / 60 facts and passed author checks;
   [handoff](research/batches/discovery-2026-10-14/practical-sources/handoff.md).
   It is parked unimported while broad datasource expansion takes priority.
   SD4100 exact order `R4086-F9001-01` is separate from the base model; Oracle
   component part numbers remain parent-scoped facts, not parent identifiers.
3. **Optional specification grouping** remains private, not production.
   [Independent review](research/batches/discovery-2026-10-14/grouping-review/review.md)
   passed 48 focused tests and 16 evidence checks but requested changes: table
   output hid distinct raw DMI values; private tests depended on overlay paths.
   CLI help assertion failure was wrapping, not missing help content.
   Fix is checkpointed in [the private handoff](research/batches/discovery-2026-10-14/grouping-fix/handoff.md);
   49 focused tests and CLI probes passed; normal-layout suite75634 also exited0 with 49 passes. Root must independently review before production application.
   Work is parked for broad datasource expansion; original overlay is immutable.
4. **Broad datasource extraction and review completed.** The separate
   [vendor discovery index](research/batches/discovery-2026-10-14/vendor-index-review/review.md)
   contains 889 rows from 243 captures: 615 Edge-Core document links, 85 Oracle
   legacy documentation links and 189 Celestica sitemap entries. All rows and
   hashes independently verified; 8 tests pass and its SQLite rebuild is
   byte-identical. Of 548 document/index endpoints, 540 are absent from the old
   catalog URL inventory; this is not device novelty. Forty revision disagreements
   remain and one known-missing route is quarantined. Accton's failed sitemap is
   unavailable, not an empty inventory. Query instructions are in the
   [integration contract](research/batches/discovery-2026-10-14/vendor-source-expansion/integration-contract.md).
5. **Breadth catalog integration pending.** Independently accepted source
   candidates total 15 sources / 467 records / 5,720 facts: Red Hat nine-company
   certification lists (242 records), NVIDIA/MiTAC whole-row/card selection
   (214 records, 154 company/model pairs) and BISDN's complete 11-row HCL.
   [Hash-bound inputs](research/batches/discovery-2026-10-14/integration-breadth/accepted-inputs.json)
   link the exact candidates and independent reviews. Root alone stages under
   `integration-breadth/`; helper preservation/hash/path fixes independently pass
   10 probes. Plan review, actual staging, fresh rebuild and promotion remain.
   Production code and the complete 61-entry PDF bundle are unchanged.
6. **Nine new Edge-Core datasheets / 40 pages** captured across nine family
   buckets within 28 HTTP exchanges. Root independently re-extracted every page;
   text matches exactly. [Corpus acceptance](research/batches/discovery-2026-10-14/edgecore-corpus-review/review.md)
   is for document search only; no structured device/manufacturer claims.
   `corpus-integration-plan/` is preparing a helper to append these to the full
   reference bundle after the breadth batch, without dropping existing entries.
7. **AMD PDF mapping review active**, isolated under `amd-qualified-review/`:
   six saved pages / 142 raw fragments, previously excluded from ODM curation.
   Visual table mapping must resolve company/model boundaries before a new
   candidate can be accepted. No network or live writes in that assignment.

All three replacement Sol workers verified full filesystem access, network
enabled and approval-never settings. Old restricted sessions are not resumed.
Their writable paths remain disjoint; root is the sole shared-state integrator.
LVFS and Dell catalog probes stopped at disallowing robots policies; metadata
was not requested and no alternate retrieval is planned. No new RRA cycle.

## operating limits and recovery

SUN-6000 remains Kladno-only; Monterrey is unmapped context. Oracle/Celestica
identities stay separate. Quanta/QCT and Oracle/Sun display families do not merge
identities; IEIT/Inspur mapping remains unresolved. ByteDance exact-code searches
found no qualifying primary evidence; do not repeat synonymous routes without
new sources. Hyve Orion/Rugged CDN robots returned403; do not retry the policy,
resources or alternate copies. Other saved access failures remain binding.

Campaign gate remains release `campaign-release-1d45317e…`, coverage
`coverage-b8ee5df9…`, 12 accepted decisions. Current campaign definition is
`research/batches/discovery-2026-10-11/sun6000-a/inventory-r2/`. Campaign-source
changes require queue → independent decisions → release → manifest gate update.
Archive authority is `data/archive/`; this milestone does not write it.

Use project direnv `python3` 3.14.7; no installs/system changes. Workspace is
untracked in its parent repository. Preserve historical files and recovery pairs;
no further cleanup/deletion is authorized. Latest recovery pair:
`data/devices-before-wave5-2026-10-12.sqlite` and
`data/import-manifest-before-wave5-2026-10-12.json`. Candidate pair remains
separate and byte-identical to promoted live. Promotion journal binds both pairs.

Full acceptance runner:
`research/batches/campaign-c2-2026-10-05/acceptance/run_offline_suite.py`, with
fresh `FCC_C1_ACCEPTANCE_LABEL`. Rebuild only to a NEW path, compare explicitly
against the candidate with `scripts/compare_rebuild.py --database ...`.
Worldwide coverage, current support, marketplace stock and auction-unit
configuration remain unproven. Stop/recheck actual handles on interruption.
