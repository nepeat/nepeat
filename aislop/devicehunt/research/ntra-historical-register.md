# Historical NTRA register, December 4, 2022

The [original NTRA PDF](https://www.tra.gov.eg/wp-content/uploads/2022/12/Type-Approved-List-December-2022-%D9%90.pdf)
is cached with SHA-256 `da373d0fdd2f804f10e14570771896e4fbd4aa6ef582aa740cb06b7cc276dc3c`.
Its 1,218 pages contain 29,208 numbered equipment rows. All pages were extracted
positionally using pdfplumber. Exactly one three-column table occurs on each
page, the first header is `No / TYPE / BRAND & MODEL`, and sequence numbers
are contiguous from 1 through 29208. All rows have readable nonempty cells;
no glyph markers were detected. This is an extraction check, not proof of
underlying registry completeness. Pages 1, 435, 460 and 1218 were visually reviewed.

Unlike the newer register, brand and model share a single source cell. The
`ntra-historical` importer keeps this cell unsplit in `cells_json`, with
`parse_status = 'combined_brand_model'`. `registry_number` and
`product_description` are populated; `brand`, `model` and `linked_models` are
NULL. No invented boundary, alias, current approval status, manufacturer or
operator assertion is produced. The document date is printed on page 1;
sequence numbers are not approval IDs.

The snapshot uses `source_id = 'ntra-register-2022-12'`, separate from
`ntra-register-2026-01`. Both contribute to `ntra` and `lookup`. Their combined
76,510 rows are observations across dates, not 76,510 unique devices. The full
PDF is also archived as `reference-pdf-ntra-2022-12`, with embedded text on all
1,218 pages and no OCR.

## Historical coverage comparison

A literal prefix screen finds 146 rows beginning with Accton, Edge-Core,
Celestica, Oracle, Amazon Data Services or AWS. Normalizing whitespace and case,
135 combined cells exactly match a newer brand-plus-model pair. Eleven do not.
Ten of those eleven contain a trailing identifier now present in the newer
register's linked-model column. These are comparison candidates, not automatic
aliases or claims that a device disappeared. The remaining old cell is
`ORACLE SUN SG-XPCIE2FC-QF8-Z`, page 315, row 7536.

[Oracle's ZFS Storage 7420 service documentation](https://docs.oracle.com/en/storage/zfs-storage/zfs-appliance/os8-8-x/service-manual/sun-zfs-storage-7420-pcie-io-cards.html)
separately lists that marketing part number as a two-port 8 Gb Fibre Channel
HBA. This is a primary-source lead for a later component import, not a new
manufacturer assertion in this run. The original Oracle page has not yet been
captured locally. Saved comparison details, including newer candidate rows,
are in `data/exports/ntra-historical-target-audit.json`.

The older source contains AS7760-32X, not the complete 7760-32X-A-AC-F ordering
code. Neither unresolved baseline label receives new corroboration.

## Use and reproduce

```sh
python3 -m devicehunt ntra SG-XPCIE2FC-QF8-Z
python3 -m devicehunt lookup SG-XPCIE2FC-QF8-Z
python3 -m devicehunt documents --source reference-pdf-ntra-2022-12 --limit -1
python3 -m devicehunt import ntra-historical data/raw/ntra-2022-12-bundle.json
```

```sql
SELECT registry_number, product_description,
       json_extract(cells_json, '$[2]') AS published_brand_and_model,
       page, source_url
FROM ntra_equipment
WHERE source_id = 'ntra-register-2022-12'
  AND json_extract(cells_json, '$[2]') LIKE 'ORACLE%'
ORDER BY CAST(registry_number AS INTEGER);
```

`devicehunt.ntra_historical.extract(pdf_path, tables_path)` reproduces the
positional capture from the reviewed PDF. The offline bundle validates the PDF
and table hashes, page order, table shape, header, row sequence and counts before
replacing this source's rows. Other snapshots remain intact. The rebuild
manifest also hashes the original PDF, embedded text and reference bundle.
