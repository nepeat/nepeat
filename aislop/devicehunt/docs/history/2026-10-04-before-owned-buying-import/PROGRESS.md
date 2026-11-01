# Current handoff

Updated 2026-10-04. Scope: [GOAL.md](GOAL.md). Rules: [AGENTS.md](AGENTS.md).
Use the [subagent workflow](docs/agent-workflow.md); read historical notes only
for a specific question. The coordinator owns current documentation updates.

## Current work

The app goal is active with the current scope. Sol 6.1 workers completed the ten
owned-device saved-evidence review and the read-only `buying-report` command.
Independent review accepted code, evidence, reports and documentation after fixing
a NIST qualifier and a SQL example. The coordinator completed milestone acceptance.
[Assignments and proof](research/owned-buying-batch-2026-10-04.md).

[Ten buying reports](data/exports/owned-buying-2026-10-04.json) expose existing
exact-record statements across all company roles, with explicit mapped-field gaps.
No model-family specifications or current boot/license guarantees are inferred.
The existing 7760 photo's power marking and Oracle BIOS rows are newly reviewed
but **not yet imported**; X7 VPAT details additionally lack original-URL provenance.
See [networking](research/owned-networking-buying-2026-10-04.md),
[Oracle](research/owned-oracle-buying-2026-10-04.md) and
[command usage](docs/buying-reports.md).

The previous NIC/Wiwynn/Hyve batch is imported and verified. All 98 saved lookups
and current CSV/JSON/HTML/FCC/hyperscaler exports reflect that database checkpoint.
[Previous batch proof](research/nics-wiwynn-hyve-batch-2026-10-04.md).
No database import or network acquisition occurred during the buying review;
the database hash remains unchanged. Current reports do not resolve all buying gaps.

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
| Tests | 19 buying/spec affected / 417 full Python pass; HTML unchanged (prior 6 report tests) |

Live database SHA-256:
`ea7d121849c34cac801c178d48be80ea5f3af09958b526228b6b6f536d0662d8`.

Last data additions: **19 identities / 25 records**: eight Inspur NIC models in ten
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

- Buying code/evidence review, 417-test log, unchanged-database audit and both
  validations: `research/batches/owned-buying-2026-10-04/`.
- Reproduce buying reports with that directory's `generate_reports.py --output
  <new-path>` using the bundled Python. Existing output snapshots are preserved.
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

1. Curate the reviewed 7760 input marking and Oracle BIOS rows into reproducible
   source-scoped inputs, preserving certification rows and photographed-unit
   scope. Review before one integrator imports/rebuilds; update buying field
   mapping as needed. Keep seller claims separate. Recover X7 VPAT URL metadata
   before considering that capture import-ready.
2. Fill buying gaps with at most one exact-model installation/service source per
   unresolved model, starting from the concrete leads in the two research notes.
   Do not transfer nearby model specs or retry recorded denied items. A document
   identity hit is not proof of operability or transferable software entitlement.
3. Keep B4020/B4010 affiliation gaps and all thirteen vendor groups active in the
   following finite collection batch; do not infer ByteDance from Accton's record.
   Reuse existing tools, exclusive integration and concise independent handoffs.

Complete historical vendor coverage, the full FCC archive and current marketplace
stock remain unproven. The 190,198 FCC-ID sitemap candidates are discovery URLs.
Read [FCC source coverage](research/fcc-source-coverage.md), relevant source notes
and saved fetch reports before collection. Hyve Ambient Series-E's 403 remains
unavailable; no new acquisition was needed for this batch. No new 7760 label is needed.

Commands and bundled runtime are in [README.md](README.md). Older handoffs are
preserved in [history](docs/history/README.md). Update this checkpoint in place;
keep source details in research notes and avoid duplicating evolving totals.
