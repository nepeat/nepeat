# Ghana NCA public equipment table

The [official NCA public search page](https://portal.nca.org.gh/search_type_approval.php)
is linked from the regulator's [type-approval FAQ](https://nca.org.gh/faqs/).
Its original HTML contains 1,153 equipment rows. A second download agrees on
all visible fields, row order and detail links. The page initializes a local
DataTables table; no script was executed to extract the records.

The table's columns are Equipment Type Number, Name of Manufacture, Brand Name,
Product Type, Model Number and Details. All rows and their public detail links
are retained in `ghana_equipment`. Manufacturer and brand remain separate.
Thirty-six approval-number cells are blank. Their distinct detail IDs remain
available, but are not promoted to approval/certificate numbers. Duplicate model
strings remain separate registry rows.

## Coverage limits

This is the entire table returned by the public page, not proof of a complete
current or historical NCA database. No independent total or authoritative
publication/update date is exposed. Accessing the page in September 2026 does
not establish that its approvals are current. Approval numbers are not decoded
to invent dates or equipment characteristics.

One row has an exact target manufacturer: Oracle America Inc., with brand Oracle,
model `Oracle's Netra Sparc T3-1`, equipment type `Netra SPARC Computer Server
(NMS)` and approval number `5N4-3H-7DF-22F`. These strings remain as printed; no
short-model alias or equivalence to another catalog identity is inferred.
The [linked detail page](https://portal.nca.org.gh/search_type_approval_view_details.php?typeApproveDetailID=709)
returned HTTP 500. That failed access is recorded separately in
`data/raw/ghana-nca/detail-access.json`. No detail-page text or certificate is
claimed as retrieved. Other detail pages were not fetched.

No other target manufacturer is mapped in this table. The two outstanding
Edge-Core baseline labels are absent. Non-target rows remain useful for local
label and approval-number searches; they do not automatically establish OEM
relationships to the five target companies. Manufacturer-register claims do
not establish factories, hyperscaler deployment or provenance of an auction unit.

## Local commands

```sh
python3 -m devicehunt import ghana-nca data/raw/ghana-nca/bundle.json
python3 -m devicehunt lookup 5N4-3H-7DF-22F
python3 -m devicehunt lookup QFX5100-96S
python3 -m devicehunt sql "SELECT approval_number,manufacturer,brand,product_type,model,detail_url FROM ghana_equipment WHERE manufacturer LIKE '%Oracle%' ORDER BY row_number"
python3 -m devicehunt sql "SELECT row_number,detail_id,manufacturer,brand,model,detail_url FROM ghana_equipment WHERE approval_number='' ORDER BY row_number"
```

The original HTML pair and bundle are under `data/raw/ghana-nca/`. Full-table,
Oracle-query and missing-approval-number exports are under `data/exports/ghana-*`
as CSV and JSON. The documented SQL was executed; read-only exports preserve
live database bytes (`ghana-sql-check.json`).

All 318 tests pass. New tests cover nested layout tables, headings, detail link
validation, duplicate link rejection, blank approval numbers, whole manufacturer
matching, manufacturer/brand separation, duplicate models, source withdrawal,
isolated source refresh and invalid snapshots rejected before mutation.
Lookup now searches 25 datasets. Catalog totals are 3,405 identities, 4,490
assertions and 188 sources. The one added identity is the full printed Oracle
label, not a proven new physical model. Baseline remains 10/10 searchable and
8/10 corroborated. Full FCC and worldwide device coverage remain unproven.

The fresh offline rebuild reproduces all 37 compared tables/search indexes and
the baseline from 86 import jobs and 1,913 hashed files. All 58 saved lookup
reports now include 25 datasets; inventories and offline HTML are refreshed.
`ghana-readonly-check.json` verifies that exporting did not change database bytes.
