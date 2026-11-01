# Current handoff

Updated 2026-10-04. Scope: [GOAL.md](GOAL.md). Rules: [AGENTS.md](AGENTS.md).
Read [the workflow](docs/agent-workflow.md) before delegation. Use GPT-6.1 Sol
workers here, or the latest available Sonnet on Claude (user baseline: 5.5).
The coordinator owns current documentation; independent review closes each batch.

## Current work

The goal is active. The networking/Oracle milestone is complete: source, code,
candidate, exports and current documentation passed independent review. The
accepted catalog is promoted and exports are refreshed.
[Assignments and accepted findings](research/networking-primary-batch-2026-10-04.md).

Added 25 dated Oracle X7 VPAT facts and ten community AS7772 device-tree facts.
Oracle SM 1U/SM 2U configurations remain separate, and processor maxima are not
PSU ratings. The new unbranded software-reference identity establishes no new
manufacturer, deployed hardware, installed firmware or link to the owned unit.
Buying reports now categorize these facts while preserving their source limits.

```sh
python3 -m devicehunt buying-report 'Oracle Server X7-2C'
python3 -m devicehunt buying-report 'AS7772-32X'
```

Use the bundled Python in [README](README.md) if needed. Use Oracle's full name:
short aliases are record-local and do not automatically select the VPAT. Omit the
vendor filter to include the unbranded community record. [Buying-report usage](docs/buying-reports.md).
Earlier QCT and owned-label/BIOS facts remain available in live queries.

## Verified live catalog

| Item | Current value |
| --- | --- |
| Database | `data/devices.sqlite`, schema 28 |
| Identities / source records / sources | 4,084 / 5,302 / 283 |
| Facts | 44,763; 35 added in this batch |
| Imported PDF archive | 56 PDFs / 3,651 pages; unchanged |
| Owned validation | 10/10: nine documents plus the reviewed 7760 label |
| ByteDance B4020/B4010 examples | 0/2 qualifying affiliation hits |
| Manufacturer inventory | 2,142 target/identity pairs / 2,141 identities; unchanged |
| All target associations | 4,221 pairs / 4,083 identities; unchanged |
| Hyperscaler assertions | 22; unchanged |
| Raw Inspur support feed | 1,064 rows; no automatic device assertions |
| Clean rebuild | 118 jobs / 2,181 hashed inputs; 43 logical tables/indexes equal |
| Tests | 10 affected tests and full suite of 427 pass on frozen code |

Live SHA-256: `7019d5858161a9cf71b8f8040e013fc23124a87e65e8343892d3265ac6320fd2`.
Every prior row, timestamp, source metadata, role and fact is intact. Candidate
and clean rebuild match with duplicate-sensitive comparisons; device/document
FTS reconstruct from their underlying rows. Integrity and foreign keys pass.
The unbranded artifact explains why total identities exceed target-associated
identities by one. No PDF or complete-reference-bundle metadata changed.

[Current HTML](data/exports/catalog.html), standard CSV/JSON/FCC/hyperscaler
outputs, collection reports and all 98 eligible saved lookups use this checkpoint.
The [Oracle/AS7772 buying snapshot](data/exports/networking-primary-2026-10-04-buying-reports.json)
uses both full queries with no vendor filter. Earlier dated buying snapshots
remain historical. HTML DOM checks pass; no screenshot was taken.

## Proof and recovery

- Manifest: `data/import-manifest.json`; accepted new input:
  `data/networking-primary-facts-2026-10-04.json`.
- Complete PDF bundle: `data/raw/reference-documents-with-vendor-discovery-2026-10-04.json`;
  all 45 reference entries remain intact; other collections account for the rest of the PDFs.
- Clean rebuild: `data/rebuilt-with-networking-primary-2026-10-04.sqlite`.
- Prior live and manifest: `data/devices-before-networking-primary-2026-10-04.sqlite`
  and `data/import-manifest-before-networking-primary-2026-10-04.json`.
- Accepted captures, code/tests, source/code/candidate reviews and promotion freeze:
  `research/batches/networking-primary-2026-10-04/`.
- [Final integration proof](research/batches/networking-primary-2026-10-04/integration/final-verification.json)
  binds live/candidate equality, preserved data, unchanged PDFs, CLI reports,
  source/code hashes and current export audits. Generic `data/exports/rebuild-comparison.json`
  matches the accepted rebuild. Helpers are historical artifacts; do not rerun mutation phases.
- [Previous QCT checkpoint](research/vendor-discovery-batch-2026-10-04.md) and
  [owned label/BIOS import](research/owned-buying-import-batch-2026-10-04.md)
  retain earlier source details. [History](docs/history/README.md) preserves old handoffs.

## Gaps and next bounded work

1. Start from the saved Ingrasys grouped document lead
   `SV1120/SV1121/SV2121/SV2221` in [ODM source notes](research/odm-models-2026-10-04.md).
   Assign one researcher at most four independently accessible product/manual
   captures and eight queries. Review exact model boundaries and company roles;
   Ingrasys is not a Foxconn alias and a parent relationship is not manufacture.
2. In parallel, review the saved NIST 4506 E4-2C detail for one bounded original-
   vendor task. Preserve tested software-module configuration scope; it is not
   general OS compatibility or installed auction hardware. [Oracle notes](research/owned-oracle-buying-2026-10-04.md).
   The X7 VPAT content provenance is recovered; do not restart that search.
3. Continue owned networking power/rack/software gaps only from a distinct new
   exact-model lead. This finite pass found no qualifying installation/power
   manual. [Networking notes](research/owned-networking-buying-2026-10-04.md).
   Three seller statements remain unimported; no new 7760 label is needed.
4. ByteDance stays 0/2: the traced community diagram has no model or ByteDance
   identifier. Its watermark is attribution context only; the OCP agenda is 403
   unavailable. Require a distinct primary affiliation lead before another pass.
   [Trace handoff](research/batches/networking-primary-2026-10-04/bytedance/handoff.md).

Keep all thirteen targets active, including original vendors. Use fresh bounded
worker briefs, one integrator and independent QA. Update and simplify docs at each
milestone. Do not repeat exhausted searches or fetch denied items through another
route; saved source failures remain authoritative. Complete historical coverage
and current marketplace stock are unproven. The 190,198 FCC sitemap URLs remain
discovery metadata; free/public collection and evidence boundaries still apply.
