# fcc_shenanigans — local continuation instructions

This is the user's standalone device-hunting workspace, copied from the earlier
ChatGPT project on 2026-09-27. Start with `PROGRESS.md`, then the relevant module,
source-specific research note and tests. `README.md` and `GOAL.md` also retain
historical milestones; prefer the current handoff and live database for totals.

## User intent and working style

Build a local SQL-queryable catalog and search tools to hunt liquidated enterprise,
networking and server hardware on eBay and other marketplaces. Prioritize exact
model/order codes and practical identification, compatibility and operability facts.
Hyperscaler connections are optional provenance context, not an inclusion requirement.
Current vendor targets are Accton, Edge-Core,
Celestica, Oracle, Amazon Data Services, Inventec, Hyve, Foxconn, Wiwynn, TD SYNNEX /
SYNNEX, Quanta, ByteDance, Inspur and additional relevant enterprise hardware vendors
and ODMs identified through source evidence. The user revised this goal on
2026-10-04; GOAL.md defines the current objective and next bounded milestone.
New scope does not automatically establish legal-name aliases, manufacturer roles
or hyperscaler relationships. Review TD SYNNEX/SYNNEX and other parent, subsidiary
or product-brand relationships with dated evidence before connecting records.
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
- ByteDance and Inspur were added on 2026-10-04. The B4020/B4010 examples live in
  `validation/vendor-examples.json`, separately from the ten owned devices.
  They are coverage expectations; ownership and ByteDance manufacturing are not
  asserted. A matching Accton applicant model alone does not validate ByteDance.
- Raw sitemap URLs, identifiers, PCI IDs and MAC OUIs are discovery/reference rows;
  do not turn them into device, application or manufacturer assertions automatically.
- The Inspur support feed is also raw discovery data (`inspur_support_entries`).
  It mixes systems, third-party parts, software and category labels; preserve full
  names, duplicate names, nullable metadata and parent IDs. Neither a leaf flag
  nor a model-looking string establishes an Inspur brand/manufacturer assertion.
- The 2018 Inspur catalog retains grouped `FS6500/6600` and
  `AS13000-M1/M3/M5/Rack` names; do not expand their suffixes into inferred SKUs.
  NF5288M5/AGX-2 is an explicit source-scoped alias. M13 memory, NF5288M5 PSU and
  X680m memory conflicts remain unresolved source statements. The SR-AI/Baidu
  joint-design assertion does not establish deployment or transfer to separate nodes.
- Preserve explicit chassis/node and appliance/server-node relationships as
  separate identities. In particular, i24 is not an alias for NS5162M5, and the
  InCloud Rail appliance column is not an alias list for its server-node column.
  Compatibility-table vendor names and historical firmware recommendations do
  not establish physical manufacture, current support or operator deployment.
- Recorded HTTP 403/402 failures and explicit access restrictions are not bypass
  invitations. Inspect saved fetch reports before collection; do not repeatedly
  retry known failures or use alternate routes to retrieve the same denied item.
  Preserve unavailable versus empty results. Stop on throttling and checkpoint work.

## Code and verification

Python standard library + SQLite/FTS5 supports normal use. Only rebuilding the
NTRA PDF imports needs optional `pypdf` and `pdfplumber`. See PROGRESS for the
working bundled Python path if macOS system Python is blocked by the Xcode license.
Run from this directory; `PYTHONPATH=.` is needed for scripts in `scripts/`.
Do not install packages or alter system/Xcode configuration just to run this project.

- Search, lookup, reports and exports should remain read-only. No network in tests
  or an offline rebuild. A missing database should fail rather than create an empty one.
- Reference-PDF dependencies must be sibling filenames beside the complete bundle.
  Use byte-identical local copies when needed; do not weaken dependency-path validation.
- Importers replace only their source in a transaction. Validate all capture inputs
  before replacement and retain original rows, raw units/dates, duplicates and gaps.
- Schema changes belong in `devicehunt/catalog.py`, rebuild count validation,
  relevant source import routing and tests. Current schema is 28.
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
coverage gaps and commands. The user explicitly resumed research on 2026-10-04.
Proceed with the expanded goal in bounded collection/import milestones; do not
launch an indiscriminate crawl of every sitemap URL. Keep the original-vendor
coverage work and ten-device validation baseline active alongside new hardware
vendors and ODMs. Prioritize useful liquidation identification and specifications;
do not restrict collection to devices with a documented hyperscaler connection.
