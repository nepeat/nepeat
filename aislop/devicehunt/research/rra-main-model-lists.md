# Explicit main-model lists in Korean approvals

Reviewed September 27, 2026. The official RRA snapshots already contained
comma-separated model lists in the labelled main-model field. The importer
previously retained these cells in the raw register but skipped them when
building the device catalog. It now imports each literal entry when the entire
main list consists of simple model labels.

This recovers **84 manufacturer assertions and 75 catalog identities** from the
existing original detail pages: 28 assertions for Celestica, 12 for Accton and
44 for Oracle. No additional remote data was needed. These are regulatory model
labels, not 75 proven distinct physical products.

| Manufacturer namespace | Example main-model cell | Imported literal entries |
|---|---|---|
| Celestica | `MTYA-R,CP3060-RTM` | `MTYA-R`; `CP3060-RTM` |
| Celestica | `WS-C3550-24,WS-C3550-24-SMI,WS-C3550-24-EMI` | All three complete names |
| Accton | `BayStack 325-24G,BayStack 325-24T` | Both complete names |
| Oracle | `Sun Fire Rack,SFE-CAB` | `Sun Fire Rack`; `SFE-CAB` |

Every assertion retains its certificate, original detail URL and hash, raw
manufacturer/applicant, complete printed main-model cell and `rra_model_role=main`.
All 1,081 raw approval rows remain unchanged. The previous 4,490 catalog records
are preserved. Company mappings and their evidence dependencies are unchanged.

## Boundaries

- Parsing uses the labelled detail HTML, never the malformed intermediate CSV
  columns, which themselves contain unquoted commas.
- Each main-list entry must match the existing simple ASCII model syntax. A
  list with a slash, colon, parenthesized annotation, empty entry, `null`, or
  other unsupported text remains raw in full. Separately valid derived models
  still remain available.
- Prefixes and suffixes are never propagated. `Sun Fire 280R,350R` yields
  `Sun Fire 280R` and `350R`, without inventing `Sun Fire 350R`.
- Names on the same certificate become distinct literal catalog identities,
  never automatic aliases. A regulatory code and commercial name may describe
  overlapping hardware. Numeric identifiers such as `15938` and `15939` remain
  literal; marketplace matching retains its existing short-numeric safeguards.
- Case-insensitive duplicate entries are imported once, with main taking
  precedence over derived. No template or wildcard expansion occurs.
- Composite/unreviewed manufacturer cells remain unmapped. No brand, physical
  factory, operator deployment or ownership of a particular unit is inferred.

## Local use

```sh
python3 -m devicehunt search CP3060-RTM --vendor Celestica --role registry_manufacturer
python3 -m devicehunt lookup 'BayStack 325-24G'
python3 -m devicehunt lookup SFE-CAB
```

```sql
SELECT d.brand, d.model, a.company AS manufacturer, f.value AS main_model_as_printed,
       r.source_url
FROM records r
JOIN devices d ON d.id=r.device_id
JOIN affiliations a ON a.record_id=r.id AND a.role='registry_manufacturer'
JOIN facts f ON f.record_id=r.id AND f.name='rra_main_model'
JOIN facts role ON role.record_id=r.id AND role.name='rra_model_role' AND role.value='main'
WHERE r.source_id GLOB 'rra-official-query-*' AND f.value LIKE '%,%'
ORDER BY d.brand,d.model,r.source_url;
```

The reviewed candidates, original hashes, rejected/unmapped rows and exact
added assertions are in `data/exports/rra-main-models-review.json`,
`rra-main-models-additions.json`/`.csv` and `rra-main-models-import-audit.json`.
The catalog now contains 3,480 identities, 4,574 assertions and 188 sources.
Manufacturer inventory has 1,813 target/identity pairs across 1,812 identities.
The baseline remains 10/10 searchable, 8/10 corroborated; the two exact Edge-Core
gaps are unchanged. Full historical/FCC/worldwide coverage remains unproven.

Tests cover literal names, no prefix propagation/aliases, duplicate roles,
ambiguous-list rejection, company-role isolation, raw/provenance retention,
withdrawal, source isolation and removal from search results.

All 320 tests pass. The 86-job, 1,913-file offline rebuild at
`data/rebuilt-with-rra-main-models.sqlite` matches all 37 compared tables/search
indexes and baseline results. All 61 saved lookup reports, three vendor-role
inventories and offline HTML are refreshed. Read-only export generation and
the documented SQL preserve the live database byte hash. The stricter delta
check confirms all existing affiliations, identifiers, facts and hyperscaler
evidence are unchanged, as are baseline results except updated source-coverage
wording. See `rra-main-models-proof-check.json` and
`rra-main-models-readonly-check.json`.
