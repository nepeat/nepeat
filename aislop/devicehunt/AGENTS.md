# devicehunt — continuation instructions

Start with [GOAL.md](GOAL.md) and [PROGRESS.md](PROGRESS.md), then only the relevant
module, source note and tests. [README.md](README.md) covers normal use. Read
[the workflow](docs/agent-workflow.md) before delegating and
[the evidence policy](docs/evidence-policy.md) before collection or import.
Historical snapshots under `docs/history/` are references, not current instructions
or status; do not load the entire archive or conversation into every worker.

## Working agreement

Hunt liquidated enterprise, networking and server hardware across the targets in
GOAL.md. Hyperscaler connections are optional. Keep all code, data and research
in this workspace. Communicate in lowercase, warmly and plainly. Act on authorized
local work without repeated permission questions.

Use free/public and open-source collection. The paid Datadory path was explicitly
rejected. Do not contact sellers/providers, buy hardware, publish data or push
without the user's authorization. Do not install packages or change system/Xcode
configuration just to run the project. Use the project runtime named in README.

## Subagents and ownership

The user approved this architecture on 2026-10-04. Delegate bounded independent
research, coding and review when useful; keep prioritization and acceptance in
the coordinating chat. Assign each worker explicit inputs, writable paths,
acceptance checks and a stop condition. Further delegation needs the coordinator's
assignment. Use fresh, small briefs and artifact links; summarize results briefly.
Use GPT-6.1 Sol for ChatGPT subagents. On Claude, use the latest available Sonnet
(user's 2026-10-04 baseline: Sonnet 5.5); verify availability on that host.
Do not silently substitute another worker model if the requested one is unavailable.

Only one designated integrator may update the import manifest, complete PDF
bundle, authoritative database and generated exports for a batch. Other workers
write disjoint assigned paths and use read-only data access. Review inputs before
integration; never run concurrent imports or export refreshes against shared state.
Use the same stages sequentially when workers are unavailable. Detailed contracts
and handoff templates are in docs/agent-workflow.md.

## Evidence and data safety

- Treat `sources/` as read-only and keep original captures immutable. Save new
  versions separately; record source URLs, capture outcomes and SHA-256 hashes.
- Preserve exact identifiers, suffixes, revisions, case-as-printed and conflicting
  statements. Company roles, chassis/node identities and source claims stay separate.
- Raw discovery rows do not become device/manufacturer/deployment claims. The
  owned baseline and ByteDance examples are expectations, not importable evidence.
- Read source-specific failures before collection. Do not repeatedly retry 402/403,
  bypass restrictions or retrieve the same denied item through another route.
  Preserve unavailable versus empty; stop on throttling and checkpoint work.
- Keep raw/generated datasets out of accidental publication. They retain upstream
  rights and may include public regulatory contact information.

## Code and verification

`data/devices.sqlite` is authoritative. Other rebuilt/candidate/backup databases
are historical or staged outputs. Current schema/counts and proof paths belong
in PROGRESS.md. Search, lookup, reports and exports must leave the database
unchanged; a missing database must fail rather than create an empty one.

- No network in tests or an offline rebuild. Importers validate all capture inputs
  before replacing only their source in a transaction; preserve duplicates,
  original rows, raw units/dates and gaps.
- Reference-PDF inputs must be sibling filenames beside the COMPLETE collection
  bundle. Use byte-identical copies as needed; never weaken path validation or
  replace the full collection with a partial bundle.
- Schema changes go in `devicehunt/catalog.py`, rebuild expected counts, relevant
  importer routing and tests. Run meaningful affected tests; run the full suite
  for schema/importer changes and before promoting a combined code/data batch.
- Rebuild into a NEW path from `data/import-manifest.json`; compare with
  `scripts/compare_rebuild.py`. Check old-data preservation, source counts,
  baseline, integrity/foreign keys and FTS. Refresh affected exports after acceptance.
- Do not overwrite historical snapshots or delete data to save space without user
  agreement. Historical session helpers are examples, not safe commands to batch-run.

## Keep the handoff small and current

Documentation upkeep is an ongoing project goal. At each milestone/interruption,
name one documentation editor; update PROGRESS with verified state, pending work,
limits and exact next steps. Keep GOAL stable, README practical and workflow rules
current. Move superseded narratives to dated archives with provenance; retain
source details in research notes and link instead of duplicating them. Have an
independent worker check links, evidence boundaries and stale claims when available.
Finish bounded milestones; do not indefinitely crawl every discovery URL or claim
complete historical coverage. Preserve original-vendor work alongside new targets.
