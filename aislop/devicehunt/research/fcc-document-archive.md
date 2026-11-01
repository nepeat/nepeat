# Local FCC exhibit archive

The first archive contains all eleven publicly linked PDFs on the cached
[FCCID.co TE1A001 detail page](https://fccid.co/fcc/2AVPL-TE1A001). These are
mirror copies, not a full FCC database download. The existing filing manifest
also lists five unavailable exhibits; this archive does not supply those.

Original PDFs, cached detail HTML and page-text captures are under
`data/raw/fcc-documents/`. Each bundle dependency is SHA-256 checked before any
source replacement. Captures identify the PDF hash, extraction method and
consecutive page numbers. The offline rebuild manifest hashes every dependency.
Text extraction uses pypdf embedded text, without OCR. The eleven PDFs contain
74 pages; 68 contain non-whitespace text. Six pages have no embedded text and
remain stored with `no_embedded_text`, including the label and photographs.

```sh
python3 -m devicehunt documents antenna
python3 -m devicehunt documents --fcc-id 2AVPL-TE1A001 --limit -1
python3 -m devicehunt documents 'TE1A001' --format json
python3 -m devicehunt lookup '2AVPL-TE1A001' --limit 5
python3 -m devicehunt sql "SELECT document_id,page_count,pages_with_text FROM documents"
python3 -m devicehunt sql "SELECT document_id,page_number,text FROM document_text WHERE text LIKE '%antenna%'"
```

`documents` searches page text with ANDed prefix tokens and returns excerpts,
one-based page numbers and absolute local PDF paths. Empty query lists pages,
including blank-text pages. `--fcc-id` filters exactly, ignoring case. `--limit`
is 20 by default; 0 returns counts and -1 returns all rows. The general `lookup`
also searches document metadata, so a metadata hit may include a page with no
text. Both commands read an existing database without modifying it.

Tables are `documents` and `document_pages`; `document_search` is the FTS5
index and `document_text` is the joined SQL view. These are separate from
`fcc_applications`, `fcc_exhibits` and catalog model assertions. A PDF phrase
alone does not establish model aliases, manufacturer, deployment or inventory
validation. No grant date is taken from the mirror's display date.

The label drawing was visually reviewed: it includes a draft FCC-ID placeholder.
Its other label strings are not promoted to production identifiers or aliases.
The source PDFs retain original rights; cached text can be incomplete, out of
reading order or omit diagram content. Open the PDF when exact wording matters.

Tests exercise refresh cleanup, blank-page preservation, content versus metadata
search, limits, malformed dependencies, source hash association, page ordering,
read-only FTS SQL, legacy databases and caller transaction preservation.

## Related AWS evidence

The subsequent [Monitron import](aws-monitron.md) uses the AWS safety index,
four guides and the dated documentation-history notice. Its original PDFs and
page captures are included in the offline rebuild manifest. Pricing, terms and
US gateway guides remain research inputs without catalog assertions. AWS guide
text is not yet included in the separate FCC document-search index.
