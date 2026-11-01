# fcc_shenanigans — local continuation instructions

This is the user's standalone device-hunting workspace, copied from the earlier
ChatGPT project on 2026-09-27. Start with `PROGRESS.md`, then the relevant module,
source-specific research note and tests. `README.md` and `GOAL.md` also retain
historical milestones; prefer the current handoff and live database for totals.

## User intent and working style

Build a local SQL-queryable catalog of devices associated with Accton, Edge-Core,
Celestica, Oracle and Amazon Data Services, especially surplus hyperscaler gear.
Keep the code, data and research together here. The user prefers free/public and
open-source collection and explicitly discarded the Datadory paid-data path.
Do not resume vendor quote/sample outreach. Communicate in lowercase, warmly and
plainly; be resourceful and act on authorized local work without repeated questions.
Do not contact sellers/providers, buy hardware, publish data or push this repository
without the user's authorization. Being inside a git checkout does not authorize a push.

## Preserve the evidence

- Treat `sources/` as read-only synced reference material, even in this copy.
- Keep original raw captures unchanged. Save new versions separately and update
  reviewed manifests and SHA-256 hashes when intentionally changing imported inputs.
- Preserve full model prefixes, suffixes, revisions, case-as-printed and source URLs.
  Do not collapse similar models or equate regulatory codes with commercial SKUs
  without explicit source evidence. Preserve conflicting assertions independently.
- Brand, OEM/manufacturer, applicant, grantee, listing holder, factory, platform
  vendor and operator are separate roles. An exact company-name association is
  not proof of manufacture. Amazon consumer hardware is not automatically ADS.
- Hyperscaler design lineage, deployment plans, dated platform deployment and the
  provenance of a particular eBay unit are different claims. Owner-reported AWS
  history is context, not scraped proof. Marketplace search links are only leads.
- The ten owned identifiers are validation expectations, not importable evidence.
  Nine have document snapshots; the 7760 full ordering code uses a reviewed label
  photo. Keep this 9+1 distinction visible. Never count seller text alone as proof.
- Raw sitemap URLs, identifiers, PCI IDs and MAC OUIs are discovery/reference rows;
  do not turn them into device, application or manufacturer assertions automatically.
- Recorded HTTP 403/402 failures and explicit access restrictions are not bypass
  invitations. Inspect saved fetch reports before collection; do not repeatedly
  retry known failures or use alternate routes to retrieve the same denied item.
  Preserve unavailable versus empty results. Stop on throttling and checkpoint work.

## Code and verification

Python standard library + SQLite/FTS5 supports normal use. Only rebuilding the
NTRA PDF imports needs optional `pypdf` and `pdfplumber`. Historical (macOS-era)
note; use the project runtime named in README.
Run from this directory; `PYTHONPATH=.` is needed for scripts in `scripts/`.
Do not install packages or alter system/Xcode configuration just to run this project.

- Search, lookup, reports and exports should remain read-only. No network in tests
  or an offline rebuild. A missing database should fail rather than create an empty one.
- Importers replace only their source in a transaction. Validate all capture inputs
  before replacement and retain original rows, raw units/dates, duplicates and gaps.
- Schema changes belong in `devicehunt/catalog.py`, rebuild count validation,
  relevant source import routing and tests. Current schema is 27.
- Run meaningful affected tests, then the suite for schema/importer changes.
  Rebuild into a NEW path from `data/import-manifest.json`; do not overwrite the
  populated database or historical snapshots to save space without user agreement.
- Compare the clean rebuild using `scripts/compare_rebuild.py`; check baseline,
  foreign keys/integrity, source counts and data preservation. Refresh affected
  exports and update PROGRESS with evidence, limits and exact next steps.
- `data/devices.sqlite` is the authoritative current database. Other
  `data/rebuilt*.sqlite` files are historical validation outputs, not competing
  current catalogs. Some old schemas cannot run today's commands without upgrade.
- Keep generated/raw datasets out of an accidental git publication. They are local
  copies with upstream rights and may contain public regulatory contact information.

## Continuation discipline

Finish bounded milestones and report actual coverage, rather than indefinitely
chasing "all devices." Do not call this a complete FCC archive or complete historical
manufacturer inventory. The next useful work is in PROGRESS, including known
coverage gaps and commands. The user requested this handoff so another agent can
continue; do not restart an unbounded crawl automatically on opening the folder.
