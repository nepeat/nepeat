# Device hunting

## Active goal — liquidation hunting, revised 2026-10-04

Status: resumed by the user's explicit request. This replaces the prior
five-vendor research scope and the September handoff's instruction to wait.
The user's latest scope revision prioritizes liquidated enterprise, networking
and server hardware. Hyperscaler connections are optional provenance context,
not an inclusion requirement.

Implementation checkpoint 2026-10-04: all thirteen target filters work in the
CLI and generated HTML. The verified catalog has 4,064 identities, 5,273 source
records and 263 sources; 408 Python tests plus 6 report-script tests pass. The
owned baseline is 10/10. ByteDance B4020/B4010 examples remain 0/2 external-evidence
hits and explicit research gaps. Eight Ingrasys platforms have separately sourced
Foxconn parent-portfolio associations. The subsequent Inventec/QCT batch adds
41 identities, including five explicit Inventec manufacturer claims and nine
separately sourced QCT/Quanta historical portfolio associations.
Schema 28 adds 1,064 raw Inspur support entries for SQL/lookup, without promoting
mixed parts/software/menu entries into device claims. Metagross is a new Inventec
platform; E850G4 and NF5280M5 have additional specifications.
A further 34 Inspur identities have reviewed compatibility/product sources.
The enterprise hardware batch adds 88 identities and 93 records from three PDFs.
Inspur has 133 associated identities and three manufacturer-backed identities;
Inventec has 81 and 44 respectively. Explicit power/chassis dependencies, one
NF5288M5/AGX-2 alias and three specification conflicts are preserved. A Baidu
SR-AI joint-design statement remains separate from deployment evidence.
A clean 114-job rebuild from 2,146 hashed inputs matched 43 tables/indexes.
See PROGRESS.md for current evidence counts and priorities. This is an active
collection goal; the checkpoint does not establish completeness.

Build a local SQL-queryable device catalog and search tools to hunt liquidated
enterprise, networking and server hardware on eBay and other marketplaces.
Prioritize exact model and ordering codes, source-qualified company roles and
practical specifications that help identify and evaluate surplus hardware.
Relevant equipment includes servers, switches, storage, enterprise appliances,
chassis/nodes, rack/power equipment and their hardware components. A device does
not need a hyperscaler connection to be useful to this project.

**Vendors: Inventec, Hyve, Foxconn, Wiwynn, TD SYNNEX / SYNNEX, Quanta, Accton,
Edge-Core, Celestica, Oracle, Amazon Data Services, ByteDance, Inspur.**

The user added ByteDance and Inspur on 2026-10-04, expanding the previous
eleven-group objective to thirteen groups. Additional relevant enterprise hardware
vendors and ODMs remain discovery leads. B4020 and B4010 are ByteDance coverage examples stored in
`validation/vendor-examples.json`; the ten owned-device baseline stays separate.
Case examples are expectations, not evidence of ownership, manufacturing or deployment.

Keep all code, data, source evidence and progress in
`/Users/nep/nocloud/git/nepeat/aislop/fcc_shenanigans`.
Use free/public and open-source acquisition. Preserve the ten owned-device
acceptance checks, including the full `AZ4444-52X-DC-10` identifier and the
separate document-versus-label evidence for `7760-32X-A-AC-F`.

Hyve is an explicit standalone vendor target in the latest user-edited goal,
not only a discovery path under TD SYNNEX. Preserve Hyve and TD SYNNEX/SYNNEX
company roles and assertions separately; the relationship does not transfer
manufacturer or operator claims between entities.

The new names expand research scope; they are not themselves manufacturer
assertions, legal-entity aliases or proof of hyperscaler deployment. Track
TD SYNNEX and SYNNEX as separately printed names; review dated entity/ownership
relationships before connecting records. Apply the same rule to parents,
subsidiaries, contract manufacturers, product brands and ODM business units.
Additional hardware vendors and ODMs enter the research queue when supported by relevant sources;
do not assume every distributor, component vendor or systems integrator is an ODM.

### Next bounded milestone

1. Review the remaining merged-cell NIC tables in the captured InCloud Rail PDF,
   preserving exact card names and table vendor roles. Continue older Hyve/Wiwynn
   and original-vendor enterprise hardware sources alongside the Inspur work.
   The reviewed 2018 catalog and Inventec brochures are now imported; see PROGRESS.md.
2. Prioritize identifiers useful for liquidation searches and practical constraints:
   power input, rack format, cooling, firmware access, boot restrictions, required
   controllers and exact revisions. Preserve absent, ambiguous and conflicting facts.
3. Continue coverage across all thirteen target groups and relevant enterprise
   hardware vendors/ODMs. Verify company and model relationships from source evidence;
   keep brand, applicant, manufacturer, parent and operator roles distinct.
4. Preserve model families versus full ordering codes and chassis/node relationships.
   Reproduce reviewed imports from hashed inputs in a new database, compare results
   and refresh local search exports. Raw `lookup` matches remain discovery evidence.
5. Record per-vendor imported identities, sources, collection failures, unresolved
   names and coverage gaps. Keep original-vendor hunting active. All ten owned
   identifiers must continue to pass; no unsupported OEM/operator claims may be added.

Milestone acceptance: exact-model searches and source-qualified inventories work;
new reviewed assertions reproduce from hashed inputs; the owned baseline and
meaningful tests pass; each vendor has documented coverage or an explicit gap.
Broader historical coverage supports the buying-research goal. Neither a source
row count nor an empty result establishes a complete worldwide hardware inventory.

On 2026-10-04, read-only verification found 3,533 identities, 4,654 records and
221 sources; the baseline still passes 10/10. The initial name scan found 24
literal matching historical FCC registration rows across Inventec, Hyve, Foxconn,
SYNNEX and Quanta, plus one existing Foxconn catalog affiliation. Wiwynn and
TD SYNNEX had no literal name matches in those two scanned datasets. These are
starting leads, not device totals or evidence of absence. See
`data/exports/odm-expansion-start-2026-10-04.json`.

Thread goal status verified active on 2026-10-04 with the revised liquidation-hunting
objective and all thirteen target groups. At this revision the goal tool reported
no existing goal, so the user's requested objective was created directly. No
unfinished collection goal was marked complete to change its wording.

## Prior collection history (through 2026-09-27)

Latest handoff: see PROGRESS.md. The working catalog is ready for local use;
complete historical coverage remains an open research goal. Schema 27 adds
231,962 public sitemap URL rows (190,198 literal FCC ID candidates), separately
from device assertions. Current totals: 3,533 identities / 4,654 assertions /
221 sources. The following entries retain prior collection history.

Latest evidence: the supplied 7760 chassis-label photo explicitly pairs full
model 7760-32X-A-AC-F with supplier SKU AS7760-32X. Validation is now 10/10
identity-supported: nine document snapshots and one label-photo-only match.
Two Amazon FCC company rows add CubScout discovery-index coverage. Current
catalog: 3,533 identities, 4,654 assertions, 220 sources. Combined FCC index:
344 source rows / 210 literal IDs. PDF archive remains 36 files/3,371 pages.
See research/edgecore-7760-label.md and research/fcc-amazon-company.md.

Recorded at the user's request, 2026-09-26.

Current validation: **10/10 searchable and identity-supported**, with distinct
counts of nine document-snapshot hits and one reviewed-label-photo-only hit.
All four Oracle identifiers pass. AZ4444-52X-DC-10 is the corrected full name.
No owned identity gap remains under the photo-inclusive policy; document-only
scraping still lacks the full 7760 ordering code. AWS owner claims remain context.
Later sections retain the investigation history;
earlier counts describe earlier snapshots. The executable baseline is
`validation/owned-hardware.json`, with latest results in
`data/exports/inventory-validation.json`.

Build a local SQL-queryable catalog and basic search tools, working toward a
list of all devices made by **Accton, Edge-Core, Celestica, Oracle, and Amazon
Data Services**. The buying use case is liquidated hardware on eBay and other
marketplaces, especially hardware designed for or used by hyperscalers.

## Evidence rules

- Separate brand, physical manufacturer/OEM, platform vendor, documentation
  vendor, regulatory applicant, and operator. An association is not OEM proof.
- Amazon consumer products do not become Amazon Data Services devices simply
  because they share the Amazon name. Explicit legal-entity matches only.
- Preserve revisions, codenames, part numbers, alternate branding, source URLs,
  and conflicting claims. Never silently merge similarly named hardware.
- Distinguish documented hyperscaler lineage from deployment, and platform
  deployment from provenance of a specific surplus unit.
- Buying research should eventually capture power input, rack format, cooling,
  firmware access, boot restrictions, required controllers, and exact revisions.
- Marketplace search URLs are leads, not checked listings or buying advice.

## First milestone

Working SQLite + full-text search + offline browser catalog, source importers,
CSV/JSON exports, and sourced initial records for all five target organizations.
The initial catalog is partial; neither FCC filings nor community hardware
wikis alone can establish a complete historical device inventory.

## Next coverage work

The baseline now contains ten identifiers: the original six below plus Oracle
E4-2C, E5-2C, X7-2C, and X10-2C, appended at the user's request. Preserve the
complete `-2C` suffix and do not infer AWS associations for these Oracle entries.

The owner's original six eBay-auction devices form the initial coverage benchmark:
CS8200-32X-UN-00, CS8210-32x-un-00 (Celestica, owner-reported AWS), and
DMC-12-48R-AS0, 7760-32X-A-AC-F, AZ4444-52X-DC-10, AS7772-32X
(Edge-Core, owner-reported AWS). Preserve these exact labels. They are validation
expectations, not scraped evidence or verified AWS deployment assertions.
The AZ4444 label was initially supplied in shortened form; its full suffix was
confirmed by the owner on 2026-09-27. The following milestones are historical.
Run `python3 -m devicehunt validate-inventory` after source expansions. The first
run finds zero hits and six source-coverage gaps. Closing these six gaps takes
priority over increasing broad catalog counts; complete ordering-code evidence
and working search are required to pass.

The NTRA PDF importer closes two benchmark gaps: DMC-12-48R-AS0 and AS7772-32X
have exact Edge-Core model rows on page 189 of the cached register. The remaining
four initially still failed. UL's public Models section was subsequently
retrieved through the web tool and its 25 complete CS-series identifiers were
automatically extracted and cached as structured factual data. Both Celestica
UN-00 labels now pass, bringing the benchmark to 4/6. This is explicitly a
web-tool extract rather than original HTML; direct HTML retrieval returned 403.
The remaining two gaps and leads are recorded in `research/owned-hardware-gaps.md`.
TEC's cached table adds OEM evidence for CS8200-32X-UN-00 and three suffixed
AZ4444 variants. A cached seller page supplies the complete 7760 ordering code.
All six inventory queries now return results, but only four exact identities
meet the corroboration check; seller-only and incomplete-label matches remain
unresolved. Preserve both measures rather than counting loose hits as proof.

ONIE platform definitions and selected Oracle/Celestica storage and networking
indexes are now imported. This extends the first milestone; it does not establish
a complete historical inventory or resolve every platform-to-product mapping.

FCC application/exhibit manifest import now covers the two reviewed Amazon Data
Services IDs (32 exhibit rows). CubScout test configuration/OEM evidence and
TE1A001 Monitron naming/recertification leads are recorded. Direct index download
returned HTTP 403; reviewed web metadata is available, but automated filing
discovery, pagination, incremental network refresh and a PDF archive remain open.

Edgecore's complete cached English product sitemap set now contributes 238
hardware product records from 239 pages (one software product excluded).
Explicit product-title aliases and vendor categories are searchable. All 51
notices linked by the cached retirement index now contribute 254 affected-model
assertions, with exact order codes, available part numbers, visible effective
dates and original grouped cells preserved. Cross-brand identity relationships,
other historical archives, and support-deadline evidence still need review.

Oracle's cached retirement table now contributes 186 model assertions, including
legacy Sun/Netra systems and reviewed switch aliases/part numbers. Three
Fujitsu-branded rows are excluded; four explicitly grouped Netra rows are expanded.
Documentation links and successor recommendations remain separate. Celestica
DS6000, DS6001, DS6010, and historical SC4200 entries have reviewed vendor evidence.

1. Expand historical Accton/Edgecore and Celestica through vendor end-of-life
   documentation; research ODM rebadges and review ONIE platform/model mappings.
2. Expand Oracle/Sun servers, storage shelves, switches, and part-number maps.
3. Investigate Amazon Data Services FCC exhibits and public AWS/OCP hardware
   disclosures; keep Annapurna Labs and other related entities separate.
4. Extend the reviewed FCC manifest importer with authorized automated discovery,
   pagination/truncation checks, incremental refresh, and exhibit archiving.
5. Add reviewed equivalence relationships and buying constraints, then optional
   marketplace listing imports. No automatic purchases or seller messages.


Oracle baseline follow-up: E4-2C now passes from cached NIST tested-platform
HTML, X7-2C from Oracle's cached source-code index, and X10-2C from its cached
licensing documentation. E5-2C remains a benchmark-only lead. The expanded
baseline now has 10/10 searchable queries and 7/10 corroborated identities;
7760-32X-A-AC-F, AZ4444-52X, and E5-2C remain explicit coverage gaps. Code tests
pass independently of this deliberately failing coverage gate.


Offline reproducibility: `rebuild` now imports the full collected snapshot set
from `data/import-manifest.json` into a new database. It checks 319 files across
24 jobs, source counts, SQLite integrity and referential integrity before
publishing. A full build reproduced 1,128 identities, 1,342 records and 83 source
entries. All nine evidence tables matched the working database after excluding
local paths and import/fetch timestamps; ten baseline searches and seven exact
identity passes were retained. Verification results are in
`data/exports/rebuild-comparison.json` and `rebuild-validation.json`. The rebuilt
database is `data/rebuilt.sqlite`; the original remains the default. 55 code
tests pass. Public searches still have not closed the three identity gaps.


FCC grantee expansion: the official EAS dataset 3b3k-34jp contributes all 50,153
historical company registration rows to a separate `fcc_grantees` table. Its
last update is 2021-03-22; this is not a complete or current device-grant export.
The local `grantees` command searches company names and codes. Before/after API
counts, update timestamps, complete CSV row checks and file hashes guard against
truncation or mixed downloads. No company row becomes a device identity.

The expanded offline rebuild now runs 25 jobs with 325 verified files. A full
build at `data/rebuilt-with-grantees.sqlite` matches all ten source/evidence/
registry tables, excluding local paths and timestamps, and retains the 10/10
search results and 7/10 exact-identity baseline. Device totals remain 1,128
identities and 1,342 assertions; there are now 84 source entries. All 58 code
tests pass. Filing discovery can start from the newly verified HED, YZK, A4H,
MRF, NVN and 2AVPL codes; see `research/fcc-source-coverage.md`.


ICASA coverage expansion: all 906 pages / 26,258 rows from the public 2025
PDF are now retained in `equipment_register`, separate from target-company
identities. 17,996 rows have complete fields and recognized TA/TE/PA references;
8,262 retain raw cells with `review_required` because the source contains shifted,
empty or unrecognized fields. There is no manufacturer/applicant column. Local
`registry` searches include raw review rows; `show` and HTML expose only exact
model-string matches from identified rows, explicitly as unverified hardware
identity leads. Nineteen catalog identities have such references, including DMC
and the three suffixed AZ4444 variants. This does not change the 7/10 baseline.

The full rebuilt database `data/rebuilt-with-registers.sqlite` reproduces all
eleven evidence/registry tables after excluding local paths and timestamps.
The manifest now contains 26 jobs and 328 files; counts are 1,128 device identities,
1,342 source assertions, 85 sources, 50,153 grantee registrations and 26,258
ICASA rows. All 60 code tests pass. The original `data/devices.sqlite` remains
the default. FCC API and three mirror-prefix requests returned HTTP 403; that
collection path is unresolved, with details recorded in the FCC coverage notes.


Buying-relevant details: the cached Edgecore product Features sections now
contribute 568 distinct source statements on 198 products, classified into 623
power/cooling/physical/software-installation facts. Source qualifiers and SKU
alternatives remain intact; no voltage, total wattage, airflow direction or
software entitlement is inferred for a specific used unit. Navigation and
compatible-product sections are excluded. The `specs` command and a dedicated
HTML details panel expose the facts; `--exact-model` prevents compatibility-only
mentions from pulling unrelated accessories into an exact product lookup.

All 64 code tests pass. Rebuild at `data/rebuilt-with-specifications.sqlite`
reproduces all eleven evidence/registry tables, excluding paths/timestamps.
The baseline remains 10/10 searchable and 7/10 corroborated. Investigation of
7760-32X-A-AC-F found an indexed AS7760 family guide whose download returned 403;
it did not establish exact ordering-code equivalence. That gap remains open.


Bahrain manufacturer expansion: the public register API returns 10,169 structured
records, all retained in `bahrain_equipment`. Two full responses agree, and six
company queries are consistent; there is no independent server total, so scope
is the returned response. Exact target manufacturer matches contribute 34 model
assertions from 33 rows (16 Amazon, 13 Accton, 3 Celestica Shanghai, 1 Oracle).
This adds 26 identities, including 16 Amazon Data Services server/switch model
names. They are regulatory manufacturer claims, not factory or deployment proof.
Amazon now has 18 catalog identities; the new leads have their own CSV export.
See `research/bahrain-manufacturer-evidence.md` for scope and evidence.

The populated catalog now has 1,154 identities, 1,376 assertions and 86 sources.
The manifest includes 27 jobs and 337 files; the full build at
`data/rebuilt-with-bahrain.sqlite` matches all twelve evidence/registry tables,
excluding local paths/timestamps. 67 code tests pass. Baseline still has 10/10
searchable labels and 7/10 exact-identity passes; suffix variants and Accton
manufacturer records do not silently supply Edgecore or AWS associations.


AWS Outposts primary-document expansion: cached AWS compliance HTML and two
linked PDFs contribute six assertions for AWS-branded SCOUT01, SCOUT02 and
CubScout. PDF manufacturer identification is Amazon Data Services, Inc.; the
HTML links map both SCOUT models to the 2U section and CubScout to 1U. These
product-family relationships are available through the hyperscaler filter.
Blank signature and placeholder date fields preclude treating the documents as
completed signed declarations. No factory, deployment, CPU, firmware usability
or used-unit service eligibility is inferred. Cross-brand and suffixed registry
identities remain separate. See `research/aws-outposts-models.md`.

The manifest now covers 28 import jobs and 341 hashed files. Full offline build
at `data/rebuilt-with-outposts.sqlite` reproduces all twelve evidence/registry
tables after excluding local paths and timestamps: 1,157 model identities,
1,382 assertions and 89 sources. All 67 tests pass; baseline remains 10/10
searchable and 7/10 corroborated. Exports and the offline HTML catalog are refreshed.
Historical Bahrain PDF retrieval timed out; it has not been imported.


Broadcom server compatibility expansion closes E5-2C: public search and detail
API snapshots agree on Oracle Server E5-2c and Oracle as compatibility partner.
Seven target-name queries were captured twice around the detail download; the
Oracle query has 24 records and the others zero within this server-guide scope.
The importer verifies totals, hashes, query stability and detail identities
before replacing evidence. No acceptance criterion was relaxed. The baseline
now has **10/10 searchable and 8/10 corroborated**; 7760-32X-A-AC-F and AZ4444-52X
remain gaps. All four Oracle labels pass. New model identities are X5-2M,
X6-2M, E6-2c and X11-2c; compatibility records retain source-specific CPU-family,
socket, form-factor and notes fields without inferring unit configurations.

Catalog totals are 1,161 identities, 1,406 assertions and 90 sources. The full
offline build at `data/rebuilt-with-broadcom.sqlite` reproduces all twelve
evidence/registry tables after excluding local paths and timestamps, including
the improved baseline. Manifest: 29 jobs, 380 hashed files. All 72 tests pass.
See `research/broadcom-server-evidence.md`; refreshed catalog/target exports
and `data/exports/oracle-compatibility.csv` include the new evidence.


Anatel bulk-register expansion: the official published ZIP contributes all
190,852 CSV records to `anatel_equipment`, preserving 21 source columns as text,
exact duplicates, empty model cells, and leading-zero approval/CNPJ identifiers.
Two downloaded archives agree byte-for-byte; the original source-index link is
cached. This establishes completeness of the published file, not the regulator's
underlying live database. `anatel QUERY` searches the full local register and
`sync --anatel` captures/imports it with integrity checks.

Exact target manufacturer rows add 235 assertions (102 Edge-Core, 63 Accton,
34 Celestica, 20 Amazon Data Services, 16 Oracle) and 100 catalog identities.
Anatel explicitly supplies EdgeCore manufacturer evidence for all three suffixed
AZ4444 variants and AS7760-32X, without proving the bare AZ4444-52X or full
7760-32X-A-AC-F labels. The inventory stays 10/10 searchable and 8/10 corroborated.
The additional Amazon models include AZ9074/AZ9075/AZ9274/AZ9275 complete
64X-DC-42 codes, NAF20_L1 and CMWA6400. These are manufacturer-register claims,
not factory or deployment proof. Commercial names remain unparsed facts.

Catalog totals: 1,261 identities, 1,641 assertions, 91 sources. Rebuild manifest:
30 jobs, 384 hashed files. All 77 tests pass. See
`research/anatel-product-register.md` for source scope and fields.

Full offline rebuild at `data/rebuilt-with-anatel.sqlite` now matches all thirteen
evidence/registry tables after excluding local paths and timestamps, including
all 190,852 Anatel rows and the 8/10 corroboration baseline. Comparison, validation
and exported catalogs are refreshed.

Canadian ISED REL follow-up: a complete public Amazon Data Services company
query adds four assertions and two new hardware identifiers, TE1A105 and TE1A195.
Both share certificate 25869-TE1A001 with TE1A001 and remain separate identities.
The labelled HTML table, displayed total and headerless CSV agree on all four
items. Only independently labelled identity fields are interpreted; all 44 CSV
columns are retained. This is a scoped company query, not a full Canadian mirror.
See `research/ised-radio-equipment.md` for the collector and fail-closed importer.

Current catalog: 1,263 identities, 1,645 assertions, 92 sources. Manifest: 31 jobs,
392 hashed files. All 83 tests pass. The offline rebuild at
`data/rebuilt-with-ised.sqlite` reproduces all thirteen evidence/registry tables,
with 10/10 searchable baseline queries and 8/10 corroborated identities. Catalog,
Amazon exports, validation reports and rebuild comparison are refreshed.

Canadian registry expansion: the collector now follows displayed pagination
links, and a version-2 importer checks contiguous ranges, table row counts,
company groups, stable totals and repeated pages before importing. Complete
captures cover Accton (167 HTML entries, 579 CSV radio-configuration rows),
Edgecore (4/29) and Oracle (12/16). Celestica has an explicit zero result for
that company-name query, retained as a scoped source observation.

The imports add 162 Accton Technology assertions, four Edgecore assertions and
twelve Oracle assertions. Five Accton Wireless Broadband rows remain outside
the target group. Fifteen Accton HTML entries have no exact CSV counterpart;
their labelled identities remain intact with discrepancy notes. Six CSV rows
match no HTML identity and remain available in the original cached export.
Oracle's group heading and actual per-row company numbers are kept separate.
No missing certificate, model suffix or manufacturer relationship is inferred.

Catalog totals: 1,416 identities, 1,823 assertions, 96 sources. Manifest: 35 jobs,
429 hashed files. All 89 tests pass. The new offline rebuild at
`data/rebuilt-with-ised-expanded.sqlite` matches all thirteen evidence/registry
tables. Baseline remains 10/10 searchable and 8/10 corroborated; the full 7760
ordering code and bare AZ4444 label remain gaps. Updated source scope and
discrepancy details are in `research/ised-radio-equipment.md` and
`data/exports/ised-query-coverage.json`; catalog and validation exports are current.

Public UL Accton expansion: the abridged AZOT7.E141136 Models section now supplies
191 reviewed platform identifiers with listing-holder evidence. The cache is a
structured web-tool factual extract, not original HTML or a complete UL listing.
AS/AZ/AIS/ECS/DMC/CSX/AMX/Wedge identifiers and explicit Minipack, Barefoot DNA
X1032C and ORV3 HSM names are extracted with source-text offsets. Voltage,
regional, version and core-count qualifiers remain attached; wildcard families
and other unparsed commercial names are not expanded. No physical OEM,
cross-brand identity or hyperscaler deployment is inferred.

This adds 118 catalog identities and 191 assertions. The public list still lacks
the full 7760-32X-A-AC-F ordering code and bare AZ4444-52X identity, so the baseline
remains 10/10 searchable and 8/10 corroborated. Source details and the bounded
extraction scope are recorded in `research/ul-accton-platforms.md`.

Current catalog: 1,534 identities, 2,014 assertions, 97 sources. Manifest: 36 jobs,
430 hashed files. All 94 tests pass. The offline rebuild at
`data/rebuilt-with-ul-accton.sqlite` matches all thirteen evidence/registry tables
and the baseline. Catalog, target exports, validation and rebuild comparison are
refreshed; `data/exports/ul-accton-platforms.json` contains the new evidence.

Unified local lookup: `python3 -m devicehunt lookup QUERY` now searches catalog
text, Anatel, ICASA, Bahrain and historical FCC grantee rows in one read-only
transaction. Unicode case-insensitive literal matching preserves punctuation
and suffixes. Counts, returned rows, truncation, source coverage and loaded
states remain separate by dataset. ISED/UL target assertions are covered through
the catalog. No cross-source identities or manufacturer relationships are inferred.

The CLI supports per-dataset limits, counts-only output and structured JSON.
ICASA review-required cells remain unparsed; duplicate Anatel rows remain
separate. Exact catalog text matches do not become validation passes, and
marketplace evidence stays labelled. The full 7760 query returns only its
seller-sourced catalog identity; AZ4444 returns the existing variants in four
datasets. See `research/local-lookup.md` and the two saved lookup examples.

All 101 tests pass, including literal punctuation, Unicode, source separation,
truncation counts, missing-source states and unchanged-file checks. This turn
does not change imported evidence: the catalog remains 1,534 identities and
2,014 assertions from 97 sources, with the existing verified offline rebuild.

Meta first-party networking evidence: four cached engineering articles add five
platform assertions. Edge-Core Minipack has a build-partner relationship;
Celestica Wedge 400, Wedge 400C and Minipack3 and Accton Minipack3N have explicit
manufacturer relationships. Historical deployment, announced plans and design
lineage remain separate. No unit provenance or cross-brand alias is inferred.
See `research/meta-networking-evidence.md` for dates, ASICs and module details.

Current catalog: 1,539 identities, 2,019 assertions, 101 sources; 37 import jobs
and 435 hashed files. All 106 tests pass. The offline rebuild at
`data/rebuilt-with-meta-networking.sqlite` matches all thirteen evidence/registry
tables. Catalog, target exports, validation, Meta search results and comparison
exports are refreshed. The ten baseline queries still return results and eight
identities are corroborated. All four Oracle entries pass; the full 7760 ordering
code and bare AZ4444 model remain gaps. Broad historical inventory and FCC filing
coverage remain incomplete.

OCP catalog expansion: twelve selected official product pages supply thirteen
model assertions through a reviewed factual web-tool capture. Original HTML
downloads returned 403; retrieval method and limited scope are recorded. Four
new identities preserve explicit suffixes: AS7816-64X-B, AS7816-64X-F,
AS5915-18X-O and AS7316-26XB. The first two have opposite airflow directions in
their page headings. Provider relationships remain separate from manufacturers;
associated design names and stale URL slugs never become aliases. See
`research/ocp-product-evidence.md` and `data/exports/ocp-products.json`.

Current catalog: 1,543 identities, 2,032 assertions, 102 sources. The manifest
has 38 jobs and 436 hashed files. All 110 tests pass. The offline rebuild at
`data/rebuilt-with-ocp.sqlite` matches all thirteen evidence/registry tables and
the validation baseline. Catalog, target exports, specifications and validation
exports are current. Baseline remains 10/10 searchable and 8/10 corroborated;
neither exact-code gap closed. OCP capture coverage is selected pages only.

Full NTRA register: all 760 pages now populate `ntra_equipment` with 47,302
positional rows. The printed sequence is exactly 1 through 47,302, once each.
47,290 rows have readable identification fields; twelve undecodable rows retain
raw cells and require review. All 119 earlier target-brand rows are present.
Fifteen AWS-branded rows on visually checked page 65 include Hopper26SF,
Atlanta variants, SCOUT01/02 and CubScout. These remain register-brand evidence;
no physical manufacturer or legal entity is inferred. Linked models and
parenthetical text remain intact. See `research/ntra-full-register.md`.

The new `ntra` command and six-dataset `lookup` expose the full local register.
Lookup now accepts null raw cells and handles missing tables in older databases
without migration or writes. Exports include the full register, exact AWS-brand
rows and an Atlanta lookup example. Schema version is 7.

Current catalog: 1,543 identities, 2,032 assertions, 103 sources. The manifest
has 39 jobs and 438 hashed files. All 114 tests pass. The offline rebuild at
`data/rebuilt-with-ntra-register.sqlite` matches all fourteen evidence/registry
tables and the baseline. Validation remains 10/10 searchable, 8/10 corroborated.
Full FCC device-filing coverage remains unavailable; this turn's source research
found no verified usable bulk export. Full historical inventory remains open.

IEEE public address registries: 58,639 assignment rows now populate
`ieee_assignments` (40,253 MA-L; 6,606 MA-M; 7,205 MA-S; 4,575 IAB). Two
downloads of each official CSV agree byte-for-byte. Exact organization names,
addresses and duplicate prefixes are preserved; no device identities or
manufacturer/operator relationships are inferred. The 78 target-name substring
leads are exported separately. There is no exact Amazon Data Services name in
these files; other Amazon entities remain separate. See
`research/ieee-address-registry.md` for source scope and examples.

The read-only `mac` command accepts full 48-bit labels and returns all rows at
the longest matching prefix. Local, group, broadcast and unspecified addresses
receive no hardware assignee inference. The unified `lookup` now covers seven
datasets. Schema version is 8. Source coverage is the four public files, not
private registrant identities or an independently audited IEEE database total.

Comparing the FTS index as well as the evidence tables exposed an existing
import-order issue: 87 entries had identical words in different order. Search
index construction now sorts devices, source records and field values. A
regression test verifies identical FTS text and row IDs despite reversed import
and fact insertion order. All 123 tests pass (`data/exports/tests-ieee.log`).

Current catalog: 1,543 identities, 2,032 assertions, 104 sources, plus the
separate raw registers. The manifest has 40 jobs and 448 hashed files. The fresh
offline build `data/rebuilt-with-ieee-stable-search.sqlite` matches all fifteen
evidence/registry tables and the full FTS index, excluding only import-time and
local-cache metadata. The earlier `data/rebuilt-with-ieee.sqlite` is superseded:
it predates the deterministic-index fix. Comparison and validation exports are
refreshed. Baseline remains 10/10 searchable and 8/10 corroborated; exact
`7760-32X-A-AC-F` and bare `AZ4444-52X` remain gaps. Full FCC device filings and
exhaustive historical manufacturer inventories remain incomplete.

FCC company-index follow-up: original public HTML from the accessible FCCID.co
mirror now supplies 171 entries in `fcc_id_index`: HED 146 across six pages,
YZK 20, A4H four and 2AVPL one. MRF and NVN have cached company-not-found pages,
recorded as mirror coverage gaps. Grantee names and card boundaries, page counts,
unique IDs and file hashes are checked before any import changes a source.
Streamed HTML fragments are reconnected without executing JavaScript. The
eight-dataset read-only lookup includes these rows and all six query scopes.

This mirror is demonstrably incomplete: FCCID.io exposes larger company counts,
and the cached Amazon list omits the already documented CubScout entry. Dates
can differ between mirrors and remain verbatim in `listed_date`. No FCC product
code is turned into a guessed model alias. Index entries do not populate
`fcc_applications`, `fcc_exhibits`, device identities or inventory validation.
See `research/fcc-company-index.md` and the exported `fcc-id-index.csv`.

All 130 tests pass (`data/exports/tests-fcc-index.log`). Schema version is 9.
The catalog remains 1,543 identities and 2,032 assertions, now with 110 sources.
The offline manifest has 41 jobs and 461 hashed files. The fresh database
`data/rebuilt-with-fcc-index.sqlite` matches all sixteen evidence/registry tables
plus the full FTS index and row IDs, excluding only import-time/cache metadata.
Comparison, baseline validation, catalog report and affected lookup exports are
refreshed. All ten baseline queries still return results; eight identities are
corroborated. Both exact model gaps and complete FCC/historical coverage remain
open. The accessible mirror's detail pages are a potential next filing-evidence
source; no application/exhibit completeness is inferred from its company index.

FCC PDF archive: schema 10 adds `documents`, `document_pages`, the joined
`document_text` view and a separate FTS5 `document_search` index. Eleven public
PDFs linked by the cached TE1A001 mirror detail page are archived with original
bytes and hashed page-text captures. There are 74 pages, 68 with embedded text
and six requiring visual reading; no OCR was performed. The new read-only
`documents` command returns page excerpts and local PDF paths. General lookup
now covers nine datasets. Document hits never automatically become catalog
identities or inventory corroboration. See `research/fcc-document-archive.md`.

All 136 tests pass (`data/exports/tests-fcc-documents.log`). The catalog remains
1,543 identities and 2,032 assertions, now with 111 sources. The offline manifest
has 42 jobs and 485 hashed files. `data/rebuilt-with-fcc-documents.sqlite` matches
all eighteen evidence/registry tables and both complete FTS indexes, including
row IDs and excluding only import-time/cache metadata. Baseline validation also
matches: all ten queries return results; eight identities are corroborated.
All four Oracle models pass. Exact 7760 and bare AZ4444 remain open gaps.

Additional Monitron manufacturer and service-availability sources are cached
but not imported, as recorded in the archive research note. A system Python
invocation now encounters an Xcode license prompt; verification used the bundled
Python runtime without changing system licensing or configuration. Full FCC
filing coverage and exhaustive historical manufacturer inventories remain open.

AWS Monitron: the cached safety index and four original PDF guides now supply
five model-specific assertions. TE1A001 and TE1A105 have explicit manufacturer
blocks; TE1A195 is connected by the exact index link to the TE1Ax9z configurable
family guide. No additional variants or aliases are generated. TG1A001 and
TG1A002 are new identities with Amazon Data Services as compliance declarant,
not inferred manufacturer. Reviewed EU guide pages name each gateway exactly.
A separate source attaches the October 31, 2024 service notice to all five
models, with only an AWS documentation-vendor role and no unit-enrollment,
transferable-warranty or deployment claim. See `research/aws-monitron.md`.

All 140 tests pass (`data/exports/tests-monitron.log`). The catalog now contains
1,545 identities, 2,042 assertions and 116 sources; schema remains 10. The offline
manifest has 43 jobs and 496 hashed files. `data/rebuilt-with-monitron.sqlite`
matches all eighteen evidence/registry tables and both FTS indexes including
row IDs, excluding only import-time/cache metadata. Baseline results also match:
10/10 searchable, 8/10 corroborated, all four Oracle models passing. Exact
7760-32X-A-AC-F and bare AZ4444-52X remain gaps. Catalog, target exports, Amazon
export, lookup examples, role-filter examples and rebuild reports are refreshed.
Full historical manufacturer inventory and full FCC coverage remain unproven.

Celestica 2025 catalog: the original eight-page PDF and hashed page capture now
replace the single web-only SC4200 seed. All 17 product cards on pages 4-5 produce
19 exact model assertions. Grouped DS4000/DS4001 and ES1010/ES1050 descriptions
stay qualified, with no inferred aliases, per-model allocation of speed options,
physical factory or hyperscaler deployment. Form factors and complete captions
are searchable. Both product pages were visually reviewed, including the
SC4200 embedded-text duplication. The source is removed from curated seeds so
routine seed imports cannot overwrite the stronger cached evidence. See
`research/celestica-2025-catalog.md` and the 19-row CSV export.

All 144 tests pass (`data/exports/tests-celestica-catalog.log`). The catalog has
1,545 identities, 2,060 assertions and 116 sources. All 19 models were already
known; this expands verified source coverage rather than unique model count.
Schema remains 10. The manifest now has 44 jobs and 499 hashed files. The fresh
`data/rebuilt-with-celestica-catalog.sqlite` matches all eighteen evidence/registry
tables and both FTS indexes including row IDs, excluding only import-time/cache
metadata. Baseline results match too: 10/10 searchable, 8/10 corroborated. Exact
searches revisited both Edge-Core gaps without finding sufficient independent
identity evidence. Neither gap was waived. Historical inventory and full FCC
filing coverage remain open. Catalog, target/specification exports, lookup
examples, inventory validation and rebuild comparison reports are refreshed.

FCC detail-page expansion: all 171 IDs in the cached company index now have
original mirror detail HTML, collected sequentially with cached-file reuse and
explicit failure recording. All requests succeeded. Import adds 171 raw page
records, 1,493 public exhibit links and 1,092 mirror text excerpts, of which 311
are explicitly truncated. Excerpts are not archived PDFs, and unflagged text is
not assumed complete. Excerpt ordinals are not converted to document IDs, pages
or guessed exhibit-link associations. Display dates/frequencies remain verbatim.
No new model, manufacturer, deployment or baseline corroboration is inferred.

Schema 11 adds `fcc_detail_pages`, `fcc_detail_links`, `fcc_text_excerpts` and
joined `fcc_excerpt_text`. The eleven-dataset lookup searches details and
excerpts, provides bounded Unicode-aware snippets and preserves truncation flags.
The full cached text and links are available to SQL. The importer checks exact
coverage against the imported FCC company index and validates all page hashes
and parsed identities before source replacement. Explicit unavailable records
are supported. See `research/fcc-detail-pages.md`, `fetch_fcc_details.py` and
exported detail-summary, exhibit-link and lookup-example files.

All 149 tests pass (`data/exports/tests-fcc-details.log`). The catalog remains
1,545 identities and 2,060 assertions, now with 117 sources. The offline manifest
has 45 jobs and 672 hashed files. `data/rebuilt-with-fcc-details.sqlite` matches
all twenty-one evidence/registry tables and both FTS indexes, including row IDs
and excluding only import-time/cache metadata. Baseline results also match:
10/10 searchable, 8/10 corroborated; both exact Edge-Core gaps remain. Report,
lookup examples, validation and comparison exports are refreshed. Full FCC and
historical manufacturer coverage remain unproven. The newly cached filing
excerpts provide model/OEM leads for review against original documents; one
example is the HEDEAP115 manual naming EAP115, EAP115a and EAP115b.

FCC model-field follow-up: original-PDF downloads for eleven test-report links
returned HTTP 403, as did an independently published FCC.report copy of Oracle
report 4621145. Attempts are recorded in `data/raw/fcc-identity-reports/`; no PDF
bytes or manufacturer/factory verification is claimed. Cached report excerpts
still support a distinct applicant-level expansion: thirty reviewed report covers
supply 38 exact model assertions. The capture binds HTML and excerpt hashes,
FCC ID, report number, MODEL NO and APPLICANT fields. It excludes ambiguous
space/footnote/OEM-subfield strings and peripheral tables, preserves comma-listed
identities separately, and deduplicates repeated copies of one filing/report.
No model aliases, retail-brand equivalences, manufacturer or deployment claims
are inferred. See `research/fcc-report-cover-models.md` and the role CSV export.

Eight new Accton-applicant namespace identities are added: 3CRWE53196, A300-2,
GW-AP11X, LR11b, RBT3K-AG-G, SMC7004WFW, WN3301G and WN3501AR. Catalog totals
are 1,553 identities, 2,098 assertions and 118 sources. Schema stays 11. All 153
tests pass (`data/exports/tests-fcc-cover-models.log`). The manifest contains
46 jobs and 673 hashed files. `data/rebuilt-with-fcc-cover-models.sqlite` matches
all twenty-one evidence/registry tables plus both complete FTS indexes including
row IDs, excluding only import-time/cache metadata. Baseline results match:
10/10 searchable, 8/10 corroborated. Both exact Edge-Core gaps, original-document
manufacturer review and complete historical/FCC coverage remain open. Catalog,
targets, lookup examples, validation and rebuild comparison are refreshed.

## OcNOS hardware compatibility expansion

The public IP Infusion HCL and its linked platform API are now cached and
importable offline. The old October 2022 PDF URL redirects to this newer HTML
catalog; no historical PDF was imported. All 44 API identities and core
specifications reconcile with the complete static table. The 21 exact target
platforms (20 Edgecore, Celestica DS1000) add scoped compatibility assertions,
including software SKUs, firmware/revision context, ASICs, ports, approximate
power/weight, and dimensions. Explicit commercial aliases remain source-backed.
No physical OEM, hyperscaler use or used-unit software entitlement is inferred.

The AS5915-18X lifecycle schedule is recorded as IP Infusion HCL dates, while
empty lifecycle fields are not turned into active-support claims. Conflicting
raw status/maxSpd fields remain unpromoted; details and SQL examples are in
`research/ocnos-hardware-compatibility.md`.

Current catalog: 1,566 identities, 2,119 assertions, 119 sources. All 157 tests
pass. The 47-job manifest verifies 676 local files. A fresh offline rebuild at
`data/rebuilt-with-ocnos.sqlite` matches all 23 compared tables/indexes and the
owned-hardware validation results. Exports and the offline report are refreshed.
The baseline remains 10/10 searchable, 8/10 corroborated. Full-label gaps for
7760-32X-A-AC-F and AZ4444-52X, broader historical coverage, and complete FCC
archive coverage remain open; the overall goal remains active.

## Full TEC register and baseline follow-up

The complete cached India TEC table is now independently queryable in
`tec_equipment`: 2,331 rows, all applicants and declared OEMs, both model fields,
certificate data, raw cells and date-review status. The 69 existing target-model
assertions retain their original evidence rules. Unified lookup now searches
12 datasets. Schema version 12 adds the register; reading an older database
reports it as not loaded without migrating or writing that database.

Blank OEMs (517 rows), one blank primary model, repeated certificate IDs (265
IDs repeat), and ten malformed expiry cells are preserved. Malformed expiry
values become NULL parsed dates with explicit review status, never fabricated
approval or support dates. Complete CSV and review-only exports are saved.
See `research/tec-full-register.md` for source scope and runnable SQL.

Additional exact-label research still leaves 7760-32X-A-AC-F and AZ4444-52X
unresolved. UL BR4384 exposes only the suffixed AZ4444 variants; a truncated
summary is not a bare-model assertion. The owned baseline remains 10/10
searchable and 8/10 corroborated. Broader FCC coverage also remains incomplete.

Validation: all 162 tests pass. The 48-job rebuild checks 676 hashed inputs;
`data/rebuilt-with-tec-register.sqlite` matches all 24 compared tables/indexes
and the baseline results. Lookup examples and the offline report are refreshed.
There are 1,566 catalog identities, 2,119 assertions and 120 sources, with the
full TEC register counted separately from devices. The overall goal stays active.

## Local auction-title matching

`match-listing` now accepts one supplied title or a local CSV batch. It searches
stored model names and explicit model/alias/part-number identifiers, with source
records attached to every match. Complete model mentions, identifier mentions,
and prefix/suffix variant leads remain distinct. Vendor filters apply to the
matching evidence record, preventing seller aliases from borrowing a stronger
source's affiliation. Different brands and revisions remain separate candidates.

The command preserves unmatched batch rows, supports JSON/table/CSV output,
reports totals and truncation, and exposes condition/accessory context terms
without pretending to identify the item being sold. Case and dash typography are
normalized; suffixes are never stripped. No network calls or catalog imports
occur. `docs/listing-matching.md` describes boundaries and usage.

Six example inputs (one historical cached seller heading and five explicitly
illustrative titles) and their JSON/CSV outputs are saved. The complete 7760
code remains seller-only, AZ4444 is variant-only, Oracle E5-2C finds its documented
identifier, and unknown text remains unmatched. These are not live offers or
source evidence. The catalog's SHA-256 stayed identical through the production
example run. All 171 tests pass, including read-only CLI, batch input validation,
identity-boundary, ambiguity and same-record provenance tests. The baseline
remains 10/10 searchable and 8/10 corroborated. No importer, schema or data changed
in this step; the prior verified rebuild remains the catalog snapshot.

The full objective remains active: complete historical and FCC coverage and the
two exact-label corroboration gaps are still unproven.

## Cross-brand factory evidence from CB certificates

Cisco's original six-page certificate PDF is now cached with embedded page text
and checked dependencies. The two visually reviewed 2023 M3 certificates on
pages 1-4 add Cisco NCS1004 and NCS1014, with Cisco retained as manufacturer and
brand and Celestica Thailand plus Fabrinet retained as `listed_factory` parties.
This is certificate-scoped factory evidence, not proof of an individual unit's
factory, hyperscaler use, or current certificate validity. The older 2021 pages
remain archived but outside this selected import.

Separate AC/DC certificates and their model-specific ratings are preserved;
NCS1014's additional AC ratings are read from the correct continuation page.
The open-ended suffix qualifier stays a fact and creates no invented variants
or aliases. Celestica searches and auction-title matching can now find these
Cisco-branded products through the explicit factory relationship. See
`research/cisco-celestica-factory-evidence.md` and exported facts/examples.

All 175 tests pass. Catalog: 1,568 identities, 2,123 assertions, 121 sources.
The 49-job manifest verifies 679 files; a new offline rebuild at
`data/rebuilt-with-cb-certificates.sqlite` matches all 24 compared tables/indexes
and the baseline. Exports and the offline catalog are refreshed. Owned-hardware
validation remains 10/10 searchable and 8/10 corroborated; complete historical
coverage, the two baseline gaps and full FCC archive coverage remain open.

## Commercial codes in Accton's public UL listing

Expanded the existing cached UL Models-section parser by 34 complete
QFX/EX/ACX/JNP/SRX/JCP identifiers. These now work in SQL, text search and local
auction-title matching, including EX4100-48T, QFX5120-32C and SRX1600. The source
now contributes 225 assertions. The cached capture and hash remain unchanged;
this is broader extraction from the same abridged public listing, not a fresh
retrieval or full UL inventory.

Each record retains character offsets and extraction scope. Accton remains a
listing-holder namespace only, with that limitation added as an explicit fact.
No retail brand, physical factory, cross-brand alias or hyperscaler association
was inferred. EX4100-F variants, ACX7024X and separate JNP/ACX codes retain their
full identities. Primary Juniper documentation was located for brand research;
a secondary factory-certificate copy and a login-gated compliance portal were
not imported. See `research/ul-accton-platforms.md` for details and SQL examples.

All 178 tests pass, including complete-token boundaries, retained variants,
source-role separation, actual auction matching and rejection before mutation.
Catalog: 1,602 identities, 2,157 assertions, 121 sources. The 49-job, 679-file
manifest rebuilt successfully into `data/rebuilt-with-ul-commercial.sqlite`;
all 24 compared tables/indexes and baseline results match the live database.
Catalog, source exports, lookup examples and offline HTML are refreshed.
`data/exports/ul-accton-listing-example.json` is an illustrative supplied title,
not a live offer or imported evidence.

Baseline remains 10/10 searchable, 8/10 corroborated. Complete historical/FCC
coverage and exact-label corroboration for 7760-32X-A-AC-F and AZ4444-52X remain
open; the overall goal is active.

## AWS Panorama AMHT-1 and service closure

An exact-label search left both remaining baseline gaps unchanged, but broader
Amazon manufacturer research found a missing device in an original AWS manual:
AMHT-1, AWS Panorama Appliance (Hub). The original eleven-page PDF and embedded
text are cached. Pages 3, 8, 9, 10 and 11 were visually reviewed; image-only
declaration pages remain archived without claimed OCR. The page-10 UK declaration
provides Amazon Data Services as legal manufacturer and AWS as brand, while page
3 independently names Amazon Development Center US as FCC responsible party.
Those roles remain separate, and no FCC identifier or physical factory was
inferred. The old safety index now yields a generic AWS page; it is retained
but not used for evidence.

A separate cached AWS notice supplies the May 20, 2025 new-customer closure,
May 31, 2026 service end, explicit device/cloud dependency and Nvidia Jetson
Xavier platform statement. Alternative firmware and repurposing remain
unverified. The importer creates one device and three assertions from two
sources, with all dependencies and source statements checked before replacement.
See `research/aws-panorama.md`, `panorama-device.json` and service-fact exports.

All 182 tests pass. Catalog: 1,603 identities, 2,160 assertions, 123 sources.
The 50-job manifest verifies 684 files; `data/rebuilt-with-panorama.sqlite`
matches all 24 compared tables/indexes and the owned-hardware baseline. Exports
and offline HTML are refreshed, and an illustrative auction-title match finds
AMHT-1. Baseline remains 10/10 searchable and 8/10 corroborated. Complete
historical/FCC coverage and the two remaining exact-label gaps are still open.

## Full-text archive for the other cached PDFs

All thirteen non-FCC original PDFs currently cached directly in `data/raw` now
have page-level offline search through the new `reference-documents` importer.
This adds 1,765 pages from the complete ICASA/NTRA registers, six Monitron guides,
two Outposts declarations, Panorama's manual, Celestica's 2025 catalog and Cisco's
NCS certificates. Seven existing captures were reused and six new embedded-text
captures generated. No network refresh or OCR was performed.

The shared `documents` command now searches vendor, regulator and FCC PDFs and
supports exact `--source` filtering alongside `--fcc-id`. Each result retains
original URL, hash, page number and local PDF path. `lookup` also sees the added
pages. The reference importer creates no device identities, aliases, company
roles or baseline passes; reference documents leave their FCC-ID field empty.
Both document importers now rebuild the shared FTS index in fixed order so its
row IDs survive different refresh orders reproducibly.

The combined archive is 24 PDFs / 1,839 pages, of which eight have no embedded
text. Other pages may contain only headers with image-only body content.
Malformed multilingual glyphs remain unaltered. Real-data checks found that
NTRA extracts some identifiers with internal spaces: `CS8210` does not find the
page that contains `CS 8210-32X-DC-11`; spaced tokens or the company name do.
This limitation and the distinct structured-registry searches are documented
in `research/reference-document-archive.md`, with saved working examples.

All 188 tests pass, including archive replacement, stale-source removal,
FCC-collection preservation, no inferred identities, read-only CLI filtering,
dependency validation and stable FTS row IDs. The 51-job, 693-file manifest
rebuild at `data/rebuilt-with-reference-documents.sqlite` matches all 24 compared
tables/indexes and the baseline. Exports and offline HTML are refreshed.
Catalog remains 1,603 identities and 2,160 assertions; sources total 136,
including thirteen newly indexed document sources. Baseline remains 10/10
searchable and 8/10 corroborated. The full objective remains active, with the
two exact-label gaps and complete historical/FCC coverage still unproven.

## Snowcone declaration identities and short auction identifiers

The public Italian Snowcone safety PDF includes English declarations for AF1
(data storage device) and the exact unsplit scope AF1 BD1 (dongle), naming
Amazon Data Services as legal manufacturer and AWS as brand. Both are imported
with page-level provenance; no factory, FCC ID, alias, deployment or current
certificate validity is inferred. Pages 7, 8 and 9 were visually reviewed.
Graphical declaration numbers and signature dates are not promoted from the
incomplete embedded text. See `research/aws-snowcone.md`.

A separate September 2022 family guide, linked by the cached AWS safety index,
provides dated return-to-AWS context for AF1. It remains a historical source
statement, not a determination of the ownership, rights or provenance of any
auction unit. It is not applied to the separately named dongle. No request was
sent to AWS for additional declarations. Both original PDFs and text captures
are now in the reference archive, bringing it to 26 PDFs and 1,852 pages.

The auction matcher now offers `--include-short-models` for three-character
alphanumeric identifiers containing both letters and digits. This makes AF1
searchable without changing the normal four-character threshold. Whole-token
boundaries, suffix-variant leads, evidence-scoped vendor filters and read-only
behavior are retained. Pure words/numbers and one- or two-character names remain
excluded. The illustrative `snowcone-listing-example.json` finds AF1 correctly;
no live offer or purchase was inferred.

All 193 tests pass. The 52-job manifest hashes 699 files and rebuilds into
`data/rebuilt-with-snowcone.sqlite`; all 24 compared tables/indexes and baseline
results match. Catalog: 1,605 identities, 2,163 assertions, 140 sources. Exports
and offline HTML are refreshed. The baseline remains 10/10 searchable and 8/10
corroborated; exact-label gaps and complete historical/FCC coverage remain open.

## Offline company-role filtering

The offline catalog now offers all roles present in its embedded snapshot.
Company and role must match the same affiliation, consistent with SQL/CLI
search: Celestica as a listed factory cannot borrow another company's
manufacturer role. The page explains this scope and retains linked source
statements. Three JavaScript behavior tests exercise the actual inline script
with a minimal DOM: association matching, combined filters, empty snapshots
and pagination reset. All three and the 14 catalog tests pass. This verifies
behavior, not browser layout; no visual browser check was performed.

The HTML was regenerated and baseline validation rerun: all four Oracle labels
still pass, with 10/10 searchable and 8/10 corroborated overall. This update
changes no imported evidence or database schema; the last verified data rebuild
remains `data/rebuilt-with-snowcone.sqlite`.

## Celestica public UL platform expansion

The public search-index Models section for AZOT.E362364 supplies 163 reviewed
non-CS identifiers beyond the existing 25-model CS capture. A separate importer
preserves complete whitespace qualifiers, source-text offsets and legal
listing-holder evidence. It creates no inferred brand, factory, manufacturer,
cross-brand alias or hyperscaler deployment. The source page returned 403 when
opened; the cache is explicitly a factual search-index extract, not original
HTML, and no further page retrieval was attempted. The final IcePack Canister
phrase remains unparsed pending grouping review. See
`research/ul-celestica-platforms.md`.

163 source assertions add 138 catalog identities. Current totals are 1,743
identities, 2,326 assertions and 141 sources. Five new importer tests bring the
Python suite to 198 passing tests; all three offline-page behavior tests also
pass. The 53-job manifest hashes 700 files and rebuilds into
`data/rebuilt-with-ul-celestica.sqlite`. All 24 compared tables/indexes and the
inventory results match the live database, ignoring only retrieval timestamps
and cache-path metadata. Target exports, local lookup examples and offline HTML
are refreshed; a 163-row source export and an illustrative listing-match example
are saved. No live seller offer is implied.

Exact-label searches again leave the two remaining baseline gaps unresolved.
All ten identifiers are searchable, eight are corroborated, and all four Oracle
entries pass. Full historical inventory and FCC filing coverage remain unproven.

## Accton remaining complete commercial identifiers

The unchanged cached AZOT7.E141136 Models section now yields 212 additional
complete commercial identifiers, raising this source from 225 to 437 assertions.
The expansion adds 201 previously absent catalog identities. It includes
JL/R/S part codes, concrete FortiSwitch-AX/FS-AX/AX names, GigaVUE names and
Vision variants. Every assertion retains its extraction scope and exact source
offsets. No retail brand, manufacturer, factory, cross-brand alias or deployment
is inferred from a name. Numeric-only labels, ambiguous 1FINITY groupings and
wildcard templates remain unparsed; the residual text is saved in an extraction
audit to make that limitation inspectable.

58 names preserve an immediately adjacent RMN annotation as a searchable fact.
The importer does not assign annotations across comma-separated lists or treat
them as identities. Tests demonstrate that an RMN text hit cannot pass the
inventory identity check. Unknown qualifiers fail before source mutation.
Four new tests bring the Python suite to 202 passing tests.

Current catalog: 1,944 identities, 2,538 assertions and 141 sources. The 53-job,
700-file manifest rebuilds into `data/rebuilt-with-ul-accton-extended.sqlite`.
All 24 compared tables/indexes and baseline results match. Source/target exports,
lookup examples and the offline catalog are refreshed; a new listing example
matches JL680A using the Accton listing-holder evidence. Baseline remains 10/10
searchable and 8/10 corroborated. No new source retrieval was needed; source
timestamps and hashes remain those of the existing captures. Full historical
inventory and FCC coverage are still unproven, and both exact-label gaps remain.

## Complete published PCI ID snapshot and local decoding

The 2026.09.25 PCI ID Repository snapshot is cached at immutable commit
`fe4b3f80e3234847cc7e8b8a32496cb94d4a6e7e`, with original data, commit metadata
and license README. All 42,438 published name entries are imported into the
separate `pci_entries` table: 2,518 vendors, 21,607 devices, 18,089 subsystems,
22 classes, 118 subclasses and 84 programming interfaces. There are no unknown
extension lines in this file. Six undefined subsystem vendor IDs remain visible
without invented names. Completeness applies to the downloaded file, not all
real PCI hardware or official PCI-SIG allocations.

The new read-only `pci` command resolves vendor/device pairs, optional subsystem
pairs and caller-supplied class codes. Exact subsystem entries precede published
generic fallback names. Vendor and subsystem namespaces are distinct. The
source's Amazon.com name is not mapped to Amazon Data Services. These entries
create no chassis identities, manufacturer affiliations or baseline passes.
Unified local lookup now covers thirteen datasets. Schema version is 13.
See `research/pci-id-repository.md` for SQL and usage examples.

Seven new tests bring the Python suite to 209 passes. Read-only old-schema
behavior, hierarchy, malformed input, duplicate rejection, hash/commit checks,
transaction ownership and preservation of unknown vendor references are covered.
The 54-job manifest hashes 704 files and rebuilds at `data/rebuilt-with-pci.sqlite`.
All 25 compared tables/indexes and baseline results match. Catalog identities
remain 1,944, assertions 2,538; sources total 142. PCI exports, local lookup
examples and offline HTML source coverage are refreshed. Baseline remains
10/10 searchable and 8/10 corroborated.

Independent primary-source research found Accton factory evidence for MX100-HW
and approved-manufacturer cable part numbers in an Aruba table, but direct PDF
downloads returned HTTP 403. These leads are documented in
`research/accton-manufacturer-leads.md` and remain outside imported assertions;
no alternate retrieval route was used. Full historical inventory, complete FCC
filings and the two exact-label gaps remain unresolved.

## Saved machine PCI inventory decoding

The new `pci-report` command reads numeric `lspci -D -vmm -n` records and decodes
their vendor/device, subsystem and class IDs entirely locally. One read
transaction covers the report. Input observations, reported drivers/revisions,
repeated Module tags and future tags remain distinct from source evidence;
nothing is imported or inferred about the chassis, manufacturer or operator.
Each PCI function retains its normalized slot. Missing programming-interface
codes stay missing, including in the extended four-or-six-digit `pci --class-code`
option. Full malformed input is rejected before lookup, and duplicate slots or
incomplete subsystem pairs fail explicitly. See `docs/pci-inventory.md`.

Five new tests bring the Python suite to 214 passes. A clearly marked synthetic
four-function example yields two named devices, one vendor-only result and one
unknown ID, with JSON and CSV examples saved. A whole-file database hash check
confirms the live-data report and baseline validation do not modify the database.
Baseline remains 8/10 corroborated. This is a read-only tool addition; no source
snapshot or schema changed, so the verified `data/rebuilt-with-pci.sqlite` data
rebuild remains applicable. Complete historical/FCC coverage and both exact-label
gaps remain open.

## WorkSpaces Thin Client document coverage

Collected the public AWS safety index and both its EU declaration and safety
guide, preserving their original bytes, link provenance and complete embedded
text. GA5Z9L now returns three local document-page hits. Visual review covers
both declaration pages and safety-guide pages 3–5. The declaration names
Amazon.com Services LLC; the guide names Amazon.com Services, Inc. Neither is
merged into Amazon Data Services, and no device assertions, aliases, FCC IDs or
baseline passes are inferred. See `research/aws-thin-client.md`.

The archive now has 28 PDFs, 1,859 pages and 1,851 pages with some embedded text.
Seventeen reference PDFs contribute 1,785 pages; eleven FCC PDFs are unchanged.
Catalog identities remain 1,944 and assertions 2,538; sources total 144.
The 54-job manifest hashes 709 files. A fresh offline rebuild at
`data/rebuilt-with-thin-client-documents.sqlite` passes integrity and reference
checks; all 25 compared tables/indexes and baseline results match the live data.
The 17 relevant archive, lookup and validation tests pass. No application code
changed. Document search examples, offline HTML, lookup exports and baseline
reports are refreshed. Baseline remains 10/10 searchable and 8/10 corroborated;
complete historical/FCC coverage and the two exact-label gaps remain open.


## Historical NTRA snapshot and unsplit brand/model search

Imported all 29,208 rows from the original December 4, 2022 NTRA PDF (1,218
pages). The new `ntra-historical` importer checks hashes, headers, three-column
shape, page order, contiguous sequence and expected counts before replacing its
source. BRAND & MODEL remains one raw cell, with separate named brand/model
fields NULL. It coexists with the newer five-column snapshot. No catalog
identity, manufacturer or AWS deployment is inferred. Four new tests cover
combined-cell discovery, malformed sequences, dependency failures and refresh
isolation; the full 218-test suite passes.

There are 146 target-prefix historical rows. A literal comparison finds 135
exact newer brand/model strings and eleven differences. Ten differences have
trailing identifiers in newer linked-model cells. The remaining old label is
ORACLE SUN SG-XPCIE2FC-QF8-Z; Oracle service documentation independently names
that marketing part as an FC HBA, a lead for later component source collection.
Comparison candidates are preserved, not merged into aliases. The complete
7760 ordering code and bare AZ4444 identity gaps remain open.

Raw NTRA coverage is now 76,510 snapshot rows across two dates, not unique
devices. PDF search totals 29 documents, 3,077 pages and 3,069 pages with some
embedded text. Catalog identities/assertions remain 1,944/2,538, with 146
sources. The 55-job, 713-file manifest rebuilds successfully at
`data/rebuilt-with-ntra-historical.sqlite`; all 25 compared tables/indexes and
baseline results match the live catalog. Lookup examples, offline HTML, baseline
exports and comparison reports are refreshed. Baseline remains 10/10 searchable
and 8/10 corroborated. See `research/ntra-historical-register.md`. Full historical
inventory and complete FCC filings remain unproven; the goal stays active.


## Oracle ZFS component options and printed part mappings

Cached three original Oracle service-manual HTML pages and added a strict
`oracle-components` importer for the Sun 7420, ZS3-2 and ZS3-4 NIC/HBA Options
tables. Thirty assertions cover fifteen marketing part numbers, adding fourteen
identities. Oracle remains documentation_vendor; no physical OEM, operator,
current support or universal interchangeability claim is made. Nine explicit
Mfg Part Number mappings remain source-scoped identifiers, separate from model
aliases and inventory-validation identity fields. The ZS3-2 multi-paragraph
row is parsed by aligned source paragraphs. Its SAS description for 7103791
differs from ZS3-4; both statements remain attached to their respective sources.

The SG-XPCIE2FC-QF8-Z adapter discovered in historical NTRA now has independent
Oracle component-option evidence and a working local listing-match example.
Numeric-only parts remain searchable with lookup/SQL but are excluded by the
existing listing matcher; this limitation is documented. All input pages are
validated before replacement. Five new tests bring the full Python suite to
223 passes. See `research/oracle-component-options.md`.

Catalog totals: 1,958 identities, 2,568 assertions and 149 sources. The 56-job
manifest hashes 717 files and rebuilds successfully at
`data/rebuilt-with-oracle-components.sqlite`. All 25 compared tables/indexes and
baseline results match. Target exports, local lookup examples, component options,
listing-match example, offline HTML and baseline reports are refreshed. Baseline
remains 10/10 searchable and 8/10 corroborated. Full historical/FCC coverage and
the two exact-label gaps remain unproven; the goal stays active.


## Explicit numeric identifiers in auction-title matching

Added `--include-numeric-identifiers` to match-listing for known model/eligible
identity fields containing 6–20 ASCII digits, optionally separated by hyphens.
The option is independent of short alphanumeric model matching and off by
default. Leading zeros and separators remain significant; longer suffixed codes
produce variant leads rather than exact matches. Small numbers, unrelated facts,
raw registry rows and source-scoped oracle_mfg_part_number mappings are not
promoted into matches. Each matching identifier retains its own vendor and
evidence tier. JSON/table/CSV output now reports both opt-in settings.

Three meaningful regression tests bring the full Python suite to 226 passes:
numeric boundaries/zeros/suffixes and option independence, same-record evidence
filtering and alias collisions, and read-only title/CSV operation. Live synthetic
Oracle examples produce one exact numeric mention (7103791), one suffix lead
and one unmatched leading-zero variant. Both conflicting Oracle description
sources remain visible on the exact match. The default still omits numeric
identifiers. Existing general examples and the Oracle component example are
refreshed. See docs/listing-matching.md and the new numeric examples in data.

A full-file database hash check before/after live example queries and baseline
validation confirms no database modification. Baseline remains 8/10 corroborated.
No source/schema changed; the verified rebuilt-with-oracle-components.sqlite
snapshot remains applicable. Full historical/FCC coverage and the two exact-label
gaps remain open.

## Taiwan NCC published equipment CSV

Added the complete downloaded NCC vv_IPCCDB.csv: 110,957 rows and all 21 source
fields in ncc_equipment (schema 14). The official dataset metadata displayed
2026-09-24 23:02; the snapshot was retrieved September 26. CSV bytes, a clearly
labelled factual metadata extract, license attribution and reviewed summary are
hashed in the import bundle. Company roles, duplicate certificate rows, blanks,
confidentiality labels, dates and validity flags stay verbatim. No identities,
company aliases, physical-factory or hyperscaler claims are inferred.

Counts include 110,446 distinct certificate numbers, 511 duplicate-certificate
rows, 65,056 blank manufacturers, 339 confidential identity rows, and validity
flags Y=110,248/N=709. Two Celestica mentions are unsplit multi-company BlackBerry
manufacturer cells; four Oracle hits cover MICROS devices. The Chinese query
智邦 returns 222 raw leads without automatically creating English company aliases.
Local lookup now covers fourteen datasets. See research/ncc-equipment-register.md
for attribution, SQL examples and limitations.

Three new regression tests cover preservation, refresh isolation, malformed and
changed input rejection, read-only search and old schemas. Updated the existing
schema migration expectation; all 229 Python tests pass. The 57-job manifest now
hashes 720 files. A fresh data/rebuilt-with-ncc.sqlite reproduces all 26 compared
tables/indexes and baseline results. Catalog totals remain 1,958 identities and
2,568 assertions; sources increase to 150. Lookup examples, offline source coverage
and inventory results are refreshed. Baseline remains 10/10 searchable, 8/10
corroborated, including all four Oracle labels. The two exact-label gaps and
complete historical/FCC coverage remain open.

Indonesia's public certificate count request returned an explicit forbidden
response, so that endpoint was stopped without interpreting denial as zero
coverage. The public page and failure are documented in
research/indonesia-certificate-access.md; no Indonesian registry data was imported.

## Taiwan BSMI published certificate XML

Added all 105,732 rows and seven fields of dataset 166852's downloaded XML to
bsmi_certificates (schema 15), with raw applicant, model annotations, expiry and
status values. Metadata describes current-year new certificates; downloaded rows
include historical expiry dates and multiple statuses, so no issue year or full
historical coverage is inferred. Original XML, factual metadata extract and
reviewed summary are hashed. The source license attribution is documented in
research/bsmi-certificates.md. There are 22 blank primary models; no inferred
manufacturer, factory or operator and no automatic model alias stripping.

Exact applicant selections give 41 Oracle Taiwan rows plus Accton SAF6101I.
Official Oracle and Accton pages corroborate the Chinese company names; these
remain applicant associations, not physical manufacturer evidence. The 42-row
audit is data/exports/bsmi-target-applicant-audit.json. Thirty-nine rows contain
model strings absent from the catalog, including WH1CL, WH2CL and annotated server
variants; they are future research leads. An English Oracle text search instead
finds a coffee-machine product, demonstrating why product-name matches must not
become vendor assertions. No new catalog identities are created this turn.

Local lookup covers fifteen datasets. Three additional tests cover XML structure
and entity rejection, verbatim preservation, duplicates, refresh isolation,
hash/count failures, false vendor inference and read-only current/old-schema
lookup. All 232 tests pass. The 58-job manifest hashes 723 files and successfully
rebuilds data/rebuilt-with-bsmi.sqlite. All 27 compared tables/indexes and baseline
results match. Lookup exports, offline source coverage and baseline results are
refreshed. Catalog totals stay 1,958 identities and 2,568 assertions, with 151
sources. Baseline stays 10/10 searchable and 8/10 corroborated. The two exact-label
gaps and comprehensive historical/FCC coverage remain unproven; goal stays active.

## Reviewed Taiwan target relationships

Added taiwan-targets, a separate importer over the cached NCC/BSMI snapshots and
newly cached primary Oracle/Accton company-name pages. Exact Chinese legal names
are mapped locally to target affiliations without changing global aliases.
Oracle's office page pairs its Taiwan legal name with Oracle Taiwan LLC, Taiwan
Branch; Accton's announcement explicitly pairs its Chinese name with Accton.
Every input hash, company association and full registry summary is checked before
replacing either target source. Raw registry tables remain unchanged.

The importer adds 269 source assertions (42 BSMI, 227 NCC), increasing identities
by 221 to 2,179 and assertions to 2,837. BSMI records use the applicant target as a
catalog namespace. NCC keeps the printed brand, including Intel 9260NGW with an
Oracle applicant and ARUBA WIRELESS NETWORKS AP-52-W with Accton as the NCC-named
manufacturer. Accton NCC roles comprise 184 applicant and 89 registry-manufacturer
claims, with overlap; Oracle has six NCC applicant claims. Multi-company cells
are not split, company substrings are not matched, annotated models remain whole,
and no physical factory, operator or current approval claim is inferred.

Three tests bring the suite to 235 passes, covering separate roles/brands, model
annotations, exact mappings and exclusions, company evidence and hash failures,
refresh behavior and no global alias changes. A live synthetic Oracle WH1CL
auction title now matches the BSMI-backed identity. The 59-job manifest verifies
726 files and rebuilds data/rebuilt-with-taiwan-targets.sqlite. All 27 compared
tables/indexes and baseline results match; sources now total 153. Baseline remains
10/10 searchable and 8/10 corroborated. The two exact-label gaps and full
historical/FCC coverage remain unproven. See research/taiwan-target-evidence.md.

## Oracle EcoDesign compliance-code relationships

A further exact-label pass for 7760-32X-A-AC-F and AZ4444-52X still yielded seller
entries or suffixed variants, without the missing independent exact-label proof.
Recorded the query scope in research/owned-hardware-gaps.md; no gate was relaxed.
Follow-up of new Oracle regulatory codes found four original Oracle EcoDesign PDFs.

Archived all four PDFs and ten pages of embedded text, and visually reviewed each
page 1 identity table. Added oracle-ecodesign importer: WH1CL relates to X9-2,
WH2CL to X9-2L, and ZU2XL relates separately to both X10M and X11M. Each document
names Oracle America Inc. as legal manufacturer. These are four source assertions
over three existing compliance-code identities, not unique aliases for marketing
models. Qualified DoC scope, raw manufacturer wording, year fields, source marks
and hashes remain intact. PSU rated output facts are explicitly separate from
wall draw; configuration-specific idle values remain only in the document archive.
No physical factory, operator, unit manufacture date or current support is inferred.

Three regression tests verify shared-code preservation, no false alias/baseline
pass, changed table/manufacturer/scope rejection, all-input validation before
replacement and repeat imports. Corrected an initially incomplete synthetic PDF
text fixture; the full suite now passes 238 tests. New live ZU2XL specs output
contains distinct X10M and X11M statements. See research/oracle-ecodesign.md.

The 60-job manifest verifies 735 files and rebuilds
data/rebuilt-with-oracle-ecodesign.sqlite. Catalog totals are 2,179 identities,
2,841 assertions and 161 sources. The document archive has 33 PDFs and 3,087 pages.
Baseline remains 10/10 searchable, 8/10 corroborated. The two exact-label gaps and
complete historical/FCC coverage remain unproven; the goal remains active.

All 27 compared tables/indexes and baseline results match the rebuilt database.
Target exports, lookup examples, offline catalog and validation results are refreshed.

## Manufacturer-focused vendor inventory

Added read-only vendor-inventory with manufacturer (default), factory and all
association scopes, optional target vendor, full totals, limits and JSON/CSV
exports. Manufacturer scope accepts manufacturer, registry_manufacturer,
declared_manufacturer and declared_oem only on imported hashed external-source
records. Factory listings remain separate. A requested company and role must
occur on the same record; printed brand, another company's manufacturer role,
applicant or listing-holder status cannot satisfy the selection. All scope keeps
seller, unhashed and unknown-role associations explicitly labelled with source
metadata. No new claims or catalog identities are created by this command.

Live manufacturer inventory contains 462 catalog identities: Accton 258,
Edge-Core 118, Celestica 40, Oracle 15 and Amazon Data Services 31. Factory scope
has two Celestica identities. All scope has 2,247 target/identity pairs over 2,179
identities. These are collected identity labels and may represent overlapping
physical hardware; none claims complete historical coverage. JSON groups evidence
by identity/target, CSV emits one matching role assertion per row. See
docs/vendor-inventory.md and data/exports/vendor-inventory-{manufacturer,factory,all}.

Five regression tests cover same-record role isolation, cross-brand factory
claims, source eligibility, unknown roles, limits/totals, read-only JSON/CSV and
caller transaction preservation. All 243 tests pass. The full database SHA-256 is
unchanged before/after all live inventory exports and baseline validation, so the
verified rebuilt-with-oracle-ecodesign.sqlite remains applicable. No source or
schema changed. Baseline remains 10/10 searchable and 8/10 corroborated; goal stays
active with the two exact-label gaps and broader historical/FCC coverage unproven.

Accton product/shop web retrieval reported 402 and the separately published
SAF6101I July 2022 datasheet returned 403 on direct download. Those requests were
stopped without alternate access routes, and no new vendor specifications were
imported. Details are retained in research/accton-manufacturer-leads.md.

## Celestica FCC mirror-index supplement

The public FCCID.io MRF company table exposes MRF13316C and MRF13316C2, absent
from the original FCCID.co capture. Added a clearly labelled factual web-tool
table extract and a hashed fcc-reviewed-index importer. The two rows preserve
the displayed 1997 dates, descriptions and permissive-change labels. Equipment
class was absent from this table and stays blank. FCC suffixes are not inferred
model names; no manufacturer/operator, detailed application or exhibit records
are created. The original mirror's zero-row not-found source remains intact.
NVN was unavailable and the MRF13316C2 detail retrieval was a cache miss; neither
was interpreted as zero filings. MRF13316C's separately viewable detail fields
were not mixed into the company-table capture.

Adding another mirror exposed an old assumption in the FCC detail importer and
download helper. They now share a selector restricted to their own original
company-index sources. A regression case checks both an overlapping FCC ID and
an additional grantee; another site's URL cannot replace the original or cause
its HTML to reach the wrong parser. Live selection still returns the original
171 rows. No new network detail downloads were attempted.

Four tests bring the full suite to 247 passes. The discovery index now has 173
rows; catalog identities/assertions remain 2,179/2,841, with 162 sources. The
61-job manifest hashes 737 files and rebuilds data/rebuilt-with-fcc-mrf.sqlite.
All 27 compared tables/indexes and baseline results match. FCC CSV, lookup
examples, offline catalog source coverage and baseline reports are refreshed.
See research/fcc-celestica-supplement.md. Baseline stays 10/10 searchable and 8/10
corroborated; complete official FCC history and the two exact-label gaps remain
unproven, so the goal remains active.

## Additional BSMI registers: raw SQL coverage and negative target audit

Added three original government XML snapshots and a strict dataset-specific
`bsmi-additional` importer: 17,797 product-certificate rows (162936), 1,818 voluntary
certificate rows (30176) and 564 declaration rows (169189). Schema 16 stores them
in three separate raw tables, with every field retained and no inferred catalog,
manufacturer, factory, alias or operator assertions. Lookup now searches 18
datasets. Repeated registration numbers and blanks survive; declaration numbers
are not unique row keys. Metadata captures explicitly identify factual web-tool
extracts, and original XML files are retained with dependency hashes and attribution.

A case-insensitive literal scan across every raw field found no matches for the
seven recorded target-name terms or ten full baseline labels. This establishes
only the audited term/snapshot result, not absence of other subsidiaries or
historical models. See research/bsmi-additional-registers.md and
 data/exports/bsmi-additional-coverage-audit.json. The three new registers therefore
add 20,179 searchable raw rows but no new target identities or baseline passes.

All fields of all imported rows match an independent XML traversal. Read-only
report and lookup exports left the database hash unchanged. The full 250-test
suite passes; one old schema-version expectation was updated from 15 to 16.
The manifest has 64 import jobs and 746 hashed files. An offline rebuild into
 data/rebuilt-with-bsmi-additional.sqlite succeeded, and all 30 compared
tables/indexes and baseline results match the live database. Source count is 165;
2,179 identities and 2,841 assertions are unchanged. Refreshed saved lookup
examples, offline source coverage and validation report.

The Celestica/Cisco CIUS public report lead returned a web cache miss and HTTP 403
on normal direct download; requests stopped without alternate access, no facts
imported. The Extreme Altitude PDF lead was also unavailable in the web tool.
Those limitations are recorded in research/cisco-celestica-factory-evidence.md.

The previous turn only restated the Oracle baseline; this turn makes concrete
progress on the local-copy/search requirement. Baseline remains 10/10 searchable
and 8/10 corroborated. The two exact-label gaps and exhaustive historical/FCC
coverage remain unproven, so the goal stays active. No live processes remain once
the final example export has completed.

## Celestica original networking index and read-only query commands

A normal download of the public Celestica networking index now returned the
original HTML successfully. Added a hashed snapshot and reviewed tile bundle,
plus `celestica-networking`: 15 product cards produce 17 model assertions. Three
related-resource headings are explicitly excluded. All 17 identities already
existed; replacing the three uncached DS6000/DS6001/DS6010 seeds adds 14 net
assertions. Removed the old source from curated.json after validating the source
migration, so later curated imports cannot overwrite the stronger evidence.
The old unsupported-browser file remains historical and is not imported.

Grouped model cards retain qualified shared descriptions, not aliases or
model-specific allocations of alternatives. Introductory text, form-factor
spellings, cooling statements, product URLs and the DS6010 AMD Helios integration
statement remain source-scoped. Only the Celestica brand role is asserted;
manufacturer/factory/operator and availability are not inferred. A separate
three-model validation reports 3/3 externally corroborated; the ten user-owned
expectations are unchanged. See research/celestica-networking-index.md.

Live verification exposed a separate query bug: specs rewrote SQLite metadata,
and search on a missing path created an empty database. Catalog now has an
explicit read-only connection mode, used by the remaining query/report CLI
branches. Reads share one snapshot and do not initialize or migrate schemas.
Missing paths fail without creating directories or reports. Init/import/sync
retain write mode. Three tests cover eleven commands, byte preservation, actual
write denial, missing paths and explicit old-schema upgrades. Three importer
tests cover migration, grouped names, scope, malformed cards and hashes. The
full suite passes 256 tests. A live DS6001 specs query returns the two source
statements and leaves the database bytes unchanged.

The offline rebuild into data/rebuilt-with-celestica-networking.sqlite succeeds:
65 jobs, 748 hashed files, 165 sources, 2,179 identities and 2,855 assertions.
All 30 compared tables/indexes and owned-baseline results match. Manufacturer
inventory stays 462 identities; the brand-only index is excluded from that scope.
Both this turn and the preceding BSMI turn make concrete progress. The two
owned exact-label gaps and exhaustive historical/FCC coverage remain unproven;
the goal remains active, with no overall blocker.

Final export refresh completed: all 27 saved lookup reports, vendor inventories,
original-card facts CSV, offline catalog, separate Celestica validation and owned
baseline were refreshed. The read-only export check passed with unchanged live
database bytes. Its first attempt correctly caught the metadata write from the
old specs CLI during verification; after fixing that CLI, the complete refresh
was rerun and passed. No processes remain running.

## Edgecore Chinese legal name and Taiwan registry coverage

The official Hsinchu Science Park record A0902 pairs 鈺登科技股份有限公司 with
Edgecore Networks Corporation and business number 25063543. Cached original HTML
and strict same-table identity checks now support this exact name in taiwan-targets
bundle version 2. Global aliases remain unchanged. Version 1 bundles retain their
original two-company scope and do not silently gain an unverified third mapping.

The full-name match selects 38 NCC rows and one additional BSMI product-certificate
row. NCC has 38 applicant roles and one manufacturer role, with one row carrying
both. Thirteen NCC rows were already selected through another target role, so
this adds 25 NCC assertions plus one BSMI assertion. The Taiwan importer now
produces 295 assertions: 42 BSMI, 252 NCC and one BSMI-product. Raw tables remain
unchanged. Applicant/manufacturer roles and printed brands stay separate.

This adds 17 catalog identities and 23 Edgecore association identities. The
identity delta is checked against the previous verified database by identity ID,
including revision: 14 brand/model pairs are new, while three additional identity
records use pairs already present with other revisions. A temporary manifest
count based only on brand/model pairs was corrected to the actual SQLite identity
count before the rebuild. Current totals are 2,196 identities, 2,881 assertions
and 166 sources. Manufacturer inventory is 463 identities; Edgecore has 119.
Only ECW3142-N adds a manufacturer claim. SMC-labelled and ZPE ZAP5220 applicant
associations, and BSMI RD1201000-C47-2MG, do not become manufacturer/factory claims.

Six Taiwan importer tests and the full 259-test suite pass. The 65-job manifest
hashes 749 files and rebuilds data/rebuilt-with-edgecore-taiwan.sqlite. All 30
compared tables/indexes and owned-inventory results match. Updated role exports,
vendor inventories, 30 saved lookup reports, offline catalog and validation report.
Read-only export refresh left live database bytes unchanged. Auction-title examples
for ZAP5220 and ECW3142-N are saved separately. See research/taiwan-target-evidence.md
and data/exports/taiwan-edgecore-mapping-audit.json.

Earlier supplementary BSMI scans were explicitly limited to seven name terms;
the newly verified Chinese name was absent from that list. The one newly mapped
product-certificate row is documented as follow-up, preserving the original
negative audit's stated scope. The other two supplementary registers have no
match for this exact name.

Cross-brand Cisco ASR-9000V-DC-E and Juniper EX4400-24X factory leads remain
unverified: one report download returned HTTP 403 and no original issuer/vendor
copy of the other was located. No claims from those leads were imported and no
access-denial workaround was attempted. See research/cross-brand-factory-leads.md.

Both this turn and the preceding Celestica/read-only turn are progress. Baseline
remains 10/10 searchable and 8/10 corroborated; exact-label and broader historical
coverage gaps remain. The goal stays active. All processes are terminal.


## Selected Korean approval mirror evidence

The preceding Snowball research turn yielded a bounded negative result and a
new distinction for future imports: V3_5C/V3_5S are documented software-reported
DeviceType values, not verified chassis labels. Its research note changes the
next action to obtaining an explicit label/declaration rather than importing
those values as model aliases. Classified as research progress, not completion.

A further exact-label pass did not close 7760-32X-A-AC-F or AZ4444-52X. A separate
public Korean approval mirror detail table explicitly names Celestica as
manufacturer/applicant for CS8000-32X-DC-11 and derived CS8001-32X-DC-11.
The table is preserved as a factual web-tool extract, not original HTML or an
original RRA record. Direct HTML returned 403 and was not retried or bypassed;
the Korean-language link returned a web cache miss. Original RRA corroboration
remains outstanding.

New importer rra-reviewed validates all capture fields and hashes before source
replacement. Main/derived identities remain separate, and the Korean certificate
uses rra_certificate_id rather than fcc_id. Country and date cells stay raw.
No printed-brand, factory, current approval or AWS deployment claim is inferred.
The importer adds two assertions to existing models: 2,196 identities, 2,883
assertions, 167 sources. Celestica manufacturer inventory remains 40 identities
with 50 source records across six sources. See research/korean-rra-reviewed.md.

Three focused tests and the full 262-test suite pass. The updated 66-job manifest
hashes 751 files and rebuilds data/rebuilt-with-rra-reviewed.sqlite. All 30
compared tables/indexes and the baseline agree with the live database. Certificate
lookup returns two catalog identities; the exact derived-model manufacturer
search returns one. Baseline remains 10/10 searchable and 8/10 corroborated.
The goal remains active: exact-label and broader historical coverage are still
unproven. This turn adds executable import support and sourced assertions, so
it is progress with no overall blocker.

Vendor inventories, the offline catalog, baseline and saved lookup reports were
refreshed successfully. The live database bytes were unchanged by export work;
see data/exports/rra-reviewed-readonly-check.json. All processes are terminal.


## Original Korean RRA query and detail archive

The preceding mirror-import turn was progress: a new checked importer and two
source assertions were added. This turn found the official public RRA search
form, CSV export and documented certificate-detail URL, so original regulatory
capture became the next available action. No overall blocker is present.

A manufacturer=Celestica query over 19900101–20260926 returns 151 numbered CSV
rows. Original detail HTML is archived for every certificate and the two query
exports agree byte-for-byte. Their CP949 CSV has unquoted commas; intermediate
cells remain raw. Detail UTF-8 HTML supplies the nine labelled fields. The
original query page is retained, but its malformed pagination links were not
followed or repaired. Completeness is limited to the returned query. The old
emsit API returned HTTP 400 over HTTP and HTTPS; no API response was imported.

New importer rra and schema-17 table rra_equipment retain all 151 details,
including composite manufacturer names. Existing exact target-name mapping
selects 48 manufacturer rows, yielding 58 main/derived assertions, 19 new catalog
identities and 25 additional Celestica manufacturer-claim identities. Remaining
103 rows are searchable raw evidence, with unmapped names audited for follow-up.
No country cell becomes a factory claim. Seven Oracle applicant associations
remain separate from manufacture. The official CS8000/CS8001 records corroborate
the prior mirror's model/manufacturer fields while preserving certificate
spelling and country discrepancies.

Current totals: 2,215 identities, 2,941 assertions, 168 sources. Manufacturer
inventory has 488 identities; Celestica has 65 across 108 records/seven sources.
All associations have 2,296 target/identity pairs across 2,215 identities.
Celestica has 245 association identities and Oracle 442. Others are unchanged.
Four focused tests and the full 266-test suite pass. A transient typo in the
manifest-writing helper was corrected; the already-started old-manifest build
failed its changed-manifest guard and did not publish a database. After that
process ended, the corrected 67-job/907-file manifest successfully rebuilt
 data/rebuilt-with-rra-official.sqlite.

See research/korean-rra-official.md, data/exports/rra-new-identities.json and
 data/exports/rra-celestica-coverage-audit.json. The full raw export is
 data/exports/rra-celestica-official.json (also CSV). Baseline remains 10/10
searchable, 8/10 corroborated. Next coverage work includes remaining target
manufacturer queries, verified legal-name mappings and both exact-label gaps.
The broader goal stays active.

Final verification: all 31 compared tables/indexes and owned-baseline results
match the fresh rebuild. Vendor inventories, offline catalog and 34 saved lookup
reports were refreshed; read-only export work preserved live database bytes.
All launched processes are terminal.

## Official Korean queries for the remaining four targets

Progress classification: concrete progress; no overall blocker or consecutive
no-progress condition. Original official RRA captures for the literal manufacturer
queries Amazon Data Services, Edgecore, Accton and Oracle are complete. They
return 57, 110, 200 and 516 approval rows respectively. Every certificate has a
matching original nine-field detail page and byte-identical before/after CSV
exports. Together with Celestica, rra_equipment now retains 1,034 query rows and
1,033 distinct certificate numbers. SUN-6000 occurs in two queries and stays
as two raw source observations.

The new reusable research/capture_rra_query.py orchestrates a fresh directory,
query metadata, detail collection and the confirming export. A bundle is only
published after all dependency hashes and fields validate. Observed certificate
underscores are now accepted and preserved; partial/network-failed captures
remain unpublished, without automatic retries.

The source audit exposed a name-normalization error: Korean company text before
a parenthesized English target disappeared under the general ASCII normalizer.
The RRA-specific mapping now refuses to discard non-ASCII letters/digits for
both manufacturer and applicant roles. Eight composite Edgecore rows stay raw,
removing 35 unsupported model assertions from the intermediate import; all five
queries were reimported with this fix. Other importers and global target aliases
are unchanged. A regression test protects these mixed-language company cells.

The four new queries add 1,041 identities and 1,322 source assertions. The catalog
now contains 3,256 identities, 4,263 assertions and 172 sources. Manufacturer
scope contains 1,649 identities: Accton 389, Edge-Core 210, Celestica 65, Oracle
812 and Amazon Data Services 173. All associations total 3,337 target/identity
pairs across 3,256 identities. These are catalog identities, not unique physical
hardware counts or proof of complete company histories.

The ten-device baseline remains 10/10 searchable and 8/10 corroborated; all four
Oracle models pass. Accton RRA approvals corroborate suffixed AZ4444 variants
and AS7760-32X, but do not establish the exact expected Edge-Core labels
AZ4444-52X and 7760-32X-A-AC-F. No suffix was stripped or inferred alias added.
Amazon manufacturer claims, including BARRELEYE and SAT1A_C2, do not establish
deployment or the provenance of any auction unit.

All 270 tests pass. The 71-job manifest hashes 1,806 input files; a fresh rebuild
at data/rebuilt-with-rra-targets.sqlite succeeds. All 31 compared tables/indexes
and owned-baseline results match the live database. See
research/korean-rra-targets.md and data/exports/rra-target-query-audit.json for
query scope, unmapped names and new identity exports. Next work includes reviewed
legal-name mappings, alternate company-query spellings and both exact-label
gaps. Exhaustive FCC and historical hardware coverage remain unproven; the
broader goal stays active.

Final verification for the four-query expansion: all 36 saved lookup reports,
three vendor inventory scopes, owned-baseline export and offline catalog are
refreshed. Read-only export work preserved the live database byte hash, recorded
in data/exports/rra-targets-readonly-check.json. Every process launched for this
expansion is terminal.

## Reviewed Celestica company names in RRA approvals

Progress classification: concrete progress. The previous goal turn expanded
authoritative source coverage and passed a full rebuild, so there is no
no-progress streak or overall blocker. A fresh exact-gap search still found
seller evidence for the full 7760 ordering code and no qualifying bare AZ4444
source. The mapped-affiliation audit found no further non-ASCII company text
being discarded into target matches; results are saved separately.

Celestica's 2025 filing Exhibit 21.1 supplies reviewed legal names for Celestica
LLC, Celestica (Thailand) Limited and Celestica Holdings Pte Limited. Public
web-tool text was successfully retrieved, then direct HTML download returned
403. Do not retry https://celestica.gcs-web.com/node/17171/html through alternative
routes. The already retrieved factual names are retained as an explicitly
labelled web-tool extract, not original HTML. No new download was substituted.

New devicehunt/rra_company_names.py supports a narrowly scoped optional company
evidence dependency on an RRA bundle. It validates the extract and its hash
before source replacement. Exact legal-name matches and reviewed Ltd/Limited
abbreviations are differentiated in per-record facts. Raw company values,
evidence URL/hash/method/reporting date and interpretation remain visible.
Composite companies and non-ASCII additions are not silently matched. Global
company aliases and other importers are unchanged. Removing the dependency and
reimporting withdraws its added claims. The later filing is not proof of parent
ownership at historical approval dates or of a physical manufacturing factory.

The reviewed spellings cover 41 additional approval rows and add 42 model
assertions across 41 identities; 39 identities are new to the catalog. DS4100
and DS5000 gain explicit Celestica LLC manufacturer claims. Celestica Holdings
records include SPARC Enterprise T1000/T2000 as derived models with Oracle
applicant affiliations; combined main-model strings remain unsplit. Other new
identities include cross-brand networking, voice and video equipment.

Current totals: 3,295 identities, 4,305 assertions and 172 sources. Manufacturer
scope contains 1,690 identities; Celestica has 106, backed by 150 source records
across seven sources. All associations have 3,378 target/identity pairs across
3,295 identities. Celestica association identities rise to 284 and Oracle to
1,168; other target counts are unchanged. The 151 Celestica raw approval rows
remain intact: 89 eligible manufacturer rows yield 100 model assertions, and
62 rows remain raw-only. Total RRA rows remain 1,034 across five queries.

All 273 tests pass. Three new tests cover optional evidence, scoped spellings,
provenance, withdrawal on reimport, composite-name rejection and corrupt or
misidentified dependencies before mutation. The 71-job manifest now hashes
1,807 inputs. A fresh data/rebuilt-with-rra-companies.sqlite succeeds and matches
all 31 tables/search indexes and the owned baseline. Baseline remains 10/10
searchable, 8/10 corroborated; the exact Edge-Core gaps remain open.

See research/rra-celestica-company-names.md and
data/exports/rra-celestica-company-review.json. The compact additions CSV/JSON
contains 42 source assertions across 41 model identities with manufacturer,
applicant, mapping interpretation and original approval URLs. The original
query bundle and historical coverage audits are preserved and labelled.
The broad goal remains active: remaining legal-name mappings, query spelling
coverage, exact-label gaps and full FCC/historical coverage are still unproven.

Final verification for the company-name review: all 36 saved lookup reports,
vendor inventories, baseline results and offline catalog are refreshed. Export
work preserved live database bytes, recorded in
data/exports/rra-companies-readonly-check.json. All launched processes are
terminal. Opening the additions CSV in the app returned queued, not a claim
that the user has already viewed it.

## Alternate Edge-Core queries and verified empty results

Progress classification: concrete progress. The prior turn expanded manufacturer
evidence with reviewed legal-name provenance and a matching fresh rebuild; no
no-progress streak or overall blocker applies. Two public RRA queries now test
the alternate literal manufacturer spellings Edge-Core and Edge Core over the
same 19900101–20260926 interval.

Edge-Core returns seven previously uncollected certificates, all with original
labelled detail pages and identical before/after CSVs. They yield 13 assertions
and five new identities: 5812-54X-O-AC-B, 7712-32X-O-AC-B, CSS-24H2L, CSS-24H3L
and ECS2000-18T. Two COMMAX-applicant records explicitly name Edge-core Networks
as manufacturer. WA6202A now retains both the existing Accton/NCC and the new
Edge-Core/RRA manufacturer claims; neither overrides the other.

Edge Core returns no rows. A header-only CSV is now accepted only with a hashed
original result page explicitly reporting no matching information, complete
expected page/table structure, and a second identical CSV. Generic error pages,
extra rows, missing proof and contradictory nonempty data fail validation before
replacement. The zero is a dated query observation, not absence of company
devices or all approvals. The HTML does not echo query fields; the bundle records
the exact POST fields used. It contributes a source with records_seen=0 and no
fabricated device/registry row. A verified empty refresh affects only its own
query. Lookup now includes these zero-query source records instead of calling
them not_loaded.

Current totals: 3,300 identities, 4,318 assertions, 174 sources. Seven RRA query
sources retain 1,041 rows with 1,040 distinct certificate numbers. Manufacturer
scope has 1,696 target/identity pairs across 1,695 identities, reflecting the
two manufacturer claims for WA6202A. Edge-Core has 216 manufacturer identities
across 286 records/five sources. All associations have 3,383 pairs across 3,300
identities; Edge-Core association identities rise to 669. Other target counts
are unchanged. Both exact-label gaps remain open: baseline is 10/10 searchable
and 8/10 corroborated, including all four Oracle models.

During report refresh, a catalog lookup bottleneck was measured and corrected:
the query rescanned the FTS virtual table for each device's unindexed ID. It now
uses membership in a single matching-ID result set. Complete 19-dataset JSON
outputs match before/after for CSS-24H2L and Oracle. Full lookup wall times were
7.8010 -> 2.6696 seconds and 5.7905 -> 2.4968 seconds respectively, with execution
order alternated. These are measurements on this snapshot, not guarantees.
Raw-register scans remain; no schema/index migration or model matching change
was introduced. A regression verifies duplicate/stale/missing index behavior.

All 279 tests pass. The 73-job manifest hashes 1,823 files, and the fresh
data/rebuilt-with-rra-spellings.sqlite matches all 31 checked tables/search
indexes and the baseline. See research/rra-edge-core-spellings.md,
data/exports/rra-edge-core-spelling-audit.json and
data/exports/lookup-membership-benchmark.json.

Additional FCC bulk-source screening found the same official historical grantee
export, plus radio-station and surveillance-identifier databases with different
scopes. Those alternatives were not imported or represented as full equipment
authorization data. No denied FCC endpoint was retried. Full FCC filing coverage,
remaining company-name/query coverage and both exact-label gaps remain unproven.
The broader goal stays active.

Final verification for the spelling/lookup turn: all 36 saved reports, vendor
inventories, baseline output and offline catalog are refreshed. The running
export process was allowed to finish; its RRA source metadata was then updated
to include the zero query. A full regenerated representative report agrees,
and read-only checks confirm unchanged database bytes. Evidence is in
data/exports/rra-spellings-readonly-check.json and
data/exports/rra-zero-lookup-metadata-check.json. All processes are terminal.


## Amazon UL model names — 2026-09-27 UTC

The public Amazon Data Services UL AZOT7.E515139 search result supplied 74
factual model entries. Direct opening returned 403 and was not retried. The
reviewed search extract preserves duplicates and spelling; original HTML,
visual grouping and index freshness remain unverified. Ten unique placeholder
templates stay unexpanded in the cache. A new ul-amazon-platforms importer
adds 57 exact-name assertions, including 34 new catalog identities. Every
assertion has listing_holder scope only, with zero manufacturer, operator or
cross-brand alias inference. See research/ul-amazon-platforms.md and the
ul-amazon-models JSON/CSV exports and audit.

Current totals: 3,334 identities, 4,375 assertions, 175 sources. Manufacturer
scope remains 1,696 target/identity pairs across 1,695 identities. All associations
have 3,417 pairs across 3,334 identities; Amazon Data Services rises from 176 to
210 association identities while its manufacturer count stays 173. The baseline
remains 10/10 searchable and 8/10 corroborated, including all four Oracle labels.
The full 7760-32X-A-AC-F and bare AZ4444-52X gaps remain open.

All 282 tests pass. The 74-job manifest hashes 1,824 files and rebuilds to
data/rebuilt-with-ul-amazon.sqlite. Tests cover exact names and suffixes,
placeholder exclusion, repeated-entry provenance, role isolation, idempotent
refresh and invalid-capture rejection before replacing evidence.

OpenBMC discovery did not establish new target-maker records: the initially
retrieved upstream recursive tree and a later master commit referenced different
trees. A guard stopped dependent downloads before mixing snapshots. Discovery
files are retained outside the import manifest; no target manufacturing or
negative exhaustive coverage claim was inferred. Any continuation must pin
its tree and source files to the same immutable commit. FCC coverage and the
broader exhaustive goal remain unproven, so the goal stays active.

Final verification: the fresh database matches all 31 checked tables and search
indexes plus baseline results. All 38 saved lookup reports, three vendor scopes,
baseline output and offline HTML are refreshed. Read-only exports preserved
the database hash. See data/exports/rebuild-comparison.json,
data/exports/rebuild-validation.json and data/exports/ul-amazon-readonly-check.json.
All processes are terminal.


## Accton Singapore manufacturer spelling — 2026-09-27 UTC

Reviewed the 50 previously unmapped Accton-query manufacturer rows. A public
search extract of Accton's 2013 consolidated report explicitly names Accton
Technology Corp. Pte Ltd (Singapore), reported under Edgecore Networks. Direct
PDF opening returned 402 and was not retried. The selected factual extract is
hashed separately; it does not claim original PDF capture, visual review, an
exact reporting date, or ownership at historical approval dates. Optional RRA
company evidence now recognizes that whole legal name, scoped to this bundle.
The raw legal manufacturer and dated parent/qualifier interpretation remain
facts. Accton is the named manufacturer namespace; no parent manufacturing,
Edge-Core model alias, SMC expansion, current ownership or deployment is inferred.

Fourteen raw approval rows use that company spelling. Thirteen add model claims
for twelve identities, ten new to the catalog. EN1207F-TX/WOL remains unparsed
but raw searchable; no slash alias was invented. Thirty-six rows still have
unmapped manufacturer names. The audit lists them for further review. Accton's
query now has 176 model assertions; all seven RRA queries have 1,448 assertions.

Current totals: 3,344 identities, 4,388 assertions, 175 sources. Manufacturer
scope has 1,708 target/identity pairs across 1,707 identities; Accton has 401,
backed by 461 records/seven sources. All associations have 3,427 pairs across
3,344 identities; Accton has 1,096. Other target counts are unchanged. The ten
owned labels remain unchanged. See research/rra-accton-company-name.md and
exports rra-accton-company-review.json and accton-manufacturer-additions.csv.

All 285 tests pass. The 74-job manifest now hashes 1,825 files. Rebuild target:
data/rebuilt-with-accton-company.sqlite. New tests verify scoped company mapping,
preserved provenance, withdrawal, composite/unreviewed-name exclusion and
invalid evidence rejection before mutation. Full historical/FCC coverage and
the two exact-label gaps remain unproven; the goal remains active.

Final verification: the fresh rebuild matches all 31 checked tables/search
indexes and baseline results. The baseline is 10/10 searchable, 8/10 corroborated.
All 40 saved lookup reports, vendor inventory scopes and offline HTML are
current. The slash-qualified EN1207F-TX/WOL lookup finds one raw RRA row.
Read-only exports preserved database bytes; see accton-company-readonly-check.json,
rebuild-comparison.json and rebuild-validation.json under data/exports.
All processes are terminal.


## PICOS switch hardware table — 2026-09-27 UTC

Exact-label follow-up still finds seller-only complete 7760 codes and suffixed
AZ4444 regulatory entries, not new independent exact-label corroboration. The
public PICOS 4.5 Edgecore/Accton guide supplied an original HTML table of 26
platforms. All rows are captured and imported with exact row-level port layout,
ASIC and CPU text, plus nine explicit parenthetical DCS/EPS model codes.
Navigation/sidebar models are excluded. New parser guards validate title,
heading, table count/headers/cell spans, expected row count, duplicates and
complete model-cell syntax before any source replacement.

The joint EdgeCore/Accton heading supplies compatibility_partner associations
for both named companies. Edge-Core is the identity namespace, not manufacturer
or retail-brand proof. Guide context does not establish specific release
support, minimum firmware, image access, license transfer, AWS deployment or
auction-unit configuration. Missing models are not proven unsupported. Two new
identities are AS7312-54XS and AS9737-32DB; other-brand identities stay separate.
See research/picos-hardware.md and data/exports/picos45-platforms.csv/json.

Current totals: 3,346 identities, 4,414 assertions, 176 sources. Manufacturer
scope remains 1,708 target/identity pairs across 1,707 identities. All associations
have 3,455 pairs across 3,346 identities: Accton 1,122, Edge-Core 671,
Celestica 284, Oracle 1,168, Amazon Data Services 210. Manufacturer/factory
counts are unchanged. No owned baseline labels or operator claims changed.

All 288 tests pass. The 75-job manifest hashes 1,827 files; rebuild target is
data/rebuilt-with-picos.sqlite. Tests verify exact aliases and row-scoped specs,
sidebar exclusions, partner-role isolation, idempotent refresh and failure before
mutation for malformed tables, ambiguous names or unverified dependencies.
Full FCC/historical coverage and the two exact-label gaps remain unproven;
the broader goal remains active.

Final verification: all 31 compared tables/search indexes and baseline results
match the fresh rebuild. All 42 saved lookup reports, vendor inventories,
baseline and offline HTML are refreshed. Read-only exports, example search and
specification queries preserve database bytes (picos-readonly-check.json).
The baseline remains 10/10 searchable and 8/10 corroborated.
The two new identity rows are not necessarily new physical designs: the existing
vendor DCS511 record already carries AS9737-32DB as an independently sourced alias.
All processes are terminal.


## Legacy Sun approvals and Oracle lineage — 2026-09-27 UTC

Collected the official RRA query for Sun Microsystems: 40 raw approvals and
40 original detail pages. The initial collector correctly rejected different
before/after CSV bytes. Inspection found only same-date TYPE 6 USB and Crossbow
USB certificates swapping row positions 17/18; all certificate-keyed fields
were identical. Both exports and the failed candidate remain intact. A reviewed
candidate opts into exact_rows_ignore_order. Default byte equality is unchanged;
the optional mode validates each CSV's numbering/unique certificates and exact
per-certificate field values, rejecting changed/missing/extra/duplicate rows.
No retry or weakened model/identity matching was needed. The collector now has
an explicit --allow-row-reordering option for this comparison mode.

Oracle's 2010 Form 8-K item 2.01 confirms Sun became its subsidiary on January
26, 2010; item 8.01 dates announcement January 27. Public web-tool text was read;
direct original HTML returned 403 and was not retried. A separately hashed
factual extract supplies acquisition lineage, not original HTML. The denied URL
is https://www.sec.gov/Archives/edgar/data/1341439/000119312510015241/d8k.htm.

The dedicated rra-sun-portfolio importer validates the RRA bundle and lineage
proof before replacing either source. All 40 registry rows remain searchable.
Thirty whole-company matches yield 29 model assertions across 26 Sun identities.
Five matched-company rows have unparsed Korean-qualified model cells; ten other
rows retain unresolved manufacturer names. Sun remains manufacturer/namespace;
Oracle receives acquired_company_portfolio only. No Oracle manufacturer target,
pre-acquisition ownership, current support, factory, deployment or cross-brand
alias is inferred. ULTRA60 and ULTRA 60 remain separate printed names.

Current totals: 3,372 identities, 4,443 assertions, 178 sources. Eight RRA queries
retain 1,081 rows/1,080 distinct certificates. Manufacturer scope is unchanged:
1,708 target/identity pairs across 1,707 identities; Oracle 812. All associations
have 3,481 pairs across 3,372 identities; Oracle rises to 1,194. Other company
counts are unchanged. See research/sun-legacy-portfolio.md, rra-sun-audit.json
and sun-legacy-portfolio.csv/json under data/exports.

All 293 tests pass. The 76-job manifest hashes 1,872 files; rebuild target is
data/rebuilt-with-sun.sqlite. Tests cover order-only comparisons, changed-data
rejection, historical manufacturer/portfolio role isolation, whole-company
selection, withdrawal and invalid lineage rejection before source mutation.
Full historical/FCC coverage and both exact-label gaps remain unproven;
the broader goal remains active.

Final verification: all 31 compared tables/search indexes and baseline results
match data/rebuilt-with-sun.sqlite. All 44 saved lookup reports, manufacturer/
factory/all inventories, baseline and offline HTML are refreshed. The portfolio
role search returns 26 Sun identities; exports include the original approval date,
acquisition completion date and separate lineage URL. Read-only checks preserve
database bytes (sun-readonly-check.json). Baseline remains 10/10 searchable,
8/10 corroborated. All processes are terminal.


## ENERGY STAR enterprise-server snapshot — 2026-09-27 UTC

Imported all 495 published server records with all 196 original fields retained
in energystar_servers, plus three Oracle model assertions (X9-2L, X10M, X11M).
Original full CSV and before/after metadata are hashed. Column definitions,
rowsUpdatedAt, non-null primary-ID count, unique IDs and row widths agree.
This proves coverage of the captured published snapshot, not historical or
worldwide server completeness. Historical qualification spreadsheets linked
by EPA remain uncollected leads. See research/energystar-servers.md.

Brand and program_partner remain distinct from manufacturer/factory claims.
Three test configurations retain separate processor, memory, CPU/PSU counts,
PSU model/rated output and idle watts. SERT total power scores remain raw only.
Configuration IDs, PSU names and related longer model names are not inferred
aliases. Three short printed model identities are new catalog rows; this is
not proof of three new physical designs.

Current totals: 3,375 identities, 4,446 assertions, 179 sources. Manufacturer
scope remains 1,708 target/identity pairs across 1,707 identities; Oracle 812.
All associations have 3,484 pairs across 3,375 identities; Oracle 1,197. Other
vendor/factory counts are unchanged. Owned baseline remains 10/10 searchable,
8/10 corroborated, with all four Oracle models passing. The two exact-label
gaps remain 7760-32X-A-AC-F and bare AZ4444-52X.

All 296 tests pass. Schema 18 adds the raw table; unified lookup now covers
20 datasets. Tests cover all-brand retention, brand/partner role isolation,
configuration power separation, exclusion of patterns and non-target brands,
withdrawal, idempotence and rejection before replacement of changed metadata,
truncation, duplicate IDs, malformed rows, wrong headers and bad dependencies.
The offline manifest now has 77 jobs and 1,876 hashed files.

Final verification: all 32 compared tables/search indexes and baseline results
match data/rebuilt-with-energystar.sqlite. All 46 saved lookup reports now
include 20 datasets. Manufacturer/factory/all inventories, baseline and offline
HTML are refreshed. Read-only exports and specification queries preserve
database bytes (energystar-readonly-check.json). All processes are terminal.
Full historical/FCC coverage and both exact-label gaps remain unproven; the
broader goal remains active.


## Historical ENERGY STAR server workbooks — 2026-09-27 UTC

Downloaded both original workbooks and official linking page successfully.
Version 2 has 279 qualification rows, preceding the June 17, 2019 specification
change. Version 1.1 has 148 single-configuration and 446 family rows, preceding
the December 16, 2013 change. All 873 rows retain original sheet/row locations,
all fields, duplicate family configurations and source URLs in the new
energystar_server_archives table. Complete extracted cells include definition
and criteria sheets; original XLS/XLSX files remain retained. This is published
file coverage, not every historical qualification or manufactured server.

Optional extraction uses openpyxl 3.1.5 / xlrd 2.0.2 (latter installed only in
/private/tmp/devicehunt-xls-reader). Normal rebuild needs neither. Extraction
was rerun and exactly matched both saved JSON cell structures. XLS dates
preserve original serial/date-system plus interpreted ISO values. Import
verifies hashes, workbook binding, extraction method, source link, sheet presence,
widths, reviewed header hashes/counts, identity fields and unique version 2 IDs.
Merged header cells explicitly preserve minimum/maximum subheaders.

Seven version 2 Oracle assertions cover X5-4 and Oracle Server X6-2/X6-2L,
X7-2/X7-2L, X8-2/X8-2L. Eight version 1.1 family rows give three Sun Fire model
identities (X4170 M2, X4270 M2, X4470), using printed Model Name and preserving
Model Number/configuration IDs as facts. Sun Fire remains brand/namespace;
Oracle Corporation is program_partner, not manufacturer. No aliases, deployment
or hardware-manufacturer claims are inferred. Original field names distinguish
PSU output, idle, full-load and test voltage/frequency; N/A, scores and allowances
are not watt measurements. Family 1 X4170 M2 idle fields are 161/229 W under
230 V headings; full-load 345/511 W separately prints 220 V / 60 Hz. This source
wording is preserved. See research/energystar-server-archives.md.

Current totals: 3,379 identities, 4,461 assertions, 181 sources. Four new identity
rows are not necessarily four new physical designs. Manufacturer scope remains
1,708 target/identity pairs across 1,707 identities; Oracle 812. All associations
have 3,488 pairs across 3,379 identities; Oracle 1,201. Other company/factory
counts are unchanged. Schema 19 adds the archive table; lookup now covers
21 datasets. All 300 tests pass, including family-header resolution, raw-date
retention, role isolation, power/test-voltage scoping, all-brand lookup, withdrawal
and invalid extraction rejection before source replacement. The offline rebuild
manifest now has 79 jobs and 1,883 hashed files.

Final verification: all 33 compared tables/search indexes and baseline results
match data/rebuilt-with-energystar-archive.sqlite. All 48 saved lookup reports
now include 21 datasets; manufacturer/factory/all inventories, baseline and
offline HTML are refreshed. Read-only exports/specification queries preserve
database bytes (energystar-archive-readonly-check.json). Baseline remains
10/10 searchable and 8/10 corroborated, including all four Oracle entries.
All processes are terminal. Full historical/FCC coverage and both exact-label
gaps remain unproven; the broader goal remains active.


## ENERGY STAR network and storage registers — 2026-09-27 UTC

Collected official complete public network (96 rows/223 fields), file-I/O
storage (35/144) and block-I/O storage (83/114) CSVs, with before/after metadata.
Also collected both storage archives preceding March 15, 2021: file-I/O 9 rows/
282 fields, block-I/O 229/289. All 452 qualification rows retain source IDs,
original fields, model names/numbers, mapped dates, worksheet/row location and
URLs in energystar_datacenter. Both historical cell extractions were reproduced
and matched; originals and official linking page remain hashed dependencies.

None names the five target companies in partner/brand fields. A literal scan
of all exported values for Accton, Edgecore/Edge-Core/Edge Core, Celestica,
Oracle and Amazon Data Services also found no matches. Five sources therefore
add zero target model assertions. This is evidence of these published lists'
coverage, not proof those companies never made eligible products. No controller,
PSU, module or brand is converted to an inferred OEM relationship. Data remains
useful for all-brand SQL and local label/specification cross-checks, e.g. Dell
E21W's separate qualification rows and Viking VDS2249R. The documented E21W
SQL query was executed and exported.

The current file-I/O export formats numeric PD_ID values with commas. Only
that source opts into validated three-digit-group normalization, with numeric
source metadata required. Raw JSON retains the original comma-formatted ID;
the mapped ID and detail URL use digits. Duplicate detection compares normalized
IDs. Models and other datasets retain their prior rules. Storage GB/W and
MiBPS/W ratios are not converted to watts; all test/configuration fields remain
raw facts without auction-unit consumption estimates.

Current totals remain 3,379 identities and 4,461 assertions; sources rise to
186. Manufacturer/factory/all target inventories are unchanged. Schema 20 adds
the raw table; unified lookup now covers 22 datasets. All 305 tests pass,
including normalized-ID collisions, bad grouping/type/changed metadata, raw
value preservation, component-role isolation, zero-target source visibility,
source refresh isolation and archival drift rejection before replacement.
The manifest now has 84 jobs and 1,902 hashed files. See
research/energystar-network-storage.md and data/exports/energystar-datacenter-*.

Final verification: all 34 compared tables/search indexes and baseline results
match data/rebuilt-with-energystar-datacenter.sqlite. All 50 saved lookup reports
now include 22 datasets; manufacturer/factory/all inventories, baseline and
offline HTML are refreshed. Read-only exports and example SQL queries preserve
database bytes (energystar-datacenter-readonly-check.json). Baseline remains
10/10 searchable and 8/10 corroborated. All processes are terminal. Full
historical/FCC coverage and both exact-label gaps remain unproven; the broader
goal remains active.


## Canadian REL raw CSV search and bulk-scope review — 2026-09-27 UTC

Revisited public ISED bulk download options. The official Site Data Tool page
links two original XLSX files with historical data through February 2023. Both
downloaded successfully (73,265,782 / 89,334,785 bytes). The official guide
and workbook inspection establish that these are historical SMS licensed-site
records cross-referenced to selected REL identifiers, not a full REL export.
Site_Data_Extract contains LICENSEE/transmitter/site fields; its certificate and
HVIN columns contain lookup formulas referencing a small TX_model sheet. No
formulas were evaluated, and no site/licensee was made a hardware manufacturer
or device. Files and scope-review.json are retained in data/raw/ised-site-data
outside the equipment rebuild manifest. The initial web index fetch timed out;
a normal direct download succeeded, with no denial or alternate-access retry.
The complete current REL bulk export remains unverified.

Made all checked CSV rows from the existing five company-query captures
independently SQL-searchable in ised_equipment: Amazon 4, Accton-query 579,
Edgecore 29, Oracle 16, total 628. Celestica's verified zero remains source
metadata, with no fabricated row. Duplicate RF configurations, non-target
companies, all 44 original cells, per-row company numbers, blank certificates
and timestamp offsets remain preserved. Only independently labelled identity
columns are mapped; remaining cells stay uninterpreted. Each row records whether
it exactly matches a labelled HTML identity under the existing comparison.

The six unmatched Accton CSV rows name MN-500/MN-510/MN-520, SMC2662W V.2,
SMC2664W and WN3501C. Their certificate cells have trailing spaces; no trimming
or identity reconciliation forces a match. The fifteen HTML-only identities
remain catalog assertions with discrepancy facts. Raw-only and non-target rows
do not create aliases, catalogue identities, manufacturer claims or baseline
passes. Refresh replaces only its source; invalid captures fail before mutation.
Documented SQL queries were run and saved in ised-raw-query-check.json.

Current totals remain 3,379 identities, 4,461 assertions, 186 sources; all role
inventories are unchanged. Schema 21 adds the raw table; lookup now covers
23 datasets and preserves explicit zero-query source visibility. All 307 tests
pass. Added checks cover unmatched/raw-only lookup, all-cell retention, duplicate
RF rows, preserved per-row company IDs, non-target isolation, withdrawal and
source isolation, plus invalid-capture preservation. The existing 84-job,
1,902-file manifest now also verifies all 628 raw ISED rows. No new REL company
queries were submitted for this expansion.

Final verification: all 35 compared tables/search indexes and baseline results
match data/rebuilt-with-ised-raw.sqlite. All 52 saved lookup reports now include
23 datasets. Manufacturer/factory/all inventories, baseline and offline HTML
are refreshed. Read-only exports and SQL checks preserve database bytes
(ised-raw-readonly-check.json). Baseline remains 10/10 searchable and 8/10
corroborated. All processes are terminal. Full historical/FCC/REL coverage and
both exact-label gaps remain unproven; the broader goal remains active.


## Celestica historical legal-name expansion — 2026-09-27

Completed the reviewed-name work already underway. Two separately hashed
factual exhibit extracts support five additional Celestica legal entities and
six observed whole-cell spellings in the existing Korean capture. The SEC 8.1
extract was read through the web tool; archived corporate Exhibit 21.1 was
available as factual search text only, with its original PDF returning 403.
Reporting periods remain unverified. Per-record facts retain the legal name,
interpretation, source, hash, capture method and historical-ownership limitation.
No global aliases, composite-company splitting or factory/deployment inference.

Sixteen additional raw manufacturer rows are eligible. Fifteen have parseable
model cells, adding 21 model assertions and 21 identities; Kladno's combined
15938,15939 main cell remains raw-only. The Celestica query now contributes
121 assertions from 105 eligible manufacturer rows, with 46 unmapped. Current
catalog: 3,400 identities, 4,482 assertions, 186 sources. Manufacturer inventory
has 1,729 target/identity pairs across 1,728 identities; Celestica has 127 such
identities and 171 supporting records. All-association inventory is 3,509 pairs
across 3,400 identities. Factory inventory is unchanged.

All 310 tests pass. The unchanged schema-21 offline rebuild verifies 84 jobs
and 1,905 hashed files. All 35 tables/search indexes and baseline results match
in data/rebuilt-with-celestica-historical.sqlite. Oracle E4-2C, E5-2C, X7-2C and
X10-2C remain exact-model passes, preserving their full suffixes and no inferred
operator. Full baseline: 10/10 searchable, 8/10 corroborated. The two original
Edge-Core exact-label gaps remain open. Full historical/FCC/REL completeness
remains unproven. See research/rra-celestica-company-names.md and the
celestica-historical-* audit/exports for this expansion.

Final export verification: all 54 saved lookup reports, the three vendor-role
inventories and offline HTML are refreshed. Read-only exports preserve database
bytes (celestica-historical-readonly-check.json). All processes are terminal.


## Nigeria NCC complete published equipment file — 2026-09-27

The preceding goal turn made progress by completing evidence-backed Celestica
mappings and verifying their offline rebuild. Renewed exact-label searches in
this turn did not close the two original Edge-Core baseline gaps. A new official
source found during that research is Nigeria NCC's approved-equipment page and
its linked September 2026 CSV. Both original HTML downloads agree with all
8,517 CSV rows after display-whitespace normalization. The complete published
file is retained; completeness of the regulator's full historical/live database
is not independently established.

Schema 22 adds nigeria_equipment, keeping all six columns, duplicates, original
cells, source URL and flagged decoding issues. Five CSV cells contain invalid
UTF-8, also displayed as replacement characters by the official page. Their
original cell bytes remain base64-encoded in invalid_cell_bytes_json; no legacy
encoding is guessed and no damaged row becomes a catalog assertion. Existing
source garbling and scientific-notation labels remain verbatim. Serial numbers
are row numbers, never certificate IDs. No certification dates/validity claims
are invented. Nigeria NCC and Taiwan NCC have separate datasets.

Seven exact manufacturer rows add assertions: Amazon Data Services SCOUT01,
SCOUT02, ATLANTA23SF, ATLANTA23SFX; Oracle X8-2L and ZS7-2; Edge-Core AS7315-27x.
Four catalog identities are new within those source manufacturer namespaces,
not four proven new physical models. Different AWS branding and longer Oracle
names remain separate. Oracle descriptions mentioning wifi module PU2SL stay
facts, not inferred aliases. Three Accton Tech. Corporation MetroLinq rows remain
raw searchable leads because that company abbreviation is not yet reviewed.
No applicant, certificate holder, component maker or source description becomes
an inferred manufacturer or operator.

Current totals: 3,404 identities, 4,489 assertions, 187 sources. Manufacturer
scope has 1,734 target/identity pairs across 1,733 identities: Accton 401,
Edge-Core 217, Celestica 127, Oracle 814, Amazon Data Services 175. All associations
have 3,513 pairs across 3,404 identities. Factory counts are unchanged. All 314
tests pass, including malformed captures failing before replacement, damaged
bytes retained, role isolation, duplicate identities, withdrawal and source
isolation. Lookup now covers 24 datasets. Rebuild manifest: 85 jobs, 1,909
hashed files. data/rebuilt-with-nigeria.sqlite was built successfully.

Full/raw and role exports plus documented executed SQL are in nigeria-*.csv/json.
The owned baseline remains 10/10 searchable, 8/10 corroborated. Both missing
exact labels are absent from the new register. Full historical/FCC/REL coverage
remains unproven; the broader goal remains active. See
research/nigeria-approved-equipment.md for field definitions and limitations.

Final verification: all 36 compared tables/search indexes and the baseline
match the fresh rebuild. All 56 saved lookup reports, manufacturer/factory/all
inventories and offline HTML are refreshed. Read-only exports preserve live
database bytes (nigeria-readonly-check.json). All processes are terminal.


## Ghana NCA public equipment table — 2026-09-27

The preceding goal turn made progress by adding and verifying the full Nigerian
published register. This turn found another accessible public source through
Ghana NCA's official FAQ. All 1,153 rows in its public equipment table agree
between two original HTML downloads, including order, fields and detail links.
This is complete coverage of the returned table, not proof of the underlying
current/historical database. No independent total or authoritative publication
or update date is exposed; no approval date is decoded from an identifier.

Schema 23 adds ghana_equipment: all five labelled equipment columns plus original
visible cells, row number, distinct detail ID, detail URL and source URL.
Thirty-six blank approval-number cells remain blank. Manufacturer and brand
stay separate. Details links are references only. The sole target-manufacturer
row is Oracle America Inc. with brand Oracle, full printed model Oracle's Netra
Sparc T3-1 and approval 5N4-3H-7DF-22F. Its detail request returned HTTP 500;
that access result is retained separately without claiming a fetched certificate.
Other detail pages were not fetched. The table assertion preserves the full
printed label; no shortened alias or cross-source equivalence is inferred.

The table adds one identity and assertion, bringing totals to 3,405 identities,
4,490 assertions and 188 sources. This is not a proven new physical model.
Manufacturer scope is 1,735 target/identity pairs across 1,734 identities: Accton
401, Edge-Core 217, Celestica 127, Oracle 815, Amazon Data Services 175. All
associations are 3,514 pairs across 3,405 identities. Factory counts unchanged.
Both missing exact Edge-Core labels are absent. Baseline remains 10/10 searchable,
8/10 corroborated, with all four Oracle owned-hardware labels passing.

All 318 tests pass. New coverage checks nested layout/table handling, malformed
headings and links, duplicate detail IDs, whole manufacturer/brand roles, duplicate
models, blank approval IDs, invalid-capture preservation, source withdrawal and
isolation. Unified lookup now covers 25 datasets. Documented SQL and full/raw
exports are in ghana-*.csv/json; read-only exports preserve database bytes.
The 86-job, 1,913-file offline rebuild completed at data/rebuilt-with-ghana.sqlite.
See research/ghana-approved-equipment.md. Full historical/FCC/worldwide coverage
and both exact-label gaps remain unproven; the broader goal remains active.

Final verification: all 37 tables/search indexes and baseline results match the
fresh rebuild. All 58 saved lookup reports, manufacturer/factory/all inventories
and offline HTML are refreshed. Read-only exports preserve live database bytes
(ghana-readonly-check.json). All processes are terminal.


## RRA explicit main-model lists — 2026-09-27

The preceding baseline-only goal turn was no progress toward broader coverage:
it repeated the existing Oracle validation result. This turn revalidated the
catalog state and found an actionable parser omission in cached Korean detail
pages. Main-model cells containing several explicit comma-separated labels were
retained raw but excluded from the catalog. Fully simple main lists now yield
literal entries, retaining the complete original cell, certificate, detail URL
and hash. Any annotated or incomplete main list remains raw in full. No prefix
or suffix propagation, model alias, company-name expansion, factory claim or
operator inference was introduced. Numeric and short labels retain existing
marketplace safeguards. Main/derived case-insensitive duplicates occur once.

This recovers 84 assertions: Celestica 28, Accton 12 and Oracle 44. There are
75 new catalog identities, not 75 proven new physical products. Examples include
Celestica CP3060-RTM and the three explicit WS-C3550-24 labels, Accton BayStack
325-24G/325-24T, and Oracle SFE-CAB. All 1,081 raw RRA rows, all 4,490 pre-existing
records and their affiliations, identifiers, facts and hyperscaler evidence are
unchanged. The initial import audit stopped on an overly strict comparison of
baseline source-coverage wording after all imports succeeded. The follow-up
proof check compares against the verified previous Ghana rebuild and confirms
every baseline result/evidence/search hit is unchanged apart from that wording.
It did not repeat the imports or weaken identity validation.

Current totals: 3,480 identities, 4,574 assertions, 188 sources; schema remains 23.
Manufacturer scope has 1,813 target/identity pairs across 1,812 identities:
Accton 409, Edge-Core 217, Celestica 155, Oracle 857, Amazon Data Services 175.
All associations have 3,609 pairs across 3,480 identities; factory counts unchanged.
The seven target RRA queries supply 1,553 manufacturer assertions. All eight
cached RRA queries, including Sun, retain original source hashes and raw rows.
The manifest still has 86 jobs and 1,913 hashed inputs, with updated expected counts.

All 320 tests pass. A fresh offline rebuild at
data/rebuilt-with-rra-main-models.sqlite completed, with all 37 tables/search
indexes and baseline results matching live data. The documented SQL returns all
84 additions, and proof checks preserve database bytes. See
research/rra-main-model-lists.md and data/exports/rra-main-models-* for source
review, additions, delta audit and verification. The ten-device baseline remains
10/10 searchable and 8/10 corroborated. Full historical/FCC/worldwide coverage
and both exact Edge-Core labels remain unproven; the goal remains active.

Final verification: all 61 saved lookup reports cover 25 datasets. Manufacturer,
factory and all-association inventories plus offline HTML are refreshed.
rra-main-models-readonly-check.json verifies unchanged database bytes during
exports. All import, test, rebuild, comparison and refresh processes are terminal.


## Reviewed RRA manufacturer lists — 2026-09-27

The preceding goal turn made progress by recovering literal main-model lists.
This turn reviewed previously excluded multi-company manufacturer fields and
implemented a separate extraction for 19 exact comma-list spellings. It selects
complete target names with the same checked query-scoped company evidence, not
substrings or a generic comma parser. Unknown companion text stays context.
Parenthesized target-company relationships and geographic qualifiers remain
unresolved. The default RRA importer, global aliases and original rows are unchanged.

The source rra-reviewed-manufacturer-lists reuses original cached details from
three query bundles. Its metadata explicitly says this is a derived extraction,
not a new retrieval or independent corroboration. Each assertion retains the
full manufacturer field, selected legal names, companion text, original detail
URL/hash, query, bundle hash and applicable historical legal-name evidence.
Repeated certificate observations are preserved and deduplicated into claims;
conflicting observations fail before replacement. Two named Celestica entities
remain separate affiliations on the same model record. These are non-exclusive
regulatory manufacturer claims, not sole manufacture, factories, brands,
supplier contracts, operator deployment or provenance of auction units.

Twenty-two approvals yield 45 model assertions: Accton 6, Celestica 37 and
Oracle 2. There are 42 new catalog identities, not 42 proven distinct physical
products. Useful names include Celestica-associated Juniper M120/M320/T640,
T6300, X6200_Series and XFrame-II-LP; Accton WS-AP3640-ROW and J8131A; Oracle
DC1ST/DC2ST-3ST. Fullwidth-M derived Oracle labels remain raw and unaltered.

Current totals: 3,522 identities, 4,619 assertions, 189 source entries including
the derived extraction. Manufacturer inventory is 1,856 target/identity pairs
across 1,855 identities: Accton 415, Edge-Core 217, Celestica 190, Oracle 859,
Amazon Data Services 175. All associations have 3,653 pairs across 3,522 identities.
Factory counts are unchanged. No existing device, record, affiliation, identifier,
fact, hyperscaler evidence or original source claim was changed or removed.
All 1,081 raw RRA rows remain byte-equivalent at the field level. The ten-device
baseline's results, evidence and search hits are unchanged: 10/10 searchable,
8/10 corroborated. Both exact Edge-Core gaps remain open.

All 326 tests pass. A fresh offline rebuild using 87 jobs and 1,914 hashed files
completed at data/rebuilt-with-rra-manufacturer-lists.sqlite. All 37 compared
tables/search indexes and baseline results match live data. The documented SQL
returns 47 named-manufacturer affiliations across the 45 added records. The
delta/SQL proof check preserves database bytes and verifies previous evidence
against the prior verified rebuild. See research/rra-manufacturer-lists.md and
data/exports/rra-manufacturer-lists-* for review, additions, provenance and audit.
Full historical/FCC/worldwide coverage remains unproven; the goal remains active.

Final manufacturer-list verification: all 64 saved lookup reports cover 25
datasets. All three vendor inventories and the offline catalog are refreshed;
rra-manufacturer-lists-readonly-check.json verifies unchanged database bytes
during exports. All import, test, rebuild, comparison and refresh processes
are terminal.


## Interior model fragments in local search — 2026-09-27

The preceding goal turn made progress by adding reviewed manufacturer-list
assertions. Renewed exact Edge-Core searches still yielded seller ordering codes
and suffixed AZ4444 variants. A new lead is Master certificate MT-3593/2015 for
AS7710-32X/AS7760-32X: PDF text was readable through the web tool, then a direct
download returned HTTP 403. No original PDF was archived or data assertion made.
Do not retry or route around the denied URL:
https://manuals.plus/m/964c2777a02dbbbdc1eaf9a3a484d06a52b2255cfba6e0c96b47e81171d2275c.pdf
Selected factual leads and limitations are in research/as7760-certificate-lead.md.
FCC bulk searches again returned mainly ULS licensing/station projects; these
are not device-ID databases and were not downloaded.

The investigation exposed a concrete discovery bug: search 7760 returned only
the seller ordering code, missing both AS7760-32X identities. Catalog.search now
allows each query term to match its existing full-text word-prefix index or,
for terms of at least three characters, an interior fragment of the model or
explicit identity fields. Unicode case folding is used. Underscores separate
query terms just like punctuation. Every term must match; vendor/role/hyperscaler
filters apply after combination. Arbitrary fact and ONIE-target interiors are
not added to fragment matching. Short terms retain prefix semantics. Stored
identities, aliases, index text, validation and marketplace matching are unchanged.

Live examples: 7760 now returns 3 identities instead of 1; with Edge-Core and
registry_manufacturer it returns the existing AS7760-32X record instead of zero.
Celestica 8200 now finds 3 identities, 8210 finds 2; Edge-Core 7772 finds 2.
Thirteen search cases match the verified prior rebuild, with no prior result
lost in those cases. The measured maximum was 13.006 ms in this one local audit.
All 329 tests pass, including mixed term/filter behavior, literal operator
handling, Unicode, allowed fields, short terms, limits and read-only transactions.
Database bytes and all baseline results are unchanged. No schema, source data,
manifest or rebuild changes were required; the existing verified rebuilt database
remains current. Query exports and audit are in data/exports/search-fragment-*;
docs/model-fragment-search.md explains usage and boundaries.

Current counts remain 3,522 identities, 4,619 assertions and 189 source entries;
33 PDFs are archived. Baseline remains 10/10 searchable and 8/10 corroborated.
Both exact Edge-Core gaps and full historical/FCC/worldwide completeness remain
unproven. The goal remains active. All work processes in this turn are terminal.

## Offline company evidence scopes

The previous baseline-only continuation reconfirmed existing results without
adding coverage. This turn improves the local hunting workflow: the offline
browser now defaults to manufacturer claims and offers factory listings and all
associations. The Python report generator shares source eligibility and role
classification with vendor-inventory. Company, scope and individual role must
match the same affiliation; qualifying evidence never transfers from a different
company or source on the same device. Seller and unhashed claims remain available
under all associations. Cards expose the specific evidence matching the filters.

Regenerated data/exports/catalog.html. All 330 Python tests and six JavaScript
behavior tests pass. An audit executes the actual inline script against all
3,522 embedded identities and compares every displayed ID to fresh SQL inventory
results for three scopes and six company selections (18 passing comparisons).
Unfiltered scope totals are 1,855 manufacturer identities, two factory identities
and 3,522 all-association identities. Audit artifacts are report-scope-audit.json,
report-scope-expected.json and report-scope-audit.log in data/exports. This is a
minimal DOM behavior audit, not browser visual verification.

Database bytes remain unchanged (SHA-256
00c4f7ee086efbe18b0235dbb4ca314605e59aa0ed443089c8be24924fae2c02).
No source, schema, import manifest or baseline changes. Current coverage remains
3,522 identities, 4,619 assertions, 189 source entries and baseline 10/10
searchable, 8/10 corroborated. Full coverage and the two exact Edge-Core gaps
remain open; the goal remains active. All processes from this turn are terminal.

## Owner confirms full AZ4444 device name

On 2026-09-27 the owner clarified that AZ4444-52X was a simplified label and
the full name is AZ4444-52X-DC-10. Updated validation/owned-hardware.json with
the full name and explicit correction history. Existing anatel-products evidence
already provides the exact model and EdgeCore manufacturer on one source record;
the unchanged validator now returns 10/10 searchable and 9/10 corroborated.
The sole remaining baseline gap is 7760-32X-A-AC-F. Accton evidence stays in its
own namespace; no alias, new source assertion or AWS provenance was inferred.

The current database and existing verified rebuild give identical results under
the corrected fixture. Database bytes retain SHA-256
00c4f7ee086efbe18b0235dbb4ca314605e59aa0ed443089c8be24924fae2c02.
Updated inventory-validation.json and rebuild-validation.json; the scoped audit
is data/exports/az4444-baseline-correction.json. Earlier two-gap counts remain
historical. No import or report regeneration is needed for a fixture-only change.

Before this correction, a source discovery search located the Kenya CA register
at https://www.ca.go.ke/node/255, linking “List of Type Approved Equipment August
2026” (web page link 64). Only the index was read; no PDF was downloaded or data
imported. Its stated coverage is nonexhaustive. That new source remains a possible
broader coverage lead; do not repeat the now-resolved bare-AZ4444 investigation.

## Kenya CA register imported

Downloaded the original August 2026 equipment PDF and official index, screened
all 203 embedded-text pages, and visually reviewed the five target-bearing pages.
Six reviewed rows add three catalog identities and six manufacturer assertions:
Amazon Data Services TIMBERWLFL10, Edge-Core ACCTON-AS8060, and the full printed
ECS TABLET PC (TR10RS1) label are new; HOPPER26SF and ECS5550-30X gain additional
evidence. Brand, company/location, category, physical PDF page and printed page
remain separate. A two-line CAMILLO / GOLGI cell is retained as unresolved in the
archive rather than split or aliased. The tablet attribution and adjacent
conflicting ECS5550-30X manufacturer row are explicitly flagged as source claims.
No new hyperscaler relationship, factory proof or unit provenance was inferred.

All 203 pages are searchable with documents and SQL through the reference archive.
Current totals: 3,525 identities, 4,625 assertions, 191 source entries; 34 PDFs,
3,290 pages and 3,282 pages with embedded text. Manufacturer scope: 1,859
target/identity pairs, 1,858 distinct identities (Accton 415, Edge-Core 219,
Celestica 190, Oracle 859, Amazon Data Services 176). Factory scope remains two
Celestica identities. All associations: 3,656 pairs / 3,525 identities.

The fresh data/rebuilt-with-kenya.sqlite matches all 37 checked tables/indexes
and the corrected baseline using 88 import jobs / 1,919 hashed files. Prior
device evidence remains intact. Six archive tests and six inline-script tests
pass, along with 18 browser-script/SQL identity-set comparisons. This last check
uses the DOM harness; no browser visual QA is claimed. Inventories, offline HTML,
reference-document export and 67 lookup reports are refreshed. Read-only exports
preserve the database hash; see data/exports/kenya-export-audit.json.

Source review: research/kenya-approved-equipment.md. Reviewed source rows:
data/kenya-approved-reviewed.json. Original PDF and full text are in data/raw;
index and selected-page table capture are under data/raw/kenya-ca. The reference
bundle and all five new input files are covered by the rebuild manifest.

Baseline remains 10/10 searchable, 9/10 corroborated. The sole baseline gap is
7760-32X-A-AC-F. Full historical, worldwide and FCC device coverage remains
unproven; the goal stays active. All work processes in this turn are terminal.

## FCC report-cover manufacturer roles

Bulk EAS source screening did not establish a complete filing download. The newly
reviewed FCCIDSearch.com home page was web-readable, but direct /grantees and
/search downloads returned HTTP 403; both were stopped without retry or alternate
route. No filing data from those denied pages was saved or imported. Access notes
are data/raw/fccidsearch-review/access-results.json, outside the import manifest.

The existing cached FCCID.co excerpts contain explicitly labelled Sporton cover
fields. A new fcc-sporton-covers importer validates the reviewed heading/field
sequence, FCC ID, applicant/grantee, complete simple model tokens, original HTML
and excerpt hashes before replacing evidence. Seven reviewed excerpts supply ten
model assertions. Named manufacturer remains separate from applicant and brand;
seven assertions name Accton Technology Corporation, while Edimax, Joy Technology
and the unrecognized spelling Accton Networks Corporation remain unassigned to
target manufacturers. No family aliases, brand-list/model pairing, factory audit,
operator deployment or unit provenance is inferred. Source is regulatory-mirror
text with truncation preserved, not newly downloaded original PDFs.

Three new identities: Accton-namespace ECG9210-04 and SU-A-MB-12-EZ, plus
Edge-Core-namespace ECW7211-L. Namespace follows applicant and is not a manufacturer
claim. Current totals: 3,528 identities, 4,635 assertions, 192 sources. Manufacturer
scope is 1,864 target/identity pairs / 1,863 unique identities: Accton 420,
Edge-Core 219, Celestica 190, Oracle 859, Amazon Data Services 176. All associations
are 3,660 pairs / 3,528 identities. Factory, FCC index/detail and PDF counts are
unchanged. Existing device evidence remains intact.

All 335 Python tests pass, including five new role/field/capture/refresh checks.
Fresh data/rebuilt-with-fcc-sporton.sqlite reproduces all 37 checked tables/indexes
and baseline results using 89 import jobs and 1,920 hashed files. Six inline-script
tests and 18 live snapshot/SQL scope comparisons pass (no browser visual QA).
Inventories, offline HTML and 69 lookup reports are refreshed. Read-only exports
preserve SHA-256 aa5f1aa73df72a851e43e6f492ace6e34dca978bbea9c0425794f161a33339ba.

Source review: research/fcc-sporton-cover-roles.md; capture:
data/raw/fcc-sporton-covers.json; code: devicehunt/fcc_sporton.py. Exclusions,
source-role exports and audits are data/exports/fcc-sporton-*. Full original
PDF/exhaustive FCC coverage remains unproven. Baseline stays 10/10 searchable,
9/10 corroborated, with only 7760-32X-A-AC-F unresolved. The goal remains active;
all processes started in this turn are terminal.

## Single-scan local lookup

On 2026-09-27, removed duplicate filtered scans from unified lookup. Positive
limits now compute total matches with a window count before pagination;
count-only and unlimited requests each use one filtered scan. No schema, source,
identity, affiliation, or corroboration changes. Unicode literal matching,
duplicate rows, suffixes, source scope, transaction ownership and result order
are preserved.

All 336 Python tests pass, including a predicate-evaluation regression test for
positive, zero and unlimited limits with matches and no matches. A live audit
compared complete before/after outputs for 19 query/limit combinations across
all 25 datasets; every comparison agrees. Database bytes retain SHA-256
aa5f1aa73df72a851e43e6f492ace6e34dca978bbea9c0425794f161a33339ba.
Median wall time across three alternating runs improved from 2.6914 to 1.4746
seconds for Oracle and 2.1704 to 1.6321 seconds for Inc. These local measurements
are not guarantees, and counts-only requests already avoided duplicate scans.
The first TIMBERWLFL10 measurements overlapped the test suite and should not be
used for a clean speed comparison.

Evidence: data/exports/lookup-window-audit.json and .log,
data/exports/tests-lookup-window.log; notes: research/local-lookup.md.
No reimport or export refresh is needed because data and complete result payloads
are unchanged. Baseline remains 10/10 searchable, 9/10 corroborated; only
7760-32X-A-AC-F lacks qualifying exact-identity evidence. Exhaustive historical
device/FCC coverage remains unproven, so the goal remains active. Both audit and
test processes completed successfully.

## Additional public approval tables

On 2026-09-27, downloaded the Saint Vincent and the Grenadines NTRC public HTML
register (2,896 rows) and Mauritius ICTA's advertised All-rows public table query
(2,089 rows). ICTA's total and unfiltered count agree with the downloaded count;
a second response reproduces every ordered row. Its cached HTML preview trims
one trailing space retained in the API, so only preview comparison normalizes
whitespace. Original API cells remain verbatim. NTRC has no independently exposed
total or publication date, and neither snapshot proves complete history.

Schema 24 adds public_equipment_rows. The public-register importer validates
hashes, URLs, headings, widths, counts and API stability before replacing a
source's rows. Unified lookup now has 26 dataset sections, including these raw
tables with distinct brand and make columns, foreign-certification text, original
cells and source metadata. No manufacturer, model equivalence, current approval
validity or operator is inferred. There are no explicit target-company hits in
either company column; Amazon F100 Ghost is not attributed to Amazon Data Services.
No new catalog identities or assertions were manufactured to inflate coverage.

Current totals: 3,528 identities, 4,635 assertions, 194 sources, and 4,985 added
raw reference rows. Manufacturer inventory remains 1,864 target/identity pairs
and 1,863 unique identities. Baseline remains 10/10 searchable, 9/10 corroborated;
7760-32X-A-AC-F is still the exact-identity corroboration gap. The original FCC
filing database and exhaustive device coverage remain incomplete/unproven.

All 340 Python tests pass. The offline manifest now contains 91 jobs and 1,928
hashed files. A fresh data/rebuilt-with-public-registers.sqlite reproduces all
38 checked tables/indexes and the baseline. A separate comparison against the
previous Sporton rebuild proves all prior evidence is preserved (37 tables,
excluding just the two newly added source metadata rows). Every raw stored cell
equals its capture. Inventories, offline HTML and saved lookup reports refreshed.

Evidence: research/additional-public-registers.md;
data/exports/public-registers-import-audit.json,
public-registers-preservation.json, public-registers-export-audit.json,
rebuild-comparison.json, rebuild-validation.json and tests-public-registers.log.
All processes launched in this turn completed. The goal remains active.

## Celestica exact chassis ordering codes

On 2026-09-27, downloaded the original 49-page Celestica DS4000/DS4001 installation
guide. Physical pages 7 and 8 (printed pages 4 and 5) were rendered and visually
reviewed. Eight chassis rows supply six DS4000 and two DS4001 full ordering codes.
Each code is a part_number identifier on its own source record, attached to the
existing documented model family, with the full published configuration and page
URL. Cables and brackets in adjacent rows are excluded from chassis identifiers.
Brand/vendor evidence does not establish a physical factory or hyperscaler.
PSU ratings are not measured consumption; no auction-unit configuration or suffix
decoding is inferred. The whole guide is additionally archived for PDF search.

Input: data/celestica-chassis-skus-reviewed.json. Source:
celestica-ds4000-chassis-skus. Examples: R4023-F9102-A0 resolves to DS4000;
R3240-F9103-A0 resolves to DS4001. Eight independently re-extracted table rows
match the reviewed cells, stored facts and identifiers. All eight pass literal
lookup and exact auction-title identifier matching; unknown extensions remain
variant leads. Prior device records, identifiers, affiliations, facts, hyperscaler
evidence, documents/pages and public-register rows are retained.

Totals: 3,528 identities, 4,643 assertions, 196 sources. This is eight added
ordering configurations, not eight new model families. Manufacturer inventory
remains 1,864 target/identity pairs and 1,863 unique identities. PDF archive:
35 documents, 3,339 pages, 3,331 with embedded text; the 24 non-FCC reference
documents contribute 3,265 pages. The manifest has 92 jobs and 1,931 hashed files.
A fresh data/rebuilt-with-celestica-skus.sqlite agrees across all 38 checked
tables/indexes and the owned baseline. No application code changed in this pass;
validation used source re-extraction, exact lookup/listing checks, preservation
checks and the full offline rebuild. Inventories, target CSV/JSON, offline HTML
and all 73 saved lookup reports are refreshed.

Evidence: research/celestica-chassis-ordering-codes.md;
data/exports/celestica-chassis-skus.csv and .json; celestica-skus-audit.json;
celestica-skus-preservation.json; celestica-skus-export-audit.json;
rebuild-comparison.json and rebuild-validation.json. Export/audit reads preserve
database SHA-256 c2724a4e09998fa5f31c11a880b16616fc226bdd189c5af3f9beeb3b18ce4eb7.

Renewed 7760-32X-A-AC-F research still returned seller descriptions, not qualifying
independent exact-code evidence. Baseline stays 10/10 searchable and 9/10
corroborated. Full original FCC and exhaustive historical device coverage remain
unproven. All started processes are terminal; the goal remains active.

## Exact part-number specification lookup

On 2026-09-27, added specs --exact-part-number for configuration-level buying
research. It selects only categorized specification statements on records that
explicitly identify the complete part_number. Vendor filtering is bound to those
same records. It does not borrow another SKU's or the model family's statements,
nor treat aliases, textual mentions or unknown suffixes as exact part numbers.
Case folding and outer-whitespace trimming preserve punctuation and suffixes.
Results retain record IDs, original matching identifiers, revisions, source URLs
and tiers; conflicting claims remain separate. A known part with no categorized
specifications returns an empty result. Existing model and broad-search modes are
unchanged; the two exact modes are mutually exclusive.

All 345 Python tests pass, including five new tests for same-record vendor/fact
scope, seller-tier preservation, literal Unicode, missing search index,
suffixes, empty specifications, collisions across revisions, limits, read-only
CLI output and incompatible modes. A live audit verifies all eight Celestica
chassis SKUs each return their own two categorized statements; unknown extensions
return none. Example: python3 -m devicehunt specs R4023-F9102-A0 --exact-part-number.

Evidence: data/exports/exact-part-specs-audit.json and tests-exact-part-specs.log.
All-specification CSV/JSON exports and the R4023-F9102-A0 example are refreshed.
No source, schema or database content changed; audit SHA-256 remains
c2724a4e09998fa5f31c11a880b16616fc226bdd189c5af3f9beeb3b18ce4eb7. Existing offline
rebuild evidence remains applicable. Baseline is still 10/10 searchable, 9/10
corroborated. Full inventory/FCC coverage and the exact 7760 corroboration gap
remain open. The test process is terminal; the goal remains active.

## MetroLinq manufacturer fields from mirrored document text

On 2026-09-27, followed up Nigeria NCC's three unassigned Accton Tech.
Corporation rows. The previous correction-confirmation turn added no new
coverage; this turn adds three source assertions through the existing curated
importer. The original Sporton report's indexed cover explicitly names Accton
Technology Corp and Joy Technology (Shen Zhen) Co. Ltd as manufacturers for
ML1-60-35 and ML1-60-19. The ML-60-10G-360 quick-start guide's web-tool text
explicitly names Accton Technology Corporation. All are IgniteNet-branded.
Applicant roles, both printed manufacturers and complete model labels remain
separate; there is no global company-alias expansion or cross-brand merge.
ML2.5-60-BF-18 and the Nigeria abbreviation remain unresolved manufacturer leads.

Both original-byte PDF download attempts returned HTTP 403 and were not retried
or downloaded elsewhere. Added those URLs to the documented access limitations
in research/metrolinq-manufacturer-evidence.md. Local snapshots contain reviewed
factual extracts only, labeled primary_document_mirror, not original PDFs or
regulator retrievals. No visual PDF verification, electrical guidance, factory
assignment, current approval or AWS provenance is claimed.

Authoritative totals are 3,530 identities, 4,646 assertions and 198 sources.
Manufacturer scope has 1,867 target/identity pairs and 1,866 unique identities:
Accton 423, Edge-Core 219, Celestica 190, Oracle 859, Amazon Data Services 176.
The two additional brand/model identities do not imply two new physical designs.
The manifest now has 93 jobs and 1,934 hashed files; PDF archive counts remain
35 documents / 3,339 pages because the new extracts are not PDF archives.

Live import checks validate explicit target roles, unmapped Joy Technology,
printed FCC-ID scope and absence of hyperscaler assertions. Preservation checks
confirm no earlier rows disappeared from 36 non-FTS tables. A fresh rebuild at
data/rebuilt-with-metrolinq.sqlite matches all 38 compared tables/indexes and
owned-baseline results. All three model mentions match their sourced IgniteNet
identities; unknown suffixes are not exact matches. Embedded offline HTML data
contains all three manufacturer claims; visual browser review was not performed.
Application code is unchanged; verification used data-specific assertions and
full offline reconstruction rather than repeating the unchanged unit suite.

Inventories, target CSV/JSON, offline HTML and saved lookup reports are refreshed.
Evidence: data/exports/metrolinq-import-audit.json, metrolinq-preservation.json,
metrolinq-search-audit.json, metrolinq-listing-checks.json, metrolinq-export-audit.json,
metrolinq-manufacturer-claims.csv/.json and rebuild-comparison.json.
Read-only exports/checks preserve database SHA256
49c62b8d2be085f63b91d9781223a2b8a4383ca1bdeae66afab0504fe522b13b.
Baseline remains 10/10 searchable, 9/10 corroborated; 7760-32X-A-AC-F is still
seller-only. Full original FCC filings and exhaustive historical device coverage
remain unproven, so the goal remains active. All started processes are terminal.

## FCC collection gap report

On 2026-09-27, reviewed the existing FCC company-index scope before seeking
broader coverage. The previous turn made verified progress by importing three
MetroLinq manufacturer assertions. An additional Electric Garden Accton-index
download returned HTTP 403 and was stopped without retry or alternate download:
https://electric.garden/accton-hed . No data from that attempt was imported.

Added devicehunt/fcc_coverage.py and CLI fcc-coverage. It compares explicit
locally known target-associated identifier strings across discovery indexes,
captured detail pages, application records and archived PDF metadata. Company
filtering for catalog claims is bound to the same source record as the explicit
fcc_id identifier; no company association is borrowed from another record for
the model. Stored reviewed grantee-code associations supply additional discovery
IDs. Original roles, source tiers, duplicate mirror rows, exact suffixes and
failed capture reasons remain visible. Related-ID fields and arbitrary text do
not seed the report. Missing tables are unknown, not zero coverage, and caller
transactions are preserved. No network, migration, download or import is performed.

The current report has 244 distinct case-insensitive, whitespace-trimmed FCC ID
strings, preserving all punctuation. Of these, 173 have index records, 171 have
captured detail pages, two have detailed application records, and one has FCC-
linked archived PDFs (11 documents). The 71 strings without an index record are
collection leads, not proven new authorizations; spelling variants and source
strength remain review items. Per-company identifier sets can overlap. Complete
filing/exhibit coverage is not established by any individual collection stage.

Five new tests plus the existing suite pass (350 total). Tests exercise stage
separation, duplicates, failed detail captures, same-record vendor scope,
seller provenance, application-only IDs, literal suffixes, missing tables,
limits, transaction ownership and read-only CLI formats. Independent SQL agrees
with all five live summary counts. Saved seven report pairs for all companies,
missing-index entries and each vendor, plus fcc-coverage-sql-audit.json and
fcc-coverage-readonly-audit.json. Documentation: research/fcc-collection-coverage.md.

Database contents and schema remain unchanged at 3,530 identities, 4,646
assertions and 198 sources. Read-only checks preserve SHA256
49c62b8d2be085f63b91d9781223a2b8a4383ca1bdeae66afab0504fe522b13b.
The existing full rebuild comparison remains applicable; owned baseline remains
10/10 searchable and 9/10 corroborated. Full original FCC coverage, exhaustive
historical manufacturer inventory and exact 7760-32X-A-AC-F corroboration remain
unproven. All launched processes are terminal; goal remains active. The new gap
reports provide specific existing-source-backed FCC identifiers for follow-up.

## Verified missing-detail probes retained locally

On 2026-09-27, followed the FCC coverage report added in the previous turn
(verified code/report progress). Probed the normal FCCID.co detail URLs for
HEDML2560, HEDML60LW and HEDMR3202A. All three returned original HTML explicitly
titled Not Found | FCC ID Database, with no device heading, application-details
section or exhibit links. Saved the HTML and reviewed URL/hash metadata under
data/raw/fcc-gap-probes/. This establishes a gap in that mirror, not absence of
FCC filings or devices. No denied endpoint was retried.

The separate Accton China legal-name lead remains unresolved: an original
2018 consolidated-report download returned HTTP 403 and was stopped. Do not
retry or obtain the same file through an alternate download:
https://www.accton.com/wp-content/uploads/2019/05/2018-Consolidated-Report.pdf .
No company mapping or OS-LS-6248 manufacturer assertion was added.

Added the fcc-gap-probes importer and dispatcher entry. All page contents,
IDs, URLs, filenames and checksums are validated before replacing this source.
It stores only unavailable detail attempts, preserving dated review metadata
and blank grantee/company fields. No model, manufacturer, index, application,
exhibit or document claims are generated. Other mirrors' successful captures
remain separate and win the coverage report's collected-stage check. Raw lookup
now searches failure status/issue as well as ID and captured fields, and its
scope describes supplemental probes correctly.

All 353 tests pass, including three new importer tests and regression checks
for failure lookup, duplicate/malformed/contradictory pages, path/hash/URL guards,
scoped idempotent refresh, retained success from another source, company non-
inference and zero fabricated filing/model rows. Live SQL confirms the three
unavailable rows. All previous rows remain unchanged across 38 checked tables
and indexes; the only additions are one source and three negative detail rows.
A fresh data/rebuilt-with-fcc-gap-probes.sqlite matches all 38 tables/indexes and
the baseline. No schema change was needed.

Current catalog: 3,530 identities, 4,646 assertions, 199 sources. Raw detail table:
174 attempts, with 171 captured pages and three unavailable probes. Collected
FCC-stage coverage remains unchanged: 244 known identifier strings, 173 indexed,
171 with captured detail, two with application records and one with eleven
archived PDFs. The manifest has 94 jobs and 1,938 hashed files. Manufacturer
scope remains 1,867 target/identity pairs across 1,866 identities.

Inventories, offline HTML, detail CSV/JSON and seven FCC coverage reports were
refreshed. Saved lookup reports were refreshed again after adding issue/status
search and correcting the section scope. Evidence includes
research/fcc-gap-probes.md; data/exports/fcc-gap-probes-import-audit.json,
fcc-gap-probes-preservation.json, fcc-gap-probes-export-audit.json,
fcc-probe-lookup-scope-audit.json, tests-fcc-gap-probes.log and the fresh rebuild
comparison. Database SHA256:
91648946b4622adbf5b49e3e78cbd15ccfdad8712adacbda4a87c7de495077af.
The 10-device baseline remains 10/10 searchable and 9/10 corroborated, with
7760-32X-A-AC-F the seller-only gap. Full FCC and exhaustive historical target
inventory coverage remain unproven. The goal remains active.

Final export verification: all 79 saved query-format lookup reports carry the
updated section scope, and both live failure-status/issue searches return the
three probes with source metadata. Read-only export hash is unchanged. All
started processes, including the second lookup refresh, are terminal.

## Japanese approval mirror queries in local SQL

On 2026-09-27, pursued Japanese radio-equipment coverage after the prior turn's
verified progress preserving three FCC mirror gaps. The e-Gov API catalog lists
an official MIC equipment-search API with JSON/XML/CSV output. Its linked guide
returned HTTP 403 and was not retried or used to invent API parameters:
https://www.tele.soumu.go.jp/j/sys/equ/tech/webapi/ . No official API records
were imported. Preserve this URL as an access limitation for future work.

The independent Lang-ship mirror was accessible through its public search form.
Captured all returned rows for six English company-name queries twice: Accton
177, Edgecore 21, Edge-Core zero, Celestica zero, Oracle one, Amazon Data Services
zero. Original HTML is stored in data/raw/japan-mic/, with reviewed URLs and
checksums. Both captures agree on ordered display cells and certificate links.
Each query is below the displayed 2,000-row cap; no independent total or full
Japanese/alternate-name coverage was established. Operator documentation says
its upstream data undergoes fullwidth-to-halfwidth normalization. Source tier
is regulatory_mirror. The official guide and unrelated future-dated home-page
entries are not imported as approval evidence.

Added devicehunt/japan_mic.py, a japan-mic import dispatcher, schema version 25
and the raw japan_equipment table. It preserves six source fields, full grouped
model strings, radio classes, duplicates, certificate links, source query URLs
and display-cell JSON. Registered person/entity name is not automatically
manufacturer, brand, factory or operator. No device identities, affiliations,
model aliases or FCC claims are generated. Accton Wireless Broadband matches
remain their separate printed name. The 199 rows contain 146 distinct approval
strings and 86 model-field strings; neither is a physical-device count.

Unified lookup now includes a Japanese section and all six query scopes,
including zero-result captures. Queries for Bump Bar K3-10W, ML2.5-60-BF-18 and
EAP112 return one, two and two Japanese rows respectively. The Oracle field is
Bump Bar K3-10W, Bump Bar K3-20W under approval 219-267009; it stays grouped.
CSV/JSON exports: data/exports/japan-equipment.csv and .json.
Documentation: research/japan-mic-mirror.md.

All 356 tests pass, including three new importer cases covering raw duplicates,
grouped and Unicode labels, zero results, changed captures, malformed headers,
filters and links, hash/path/count guards, cap rejection and scoped refresh.
Every live row and detail URL agrees with fresh parsing of both original
captures. The earlier 38 tables/indexes retain all their rows, with only six
new source rows; no catalog assertion changed. A fresh rebuild at
data/rebuilt-with-japan.sqlite matches all 39 compared tables/indexes and the
owned baseline. Inventories, offline HTML and all 82 saved query-format lookup
reports are refreshed. The manifest has 95 jobs and 1,954 hashed files.

Current catalog: 3,530 identities, 4,646 assertions and 205 sources. Manufacturer
scope remains 1,867 target/identity pairs across 1,866 identities. Baseline is
10/10 searchable and 9/10 corroborated; 7760-32X-A-AC-F remains seller-only.
Database SHA256:
55be3d43844851e4a40912ece192709ffc5d715d73c8a8f48e66ce7d872bd2c8.
Audits: japan-import-audit.json, japan-sql-lookup-audit.json,
japan-preservation.json, japan-export-audit.json, tests-japan-mic.log and the
fresh rebuild comparison under data/exports/. All started processes are terminal.
Full worldwide/FCC device coverage and exact remaining baseline corroboration
are unproven; the goal remains active. Wider company-name and language queries
on the Japanese source are possible follow-up coverage work, subject to its cap.

## Japanese company-name variants

On 2026-09-27, expanded the previous turn's verified Japanese-query import with
six additional name searches. Downloaded each public query twice and checked
all ordered display cells and detail links for equality. Results: Amazon 143;
アクトン four; オラクル, セレスティカ, エッジコア and 智邦 zero each. The original
six query sources and their 199 rows remain intact. All new queries are below
the mirror's 2,000-row display cap, without an independent completeness total.

The broad Amazon results name Amazon.com Services LLC (two capitalizations),
Amazon com Services, Inc, Amazon Robotics, Amazon Robotics LLC, and unrelated
Amazonen-Werke H. Dreyer GmbH & Co. KG. None names Amazon Data Services. No
company equivalence or hyperscaler role was inferred. The Japanese name
アクトンテクノロジィ株式会社 appears with AD11A-1 (NYCA0053/GZCA0036) and AD11C-1
(NYCA0054/GZCA0037), all displaying 2000-10-03. Names, two radio-class rows per
model and date strings remain raw mirror claims; no English corporate alias,
manufacturer or catalog identity was added.

The japan-mic importer now supports two explicit disjoint query collections,
base and name_variants. Each validates its own full set and replaces only its
sources. Old base bundles without a collection field remain valid. Lookup
includes all Japanese source metadata dynamically, preserving additional zero-
result query scopes. A new regression verifies coexisting collections, rejects
relabelling one as the other, preserves the base on supplemental refresh, and
keeps Japanese company text from becoming a target affiliation. All 357 tests
pass. Live re-parsing verifies every new field and certificate link; exact raw
lookup returns two rows each for AD11A-1 and AD11C-1, zero for Amazon Data
Services, with all 12 source scopes displayed.

Combined Japanese table: 346 rows, 191 distinct approval-number strings and 131
model-field strings, not unique physical-device counts. Raw inputs are under
data/raw/japan-mic-name-review/; supplementary exports are japan-name-review.csv
and .json; combined japan-equipment exports are refreshed. The manifest has
96 jobs and 1,969 hashed files. Catalog identities/assertions remain 3,530/4,646;
sources are now 211. Schema stays 25 and manufacturer inventory stays 1,867
target/identity pairs across 1,866 identities. No catalog affiliation changed.

Database SHA256:
f12b45781a54f1020450f634f65322bb8ff30b10b0063fe97595a195f6aa43ae.
Evidence includes japan-name-import-audit.json, japan-name-sql-lookup-audit.json,
japan-names-preservation.json, japan-names-export-audit.json and
japan-names-rebuild-comparison.log under data/exports/, plus the updated
research/japan-mic-mirror.md. Baseline remains 10/10 searchable and 9/10
corroborated, with 7760-32X-A-AC-F the seller-only gap. Complete FCC/worldwide
inventory remains unproven; goal stays active.

Final verification: the fresh rebuild matches all 39 tables/indexes and the
owned baseline. The preservation audit finds no prior rows missing. All 85
saved lookup reports are refreshed; read-only exports leave database bytes
unchanged. All processes started for this expansion have completed.

## Celestica storage manufacturer claims from original product pages

On 2026-09-27, downloaded original model-specific Celestica HTML for SC6100,
SD4100 and SD6200 (HTTP 200, unchanged final URLs). Checked each document title,
model heading, full chassis-description heading and explicit manufacturer
paragraph before importing. The three identities previously had brand and
listing-holder evidence only. Added three source-specific manufacturer records;
Celestica manufacturer inventory rises from 190 to 193 identities. No identity,
alias, SKU, factory, subsidiary, deployment claim or hyperscaler assertion was
created. Chassis specifications do not establish installed drives or the
configuration of a surplus unit.

Reviewed input: data/celestica-storage-manufacturer-reviewed.json. Original
HTML, retrieval metadata and checked field extracts are retained under
data/raw/celestica-storage-manufacturer/. Replay uses the existing curated
importer. The manifest now has 97 import jobs and 1,975 hashed files. Current
catalog: 3,530 identities, 4,649 records, 214 sources; schema remains 25.
Manufacturer inventory: 1,870 target/identity pairs across 1,869 identities.

The complete 357-test suite passes. The fresh offline rebuild is
data/rebuilt-with-storage-manufacturer.sqlite; all 39 compared tables/indexes
and baseline results match the live database. A separate preservation audit
confirms all previous source evidence survives, with exactly the three existing
model search documents updated. Exact model specifications and auction-title
mentions expose the new sources; an unknown suffix returns only variant leads,
not an exact model or ordering-code match. Owned baseline remains 10/10
searchable and 9/10 corroborated.

Database SHA256:
a328ab7c73dd222150c75aed4b880b3c3de91a3e9332473d8996cf387b3466be.
Audits: celestica-storage-manufacturer-import-audit.json,
celestica-storage-manufacturer-search-audit.json,
storage-manufacturer-preservation.json, storage-manufacturer-rebuild-comparison.log,
and tests-storage-manufacturer.log under data/exports/. Source scope and SQL
example are documented in research/celestica-storage-manufacturer.md.

Research boundary: another exact full-code search still returned marketplace
claims for 7760-32X-A-AC-F. No baseline relaxation or AS7760 equivalence was
introduced. Juniper's public Compliance Advisor landing page states that login
is needed for more information; no authenticated access was attempted. Indexed
EX4100 certificate compilation excerpts are leads only, not new factory
assertions. Earlier denied downloads were not retried. Full historical device
and FCC coverage remains unproven, so the goal stays active.

Final exports: offline catalog, target CSV/JSON and all three inventory scopes
are refreshed, along with 88 saved lookup reports. The export audit confirms
unchanged database bytes. All processes started for this addition are terminal.

## Source-scoped specifications alongside auction-title matches

On 2026-09-27, added optional `match-listing --with-specs` for single titles and
CSV batches. The matcher loads categorized facts and part-number scopes in its
existing consistent read snapshot. Only exact model/identifier mentions receive
statements from that same record. Variant leads receive an empty list. Records
without categorized facts do not borrow family, sibling SKU or other-vendor
statements. Original wording, qualifiers, source IDs/URLs/hashes, company roles,
marketplace tiers and device revisions remain visible. Family-model searches
retain each configuration's record part numbers rather than implying that all
configuration statements apply to an auction unit.

JSON includes specifications and record_part_numbers on each evidence match.
Opt-in table/CSV output adds the candidate revision and a specifications_json
column with full record provenance. It groups repeated identifiers from one
record without duplicating statements and preserves empty results. Default
matching output is unchanged. No network request or catalog mutation is added.

Five new regression tests cover conflicting front/back configurations, exact
part-number/vendor/record binding, revisions and seller tiers, blank records,
alias and unknown-suffix isolation, family scope labels, CSV/JSON batch output,
limits, unmatched rows, stable snapshot behavior, caller transactions and result
mutation isolation. All 362 tests pass. Live checks cover the eight reviewed
Celestica chassis SKUs, each with its two original configuration statements;
all unknown suffix probes receive no statements. The DS4000 family preserves
six separately labelled chassis configurations. SC6100, SD4100 and SD6200 each
expose the manufacturer-page chassis description under its own source.

Examples are illustrative titles, not live offers: data/exports/
listing-specifications-examples.json and .csv, plus the actual CLI result in
listing-specifications-cli-example.json. Audit: listing-specifications-audit.json;
full tests: tests-listing-specifications.log. Guide: docs/listing-matching.md.
No source, manifest, schema, inventory or database bytes changed. SHA256 remains
a328ab7c73dd222150c75aed4b880b3c3de91a3e9332473d8996cf387b3466be.
The previous complete offline rebuild remains the verified data snapshot.
Owned baseline is still 10/10 searchable and 9/10 corroborated; the remaining
7760-32X-A-AC-F gap and complete historical/FCC coverage remain unproven.
All started processes have completed; the goal remains active.

## Original Wedge 100 manufacturer evidence and spaced-model matching

On 2026-09-27, checked Facebook's original 2016-10-18 article against the saved
HTML and live page. Added source meta-wedge100-2016 with two records: Wedge 100
in the Accton manufacturer namespace, and Edgecore Wedge 100-32X. Both explicitly
name Accton Technology as manufacturer. Historical Facebook production use
applies to Wedge 100; the commercial 32X record records design lineage only.
No auction-unit provenance, current availability, new alias, or equivalence of
spaced/unspaced or cross-brand identities was inferred. Rack/power statements
stay on Wedge 100 and the historical shipping-software statement stays on 32X.

Current catalog: 3,532 identities, 4,651 source records, 215 sources and 15
hyperscaler relationship records. Manufacturer inventory has 1,872 target/identity
pairs across 1,871 identities. Accton increases from 423 to 425; the other target
manufacturer counts remain Edge-Core 219, Celestica 193, Oracle 859 and Amazon
Data Services 176. Source roles stay distinct from retail brand labels.

Live auction-title checks exposed a pre-existing gap: suffix matching considered
single tokens only, so spaced names with extra suffixes produced no variant
leads. Matcher now precompiles full-phrase extension patterns and shorter final
code prefixes. It preserves word boundaries, spaces, source/vendor scopes and
exact-match precedence. Variant leads receive no specifications. Two regression
tests cover spaced models/aliases, multiple suffixes, unknown extensions, case,
Unicode dashes, boundaries, separate unspaced identities and record-specific
facts. All 364 tests pass; exact and variant live checks pass too.

The manifest now contains 98 import jobs and 1,979 hashed files. Offline rebuild
at data/rebuilt-with-wedge100.sqlite matches the live catalog across all 39
compared tables/indexes and the complete owned baseline. Prior rows and search
document text are preserved; only two new search documents are added. FTS rowids
can move on reindex, so preservation compares device IDs/text while the fresh
rebuild comparison also checks rowids. Owned baseline stays 10/10 searchable and
9/10 corroborated. 7760-32X-A-AC-F remains seller-only.

Evidence guide: research/meta-wedge100-evidence.md. Audit outputs under
data/exports/ use meta-wedge100- prefixes; tests-meta-wedge100.log records tests.
Database SHA256: 86d17f9fbd4dd94bc31045b2626f9e23565c3e08321e07079125c3c612413633.

Full historical device and FCC coverage remains unproven. This addition closes
neither that completeness requirement nor the outstanding baseline gap; the
goal remains active.

Final export verification: the offline HTML catalog, target CSV/JSON, all three
vendor-inventory scopes and 90 saved lookup reports are refreshed. Exporting
left the database bytes unchanged. The new source is present in catalog.html.
All processes started for this addition and matcher fix have completed.

## Specification queries bind aliases and company filters to source records

On 2026-09-27, a live audit found that specs --vendor filtered device identities
before returning every statement on those identities. That let a different
record supply the vendor association. Exact model aliases likewise selected
all sibling configuration records. Corrected both paths: vendor associations
must now occur on each returned statement's own record, and exact aliases match
only the records carrying them. Direct family model queries retain all qualified
records and their conflicting configurations. Exact model/alias matching uses
complete Unicode case-folded strings without the discovery index. Complete
part-number matching retains its existing record scope.

All specification modes now expose record_id and revision, preserving source
URLs/tiers and separate claims. CLI help and docs/specification-search.md explain
the scope. Default broad search can still discover compatible accessories; a
vendor association does not itself prove manufacturing. Exact suffixes and
internal spacing are preserved. No-fact aliases cannot borrow sibling facts.

Live before/after evidence in data/exports/specification-scope-before.json and
specification-scope-audit.json shows all 913 unfiltered statements preserved.
Accton-filtered statements fall from 105 to 29: 76 lack an Accton affiliation on
their own record. Other vendor counts remain Edge-Core 719, Celestica 69,
Oracle 123 and Amazon Data Services 0. These are statement counts, not device
counts. All 277 model strings with categorized specifications were checked for
preservation of directly matching source records. General specifications JSON/CSV,
five per-vendor JSON exports and the DCS204 CSV example are refreshed.

Four regression tests cover record-bound vendor/alias scope, no-fact aliases,
collisions across models and revisions, literal Unicode/punctuation/suffixes,
index-independent exact lookup, limits and read-only CLI JSON/CSV output.
All 368 tests pass (data/exports/tests-specification-scope.log). Catalog rows,
sources, manifest and database bytes are unchanged. Database SHA256 remains
86d17f9fbd4dd94bc31045b2626f9e23565c3e08321e07079125c3c612413633;
the preceding verified complete rebuild remains current. The owned baseline
remains 10/10 searchable and 9/10 corroborated. All processes started for this
fix have completed. Complete historical/FCC coverage and corroboration of the
full 7760-32X-A-AC-F label remain open, so the goal remains active.

## Original Minipack2 reference PDF archived and indexed

On 2026-09-27, downloaded the original Meta-authored OCP Minipack2 generic
specification (Rev 1.0, document date 2021-09-16). The PDF was retrieved with
HTTP 200, saved unchanged and hashed. All 32 pages have embedded text. pypdf
reported cross-reference warnings while recovering the document; rendered pages
9, 13, 29 and 31 were reviewed against the extracted text. No OCR was performed.

The reference supplies useful generic requirements, historical Meta F16 platform
use, and front-panel/board label locations for auction-photo research. The
supplier appendix is blank and Celestica is absent from the extracted text.
Existing Minipack2, Minipack2-AC and Minipack2-48VDC-F records remain UL
listing-holder claims. No new identity, manufacturer, alias, deployment-table
assertion or suffixed-unit specification was inferred from this PDF. It is
searchable as document text, not a qualified Celestica specs record.

A separate rackcdn OCP presentation surfaced in search. Its attempted download
failed the script's PDF signature/size assertion; no PDF was saved and the
response detail was not retained. The web open also failed. No supplier claim
relies on this retrieval or a third-party copy. Earlier denied UL downloads
were not retried.

The reference collection has 25 PDFs/3,297 pages (3,295 with text). Across all
collections there are 36 PDFs/3,371 pages. Sources increase to 216; device
identities remain 3,532, records 4,651 and hyperscaler relationship rows 15.
The manifest retains 98 jobs and now hashes 1,983 files. Reference bundle
metadata hashes change for its old 24 archive sources; their individual PDF
hashes, full page contents and other fields remain preserved. Device claims,
raw registers and device-search text are unchanged. The only new source is
reference-pdf-meta-minipack2-spec.

Twelve document-import/search tests pass. Live searches verify all 32 pages and
four targeted queries (deployment, nominal input voltage, asset label and
supplier fields), plus the documented SQL example. The owned baseline remains
10/10 searchable and 9/10 corroborated. Complete offline rebuild at
data/rebuilt-with-minipack2-reference.sqlite matches all 39 tables/indexes
and complete baseline results. Audits use minipack2-reference- prefixes under
data/exports/; source scope and commands are in research/minipack2-reference.md.
Database SHA256: 946fd162b2d5c3a79c2dd5795e320ec565f99e1576e4bb77e80c84b97438566f.

Full historical/FCC coverage and independent 7760-32X-A-AC-F corroboration
remain unproven; the goal remains active.

Final refresh completed: offline HTML, target CSV/JSON, three vendor inventory
scopes and 91 saved lookup reports are current. Exporting preserved the database
bytes. All processes started for the Minipack2 reference addition are terminal.

## Source-scoped hyperscaler inventory and readable shortlist

On 2026-09-27, added the read-only hyperscaler-inventory command. Existing
search --hyperscaler returns identity names; this new report exposes each
relationship's detail, source URL/tier/hash/revision, publication facts and
company roles. JSON/CSV preserve record IDs and matching affiliations. Table
output shows compact company-role and date summaries. A readable linked
shortlist is saved at data/exports/hyperscaler-shortlist.md.

Default output requires the same external-source snapshot metadata eligibility
as vendor-inventory. It includes 11 assertions/11 identities: 3 documented
platform deployments, 4 design-lineage claims, 1 deployment announcement and
3 product-family connections. --include-unverified exposes all 15 assertions
across 13 identities, with four uncached claims retaining their weak status.
No weaker or unknown relationship becomes a deployment claim. Eligibility is
based on saved metadata, not a fresh byte-level or independent factual audit.

Vendor and exact role filters must match one affiliation on the hyperscaler
relationship's own record. No brand mapping, manufacturer claim, target legal
entity or hyperscaler relationship is borrowed from another record. Operator
filters compare complete labels case-insensitively; relationship names match
literally. Counts are computed before pagination. Source dates are historical,
and retrieval timestamps do not become deployment dates. The new report cannot
prove individual-unit provenance, present availability or a complete inventory.

Live audit preserves all 15 hyperscaler_evidence rows and verifies affiliations
against their actual record IDs. Eligible vendor-filtered assertions: Accton 4,
Edge-Core 3, Celestica 3, Oracle 0, Amazon Data Services 0. Counts overlap and
are relationship assertions, not unique physical product totals. The three AWS
product-family rows do not carry an ADS legal-entity affiliation on that same
record, so the latter filter is empty despite other ADS catalog evidence.
Documented deployment plus literal manufacturer role yields Wedge 100, Wedge 400
and Wedge 400C. Minipack2 remains separate document evidence.

Six regression tests cover record/affiliation scope, seller and uncached
eligibility, future relationship preservation, distinct revisions and dates,
limits/exact filters, JSON/CSV/table output, missing database handling, read-only
behavior and preservation of caller transactions. All 374 tests pass in
data/exports/tests-hyperscaler-inventory.log. Live results, audit and eight
JSON/CSV report pairs use hyperscaler- prefixes under data/exports/.
Guide: docs/hyperscaler-inventory.md. The desktop request to show the readable
shortlist returned queued; no rendered-view verification is claimed.

No catalog source, schema, manifest or database bytes changed. SHA256 remains
946fd162b2d5c3a79c2dd5795e320ec565f99e1576e4bb77e80c84b97438566f.
The preceding verified offline rebuild remains current. Owned validation stays
10/10 searchable and 9/10 corroborated; complete historical/FCC coverage and
7760-32X-A-AC-F corroboration remain open. All started processes are terminal;
the goal remains active.

## Cached AWS Monitron claims linked to product evidence

On 2026-09-27, followed up on the hyperscaler report's uncached claims. The
monitron-model source used the exact original AWS safety-index URL already saved
in the separately validated Monitron bundle. Its original HTML is now attached
after checking the bundle hash and TE1A001 guide link. The source's outdated
uncached coverage note is corrected. No external retrieval was needed.

The Monitron importer now adds AWS/documented_product relationships to its five
existing validated guide records: TE1A001, TE1A105, TE1A195, TG1A001 and TG1A002.
The first three retain explicit ADS manufacturer roles; gateways retain only
compliance-declarant roles. Each relationship explains the industrial monitoring
scope and excludes data-center deployment and unit-history/service-entitlement
inference. Service-history records do not gain hyperscaler relationships. The
legacy index record remains documentation-only and borrows no manufacturer role.

The default report now includes 17 eligible assertions across 16 identities.
Six product assertions cover five Monitron models because TE1A001 has separate
index and guide records; they are not independent votes. Including unverified
claims yields 20 assertions across 17 identities, with three remaining uncached
legacy claims. The ADS vendor filter now returns five product relationships,
three with manufacturer roles and two with declarant roles. Documented deployment
remains three assertions globally and zero for the strict ADS record filter.

No device identity, source-record count, manufacturer/factory inventory, alias
or prior relationship is removed or changed. Exactly five new product links
and five changed search documents are verified. Five source metadata rows change:
four guide coverage descriptions and the legacy index source's snapshot/coverage.
The reviewed curated-file hash is updated in the existing 98-job/1,983-file
manifest; all raw dependencies were already archived. Catalog counts remain
3,532 identities, 4,651 records and 216 sources.

The expanded importer regression verifies all five links, repeated import,
company/role boundaries and no deployment or service-history promotion.
All 374 tests pass. A complete rebuild at data/rebuilt-with-monitron-links.sqlite
matches the live catalog across all 39 tables/indexes and full baseline results.
Preservation, import, hyperscaler report, export and rebuild audits use the
monitron-product-links- prefix under data/exports/. Current report docs and the
source review in research/monitron-product-links.md are updated. The generic
hyperscaler report audit also reflects the refreshed data.

Offline HTML, target exports, three inventory scopes, hyperscaler JSON/CSV
reports, readable shortlist and 91 saved lookups are refreshed. Exporting leaves
database bytes unchanged. SHA256:
921e3b045e38ad804dc82926dedf9c74e207a64358362106215c31f140b437da.
The owned baseline remains 10/10 searchable and 9/10 corroborated. Full historical
and FCC coverage and 7760-32X-A-AC-F corroboration remain open. All processes
started for this update are terminal, and the goal remains active.

## Joy Technology historical Accton portfolio

On 2026-09-27, reviewed two previously unassigned Anatel manufacturer rows.
Joy Technology (Shenzhen) Co., Ltd. is an exact match to the subsidiary named in
Accton's 2024-09-26 announcement. A later Accton consolidated filing, note 32,
reports the 100% interest disposal completed and control transferred 2025-04-30.
Both facts were reviewed in web-search text. Original HTML/PDF retrievals
returned HTTP 402; neither was retried. The selected factual extract is labeled
as such. The original 2023 annual report also returned 402 and was not retried.

The new anatel-accton-portfolio importer validates that dated company evidence
and the complete existing Anatel snapshot before replacing its own source.
Exact Joy manufacturer cells create a Joy identity namespace and original
registry_manufacturer affiliation with no Accton target. Accton's separate
historical_subsidiary_portfolio affiliation appears in all-association inventory.
This does not assert parent manufacture, ownership at the 2017 approval date,
current ownership, hyperscaler deployment, retail-brand equivalence, or aliases.

One matched raw row adds OmniSwitch 6855-24, approval 005331103709, CSV record
21008. Applicant remains ALCATEL-LUCENT BRASIL TELECOMUNICACOES LTDA. The other
candidate, Accton Technology (China) CO. LTD / OS-LS-6248, remains unresolved:
corporate documents use differing legal names, so no mapping was added.

Catalog: 3,533 identities, 4,652 records, 217 sources; manifest: 99 jobs and
1,986 hashed files. Direct manufacturer inventory remains unchanged at 1,872
target/identity pairs, representing 1,871 unique identities. There are no new
hyperscaler assertions. All prior records, facts, raw rows and indexed text
are preserved across 39 tables/indexes (search-index row numbering excluded).

Three new tests cover role and date separation, duplicate rows, idempotence,
refresh withdrawal, composite-name exclusion and validation before mutation.
All 377 tests pass. The complete offline rebuild at
data/rebuilt-with-accton-joy.sqlite matches all 39 tables/indexes and the full
owned baseline. Import, preservation, rebuild and export audits use accton-joy
in their filenames under data/exports/. Research scope and queries are in
research/accton-joy-portfolio.md.

Refreshed the HTML catalog, target lists, all inventory scopes, hyperscaler
reports and 92 saved lookup reports. HTML embedded data contains the correct
Joy manufacturer and Accton historical role; browser rendering was not verified.
Exports leave database bytes unchanged. Database SHA256:
4816a396983292659e9ccb93501a6df4daef989c8382a33e597597a0f341befa.
Owned baseline remains 10/10 searchable and 9/10 corroborated. Full historical
and FCC coverage and 7760-32X-A-AC-F corroboration remain open. All processes
started for this update are terminal; the goal remains active.

## Minipack3 historical fabric-role evidence

On 2026-09-27, another complete-code search for 7760-32X-A-AC-F still returned
seller descriptions, not qualifying corroboration. No alias or validation change
was made. Reviewed the already archived 2025 Meta article and its live public
text instead: it describes Minipack3 and Cisco 8501 as foundations used in
building Meta's next-generation frontend and backend data-center fabrics.

The Meta importer now emits a separate documented_fabric_role assertion on the
existing Celestica/Minipack3 identity. The 2024 manufacturer/deployment-plan record
is preserved. The 2025 paragraph does not name Celestica, so its own affiliation
is Meta/documentation_vendor only. The validated 2024 manufacturer-caption source
ID, URL and original HTML hash are retained as identity-namespace evidence.
Both articles must validate before any Meta source replacement. This does not
lend the 2024 manufacturer affiliation to the 2025 record; strict Celestica
hyperscaler filtering excludes that fabric-role assertion. Minipack3N remains
a separate Accton identity, and Cisco 8501 is not imported into target inventory.

Reported fabric role remains distinct from explicit completed deployment or
unit provenance. The displayed 2025-10-13 date and 2025-10-14 UTC timestamp are
preserved. Existing documented-deployment assertions remain three. Default
hyperscaler inventory now has 18 assertions across 16 identities; including
unverified claims yields 21 assertions across 17 identities. All target inventory
identity memberships and counts remain unchanged.

The database still has 3,533 identities and 217 sources, now with 4,653 records.
One record, affiliation, relationship and seven facts were added. Only the
existing Minipack3 search document and meta-ocp-2025 records_seen count change
semantically; all prior assertions and raw source rows are preserved. Manifest
remains 99 jobs/1,986 hashed files, with updated expected record counts. No new
raw source download or archived document was needed.

Two new regressions verify model/tense/negation checks, validation of the separate
namespace source before mutation, manufacturer isolation, and unchanged deployment
categories. All 379 tests pass. data/rebuilt-with-minipack3-fabric-role.sqlite
matches all 39 live tables/indexes and the complete owned-baseline output.
Preservation, import, hyperscaler, export and embedded-HTML checks use the
minipack3-fabric-role prefix under data/exports/; compare/rebuild/test logs have
the same suffix. Research scope: research/minipack3-fabric-role.md.

Refreshed HTML catalog, target exports, inventory scopes, hyperscaler reports,
readable shortlist and 93 saved lookups. Embedded HTML data has separate Minipack3
announcement/fabric-role assertions and unchanged Minipack3N lineage; no browser
rendering verification is claimed. Exporting preserves database bytes. SHA256:
ea3ba177c825ce388acf4997e5c6e29e3c52b300911f3c6c3e2ddaeb229f1a4b.
Owned baseline stays 10/10 searchable and 9/10 corroborated. Complete historical
and FCC coverage and the 7760-32X-A-AC-F corroboration gap remain open. All
processes started for this update are terminal; the goal remains active.

## CubScout tested-component search and explicit capture limits

On 2026-09-27, followed the missing local FCC-detail/PDF evidence for
2AVPL-CUBSCOUT. Web-tool text exposed its FCC.report filing page and Sporton
report FR181018AR, issued 2022-01-19. Direct original downloads of the filing
HTML, report 5661528.pdf and label 5657033.pdf all returned HTTP 403. None was
retried or downloaded via another route. Those exact denied URLs and timestamps
are retained in data/raw/cubscout-fcc/fetch-report.json; do not retry them.

Saved only a labeled factual extract of already retrieved cover fields and
configuration tables on report pages 6–7. It contains three configurations and
30 component rows, including per-configuration quantities. Original PDF bytes
and visual page inspection remain unavailable; the combined Model / Description
cells are extracted text, not verified ordering codes or compatibility claims.

The existing cubscout-report source now references that factual extract, with
honest coverage and capture metadata. Its ADS affiliation is corrected from
fcc_grantee to regulatory_applicant because that is the report's cover field;
separate FCC application records retain grantee roles. Foxconn Interconnect
Technology Limited Taiwan Branch remains the named manufacturer, AWS the brand.
Four facts were added: versioned tested_configurations JSON, publication date,
capture method and original-document status. No component is a parent alias or
new target-manufactured device. No original document count or FCC collection
stage count increases.

New read-only tested-components command supports literal component/parent-model
search, parent-record vendor association, limits and table/JSON/CSV output.
It preserves quantities, configurations, source pages, manufacturer strings,
record IDs and source/capture metadata. It rejects malformed provenance,
duplicate configuration IDs and invalid quantities/pages rather than dropping
data. Components remain within their own tested configurations; for example,
Samsung PM883 SSD quantity is two in configuration 2 and one in configuration 3.
The existing facts table stores the JSON; docs/tested-components.md includes
a native SQLite query executed verbatim and checked against all 30 API rows.

All 382 tests pass. The three component tests were rerun after compact table
formatting was added; all three formats, absent-DB read-only behavior and caller
transaction preservation pass. The full rebuild at
data/rebuilt-with-cubscout-components.sqlite matches all 39 tables/indexes and
complete baseline results. The preservation audit isolates one source metadata
update, one raw record update, one applicant-role correction, four new facts and
one changed CubScout search document. Other catalog and raw data are unchanged.

Counts remain 3,533 identities, 4,653 records and 217 sources. All manufacturer,
factory and all-association identity memberships remain unchanged. The existing
CubScout AWS-brand relationship becomes eligible through the saved factual
extract, so default hyperscaler inventory now has 19 assertions/17 identities;
all 21 assertions remain, with two uncached claims. ADS filtering adds CubScout's
applicant-role brand assertion to five Monitron relationships. The three strict
ADS manufacturer relationships and three global documented-deployment assertions
remain unchanged. Snapshot eligibility is not an original-PDF claim.

Manifest remains 99 jobs, now 1,988 hashed files. The curated input hash changes;
the selected extract and failed-download log are new dependencies. Refreshed
HTML, target exports, inventory scopes, hyperscaler reports/shortlist, seven FCC
coverage report pairs and 94 saved lookups. New component JSON/CSV exports and
SQL, preservation, import, export and HTML-data audits use cubscout-components
or cubscout-tested-components prefixes under data/exports/. Embedded HTML data
checks passed; browser rendering was not verified. Database SHA256 is
a007484957c4b30d40a49a8bb10e48a56d36c6343906a26ccf3fe98d3e3ee7d7.
Owned baseline remains 10/10 searchable and 9/10 corroborated. Full historical
and FCC coverage and 7760-32X-A-AC-F corroboration remain open. All processes
started for this update are terminal; the goal remains active.


## Accton FCC.report company-table supplement

On 2026-09-27, saved selected factual web-tool extracts of all 235 displayed
rows across three Accton company pages (100/100/35), covering 169 literal FCC
IDs. Source: https://fcc.report/company/Accton-Technology-Corp. All three web
captures preceded a direct original-HTML download attempt, which returned HTTP
403 on page 1. The fetch stopped there; no original paginated HTML was attempted.
Do not retry the denied base URL or fetch the same page by another route.
The originals are not archived. Three factual JSON extracts and fetch-report.json
are under data/raw/fcc-company-accton/, with dependency hashes in bundle.json.

New fcc-company importer validates company identity, hashes, pagination, source
line order, dates and reported refresh before replacing only its own data.
Schema 26 adds fcc_company_rows, preserving duplicate rows, date/purpose/frequency
strings, unverified Model cells, page-level company FRN and exact page URLs.
The discovery index retains the first displayed row per literal FCC ID; the
complete 235 rows remain queryable. No detail URLs, models, aliases, grants,
manufacture or deployment are inferred. The original FCCID.co detail collector
remains isolated to its original 171 entries. Research and SQL examples:
research/fcc-accton-company.md.

There are 36 newly indexed strings, including nine already known through other
local evidence and 27 new to the coverage report. Hyphen variants remain distinct.
Combined index: 342 source rows / 209 literal IDs. Known FCC strings: 271;
index 209 collected/62 missing, detail 171/100, application 2/269, FCC-linked PDF
1/270. HEDML16035, HEDML2560 and HEDML60LW now have index evidence, while their
prior detail failures remain intact. These are discovery leads, not a complete
historical FCC register. Page extracts may have different web-cache ages.

Catalog remains 3,533 identities and 4,653 records, now 218 sources. Manufacturer,
factory and all-association inventories are unchanged; owned baseline stays
10/10 searchable and 9/10 corroborated. AZ4444-52X-DC-10 is the corrected full
owner label and passes exact Edge-Core/Anatel evidence. The shortened name is
correction history, not an alias. Only 7760-32X-A-AC-F remains uncorroborated.

All 385 tests pass after updating expectations for the new lookup dataset and
schema version. A complete offline rebuild at
 data/rebuilt-with-accton-fcc-company.sqlite
matches all 40 tables/indexes and complete baseline results. Preservation audit
confirms all rows in the prior 39 tables/indexes remain unchanged, with new source
and index rows isolated. Manifest: 100 import jobs / 1,993 hashed files.

Refreshed HTML, target exports, three inventory scopes, 94 saved lookups, seven
FCC coverage JSON/CSV pairs and the combined index CSV. SQL examples were executed
and checked. HTML embedded data and source rows pass checks; browser rendering
was not verified. Import, SQL, preservation, export and HTML audits use the
fcc-company-accton prefix in data/exports/. Rebuild comparison covers all 40
tables. Read-only exports preserve database SHA256:
4af9acdf3d9edd729ac815268703cb037a7076f4d35f3750cdc4172512f5b102.
All started processes are terminal. Full historical coverage and the remaining
baseline gap are open; the goal remains active.


## Supplied 7760 label resolves the owned identifier mapping

On 2026-09-27, an exact full-code research pass still found seller descriptions;
Oracle and Edgecore FCC.report company probes yielded a cache miss and an
unavailable response respectively, with no imported data from those probes.
The user then supplied the eBay listing URL https://www.ebay.com/itm/389108536363
and an original clipboard image. Visual review of the image directly reads
Edge-corE, Model No. 7760-32X-A-AC-F and Supplier Sku AS7760-32X together; the
adjacent compliance label repeats AS7760-32X. This changes the next action:
do not keep repeating broad exact-code searches to establish this pairing.

The exact supplied image is saved at data/raw/edgecore-7760-label.png and hashed
by data/label-evidence.json and the rebuild manifest. The source tier is explicitly
physical_label_photo. The user attributed the photo to the listing; exact
live-gallery matching was not independently verified. Serial and MAC are retained
only in the original image, not transcribed into searchable fields. Firmware
V2015.02.00.30, hardware R011 and revision R20 are scoped to the photographed
unit. Edge-Core brand only: no manufacturer, factory, AWS deployment or actual
ownership of this particular pictured unit is inferred.

The added record attaches to the existing full-code identity. AS7760-32X is its
source-scoped part_number identifier and supplier_sku fact; other AS7760 identities
remain separate. No family-wide or cross-brand merge occurs. Validator now
accepts reviewed, archived label photos as identity evidence while preserving
seller/owner/uncached rejection. New evidence_summary distinguishes 9 document
snapshot hits from 1 label-photo-only hit, with zero unresolved identity gaps.
Thus 10/10 are searchable and supported under the explicit photo-inclusive
policy; independent document-only scraped coverage remains 9/10. The baseline
fixture and all AWS owner claims are unchanged. See research/edgecore-7760-label.md.

## Amazon company FCCID index, separate from same-name ULS records

The FCC.report Amazon company record at
https://fcc.report/company/amazon-data-services-inc-2 exposes two FCCID rows under
FRN 0029198181: 2AVPL-CUBSCOUT and 2AVPL-TE1A001. Saved selected factual web-tool
text with line references, original dates, Model cells and frequency strings;
original HTML download was not attempted. Same-name ULS company records with
other FRNs and radio licenses were not imported as devices or merged into this
identity. New reviewed importer profile validates exact company/FRN/URL/grantee
and one-page shape, while the Accton profile and original detail collector retain
their existing behavior. Tests reject the ULS identities and wrong URLs.

Company raw rows now total 237; combined FCC index has 344 source rows / 210
literal IDs. CubScout newly has index coverage. Known FCC strings remain 271:
index 210/61 collected/missing, details 171/100, applications 2/269 and FCC-linked
PDF IDs 1/270. No new models, detailed applications or original PDFs are inferred.
See research/fcc-amazon-company.md and data/raw/fcc-company-amazon/.

## Verification of both additions

Catalog: 3,533 identities, 4,654 records, 220 sources, schema 26. Manufacturer
inventory stays 1,872 target/identity pairs / 1,871 identities, with the same
425/219/193/859/176 per-target counts. All 21 hyperscaler assertions remain;
default eligible inventory remains 19 assertions/17 identities. Manufacturer
and deployment evidence have not changed. Manifest: 102 jobs / 1,997 hashed
files. All 388 tests pass, including label evidence provenance, saved-snapshot
requirements, seller rejection and manufacturer/operator isolation.

data/rebuilt-with-7760-label.sqlite matches all 40 tables/indexes and complete
baseline results; separate document/photo counts also match. Preservation audit
checks all prior rows, excluding only new source rows and the one updated
full-code search document. Refreshed HTML, target exports, three inventory scopes,
94 saved lookups, hyperscaler reports/shortlist and seven FCC coverage report
pairs. Embedded HTML and documented SQL pass; browser rendering is unverified.
Audit and test files use 7760-label prefixes under data/exports/.

Database SHA256 after read-only exports:
8405cf108febcbeb86619166f9d2ca8293784e1d367dd8f4779b97f02de80a44.
All started processes are terminal. No owned identifier question remains open.
Full historical/source completeness remains unproven; the overall goal is active.


## Bulk-data acquisition review and cost preference

On 2026-09-27, rechecked bulk equipment-authorization leads without retrying
previously denied FCC endpoints. Search results again included ULS/license
loaders, which do not supply device authorizations. A new commercial lead,
Datadory, advertises FCCID.io-derived equipment-authorization file/API delivery.
Downloaded its dataset, search-interface and pricing descriptions as original
HTML. All three returned HTTP 200; hashes, URLs and timestamps are saved in
 data/raw/fcc-bulk-review/review-bundle.json
with the qualification review in review.json. This is acquisition research,
not a device import; none of its marketing claims populate catalog facts.

The provider publishes no total FCC-ID count or numeric prices. Its published
field dictionary lacks stable application/exhibit identifiers and its wording
alternates filing-level records with flattened per-ID observations. A sample
would need to prove that multiple applications, dates and exhibits remain
separate, along with declared target-grantee coverage, source URLs, local
retention rights and total backfill/update cost. Pricing is on request for all
tiers and measured in field/record/period observations, not simply device rows.
No sample form, email, subscription or purchase was submitted.

The user asked how much and said that if it is too expensive, they would rather
rebuild it or use an open-source solution. No numeric budget was specified.
Continue the existing local implementation/free-source path; paid delivery is
only an optional lead until there is a verified price and sample. Do not repeat
an acquisition-preference question or assume permission to buy/contact anyone.
See research/fcc-bulk-provider-review.md for the reviewed evidence and limits.

Updated the obsolete lead-in counts in research/fcc-source-coverage.md to
210 indexed / 61 missing out of 271 known strings. The catalog, manifest and
verified rebuild are unchanged: 3,533 identities, 4,654 records, 220 sources;
388 tests last passed on the implementation, with no code change in this review.
The 10/10 identity-supported baseline remains nine document snapshots plus one
label photo. Database SHA256 remains
8405cf108febcbeb86619166f9d2ca8293784e1d367dd8f4779b97f02de80a44.
All source-review checks completed; no live job or quote request is pending.
The previous goal turn made progress by closing the label mapping and adding
Amazon index rows; this turn adds a verified acquisition lead and its missing
qualification conditions. Full historical/device coverage remains unproven;
the goal remains active.


## Commercial provider excluded by user

The user explicitly said to ignore Datadory and continue figuring out the public
sources ourselves. This supersedes the preceding optional paid-source lead.
Do not pursue quotes, samples, contact or subscriptions for that provider.
Retain downloaded descriptions only as review history. Continue our existing
local implementation and public/open-source source discovery; no price question
is pending. Next check: public mirror sitemaps as enumerable discovery inputs.
