# Canadian ISED radio equipment capture

Reviewed 2026-09-26. Five company-name queries now have validated snapshots.
This is not a copy of Canada's full radio equipment database.

[ISED's REL description](https://ised-isde.canada.ca/site/certification-engineering-bureau/en/wireless-program/radio-equipment-list-rel)
links to the [public search form](https://sms-sgs.ic.gc.ca/equipmentSearch/searchRadioEquipments?lang=en_CA).
The form works with an ordinary in-memory session cookie. No account or private
application is used. A submitted search and its CSV export each have a public
processing/refresh step; the prepared export exposes a `downloadCSV` link.

## Imported evidence

Company group: **25869 — Amazon Data Services, Inc.** Four items were reported,
all four were displayed, and the CSV contains four corresponding rows.

| Hardware version (HVIN) | Certification number | Source approval date | Firmware version (FVIN) |
|---|---|---|---|
| CubScout | 25869-CUBSCOUT | 2022-11-21 | blank |
| TE1A105 | 25869-TE1A001 | 2023-07-10 | 1.3.1 |
| TE1A195 | 25869-TE1A001 | 2023-07-10 | 1.3.1 |
| TE1A001 | 25869-TE1A001 | 2020-12-10 | 1.3.1 |

The last three share a certification number; their hardware identifiers remain
separate. Company is stored as `regulatory_company`, not physical manufacturer
or deployment proof. PMN and HMN are facts, not exact-model aliases. Firmware
values describe the source entry, not the configuration of a surplus unit.

The CSV has **44 columns and no header**. Only positions independently matched
to the labelled HTML fields are interpreted: company number/name, HVIN, PMN,
HMN, FVIN, certification number and approval timestamp. All columns are retained
as text in `ised_equipment.cells_json`, as well as matched catalog records;
radio metrics and unlabelled fields are not
assigned speculative meanings. Original timestamp offsets are preserved.

## Reproduction and limits

```sh
# Network capture into a NEW directory. No cookies are saved to disk.
python3 research/capture_ised.py 'Amazon Data Services' data/raw/ised-next
# Offline validation and import; fails before replacing evidence if incomplete.
python3 -m devicehunt import ised data/raw/ised-next/capture.json
python3 -m devicehunt search TE1A195 --vendor 'Amazon Data Services'
```

The reviewed capture is `data/raw/ised-amazon-export/capture.json`, with all seven
responses hashed. The original version-1 importer requires a completed capture,
one exact target company group, the full query count on the displayed page,
identical search and prepared-export tables, and matching CSV identities/counts.
This remains the validation path for the original Amazon capture. A query that
returns no usable result page is **not** treated as a zero.

The collector now writes version-2 captures, follows displayed Next links and
records every selected page. The corresponding importer checks contiguous page
ranges, each table's row count, stable query totals, company groups and repeated
page consistency after export. Missing pages, overlap, changed totals, unexpected
CSV layouts and corrupt dependencies fail before replacing catalog evidence.
An explicit zero on the REL result page is retained as a query observation.
There is no automatic all-company sweep or full-database completeness claim.

Earlier probe directories contain processing pages and incomplete exports.
They are excluded from the rebuild manifest and catalog evidence. The complete
capture contributes four assertions and two new identities (TE1A105, TE1A195).
The ten-device inventory remains **10/10 searchable, 8/10 corroborated**.

## Expanded queries and source discrepancies

| Query | Labelled entries | CSV rows | Imported target assertions | HTML entries without exact CSV match |
|---|---:|---:|---:|---:|
| Amazon Data Services | 4 | 4 | 4 | 0 |
| Accton | 167 | 579 | 162 | 15 |
| Edgecore | 4 | 29 | 4 | 0 |
| Oracle | 12 | 16 | 12 | 0 |
| Celestica | 0 | 0 | 0 | 0 |

Accton spans nine pages. Its other five entries belong to **Accton Wireless
Broadband Corp.**, which is retained in cached results but excluded from the
Accton Technology target group. Each RF configuration produces a CSV row, so
CSV row counts are not device counts. The importer preserves the labelled HTML
identity and attaches all CSV rows that agree exactly on company name, HVIN,
PMN, HMN, FVIN, certification number and approval date. Fifteen Accton HTML rows
have no such match; six CSV rows match no HTML identity. These discrepancies are
reported, not silently repaired. The full CSV remains cached. Matching raw CSV
rows are also stored in each catalog assertion's `raw_json`. Every CSV row is
now independently available in `ised_equipment`, including the six unmatched
Accton rows and rows from non-target company groups.

ISED's grouping header combines the Oracle name under number `1894B`, but the
CSV identifies most Oracle rows with company number `9870A`. Thus the grouping
header number is stored separately; per-row company numbers are supplied only
by exact CSV matches. Certification strings remain literal, including old
formats lacking a hyphen and an empty Accton certificate cell. No guessed
certificate prefix is inserted. Product and host marketing names remain facts,
not aliases that could satisfy exact-model validation.

Edgecore's four entries describe EAP101 and EAP102 with separate approvals and
firmware fields. Oracle's twelve include SunSPOT, MICROS products and Bump Bar
K3-10W/K3-20W. These are public regulatory company associations, not physical OEM
or hyperscaler deployment assertions. Celestica's zero applies only to this
name query. The hyphenated `Edge-Core` probe returned a form error reading
"No results found" rather than an explicit zero result page; it is not imported.
The unhyphenated spelling returned the four checked entries above.

Validated expanded captures:

- `data/raw/ised-accton-paged/capture.json`
- `data/raw/ised-edgecore-paged/capture.json`
- `data/raw/ised-oracle/capture-paged.json`
- `data/raw/ised-celestica/capture-paged.json`

The last two are version-2 annotations of the already downloaded complete
single-page captures, with unchanged original response bytes and hashes. Query
sources use the `ised-rel-query-` prefix; the original Amazon source retains
`ised-rel-25869`. All are included in the offline rebuild manifest.

## Complete raw query exports in SQL

Schema 21 adds `ised_equipment`, containing all **628 raw CSV rows** from these
captures: 579 Accton-query, 29 Edgecore, 16 Oracle and 4 Amazon. Celestica's
verified zero query remains visible in lookup source metadata without inventing
a device row. These counts describe query-export radio configurations; they
must not be added together as unique devices or presented as a full REL mirror.

```sh
python3 -m devicehunt lookup 'MN-500'
python3 -m devicehunt lookup 'SMC2662W V.2'
```

```sql
SELECT company_number, company_name, hvin, certification_number,
       approval_timestamp_raw, source_id, row_number
FROM ised_equipment
WHERE matches_html_identity = 0
ORDER BY source_id, row_number;

SELECT source_id, count(*) AS csv_rows,
       sum(matches_html_identity = 0) AS unmatched_rows
FROM ised_equipment GROUP BY source_id;
```

`matches_html_identity` means the CSV identity agrees exactly with a labelled
HTML identity using the existing comparison, including company name, HVIN,
PMN, HMN, FVIN, certificate and approval date. It does not mean manufacturer
verification or current qualification. The six unmatched Accton rows name
MN-500, MN-510, MN-520, SMC2662W V.2, SMC2664W and WN3501C. Their certificate
cells have trailing spaces, retained verbatim; no trimming or reconciliation
is applied to force an exact match. The fifteen HTML-only identities remain
catalog records with an explicit discrepancy fact.

Raw queries search all 44 cells, including uninterpreted fields, while returning
the independently mapped identity fields and match status. Duplicate rows,
empty values, old certificate formats, company numbers and timestamp offsets
remain intact. Raw-only rows and component/marketing-name mentions cannot
satisfy the owned-inventory validator or create catalog aliases. Import refresh
replaces only that source's raw rows, and malformed or incomplete captures are
rejected before replacement. No new network captures were needed for this
expansion: the already hashed original exports are the evidence.

Exports: `data/exports/ised-raw-register.csv` and `.json`,
`ised-unmatched-csv-rows.csv` and `.json`, and `ised-raw-import-audit.json`.
The existing offline rebuild manifest now checks the raw row total too.

## Public bulk-download scope investigation

ISED's [Site Data Tool page](https://ised-isde.canada.ca/site/certification-engineering-bureau/en/node/141)
offers two historical workbooks, with data through February 2023. Its
[official guide](https://ised-isde.canada.ca/site/certification-engineering-bureau/sites/default/files/attachments/2023/Instructions%20on%20how%20to%20use%20the%20Site%20Data%20Tool.pdf)
explains that old SMS station records were matched against REL entries. These
are licensed-site data, **not a full REL export**. Original workbook inspection
confirms a `Site_Data_Extract` sheet with LICENSEE and transmitter fields, plus
a small `TX_model` reference sheet and lookup formulas. No formulas were run,
and no station/licensee was imported as a hardware manufacturer or device.
Downloads and a hashed scope review are retained in `data/raw/ised-site-data/`
outside the equipment rebuild manifest. A complete current REL bulk export
remains unverified.
