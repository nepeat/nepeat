# devicehunt

Hunt liquidated **enterprise, networking and server hardware** on eBay and other
marketplaces using a local SQL-queryable catalog and offline search tools. Exact
model/order codes, source-qualified company roles and practical specifications
support identification and buying research. Hyperscaler connections are optional
provenance context; they are not required for a device to be in scope.

Research resumed 2026-10-04. The active targets are Accton, Edge-Core, Celestica,
Oracle, Amazon Data Services, Inventec, Hyve, Foxconn, Wiwynn, TD SYNNEX / SYNNEX,
Quanta, ByteDance and Inspur, with other relevant enterprise hardware vendors and
ODMs retained as evidence-backed leads.
See [the revised goal](GOAL.md) and [current next steps](PROGRESS.md).

All thirteen filters work in the CLI and offline HTML catalog. Current totals:
4,064 identities / 5,273 source records. Inspur has 133 source-qualified identities;
ByteDance B4020/B4010 remain explicit evidence gaps. A B4010 compliance statement
names Accton as applicant, with brand N/A; it does not establish ByteDance or
manufacturer attribution. See [the new source review](research/bytedance-inspur-2026-10-04.md).

The latest enterprise hardware batch adds **88 identities and 93 source records**.
It covers Inspur servers, storage, Fibre Channel switches, physical appliances and
rack nodes, plus Inventec Artemis II Rack and X680m. Power-shelf dependencies,
chassis/node distinctions and conflicting specifications remain explicit. Use
all associations to see brand/platform records; manufacturer counts are unchanged.
See [source review and liquidation-search examples](research/inspur-2018-inventec-2026-10-04.md).

Eight Ingrasys server/storage names now have separately sourced Foxconn
parent-portfolio associations. Use all associations to include them; manufacturer
counts are unchanged. See [the model and document review](research/ingrasys-servers-2026-10-04.md).

The earlier Inventec/QCT batch added 41 identities and 53 source records. Inventec now has
81 associated identities, 44 with manufacturer claims; five named products have
an explicit dated Inventec EBG manufacture statement. Quanta has 73 associated
identities, including nine QCT models with historical parent-portfolio evidence;
its manufacturer inventory remains 41. Original model prefixes/suffixes and
separate company roles are preserved. See [the evidence and queries](research/inventec-qct-2026-10-04.md).

Use `vendor-inventory Inspur --scope all --limit -1`, `documents B4010`, or
`validate-inventory --inventory validation/vendor-examples.json` after the
`python3 -m devicehunt` prefix. The examples are separate from the ten owned
devices and are never imported as evidence. [Exact mappings and exclusions](research/odm-target-registry.md)
preserve brand, applicant, manufacturer, parent and operator roles.

Schema 28 also stores **1,064 raw Inspur support-menu rows** in
`inspur_support_entries`. Use `lookup 'i24&NS5162M5'` or SQL to search these
locally. They include third-party parts/software and are not 1,064 verified
Inspur devices. Metagross now has a reviewed Inventec platform record; E850G4
and NF5280M5 have additional original specifications. See
[the support-feed review and queries](research/storage-inspur-2026-10-04.md).

The earlier Inspur compatibility batch added 34 identities from reviewed hardware tables and
primary product documents. Appliance/server-node pairs and the i24/NS5162M5
chassis/node relationship remain explicit, separate identities. Inspur's
manufacturer-backed count stays three. Inventec download pages 2 and 3 are
archived with unresolved model/title mismatches preserved. See
[the hardware review and SQL](research/inspur-rail-2026-10-04.md).

Start with [PROGRESS.md](PROGRESS.md) for the current handoff, quick-start commands,
verified totals and prioritized remaining work. Older sections below retain the
collection history and can contain earlier snapshot counts.

Public FCCID.co sitemaps now provide **190,198 distinct FCC ID discovery URLs**
and 41,637 company URLs for local SQL/lookup searches. This is a partial mirror
snapshot, not a complete filing or PDF archive. See [coverage and SQL](research/fcc-sitemaps.md).

Owned baseline: **10/10 searchable and identity-supported**. Nine matches use
collected documents; the tenth uses a reviewed user-supplied label photo pairing
`7760-32X-A-AC-F` with supplier SKU `AS7760-32X`. Source provenance stays explicit;
this does not prove AWS deployment. See [label evidence](research/edgecore-7760-label.md).

Accton’s FCC.report company index now adds 235 searchable table rows across three
reviewed page extracts. The combined discovery index has 344 source rows covering
210 literal FCC IDs. Query `fcc_company_rows` or use `lookup HEDML2560`. These
are discovery leads, not additional device models or complete filings. See
[company index scope and SQL](research/fcc-accton-company.md).

Use `python3 -m devicehunt tested-components PM883` to find parts within explicit
tested configurations. CubScout now has 30 component rows across three report
configurations, retaining quantities, page references, company roles and capture
limitations. See [component search and SQL examples](docs/tested-components.md).

The hyperscaler shortlist now includes Meta's October 2025 account of Minipack3's
role in building its data-center fabrics. This dated platform-role statement
appears alongside the separate 2024 deployment plan; it adds no manufacturer
claim or assertion that a particular auction unit came from Meta. See
[the source distinctions](research/minipack3-fabric-role.md).

OmniSwitch 6855-24 now appears in Accton's historical subsidiary portfolio,
with Joy Technology preserved as its registered manufacturer. The dated
corporate link includes the 2025 transfer of control and appears only in the
all-association inventory. See [evidence and local queries](research/accton-joy-portfolio.md).

AWS Monitron product relationships now appear in the hyperscaler report with
their actual Amazon Data Services roles: three sensor manufacturer claims and
two gateway compliance-declarant claims. These product identities do not assert
data-center deployment. One older TE1A001 claim is also backed by its saved
original AWS page. See [evidence update](research/monitron-product-links.md).

Use `python3 -m devicehunt hyperscaler-inventory` for a source-linked shortlist
that distinguishes documented deployment, design lineage, announcements and
product-family connections. Company roles and historical dates stay visible.
See [filters and evidence scope](docs/hyperscaler-inventory.md).

The original 32-page Meta Minipack2 specification is now searchable offline,
including power, cooling, module and label requirements. Its supplier appendix
is blank, so it adds reference material without asserting Celestica manufacturing.
See [document searches and source scope](research/minipack2-reference.md).

Specification searches now bind vendor filters and exact model-alias matches to
the statement's own source record. Results include record IDs and revisions;
exact modes work independently of the discovery index. See
[specification search scopes and examples](docs/specification-search.md).

Wedge 100 and Edgecore Wedge 100-32X now have explicit Accton manufacturer
claims from Facebook's original 2016 article. Historical platform deployment
and commercial design lineage remain separate. Auction matching also recognizes
suffix leads for spaced model names, without attaching specifications to those
variants. See [Wedge 100 evidence and offline queries](research/meta-wedge100-evidence.md).

Auction matching can now include source-linked power, cooling, physical and
installation statements with `match-listing --with-specs`. Exact ordering codes
keep their own record's configuration; unmatched suffix variants receive no
specifications. See [usage and output fields](docs/listing-matching.md#specifications-with-listing-matches).

SC6100, SD4100 and SD6200 now have explicit Celestica manufacturer claims from
their original product pages, alongside searchable chassis descriptions. These
upgrade existing brand-only evidence; Celestica's manufacturer inventory now has
193 identities. See [reviewed source scope](research/celestica-storage-manufacturer.md).

Local search now includes 346 Japanese radio-approval rows from twelve company-name
queries on the Lang-ship mirror. Try `python3 -m devicehunt lookup 'Bump Bar K3-10W'`
or query `japan_equipment` in SQL. Registered company names are kept separate
from manufacturer claims. See [Japanese source coverage](research/japan-mic-mirror.md).

The FCC gap report now retains three verified mirror not-found responses, so
missing filings can show their prior failed lookup. They remain coverage gaps;
no device or filing absence is inferred. See [captured probe evidence](research/fcc-gap-probes.md).

Use `python3 -m devicehunt fcc-coverage --missing index --limit -1` to find
known FCC identifier strings missing from the local discovery index. The report
separates index pages, detail captures, application records and PDFs, preserving
source roles and failed captures. See [coverage report and limits](research/fcc-collection-coverage.md).

Three IgniteNet MetroLinq models now have explicit Accton manufacturer claims:
ML1-60-35, ML1-60-19 and ML-60-10G-360. Evidence is preserved as reviewed
mirror-document text extracts; original PDF downloads were unavailable. Try
`python3 -m devicehunt lookup ML1-60-19`. See
[manufacturer fields and capture limitations](research/metrolinq-manufacturer-evidence.md).

Eight exact Celestica DS4000/DS4001 chassis ordering codes now resolve to their
documented model families. Try `python3 -m devicehunt lookup R4023-F9102-A0`.
Each code keeps its own configuration and original PDF page reference; cables
and brackets are excluded from chassis identifiers. See
[ordering-code evidence](research/celestica-chassis-ordering-codes.md).

Additional local reference tables now include 2,896 Saint Vincent NTRC and
2,089 Mauritius ICTA approval rows. Search them with `lookup` or query
`public_equipment_rows` in SQL. Neither snapshot adds a verified target-company
device; brand, make and foreign-certification text remain raw source claims.
See [coverage and download checks](research/additional-public-registers.md).

A local, evidence-preserving hardware catalog for Accton, Edge-Core, Celestica,
Oracle, and Amazon Data Services. Python 3.9+ and SQLite with FTS5; no pip
dependencies, account, API key, or database server. See [GOAL.md](GOAL.md).
The optional NTRA PDF importer additionally needs `pypdf` and `pdfplumber`;
using the populated database and all other importers still needs only the standard library.

The [ENERGY STAR server export](research/energystar-servers.md) adds 495 all-brand
records with all original fields available to SQL. Oracle X9-2L, X10M and X11M
have separate test-configuration idle measurements and PSU ratings. Try
`python3 -m devicehunt lookup 'EPYC 9J25'` or
`python3 -m devicehunt specs X10M --exact-model`.
Two [historical ENERGY STAR workbooks](research/energystar-server-archives.md)
add 873 qualification rows, including Oracle X5–X8 and Sun Fire configurations.
Try `python3 -m devicehunt lookup 'Sun Fire X4170 M2'`; historical records keep
their configuration/voltage context and separate program-partner roles.
The [network and storage lists](research/energystar-network-storage.md) add
452 all-brand qualification rows for local cross-checks, including historical
storage files. These contain no target-company matches; that coverage finding
is retained without inferring hardware manufacturers from component names.

The OcNOS compatibility importer adds sourced software SKUs, firmware/revision
context, port layouts, and buying specifications for 20 Edgecore platforms and
Celestica DS1000. Try `python3 -m devicehunt specs DS1000 --exact-model` or
`python3 -m devicehunt search OcNOS --vendor Edge-Core --limit -1`.
See [source scope and SQL examples](research/ocnos-hardware-compatibility.md).

The [PICOS hardware table](research/picos-hardware.md) adds original-HTML evidence
for 26 Edgecore/Accton platforms, including port layouts, ASICs, CPU families and
nine explicit DCS/EPS model codes. Try `python3 -m devicehunt lookup AS9737-32DB`.
These are software-vendor specifications, with guide/version limitations retained.

Cross-brand factory evidence is also searchable: Cisco's published certificates
name Celestica Thailand among the factories for NCS1004 and NCS1014. Use
`python3 -m devicehunt search --vendor Celestica --role listed_factory`.
See [certificate scope and separate power configurations](research/cisco-celestica-factory-evidence.md).

Accton's cached public UL listing now supplies 437 searchable identifiers,
including 34 QFX/EX/ACX/JNP/SRX/JCP codes and 212 additional complete commercial
codes and named models. Adjacent regulatory-model notes are searchable facts,
not inferred aliases. Try
`python3 -m devicehunt search EX4100 --vendor Accton --role listing_holder`.
These records establish a listing-holder association; Accton is their catalog
namespace, not an asserted retail brand or physical manufacturer. See
[exact-model scope and SQL examples](research/ul-accton-platforms.md).

Amazon Data Services' public UL listing adds 57 exact model names, including
`CS42_1RP_G2`, `BARRELEYE COMPUTE` and `AZ9075-64X-DC-42`. Ten placeholder
templates remain unexpanded. These are listing-holder associations, with
search-extract provenance; see [local queries and scope](research/ul-amazon-platforms.md).

AWS Panorama AMHT-1 is searchable with an explicit Amazon Data Services legal
manufacturer statement. Its service notice is also preserved: AWS states that
Panorama devices depend on the cloud service that ended May 31, 2026. Alternative
firmware and repurposing remain unverified. See
[appliance evidence and service queries](research/aws-panorama.md).

Snowcone declarations also identify AF1 (data storage device) and the unsplit
printed scope AF1 BD1 (dongle), both with Amazon Data Services as legal manufacturer.
See [declarations and dated return context](research/aws-snowcone.md). For AF1
auction-title matching, use `--include-short-models`; this enables three-character
codes containing letters and digits without relaxing model boundaries.

## Use the populated catalog

The [Ghana NCA public table](research/ghana-approved-equipment.md) adds 1,153
searchable rows and a manufacturer assertion for Oracle's Netra Sparc T3-1.
Try `python3 -m devicehunt lookup 5N4-3H-7DF-22F`. Blank approval numbers remain
blank, and detail links are distinguished from fetched certificate evidence.

The [Nigeria NCC register](research/nigeria-approved-equipment.md) contributes
8,517 published equipment rows, with separate manufacturer, certificate-holder
and applicant columns. Try `python3 -m devicehunt lookup ATLANTA23SFX` or query
`nigeria_equipment` directly. Seven rows add target manufacturer assertions;
damaged text and unreviewed company spellings remain flagged or raw evidence.

The [Canadian REL query exports](research/ised-radio-equipment.md) now expose all
628 raw CSV rows independently of catalog identities. Try
`python3 -m devicehunt lookup 'MN-500'`. Six rows without an exact web-table
match remain explicitly marked; company associations are not manufacturer claims.

Official Korean RRA manufacturer queries now retain 1,081 approval rows across
all five targets and legacy Sun, covering 1,080 distinct certificates. Try
`python3 -m devicehunt lookup BARRELEYE`, `lookup SAT1A_C2`, or query
`rra_equipment` directly. See
[query coverage, manufacturer roles and local exports](research/korean-rra-targets.md).
The importer now reads [explicit main-model lists](research/rra-main-model-lists.md),
recovering 84 manufacturer assertions and 75 catalog identities from existing
approval pages. Names remain literal, without inferred aliases or shared prefixes.
A separate [reviewed manufacturer-list extraction](research/rra-manufacturer-lists.md)
adds 45 assertions from 22 approvals naming multiple companies. It preserves the
full field and each selected legal name; this reuses existing source pages and
does not imply sole manufacture or a particular physical factory.
The [Sun legacy query](research/sun-legacy-portfolio.md) adds 40 approvals and
26 Sun identities linked to Oracle through a dated acquired-company portfolio
relationship. It does not relabel Sun as Oracle manufacturer evidence.
A [reviewed Accton Singapore legal name](research/rra-accton-company-name.md)
adds 13 manufacturer assertions across twelve model identities from existing
approvals; unverified company spellings remain raw searchable evidence.
Separate Edge-Core spellings found seven additional approvals. Empty queries
require explicit original no-results proof before import; see
[spelling coverage and zero-result safeguards](research/rra-edge-core-spellings.md).
Reviewed Celestica legal-name spellings add 63 manufacturer model assertions,
including DS4100/DS5000 and cross-brand equipment. Their company mappings retain
the filing extract and interpretation separately from original approval HTML.
See [legal-name evidence and examples](research/rra-celestica-company-names.md).

A reviewed Korean approval mirror adds manufacturer/applicant evidence for
Celestica CS8000-32X-DC-11 and its separately listed derived model
CS8001-32X-DC-11. Try `python3 -m devicehunt lookup RR-ZZR-CS8000-32X-DC`.
See [capture provenance and model boundaries](research/korean-rra-reviewed.md).

Celestica’s networking index is now archived as original HTML: 15 product cards
covering 17 models, including locally corroborated DS6000, DS6001 and DS6010.
Try `python3 -m devicehunt specs DS6001 --exact-model`. See
[model scope and source upgrade](research/celestica-networking-index.md).

Celestica FCC discovery now includes MRF13316C and MRF13316C2 from a separately
labelled public mirror-table extract. Try `python3 -m devicehunt lookup MRF13316C`.
See [source disagreement and capture scope](research/fcc-celestica-supplement.md).

Start with `python3 -m devicehunt vendor-inventory --limit -1` for the collected
manufacturer/OEM claims across all five targets. Add a vendor name, or use
`--scope factory` / `--scope all` to inspect factory listings or broader
associations. [Inventory guide, current counts and SQL](docs/vendor-inventory.md).

Oracle EcoDesign documents connect WH1CL to X9-2, WH2CL to X9-2L, and ZU2XL to
both X10M and X11M. Named legal-manufacturer claims and qualified product scopes
remain separate source records. Try `python3 -m devicehunt lookup ZU2XL`.
See [Oracle compliance-code scope and power-rating caveats](research/oracle-ecodesign.md).

Reviewed Taiwan company-name evidence now connects 295 BSMI/NCC assertions to
Oracle, Accton and Edgecore vendor searches. Try
`python3 -m devicehunt search AP-52-W --vendor Accton --role registry_manufacturer`.
Printed brands and applicant/manufacturer roles stay separate. See
[Taiwan target evidence and queries](research/taiwan-target-evidence.md).

Taiwan BSMI adds **105,732 certificate rows** in `bsmi_certificates`, with raw
applicant names, primary models, expiry dates and statuses. Exact Chinese applicant
queries expose 41 Oracle rows and an Accton SAF6101I lead. See
[BSMI source scope and SQL examples](research/bsmi-certificates.md).

Three additional BSMI registers add **20,179 raw rows**: product certificates,
voluntary certificates and declarations of conformity. They are separately
queryable and included in `lookup`. The reviewed target-name and owned-label
scans found no matches in these three snapshots. See
[register scope, coverage audit and SQL](research/bsmi-additional-registers.md).

Taiwan's published NCC export is local too: **110,957 CSV rows**, with brand,
model, applicant and manufacturer kept separate. Try
`python3 -m devicehunt lookup Celestica --limit -1` or query `ncc_equipment`.
Blank fields, confidential labels and raw validity flags remain visible.
See [NCC scope, attribution and SQL examples](research/ncc-equipment-register.md).

The complete published PCI ID snapshot is also local: 42,438 community vendor,
device, subsystem and class-name entries. Try `python3 -m devicehunt pci 1d0f:ec20`
or query `pci_entries` directly. PCI names remain separate from chassis and
manufacturer evidence. See [scope, SQL examples and refresh notes](research/pci-id-repository.md).
For a saved Linux component inventory, use `python3 -m devicehunt pci-report FILE`
with numeric `lspci -D -vmm -n` output. [Inventory-decoding guide](docs/pci-inventory.md).

Run commands from this directory. Data lives in `data/devices.sqlite`; the CLI
defaults to that absolute project location even when called elsewhere. Search, specifications,
detail views, statistics and exports open it read-only. A missing database path
is an error rather than an empty catalog. For an older database, explicitly run
`python3 -m devicehunt init` when a schema upgrade is needed.

```sh
python3 -m devicehunt stats
python3 -m devicehunt search '100G' --vendor Celestica
python3 -m devicehunt search 'AS5812-54X'
python3 -m devicehunt search 7760 --vendor Edge-Core
python3 -m devicehunt search '8200' --vendor Celestica
python3 -m devicehunt search --vendor Accton --role manufacturer
python3 -m devicehunt search --hyperscaler
python3 -m devicehunt search --vendor 'Amazon Data Services' --format json
python3 -m devicehunt show DEVICE_ID
python3 -m devicehunt shopping DEVICE_ID
python3 -m devicehunt match-listing --title 'Oracle E5-2C server for parts'
python3 -m devicehunt match-listing --title 'Oracle 7103791 for parts' --vendor Oracle --include-numeric-identifiers
python3 -m devicehunt match-listing --csv my-listings.csv --format json
python3 -m devicehunt match-listing --title 'Celestica R4023-F9102-A0' --with-specs --format json
```

`search` combines query terms and also finds interior fragments of at least
three characters in model/identity fields: `7760` now finds `AS7760-32X`.
Vendor and role filters still apply; fragments are discovery matches, not
equivalence or validation evidence. See [rules and examples](docs/model-fragment-search.md).

Listing matching runs locally and distinguishes complete model/alias mentions
from suffix-variant leads. It preserves source evidence and never imports your
listing text into the catalog. See [examples and matching rules](docs/listing-matching.md).

For a label or company that may appear outside the target catalog, use one
read-only lookup across the catalog, NTRA, TEC, Anatel, ICASA, Bahrain, IEEE address
assignments, PCI IDs, selected FCC-ID company indexes, cached PDF pages and historical FCC company registrations:

```sh
python3 -m devicehunt lookup 'AZ4444-52X'
python3 -m devicehunt lookup 'Amazon Data Services' --limit 5
python3 -m devicehunt lookup '7760-32X-A-AC-F' --format json
python3 -m devicehunt lookup 'Celestica' --limit 0
```

`lookup` uses a Unicode case-insensitive literal substring, retaining punctuation
and suffixes. It reports counts and truncation separately for each dataset;
`--limit` applies per dataset, `0` shows counts only, and `-1` returns all matches.
`not_loaded` is distinguished from a loaded source with no matches. Counts use
different units and must not be summed as unique devices. Search results and
exact text matches do not satisfy inventory validation by themselves. NTRA and ICASA
rows requiring review keep their raw cells; FCC grantees remain company records.
ISED and UL target assertions are searched through the catalog. JSON includes
source scope, URLs, snapshot metadata and catalog evidence roles/tiers. See
[lookup behavior and checks](research/local-lookup.md).

The FCC-ID dataset has 171 entries from six target grantee queries against a
public mirror. It is separate from device models and detailed grant records;
the mirror omits known filings, and its displayed dates remain verbatim.
See [FCC index coverage and SQL examples](research/fcc-company-index.md). All 171 indexed detail pages
are now cached with 1,493 public exhibit links and 1,092 text excerpts; 311 excerpts
are explicitly truncated. Use `lookup` or SQL to search them locally. See
[filing-detail scope and queries](research/fcc-detail-pages.md). A reviewed subset of report covers
also supplies 38 exact model assertions with Accton as regulatory applicant;
[scope and exclusions](research/fcc-report-cover-models.md) remain explicit.

Search cached vendor, regulator and FCC PDF contents and get page numbers plus local file paths:

```sh
python3 -m devicehunt documents antenna
python3 -m devicehunt documents --fcc-id 2AVPL-TE1A001 --limit -1
python3 -m devicehunt documents AMHT-1 --source reference-pdf-aws-panorama
python3 -m devicehunt documents Celestica --source reference-pdf-cisco-ncs-certificates
```

The combined archive has 29 PDFs and 3,077 pages: eleven FCC exhibits plus all
eighteen other original PDFs currently cached in `data/raw`. These include full
ICASA/NTRA registers (including NTRA December 2022) and AWS, Celestica and Cisco documents. Eight pages have no
embedded text; some other pages contain only headers while their main content
is an image. No OCR is performed. Source filters retain provenance and document
matches never create automatic model/manufacturer assertions. See
[reference archive and SQL examples](research/reference-document-archive.md) and
[FCC archive scope](research/fcc-document-archive.md).
The [WorkSpaces Thin Client review](research/aws-thin-client.md) adds searchable
GA5Z9L documents while preserving their separate Amazon company names.

The AWS Monitron evidence covers three sensors and two gateways. Manufacturer
and compliance-declarant roles remain separate, with a dated notice about
new-customer service access. See [models, evidence and queries](research/aws-monitron.md).

Celestica's original 2025 portfolio PDF contributes 19 model descriptions with
page references and qualified grouped-model statements. See
[coverage and SQL examples](research/celestica-2025-catalog.md).

For a MAC address visible on an auction label, use the local IEEE lookup:

```sh
python3 -m devicehunt mac 8C:1F:64:72:30:01
python3 -m devicehunt mac 8c1f.6472.3001 --format json
```

These examples use an invented device suffix within a real Celestica assignment.
The snapshot has 58,639 rows across the complete published MA-L, MA-M, MA-S and
IAB CSV files. Lookup uses the longest matching prefix and preserves duplicate
names. Local/group addresses are classified without hardware attribution. The
registered organization is a lead, not proof of a chassis model, factory or
operator. See [coverage, SQL examples and refresh instructions](research/ieee-address-registry.md).

`show` includes evidence, raw source records, URLs, and observed timestamps.
`shopping` generates eBay and web search links, without fetching listings or
claiming availability. Search defaults to 100 rows; use `--limit -1` for all.
Ordinary CLI text search ANDs prefix tokens; hyphenated model names are safe.

Open `data/exports/catalog.html` in a browser for a self-contained offline search
page. It needs no server or internet connection, except when following external
source/marketplace links. It searches substrings and starts with manufacturer
claims. The evidence-scope selector also offers factory listings and all
associations, including seller, brand and compatibility leads. Manufacturer and
factory scopes use the same source eligibility and role classification as
`vendor-inventory`. Company, scope and role must match one recorded association;
an Oracle applicant cannot borrow Celestica's manufacturer claim. Text and
hyperscaler filters further narrow the results. Each card shows the evidence
matching your filters, with its source type and link; other associations remain
available in the full evidence details. Source roles do not establish the history
of a particular surplus unit. This is a snapshot: regenerate after importing.

## SQL

The [reviewed FCC Sporton covers](research/fcc-sporton-cover-roles.md) now separate
applicant, named manufacturer and brand. For example, `EW-7822GMn` has Accton as
applicant but Edimax as the report's manufacturer; the Accton namespace alone
does not make that an Accton manufacturing claim. These are cached mirror excerpts,
with explicit source limitations and no inferred factory or deployment history.

The [Kenya CA August 2026 register](research/kenya-approved-equipment.md) adds
six reviewed manufacturer assertions and three identities, including the literal
labels `TIMBERWLFL10` and `ACCTON-AS8060`. All 203 PDF pages are archived for local
document search and SQL. Source contradictions and the unresolved two-line
`CAMILLO / GOLGI` model cell remain visible. These additions do not establish
factory identity, current certification or deployment history.

Celestica's public UL listing now contributes 163 additional non-CS identifiers,
including storage models and complete Wedge/Minipack variants, with listing-holder
evidence only. See [capture scope and local queries](research/ul-celestica-platforms.md).

The Canadian REL importer now includes a checked Amazon Data Services query
with CubScout, TE1A001, TE1A105 and TE1A195, plus paginated Accton, Edgecore and
Oracle queries. Shared certification numbers retain separate hardware versions.
Capture/import instructions, query limits and HTML/CSV discrepancies are in
[the ISED source review](research/ised-radio-equipment.md).

Open the SQLite file in your favorite SQL client, or use the read-only command:

```sh
python3 -m devicehunt sql 'SELECT target, COUNT(DISTINCT id) AS devices FROM target_devices GROUP BY target'
python3 -m devicehunt sql "SELECT brand, model, role FROM target_devices WHERE target = 'Accton' AND role = 'manufacturer'"
python3 -m devicehunt sql "SELECT * FROM device_facts WHERE name = 'asic' AND value LIKE '%Trident%'"
python3 -m devicehunt sql "SELECT * FROM device_identifiers WHERE kind = 'fcc_id'"
```

Tables: `sources`, `devices`, `records`, `affiliations`, `identifiers`, `facts`,
`hyperscaler_evidence`, `fcc_applications`, `fcc_exhibits`, `fcc_grantees`, `fcc_id_index`, `fcc_company_rows`, `equipment_register`, `bahrain_equipment`, `anatel_equipment`, `ntra_equipment`, `ieee_assignments`, and the FTS5
`search_index`. PDF tables: `documents`, `document_pages`, and FTS5 `document_search`.
Mirror detail tables: `fcc_detail_pages`, `fcc_detail_links`, `fcc_text_excerpts`;
joined view: `fcc_excerpt_text`.
Views: `document_text`, `target_devices`,
`device_facts`, `device_identifiers`. Facts are per-source assertions, so
contradictory chipset claims remain queryable. `sources` tracks content hashes,
cache paths, scan counts, scope, and revisions. `records.raw_json` preserves the
imported record. `fetched_at` is snapshot filesystem time; `observed_at` is local
import time, not grant/manufacture time. Reviewed web-only seeds have no claimed
local raw snapshot or fetch timestamp.

`devices.brand` is an identity namespace from the source and may be a catalog
vendor/applicant label. **Use affiliations.role for the actual relationship**;
do not interpret the brand column as physical manufacturing proof. Company
filtering includes all evidenced roles by default. Counts overlap between
targets, and different brands, codenames, families, or grouped product entries
can describe overlapping hardware. This is not a count of unique physical designs.

## Refresh, rebuild, export

```sh
# Small indexes plus manually reviewed seed records; reuse cached downloads.
python3 -m devicehunt sync
# Also scan all 9,260 records in the pinned DeviceCode snapshot (~171 MiB download).
python3 -m devicehunt sync --devicecode
# Include all 111 Accton/Celestica build definitions in the pinned ONIE snapshot.
python3 -m devicehunt sync --onie
# Cache English product pages from all Edgecore product sitemaps (~100 MiB).
python3 -m devicehunt sync --edgecore
# Cache and import every notice linked by the Edgecore retirement archive.
python3 -m devicehunt sync --edgecore-eol
# Re-fetch indexes. DeviceCode and ONIE revisions remain pinned in code.
python3 -m devicehunt sync --refresh

python3 -m devicehunt import devicecode data/raw/devicecode-138e8afd.tar.gz
python3 -m devicehunt import curated data/curated.json
python3 -m devicehunt import onie data/raw/onie-platforms.json
python3 -m devicehunt import fcc data/fcc-manifest.json
python3 -m devicehunt import edgecore data/raw/edgecore/products.json
python3 -m devicehunt import edgecore-eol data/raw/edgecore/2024-oct-eol-notice-tofino-based-products
python3 -m devicehunt import edgecore-notices data/raw/edgecore/notices.json
python3 -m devicehunt import oracle-eol data/raw/oracle-eol.html
# With optional PDF dependencies available:
python3 -m devicehunt import ntra data/raw/ntra-2026-01.pdf
python3 -m devicehunt import ul-capture data/raw/ul-celestica-models.json
python3 -m devicehunt import tec data/raw/tec-equipment.html
python3 -m devicehunt import ecrunch data/raw/ecrunch-7760.html
python3 -m devicehunt export data/exports/targets.csv
python3 -m devicehunt export data/exports/targets.json --format json
python3 -m devicehunt report data/exports/catalog.html
```

`--db /path/to/another.sqlite` goes **before** the subcommand. For a complete
offline rebuild of the collected catalog, use a new output filename:

```sh
python3 -m devicehunt --db data/catalog-copy.sqlite rebuild
python3 -m devicehunt --db data/catalog-copy.sqlite validate-inventory
```

This uses `data/import-manifest.json`: 112 import jobs and 2,120 SHA-256-checked
input files, including the regulatory registers, Oracle, UL and Meta sources.
It requires the optional `pypdf` and `pdfplumber` packages for the NTRA PDF.
No network requests are made. The output must not already exist. Imports run in
a temporary database; only a successful full build with matching source/count
expectations and SQLite integrity checks is published. Missing files, altered
snapshots and parser failures leave the destination absent and existing
catalogs untouched. A rebuild does not turn coverage gaps into passing results.
Search-index rows and their constituent text use deterministic ordering, so
repeated imports and offline rebuilds reproduce the same search data.

Keep this directory's code and `data/` together when copying the project to
another machine. Manifest paths are relative to `data/`; source URLs and
licenses remain in the rebuilt database. After deliberately refreshing source
snapshots, review and update their manifest hashes and expected counts before
rebuilding. This manifest verifies the collected snapshot set, not completeness
of a manufacturer's historical inventory.

Individual imports remain available; supported names are listed by
`python3 -m devicehunt import --help`. Sync defaults to reusing existing
cache files. Each successful source replaces its own records atomically; a parser
failure rolls back that source. Earlier successful sources in a sync stay imported.
There is no cross-source automatic fuzzy merging. Old assertions from a replaced
snapshot are not retained in the DB; retain raw snapshots if you need history.

The curated seed file is manually maintained; `sync` imports it, but does not
claim to re-crawl its source URLs. Add reviewed records with citations there.
For a new source type, implement an importer rather than adding an unrestricted
crawler. Downloaded archives are scanned without extracting upstream paths.

## Coverage and limitations

- **DeviceCode:** full consolidated snapshot at commit
  `138e8afdef93488a430cbf9d62cd0ef84e4e1e7f`, matched on explicit brand,
  manufacturer, or FCC-grantee fields. Finds OEM records beneath other brands.
  This is a 2026-07-07 upstream snapshot, mostly community hardware-wiki content.
- **SONiC / OpenNetworkLinux:** platform tables, including older switch models.
  The ONL table dates to 2018. Listing is not a guarantee of a usable current NOS.
- **Edgecore:** all 239 English product URLs in the cached two product sitemaps
  scanned; 238 hardware product records imported, with ECView Pro software
  excluded. Includes inactive models, families, optics, cables and power
  accessories. Vendor descriptions, categories and structured properties are
  retained; they do not establish physical OEM, stock availability or previous
  hyperscaler use. Explicit model pairings in headings/document titles become
  searchable aliases (for example DCS501 / AS7712-32X), without merging brands.
  Compatibility lists remain separate vendor properties. All 51 notices linked
  by the cached retirement index are processed, yielding 254 affected-model
  assertions (one software-only notice excluded). This includes repeated models
  across notices and reviewed expansion of explicit grouped names. Manufacturer
  part numbers and configuration descriptions are retained when present.
  Replacement models are recommendations, not aliases.
- **ONIE:** all 111 direct Accton/Celestica `machine.make` definitions at commit
  `ce480be7094deef7198ee4d50af8427ea89b93e3`. Downloads are checked against the
  Git blob hashes in the complete machine index; incomplete bundles are rejected.
  Build files are parsed as data, never executed. Only an explicit Accton AS model
  comment agreeing with its directory maps to a model identity; other entries
  remain `onie:...` platform identifiers. Copied comments and internal names do
  not establish model equivalence. `build_*` facts are literal settings, not
  evaluated build configuration or a promise about a used unit's firmware.
- **Oracle:** four official server indexes plus storage, Ethernet, and legacy
  unified-storage indexes. Includes selected legacy Sun products, storage shelves,
  flash accelerators, switches, and adapters. The cached official retirement table
  adds 186 model assertions from 185 product rows: three Fujitsu rows excluded,
  four explicit grouped Netra rows expanded. Documentation links stay associated
  with their original cell; successor recommendations are not aliases. Reviewed
  switch aliases and part numbers are searchable. Listing as retired does not
  establish an exact support deadline. Further archives, newer ZFS systems,
  exact variants, and broader part-number maps remain work.
  The source-code index adds 74 hardware headings, including X7-2C, with
  same-section archive links where present. The licensing page explicitly names
  X10-2c as a possible Private Cloud Appliance component. NIST CMVP 4506 adds
  four named Oracle tested platforms, including E4-2C; this certifies a software
  module, not server hardware. E5-2c is separately labeled as a benchmark lead.
- **Celestica:** vendor-reviewed networking models, six storage products from
  the current vendor index, and project tables/build definitions. Reviewed web
  evidence adds DS6000, DS6001, DS6010, and the SC4200 historical catalog entry.
  These four records do not claim a local copy of their original source document.
  Codename/model equivalences are not silently inferred.
- **Amazon Data Services:** two FCC identities for grantee `2AVPL`, with two
  stable application IDs and 32 reviewed exhibit index rows. CubScout's test
  report identifies AWS branding, Foxconn manufacturing, SPARROW motherboard,
  and three tested PSU configurations. AWS documentation identifies TE1A001 as
  a Monitron sensor; an authorization letter links it to an SKF recertification
  request. These assertions have separate sources. Consumer Amazon products
  are not folded into this target.
- **Hyperscaler filter:** explicit design-lineage or regulatory-applicant evidence,
  not automatic inference from OCP support or from selling data-center switches.
  It does not establish previous ownership of a marketplace unit.
- **FCC:** a reviewed JSON manifest importer, not a comprehensive crawler or
  exhibit archive. Direct retrieval of the filing index returned HTTP 403;
  reviewed web evidence is recorded without claiming a raw snapshot. Public device
  authorizations alone cannot enumerate all wired or internal hyperscaler hardware.
- **Egypt NTRA:** 119 exact target-brand rows (6 Celestica, 18 Edge-Core,
  95 Oracle) from the cached 760-page equipment register at its January 2026 URL.
  Every page is screened, and seven candidate pages receive positional table
  extraction. Brand and model columns remain separate from linked models.
  `registry_brand` is a register association, not proof of manufacturing or AWS
  deployment. Grouped names and source spelling are preserved. The snapshot is
  pinned by SHA-256; changed PDFs require review before import. The document's
  English and Arabic printed dates disagree, so no approval date is inferred.
  This import is available explicitly; the ordinary `sync` does not refresh it.
- **UL Celestica public listing:** 25 complete CS-series identifiers extracted
  automatically from the public Models section of AZOT.E362364 using web-tool
  retrieval. The cached JSON preserves selected factual fields, source URL,
  retrieval time, and extraction pattern; it is not original HTML or the entire
  listing. Direct HTML retrieval returned HTTP 403. `listing_holder` associates
  these records with Celestica; it does not assert physical manufacturing, AWS
  deployment, or current certification validity. The explicit `ul-capture` import
  validates the source profile, holder, document, scope, and full model syntax.
  It replays the saved extract offline; ordinary `sync` does not recrawl UL.
- **India TEC:** all 2,331 rows in the cached MTCTE HTML table are retained in
  the separately queryable `tec_equipment` table. All applicants and declared
  OEMs are searchable through `lookup`.
  See [register details and SQL examples](research/tec-full-register.md). The 42
  target-related certificate rows yield 69 model assertions. The snapshot's
  newest displayed issue date is July 2024, despite the recent download. OEM Name
  and Company Name become `declared_oem` and `regulatory_applicant` relationships.
  Explicit models in the additional-model column receive separate identities,
  not aliases. Certificate IDs and historical dates remain searchable; validity
  dates do not establish current status or a vendor support deadline.
- **Seller evidence:** one cached eCrunch page supplies 7760-32X-A-AC-F and its
  seller SKU. Its tier is `marketplace` and role is `seller_reported_brand`.
  No price, live availability, technical specifications, or AWS history is
  established by this import. Seller-only matches remain leads in validation.

## FCC application and exhibit queries

```sh
python3 -m devicehunt search 'SPARROW'
python3 -m devicehunt search 'Monitron'
python3 -m devicehunt sql "SELECT a.fcc_id, e.title, e.availability, e.document_url FROM fcc_applications a JOIN fcc_exhibits e USING(record_id) WHERE e.document_type = 'External Photos'"
```

`data/fcc-manifest.json` is a reviewed, versioned input. `sync` imports it without
re-crawling the web. Counts and declared FCC IDs must agree with its contents;
incomplete or duplicate applications are rejected before replacement. These
checks validate the supplied manifest, not the completeness of a remote database.
Each import replaces the **whole `fcc-manifest` source**; retain other selected
filings in the manifest when adding new ones. Separate application IDs can share
one device identity. Exhibit availability is `listed_public`, `restricted`, or
`metadata_only`; no status asserts that a PDF has been downloaded or verified.
The offline report includes these manifests and external document links.

## Edgecore product snapshots and retirement notices

```sh
python3 -m devicehunt search 'DCS501'
python3 -m devicehunt search 'Wedge100BF-32X-O-48V-F'
python3 -m devicehunt sql "SELECT model, value FROM device_facts WHERE name = 'vendor_category' AND value LIKE '%AI Server%'"
python3 -m devicehunt sql "SELECT model, value, source_url FROM device_facts WHERE name = 'vendor_replacement_text'"
python3 -m devicehunt search 'FP2ZZ6632026A'
python3 -m devicehunt sql "SELECT model, value AS notice_effective_date, source_url FROM device_facts WHERE name = 'notice_effective_date' AND value >= '2025-01-01'"
```

The product importer verifies checksums, every sitemap in the cached index, and
every English product URL before replacing its source. Keep `products.json`
together with its sibling HTML/XML files for offline rebuilding. Cached pages
are reused after an interrupted download. `--refresh --edgecore` re-fetches the
snapshot; a failure leaves the prior database source intact, although raw cache
files may already have changed. Copy the entire cache folder before refreshing
if you need to preserve that historical snapshot.

The EOL importer accepts individual cached notices with explicit `EOL Model`,
`EOL Equipment`, or `EOS Equipment` tables. Known grouped cells have reviewed
expansions; unknown combined cells are rejected for review. The source model
cell and column label remain queryable. `notice_effective_date` comes from the
visible text, excluding scripts and stale metadata; it is **not** an inferred
support deadline or necessarily the final sale date. Raw date wording is kept.
Ordering-code suffixes remain intact. Each notice is a separate source.

The archive importer checks every notice URL in its cached index, content hashes,
canonical URLs and parseability before importing. If the index gains pagination,
it fails explicitly until the collector supports it. Each notice refresh is
atomic; a later failure does not undo earlier successful source imports. Prior
notices absent from a newer index remain as historical evidence. Retain
`notices.json` and its sibling HTML files for offline rebuilding.

Source data retains its upstream terms. DeviceCode includes CC-BY-SA 3.0/4.0,
CC0 FCC metadata, and GPL OUI overlays; see its repository LICENSE.md before
redistributing a combined dataset. Raw files, exports, and DBs are gitignored.
The project mirror's `sources/` and `AGENTS.md` are untouched.

## Validate

```sh
python3 -m unittest discover -s tests -v
node --test tests/report.test.cjs
python3 -m devicehunt sql 'SELECT COUNT(*) AS records FROM records'
python3 -m devicehunt validate-inventory --output data/exports/inventory-validation.json
```

`validate-inventory` checks the ten user-provided hardware identifiers in
`validation/owned-hardware.json`. Run it after every source expansion or refresh.
The original six are owner-reported eBay hardware; the appended Oracle baseline
contains E4-2C, E5-2C, X7-2C, and X10-2C, without an inferred operator association.
The owner corrected the shortened AZ4444 label to `AZ4444-52X-DC-10`; the fixture
preserves the previous label as correction history, not a catalog alias.
It opens the catalog read-only and never imports the expected devices. Exit codes
are **0** for all hits, **2** for any gap, and **1** for a command/input error.
Use `--format json` for full evidence, or `--inventory PATH` for another test set.

A hit requires the complete identifier in a model or identity field from an
external source with imported snapshot metadata, the expected vendor on that
same source record, and the evidenced device returned by ordinary text search.
Case and separator typography are normalized; prefixes, revisions, leading zeroes,
and ordering-code suffixes remain significant. Family matches, ONIE build targets,
replacement mentions, and unsnapshotted manual records cannot satisfy the check.
Potential identity leads and ordinary search results are included separately.
The report distinguishes `source_coverage` gaps from `search_index` gaps and
records source URLs/hashes and the inventory hash. It checks imported data, not
the current remote pages or the continued existence of raw cache files.
The owner's AWS associations are preserved as context; this check does not
independently validate deployment or the previous ownership of individual units.

Initial six-device validation: **0 hits, 6 source-coverage gaps** on 2026-09-26.
After NTRA import: **2 hits, 4 gaps**. DMC-12-48R-AS0 and AS7772-32X pass;
after the UL public-listing extract: **4 hits, 2 gaps**. Both Celestica UN-00
models passed. 7760-32X-A-AC-F and the originally shortened AZ4444-52X were gaps. Validation evidence
includes source tier and coverage so a structured web-tool extract can be
distinguished from a downloaded original document.
After TEC and seller imports, **all six queries return search results**, while
the stricter exact-identity check remains **4/6**: 7760 has an exact seller-only
match and AZ4444 has three suffixed variant leads. The report includes
`search_summary` separately from corroborated identity counts. Exit code 2
flagged the two unresolved identity gaps at that stage.
With the four Oracle identifiers appended and the additional sources imported,
the ten-device run reached **10/10 queries with results and 8/10 corroborated
exact identities**. E4-2C passes from NIST tested configurations; X7-2C and
X10-2C pass from Oracle documentation. E5-2C passes from the imported Broadcom
compatibility guide. All four Oracle labels retain the full `-2C` suffix;
7760-32X-A-AC-F and the shortened AZ4444 label were still gaps.
After the owner's full-name correction to `AZ4444-52X-DC-10`, the current run is
**10/10 searchable and 9/10 corroborated**. Existing Anatel evidence names the
exact model and EdgeCore manufacturer. That was the last document-only gap; a later supplied label photo resolves its identity mapping as described above.
See `research/owned-hardware-gaps.md` for evidence and remaining leads.
Passing code tests do not imply passing this live catalog coverage check.

Tests exercise OEM matching, Amazon entity separation, punctuation searches,
conflicting evidence, idempotency, stale-record removal, rollback on parser
failure, read-only SQL, archive path handling, HTML data escaping, ONIE bundle
completeness/checksums, copied model comments, and vendor navigation filtering.
Oracle retirement tests also cover grouped models, exact documentation-cell
provenance, successor/alias separation, third-party exclusions, and rejection of
unexpected rows without deleting previously imported evidence.


### Additional Oracle snapshot imports

These imports are local and repeatable; `sync` does not fetch these three pages.
Source URLs, SHA-256 hashes, and scope are retained in the database.

```sh
python3 -m devicehunt import oracle-oss data/raw/oracle-oss-x86.html
python3 -m devicehunt import oracle-x10-license data/raw/oracle-x10-2c-license.html
python3 -m devicehunt import nist-4506 data/raw/nist-4506.html
python3 -m devicehunt import curated data/curated.json
python3 -m devicehunt validate-inventory --output data/exports/inventory-validation.json
```

Oracle source tests keep `E5-2L` distinct from `E5-2C`, prevent archive links in
adjacent version sections from being guessed as hardware evidence, require the
explicit licensing statement, and constrain NIST extraction to tested platforms.


## Full historical FCC company register

The database now includes all **50,153 company registration rows** in the FCC's
public [EAS grantee export](https://opendata.fcc.gov/d/3b3k-34jp). Dataset metadata
says **last updated March 22, 2021**. This is a historical company/code directory,
not the full FCC device authorization database or a current active-company list.
The original CSV, metadata and row-count responses are retained locally.

```sh
python3 -m devicehunt grantees 'Celestica'
python3 -m devicehunt grantees 'HED'
python3 -m devicehunt grantees 'Amazon Data' --format json
python3 -m devicehunt sql "SELECT grantee_code, grantee_name, country FROM fcc_grantees WHERE grantee_name LIKE '%Oracle%'"
python3 -m devicehunt import fcc-grantees data/raw/fcc-grantees-bundle.json
# Optional download as part of a normal sync; --refresh re-fetches the register.
python3 -m devicehunt sync --fcc-grantees
```

`grantees` searches company names and codes. It defaults to 100 results; use
`--limit -1` to export all rows with `--format csv`. Use SQL for address, country,
contact and registration-date fields; `date_received` preserves the source's
original date/time string. Grantees have their own table and never become device
identities or evidence of physical manufacturing. Exact source names are retained,
including different legal entities and one registration with an empty name.

The collector compares API row counts and dataset update timestamps before and
after downloading, then verifies every CSV row and rejects duplicate codes or
truncation. The bundle hashes every input. All rows participate in offline
rebuilds, which check the grantee count separately. This does not download filings
or exhibits for those codes; filing discovery remains further coverage work.

Useful observed codes: Accton Technology Corp `HED`, Edgecore Networks Corporation
`YZK`, Oracle America `A4H`, Amazon Data Services `2AVPL`, Celestica Incorporated
`MRF`, and Celestica Limited `NVN`. Legacy Sun registrations `GWV` / `UDM` and
Accton Wireless Broadband `V8Y` stay distinct legal-entity leads.


## ICASA equipment register

The complete cached [2025 ICASA equipment PDF](https://www.icasa.org.za/uploads/files/List-of-Type-Approved-Equipment-2025.pdf)
is available locally: 906 pages and 26,258 positional rows. Of these, 17,996 have
nonempty fields and a recognized TA/TE/PA reference; 8,262 require review because
of empty, shifted or unrecognized fields. This classification concerns parsing,
not current regulatory validity. The register lacks a manufacturer/applicant
column, so these rows remain separate from target-company identities.

```sh
python3 -m devicehunt registry 'AZ4444'
python3 -m devicehunt registry 'DMC-12-48R-AS0' --format json
python3 -m devicehunt sql "SELECT model, approval_number, page FROM equipment_register WHERE parse_status='identified' AND model LIKE 'AZ4444%'"
python3 -m devicehunt import icasa data/raw/icasa-2025-bundle.json
```

Raw cells remain searchable for every row, including those needing review. Such
rows have NULL interpreted fields; inspect the cited PDF page before interpreting
them. `registry` defaults to 100 rows; use `--limit -1` to export all results.
`show` and the offline HTML catalog also display exact, case-insensitive model
string matches from clearly aligned rows. These matches supply regulatory leads;
they do not verify the same hardware, manufacturer, or owner. Grouped model cells
are preserved whole. Prefixes and suffixes are never discarded to create a match.

The extraction bundle retains the original PDF hash, extracted-table hash and
expected counts. Imports and offline rebuilds need only the standard library for
this source. To reproduce extraction from the reviewed PDF, install optional
`pdfplumber` (this capture used 0.11.9) and call `devicehunt.icasa.extract(pdf_path, tables_path)`; review
changed extracted output before updating its bundle hash. Existing NTRA PDF
rebuilds still require their optional PDF packages.


## Buying-relevant specifications

Cached Edgecore product pages now contribute power, cooling, physical format
and software-installation statements from their explicit Features sections.
Navigation, compatible-product lists and unrelated page sections are excluded.
Repeated layout blocks are deduplicated. Source wording, SKU alternatives and
qualifiers remain intact: a transceiver wattage is not interpreted as chassis
consumption, and AC/DC or airflow alternatives do not describe every used unit.

```sh
# Exact model / explicit alias: excludes accessories that only mention this model.
python3 -m devicehunt specs DCS204 --exact-model
python3 -m devicehunt specs AS7726-32X --exact-model --format json
# Complete part number: only specifications attached to that code's source records.
python3 -m devicehunt specs R4023-F9102-A0 --exact-part-number --format json
# Keyword search across the catalog, with each result labeled by its own model.
python3 -m devicehunt specs '48V' --vendor Edge-Core
python3 -m devicehunt specs --limit -1 --format csv
python3 -m devicehunt sql "SELECT brand,model,value,source_url FROM device_facts WHERE name='cooling_description'"
```

The offline HTML page has a dedicated power/cooling/installation section for
products with these statements. They remain ordinary sourced facts in SQL:
`power_description`, `cooling_description`, `physical_description`, and
`software_installation_description`. Missing facts mean unknown; no PSU rating,
voltage, airflow direction, NOS availability, or switch-software entitlement is
derived from a model suffix. A `specs` query without `--exact-model` uses the
same broad full-text matching as `search` and can return compatible accessories.
The exact flag compares complete model/explicit model-alias strings using Unicode
case folding and outer whitespace trimming, independently of the discovery index.
It preserves suffixes and does not turn a family into an exact ordering code.
An alias qualifies only the record carrying that alias. In every mode, vendor
filtering requires an affiliation on the statement's own source record. All
results include record IDs and revisions; other records cannot lend an alias
or company association. See [specification search scope](docs/specification-search.md).
`--exact-part-number` instead selects specification statements from records that
explicitly carry the complete `part_number` identifier. Vendor filtering applies
to those same records. JSON includes their record IDs, revisions and original
matching part-number strings. Conflicting records remain separate; an empty
result means no categorized specifications on a matching record, even if family
specifications exist elsewhere. The two exact modes are mutually exclusive.

Exports: `data/exports/specifications.csv` contains all categorized claims;
`data/exports/dcs204-specifications.csv` is a small example for DCS204. Reimport
`edgecore` and regenerate these exports after changing its cached source set.


## Bahrain TRA manufacturer records and Amazon leads

The [Bahrain public equipment register](https://etra.tra.org.bh/approved-equipment)
provides a structured public API used by its own search/download interface.
The captured unfiltered response contains **10,169 records**. All are retained in
`bahrain_equipment`; 33 rows with exact target manufacturer names contribute
34 model assertions to the main catalog (one row explicitly lists two models).
The registered manufacturer role is `registry_manufacturer`. It does not prove
the physical factory, current approval status, deployment or a used unit's owner.

```sh
python3 -m devicehunt search --vendor 'Amazon Data Services'
python3 -m devicehunt sql "SELECT model_number,device_description,authority_ref,expiry_date,status_code FROM bahrain_equipment WHERE manufacturer='AMAZON DATA SERVICES, INC.'"
python3 -m devicehunt import bahrain data/raw/bahrain-bundle.json
# Optional collection during normal sync; --refresh requests a fresh capture.
python3 -m devicehunt sync --bahrain
```

Two complete responses must agree by authority reference and content. Six company
queries must be consistent with the unfiltered response and include its expected
manufacturer matches. All response files are hashed. The website paginates its
array in the browser and builds its spreadsheet from that array. It exposes no
independent server total: completeness beyond the returned response is unverified.
This is a current collection of public records, not a complete historical archive.

Sixteen entries explicitly list Amazon Data Services, including SCOUT01/AWS,
SCOUT02/AWS, ATLANTA23SF, ATLANTA23SFX, Atlanta1U, Hopper26SF, Wukong, Ambrose87,
Ambrose76, Boxer2, Monza, BARRELEYE COMPUTE, BARRELEYE SWITCH, BARRELEYE MGMT,
HP89j and HP87j. `data/exports/amazon-bahrain-leads.csv` pairs these identifiers
with original references and eBay search links. These are discovery leads, not
checked offers. Use the vendor filter to retrieve these manufacturer associations;
no AWS operating history is inferred for the `--hyperscaler` evidence filter.

The Celestica Shanghai legal name is explicitly mapped to the Celestica target
while preserving the original legal name and regulatory role. Comma-separated
model cells produce separate records; slash names and voltage suffixes remain
intact. Marketing names such as "Server" are facts, never identity aliases.
The `expiry` column retains DD-MM-YYYY; `expiry_date` supplies YYYY-MM-DD for SQL.
Raw `RNW` / `DONE` status values are preserved rather than asserting present-day
validity or vendor support from them.

## AWS Outposts model mapping

Cached AWS compliance documents now identify AWS-branded SCOUT01 and SCOUT02
under the Outposts 2U section, and CubScout under 1U. Their declared manufacturer
is Amazon Data Services, Inc. Use `search SCOUT --hyperscaler` to find the
documented product-family relationships. These are not claims about previous
ownership or service eligibility of a surplus unit. The published PDFs have
blank signature/date fields; no completed certification is asserted.

See [reviewed evidence](research/aws-outposts-models.md) and import with
`python3 -m devicehunt import curated data/aws-outposts.json`. The offline rebuild
manifest includes the reviewed facts, original PDFs and index HTML.

## Oracle server compatibility records

`python3 -m devicehunt import broadcom data/raw/broadcom-servers-bundle.json`
imports 24 public Broadcom guide records, with exact Oracle partner/model
identities and scoped compatibility details. The importer checks API totals,
repeated queries, detail identities and hashes before replacing evidence.
`search E5-2C --vendor Oracle` now has independent corroboration: baseline
coverage reached **8/10**, with all ten labels searchable (now **9/10** after
the owner's AZ4444 full-name correction). See
[the source review and SQL examples](research/broadcom-server-evidence.md).

## Full Anatel product register

The complete published Anatel CSV snapshot is available in `anatel_equipment`: 
190,852 rows, including duplicate records and empty model cells. All 21 original
columns are retained as text, with source ID and data-record number. The catalog
adds 235 assertions for exact target manufacturer names. This is the published
file's coverage; no independent total proves completeness against Anatel's
underlying live database.

```sh
python3 -m devicehunt anatel AZ4444 --limit -1
python3 -m devicehunt anatel 'Amazon Data Services' --format csv
python3 -m devicehunt import anatel data/raw/anatel-bundle.json
python3 -m devicehunt sync --anatel
```

`sync --anatel --refresh` downloads two full copies and verifies that they agree.
As with other sync flags, it also refreshes the default small source indexes.
The importer checks the ZIP member, CSV headers/width, hashes, source-index link
and record count before replacing register evidence. It preserves approval IDs
with leading zeroes, raw statuses, expiry dates and separate applicant roles.
Commercial-name cells stay searchable facts, without becoming automatic aliases.

```sql
SELECT manufacturer, model, approval_number, certificate_status
FROM anatel_equipment
WHERE manufacturer = 'Amazon Data Services, Inc';
```

See [Anatel evidence and field mapping](research/anatel-product-register.md).

## Meta networking platforms

Four cached Meta engineering articles add five platform assertions: Edge-Core
Minipack, Celestica Wedge 400 / Wedge 400C / Minipack3, and Accton Minipack3N.
Build-partner and manufacturer roles remain distinct. Historical deployment,
announced deployment and design lineage are separate relationships; none proves
the previous owner of a surplus unit.

```sh
python3 -m devicehunt import meta-networking data/raw/meta-networking-bundle.json
python3 -m devicehunt search Wedge --hyperscaler
python3 -m devicehunt search Minipack --hyperscaler
```

See [the dated source review](research/meta-networking-evidence.md) for ASIC,
module and evidence scope details. The import verifies all four original HTML
snapshots before replacing evidence. At that earlier stage, the ten-device validation baseline was
10/10 searchable and 9/10 corroborated, including all four Oracle entries and
the owner's corrected AZ4444-52X-DC-10 label.

## OCP product evidence

`python3 -m devicehunt import ocp-capture data/raw/ocp-products-reviewed.json`
imports 13 model assertions from 12 reviewed OCP product pages. This is a
structured web-tool extract, not original HTML or a complete OCP mirror.
Exact ordering suffixes, separate solution-provider roles and selected buying
details remain visible. AS7816-64X-F and AS7816-64X-B have explicitly documented
opposite airflow directions. No linked design name becomes an automatic alias.
See [scope, sources and SQL examples](research/ocp-product-evidence.md).

## Full Egypt NTRA register

All 47,302 numbered rows in the cached 760-page NTRA PDF are now available in
`ntra_equipment`, independently of the 119 target-brand catalog assertions.
This includes 15 AWS-branded server rows. Registry brand does not establish a
manufacturer or the Amazon Data Services legal entity. Twelve rows containing
undecodable text retain raw cells and require review.

```sh
python3 -m devicehunt ntra Atlanta --limit -1
python3 -m devicehunt lookup Hopper26SF
python3 -m devicehunt import ntra-register data/raw/ntra-register-bundle.json
```

```sql
SELECT brand, model, linked_models, registry_number, source_url
FROM ntra_equipment WHERE brand = 'AWS';
```

The full register is exported in `data/exports/ntra-register.csv`; the exact AWS
brand selection is in `data/exports/ntra-aws.csv`. See
[extraction checks and source limitations](research/ntra-full-register.md).

Historical NTRA coverage now includes all 29,208 rows from December 4, 2022,
with combined brand/model cells preserved for SQL and local lookup. Together
with the January 2026 file this is 76,510 snapshot rows, not unique devices.
See [historical NTRA scope and queries](research/ntra-historical-register.md).

Oracle component options now include 30 sourced assertions for 15 marketing
part numbers across Sun 7420, ZS3-2 and ZS3-4. Nine printed manufacturing-part
mappings remain scoped to their source rows. See
[component searches, mappings and source disagreement](research/oracle-component-options.md).
