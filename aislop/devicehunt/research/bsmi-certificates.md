# Taiwan BSMI certificate snapshot

The complete downloaded XML for [official dataset 166852](https://data.gov.tw/dataset/166852)
is available locally in `bsmi_certificates`: **105,732 rows**, each retaining all
seven published fields. The metadata describes new certificates for the current
year, but the downloaded file includes historical expiry dates and several status
values. No issue year or complete historical coverage is inferred. Metadata
displayed September 19, 2025, 11:27; retrieval was September 26, 2026.

Attribution: Bureau of Standards, Metrology and Inspection (Taiwan), 2026,
**驗證登錄新發證書資料集**, dataset 166852, under
[Open Government Data License v1.0](https://data.gov.tw/license).
`data/raw/bsmi/certificates.xml` preserves the original response bytes; the
metadata JSON is explicitly a factual web-tool extract. Both are hashed in the
reviewed import bundle and offline rebuild manifest.

## Useful local queries

```sh
python3 -m devicehunt lookup 美商甲骨文有限公司台灣分公司 --limit -1
python3 -m devicehunt lookup SAF6101I
python3 -m devicehunt import bsmi data/raw/bsmi/bundle.json
```

```sql
SELECT vendor_code, certificate_number, applicant, primary_model,
       product_name, expiry_raw, status_raw
FROM bsmi_certificates
WHERE applicant IN ('美商甲骨文有限公司台灣分公司', '智邦科技股份有限公司')
ORDER BY applicant, primary_model;
```

This selects 41 Oracle applicant rows and one Accton applicant row (`SAF6101I`).
[Oracle's own offices page](https://www.oracle.com/tw/corporate/contact/field-offices/)
pairs the first Chinese name with Oracle Taiwan LLC, Taiwan Branch.
[Accton's own announcement](https://www.accton.com/tw/accton-jun-2026-sales-revenue-report/)
pairs the second with Accton. Those names support applicant associations, not
physical-manufacturer, factory or operator claims.

The audit in `data/exports/bsmi-target-applicant-audit.json` preserves these 42
rows and their exact existing catalog-model matches. Only three rows have an
exact model string already present; the other 39 are research leads, including
annotated strings that may refer to known hardware. No automatic alias stripping
or new catalog identity is performed. For example, `E2ES2U (本型式係Ampere CPU)`
stays a complete source string.

Searching the English word `Oracle` instead finds a coffee-machine product name
under an unrelated applicant. The combined lookup reports this literal text hit
as a registry row; it does not turn it into an Oracle vendor association.

## Preservation and validation

All 105,732 certificate numbers are nonempty and distinct in this snapshot.
Twenty-two primary-model fields are blank and remain blank. Raw status counts:

| Source status | Rows |
|---|---:|
| 認可 | 56,405 |
| 期限到期失效 | 30,088 |
| 註銷 | 12,524 |
| 廢止 | 6,302 |
| 轉換後註銷 | 406 |
| 撤銷 | 7 |

Expiry values and status labels remain dated source claims. No live validity
determination is made. The importer rejects malformed XML, changed field order,
unexpected elements/attributes, nested field contents, DTD/entity declarations,
hash mismatches and unexpected summary counts before replacing stored rows.
Decoded XML field text is preserved without trimming; original XML bytes retain
the source serialization. Row-number keys permit future duplicate certificates.

Schema 15 added this table. Schema 16 adds three separate BSMI registers, bringing
local lookup to eighteen datasets; see [additional source scope](bsmi-additional-registers.md). Tests cover
raw fields, duplicates, refresh isolation, XML rejection, hashes, false vendor
inference and read-only lookup on current and older schemas. Baseline remains
10/10 searchable and 8/10 independently corroborated; neither remaining exact
Edge-Core label appears in this source.

Follow-up: the independently validated `taiwan-targets` importer now promotes
these 42 exact applicant rows into the catalog while retaining applicant roles.
The applicant audit above records the comparison before that promotion. See
[Taiwan target evidence](taiwan-target-evidence.md). The raw BSMI importer itself
continues to preserve only registry rows.
