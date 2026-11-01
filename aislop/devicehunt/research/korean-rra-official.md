# Official Korean RRA manufacturer queries

The original Celestica capture below is followed by additional manufacturer
queries documented in [the five-target capture audit](korean-rra-targets.md).
Counts in this page's Celestica investigation describe that earlier snapshot.
The later [company-name review](rra-celestica-company-names.md) expands eligible
manufacturer rows using an explicitly labelled subsidiary-filing text extract.

Captured 2026-09-26 using the public
[RRA search form](https://www.rra.go.kr/ko/license/A_c_search_en.do), manufacturer
`Celestica`, dates `19900101` through `20260926`, all other filters blank.
The CSV exports before and after collection are byte-identical and contain
151 consecutive numbered rows with 151 distinct certificate numbers. Every
number has a matching original detail page through the
[regulator's documented certificate URL](https://www.rra.go.kr/ko/notice/go_7_4.do).
No login, credentials, access-denial workaround or guessed certificate was used.

This establishes the returned query snapshot, not the complete Korean database.
The search result's pagination links omit a question mark; those malformed links
were not followed or repaired. Its first page is retained as collection context.
The CSV covers 151 rows despite the page displaying ten. No independent full
database count was exposed.

## Results and roles

All 151 labelled detail records are in `rra_equipment`, including cross-brand and
composite manufacturer fields. They are searchable through `lookup` and SQL.
Dates, country lists, remarks and company spellings remain raw source claims.

48 records have a manufacturer cell that exactly matches the catalog's existing
target-name normalization. Those yield 58 main/derived model assertions,
19 new catalog identities and 25 additional Celestica manufacturer-claim
identities. Manufacturer and applicant remain separate affiliations. For example:

| Main model | Applicant | Manufacturer |
|---|---|---|
| WS-C3550-48-SMI | CISCO SYSTEMS, INC. | CELESTICA(THAILAND) LIMITED |
| VSX 3000 | Polycom Inc. | CELESTICA (THAILAND) LIMITED. |
| 3C996-T | HEWLETT-PACKARD COMPANY in one record | CELESTICA(THAILAND) LIMITED |
| AZ9074-64X-DC-42 | CELESTICA TECHNOLOGY CONSULTANCY (SHANGHAI) CO LTD | same |

AZ9075-64X-DC-42 is explicitly a derived model in the AZ9074 detail. Both are
separate identities, not aliases. No AWS operator relationship is inferred from
their naming or their occurrence in other sources. The catalog's Celestica brand
field is a manufacturer namespace here, not proof of a printed retail brand.
Seven Oracle-applicant associations are added separately; they do not become
Oracle manufacturer claims.

The other 103 records remain in the raw searchable table. Some contain legal
entity spellings not yet in the target mapping, such as CELESTICA LLC and LTD
variants; others combine Celestica with Keysight, Juniper, HP or other companies.
The importer does not split composite company cells or infer a physical factory.
Reviewing those names and importing additional target-company queries remain
follow-up work. A raw result is not silently promoted to a manufacturer claim.

## CSV and encoding issues

The form, search result and CSV use CP949 bytes, despite the HTML's UTF-8 meta
label. Detail pages decode as UTF-8. The CSV has blank lines and unquoted commas
inside intermediate fields. Only its numbered first cell and final certificate
cell are used for discovery and association. All raw cells remain in `cells_json`;
the original byte snapshots are retained. Identity and company fields come from
the detail page's complete nine-row labelled table, with the certificate checked
against both the CSV and URL. No shifted CSV column is interpreted as a model.

At this initial checkpoint, main model cells remained unsplit. The later
[main-list parser](rra-main-model-lists.md) accepts fully simple comma-separated
main lists as literal entries. Simple comma-separated derived model names can
also produce separate records; annotated or otherwise ambiguous items stay raw.
These rules do not claim that every raw model string becomes a catalog
identity. No historical/current certification validity is determined.

## Relationship to the previous mirror capture

The official record corroborates CS8000-32X-DC-11 and CS8001-32X-DC-11 with the
same named manufacturer. Its certificate is printed `R-R-ZZR-CS8000-32X-DC`;
the mirror used `RR-ZZR-CS8000-32X-DC`. Both literal values are retained under
their respective sources. No identifier alias is invented. The original country
field says China in Korean, while the English mirror includes Mexico and
Thailand in parentheses. This discrepancy remains visible in separate facts.

The documented emsit API returned HTTP 400 over both HTTP and HTTPS and supplied
no imported response. The working official detail URL is a separately documented
public interface. FCC research in this pass found the already known grantee
export and API documentation, without a new complete filing dump; no denied FCC
endpoint was retried.

## Local use and verification

```sh
python3 -m devicehunt import rra data/raw/rra-celestica-official/bundle.json
python3 -m devicehunt lookup 'Celestica'
python3 -m devicehunt lookup 'VSX 3000'
python3 -m devicehunt search WS-C3550-48-SMI --vendor Celestica --role registry_manufacturer
python3 -m devicehunt sql "SELECT main_model,derived_models,applicant,manufacturer,certificate_number,source_url FROM rra_equipment ORDER BY row_number"
```

`research/capture_rra_details.py` downloads the certificate detail URLs from a
saved official CSV into a new directory, sequentially with a short delay and no
automatic retries. The resulting progress file is not an import bundle: a
second matching CSV, source form and reviewed query metadata are required before
publication. The importer validates all dependencies before replacing its query.

Schema 17 adds `rra_equipment`. Older manifests default its expected count to
zero. Four focused tests cover raw CSV ambiguity, original detail labels,
composite companies, model boundaries, corrupt/incomplete captures and scoped
refresh. All 266 tests pass. New identities are listed in
`data/exports/rra-new-identities.json`.

Current totals: 2,215 identities, 2,941 assertions, 168 sources. Celestica has
65 manufacturer-claim identities with 108 source records across seven sources.
The owned baseline remains 10/10 searchable and 8/10 corroborated; the two
Edge-Core exact-label gaps remain open.
