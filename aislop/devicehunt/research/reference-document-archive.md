# Offline reference PDF search

The `reference-documents` importer adds full embedded page text from 24
reviewed original non-FCC PDFs cached directly under `data/raw`. This is a
reusable archive importer with a reviewed local bundle; it makes no network
requests and creates no device assertions. The existing FCC exhibit importer
continues to require an exact FCC-index match.

Together, the archives now contain **35 PDFs, 3,339 pages and 3,331 pages with
some embedded text**. The 24 reference PDFs contribute 3,265 pages.

| Reference source suffix | PDF | Pages |
|---|---|---:|
| icasa-2025 | ICASA 2025 equipment register | 906 |
| ntra-2026-01 | NTRA equipment register, January 2026 file | 760 |
| ntra-2022-12 | NTRA equipment register, December 4, 2022 | 1,218 |
| aws-monitron-sensor | Monitron sensor guide | 52 |
| aws-monitron-ex-sensor | Monitron Ex sensor guide | 2 |
| aws-monitron-wifi-eu | Monitron Wi-Fi gateway EU guide | 5 |
| aws-monitron-ethernet-eu | Monitron Ethernet gateway EU guide | 5 |
| aws-monitron-wifi-us | Monitron Wi-Fi gateway US guide | 3 |
| aws-monitron-ethernet-us | Monitron Ethernet gateway US guide | 3 |
| aws-scout | Outposts SCOUT declaration | 2 |
| aws-cubscout | Outposts CubScout declaration | 2 |
| aws-panorama | Panorama safety and compliance manual | 11 |
| celestica-2025 | Celestica 2025 portfolio catalog | 8 |
| cisco-ncs-certificates | Cisco NCS1004/NCS1014 CB certificates | 6 |
| aws-snowcone-it | Snowcone Italian guide with English declarations | 10 |
| aws-snow-family-2022 | Snowball/Snowcone September 2022 family guide | 3 |
| aws-thin-client-eu | WorkSpaces Thin Client EU declaration | 2 |
| aws-thin-client-safety | WorkSpaces Thin Client safety guide | 5 |
| oracle-x9-2-ecodesign | Oracle X9-2 EcoDesign | 3 |
| oracle-x9-2l-ecodesign | Oracle X9-2L EcoDesign | 3 |
| oracle-x10m-ecodesign | Oracle X10M EcoDesign | 2 |
| oracle-x11m-ecodesign | Oracle X11M EcoDesign | 2 |
| kenya-2026-08 | Kenya CA August 2026 equipment register | 203 |
| celestica-ds4000-guide | Celestica DS4000/DS4001 installation guide | 49 |

Each source ID is `reference-pdf-` followed by the suffix above. Original URLs,
PDF hashes, page counts and complete text dependencies are recorded in
`data/raw/reference-documents-bundle.json`. Existing page captures were reused;
six previously unindexed PDFs received new embedded-text captures in the initial
archive expansion; the two subsequently collected Snow guides also have full
embedded-text captures. The offline
rebuild manifest hashes the bundle and every dependency, including both US
Monitron PDFs, which were previously outside its selected identity imports.

## Search and SQL

```sh
python3 -m devicehunt documents AMHT-1 --source reference-pdf-aws-panorama
python3 -m devicehunt documents Celestica --source reference-pdf-cisco-ncs-certificates
python3 -m devicehunt documents 'CS 8210' --source reference-pdf-ntra-2026-01
python3 -m devicehunt documents --source reference-pdf-celestica-2025 --limit -1
python3 -m devicehunt documents antenna --fcc-id 2AVPL-TE1A001
python3 -m devicehunt lookup 'AZ4444-52X'
```

`documents` searches token prefixes in embedded page text and returns page
numbers, excerpts, hashes, original URLs and absolute local PDF paths. An empty
query lists pages, including those with no embedded text. `--source` is an exact
source-ID filter, and can be combined with `--fcc-id`. Reference documents use
an empty FCC-ID field because this archive does not assign regulatory IDs.
The broader `lookup` command also searches document metadata using its existing
literal-substring behavior. It reports page counts separately from identities
and register rows; these are different units and must not be added together.

```sql
SELECT title, source_id, page_count, pages_with_text, source_url
FROM documents
ORDER BY source_id, document_id;

SELECT title, page_number, text, source_url
FROM document_text
WHERE source_id = 'reference-pdf-aws-panorama'
  AND text LIKE '%AMHT-1%'
ORDER BY page_number;
```

## Limits and refresh behavior

This is an archive of the files already collected, not a complete vendor library
or a new source retrieval. No OCR is performed. Eight pages have no embedded
text; pages with headers can still be missing image-only body text. Panorama's
declaration images illustrate this limitation. Some Monitron multilingual text
has malformed embedded glyphs; these are retained verbatim rather than repaired
or silently converted into model evidence. Register text may lose table layout;
use the structured register tables for row relationships and the original PDF
for visual confirmation.

Embedded PDF text can also insert spaces inside identifiers. NTRA page 102
extracts `CS 8210-32X-DC-11`, so `documents CS8210` does not find that page;
`documents 'CS 8210'` or `documents Celestica` does. No spaces are silently
removed or aliases created in the archive. The existing catalog and structured
register searches remain available for their separately parsed identifiers.

Search hits do not become exact device identities, aliases, manufacturers,
operators or inventory-validation passes. The reviewed source importers remain
responsible for those interpretations. The new archive adds zero identities or
device assertions, and leaves the owner's two evidence gaps open.

The importer validates the entire bundle before replacing its collection:
unique IDs/URLs/files/content hashes, HTTPS source URLs, provenance tiers,
PDF magic, sibling-path safety, text/PDF association and consecutive page counts.
Refreshing replaces the `reference-pdf-*` collection and removes references
omitted from the new bundle, preserving FCC documents and other source data.
Both PDF importers now rebuild the shared text index in a fixed order, so row
IDs are reproducible regardless of refresh order. Searches remain read-only.

The export `data/exports/reference-documents.csv` lists these 23 documents.
The Panorama, Cisco and NTRA example searches are saved as
`reference-search-panorama.json`, `reference-search-cisco.json` and
`reference-search-ntra.json` in the same directory.
