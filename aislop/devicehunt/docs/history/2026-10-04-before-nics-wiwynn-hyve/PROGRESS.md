# Current handoff

Updated 2026-10-04. Scope: [GOAL.md](GOAL.md). Operating rules: [AGENTS.md](AGENTS.md).
Read this checkpoint first; load historical notes only for a specific question.

## Current work

The user approved subagent delegation and ongoing documentation simplification.
The goal, usage guide and handoff are now separated from historical narratives;
[workflow and task contracts](docs/agent-workflow.md) define ownership and review.
This documentation milestone does not import additional hardware or validate the
pending code change. Original docs are preserved in the [history index](docs/history/README.md).
See the [ownership and review record](research/documentation-workflow-2026-10-04.md)
for the completed documentation assignment and its checks.

App goal metadata was last observed paused, with the older objective text. The
available goal tool cannot edit that objective in place; GOAL.md carries the
current documentation-maintenance clause. Do not mark unfinished collection
complete merely to replace its app metadata.

## Verified catalog checkpoint

| Item | Current value |
| --- | --- |
| Authoritative database | `data/devices.sqlite`, schema 28 |
| Identities / source records / sources | 4,064 / 5,273 / 263 |
| Imported PDF archive | 50 PDFs / 3,641 pages |
| Owned validation | 10/10: nine document snapshots plus the reviewed 7760 label |
| ByteDance B4020/B4010 examples | 0/2 qualifying evidence hits; separate open gaps |
| Manufacturer inventory | 2,142 target/identity pairs / 2,141 distinct identities |
| All associations | 4,199 pairs / 4,064 identities |
| Raw Inspur support feed | 1,064 rows; zero automatic device assertions |
| Last completed rebuild | 114 jobs / 2,146 hashed inputs; 43 tables/indexes match |
| Last completed full tests | 408 Python + 6 report tests, BEFORE the pending plus-suffix edit |

Database SHA-256:
`935d8774acac42b1906f1e30d77b127a7d40534123d72cb1cd916a3a84be8dcb`.
Counts, hash and both validation sets were rechecked during the workflow review.

Last imported batch: **88 identities / 93 records** from Inspur 2018 and Inventec
Artemis II Rack/X680m brochures. Inspur has 133 associated / 3 manufacturer-backed
identities; Inventec has 81 / 44. Source conflicts, chassis/node distinctions and
one source-scoped NF5288M5/AGX-2 alias remain explicit. See
[the source review](research/inspur-2018-inventec-2026-10-04.md).

Proof and recovery paths:

- Final audit: `data/exports/inspur-2018-inventec-final-verification-2026-10-04.json`.
- Clean rebuild: `data/rebuilt-with-inspur-2018-inventec-2026-10-04.sqlite`.
- Previous live backup: `data/devices-before-inspur-2018-inventec-2026-10-04.sqlite`.
- Current import manifest: `data/import-manifest.json`.
- Complete PDF bundle: `data/raw/reference-documents-with-inspur-2018-inventec-2026-10-04-v2.json`.
- The rejected nested-path v1 bundle and first candidate are historical; do not
  import them or relax the sibling-path check. Latest exports include 98 saved
  lookups and CSV/JSON/HTML/FCC/hyperscaler reports. HTML was checked through a
  DOM harness; no browser screenshot is claimed.

## Next bounded batch: NICs / Wiwynn / Hyve

**Captured and visually reviewed; NOT curated, manifested or imported.**
Read the [pending-batch details](research/workflow-review-2026-10-04.md#pending-batch-nics-wiwynn-and-hyve)
before doing more work. Its proposed architecture was subsequently adopted; the
source findings and pending status remain useful.

| Assignment | Inputs / expected output |
| --- | --- |
| Research A | Review the eight literal Inspur NIC names on PDF pp14–16 of `data/raw/inspur-incloud-rail-compatibility-2026-10-04.pdf`; produce source-qualified facts, preserving the X722 conflict and blank remarks. |
| Research B | Finish the existing Wiwynn/Hyve captures in `data/raw/nics-wiwynn-hyve-2026-10-04/`; retain V versus V+, separate chassis/nodes, and conflicting HTML/PDF statements. |
| Engineering | Review pending `devicehunt/listings.py` and `tests/test_listings.py` plus-suffix change; 22 affected tests passed, full suite pending. |
| Independent review | Check proposed identities, exact suffixes, company roles, conflicts and source hashes before acceptance. |
| Integration | Append five PDFs/eight pages to the complete collection; import accepted facts into a new candidate, preserve old data, verify/rebuild/compare, then promote and refresh exports. |

The capture report records ten successes with verified hashes and one Hyve Ambient
Series-E HTTP 403. Do not repeat those captures or bypass the denial. Eleven page
renders are under `data/exports/nics-wiwynn-hyve-visual-review/`.

After this batch, prioritize original-vendor enterprise sources and useful exact
ordering codes, power/chassis dependencies, firmware access and licensing facts.
Continue ByteDance affiliation research without treating the separate Accton
applicant statement as ByteDance proof. Improve repeated batch tooling when it
reduces one-off scripts; do not rerun archived helpers wholesale.

## Coverage and continuation limits

190,198 FCC-ID sitemap candidates are discovery URLs, not a complete filing archive.
Complete historical inventories and current marketplace stock remain unproven.
Review [FCC source coverage](research/fcc-source-coverage.md), the relevant research
note and saved fetch reports before collection. Existing access failures remain
failures, not empty results or invitations to alternate retrieval routes. The
7760 label already resolves its ordering-code/SKU link; no new label request is needed.

Commands, bundled Python and SQL entrypoints are in [README.md](README.md).
Preserve the ten owned checks and separate ByteDance gaps in every data milestone.
Update this checkpoint in place after accepted work; put detailed evidence in a
source note and archive superseded narratives. The goal includes keeping these
instructions concise enough for a fresh Astra/Fable or other agent to continue.
