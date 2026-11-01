# Three additional Taiwan BSMI registers

Downloaded the three original, publicly linked XML files on September 26, 2026.
They are now in the local SQLite database and searched by `lookup`. Each raw
field is retained as text, including leading zeros, whitespace, dates, blanks
and model annotations. These are separate registers: no cross-register merge or
deduplication and no automatic manufacturer, factory or operator claims.

| Official dataset | SQLite table | Rows | Distinct nonempty certificate/registration numbers | Blank models |
|---|---|---:|---:|---:|
| [商品驗證機構資料集, 162936](https://data.gov.tw/dataset/162936) | `bsmi_product_certificates` | 17,797 | 17,797 | 0 |
| [自願性驗證證書資料集, 30176](https://data.gov.tw/dataset/30176) | `bsmi_voluntary_certificates` | 1,818 | 1,818 | 3 |
| [符合性聲明產品登記資料集, 169189](https://data.gov.tw/dataset/169189) | `bsmi_declarations` | 564 | 214 | 9 |

The declaration register has two blank registration numbers and 348 repeated-number
rows beyond the first occurrence. Registration number alone is not a row key.
The key is `(source_id, row_number)`. Certificate and registration counts are not
physical-device counts. This adds **20,179 raw rows**, not 20,179 target models.

All three scans found zero literal case-insensitive matches for Accton, Edge-Core,
Celestica, Oracle, Amazon, 智邦 or 甲骨文, and zero matches for the ten complete
owned-hardware labels. The exact queries and results are in
[`bsmi-additional-coverage-audit.json`](../data/exports/bsmi-additional-coverage-audit.json).
That is a finding about these snapshots and these terms; it does not rule out
other legal names, subsidiaries, spelling variants or historical records. No new
target identities or validation passes were inferred.

## Search and SQL

`python3 -m devicehunt lookup Oracle --limit -1` now searches 18 datasets,
including these three. For direct SQL, for example:

```sql
SELECT certificate_number, applicant, product_name, primary_model,
       expiry_raw, status_raw
FROM bsmi_product_certificates
WHERE primary_model LIKE '%switch%' OR product_name LIKE '%交換器%';

SELECT registration_number, company_name, business_number,
       product_name, primary_model, application_date_raw
FROM bsmi_declarations
WHERE company_name LIKE '%智邦%' OR primary_model LIKE '%AS7772%';

SELECT registration_number, count(*) AS rows
FROM bsmi_declarations
GROUP BY registration_number
HAVING count(*) > 1;
```

The first two examples may return zero rows. Applicant/registered-company columns
are not manufacturer columns. `status_raw` exists only in the product-certificate
table; the other two registers do not publish that column. Their missing status
must not be interpreted as valid. Dates remain raw strings; no conversion or
current-validity determination is made.

## Sources, capture and reproduction

- Product certificate XML: [original download](https://data.bsmi.gov.tw/opendata/download/313000000G-000101-001.action).
- Voluntary certificate XML: [original download](https://data.bsmi.gov.tw/opendata/download/313000000G-000086-001.action).
- Declaration XML: [original download](https://data.bsmi.gov.tw/opendata/download/313000000G-000162-001.action).

Bureau of Standards, Metrology and Inspection (Taiwan), 2026. The government
catalog lists [Open Government Data License v1.0](https://data.gov.tw/license).
Attribution is also stored in each SQLite source record and bundle metadata.

The three government metadata pages were read using the web tool. Their local
JSON captures are factual metadata extracts, **not original HTML**. Displayed
metadata update times are June 23, 2026 16:21 (162936), July 25, 2023 14:28 (30176)
and June 23, 2026 16:15 (169189). These do not establish a file's effective date.
Only completeness relative to each downloaded XML is checked; an independently
verified all-time BSMI register is not claimed.

Snapshots and bundles are under `data/raw/bsmi-additional/<dataset-id>/`.
`bsmi-additional` imports one hashed bundle at a time, with strict dataset-specific
field order, dependency hashes, metadata associations and reviewed counts checked
before replacing that source. The original BSMI importer now shares its strict XML
parser with these three fixed field schemas. Unknown fields, malformed XML and
DTD/entity declarations fail rather than silently dropping data.

All fields of every imported row were compared with a separate XML traversal.
See [`bsmi-additional-import-check.json`](../data/exports/bsmi-additional-import-check.json).
Three regression tests cover all schemas, source replacement, malformed/hash/count
failures, duplicates and empty fields, read-only lookup, older databases and
misleading product-name matches. The full suite has 250 passing tests. Schema 16
adds these three tables; the rebuild manifest includes all nine new local files.

## Follow-up: verified Edgecore legal name

The later [Taiwan company-name mapping](taiwan-target-evidence.md) verifies
鈺登科技股份有限公司 as Edgecore Networks Corporation using the official Hsinchu
Science Park company record. This name was not in the seven-term scan above.
It finds one applicant row in dataset 162936, for RD1201000-C47-2MG; it is now
promoted as an applicant association, not a manufacturer assertion. The other
two supplementary registers contain no literal match for that name. The original
negative audit remains a record of its explicitly stated query terms.
