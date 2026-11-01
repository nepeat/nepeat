# Current handoff

Updated 2026-10-04. Scope: [GOAL.md](GOAL.md). Rules: [AGENTS.md](AGENTS.md).
Read [the workflow](docs/agent-workflow.md) before delegating. Use GPT-6.1 Sol
workers here, or latest available Sonnet on Claude (dated user baseline: 5.5).
Documentation editor: `inventec_docs`; independent reviewer: `inventec_review`.

## Current work

The goal remains active across all thirteen targets. The bounded
[Inventec milestone](research/inventec-practical-batch-2026-10-04.md) is promoted:
two existing exact identities gain two records and 42 facts; the original Golduck
brief adds one PDF/two pages. Source/candidate QA, final export verification and
[independent documentation review](research/batches/inventec-practical-2026-10-04/docs-review.md)
passed; this milestone is complete.

Entei's 2020 announcement adds 20 facts covering chassis, CPU, memory, storage,
expansion and networking. Its linked product URL redirects to a generic index,
which is excluded; power, cooling, dimensions and software remain gaps.
Golduck's two-page 2020 brief adds 22 facts including power alternatives/input,
cooling, depth and management. Ratings/options are historical platform claims,
not proof of an auction unit's configuration or entitlement. Printed mm/inch width
conflict and 162-lane wording remain literal; search-only per-CPU memory is excluded.
No new manufacturer claim, alias or deployment assertion was added.

```sh
python3 -m devicehunt buying-report 'Entei'
python3 -m devicehunt buying-report 'Golduck'
```

Use bundled Python in [README](README.md) if needed. Keep exact prefixes/suffixes.
Unfiltered reports include all source-local roles and specifications. For Ingrasys,
`--vendor Foxconn` selects separate affiliated portfolio records; Ingrasys is a
source brand, not a supported target filter. [Report usage](docs/buying-reports.md).

## Verified live catalog

| Item | Current value |
| --- | --- |
| Database | `data/devices.sqlite`, schema 28 |
| Identities / source records / sources | 4,090 / 5,319 / 290 |
| Facts | 45,017; this batch adds 42 |
| Imported PDF archive | 58 PDFs / 3,663 pages |
| Owned validation | 10/10: nine documents plus the reviewed 7760 label |
| ByteDance B4020/B4010 examples | 0/2 qualifying affiliation hits |
| Manufacturer inventory | 2,142 target/identity pairs / 2,141 identities; unchanged |
| All target associations | 4,227 pairs / 4,089 identities; unchanged |
| Hyperscaler assertions | 22; unchanged |
| Raw Inspur support feed | 1,064 rows; no automatic device assertions |
| Clean rebuild | 120 jobs / 2,205 hashed inputs; all 43 logical tables/indexes equal |
| Tests | 47 affected pass; prior 428 full-suite pass reused on identical 162 app/test hashes |

Live SHA-256: `a098c390431efd77d25f162acb039d2ddd938ff5effeffd7e93fa9b0bc34ff16`.
Old rows, roles, facts and PDF/page contents are preserved; only 46 prior reference
sources' bundle hash/path/fetch metadata changed. Duplicate-sensitive comparisons,
reconstructed FTS indexes, integrity and foreign keys pass. No identities were added.
One unbranded AS7772 software-reference identity explains the total/associated gap.

## Proof and recovery

- Manifest: `data/import-manifest.json`; curated input:
  `data/inventec-practical-facts-2026-10-04.json`.
- Complete PDF bundle: `data/raw/reference-documents-with-inventec-practical-2026-10-04.json`;
  47 entries retain all 46 prior entries, with byte-identical sibling dependencies.
- Clean rebuild: `data/rebuilt-with-inventec-practical-2026-10-04.sqlite`.
- Prior live/manifest: `data/devices-before-inventec-practical-2026-10-04.sqlite`
  and `data/import-manifest-before-inventec-practical-2026-10-04.json`.
- [Source acceptance](research/batches/inventec-practical-2026-10-04/source-review.md),
  [candidate review](research/batches/inventec-practical-2026-10-04/candidate-review.md),
  [staging proof](research/batches/inventec-practical-2026-10-04/integration/staging-ready.json)
  and [promotion proof](research/batches/inventec-practical-2026-10-04/integration/promotion-result.json).
- [Final verification](research/batches/inventec-practical-2026-10-04/integration/final-verification.json)
  and [freeze](research/batches/inventec-practical-2026-10-04/integration/final-freeze.json)
  bind promoted bytes, all 58 PDF hashes, two new pages and successful terminal runs.
  Standard HTML/CSV/JSON/FCC/hyperscaler outputs, collection reports and 98 eligible
  lookups are refreshed. The unfiltered [two-query buying snapshot](data/exports/inventec-practical-2026-10-04-buying-reports.json)
  and HTML DOM checks preserve exact models, separate roles and validation gaps.
- [Previous server milestone](research/server-evidence-batch-2026-10-04.md) and
  [Ingrasys recovery provenance](research/batches/server-evidence-2026-10-04/ingrasys/handoff.md)
  retain accepted image-only PDF recovery, suffixed models and Oracle NIST details.
- Helpers are historical once run; do not rerun mutation phases or replace complete
  bundles with proposals. [History](docs/history/README.md) preserves earlier handoffs.

## Gaps and next bounded work

1. At most six queries and three new public primary captures total for exact
   `FPGA SmartNIC C5020X` and QCT `QuantaPlex T42S-2U` practical specifications.
   [Verified starting gaps](research/batches/inventec-practical-2026-10-04/next-work-verification.json)
   are corpus-scoped: dated manufacturer context for C5020X; WSSD CPU/form-factor
   and separate Quanta portfolio context for T42S. Preserve prefixes; do not infer
   a C5000X alias. E850G4/Entei/Golduck completed evidence needs no blind re-search.
2. Owned Celestica/Edge-Core power, rack and software gaps need a distinct exact-model
   lead after the exhausted pass. [Networking notes](research/owned-networking-buying-2026-10-04.md).
   Three seller statements remain unimported; no new 7760 label is needed.
3. Unsuffixed Ingrasys SV1120/SV1121/SV2121/SV2221 remain specification gaps;
   do not borrow A/I facts or refetch the malformed catalog. SV2221A air/liquid
   options versus liquid-only HTML remain conflicting statements. Remaining
   [Oracle practical gaps](research/owned-oracle-buying-2026-10-04.md) stay separate.
4. ByteDance remains 0/2 and needs a distinct primary affiliation lead. The traced
   diagram lacks model/ByteDance identity; saved OCP/trade access failures remain
   unavailable. Avoid another broad search without that lead.

Keep simplifying docs with one editor and independent QA. Worldwide coverage and
current marketplace stock remain unproven; 190,198 FCC sitemap URLs are discovery
metadata. Public/free collection and saved access restrictions still apply.
