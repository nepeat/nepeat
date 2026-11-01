# Taiwan NCC equipment register

The complete downloaded `vv_IPCCDB.csv` is now local: **110,957 published rows**
with 21 source fields in SQLite table `ncc_equipment`. This is the published
snapshot, not a claim that it contains every historical NCC certification.
The [official dataset page](https://data.gov.tw/dataset/5996) displayed a metadata
update time of September 24, 2026, 23:02; we retrieved the linked CSV on September 26.

Attribution: National Communications Commission (Taiwan), 2026,
**電信設備審驗合格清單** (dataset 5996, `vv_IPCCDB.csv`), published under
[Open Government Data License v1.0](https://data.gov.tw/license).
The dataset metadata capture is a factual web-tool extract, not original HTML.
The separately linked CSV is preserved byte-for-byte with a SHA-256 digest.

## Search locally

```sh
python3 -m devicehunt lookup Celestica --limit -1
python3 -m devicehunt lookup 智邦 --limit -1
python3 -m devicehunt import ncc data/raw/ncc/bundle.json
```

```sql
SELECT certificate_number, brand, model, applicant, manufacturer,
       approval_date_raw, certificate_validity_raw
FROM ncc_equipment
WHERE manufacturer LIKE '%Celestica%'
   OR brand LIKE '%Oracle%';
```

The two Celestica mentions are BlackBerry 8700g and 7290 rows with several
companies in the same manufacturer cell. The complete cell is retained; it
does not establish that Celestica alone made a particular unit. Oracle-branded
rows include MICROS products. Brand, applicant and manufacturer are separate
fields. Chinese company names remain verbatim, without unreviewed translations
becoming aliases. No catalog identity, factory claim or hyperscaler association
is created automatically from this import.

## Snapshot validation and limits

- 110,446 distinct nonempty certificate numbers; 511 repeated certificate rows
  beyond the first occurrence remain separate by source row number.
- 65,056 blank manufacturer cells remain blank.
- 339 rows contain an applicant-confidentiality label in equipment, brand or model.
- Raw validity flags: 110,248 `Y`, 709 `N`. These are dated source claims,
  not live approval checks. Date strings and year suffixes are unconverted.
- UTF-8 BOM, exact ordered headers, 21 cells per row, strict CSV quoting, NUL
  rejection, expected counts and dependency hashes are checked before replacing
  the previously imported source. Embedded newlines and multi-company cells survive.

All fields are available in SQL. The combined `lookup` searches identity,
company and testing fields; it searches eighteen local datasets in total and
reports row counts separately from catalog identities. Duplicate rows count
as separate registry matches. Missing older-schema tables report `not_loaded`.

Tests cover preservation of company roles, blanks, dates, confidentiality,
duplicate certificates, refresh isolation, malformed inputs, hash failures,
read-only lookup and older schemas. This source does not close either remaining
owned-device corroboration gap.

Follow-up: a separate reviewed importer now maps exact Oracle/Accton Chinese
company names using cached company-owned pages and promotes 227 selected NCC
rows with their separate brands and roles. See
[Taiwan target evidence](taiwan-target-evidence.md). The raw CSV importer remains
independent and preserves all rows.
