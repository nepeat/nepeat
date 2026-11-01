# Accton company-table discovery supplement

The [FCC.report Accton company index](https://fcc.report/company/Accton-Technology-Corp)
exposed three pages with 100, 100 and 35 rows. Selected factual web-tool extracts
retain all 235 displayed rows, covering 169 literal FCC IDs. Original HTML
download returned HTTP 403 and was not retried. The captures are not original
HTML archives; they may reflect different web-cache ages. All pages report
refresh time `2026-07-29 21:35:46`. Listed dates span 1993–2026 and remain mirror
claims, not independently verified grant dates.

This adds 36 strings absent from the previous index, of which 27 were absent
from the wider local FCC coverage report. The combined index now has 342 source
rows and 209 distinct literal IDs. Punctuation variants remain distinct pending
evidence. The catalog still has 3,533 identities and 4,653 assertions: these
discovery rows create no models, aliases, manufacturer or operator claims.

`fcc_company_rows` retains duplicate rows, dates, the literal Model column,
application purpose, frequency text, page URL, page/row order and web-text line.
The company FRN is page-level metadata, not assigned to each filing. Blank
frequency cells and the unusual `5.18 - 5.32 MHz` string remain unchanged.
The discovery index uses the first displayed row per literal ID and labels the
Model cell as unverified. Its source URL points to the actual company page;
no detail URLs are guessed. All raw rows remain available for SQL queries.

```sh
python3 -m devicehunt lookup HEDML2560
python3 -m devicehunt sql "SELECT fcc_id, listed_date, model_cell, application_purpose, frequency_raw, source_url FROM fcc_company_rows WHERE fcc_id = 'HEDML2560' ORDER BY row_number"
python3 -m devicehunt sql "SELECT count(*) AS source_rows, count(DISTINCT fcc_id) AS literal_ids FROM fcc_id_index"
python3 -m devicehunt import fcc-company data/raw/fcc-company-accton/bundle.json
```

The importer validates all three pages, dependency hashes, company identity,
pagination, dates and source-line ordering before replacing its own source rows.
It preserves duplicate filings and isolates the original FCCID.co detail-page
collector, which still selects only its 171 original entries. Tests cover scoped
refresh and withdrawal, truncation, changed metadata, corrupt hashes and lookup.
The rebuild manifest includes the bundle, three page extracts and failed-download
log. No complete official FCC archive or historical completeness is claimed.

Owned validation remains 10/10 searchable and 9/10 corroborated. The corrected
`AZ4444-52X-DC-10` passes against existing Anatel evidence; only the exact
`7760-32X-A-AC-F` corroboration gap remains.
