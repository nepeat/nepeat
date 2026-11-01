# Current handoff

Updated 2026-10-04. Scope: [GOAL.md](GOAL.md). Rules: [AGENTS.md](AGENTS.md).
Read [the workflow](docs/agent-workflow.md) before delegating. Use GPT-6.1 Sol
workers here, or the latest available Sonnet on Claude (user baseline: 5.5).
The coordinator edits current docs; an independent reviewer closes each milestone.

## Current work

The goal is active. The [server-evidence milestone](research/server-evidence-batch-2026-10-04.md)
is complete and promoted. Source, code, candidate, export and independent
[documentation review](research/batches/server-evidence-2026-10-04/docs-review.md)
passed. Current exports are refreshed and verified.

Added six precise Ingrasys identities: SV1120A, SV1120I, SV1121A, SV1121I,
SV2121A and SV2121I. Eight vendor records also enrich existing SV2221A/I.
Separate dated Foxconn portfolio records preserve the parent relationship without
asserting physical manufacture. Oracle E4-2C gains sixteen saved NIST module-test
facts, with both PAA configurations and their status/caveat intact. This is another
extraction of the existing capture, not independent corroboration.

```sh
python3 -m devicehunt buying-report 'SV1120A'
python3 -m devicehunt buying-report 'SV2121I'
python3 -m devicehunt buying-report 'Oracle Server E4-2C'
```

Use bundled Python in [README](README.md) if needed. Keep full model names and
suffixes. Omit the vendor filter to include all specification records: Ingrasys
is a distinct source brand, while `--vendor Foxconn` selects separately affiliated
portfolio records. `--vendor Ingrasys` is not a supported target filter.
[Buying-report usage](docs/buying-reports.md) explains these record-local limits.

## Verified live catalog

| Item | Current value |
| --- | --- |
| Database | `data/devices.sqlite`, schema 28 |
| Identities / source records / sources | 4,090 / 5,317 / 287 |
| Facts | 44,975; 212 added: 124 vendor, 72 portfolio/context, 16 NIST detail |
| Imported PDF archive | 57 PDFs / 3,661 pages; one reconstructed ten-page derivative added |
| Owned validation | 10/10: nine documents plus the reviewed 7760 label |
| ByteDance B4020/B4010 examples | 0/2 qualifying affiliation hits |
| Manufacturer inventory | 2,142 target/identity pairs / 2,141 identities; unchanged |
| All target associations | 4,227 pairs / 4,089 identities; six added |
| Hyperscaler assertions | 22; unchanged |
| Raw Inspur support feed | 1,064 rows; no automatic device assertions |
| Clean rebuild | 119 jobs / 2,191 hashed inputs; all 43 logical tables/indexes equal |
| Tests | 11 affected tests and full suite of 428 pass on frozen code |

Live SHA-256: `bc4aa49abd97e1a20b1f0d1830457eb2086ddaa968946df9ba53524d191c49c0`.
Old evidence, timestamps, roles, facts and PDF/page contents are preserved. Only
45 previous reference-source bundle hash/path/fetch fields refreshed. Candidate
and rebuild match with duplicate-sensitive comparisons; both FTS indexes reconstruct.
Integrity and foreign keys pass. One unbranded AS7772 software-reference identity
explains the difference between total and target-associated identity counts.

The Ingrasys original remains malformed and immutable. The archived derivative
uses its second PDF span plus only three terminal EOF bytes. All ten image-only
pages were parsed, rendered and independently reviewed; page text remains blank
with `no_embedded_text`, not missing. A separately preserved text envelope supplies
required PDF-hash/extraction metadata. No original content or importer check was
weakened. [Recovery provenance](research/batches/server-evidence-2026-10-04/ingrasys/handoff.md).

The [HTML catalog](data/exports/catalog.html), standard CSV/JSON/FCC/hyperscaler
outputs, collection reports and 98 eligible lookups are refreshed. The new
[nine-query buying snapshot](data/exports/server-evidence-2026-10-04-buying-reports.json)
uses unfiltered exact queries. DOM checks cover the six new models, parent-only
roles and ByteDance gaps. Previous dated buying snapshots remain historical.

## Proof and recovery

- Manifest: `data/import-manifest.json`; new curated input:
  `data/server-evidence-facts-2026-10-04.json`.
- Complete PDF bundle: `data/raw/reference-documents-with-server-evidence-2026-10-04.json`;
  46 entries retain all 45 prior entries. Other collections account for remaining PDFs.
- Clean rebuild: `data/rebuilt-with-server-evidence-2026-10-04.sqlite`.
- Prior live and manifest: `data/devices-before-server-evidence-2026-10-04.sqlite`
  and `data/import-manifest-before-server-evidence-2026-10-04.json`.
- [Staging proof](research/batches/server-evidence-2026-10-04/integration/staging-ready.json)
  binds preservation, source/code hashes, comparisons and real CLI reports.
  Accepted inputs, reviews, failures and transformation proofs stay in that batch.
- [Final verification](research/batches/server-evidence-2026-10-04/integration/final-verification.json)
  and [freeze](research/batches/server-evidence-2026-10-04/integration/final-freeze.json)
  bind promoted bytes, exports, all PDF hashes and completed process logs.
- Helpers are historical once run. Do not rerun mutation phases or replace the full
  bundle with a proposal. [History](docs/history/README.md) preserves old handoffs;
  the previous Ingrasys next step was corrected against actual imported records.

## Gaps and next bounded work

1. Seek exact Inventec Entei and Golduck specifications from at most three new
   public primary captures and six queries. Their live records currently contain
   only dated manufacturer/identity context. [Verified gap rows](research/batches/server-evidence-2026-10-04/next-work-verification.json)
   and [source notes](research/inventec-qct-2026-10-04.md) identify the starting point.
   E850G4 already has a practical brief; do not repeat its completed collection.
2. Preserve original-vendor work. Owned Celestica/Edge-Core power, rack and software
   gaps need a distinct new exact-model lead after the exhausted search pass.
   [Networking notes](research/owned-networking-buying-2026-10-04.md).
   Three seller statements remain unimported; no new 7760 label is needed.
3. Ingrasys unsuffixed SV1120/SV1121/SV2121/SV2221 remain specification gaps.
   Do not refetch the malformed catalog for them or borrow A/I facts. SV2221A's
   recovered air/liquid options and existing liquid-only HTML remain conflicting
   source statements. Oracle VPAT provenance and this NIST E4 extraction are done;
   [Oracle notes](research/owned-oracle-buying-2026-10-04.md) retain remaining practical gaps.
4. ByteDance remains 0/2. The traced diagram has no model/ByteDance identification;
   OCP agenda and trade-data access failures stay unavailable. Require a distinct
   primary affiliation lead before another pass; no repeated broad searches.

Keep all thirteen targets active. Use fresh bounded briefs, one integrator and
independent QA; keep simplifying docs. Complete historical coverage and current
marketplace stock remain unproven. The 190,198 FCC sitemap URLs are discovery
metadata. Public/free acquisition and saved access restrictions still apply.
