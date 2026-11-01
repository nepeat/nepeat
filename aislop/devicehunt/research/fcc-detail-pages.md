# FCC mirror filing-detail snapshot

All 171 IDs in the reviewed FCCID.co company-index snapshot now have cached
detail HTML. Downloads were sequential, paused between requests, reused the two
already captured pages, and recorded errors without retries or access bypasses.
All 171 succeeded. Original HTML occupies about 25 MiB under
`data/raw/fcc-details/`; the fetch report records URLs, hashes and check times.

The snapshot contributes 1,493 public exhibit links and 1,092 mirror-supplied
text excerpts. Of those excerpts, 311 carry an explicit truncation notice.
An unflagged excerpt is not proof that a whole document was extracted. These
excerpts are distinct from the eleven original PDF exhibits already archived
for TE1A001. No other linked file was downloaded in this expansion.

## Scope and data model

- `fcc_detail_pages`: one captured detail page per indexed FCC ID. Original
  display fields remain JSON in `fields_json`, including dates and frequency
  strings. They are not normalized into verified grant dates or inferred units.
- `fcc_detail_links`: public links, category and title, keyed by filing and
  stable page order. Modern links sometimes have punctuation and no extension;
  they are preserved without inventing document IDs.
- `fcc_text_excerpts`: category, text and explicit truncation flag, keyed by
  filing and stable page order. Multiple excerpts with the same category remain
  separate. Their order is not used to guess a PDF link, document ID or page.
- `fcc_excerpt_text`: SQL view joining excerpts to grantee and source-page data.

These are mirror page records, not an enumeration of every application for an
FCC ID. The underlying company index is known to omit filings. No detail pages
were discovered for MRF/NVN because those company searches were not present in
the mirror index; they remain coverage gaps. No dates from this mirror replace
reviewed grant dates. FCC ID product codes do not become catalog model aliases.
No new model assertions, manufacturers or deployment relationships are inferred.

## Local queries

```sh
python3 -m devicehunt lookup 'EAP115' --limit 5
python3 -m devicehunt lookup 'MICROS' --format json
python3 -m devicehunt sql "SELECT fcc_id,grantee_name,json_extract(fields_json,'$.Equipment Class') AS equipment_class FROM fcc_detail_pages"
python3 -m devicehunt sql "SELECT fcc_id,category,truncated,text FROM fcc_text_excerpts WHERE text LIKE '%EAP115%'"
python3 -m devicehunt sql "SELECT category,title,document_url FROM fcc_detail_links WHERE fcc_id='HEDEAP115' ORDER BY ordinal"
```

The general lookup now searches eleven datasets. FCC excerpt results provide a
bounded snippet around the first literal text match (or the opening text for a
metadata-only match). The complete cached excerpt is retained for SQL. Truncated
mirror text and the shortened UI snippet are different limitations. Counts are
per excerpt, not per unique document, model or device.

`data/exports/fcc-detail-summary.csv`, `fcc-exhibit-links.csv` and
`lookup-fcc-detail-example.json` are queryable/exported examples. None establishes
current authorization validity, product availability, physical OEM or the history
of an auction unit. Review the original PDF before relying on technical claims.

## Reproduction and refresh

The `fcc-details` importer requires the company-index source to be imported first.
Its bundle must cover exactly that local index, with matching grantee and URL
fields. Every HTML hash and parsed page is checked before source replacement.
Missing or blocked pages can be represented explicitly as unavailable rather
than silently dropped. The source includes all attempted rows in its count.
Refresh deletes old linked/excerpt rows through foreign-key cascades.

`research/fetch_fcc_details.py` is a resumable public-page collector. Existing
files are reused, so running it is not a freshness check on existing content.
It writes a fetch report; the separately reviewed `bundle.json` is the import
input. Only the latter can affect the database. The offline manifest includes
all 171 HTML files, bundle and fetch report, and rebuilds without network access.

Tests cover scope/checksum validation before mutation, wrong identities/company
fields, changed truncation notes, duplicate fields, excerpt refresh cleanup,
unavailable-page reporting, duplicate categories, literal Unicode snippets,
limits and the separation from catalog identities and archived PDFs.
