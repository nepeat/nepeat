# Reviewed multi-company manufacturer fields

The original Korean RRA captures contain approvals whose labelled manufacturer
field lists several complete company names. The default RRA importer preserves
these composite cells without mapping them. A separate reviewed extraction now
adds named target manufacturers from **19 exact comma-list spellings**, covering
22 approvals and 45 model assertions in the cached captures.

This reuses existing original approval pages; it is not a new retrieval or an
independent corroborating source. The source ID is
`rra-reviewed-manufacturer-lists`. The checked bundle is
`data/raw/rra-manufacturer-lists.json`; it references the original Celestica,
Accton and Oracle query bundles, their hashes and their company-name evidence.

| Target | Added model assertions | Examples |
|---|---:|---|
| Accton | 6 | J8131A, WN4201B, WS-AP2605, WS-AP3605, WS-AP3620, WS-AP3640-ROW |
| Celestica | 37 | M120, M320, T640, T6300, X6200_Series, XFrame-II-LP, QDX 6000 |
| Oracle | 2 | DC1ST, DC2ST-3ST |

The added assertions introduce 42 catalog identities. These names can include
components, regulatory codes or overlapping product labels; they are not a count
of 42 proven distinct physical products. Celestica appears on some approvals
alongside Juniper, Neterion/Foxconn, Wistron or other companies. The data supports
the named regulatory manufacturer association, not sole manufacture, a specific
factory or the maker of a particular auction unit.

## Selection and evidence boundaries

Only an exact reviewed full-cell string enables its reviewed decomposition.
There is no generic split-on-comma operation: commas inside company suffixes and
legal names are retained. Complete selected names must independently match the
existing whole-company rules or their checked query-scoped legal-name evidence.
Unrecognized companion text remains context without acquiring a target mapping.
For example, a Celestica Kladno claim does not also assert an unreviewed mapping
for the adjacent Celestica de Monterrey name.

The reviewed boundaries are in `devicehunt/rra_manufacturer_lists.py`. The full
manufacturer text, selected names, companion text, original detail URL/hash,
query and query-bundle hash remain with every assertion. Optional legal-name
evidence retains its source, capture method, historical-period limitations and
mapping interpretation. Two named entities mapping to Celestica remain two
legal-name affiliations on one model assertion. Duplicated observations of one
certificate are retained, not counted as additional model assertions; conflicting
observations fail validation before replacing evidence.

Parenthesized company relationships are not inferred. Cases such as
`Keysight Technologies, Inc. (Celestica Electronics (M) Sdn Bhd)` and
`Gigamon Inc. (ACCTON TECHNOLOGY CORPORATION)` remain outside this extraction.
Geographic qualifiers such as `NORTHERN AREA` are not stripped. Some non-target
companion names contain parentheses; those complete segments stay context.
Global company aliases and the default RRA importer are unchanged.

The manufacturer namespace remains separate from the retail brand and applicant.
Applicant matches do not become manufacturer claims. No factory, supplier
contract, hyperscaler deployment, unit ownership, current validity or support
entitlement is inferred. The prior main/derived model parser applies unchanged.
For example, the fullwidth `Ｍ` in two Oracle derived-model cells remains raw;
the importer does not silently repair it into ASCII.

## Local search and SQL

```sh
python3 -m devicehunt import rra-manufacturer-lists data/raw/rra-manufacturer-lists.json
python3 -m devicehunt search M120 --vendor Celestica --role registry_manufacturer
python3 -m devicehunt lookup WS-AP3640-ROW
python3 -m devicehunt lookup DC2ST-3ST
```

```sql
SELECT d.brand,d.model,a.company AS named_manufacturer,
       f.value AS complete_manufacturer_field,r.source_url
FROM records r
JOIN devices d ON d.id=r.device_id
JOIN affiliations a ON a.record_id=r.id AND a.role='registry_manufacturer'
JOIN facts f ON f.record_id=r.id AND f.name='rra_manufacturer_as_printed'
WHERE r.source_id='rra-reviewed-manufacturer-lists'
ORDER BY d.brand,d.model,r.source_url,a.company;
```

The review and added assertions are exported under
`data/exports/rra-manufacturer-lists-*`. The SQL has one row per named
manufacturer affiliation: 47 rows across 45 source records, because two approvals
name two mapped Celestica entities. All 1,081 original RRA rows stay unchanged.
The database now has 3,522 identities, 4,619 assertions and 189 source entries,
including this derived extraction. Manufacturer inventory has 1,856
target/identity pairs across 1,855 identities. The owned baseline is unchanged:
10/10 searchable and 8/10 corroborated, with both Edge-Core gaps still open.

Six new tests verify role isolation, complete original context and provenance,
two legal names on one model, dependency withdrawal, source isolation, duplicate
observations, conflicts, exact-list boundaries and rejection before replacement
when dependencies or expected counts change.

All 326 tests pass. The offline rebuild with 87 jobs and 1,914 hashed inputs
matches the live database across all 37 compared tables/search indexes and the
baseline. The proof check confirms the existing records and evidence are
preserved. All 64 saved lookup reports, three vendor inventories and offline
HTML are refreshed, with the live database byte hash unchanged by exports.
Full historical/FCC/worldwide coverage is still unproven.
