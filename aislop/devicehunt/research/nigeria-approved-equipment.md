# Nigeria NCC approved equipment

The official [approved-equipment page](https://www.ncc.gov.ng/page/approved-equipment)
links a [September 2026 CSV](https://www.ncc.gov.ng/sites/default/files/2026-09/type_approval_communications_equipment_3.csv).
The cached CSV has 8,517 rows and six columns: SN, APPLICANT, CERTIFICATE HOLDER,
EQUIPMENT NAME, MODELS and MANUFACTURER. Every row agrees with two downloaded
HTML tables after display-whitespace normalization. Original HTML/CSV files and
checksums are retained under `data/raw/nigeria-ncc/`.

This covers the complete linked file, not independently proven completeness of
the regulator's historical or live database. There are no certificate IDs,
approval dates or validity fields in this export. SN is a row serial, never a
certificate reference. Applicant, certificate holder and manufacturer remain
separate claims. Nigeria NCC is distinct from the Taiwanese NCC dataset already
in the catalog.

## Local search and SQL

```sh
python3 -m devicehunt import nigeria-ncc data/raw/nigeria-ncc/bundle.json
python3 -m devicehunt lookup ATLANTA23SFX
python3 -m devicehunt lookup ML1-60-35
python3 -m devicehunt sql "SELECT models,manufacturer,equipment_name,applicant,certificate_holder FROM nigeria_equipment WHERE manufacturer LIKE '%AMAZON DATA%' ORDER BY row_number"
python3 -m devicehunt sql "SELECT row_number,equipment_name,invalid_cell_bytes_json FROM nigeria_equipment WHERE invalid_cell_bytes_json <> '{}'"
```

All rows are stored in `nigeria_equipment`, preserving the six CSV cells,
whitespace, duplicates, source URL and extraction issues. `lookup` searches
these independently of whether a catalog identity was asserted.

Only a whole, previously reviewed target-company manufacturer name and a simple
individual model label create a catalog assertion. Seven source rows qualify:

| Target | Models as printed |
|---|---|
| Amazon Data Services | SCOUT02, ATLANTA23SF, ATLANTA23SFX, SCOUT01 |
| Oracle | X8-2L, ZS7-2 |
| Edge-Core | AS7315-27x |

Manufacturer claims do not establish a physical factory or hyperscaler use.
The two Oracle rows' equipment descriptions mention a `wifi module PU2SL`; that
text stays a source fact, not an inferred model alias or hardware configuration.
`Accton Tech. Corporation` appears in three MetroLinq rows but remains an
unreviewed company spelling. Those rows are searchable raw evidence, without
silently extending the shared Accton aliases. Applicant/holder matches alone
cannot turn another company's hardware into a target manufacturer claim.

## Text damage and verification

Most CSV cells are UTF-8. Five contain invalid UTF-8 bytes. The official web page
already displays replacement characters in those cells. Import decoding follows
that display and records each affected original cell's bytes as base64 under
its zero-based column number in `invalid_cell_bytes_json`. No guessed legacy
encoding or source-text repair is used. Other pre-existing garbled source text
and scientific-notation model labels are retained verbatim. The original CSV
remains the byte-authoritative artifact.

Rows with any decoding error never supply catalog identities. Grouped or
annotated model cells stay raw rather than being shortened or split. Failed
hash, heading, numbering, linked-URL, count or content checks stop before source
replacement. A refresh replaces only this source and removes withdrawn claims.

## Imported snapshot result

The seven assertions add four catalog identities under the source's manufacturer
namespaces: Amazon Data Services SCOUT01/SCOUT02 and Oracle X8-2L/ZS7-2. These
are not four proven new physical models: AWS-branded records and longer Oracle
names remain separate. ATLANTA23SF, ATLANTA23SFX and AS7315-27x already had
catalog identities; this source adds independent manufacturer evidence.

Total catalog: 3,404 identities, 4,489 assertions, 187 sources. Manufacturer scope
has 1,734 target/identity pairs across 1,733 identities. All 314 code tests pass.
The ten-device owned-hardware baseline remains 10/10 searchable, 8/10
corroborated. Neither outstanding exact label occurs in this register.

Full register, target roles, text issues and the executed Amazon SQL query are
exported as `data/exports/nigeria-*.csv` and `.json`. The byte hash stayed
unchanged during these read-only exports; see `nigeria-sql-check.json`.

Offline reproduction passed: all 36 compared tables/search indexes and baseline
results match `data/rebuilt-with-nigeria.sqlite`, using 85 import jobs and 1,909
hashed inputs. All 56 saved lookup reports now cover 24 datasets. Refreshed
inventories and offline HTML preserve the live database's byte hash.
