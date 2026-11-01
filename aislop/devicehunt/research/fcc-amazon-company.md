# Amazon FCC company-table supplement

The [FCC.report Amazon FCCID company record](https://fcc.report/company/amazon-data-services-inc-2)
shows FRN `0029198181` and these two rows, reviewed 2026-09-27:

| Listed date | FCC ID | Model cell | Purpose | Frequency text |
|---|---|---|---|---|
| 2022-01-27 | 2AVPL-CUBSCOUT | CUBSCOUT | Original Equipment | 13.553 - 13.567 MHz |
| 2020-08-11 | 2AVPL-TE1A001 | TE1A001 | Original Equipment | 2402 - 2480 MHz |

Saved selected factual web-tool text, not original HTML; no original download
was attempted. The page reports refresh `2026-07-29 21:23:42`. The extract and
bundle are in `data/raw/fcc-company-amazon/`, with hashes in the rebuild manifest.
Dates, Model cells and frequencies remain mirror claims. No complete historical
filing inventory, models, manufacturer, operator, applications or PDFs are added.

The same-name records linked by this page have other FRNs and ULS license rows.
Those were not merged into the FCCID identity or imported as hardware. The
importer validates the exact company, FRN, URL, grantee prefix and one-page shape
before source-scoped replacement. Tests reject the ULS FRNs and wrong page URL,
and preserve the separate Accton capture and original FCCID.co detail collector.

The raw company table now has 237 rows; the combined discovery index has 344
source rows covering 210 literal IDs. CubScout newly gains index coverage; its
existing application evidence is unchanged. Of 271 locally known FCC strings,
61 remain missing from the index. No full archival completeness is claimed.

```sh
python3 -m devicehunt lookup 2AVPL-CUBSCOUT --format json
python3 -m devicehunt sql "SELECT * FROM fcc_company_rows WHERE source_id='fcc-company-amazon' ORDER BY row_number"
```
