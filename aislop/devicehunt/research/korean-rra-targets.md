# Official Korean RRA queries for all five targets

A later [Sun legacy query](sun-legacy-portfolio.md) adds 40 raw approvals and
keeps Oracle corporate lineage separate from the Sun manufacturer role.
The combined current register has 1,081 rows and 1,080 distinct certificates.

The counts below describe the initial five-query capture. Subsequent
[Celestica legal-name review](rra-celestica-company-names.md) adds 42 assertions
and 39 identities without changing the 1,034 raw approval rows. Current inventory
counts are in [the inventory guide](../docs/vendor-inventory.md).
The subsequent [Edge-Core spelling checks](rra-edge-core-spellings.md) add seven
approval rows and preserve one separately confirmed zero-result query.
The [Accton Singapore name review](rra-accton-company-name.md) adds 13 model
assertions from those existing rows; the table below remains the original capture.

The public manufacturer search was captured on 2026-09-26 for the literal query
strings below, with dates `19900101`–`20260926` and every other filter blank.
Each source includes the original form, byte-identical CSV exports before and
after collection, and a matching nine-field detail page for every certificate.
Original source URLs and SHA-256 hashes are retained locally.

| Literal manufacturer query | Approval rows | Exact mapped manufacturer rows | Model assertions |
|---|---:|---:|---:|
| Celestica | 151 | 48 | 58 |
| Amazon Data Services | 57 | 57 | 161 |
| Edgecore | 110 | 101 | 146 |
| Accton | 200 | 150 | 163 |
| Oracle | 516 | 509 | 852 |

Together these preserve 1,034 query rows and 1,033 distinct certificate numbers.
SUN-6000 appears in both the Celestica and Oracle queries with the same composite
manufacturer cell; both query observations remain raw evidence. The four newer
queries add 1,041 catalog identities and 1,322 assertions, bringing the complete
collected catalog to 3,256 identities, 4,263 assertions and 172 sources.

These are returned query snapshots, not a complete Korean device registry or
complete historical company inventories. Alternate spellings, legacy legal
entities, renamed companies, and devices outside this certification system can
be missed. In particular, the literal Edgecore query does not claim to cover
every spelling containing a hyphen or space.

The [public search form](https://www.rra.go.kr/ko/license/A_c_search_en.do)
links the CSV export; the
[regulator's documentation](https://www.rra.go.kr/ko/notice/go_7_4.do)
describes the certificate detail URL. Collection is sequential per query, with
a short delay, no credentials and no automatic retries. Partial captures remain
on disk and are not published as import bundles.

## Evidence and model boundaries

CSV intermediate columns contain unquoted commas and cannot safely supply model
or company fields. Discovery uses only the row number and certificate. The
complete labelled detail table supplies applicant, manufacturer, main model and
derived model claims. Observed underscores in eight certificate numbers are
preserved literally; each complete certificate is URL-encoded for retrieval.

Only a whole manufacturer cell matching an existing exact target-name mapping
creates catalog manufacturer assertions. Composite company cells and unmapped
legal names stay in the raw `rra_equipment` table and remain searchable through
`lookup`. Applicant and manufacturer are separate roles. The catalog brand for
these imports is a manufacturer namespace, not necessarily a retail brand.
The separate [reviewed manufacturer-list extraction](rra-manufacturer-lists.md)
now selects complete names from 19 explicitly reviewed full-cell spellings;
the default importer and original rows remain unchanged.
Simple comma-separated main-model lists and derived models become distinct
literal identities. Main lists with annotations or empty entries stay raw in full;
no prefix/suffix propagation or model aliases are inferred. The
[main-list expansion](rra-main-model-lists.md) recovers 84 assertions from existing
pages. Neither country cells nor manufacturer claims prove the physical
factory, operator deployment or provenance of a particular auction unit.

The RRA importer additionally rejects a target mapping when the company cell
contains non-ASCII letters or digits: the currently reviewed aliases are ASCII,
and Korean text must not disappear during normalization. This keeps eight rows
for `주식회사 트렌토 시스템즈 (Edgecore Networks Corporation)` as composite raw
evidence. The same rule protects applicant affiliations. Other-source name
normalization is unchanged. `Oracle audio technologies` also remains unmapped;
a search substring alone is not a reviewed company identity.

The Amazon detail records explicitly name **Amazon Data Services, Inc.** as
manufacturer. Examples include BARRELEYE COMPUTE, BARRELEYE SWITCH, BARRELEYE MGMT
and SAT1A_C2. Derived models materially expand the inventory beyond the 57
approval rows. A shared code such as AZ9074-64X-DC-42 also occurs in the separate
Celestica capture; both manufacturer claims and their sources are retained.
The importer does not merge those company identities or infer a supplier contract.

Accton approvals include AZ4444-52X-AC-00, AZ4444-52X-DC-42 and AZ4444-52X-DC-10,
plus AS7760-32X as a derived model of AS7710-32X. None establishes the baseline's
bare Edge-Core AZ4444-52X or complete 7760-32X-A-AC-F ordering code. Those exact
coverage gaps remain open.

## Repeatable collection and local searches

```sh
python3 -m research.capture_rra_query 'Amazon Data Services' data/raw/rra-new-query --end 20260926
python3 -m devicehunt import rra data/raw/rra-new-query/bundle.json
python3 -m devicehunt lookup BARRELEYE
python3 -m devicehunt search SAT1A_C2 --vendor 'Amazon Data Services' --role registry_manufacturer
python3 -m devicehunt vendor-inventory Oracle --limit -1
python3 -m devicehunt sql "SELECT main_model,derived_models,manufacturer,applicant,certificate_number,source_url FROM rra_equipment WHERE manufacturer LIKE '%Amazon Data Services%' ORDER BY main_model"
```

The collector requires a new output directory. Optional `--form` and `--before`
reuse an already saved official form and a CSV obtained using the exact same
query fields. They do not change the required final export, dependency hashes,
row coverage, certificate equality or manufacturer checks. The importer verifies
the entire bundle before replacing data for its query.

The source-specific CSV/JSON exports are `data/exports/rra-<query>-official.*`.
The audit `data/exports/rra-target-query-audit.json` records raw row counts,
eligible manufacturer rows, model assertions and unmapped name frequencies.
New identities relative to the previous verified Celestica-only rebuild are in
`data/exports/rra-target-new-identities.json`.

The ten-device owned baseline remains 10/10 searchable and 8/10 independently
corroborated. All four Oracle entries pass; no expected label was changed or
seeded into source evidence.

Verification: all 270 tests pass. The 71-job manifest verifies 1,806 input files
and reproduces the live database in `data/rebuilt-with-rra-targets.sqlite`;
31 tables/search indexes and the owned-baseline results match. The three vendor
inventory exports, offline catalog and 36 saved lookup reports are refreshed.
Their read-only generation leaves the live database byte hash unchanged.
