# Workflow review — 2026-10-04

This is a proposed division of work in response to the user's review request.
It is not a standing instruction to spawn workers. No subagents were launched
during this review. The goal tool reports the collection goal as paused; this
review did not resume it or change its objective.

## Current verified state

Read-only SQL, database SHA-256 and both validation sets were checked again:

- Schema 28; 4,064 identities, 5,273 records, 263 sources.
- 50 imported PDFs, 3,641 searchable pages.
- Owned validation: 10/10, retaining nine documents plus one label photo.
- ByteDance B4020/B4010: 0/2 qualifying affiliation hits; explicit gaps.
- Live database SHA-256:
  `935d8774acac42b1906f1e30d77b127a7d40534123d72cb1cd916a3a84be8dcb`.
- Last completed milestone: 408 Python tests, six report tests, 114 import jobs,
  2,146 hashed inputs, and a clean comparison across 43 tables/indexes.
  Those full-suite results predate the pending listing-code change below.

The objective remains liquidation hunting across thirteen vendor groups, with
exact identifiers, practical specifications and source-qualified roles. Hyperscaler
provenance is optional. Complete FCC or historical vendor coverage is unproven.

## Recommended operating model

Keep the main chat responsible for priorities, bounded assignments, acceptance
decisions and concise user updates. Delegate source reading, development and
verification with explicit ownership. This session supports the coordinator plus
up to three concurrent workers; rotate roles instead of launching one per vendor.

| Role | Output | Write boundary |
| --- | --- | --- |
| Research A | Captures, hashes, model/role/specification assertions and exclusions for one source group | Its assigned new batch directory and note |
| Research B | Independent source group with the same evidence contract | A different batch directory and note |
| Engineering | A bounded code change and meaningful affected tests | Explicitly assigned modules/tests; no shared catalog writes |
| Evidence review, after a research slot frees | Review of exact suffixes, page/table scope, company roles and conflicts | Review report only |
| Integration, after accepted handoffs | Candidate import, preservation checks, tests, clean rebuild, promotion and exports | Sole writer for manifest, complete PDF bundle, live database and generated exports |

Research and engineering can run concurrently. Integration must wait for accepted
inputs and stable code. One worker owns all shared writes for a batch; nobody
else imports or refreshes exports concurrently. The coordinator reviews compact
results and proof paths rather than replaying every page or command output.

Start workers with fresh, bounded briefs and relevant file paths rather than the
whole conversation. Every brief should name the question, owned paths, source
limits, existing failures, expected artifacts, acceptance criteria and stopping
condition. Return a short summary plus artifact paths; keep long logs in files.
Workers must still follow AGENTS.md, public acquisition limits and evidence rules.

Official OpenAI documentation supports independent bounded tasks with separate
contexts, and notes that additional workers may increase total token use and
are less suitable for dependent steps or shared mutable resources:
[Multi-agent guidance](https://developers.openai.com/api/docs/guides/responses-multi-agent).
The role assignment above is a project-specific recommendation.

## Reduce recurring overhead

Before this review, GOAL.md had 3,526 lines, PROGRESS.md 895 and README.md 1,080.
There are 189 Python/JavaScript session helpers; the helper README counts 188
through the last completed milestone, before the new fetch helper.

Recommended follow-on work: preserve historical milestones in an archive and
keep GOAL.md to the objective/acceptance criteria, PROGRESS.md to the current
checkpoint and work queue, and README.md to normal usage. Consolidate repeated
staging/preservation/promotion checks into a reusable batch command using the
existing import and rebuild modules. Keep source-specific review notes separate.
No history or data was deleted or relocated during this review.

Measure batches by useful exact models, practical facts, resolved gaps and
reproducible evidence, rather than pages fetched alone. Continue original vendors
alongside newer ODMs. A finite source set is a milestone; "all devices" remains
a research direction rather than a completion claim.

## Pending batch: NICs, Wiwynn and Hyve

Captured but NOT curated, added to the import manifest, or imported into the live
catalog. Use the existing captures rather than repeating acquisition:

- `data/raw/nics-wiwynn-hyve-2026-10-04/`: ten successful originals, including
  five PDFs with eight pages, plus derived `wiwynn-product-tables.json`.
- All ten capture hashes were verified during this review.
- Eleven attempted URLs: the Hyve Ambient Series-E OCP document returned HTTP
  403. Preserve that failure; do not retry or retrieve the denied item elsewhere.
- The Inspur NIC tables already exist in
  `data/raw/inspur-incloud-rail-compatibility-2026-10-04.pdf`, PDF pages 14–16.
- Eleven rendered pages were visually reviewed under
  `data/exports/nics-wiwynn-hyve-visual-review/`.

Research A could finish the eight literal Inspur NIC names, using the table-vendor
role without inferring manufacture. Preserve the general X722 10-Gigabit exclusion
on page 14 separately from the specific Inspur X722 management-card row on page
15; blank remarks stay blank. The names are X710_2Port_LC_PCIe,
82599_2Port_LC_PCIe, Inspur_Vostok_I350_1G_RJ_PCIEx4-G3_2,
I350_4Port_RJ45_PCIe, Inspur_Andes-M6_X710_10G_LC_OCP3x8_2,
NIC_10Gbps_2Port_LC_Inspur_X710_PCIe_MM,
Inspur_Sbt_X722_10G_LC_PCIEx8-G3_2 and E810_2Port_LC_PCIe.

Research B could finish the Wiwynn/Hyve documents. Relevant identities:
SV7100G2; SV7220G3 family; SV7220G3-V, SV7220G3-V+ and SV7220G3-S;
ST7000G2, SV7000G2 and SV7000G2-L; Hyve Hercules, HS9124 motherboard and
JBOF named platform. These are review candidates, not verified added counts.

Critical review points:

- SV7220G3-V+ has a superscript plus in the middle table column on PDF page 2.
  Embedded text separates that plus from the model. It is distinct from V and S.
- Wiwynn rack bus-bar power and chassis/node distinctions matter for bare sleds.
  HTML and PDF dimensions, networking and management statements can differ.
- The SV7220G3 HTML download title says SV7200G3; do not create an alias from it.
- The Hyve Hercules May 2026 brochure's introductory paragraph describes Orion,
  conflicting with its Hercules storage table. Preserve the conflict; do not merge
  Hercules, Hercules 9120, Orion or the separate HS9124 motherboard.
- Hyve JBOF prints `12V @ 11-220VAC`. Keep the unusual input text and uncertainty;
  do not silently repair it to 110V. JBOF has no numeric ordering code printed.
- TD SYNNEX ownership statements are dated parent-portfolio evidence, not
  manufacturer assertions. No operator inference follows from these documents.

Engineering has an existing pending change in `devicehunt/listings.py` and two
new tests in `tests/test_listings.py`. Plus signs now participate in exact-model
boundaries and variant leads, so V cannot borrow V+ specifications. The 22
listing/specification tests passed before interruption; the full suite has not
been rerun since that edit. Review the change before integration.

An integrator should append the five PDFs to the COMPLETE current reference
bundle, using validated sibling filenames. Build a new candidate, preserve old
rows and roles, update manifest hashes only after acceptance, and complete the
normal rebuild/comparison/promotion/export checks. The authoritative complete
bundle remains `data/raw/reference-documents-with-inspur-2018-inventec-2026-10-04-v2.json`.
