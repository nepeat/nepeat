# Public FCCID.co sitemap snapshot, 2026-09-27

The original public `https://fccid.co/sitemap.xml` and its advertised child
files are archived in `data/raw/fcc-sitemaps/`. The saved robots file advertises
this index. Collection used ordinary sequential public requests, reused verified
captures, and recorded errors without retrying them. No internal API or paid
provider was used. Datadory is excluded at the user's request.

The index advertised 48 children. 38 returned valid XML and ten (1980–1989)
returned HTTP 400. The index did not advertise 2017 or 2018. Small yearly lists
(e.g. 16 URLs for 2019) are another reason not to infer completeness.

| Captured URL kind | Rows / distinct URLs |
|---|---:|
| FCC detail discovery | 190,198 |
| Company | 41,637 |
| Equipment class | 123 |
| Other navigation | 4 |
| Total | 231,962 |

These are raw discovery URLs, not imported device models, filings, grants or
exhibits. `lastmod_raw` is website metadata, not a grant date. All original URL
rows are retained; the importer also preserves duplicates if future captures
contain them. Missing captures have NULL row counts, distinct from valid empty
files. The import adds no manufacturer, grantee or hyperscaler assertion.

Of 271 previously known target-associated FCC identifier strings, 205 occur in
this snapshot and 66 do not. Literal prefix leads for the six previously reviewed
grantee codes contain 146 HED, 20 YZK, 4 A4H, 1 2AVPL, 0 MRF and 0 NVN URLs.
Those 171 leads overlap the original company-query collection; the larger global
snapshot has not resolved the missing target filings. Other known IDs can use
other grantee codes. Prefix matching does not itself verify the grantee.

`fcc-coverage` intentionally continues to report the previously collected target
index/detail/application/PDF stages. It does not silently expand its denominator
to every raw sitemap URL. Use the separate sitemap summary and SQL below.

```sql
SELECT page_kind, count(*) AS rows
FROM fcc_sitemap_urls GROUP BY page_kind;

SELECT sitemap_url, capture_state, issue, row_count
FROM fcc_sitemap_pages ORDER BY sitemap_url;

SELECT fcc_id, record_url, sitemap_url, lastmod_raw
FROM fcc_sitemap_urls WHERE fcc_id LIKE 'HED%' ORDER BY fcc_id;
```

`python3 -m devicehunt lookup HEDML2560` includes the separate `fcc_sitemaps`
section. Read-only reports live at `data/exports/fcc-sitemap-summary.json`,
`fcc-sitemap-pages.csv` and `fcc-sitemap-prefix-leads.csv`.

The bundle checks all dependencies and advertised child accounting before a
transactional source replacement. Importer tests cover malformed XML, changed
hashes/counts, missing children, unavailable versus empty pages, repeated URLs,
refresh withdrawal and rollback if a dependency changes during insertion.
The offline rebuild manifest includes all original XML and capture reports.
