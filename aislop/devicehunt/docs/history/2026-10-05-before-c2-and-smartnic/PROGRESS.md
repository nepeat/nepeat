# current handoff

updated 2026-10-04. objective: [GOAL](GOAL.md); rules: [AGENTS](AGENTS.md),
[workflow](docs/agent-workflow.md), [evidence policy](docs/evidence-policy.md).
coordinator/integrator/documentation editor: `/root`. archive release reviewers:
`archive_final_review` and `archive_docs_high`, GPT-6.1 Sol at high effort.
use explicit high effort for future Sol workers per the user's medium-high preference.

## completed archive and storage milestone

[archive milestones 0–6](research/archive-implementation-2026-10-04.md) are
implemented and accepted for the finite pilot: immutable objects and receipts,
reconstructible registry, versioned saved/local derivations, bounded capture
adapter, reviewed claim lineage, frozen catalog release and a second XML source.
[usage and restoration](docs/archive-storage.md); [original plan](docs/artifact-archive-plan.md).

47 reference PDFs and their complete saved text were registered in place.
Ingrasys retains its malformed original and explicit reconstruction chain.
the offline FCC adapter preserved 48 advertised child outcomes (38 saved successes,
10 unavailable), with zero new network attempts. five consumers used three local
runs; two compatible executions were reused. XML discovery adds no hardware claims.

four **existing** Golduck facts now resolve through `python3 -m devicehunt evidence
Golduck`: original PDF, page/row selector, literal excerpt, capture/extraction
lineage and review decision. the width-unit conflict and 162-lane wording remain
literal. other catalog facts have not all been migrated to reviewed lineage.
legacy missing times/versions remain unknown; no OCR or automatic entity matching
is claimed. raw originals and complete sibling collections are preserved.

## accepted catalog

| item | verified state |
| --- | --- |
| authority | `data/devices.sqlite`, schema 29 |
| identities / records / sources | 4,090 / 5,319 / 290; unchanged |
| facts | 45,017; unchanged |
| catalog PDF archive | 58 PDFs / 3,663 pages |
| accepted lineage release | 146 artifacts / 374 receipts / four reviewed statements |
| owned baseline | 10/10: nine documents plus the reviewed 7760 label |
| ByteDance B4020/B4010 expectations | 0/2 qualifying affiliation hits; unresolved |
| manufacturer inventory | 2,142 target/identity pairs / 2,141 identities |
| all target associations | 4,227 pairs / 4,089 identities |
| hyperscaler assertions | 22; unchanged |
| raw Inspur support feed | 1,064 rows; no automatic device assertions |
| offline rebuild | 121 jobs / 2,255 hashed inputs / all 52 logical tables and indexes equal |
| verification | 491 tests; duplicate-sensitive preservation, integrity, foreign keys and FTS pass |
| exports | standard outputs, 98 lookup reports and four-claim evidence JSON refreshed |

live SHA-256: `62ab16eb8ab562ebbd6eaf6951987800fa8f4482879007d1713d0b8529ce0e4a`.
all 43 old logical tables are preserved, except 285 hash-verified source-cache
paths relocated from the prior macOS workspace. the current rebuild uses saved
complete NTRA positional tables and text, reproducing all 169 existing target
records without optional PDF libraries. no packages were installed.

## storage and recovery

124 loose SQLite files became three catalog/recovery/rebuild files plus two tiny
registry/lease databases. 123 historical databases retain their exact bytes in
[compressed history](data/history/README.md), with restored-file integrity checks.

[measured database storage](research/batches/archive-implementation-2026-10-04/storage-final.json),
including compressed history: **33.17 GB → 4.43 GB logical**; filesystem allocation
**8.12 GB → 3.73 GB**. the entire `data/` directory is about 5.61 GB logical / 4.39 GB
allocated after this batch. these are decimal GB; logical size differs from disk
allocation. no historical snapshot content or immutable source capture was discarded.

| role | current path |
| --- | --- |
| accepted manifest | `data/import-manifest.json` |
| frozen lineage release | `data/archive-release-2026-10-04.json` |
| complete reference collection | `data/raw/reference-documents-with-inventec-practical-2026-10-04.json` |
| latest verified rebuild | `data/catalog-archive-rebuilt-2026-10-04.sqlite` |
| immediate recovery catalog / manifest | `data/devices-before-archive-2026-10-04.sqlite` / `data/import-manifest-before-archive-2026-10-04.json` |
| acquisition metadata / immutable outputs | `data/archive/` |

[final acceptance](research/batches/archive-implementation-2026-10-04/promotion-review.md),
[preservation](research/batches/archive-implementation-2026-10-04/preservation-proof.json),
[archive restore](research/batches/archive-implementation-2026-10-04/archive-restore-proof.json),
[promotion](research/batches/archive-implementation-2026-10-04/promotion-result.json),
[post-promotion check](research/batches/archive-implementation-2026-10-04/post-promotion-proof.json)
and [documentation review](research/batches/archive-implementation-2026-10-04/documentation-review.md)
record the gates. the description-only final manifest correction is separately
reviewed; its current hash is in promotion-result. one-shot batch helpers are
historical, not commands to rerun. [prior handoff](docs/history/2026-10-04-before-artifact-release/PROGRESS.md)
retains the displaced detailed Inventec checkpoint and source proof links.

## next bounded hardware work

The [ongoing campaign plan](docs/campaign-plan.md) has Fable (`claude-fable-5-1`,
two Claude Code CLI consultations) and Astra/coordinator review;
[dispositions and provenance](research/campaign-planning-2026-10-04.md).
Completed campaign c0: [RRA/ISED pins and accepted state contract](research/campaign-pilot-2026-10-04.md),
coordinator/editor `/root`; independent reviewer `campaign_contract_review`,
GPT-6.1 Sol high effort. Twelve saved jobs / 1,121 source paths are pinned in place;
18 fixture checks and network-disabled verification pass, with protected hashes
unchanged. No application code, acquisition registry or accepted data changed.
Campaign c1 code is accepted and independently reviewed:
[indexed publication/recovery and durable capture state](research/campaign-c1-2026-10-04.md).
The final core passes 39 affected tests; state/capture integration passes 52.
All 11 frozen code hashes match the reviewed proofs. The final network-guarded
full suite passes **542 tests**, with no forbidden calls, failures or skips.
[Combined acceptance](research/batches/campaign-c1-2026-10-04/acceptance/c1-acceptance.json)
rechecks all 1,549 protected file entries and unchanged archive membership after
the suite; practical documentation also passes independent review.
Accepted data and the live archive are unchanged. The existing acquisition registry
still needs a separate exclusive offline upgrade, with a copied rehearsal and
verified metadata backups, before new-code writes. No live account is activated.
After that gate, c2 implements the bounded saved-input runner and RRA/ISED adapters;
c3 covers reviewed releases/coverage reports, and c4 supervised live refreshes.
The parallel [three-source trade feasibility check](research/batches/campaign-c1-2026-10-04/trade/source-matrix.md)
is complete and independently reviewed. It established no sustainable free
shipment-level feed for this campaign; no trade adapter or data import follows.
This does not supersede the hardware integration assignments below.

1. [SmartNIC/QCT](research/smartnic-qct-batch-2026-10-04.md) still awaits exclusive
   integration: accepted 56 facts and three PDFs were **not** promoted by this
   archive batch. start from the current schema-29 catalog/manifest with a new
   named integrator; retain the complete reference collection and accepted lineage.
2. original vendors remain active. exact-model Celestica/Edge-Core power/rack/
   software gaps need a distinct primary lead; [exhausted networking pass](research/owned-networking-buying-2026-10-04.md).
   seller-only statements remain unimported; no new 7760 label is needed.
3. unsuffixed Ingrasys SV1120/SV1121/SV2121/SV2221 remain gaps; do not borrow
   suffixed facts or refetch the malformed catalog. retain SV2221A cooling conflicts
   and separate [Oracle gaps](research/owned-oracle-buying-2026-10-04.md).
4. ByteDance needs a distinct primary affiliation lead; saved OCP/trade failures
   remain unavailable. avoid another broad search without that lead.

worldwide coverage, current support and marketplace stock remain unproven.
public/free collection, saved access restrictions, exact suffixes and separate
company roles still apply. future archive expansion needs another named finite
source/claim set, not an unbounded crawl.
