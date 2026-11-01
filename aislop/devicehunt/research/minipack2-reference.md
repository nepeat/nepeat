# Minipack2 original specification in the local archive

The [original Meta Minipack2 specification hosted by OCP](https://www.opencompute.org/documents/minipack2-generic-ocp-specification-rev1-0-pdf)
is saved as `data/raw/meta-minipack2-spec.pdf`. Its cover identifies revision 1.0
and a document date of September 16, 2021. All 32 pages have embedded text in
the local SQLite document archive. These are generic design requirements, not
an inspection of a particular used switch.

Useful review locations:

| PDF page | What to inspect |
|---|---|
| 9 | Historical statement of deployment in Meta's F16 fabric |
| 12–13 | 4RU/19-inch rack design; modular ports and configuration requirements |
| 13 | AC-input nominal voltages of 208/230/277V, front-to-back cooling, and conditional power-consumption target |
| 29 | Front pull-out model/serial tag, Meta asset marking and board label requirements |
| 31 | Blank supplier company and product fields |

The document does not name Celestica. Existing Celestica Minipack2,
Minipack2-AC and Minipack2-48VDC-F identities retain their separate UL
listing-holder evidence. This PDF creates no manufacturer, alias, variant
equivalence or individual-unit provenance assertion. In particular, its generic
power description must not be transferred to a suffixed commercial unit without
checking that unit's documentation and labels.

## Search offline

```sh
python3 -m devicehunt documents 'asset tag' --source reference-pdf-meta-minipack2-spec
python3 -m devicehunt documents 277V --source reference-pdf-meta-minipack2-spec
python3 -m devicehunt lookup Minipack2
```

Document results include the original URL, local PDF path and page number.
`lookup` keeps document-text hits separate from catalog identities and company
claims. These passages do not appear as qualified Celestica `specs` statements.

```sql
SELECT d.title, p.page_number, p.text, d.source_url, d.pdf_sha256
FROM documents d JOIN document_pages p USING(document_key)
WHERE d.source_id = 'reference-pdf-meta-minipack2-spec'
  AND p.page_number IN (9, 13, 29, 31)
ORDER BY p.page_number;
```

## Capture and validation

Captured September 27, 2026 from the original OCP URL with HTTP 200. PDF SHA256:
`7f71b13da4039fee1f4f24c2dc63f91301a9c83d37f89e4e470fed9109b96842`.
The adjacent JSON contains pypdf page text; no OCR was performed. pypdf reported
cross-reference warnings while recovering the document. Rendered pages 9, 13,
29 and 31 were visually checked against the extracted content. Image-only text
can remain unindexed even when a page has other embedded text.

The separate rackcdn OCP presentation URL surfaced in search, but its download
failed the PDF signature/size check and produced no saved PDF. The download
script did not retain the response detail. No claim relies on that retrieval or
on a third-party copy of the presentation.

The reference bundle retains all 24 previous PDFs and adds this document.
Its bundle hash changes for all 25 archive-source metadata rows; individual
PDF hashes remain available in `documents.pdf_sha256`. All prior document
contents and catalog assertions are preserved. Audits and query examples use
the `minipack2-reference-` prefix in `data/exports/`.

Current archive: 36 PDFs and 3,371 pages across FCC and reference collections.
Device identities remain 3,532 with 4,651 records; sources increase to 216.
The owned baseline remains 10/10 searchable and 9/10 corroborated. Neither the
7760 full-code gap nor complete historical/FCC coverage is resolved.
