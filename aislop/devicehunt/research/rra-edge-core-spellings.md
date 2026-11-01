# Edge-Core spelling coverage in the Korean registry

The initial manufacturer query used literal `Edgecore`. Separate public queries
on 2026-09-26 used `Edge-Core` and `Edge Core`, with dates `19900101` through
`20260926` and every other field blank. The official source is the
[RRA search form](https://www.rra.go.kr/ko/license/A_c_search_en.do).

| Literal manufacturer query | Returned approval rows | Model assertions |
|---|---:|---:|
| Edgecore | 110 | 146 |
| Edge-Core | 7 | 13 |
| Edge Core | 0, explicitly confirmed | 0 |

All seven hyphenated-query certificate numbers were absent from the previous
capture. Their original labelled detail pages validate against the certificates,
and the before/after CSV exports are byte-identical. They add five catalog
identities: `5812-54X-O-AC-B`, `7712-32X-O-AC-B`, `CSS-24H2L`, `CSS-24H3L` and
`ECS2000-18T`. Main and derived model labels remain separate; full suffixes are
preserved. Two COMMAX-applicant records explicitly name Edge-core Networks as
manufacturer. The applicant is not substituted for the manufacturer.

WA6202A now has an Edge-Core manufacturer claim from RRA alongside its existing
Accton manufacturer claim in the NCC reviewed source. Both remain attached to
the same literal catalog identity with their original evidence. The importer
does not choose a winning manufacturer or infer a contract between the companies.

## Distinguishing a confirmed zero from missing data

The spaced query returned a header-only CSV. That alone is insufficient: it
could reflect a failed or incomplete export. The separately retrieved original
search-result page contains the expected title, Search Results heading, six
table column headings, and one cell explicitly reporting no matching information.
A second identical header-only CSV confirms the dated snapshot.

The importer now accepts zero rows **only** with this checked, hashed result
page, the source form and matching before/after CSVs. It rejects generic error
pages, missing proof, altered headers, extra result rows/tables, nonempty details,
contradictory row counts and a zero-result claim alongside nonempty CSV data.
The default low-level CSV parser continues to reject empty data unless the
caller explicitly opts into this validation path.

The HTML does not echo the search criteria; the bundle preserves the exact POST
fields used by the collector. The zero result belongs to this literal query and
date range. It does not mean the company has no devices, no Korean approvals or
no records under other spellings. It remains a source record with `records_seen=0`
and a coverage statement, without a fabricated device or raw equipment row.

```sh
python3 -m research.capture_rra_query 'Edge-Core' data/raw/rra-new-hyphen-query --end 20260926
python3 -m research.capture_rra_query 'Edge Core' data/raw/rra-new-space-query --end 20260926
python3 -m devicehunt lookup CSS-24H2L
python3 -m devicehunt vendor-inventory Edge-Core --limit -1
python3 -m devicehunt sql "SELECT id,records_seen,coverage FROM sources WHERE id LIKE 'rra-official-query-%' ORDER BY id"
```

The collector fetches the no-results page only for a header-only export. Optional
`--empty-result` reuses an already retrieved official page for the exact same
query, with the same validation and final confirming CSV. A network failure
retains partial files but publishes no bundle and triggers no automatic retry.
A valid empty refresh withdraws only its own query's old records, preserving
other queries and sources.
Unified lookup includes metadata for verified zero queries even when they have
no equipment rows, so a loaded zero is not misreported as an uncollected source.

## Results and verification

The catalog now has 3,300 identities, 4,318 assertions and 174 sources. The RRA
raw table has 1,041 query rows covering 1,040 distinct certificates across seven
query sources. The extra source for the confirmed zero contributes no raw rows.
Edge-Core manufacturer scope has 216 identities across 286 source records/five
sources. Across all targets, 1,696 manufacturer target/identity pairs cover 1,695
distinct identities because WA6202A has claims for two target companies.

The audit is `data/exports/rra-edge-core-spelling-audit.json`; seven complete raw
rows are exported as `data/exports/rra-edge-core-hyphen-official.csv` and JSON.
Both owned-baseline gaps remain open. The benchmark remains 10/10 searchable
and 8/10 independently corroborated, including all four Oracle models.

All 279 tests pass. Five new zero-result tests cover explicit zero handling,
query-scoped withdrawal, contradictory/missing proof, collection/network-failure
behavior and source visibility alongside nonempty queries. A separate catalog
search regression covers duplicate, stale and missing text-index entries.
A fresh rebuild from 73 jobs and 1,823 hashed files matches all 31 checked
tables/search indexes and the owned baseline.
