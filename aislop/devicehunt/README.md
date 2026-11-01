# devicehunt

hunt liquidated enterprise, networking and server hardware with a local SQLite
catalog and offline search tools. exact model/order codes and sourced specifications
help identify surplus equipment; hyperscaler connections are optional context.

## start here

| need | read |
| --- | --- |
| current objective and scope | [GOAL.md](GOAL.md) |
| verified catalog state, unfinished work and next steps | [PROGRESS.md](PROGRESS.md) |
| project rules and evidence boundaries | [AGENTS.md](AGENTS.md) |
| research, development and verification workflow | [agent workflow](docs/agent-workflow.md) |

`data/devices.sqlite` is the authoritative catalog. `data/rebuilt*.sqlite` files
are validation outputs, not competing catalogs. selected historical snapshots
are retained in a compressed, deduplicated history archive. the authorized
2026-10-11 cleanup deleted 23 reducible files after hashing, retaining their
manifests for logical rebuild; see the
[cleanup ledger](research/batches/discovery-2026-10-11/integration/cleanup-2026-10-11.json).
see
[evidence archive and storage](docs/archive-storage.md) for registration, reviewed
evidence lookup, offline reuse and exact historical restoration. current totals
belong in PROGRESS; use `stats` for the database you actually opened.

## search the populated catalog

run commands from this directory. normal use needs Python 3.9+ with SQLite/FTS5
and the standard library. the workspace lives on Linux at
`/home/erin/nepeat/aislop/devicehunt` (renamed from `fcc_shenanigans` on 2026-10-10)
and uses the project's direnv `python3` (3.14 when last verified). do not install
packages or change system configuration just to use the catalog. the acquisition
registry is bound to this absolute path. to move the whole workspace, stop all
campaign/capture work, `mv` the directory, then run from the destination:

```sh
PYTHONPATH=. python3 scripts/relocate_archive.py --from-root OLD --quiesced --decision 'why'
PYTHONPATH=. python3 scripts/relocate_catalog.py --from-root OLD --audit data/exports/relocation-audit-DATE.json
```

`--audit` must name a new workspace-relative file in an existing directory. a copy is
never relocated: the old root must no longer exist, and no copy of the old tree should be kept.

```sh
python3 -m devicehunt stats
python3 -m devicehunt search 7760 --vendor Edge-Core
python3 -m devicehunt lookup '7760-32X-A-AC-F' --format json
python3 -m devicehunt vendor-inventory Accton --scope manufacturer --limit -1
python3 -m devicehunt vendor-inventory Inspur --scope all --limit -1
python3 -m devicehunt specs R4023-F9102-A0 --exact-part-number
python3 -m devicehunt buying-report E5-2C --format json
python3 -m devicehunt documents X680m
python3 -m devicehunt evidence Golduck
python3 -m devicehunt match-listing --title 'Celestica R4023-F9102-A0 empty chassis' --with-specs
```

the listing title is illustrative. `search` finds catalog entries; `lookup` also
searches raw discovery/reference datasets without merging their evidence. `specs`
keeps each statement attached to its source record. retain full prefixes, suffixes,
revisions and case-as-printed when comparing labels or ordering codes.

`specs --include-source-facts` (opt-in) also lists reviewed practical source facts (PSU quantity/rating, NetBox airflow, U height, weight, form factor, node counts) beside the curated statements, labelled `origin=source fact` with the fact name, source and tier; software-reference counts (SONiC/PDDF/Stratum/entity-manager) are excluded. see [specification search](docs/specification-search.md).

`lookup`'s catalog rows end with `matched_in`: where the query was found (identity fields,
fact names — never values). a part number, PSU or FRU string found only in facts is a
discovery lead, not fitment evidence. software-reference identities (brands such as
`SONiC platform`, `ONIE machine`, `OpenBMC entity-manager config`, `Meta OpenBMC platform`,
`Linux BMC device tree`, `coreboot mainboard`, `Stratum platform`) list strings their files
contain; certification, benchmark and community listings (Ubuntu / Red Hat certified,
SPEC results, NetBox device types, linuxhw DMI dumps) describe one tested or listed
configuration and carry non-manufacturer roles only. see the
[discovery batch note](research/discovery-2026-10-11.md).

use `show DEVICE_ID` for full evidence or `shopping DEVICE_ID` for search links,
after the `python3 -m devicehunt` prefix. search terms retain the exact model,
stored title and explicit identifiers; software-reference entries have no hardware
shopping links. the CLI and newly generated HTML catalogs share these rules.
links do not establish stock, price or unit provenance. use `--help` after any
subcommand for its filters and output formats. `--db PATH` goes before the
subcommand; a missing database fails.

open [the offline HTML catalog](data/exports/catalog.html) in a browser. it starts
with manufacturer claims; choose **all associations** to include brand, platform,
applicant and other roles. it is a generated snapshot; external links need internet.

| task | guide |
| --- | --- |
| search model fragments | [matching rules](docs/model-fragment-search.md) |
| match auction titles or a local CSV | [listing matching](docs/listing-matching.md) |
| distinguish manufacturer, factory and other associations | [vendor inventory](docs/vendor-inventory.md) |
| search exact-model or exact-part-number specifications | [specification search](docs/specification-search.md) |
| inspect practical statements and buying-information gaps | [buying reports](docs/buying-reports.md) |
| find parts in tested configurations | [tested components](docs/tested-components.md) |
| decode a saved Linux PCI inventory | [PCI inventory](docs/pci-inventory.md) |
| inspect deployment, design and product-family claims | [hyperscaler inventory](docs/hyperscaler-inventory.md) |
| inspect collected FCC evidence and gaps | [FCC coverage](research/fcc-collection-coverage.md) |

### model collision review leads

```sh
python3 -m devicehunt model-collisions --target Inspur --limit -1
python3 -m devicehunt model-collisions --format json
```

lists identities whose model strings collide after casefolding, collapsing whitespace and
optionally stripping one leading token equal to the identity's own brand, while the
(brand, model) pairs differ. reasons: `same_model_different_brand`, `brand_token_prefix`,
`brand_token_prefix_different_brand`. suffixes, hyphens and revision letters are never
stripped. software/firmware/BMC/device-tree/config kinds are skipped unless `--all-kinds`.
each group is flagged `short_key` (key under 6 characters) and `kinds_differ`; nothing is
dropped for them unless you pass `--min-key-length N`. `--target` keeps a group if any
member's brand resolves to the target or any member has an affiliation with that target;
all members of a kept group are shown, including ones with no such tie. read-only review
leads: nothing is merged, aliased or asserted equivalent.

### brand families (display only)

`data/brand-families.json` is a small reviewed table (Quanta / QCT / Quanta Cloud Technology, and
Oracle / Sun Microsystems) used only to group output: `lookup` catalog rows gain `brand_family`
(JSON also `family_members_hit`), `match-listing` candidates gain `brand_family` and
`same_family_candidates` (other candidates in the family), `model-collisions` groups gain `family`
(`same_family` when every member is in one family), and each of these outputs carries the table's
sha256 under `brand_families`. no ranking, row set, identity, target count or `vendor-inventory`
output changes, with or without the file. every row needs evidence (url, local capture path,
sha256), a relation type, a date and a reviewer; rows are `pending` (reviewer null) until an
independent reviewer signs. the own-brand model-prefix alias rule is `model_collisions.stripped_key`
(`brand_families.alias_key`); the generated pair list in the file is a same-brand audit list only (it includes `Inspur Corporation` / `(IEI)` pairs that imply no IEIT/Inspur relation), and no output consumes aliases (there is no "also known as" display). check the file
with `python3 -m devicehunt.brand_families check`. see
[the proposal](research/brand-family-proposal-2026-10-11.md).

## SQL and evidence

use the read-only SQL command, or open the database read-only in a SQLite client:

```sh
python3 -m devicehunt sql "SELECT brand, model, role FROM target_devices WHERE target = 'Accton' AND role = 'manufacturer'"
python3 -m devicehunt sql "SELECT * FROM device_facts WHERE name = 'asic' AND value LIKE '%Trident%'"
python3 -m devicehunt sql "SELECT * FROM device_identifiers WHERE kind = 'fcc_id'"
python3 -m devicehunt sql "SELECT name, type FROM sqlite_master WHERE type IN ('table', 'view') ORDER BY name"
```

`sources` records provenance and hashes; `records`, `affiliations`, `identifiers`
and `facts` retain source-specific assertions. `document_text` exposes captured
PDF text. inspect `affiliations.role`: `devices.brand` is an identity namespace,
not manufacturing proof. conflicting statements remain separate.

raw sitemap URLs, register rows, PCI IDs, MAC OUIs and Inspur support-menu entries
are discovery/reference data. they do not automatically establish devices,
manufacture or deployment. marketplace seller text remains seller-only evidence,
not independent identity, manufacture or deployment proof. validation expectations
must not be imported as sources. coverage is bounded, not a complete FCC archive
or historical manufacturer inventory.

## validate, rebuild and export

```sh
python3 -m devicehunt validate-inventory
python3 -m devicehunt validate-inventory --inventory validation/vendor-examples.json
```

the owned baseline is ten identifiers: **9 document snapshots + 1 reviewed label
photo** for `7760-32X-A-AC-F`. the separate vendor examples are coverage expectations,
not ownership or manufacturer claims. validation exits with status 2 for evidence
gaps; see PROGRESS for the latest verified results.

an offline rebuild uses the reviewed manifest and saved extractions; it needs
only the standard-library runtime above. optional PDF libraries are for fresh
extraction, not the current rebuild. [PDF extraction](docs/pdf-extraction.md)
documents the local Poppler utility and its review status. choose a destination that does not exist;
preserve the populated catalog and byte-restorable historical snapshots:

```sh
python3 -m devicehunt --db data/rebuilt-next.sqlite rebuild --manifest data/import-manifest.json
python3 -m devicehunt --db data/rebuilt-next.sqlite validate-inventory
PYTHONPATH=. python3 scripts/compare_rebuild.py data/rebuilt-next.sqlite --output-prefix rebuilt-next
```

the comparison checks tables/indexes and the owned baseline against
`data/devices.sqlite`, writing audit files under `data/exports/`. follow the
[agent workflow](docs/agent-workflow.md) for affected tests and promotion checks.
rebuilding does not authorize replacing the current catalog.

these commands read the database and write export files:

```sh
python3 -m devicehunt export data/exports/targets.csv
python3 -m devicehunt export data/exports/targets.json --format json
python3 -m devicehunt report data/exports/catalog.html
```

## offline collection campaign

the pinned RRA/ISED pilot replays saved inputs through `data/archive/` with no
network path. `plan`, `status` and `report` are read-only; `run` and `resume`
write archive receipts and operational checkpoints, never the catalog:

```sh
python3 -m devicehunt.campaign plan
python3 -m devicehunt.campaign run --offline
python3 -m devicehunt.campaign status
python3 -m devicehunt.campaign report
python3 -m devicehunt.campaign resume RUN_ID --decision 'why this resume is safe'
```

`python3 -m devicehunt.coverage freeze RUN_ID --output DIR` freezes one finished run
into a claim-free, content-addressed coverage manifest; `validate PATH` re-checks it
against the archive's receipts and bytes. a repeated run reuses completed jobs and adds no new bytes. reports separate
validated, explicit-zero, failed and pending units; saved query snapshots are
not regulator-wide coverage. candidate bundles are not accepted catalog evidence.
see the [campaign plan](docs/campaign-plan.md) and
[state contract](docs/campaign-state-contract.md) before extending it.

a supervised **live** RRA refresh (c4) is coordinator-run only, through
`research/batches/hunt-2026-10-10/c4/live_rra.py` (`smoke`, `refresh UNIT --cycle C`, `settle`, `rebundle`,
`reconcile`): each phase allocates and activates its own finite account and never retries. it needs explicit user
authorization for each new scope; the first refresh and approved cycle 2 are
already complete. a new cycle or recurring schedule is a new scope; see PROGRESS
and the [c4 notes](research/batches/hunt-2026-10-10/c4/c4-notes.md).
after a reboot or suspend, a timed account refuses with "clock/boot anomaly requires explicit reconciliation";
`live_rra.py reconcile ACCOUNT --label L --decision WHY` (no HTTP) re-baselines it so its remaining time only
shrinks ([S4 handoff](research/batches/ops-2026-10-10/s4/handoff.md); the API is tested, the driver phase
not yet exercised live). a throttle with the same anomaly uses
`CampaignState.reconcile_throttle(event_id, decision=...)`.

### reviewing campaign results before import (c3b gate)

a reviewer accepts or rejects each unit's **whole** projection (everything one source
contributes to the catalog). unaccepted units keep their previously accepted inputs. every
file is immutable and content-addressed:

```sh
python3 -m devicehunt.review_queue queue COVERAGE.json --output data/campaign-reviews/COVERAGE_ID
python3 -m devicehunt.review_queue show data/campaign-reviews/COVERAGE_ID/items/ITEM.json
python3 -m devicehunt.review_queue decide ITEM.json accepted|rejected --reviewer NAME --reason WHY \
  --queue data/campaign-reviews/COVERAGE_ID
python3 -m devicehunt.review_queue release data/campaign-reviews/COVERAGE_ID --coverage-manifest COVERAGE.json
```

`data/import-manifest.json` declares `campaign_gate` (the release plus its coverage
manifest, both hashed). rebuild refuses gated jobs whose inputs differ from the release,
and refuses any gated source whose imported projection differs from the reviewed one. build the
queue against the gated accepted manifest: each unit's baseline job comes from the accepted
release, so a refreshed unit whose bundle moved to a new path keeps its old job unless accepted,
and the release then forbids importing the unaccepted new path (`candidate_job`). `check-release PATH`
re-validates a release. the reviewer must not be the author of the change under review; this is a
procedural rule, not enforced by the CLI. the campaign's default definition is now the SUN-6000
re-pin (`research/batches/discovery-2026-10-11/sun6000-a/inventory-r2/`; before it the c3b re-pin).

## project files and history

keep code and `data/` together when copying this workspace. `data/import-manifest.json`
tracks reviewed rebuild inputs; `data/raw/` holds original captures; `research/`
holds source reviews and limitations; `validation/` holds expectations. treat
`sources/` as read-only synced material. retain originals when collecting new versions.

collection/import commands change data; consult AGENTS and the workflow before
using them. respect saved access failures, and do not contact sellers/providers,
publish data or push this checkout without authorization.

[the archived README](docs/history/2026-10-04-before-workflow-simplification/README.md)
preserves earlier batch narratives and source-specific examples. its original
relative links were rooted at the project directory; its totals are historical.
