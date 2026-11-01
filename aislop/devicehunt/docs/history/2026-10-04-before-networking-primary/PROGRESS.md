# Current handoff

Updated 2026-10-04. Scope: [GOAL.md](GOAL.md). Rules: [AGENTS.md](AGENTS.md).
Read [the subagent workflow](docs/agent-workflow.md) before delegation. The
coordinator owns current documentation; this batch used GPT-6.1 Sol workers.

## Current work

The goal is active. This vendor-discovery milestone is complete: the reviewed
candidate is live, current exports are refreshed, and final documentation review
passed. [Assignments and proof](research/vendor-discovery-batch-2026-10-04.md).

Added **38 facts in two existing QCT identities**: QuantaGrid D51B-2U and
QuantaGrid D52BQ-2U. These include PSU options, cooling, chassis, memory, storage
and management statements. QCT roles and the earlier Quanta portfolio records
remain separate; no manufacturer or deployment assertion was added.

```sh
python3 -m devicehunt buying-report 'QuantaGrid D51B-2U'
python3 -m devicehunt buying-report 'QuantaGrid D52BQ-2U'
```

Use the bundled Python from [README](README.md) if needed. Keep the full model
prefix and omit the vendor filter to see every exact-record source. [Buying
usage](docs/buying-reports.md) explains category limits and record-local filters.
The D51B source is a QCT-branded public-mirror PDF; the D52BQ source is original
public HTML. Neither states input voltage. T41S partner-branded material stays
research only. [Source handoff](research/batches/vendor-discovery-2026-10-04/qct/handoff.md).

ByteDance remains **0/2**. A newly saved community article gives a B4020 diagram
lead, without a cited primary ByteDance source. Its specifications and affiliation
remain unimported. [Search results and next lead](research/batches/vendor-discovery-2026-10-04/bytedance/handoff.md).

## Verified live catalog

| Item | Current value |
| --- | --- |
| Database | `data/devices.sqlite`, schema 28 |
| Identities / source records / sources | 4,083 / 5,300 / 281 |
| Facts | 44,728; 38 added in this batch |
| Imported PDF archive | 56 PDFs / 3,651 pages; one two-page PDF added |
| Owned validation | 10/10: nine documents plus the reviewed 7760 label |
| ByteDance B4020/B4010 examples | 0/2 qualifying affiliation hits |
| Manufacturer inventory | 2,142 target/identity pairs / 2,141 identities; unchanged |
| All associations | 4,221 pairs / 4,083 identities; unchanged |
| Hyperscaler assertions | 22; unchanged |
| Raw Inspur support feed | 1,064 rows; no automatic device assertions |
| Clean rebuild | 117 jobs / 2,178 hashed inputs; 43 logical tables/indexes equal |
| Tests | 19 affected tests pass; application/test code unchanged; previous full suite 424 pass |

Live database SHA-256:
`5b1317f554968fae6c20a4dc0cd2d4d8524ee1c5e60a662675e36a763203a904`.
All prior records, timestamps, raw rows, roles, identifiers and facts remain intact.
Only the prior 44 reference-source bundle hash/cache/fetch metadata refreshed.
The complete reference collection now has 45 entries; other collections account
for the rest of the 56-PDF archive. Garbled embedded text remains literal; reviewed
D51B specifications come from its visually inspected page 2, without OCR.

[Current QCT buying reports](data/exports/vendor-discovery-2026-10-04-buying-reports.json)
and [HTML catalog](data/exports/catalog.html) use this checkpoint. All 98 eligible
saved lookups and normal CSV/JSON/FCC/hyperscaler outputs are refreshed. HTML DOM
checks pass; no screenshot was taken. Earlier buying snapshots remain historical.

## Proof and recovery

- Current manifest: `data/import-manifest.json`; new facts input:
  `data/vendor-discovery-facts-2026-10-04.json`.
- Complete PDF bundle: `data/raw/reference-documents-with-vendor-discovery-2026-10-04.json`.
- Clean rebuild: `data/rebuilt-with-vendor-discovery-2026-10-04.sqlite`.
- Previous live: `data/devices-before-vendor-discovery-2026-10-04.sqlite`.
- Previous manifest: `data/import-manifest-before-vendor-discovery-2026-10-04.json`.
- Accepted inputs, independent reviews, tests, preservation and CLI/FTS proofs:
  `research/batches/vendor-discovery-2026-10-04/`.
- [Final integration audit](research/batches/vendor-discovery-2026-10-04/integration/final-verification.json)
  binds all 56 PDF hashes and current export audits to the live database. Generic
  `data/exports/rebuild-comparison.json` matches the accepted clean rebuild.
- The batch's integration helpers are historical evidence. Do not rerun mutation
  phases against live state. Both FTS logical contents and duplicate multiplicities
  were checked; physical SQLite segment layout is not logical data.
- [Previous buying import](research/owned-buying-import-batch-2026-10-04.md) preserves
  BIOS certification arrays and photographed 7760 power evidence. Its saved buying
  reports are historical snapshots. Current live retains all those source facts.

## Next bounded work

1. Trace the saved ByteDance community diagram to at most three original
   presentation/product sources. Preserve negative results; the article and Accton
   B4010 applicant record do not establish ByteDance affiliation. Do not repeat the
   exhausted broad query set or retry the known unavailable trade-data page.
2. Return to original-vendor buying gaps with one exact-model installation/service
   source per unresolved owned model, prioritizing Celestica/Edge-Core power and
   required rack components. [Networking gaps](research/owned-networking-buying-2026-10-04.md).
   Recover X7 VPAT original-URL metadata before import; NIST detail and three seller
   statements remain separate, unimported evidence classes. [Oracle gaps](research/owned-oracle-buying-2026-10-04.md).
3. Keep all thirteen targets active. For later QCT work, seek independently
   accessible installation documentation for input/derating requirements and review
   T41S provenance before importing its captured partner document. Use one integrator,
   independent QA and ongoing documentation simplification at each milestone.

Complete historical coverage and current marketplace stock remain unproven.
The 190,198 FCC-ID sitemap candidates are discovery URLs. Respect saved access
restrictions, including Eximpedia, denied UL/certificates, Hyve Ambient Series-E
and QCT gated downloads; do not retrieve denied items by alternate routes.
No new 7760 label is required. [History](docs/history/README.md) preserves prior
checkpoints; detailed source evidence belongs in research notes, not worker briefs.
