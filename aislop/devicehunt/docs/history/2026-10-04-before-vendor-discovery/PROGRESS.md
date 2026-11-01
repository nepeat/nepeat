# Current handoff

Updated 2026-10-04. Scope: [GOAL.md](GOAL.md). Rules: [AGENTS.md](AGENTS.md).
Read [the subagent workflow](docs/agent-workflow.md) before delegation. The
coordinator owns current documentation; all workers in this batch used Sol 6.1.

## Current work

The goal is active. This buying-facts milestone is complete: the reviewed facts
are promoted, current exports are refreshed and final documentation review passed.
Independent source/code review and candidate/rebuild acceptance passed before promotion.
[Assignments and proof](research/owned-buying-import-batch-2026-10-04.md).

New SQL evidence: **206 literal BIOS certification rows in 24 Oracle records**,
stored as 24 JSON-array facts, plus the 7760 photographed input marking and its
location reference. All previous facts, identifiers and roles are preserved.
No new identity, manufacturer, operator or deployment assertion was added.

Use `python3 -m devicehunt buying-report E5-2C` or
`python3 -m devicehunt buying-report 7760-32X-A-AC-F` (bundled runtime in README).
[Buying usage and SQL](docs/buying-reports.md) explain exact-record matching,
array-row queries, company roles and gaps. BIOS rows are historical submitted
configurations; the input marking describes the photograph. Neither proves an
auction unit's configuration, bootability or software entitlement.

[Latest ten-device buying reports](data/exports/owned-buying-import-2026-10-04-buying-reports.json)
include all exact-record company roles. All 98 saved lookups and normal
CSV/JSON/HTML/FCC/hyperscaler exports reflect this database; prior buying snapshots
remain historical. HTML verification used the DOM harness, without a screenshot.

## Verified live catalog

| Item | Current value |
| --- | --- |
| Database | `data/devices.sqlite`, schema 28 |
| Identities / source records / sources | 4,083 / 5,298 / 278 |
| Facts | 44,690; 26 added in this batch |
| Imported PDF archive | 55 PDFs / 3,649 pages; unchanged |
| Owned validation | 10/10: nine documents plus the reviewed 7760 label |
| ByteDance B4020/B4010 examples | 0/2 qualifying affiliation hits; separate gaps |
| Manufacturer inventory | 2,142 target/identity pairs / 2,141 identities; unchanged |
| All associations | 4,221 pairs / 4,083 identities; unchanged |
| Hyperscaler assertions | 22; unchanged |
| Raw Inspur support feed | 1,064 rows; no automatic device assertions |
| Clean rebuild | 116 jobs / 2,172 hashed inputs; 43 logical tables/indexes equal |
| Tests | 18 affected / 424 full Python pass; final HTML DOM checks pass |

Live database SHA-256:
`1e7a877f46b0140c62993de0ebc48b0378773db30c0e889cdfb67244371d7074`.
No new acquisition or PDF bundle change was needed. The original label input,
all captures and the previous live database remain preserved.

## Proof and recovery

- Current input manifest: `data/import-manifest.json`. The existing label job
  now uses `data/label-evidence-with-input-marking-2026-10-04.json`; the old input
  remains hashed. Broadcom uses its unchanged complete capture bundle.
- Complete PDF bundle: `data/raw/reference-documents-with-nics-wiwynn-hyve-2026-10-04.json`.
- Clean rebuild: `data/rebuilt-with-owned-buying-import-2026-10-04.sqlite`.
- Previous live: `data/devices-before-owned-buying-import-2026-10-04.sqlite`.
- Manifest backup: `data/import-manifest-before-owned-buying-import-2026-10-04.json`.
- Accepted hashes, independent reviews, 424-test log, preservation and CLI/SQL/DOM
  proof: `research/batches/owned-buying-import-2026-10-04/`.
- Final audit: that batch directory's `integration/final-verification.json`;
  generic `data/exports/rebuild-comparison.json` matches this live checkpoint.
- Candidate/live equality uses all 43 logical tables, including duplicate-sensitive
  comparisons of both FTS contents. Physical FTS storage layout is not logical data.
- The batch driver is historical integration evidence. Do not rerun its mutation
  phases against current live state. Candidates/rebuilds are not competing catalogs.
- Read [BIOS source semantics](research/broadcom-server-evidence.md) and
  [label scope](research/edgecore-7760-label.md). Source reviews preserve the
  [networking](research/owned-networking-buying-2026-10-04.md) and
  [Oracle](research/owned-oracle-buying-2026-10-04.md) unknowns.

## Next bounded work

1. Return to vendor discovery with two independent finite source groups: a primary
   ByteDance B4020/B4010 evidence pass and up to three server/network/storage
   datasheets for an under-covered expanded vendor, using saved leads first.
   Read [ByteDance exclusions](research/bytedance-inspur-2026-10-04.md) and
   [entity boundaries](research/odm-target-registry.md). Record missing results;
   do not equate Accton's B4010 applicant record with ByteDance affiliation.
2. Keep original-vendor buying gaps active: one exact-model installation/service
   source per unresolved owned model in a subsequent bounded pass. Recover X7
   VPAT original-URL metadata before importing its variant-specific descriptions.
   NIST configuration detail and the three seller statements remain unimported;
   they are separate evidence classes, not general boot or licensing guarantees.
3. Keep all thirteen targets in scope, preserve the 9+1 owned validation distinction,
   and use one integrator after independent review. Simplify repeated integration
   code when useful; keep current docs short and archive superseded narratives.

Complete historical vendor/FCC coverage and current marketplace stock remain
unproven. The 190,198 FCC-ID sitemap candidates are discovery URLs. Respect saved
402/403 outcomes (including Eximpedia, denied UL/certificate originals and Hyve
Ambient Series-E); no alternate-route retries. No new 7760 label is required.

[README](README.md) covers commands and runtime; [history](docs/history/README.md)
preserves earlier checkpoints. Update this handoff in place; source details stay
in research notes rather than being copied into every worker brief.
