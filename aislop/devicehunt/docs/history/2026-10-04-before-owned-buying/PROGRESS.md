# Current handoff

Updated 2026-10-04. Scope: [GOAL.md](GOAL.md). Rules: [AGENTS.md](AGENTS.md).
Use the [subagent workflow](docs/agent-workflow.md); read historical notes only
for a specific question. The coordinator owns current documentation updates.

## Current work

The user resumed the goal with **GPT-6.1 Sol workers on ChatGPT** and **latest
Sonnet on Claude** (user baseline 5.5). Two source workers, an engineer, an
independent reviewer and one exclusive integrator handled this bounded batch.

The NIC/Wiwynn/Hyve batch is imported and verified. Preservation, independent
review, actual CLI/DOM checks and a clean rebuild passed before promotion. All
98 saved lookups and current CSV/JSON/HTML/FCC/hyperscaler exports are refreshed.
[Assignments, acceptance and batch proof](research/nics-wiwynn-hyve-batch-2026-10-04.md).

App metadata still reports the older paused goal and cannot be edited in place
with the available goal tools. The user's explicit current instruction resumes
work; GOAL.md carries the current scope, model policy and documentation clause.
Do not mark unfinished collection complete merely to replace app metadata.

## Verified live catalog

| Item | Current value |
| --- | --- |
| Database | `data/devices.sqlite`, schema 28 |
| Identities / source records / sources | 4,083 / 5,298 / 278 |
| Imported PDF archive | 55 PDFs / 3,649 pages |
| Owned validation | 10/10: nine documents plus the reviewed 7760 label |
| ByteDance B4020/B4010 examples | 0/2 qualifying affiliation hits; separate gaps |
| Manufacturer inventory | 2,142 target/identity pairs / 2,141 identities; unchanged |
| All associations | 4,221 pairs / 4,083 identities |
| Raw Inspur support feed | 1,064 rows; no automatic device assertions |
| Clean rebuild | 116 jobs / 2,171 hashed inputs; 43 tables/indexes match |
| Tests | 23 affected / 411 full Python / 6 report tests pass |

Live database SHA-256:
`ea7d121849c34cac801c178d48be80ea5f3af09958b526228b6b6f536d0662d8`.

Latest additions: **19 identities / 25 records**: eight Inspur NIC models in ten
literal table rows, eight Wiwynn identities and three Hyve identities. Source roles
remain compatibility-table vendor, brand/platform, designer or dated parent
portfolio. Manufacturer/factory counts and 22 hyperscaler assertions are unchanged.
Use all associations to see these additions: Inspur 141, Wiwynn 23, Hyve 33 and
TD SYNNEX 5 associated identities. These are not manufacturer counts.

The V/V+ listing matcher now keeps plus suffixes distinct, including phrases;
unknown variants receive no borrowed specifications. Source conflicts, three
blank NIC remarks and literal JBOF voltage remain explicit. Read
[Inspur NIC evidence](research/inspur-nics-2026-10-04.md) and
[Wiwynn/Hyve evidence](research/wiwynn-hyve-2026-10-04.md).

## Proof and recovery

- Current input manifest: `data/import-manifest.json`.
- Complete PDF bundle: `data/raw/reference-documents-with-nics-wiwynn-hyve-2026-10-04.json`.
- Clean rebuild: `data/rebuilt-with-nics-wiwynn-hyve-2026-10-04.sqlite`.
- Previous live backup: `data/devices-before-nics-wiwynn-hyve-2026-10-04.sqlite`.
- Batch driver, accepted hashes, code/test logs and independent review:
  `research/batches/nics-wiwynn-hyve-2026-10-04/`.
- Final audit: `data/exports/nics-wiwynn-hyve-2026-10-04-final-verification.json`.
- Preservation, promotion, CLI/DOM and verification reports use
  `data/exports/nics-wiwynn-hyve-2026-10-04-*`; clean comparison uses
  `data/exports/nics-wiwynn-hyve-rebuild-2026-10-04-*`.
- HTML checks use the DOM harness; no browser screenshot is claimed. Rebuilds
  and candidates remain historical/staged copies, not competing live catalogs.

## Next bounded work

1. Review buying constraints for the original ten owned models, starting with
   saved evidence: assign the six networking identifiers to one worker and four
   Oracle identifiers to another. Identify sourced power/chassis dependencies,
   firmware/boot/licensing facts and explicit unknowns; identity hits alone do
   not establish operability of a liquidated unit.
2. Keep B4020/B4010 affiliation gaps and expanded vendor coverage active. Use
   finite, concrete primary-source leads; do not retry recorded denied items or
   infer ByteDance from the separate Accton applicant record.
3. Reuse existing batch tools and simplify repeated integration work when useful.
   Keep research, engineering and review paths disjoint; only one integrator
   changes shared state. Finish with a concise, independently checked handoff.

Complete historical vendor coverage, the full FCC archive and current marketplace
stock remain unproven. The 190,198 FCC-ID sitemap candidates are discovery URLs.
Read [FCC source coverage](research/fcc-source-coverage.md), relevant source notes
and saved fetch reports before collection. Hyve Ambient Series-E's 403 remains
unavailable; no new acquisition was needed for this batch. No new 7760 label is needed.

Commands and bundled runtime are in [README.md](README.md). Older handoffs are
preserved in [history](docs/history/README.md). Update this checkpoint in place;
keep source details in research notes and avoid duplicating evolving totals.
