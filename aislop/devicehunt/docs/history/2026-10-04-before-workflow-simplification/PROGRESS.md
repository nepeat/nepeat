# device hunting — active continuation

## Workflow review and unfinished batch — 2026-10-04

The user requested a review of splitting research, development and verification
among subagents. [Review, proposed ownership and pending-batch checkpoint](research/workflow-review-2026-10-04.md).
The live database was rechecked and still matches the verified enterprise batch
below. NIC/Wiwynn/Hyve captures are not imported. A plus-suffix listing fix has
passed 22 affected tests; full-suite validation remains pending for that change.
No subagents were launched during the review; the goal tool currently reports paused.

## Scope revised: liquidation hunting — 2026-10-04

The user's current goal is to hunt **liquidated enterprise, networking and server
hardware** on eBay and other marketplaces. Hyperscaler connections remain useful
provenance context, but are not an inclusion requirement. Keep all thirteen vendor
groups, the ten owned-device baseline and the separate ByteDance B4020/B4010
coverage examples. Prioritize exact identifiers and practical specifications for
surplus servers, switches, storage, appliances and related hardware. GOAL.md,
AGENTS.md, README.md and the active thread goal now reflect this scope.

## Enterprise hardware batch verified — 2026-10-04

Current live catalog: **4,064 identities / 5,273 source records / 263 sources**,
**schema 28**. This batch adds **88 identities (86 Inspur + 2 Inventec)** and
**93 source records** from the Inspur 2018 catalog and Inventec Artemis II Rack
and X680m brochures. All prior identities, assertions, raw rows and display
metadata passed preservation checks.

Inspur now has **133 associated identities / 3 manufacturer-backed identities**;
Inventec has **81 / 44**. Manufacturer inventory remains **2,142 target/identity
pairs / 2,141 identities**; all associations have **4,199 pairs / 4,064 identities**.
Use `--scope all` (or all associations in the HTML catalog) for the new brand/platform
records. No new manufacturer, factory or FCC-ID assertion was added.

The owned baseline stays **10/10 (9 documents + 1 label photo)**. ByteDance
B4020/B4010 remain **0/2 explicit evidence gaps**. Raw Inspur support-menu rows
remain 1,064, with no automatic conversion to device claims.

The new facts include Open19 power-shelf requirements, rack bus-bar/12V power,
separate chassis/node and GPU-box identities, Fibre Channel switch specifications,
storage systems and physical enterprise appliances. Exact suffixes and grouped
spellings are retained. `NF5288M5` explicitly pairs with `AGX-2` in its source;
this is the only new alias. M13 memory, NF5288M5 PSU and X680m memory conflicts
are recorded with page references. Software-only names and unnamed modules stay
outside the curated hardware inventory.

One new source assertion describes **SR-AI rack joint development with Baidu**.
It is design lineage, not deployment or auction-unit provenance; it is not inherited
by separate nodes or enclosures. Hyperscaler evidence now has 22 total assertions;
this optional context does not limit the broader enterprise liquidation scope.
See [source review, exact-model queries and SQL](research/inspur-2018-inventec-2026-10-04.md).

**408 Python tests + 6 report tests pass**. A clean **114-job offline rebuild from
2,146 hashed inputs** matches all **43 tables/FTS indexes** and the baseline.
The full archive has **50 PDFs / 3,641 pages**, including the three new originals
and their 54 pages. All 98 saved lookup reports and current CSV/JSON/HTML, FCC and
hyperscaler exports were refreshed. Actual CLI checks cover vendor inventories,
exact-model and alias specifications, PDF search, both validation sets, and a
clearly illustrative auction title with source-scoped specifications. The generated
HTML payload and matching script pass the DOM harness; no browser screenshot
is claimed and the previous file:// restriction remains respected.

Current database SHA-256:
`935d8774acac42b1906f1e30d77b127a7d40534123d72cb1cd916a3a84be8dcb`.
Clean rebuild: `data/rebuilt-with-inspur-2018-inventec-2026-10-04.sqlite`.
Previous live backup: `data/devices-before-inspur-2018-inventec-2026-10-04.sqlite`.
Curated input: `data/inspur-2018-inventec-facts-2026-10-04.json`.
Complete PDF bundle: `data/raw/reference-documents-with-inspur-2018-inventec-2026-10-04-v2.json`.
Manifest backup: `data/import-manifest-before-inspur-2018-inventec-2026-10-04.json`.

Original captures remain under `data/raw/inspur-2018-inventec-2026-10-04/` with
request reports and hashes. The first nested-path PDF bundle was rejected by the
sibling-filename validation before import; v2 uses byte-identical sibling copies.
The rejected v1 bundle and untouched first candidate are historical artifacts.
Do not use them as the current catalog or weaken the path validation. The complete
v2 bundle replaces the entire reference collection; never substitute a partial one.

Proof under `data/exports/`: `inspur-2018-inventec-review-2026-10-04.json`,
`inspur-2018-inventec-rebuild-2026-10-04-comparison.json`,
`inspur-2018-inventec-promotion-2026-10-04.json`,
`inspur-2018-inventec-cli-checks-2026-10-04.json`,
`inspur-2018-inventec-ui-audit-2026-10-04.json`,
`inspur-2018-inventec-final-verification-2026-10-04.json`, plus dated logs.
The seven batch helpers are archived under `research/session-tools/`; read their
guards and historical context rather than rerunning mutations.

Queries (prefix with `python3 -m devicehunt`, using the bundled runtime below):

- `vendor-inventory Inspur --scope all --limit -1`
- `specs OB5162M5 --vendor Inspur --exact-model`
- `specs SN3116J4 --vendor Inspur --exact-model`
- `specs AGX-2 --vendor Inspur --exact-model`
- `specs 1020E --vendor Inspur --exact-model`
- `documents X680m`
- `match-listing --title 'Inspur OB5162M5 bare node untested' --vendor Inspur --with-specs`

The last command uses an illustrative title, not a captured marketplace listing.

Next bounded work:

1. Review the uncurated merged-cell NIC tables in the already captured InCloud Rail
   PDF. Retain full card identifiers and explicit table vendor roles; compatibility
   does not establish manufacture. Additional detailed 2018 specifications remain
   full-text-only and may be reviewed when useful to an exact hardware search.
2. Broaden older Hyve/Wiwynn systems and original-vendor enterprise hardware sources
   alongside the Inspur work. Prioritize exact order codes, dependent chassis/power
   parts, firmware access and licensing facts useful for evaluating liquidation lots.
3. Continue primary ByteDance B4020/B4010 affiliation evidence. Preserve the separate
   Accton applicant statement and recorded 402/403/gated-source failures. No paid
   trade-data acquisition or seller/provider outreach is authorized.

Complete historical coverage and current marketplace stock remain unproven. The
full goal stays active. Older milestones below retain their original snapshot counts.

## Inspur hardware tables verified — 2026-10-04

Current live catalog: **3,976 identities / 5,180 source records / 257 sources**,
**schema 28**. All thirteen target groups remain enabled. This batch adds **34
Inspur identities / 37 source records** and one original 64-page PDF. All prior
identities, assertions, raw rows and display metadata are preserved.

Inspur now has **47 associated identities / 3 manufacturer-backed identities**.
The additions include nine separately named appliance/server-node pairs, four
SSDs, five storage controllers, five switches, two FPGA cards and the NS5162M5
compute node. NF5280M5 was already present. The existing i24 chassis gains its
explicit relationship to NS5162M5; they remain separate identities. NE5260M5
also gains a 2019 launch source with its 430 mm chassis depth. The new compatibility
list has a December 2023 cover and March 2024 PDF metadata; these are not release
dates or current-support guarantees.

Manufacturer inventory remains **2,142 target/identity pairs / 2,141 identities**;
all associations now have **4,111 pairs / 3,976 identities**. Inventec stays at
79 associated / 44 manufacturer-backed identities. No manufacturer, factory,
FCC-ID or hyperscaler assertion was added. ByteDance B4020/B4010 remain **0/2
explicit evidence gaps**. The owned baseline remains **10/10 (9 documents +
1 label photo)**. The 1,064 raw Inspur support-menu entries remain unchanged.

The original InCloud Rail PDF is archived and searchable in full. Its reviewed
tables retain exact spellings, including `S6550V2-48TQ-AC/D` as one grouped name.
Third-party components and software are not relabeled Inspur. The nine appliance
and node columns are stored as source-qualified facts, with no alias identifiers.
Seven relevant pages across this PDF and the existing i24 paper were visually
checked. See [source review and SQL](research/inspur-rail-2026-10-04.md).

Inventec download pages 2 and 3 both succeeded, with ten rows each. All three
pagination destinations are now captured, without claiming historical completeness.
The NX-H1313 row points to an NX-H1331 document title; no equivalence was inferred.
Artemis II Rack and X680m remain next product-document leads. No gated NX details
or private serial, warranty, activation or contact endpoints were accessed.

**408 Python tests + 6 report tests pass**. A clean **113-job offline rebuild
from 2,130 hashed inputs** matches all **43 tables/FTS indexes** and the baseline.
The complete archive has **47 PDFs / 3,587 pages**. Previous source roles and
all historical data rows passed preservation checks before promotion.

Current database SHA-256:
`79a824face1fea6949b930d81a1500a8bebae4a1fe734e84b8c892ad8684994f`.
Clean rebuild: `data/rebuilt-with-inspur-rail-2026-10-04.sqlite`.
Previous live backup: `data/devices-before-inspur-rail-2026-10-04.sqlite`.
Latest complete PDF collection: `data/raw/reference-documents-with-inspur-rail-2026-10-04.json`.
Manifest backup: `data/import-manifest-before-inspur-rail-2026-10-04.json`.
Never replace the complete PDF collection with a partial bundle.

Proof under `data/exports/`: `inspur-rail-review-2026-10-04.json`,
`inspur-rail-rebuild-2026-10-04-comparison.json`,
`inspur-rail-promotion-2026-10-04.json`, plus the dated test/rebuild logs.
The seven one-off fetch/build/stage/promotion/audit helpers are archived under
`research/session-tools/`. Read their guards; do not batch rerun historical mutations.

All 98 saved lookup reports and current CSV/JSON/HTML, FCC and hyperscaler
exports were refreshed. `inspur-rail-final-verification-2026-10-04.json` verifies
all 2,130 input hashes, all 47 PDF hashes, SQLite integrity/foreign keys, owned
and example validation, raw support-row equality and the 43-table comparison.
The live database hash stayed unchanged throughout exports and read-only checks.
`inspur-rail-ui-audit-2026-10-04.json` exercises the generated payload and matching
inline script: Inspur 3/47 manufacturer/all, Inventec 44/79, ByteDance zero,
and the grouped switch identity retained. No browser screenshot is claimed;
the previous file:// navigation restriction remains respected. Real CLI queries
are recorded in `inspur-rail-cli-checks-2026-10-04.json`. All 181 one-off helpers
remain archived; older scripts retain the historical states they were written for.

Queries (prefix with `python3 -m devicehunt`, using the bundled runtime below):

- `vendor-inventory Inspur --scope all --limit -1`
- `specs NS5162M5 --vendor Inspur --exact-model`
- `specs i24 --vendor Inspur --exact-model`
- `specs NE5260M5 --vendor Inspur --exact-model`
- `documents CN61108PC-V-H`
- `lookup 'i24&NS5162M5'`

The exact specification queries separate node/chassis facts; broad search can
find both because their relationship names both. The source review includes SQL
returning the nine appliance/server-node mappings.

Next bounded work:

1. Review permitted original Inventec Artemis II Rack and X680m product documents
   from the captured page 3. Keep NX-H1313/NX-H1331, P9000IG6/P9000G6 and grouped
   K905G6_G6P names separate until an explicit source resolves them.
2. Continue historical Inspur systems from the support feed. A discovered original
   2018 product catalog is a useful uncollected lead:
   `https://en.inspur.com/eportal/fileDir/lcjtww/resource/cms/article/2448179/2448543/2019032710162840541.pdf`.
   The InCloud Rail NIC tables remain full-text-only pending merged-cell/name review.
   Continue older Hyve/Wiwynn and original-vendor sources alongside this work.
3. Continue primary ByteDance B4020/B4010 affiliation evidence. The Accton applicant
   statement remains separate; paid trade-data previews remain unverified. Respect
   the existing 402/403/gated-source records and avoid retrying known failures.

All four new original requests succeeded. Complete historical coverage remains
unproven; the full goal stays active. Older milestones below are historical snapshots.

## Inspur support feed / Inventec storage verified — 2026-10-04

Current live catalog: **3,942 identities / 5,143 source records / 253 sources**,
**schema 28**. All thirteen target groups remain enabled. This batch adds one
identity (Inventec Metagross), three model assertions, six sources and **1,064
raw support entries**. E850G4 and NF5280M5 gain original specification evidence.
All previous identities, assertions, raw rows and display metadata survive.

The public Inspur download page references a model-name JSON feed. Its 1,064
rows contain **1,055 distinct printed names**, including systems, third-party
cards, software and category labels. The complete captured response is now in
`inspur_support_entries`, available to SQL and `lookup`. Exact names, duplicates,
compound names, parent IDs, nullable levels and raw JSON remain intact. The feed
creates **zero catalog identities or manufacturer claims**. A leaf flag alone is
not a hardware classification; a match is not proof of manufacture or deployment.

Inventec has **79 associated identities / 44 manufacturer-backed identities**.
Inspur stays at **13 associated / 3 manufacturer-backed identities**; its broad
support feed does not inflate those counts. Manufacturer inventory is unchanged
at 2,142 target/identity pairs / 2,141 identities. All associations now have
4,077 pairs / 3,942 identities. Hyperscaler assertions and FCC claims are unchanged.
ByteDance stays at zero; B4020/B4010 remain **0/2 explicit evidence gaps**.
The owned baseline remains **10/10 (9 documents + 1 label photo)**.

Metagross is a 2U/42-drive SATA enclosure with two 800 W supplies and
source-qualified rack/management facts. E850G4's original brief adds its 450 mm
depth and CRPS options. NF5280M5's revision 5.0 white paper adds dimensional,
memory, BMC and conditional power facts; apparent errors in other PSU lines are
flagged without correcting the original text or inferring interchangeable parts.
See [the source review, limits and queries](research/storage-inspur-2026-10-04.md).

**408 Python tests + 6 report tests pass**. A new 112-job offline rebuild from
2,120 hashed inputs matches all **43 tables/FTS indexes**, including the new raw
table, and the owned baseline. The archive has **46 PDFs / 3,523 pages**, adding
two original PDFs / 49 pages. Selected table pages were visually reviewed.

Current database SHA-256: `7efd5967042863e15e7bea02adf7255fefe43ba7cae6c0dde0b4a3a3ae923c2f`.
Clean rebuild: `data/rebuilt-with-storage-inspur-2026-10-04.sqlite`.
Previous live backup: `data/devices-before-storage-inspur-2026-10-04.sqlite` (schema 27).
Latest complete PDF collection: `data/raw/reference-documents-with-storage-inspur-2026-10-04.json`.
Manifest backup: `data/import-manifest-before-storage-inspur-2026-10-04.json`.
Do not import a partial PDF collection; it replaces all reference documents.

Proof under `data/exports/`: `storage-inspur-review-2026-10-04.json`,
`storage-inspur-rebuild-2026-10-04-comparison.json`,
`storage-inspur-promotion-2026-10-04.json`, plus the dated test/rebuild logs.
Raw importer: `devicehunt/inspur_support.py`; four new tests cover mixed/raw
rows, duplicates, nullable metadata, literal lookup, invalid replacement,
idempotency, independent sources, read-only behavior and offline count checks.
All 98 saved lookup reports and current CSV/JSON/HTML, FCC and hyperscaler exports
were refreshed. `storage-inspur-final-verification-2026-10-04.json` verifies all
2,120 input hashes, 46 PDF hashes, SQLite integrity/foreign keys, owned/example
validation, exact equality of all 1,064 raw JSON rows and the 43-table comparison.
The live database hash stayed unchanged throughout export and read-only checks.
`storage-inspur-ui-audit-2026-10-04.json` exercises the generated HTML payload
and matching inline script: Inventec manufacturer/all 44/79, Inspur 3/13,
Metagross manufacturer/all 0/1, ByteDance zero. No browser screenshot is claimed;
the prior file:// navigation restriction remains respected. CLI proof is in
`storage-inspur-cli-checks-2026-10-04.json`. The 174 one-off helpers are archived
under `research/session-tools/`; read their guards and never batch rerun them.

Queries (prefix with `python3 -m devicehunt`, using the bundled runtime below):

- `lookup 'i24&NS5162M5' --format json`
- `sql "SELECT product_id,parent_id,name FROM inspur_support_entries WHERE parent_id='115' ORDER BY name"`
- `search Metagross --vendor Inventec`
- `specs Metagross`, `specs E850G4`, `specs NF5280M5`

The support entries are searched with SQL/lookup, outside the HTML device-model
inventory. Parent ID `115` returns 148 raw entries; its category meaning still
needs source review. Names such as `i24&NS5162M5` remain grouped, not aliases.

Next bounded work:

1. Review the support feed's exact system names and public category tree against
   product/compliance sources. It includes substantial historical server/storage
   coverage, but not every entry is hardware or Inspur-branded. Avoid serial,
   warranty, activation and contact endpoints; no such actions were performed.
2. Review the already-captured Inventec download-center index and its public
   pagination (only page 1 was captured); continue older Hyve/Wiwynn/Inspur and
   original-vendor sources. Do not treat unseen pages as collected coverage.
3. Continue primary ByteDance B4020/B4010 affiliation evidence. Keep the Accton
   applicant statement separate and trade-data previews unverified. The known
   Eximpedia 402 was not retried. Existing known access denials remain in force.

All seven original URL fetches in this batch succeeded; no new access-denial
workaround was attempted. Complete historical coverage remains unproven and the
full goal stays active. Older milestones below describe their own snapshots.

## Inventec / QCT batch verified — 2026-10-04

Current live catalog: **3,941 identities / 5,140 source records / 247 sources**,
schema 27. All thirteen target groups remain enabled. This batch adds **41
identities / 53 records / 9 sources** while preserving prior identities, source
assertions, raw rows and display metadata through two staged preservation checks.

Inventec now has **78 associated identities, 44 with manufacturer claims**.
The new `inventec-server-index` importer preserves 28 exact public model headings
and source URLs. Four names have login-restricted detail links: only their public
index names/access status were imported, with no requests for restricted details.
Horsea and Seadra have separately captured power/rack/management specifications.
An original 2020-11-04 Inventec announcement explicitly attributes development
and manufacture of **Entei, Horsea, Golduck, E850G4 and FPGA SmartNIC C5020X** to
its EBG division. The separately IAC-made vehicle device is excluded.

Quanta now has **73 associated identities; its manufacturer inventory stays 41**.
Nine full QuantaGrid/QuantaPlex names retain QCT as brand. They have separate
`historical_subsidiary_portfolio` associations using QCT's dated parent-company
statement. The explicit cross-source inference retains both source URLs/hashes;
QCT is not a global Quanta alias and no manufacturer/operator claim transfers.
See [the complete source review and queries](research/inventec-qct-2026-10-04.md).

All-target manufacturer inventory: **2,142 target/identity pairs / 2,141 distinct
identities**. All associations: **4,076 pairs / 3,941 identities**. No factory,
hyperscaler or FCC filing assertions were added. Inspur stays at 13 associated
identities (3 manufacturer); ByteDance stays at zero. The owned baseline remains
**10/10 (9 documents + 1 label photo)**. ByteDance B4020/B4010 examples remain
**0/2 explicit gaps**, separate from owned devices; Accton's B4010 applicant
statement does not establish a ByteDance connection.

**404 Python tests + 6 report tests pass**. A new 110-job offline rebuild from
2,104 hashed inputs matches all 42 tables/FTS indexes and the owned baseline.
The document archive has **44 PDFs / 3,474 pages**, adding three original QCT/
Quanta PDFs and 62 pages. Original documents and extracted text are preserved.
Selected PDF table/relationship pages were visually reviewed before import.

Current database SHA-256: `58f8eb5d0d273b2328a53e12a777796c149208943aca317aa4e368cd5e09c544`.
Clean rebuild: `data/rebuilt-with-inventec-qct-2026-10-04.sqlite`.
Previous live backup: `data/devices-before-inventec-qct-2026-10-04.sqlite`.
Latest complete PDF collection: `data/raw/reference-documents-with-inventec-qct-2026-10-04.json`.
Manifest backup: `data/import-manifest-before-inventec-qct-2026-10-04.json`.
The reference importer replaces the whole collection: never import a partial bundle.

Proof under `data/exports/`: `inventec-qct-final-review-2026-10-04.json`,
`inventec-qct-rebuild-2026-10-04-comparison.json`,
`inventec-qct-promotion-2026-10-04.json`, plus dated tests and rebuild logs.
The first review covers the index/QCT stage; the final review also links the
second preservation check that adds five Inventec manufacturer records.
`inventec-qct-ui-audit-2026-10-04.json` verifies the actual generated HTML payload
with its inline script and DOM test harness: Inventec manufacturer/all 44/78,
Quanta manufacturer/all 41/73, nine QCT historical portfolio matches, and no
Inventec manufacturer match for the gated NX-H1331 index name. No browser
screenshot is claimed; the prior automated file:// navigation restriction is
respected. `inventec-qct-cli-checks-2026-10-04.json` records four working search/
specification commands and the intentionally failing ByteDance example check.
All 98 saved lookup reports and current CSV/JSON/HTML, FCC and hyperscaler exports
were refreshed. `inventec-qct-final-verification-2026-10-04.json` confirms the
current database hash across exports, all 44 PDF hashes, all 2,104 manifest input
hashes, SQLite integrity/foreign keys, owned/example results and the 42-table
comparison. Verification and exports left the live database unchanged. The 167
one-off research/import/audit helpers remain archived in `research/session-tools/`;
read their guards before use and never batch rerun them.

Try `search Horsea --vendor Inventec --role manufacturer`, `specs Horsea`,
`search K905G6_G6P --vendor Inventec`, or
`search 'QuantaPlex T41S-2U' --vendor Quanta` with the usual
`python3 -m devicehunt` prefix. Use all associations in the HTML to include the
QCT portfolio and index-only Inventec records. Exact underscore/parenthetical
headings and full QuantaGrid/QuantaPlex prefixes remain separate identities.

Next bounded work:
1. Seek primary ByteDance-labelled/compliance/product evidence for B4020/B4010.
   Keep trade-data preview claims unverified and exclude IMPERX's B4020 camera
   name collision. Neither example may be imported merely to make validation pass.
2. Continue older Hyve/Wiwynn/Inspur platforms and Inventec storage/networking
   sources. The new Inventec importer covers only the English server category.
   Recover a valid Ingrasys server catalog before assigning specs to its four
   unsuffixed index names; preserve the prior malformed capture outcome.
3. Continue original-vendor FCC/order-code gaps and review NCC/BSMI exact legal
   names without joining composite companies or unreviewed subsidiaries.

New access failures: QCT's Broadwell CPU-upgrade download index and its 2016
European-office press page returned 403. Their exact URLs/outcomes are retained
in `data/raw/inventec-qct-2026-10-04/fetch-report.json`; do not retry or bypass.
No firmware package, gated Inventec detail or paid trade-data content was fetched.
Complete historical coverage remains unproven; the full goal stays active.
Older milestones below are historical and their counts describe those snapshots.

## Ingrasys server/storage batch verified — 2026-10-04

Current live catalog: **3,900 identities / 5,087 source records / 238 sources**,
schema 27. All thirteen requested target groups remain enabled. This batch adds
eight Ingrasys names through 17 source-qualified assertions:
SV1120, SV1121, SV2121, SV2221, SV2221A, SV2221I, ES2000 and ES2100.
The first four are DOCS-index evidence only; their downloaded server PDF was
malformed and contributed no specifications. A/I suffixes remain separate.

The current Ingrasys company page names Foxconn Technology Group as parent.
Each model has a separately sourced, dated `parent_company_portfolio` association
with the canonical Foxconn search label. The company page does not name these
models; this explicit cross-source inference retains both source URLs/hashes.
Ingrasys and Foxconn Technology Group remain excluded from global Foxconn aliases.
No manufacturer, factory, operator or hyperscaler assertion was added.

Foxconn all-association inventory: **198 identities**, including these eight.
Its manufacturer count remains **169**. All-target manufacturer inventory remains
2,137 target/identity pairs / 2,136 identities; all associations now have
4,035 pairs / 3,900 identities. Inspur remains 13 associated identities (3 with
manufacturer claims), ByteDance remains zero. B4020/B4010 examples remain **0/2**
with explicit gaps; the owned baseline is still **10/10 (9 documents + 1 photo)**.

The archive now contains **41 PDFs / 3,412 pages**. One new storage PDF derivative
removes only a three-byte UTF-8 BOM from the separately retained original HTTP
body. Both image-only pages were visually reviewed; blank embedded-text status
is preserved. Curated model specs are identified as manual visual transcription.
ES2000 and ES2100 retain their distinct switch silicon and port counts. See
[the source/format review](research/ingrasys-servers-2026-10-04.md).

**399 Python tests + 6 report tests pass** after the report heading was updated
to follow the same thirteen-target registry as its selector. The fresh 107-job
rebuild from 2,083 hashed inputs matches all 42 tables/FTS indexes and the owned
baseline. Original rows, device display metadata and source assertions survive.
Actual generated HTML/script checks verify Foxconn manufacturer/all/parent
counts of 169/198/8 and no manufacturer match for SV2221A. No browser screenshot
is claimed; the prior automated file:// navigation restriction remains respected.

Current database SHA-256: `ff38451b49d4d78f40aa4ee72e69d4cca3f0fa05b7fa0c7e94bfd1a0e059776f`.
Clean rebuild: `data/rebuilt-with-ingrasys-2026-10-04.sqlite`.
Previous live backup: `data/devices-before-ingrasys-2026-10-04.sqlite`.
Latest complete PDF collection: `data/raw/reference-documents-with-ingrasys-2026-10-04.json`.
Manifest backup: `data/import-manifest-before-ingrasys-2026-10-04.json`.
The reference importer replaces the whole collection: never import a partial bundle.

Proof under `data/exports/`: `ingrasys-review-2026-10-04.json`,
`ingrasys-rebuild-2026-10-04-comparison.json`, `ingrasys-promotion-2026-10-04.json`,
`ingrasys-ui-audit-2026-10-04.json`, `ingrasys-cli-checks-2026-10-04.json`, and
`ingrasys-final-verification-2026-10-04.json`, plus the dated test/rebuild/export
logs. All 98 saved lookups and current exports were refreshed; all 41 local
PDF hashes verified. One-off scripts remain archived in
`research/session-tools/`; read their guards before running, do not batch rerun.

Try `search SV2221A --vendor Foxconn`, `search ES2000`, `specs ES2100`, or
`documents --source reference-pdf-ingrasys-es2000-es2100` with the usual
`python3 -m devicehunt` prefix. The last command lists both image-only pages;
text search cannot find their printed words. Use all associations in the HTML
to include parent-portfolio records; the default manufacturer scope excludes them.

Next bounded work:
1. Seek primary ByteDance-labelled/compliance/product evidence for B4020/B4010.
   Keep the Accton B4010 applicant statement separate; no ByteDance link yet.
2. Pursue exact Inventec/Quanta server/storage identifiers and older Hyve/Wiwynn/
   Inspur platforms. Recover a valid Ingrasys server catalog before assigning
   specs to the four unsuffixed index names.
3. Continue original-vendor FCC/order-code gaps and review exact NCC/BSMI legal
   names without joining composite companies or unreviewed subsidiaries.

New failed access: a web recheck of the previously captured Foxconn press URL
`https://www.foxconn.com/en-us/press-center/press-releases/latest-news/1172`
returned 403. The old capture survives; do not retry/bypass. Current Ingrasys
company evidence is independently captured. Complete historical coverage remains
unproven and the full goal stays active. Older milestones below are historical.

## ByteDance / Inspur expansion verified — 2026-10-04

The active project scope now has **thirteen target groups**, adding ByteDance and
Inspur to the previous eleven. GOAL.md and AGENTS.md contain the expanded scope.
The app goal remains active; its older objective text may still name eleven
groups because the available goal tool edits status only. Do not complete or
recreate the unfinished goal to change its wording; follow the latest user scope.

Current live catalog: **3,892 identities / 5,070 source records / 231 sources**,
schema 27. It includes **13 Inspur identities** across four sources, three with
regulator manufacturer claims. ByteDance has **zero qualifying catalog identities**
so far. The examples `BYTEDANCE B4020` and `BYTEDANCE B4010` are in
`validation/vendor-examples.json`: **0/2 supported; both explicit coverage gaps**.
These are not asserted as owned devices. The owned baseline remains **10/10**
(9 documents + 1 label photo).

An original Accton-hosted BTL PDF names B4010 with brand N/A and Accton as
applicant. Its separate catalog record provides a model lead, without claiming
ByteDance manufacture, brand or deployment. No FCC ID is assigned. The original
34-page Inspur i24 white paper supports the exact legal-name mapping and a
platform record. See [the source review and exclusions](research/bytedance-inspur-2026-10-04.md).

Two five-vendor assumptions were fixed: the NTRA PDF page screen and the offline
HTML vendor selector now follow the target registry. NTRA records rose from 119
to 169: +17 Hyve, +20 Foxconn, +9 Quanta, +4 Inspur brand/model assertions.
Including the other new sources, this pass adds 53 identities / 60 records.
All original source assertions, raw rows and identity keys survive. Seven Quanta
display titles/categories now come from the earlier NTRA source in rebuild order;
those changes are individually recorded in the preservation audit. Manufacturer
counts for the original five targets are unchanged.

Manufacturer inventory: 2,137 target/identity pairs, 2,136 distinct identities.
All associations: 4,027 pairs, 3,892 identities. Hyperscaler assertions and FCC
discovery/filing claims are separate from these totals. The PDF archive now has
**40 PDFs / 3,410 pages**, including vendor/reference documents.

The interrupted prior milestone was completed first: original Wiwynn SV7100G4 /
SV7100G4-C and Hyve Jade datasheets were imported, preserving chassis/blade and
parent-company distinctions. See [that model review](research/odm-models-2026-10-04.md).

Verification: **399 Python tests + 6 report-script tests**; fresh 106-job rebuild
from 2,071 hashed inputs matched all 42 tables/FTS indexes and the owned baseline.
The report's generated controls include all thirteen targets, including zero-hit
ByteDance. Browser visual QA was unavailable: automated file:// navigation was
blocked by browser policy, with no workaround attempted. DOM-script and generated
HTML checks remain the UI validation evidence.

Current database SHA-256: `edf5b96aabde30ea811de0089f45ea3dd7f8ab6062c6a728f988e2722d3310fe`.
Clean rebuild: `data/rebuilt-with-bytedance-inspur-2026-10-04.sqlite`.
Previous live snapshot: `data/devices-before-bytedance-inspur-2026-10-04.sqlite`.
Proof under `data/exports/`: `bytedance-inspur-review-2026-10-04.json`,
`bytedance-inspur-rebuild-2026-10-04-comparison.json`,
`bytedance-inspur-promotion-2026-10-04.json`, `vendor-example-validation.json`,
`latest-export-audit.json`, `bytedance-inspur-final-verification-2026-10-04.json`,
and the dated test logs. All 98 saved lookups were refreshed and all 40 local
PDF hashes verified after promotion.

Try these with the `python3 -m devicehunt` prefix (use the bundled runtime below):
`vendor-inventory Inspur --scope all --limit -1`, `search B4010`,
`documents B4010`, `search Jade --vendor Hyve`, and
`validate-inventory --inventory validation/vendor-examples.json`.
The example check intentionally exits 2 for its two unresolved gaps.

Next bounded work:
1. Find primary ByteDance-labelled/compliance/product evidence for B4020/B4010;
   keep Accton applicant, OEM, product brand and operator claims separate.
2. Pursue the captured Ingrasys DOCS model groups, older liquidation-friendly
   Hyve/Wiwynn/Inspur platforms, and Inventec/Quanta server/storage identifiers.
3. Review additional NCC/BSMI company mappings and original-vendor FCC/order-code
   gaps. Exact name filtering excludes unreviewed subsidiaries/composite cells.

New unavailable sources: Inspur about page returned 403; Eximpedia company page
returned 402. Preserve those outcomes and do not retry through alternate routes.
Trade-data preview labels remain discovery leads; no paid acquisition is authorized.
The full goal remains active; complete historical coverage is unproven.

## ODM expansion verified and promoted — 2026-10-04

The code now accepts all eleven target groups. Exact legal-name aliases exclude
unreviewed subsidiaries and name collisions; see
[the registry review](research/odm-target-registry.md). SONiC's cached Inventec /
Quanta entries and their ONL sections are included in the expanded rebuild.

The promoted catalog preserves every previous assertion/raw row and adds 304
identities and 353 source records: **3,837 identities / 5,007 records / 223 sources**.
The original five target inventories are unchanged. New manufacturer-claim
identity counts: Inventec 39, Hyve 11, Foxconn 169, Wiwynn 2, Quanta 41. These
include community/regulatory claims and non-datacenter devices, not a count of
proven hyperscaler platforms. TD SYNNEX has zero manufacturer claims; its one
current product association is explicitly a dated Hyve parent-company role.

New official product captures contribute twelve Wiwynn index-card identities
and Hyve Hercules 9120. EP100G2/EP200G2 card text and ST7000G4/SV7000G4 URL conflicts
are preserved as flagged facts, not aliases. Original HTML and fetch outcomes
are cached under `data/raw/odm-primary-2026-10-04/`.

395 tests pass; the live catalog retains 10/10 owned baseline hits (9 documents +
1 label photo). A second full rebuild matched all 42 checked tables/FTS indexes
and baseline results against the reviewed 104-job / 2,050-file manifest. The
previous database is preserved at `data/devices-before-odm-2026-10-04.sqlite`.

Current database SHA-256: `b241e62c0d4b8ea3edd8d100a4fe50b85df26d3b7b68f8a4f7571a052a7bcc47`.
Verification: `odm-candidate-review-2026-10-04.json`,
`odm-verified-rebuild-2026-10-04-comparison.json`, `odm-promotion-2026-10-04.json`
and `tests-odm-expansion.log` under `data/exports/`. The latest clean rebuild is
`data/rebuilt-with-odm-targets-2026-10-04.sqlite`. Historical handoff hashes below
refer to the prior snapshot.

Try `vendor-inventory Hyve --limit -1`, `search ATLANTA13 --vendor Hyve`,
`search Hornet16F --vendor Wiwynn`, `search D7032Q28B --vendor Inventec`, or
`vendor-inventory 'TD SYNNEX' --scope all`. Complete example commands need the
`python3 -m devicehunt` prefix. Snapshot-qualified manufacturer totals across
all targets are 2,134 target/identity pairs / 2,133 identities; all associations
are 3,971 pairs / 3,837 identities. Original five target inventories are unchanged.
Hyve's stored ATLANTA model fields retain their `/ Server` suffix text verbatim.

FCC coverage now counts 417 known target-associated strings: index 210, detail
171, application 2, FCC-ID-linked PDF 1. Missing at those stages: 207 / 246 /
415 / 416. The global sitemap contains 306 of the 417 strings and lacks 111.
The PDF archive remains 36 files / 3,371 pages, including non-FCC references.
Source failures remain recorded; the SEC merger exhibit's original download
returned 403 and must not be retried through another route.

Next priorities:
1. Review/capture more original model-specific Hyve, Wiwynn, Inventec and Quanta
   server/storage evidence and narrow the datacenter buying shortlist. Manufacturer
   totals include consumer devices and community claims; do not count them as
   known hyperscaler deployments.
2. Resolve Foxconn/Hon Hai/FIT/Ingrasys and QCT/Quanta corporate relationships with
   dated evidence and explicit roles. The existing CubScout FIT manufacturer claim
   remains separate and unassigned to the Foxconn target; no blanket alias was added.
3. Expand regulator queries and review additional exact company/Chinese legal names
   for all new vendors, then import source-qualified model records. NCC/BSMI raw
   snapshots have not yet received a comprehensive new-vendor mapping pass.
4. Use the refreshed per-vendor FCC gap reports for bounded filing/exhibit collection,
   preserving known denials and incomplete-mirror limits. Continue original-company
   coverage and exact ordering-code research alongside these new vendors.

The full goal remains active; complete historical device/FCC coverage is unproven.

## Initial resumption snapshot — before the expanded import

The user requested continued hunting for all five original vendors and added
**Inventec, Hyve, Foxconn, Wiwynn, TD SYNNEX / SYNNEX, Quanta, and other hyperscaler
ODMs**. Read [GOAL.md](GOAL.md) for the authoritative expanded objective and
bounded next milestone. The wait-at-handoff instruction below is historical and
has been superseded by this explicit continuation request. Root `AGENTS.md` was
updated for this scope; the copy in `docs/handoff-AGENTS.md` is historical.

The user subsequently edited the thread goal directly and then added Hyve as
a standalone target; its active status and
expanded objective were verified on 2026-10-04. GOAL.md now mirrors that objective.
Its eleven named vendor groups are the current target scope; other hyperscaler ODMs
remain discovery leads. The earlier app goal-edit limitation is resolved.

Current read-only verification: schema 27, 3,533 identities, 4,654 records,
221 sources; owned baseline 10/10. The database SHA-256 still matches the
September handoff. No new device claims or target mappings have been imported.
The CLI's reviewed target filters currently still cover the original five;
generic `lookup` can already search arbitrary text across raw datasets.

Initial cached name discovery (case-insensitive literal matches, not entity
resolution or current FCC coverage):

| Added name | Historical grantee rows | Existing catalog affiliations |
|---|---:|---:|
| Inventec | 9 | 0 |
| Hyve | 1 | 0 |
| Foxconn | 5 | 1 |
| Wiwynn | 0 | 0 |
| TD SYNNEX | 0 | 0 |
| SYNNEX | 1 | 0 |
| Quanta | 8 | 0 |

Full candidate rows, source IDs and the baseline are in
`data/exports/odm-expansion-start-2026-10-04.json`. Zero means no literal name match
in these two scanned datasets, not that no devices or filings exist. The existing
Foxconn relationship is a source record, not an imported Foxconn device inventory.

The first primary-source discovery pass is saved in
[research/odm-expansion-2026-10-04.md](research/odm-expansion-2026-10-04.md):
Ingrasys/Foxconn, QCT/Quanta, Hyve/TD SYNNEX, Inventec's enterprise product scope,
and five named Wiwynn platforms. The raw Quanta scan includes substring false
positives; none were promoted to entity mappings or imported device claims.

**Next action:** review company/legal-name mappings and grantee candidates, then
extend the target registry and backfill existing snapshots with explicit roles.
Only after the mappings are reviewed should target importers and manifest counts
change. Continue original-company gaps in parallel within each bounded milestone.
Preserve the 9-document + 1-label baseline distinction, exact ordering codes,
source hashes, and separate manufacturer, applicant, listing-holder and operator
roles. Free/public/open-source acquisition remains the user's preference.

## Historical handoff — 2026-09-27

Handoff verification completed from this destination: **392 tests pass**, all 42
rebuild comparisons match, 36 cached PDFs resolve locally with verified hashes,
and 94 saved lookup reports were refreshed. See
`data/exports/handoff-verification.json` and `handoff-environment.json`.
Current relocated database SHA-256:
`57ecfa75fb3bd48fe2b9a47318a73635645c96b2084fe94719aa966572a0d678`.

Updated 2026-09-27. **Usable local catalog and tools are complete as a first
milestone; complete historical device/FCC coverage is not established.** The user
requested a wrap-up and this standalone workspace so a future Astra or Fable can
continue. Do not restart an unbounded crawl automatically.

Destination: `/Users/nep/nocloud/git/nepeat/aislop/fcc_shenanigans`.
Original workspace: `/Users/nep/.codex/.chatgpt-projects/g-p-6ab8261241248191896f6d9090905253`.
The destination is the continuation workspace. The original was retained as a
copy, not deleted. All workspace code/data/research and historical database
snapshots were included; original project instructions are also saved at
`docs/original-project-AGENTS.md`. Root `AGENTS.md` contains the new local guidance.

## What the user wants

List devices made by Accton, Edge-Core, Celestica, Oracle and Amazon Data Services;
search them locally with SQL/basic search to find liquidated hyperscaler hardware
on eBay and other marketplaces. Public sources and open-source collection are the
chosen path. The user explicitly discarded Datadory/paid-provider exploration;
no quote, sample request, subscription, outreach or purchase was authorized.

## Start here

Open `data/exports/catalog.html` for the offline catalog. No server, account or
network is required. Select manufacturer, factory or all-association scopes;
expand evidence to inspect the exact role and source. It is a device catalog,
not a browser for every row in all raw reference datasets.

Run commands from this folder with Python 3.9+ and SQLite FTS5:

```sh
python3 -m devicehunt stats
python3 -m devicehunt search 7760
python3 -m devicehunt vendor-inventory --limit -1
python3 -m devicehunt vendor-inventory Celestica --scope all --limit -1
python3 -m devicehunt hyperscaler-inventory
python3 -m devicehunt lookup 'AZ4444-52X-DC-10'
python3 -m devicehunt lookup 'HEDML2560' --limit 5
python3 -m devicehunt specs 'DS6001' --exact-model
python3 -m devicehunt tested-components PM883
python3 -m devicehunt documents antenna
python3 -m devicehunt match-listing --title 'Celestica R4023-F9102-A0' --with-specs
python3 -m devicehunt validate-inventory
```

On this Mac, system Python/git commands can trigger an unaccepted Xcode-license
error. The working Python is:
`/Users/nep/.cache/codex-runtimes/codex-primary-runtime/dependencies/python/bin/python3`.
Use that executable instead of changing system settings. Ordinary use needs no
third-party packages; a full NTRA PDF rebuild additionally needs `pypdf` and
`pdfplumber`, available in that bundled runtime. Git initialization/commit/push
was not part of the handoff.

`data/devices.sqlite` is authoritative (schema **27**). Open it read-only in a SQL
client, or use `sqlite3 -readonly data/devices.sqlite`. Examples:

```sql
SELECT target, role, brand, model FROM target_devices LIMIT 20;
SELECT page_kind, count(*) FROM fcc_sitemap_urls GROUP BY page_kind;
SELECT fcc_id, record_url FROM fcc_sitemap_urls WHERE fcc_id LIKE 'HED%';
SELECT sitemap_url, capture_state, issue, row_count FROM fcc_sitemap_pages;
```

`lookup` scans raw datasets as well as catalog identities and can take several
seconds. Its counts use different units and must not be summed as device totals.
`--limit` applies per dataset; use `-1` deliberately for full output.

## Current verified state

| Item | Count / scope |
|---|---|
| Catalog | 3,533 identities; 4,654 source records; 221 sources |
| Manufacturer/OEM inventory | 1,872 target/identity pairs, 1,871 unique identities |
| Manufacturer identities by target | Accton 425; Edge-Core 219; Celestica 193; Oracle 859; ADS 176 |
| Factory listings | 2 Celestica identities; separate from manufacturer scope |
| All company associations | 3,666 target/identity pairs; 3,533 identities |
| Owned baseline | 10/10 searchable and identity-supported; 9 document hits + 1 label-photo-only hit |
| Hyperscaler evidence | Default eligible report: 19 assertions / 17 identities; all assertions: 21 / 17 |
| Strict documented deployment + manufacturer subset | 3 assertions (Wedge 100, Wedge 400, Wedge 400C) |
| FCC target discovery index | 344 source rows / 210 distinct literal identifier strings |
| FCC company table | 237 raw rows: Accton 235, ADS 2 |
| Target FCC detail attempts | 174: 171 captured, 3 unavailable/not-found |
| FCC detail contents | 1,493 exhibit links and 1,092 mirror text excerpts |
| Cached PDFs | 36 files, 3,371 pages; not a complete FCC archive |
| New public sitemap snapshot | 231,962 URL rows; 190,198 distinct FCC-ID discovery URLs |
| Sitemap capture coverage | 38 successful children / 48 advertised; 10 HTTP 400; 2017/2018 unadvertised |
| Offline rebuild inputs | 103 import jobs; 2,041 hashed files |
| Checks | 392 tests pass; clean rebuild matches all 42 checked tables/indexes and owned baseline |

These are source-qualified assertions and catalog identities, not a unique count
of physical product designs. Revisions, regulatory names and parts may overlap.
No "all devices ever made" or worldwide FCC completeness claim is warranted.

Large additional local tables include FCC grantees (50,153), Anatel (190,852),
NCC Taiwan (110,957), BSMI certificates (105,732 plus three smaller registers),
NTRA (76,510), IEEE MAC assignments (58,639), PCI IDs (42,438), ICASA (26,258),
Bahrain (10,169), Nigeria (8,517), Mauritius/St Vincent (4,985), TEC (2,331),
RRA (1,081 rows), ENERGY STAR (1,820 rows across three datasets), Ghana (1,153),
ISED (628), and selected Japanese queries (346). These snapshots have different
scope, age, quality and company roles. Inspect the matching research notes.

## Owned-hardware acceptance baseline

`validation/owned-hardware.json` preserves all ten exact identifiers:

- Celestica: `CS8200-32X-UN-00`, `CS8210-32x-un-00`.
- Edge-Core: `DMC-12-48R-AS0`, `7760-32X-A-AC-F`, `AZ4444-52X-DC-10`, `AS7772-32X`.
- Oracle: `E4-2C`, `E5-2C`, `X7-2C`, `X10-2C`.

The shortened AZ4444 name is superseded by the full `-DC-10` code. The user owns
these via eBay; the first six have owner-reported AWS association. That is context,
not independent proof of deployment or a particular unit's history.

The user supplied a label photo from listing `https://www.ebay.com/itm/389108536363`.
It explicitly pairs `7760-32X-A-AC-F` with supplier SKU `AS7760-32X`. Original bytes
are saved at `data/raw/edgecore-7760-label.png`, imported through
`data/label-evidence.json`. Serial/MAC text was not added to searchable fields.
The label qualifies as physical identity evidence, separately from seller claims;
it adds no manufacturer or AWS assertion. Document-only coverage remains 9/10.
See `research/edgecore-7760-label.md` and `data/exports/inventory-validation.json`.

## What was just finished

The public FCCID.co sitemap collector and schema-27 importer retain original XML,
reported site dates, duplicate rows if present, missing files and SHA-256 checks.
A failed child is not a zero-row success. A source refresh is transactional and
hash changes during insertion roll back. `lookup` includes these rows in its own
section. No models, grantees, manufacturers or filings are inferred from a URL.

The global snapshot contains only 205 of our 271 previously known target-associated
FCC strings; 66 are absent. Its 171 literal-prefix leads overlap our original
mirror collection. This confirms that a large mirror sitemap still has gaps.
`fcc-coverage` reports the existing target stages independently: index 210/271,
detail 171/271, application 2/271, FCC-ID-linked PDF 1/271. The 36-PDF archive
also includes vendor/reference documents, so do not conflate these counts.

See `research/fcc-sitemaps.md`, `data/exports/fcc-sitemap-summary.json`,
`fcc-sitemap-pages.csv`, `fcc-sitemap-prefix-leads.csv`, and
`data/raw/fcc-sitemaps/collection-report.json` for exact evidence.

## Prioritized next work for a future agent

1. Establish a defensible target-grantee and historical filing inventory. Review
   the 61 known strings missing from the existing target index, and the 100 without
   detail captures. Identify legal names/legacy entities with explicit evidence.
   Do not use substring company matching, guess aliases or broaden ADS to Amazon.
2. Build bounded, resumable public collection for outstanding allowed filing
   metadata and exhibits. Keep index discovery, applications, exhibits, downloaded
   originals, mirror excerpts, and unavailable files as separate states. Inspect
   prior fetch reports first. Do not restart the 190k-URL crawl indiscriminately.
3. Address documented sitemap gaps via independent permissible source datasets,
   not access-control bypasses. The 1980–1989 requests returned 400; 2017/2018 are
   absent from the advertised index; very small 2016/2019 lists look incomplete.
   A government or mirror total is still needed before claiming complete coverage.
4. Expand original vendor/OEM, compliance, service, OCP, hardware-support and
   historical product sources. FCC data alone cannot enumerate all non-radio
   datacenter equipment. Preserve cross-brand OEM roles with dated evidence.
5. Improve buying usefulness for exact variants: power/voltage, rack format,
   cooling/airflow, optics, controllers, firmware/boot restrictions, licensing and
   service dependencies. Already supported: specifications, listing matching,
   tested components and source-linked hyperscaler shortlist.
6. Optional corroboration: find an independently accessible manufacturer or
   regulatory document for the full 7760 ordering code. It is no longer an owned
   identity blocker because the label photo resolved it; do not churn on denied URLs.

Open OCR/structured extraction, ambiguous model reconciliation and wider bulk
collection only as bounded tasks with measurable acceptance checks. Reachability
or a source's advertised completeness is not verified archive completeness.

## Known access failures and limits

Read `research/fcc-source-coverage.md` and source-specific fetch reports before
trying endpoints. Do not repeatedly retry denied documents or fetch those same
items through alternate routes. Key prior failures include FCC OET API/getFCCIDList
403; FCC.report CubScout original PDFs 5661528/5657033 and detail page 403;
FCC.report Accton company original HTML 403; FCC.report sitemap 403;
FCCID.io robots and several company/document URLs 403; numerous Accton corporate
PDF/pages 402/403; public UL original pages 403; and individual FCCID.co PDF errors.
Many reviewed web-text extracts are preserved but are not original HTML/PDFs.
The detailed capture reports, rather than this abbreviated list, are authoritative.
No need to re-request the 7760 chassis label. No paid-provider question is pending.

Browser rendering QA was blocked in the original environment; do not claim a
fresh visual inspection of the offline HTML. The data and command outputs were
checked. Historical source notes and dated reports retain earlier counts.

## Reproduce / verify

From this folder (substitute the bundled Python above if needed):

```sh
python3 -m unittest discover -s tests
python3 -m devicehunt --db data/rebuilt-next.sqlite rebuild --manifest data/import-manifest.json
PYTHONPATH=. python3 scripts/compare_rebuild.py data/rebuilt-next.sqlite
PYTHONPATH=. python3 scripts/refresh_exports.py
```

A rebuild must use a NEW destination. It uses only saved inputs, verifies hashes
before and after import, and checks counts, integrity and foreign keys. The latest
existing verified output is `data/rebuilt-with-fcc-sitemaps.sqlite`; older rebuilds
are historical. `compare_rebuild.py` ignores observation/capture timestamps and
cache-path locations, but compares all data columns otherwise and the FTS row IDs.
The latest test/rebuild/comparison logs are `data/exports/tests-fcc-sitemaps.log`,
`rebuild-fcc-sitemaps.log`, `compare-fcc-sitemaps.log` and `rebuild-comparison.json`.
The pre-relocation live database SHA-256 is
`ab667caef522a7534ded33e134e273309b9b315a928954e24a4a884aaf7c88c8`.

Current database `sources.cache_path` values are rebased and hash-verified in the
new folder by `scripts/relocate_catalog.py`; hashes and evidence claims remain
unchanged. `data/exports/relocation-audit.json` records its new hash. Historical
rebuilds/reports intentionally keep their original paths and timestamps. Rebuild
or adapt them if needed; the current DB must not depend on the old directory.

`data/exports/handoff-copy-manifest.json` records byte-level copy verification
before the intentional root-AGENTS replacement and current-cache-path relocation.
`data/exports/handoff-verification.json` records the destination checks. All files
are local; no repository publication occurred. Raw/export/database files are
ignored by the existing `.gitignore` but are physically present in the handoff.

## Code map

- `devicehunt/catalog.py`: schema, source/identity/role model, SQLite/FTS.
- `devicehunt/__main__.py`: CLI, read-only reports, HTML export.
- `devicehunt/pipeline.py`, `rebuild.py`: importer routing, snapshot reproduction.
- `devicehunt/lookup.py`, `specifications.py`, `listings.py`: local discovery/use.
- `devicehunt/vendor_inventory.py`, `hyperscaler_inventory.py`: qualified lists.
- `devicehunt/validation.py`: owned baseline, including 9+1 provenance policy.
- `devicehunt/fcc_coverage.py`, `fcc_index.py`, `fcc_company.py`, `fcc_sitemaps.py`:
  distinct FCC discovery and collection stages.
- `data/import-manifest.json`: current import jobs, checksums and expected counts.
- `data/raw/`: original snapshots and reviewed extracts; preserve provenance.
- `research/`: source evidence, limits, collectors and historical investigations.
- `research/session-tools/`: archived one-off transient helpers; inspect, do NOT
  bulk-run them. Current normal procedures are in `scripts/` and the CLI.
- `docs/`, `examples/`, `tests/`, `validation/`: user guides, examples, checks.

The original unbounded "all devices" objective remains incomplete. This is a
verified handoff checkpoint, not an assertion that every historical product has
been found. Continue when the user requests the next bounded milestone. :3
