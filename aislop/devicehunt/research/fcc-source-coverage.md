# FCC source coverage

Current expanded-target snapshot (2026-10-04): 417 known associated FCC strings;
210 have index records, 171 have detail captures, 2 have application records and
1 has an FCC-ID-linked PDF. The global sitemap contains 306 of these strings.
See `data/exports/fcc-coverage-all.json` and `fcc-sitemap-summary.json`.
The following dated entries retain prior-scope collection history.

Current local coverage (2026-09-27): 271 known target-associated FCC identifier
strings; the discovery index covers 210 and lacks 61. Its 344 source rows include
multiple mirrors. There are still two detailed application records. This is a
measured subset, not a complete FCC authorization archive. See
[comparison rules and counts](fcc-collection-coverage.md).

The public [sitemap snapshot](fcc-sitemaps.md) adds 190,198 distinct FCC discovery
URLs in a separate raw table; it is also incomplete and does not change the
above target-stage accounting. The user discarded the paid-provider path. The
[commercial-source review](fcc-bulk-provider-review.md) is historical only.
Earlier sections below retain discovery and access history.

Reviewed 2026-09-26.

The [official EAS grantee dataset](https://opendata.fcc.gov/d/3b3k-34jp), also
[listed on data.gov](https://catalog.data.gov/dataset/eas-equipment-authorization-grantee-registrations),
provides a complete downloadable historical company register. Its API reports
50,153 rows and `rowsUpdatedAt=1616396691` (2021-03-22 UTC). The collected CSV
has the same count, unique grantee codes, and ten fields. One name is blank
(code OAF); that record is retained without inventing a company.

This answers the bulk-download question for **grantee registrations only**.
It does not supply device model names, authorization grants or exhibit documents.
The local catalog still contains only two reviewed Amazon FCC device applications
and their 32 exhibit-index rows, plus FCC identifiers from other sources.

Cached bundle: `data/raw/fcc-grantees-bundle.json`. It references original CSV,
metadata before/after, and count responses before/after, each hashed. Importer:
`fcc-grantees`. SQL table: `fcc_grantees`. Search command: `grantees`.
Company registrations do not enter the device index or inventory coverage gate.

Observed target-name matches:

| Code | Original legal name | Treatment |
|---|---|---|
| HED | Accton Technology Corp | Target company filing lead |
| YZK | Edgecore Networks Corporation | Target company filing lead |
| A4H | Oracle America, Inc | Target company filing lead |
| 2AVPL | Amazon Data Services, Inc. | Target company filing lead |
| MRF | Celestica Incorporated | Separate legal entity filing lead |
| NVN | Celestica Limited | Separate legal entity filing lead |
| GWV | Sun Microsystems Inc | Legacy company; no automatic Oracle merge |
| UDM | Sun Microsystems, Inc | Legacy company; separate registration |
| V8Y | Accton Wireless Broadband Corp. | Related-name lead; no assumed parent equivalence |

The official KDB entry 953436, EAS Web API Services, was identified. Indexed
primary-source text says `getFCCIDList` accepts complete or partial FCC IDs and
returns approved matching IDs. It names attachment "953436 D01 OET Laboratory
Services API v03", dated 2023-10-23. Direct page retrieval returned HTTP 403, so
the attachment, request parameters and pagination/completeness semantics remain
unverified; no guessed API results are imported. Investigate the official
interface for these grantee codes before attempting a filing collector.


Follow-up: the documented public request
`https://apps.fcc.gov/OETLabServices/getFCCIDList?fccId=2AVPL` returned HTTP 403.
No response records were imported. FCC.report indexes for YZK, MRF and NVN also
returned HTTP 403. A [mirror of the FCC API document](https://keybridgewireless.com/codex/read/documentation/fcc-part-96/kdb-2023-equipment-authorization-api)
confirms the `fccId` parameter and returned fields. It does not provide an
independent total count; live collection and completeness verification remain
open. Access errors were not treated as empty datasets.

Bulk-source follow-up on 2026-09-26: the FCC.report coverage page describes a
large FCC-ID metadata index refreshed through an incremental local bridge, but
the reviewed page provides no downloadable complete equipment-authorization
snapshot. Its displayed counts vary with the page snapshot date and do not
verify this project's coverage. Public GitHub search results primarily returned
ULS amateur-license loaders, the same FCC grantee dataset, or per-grantee web
scrapers; none supplied a verified full device-filing export for this catalog.
The FCC application count remains two. This is a scoped research result, not
proof that no bulk service exists.

## Selected company indexes now cached

The accessible FCCID.co mirror supplied original HTML for all its listed pages
under HED, YZK, A4H and 2AVPL: 146, 20, four and one index entries respectively.
MRF and NVN return explicit company-not-found pages, retained as coverage gaps.
The 171 rows are now in the separate `fcc_id_index` table and unified lookup;
they do not create model identities or detailed application records. The two
existing Amazon application records and 32 exhibit-index rows remain unchanged.

This mirror demonstrably omits records: FCCID.io's visible company counts are
higher, and FCCID.co's Amazon index lacks the already documented CubScout entry.
Displayed dates can also disagree between mirrors and remain verbatim. Original
HTML, page checksums and a factual comparison note are cached. See
[company-index review](fcc-company-index.md) for counts, access results, parser
checks and SQL examples. Complete official FCC filings remain unavailable here.

## Additional bulk-source screening

A further public-source search on 2026-09-26 still identified the same official
historical grantee export. Two other downloadable database leads were reviewed
for scope before any import:

- [Squirrel Shock downloads](https://squirrelshock.info/downloads/) describe
  licensed radio-station records from ELS, ULS, ICFS and LMS. That is a different
  record type from FCC equipment authorizations and does not supply the requested
  full device-ID database.
- [Argus's own repository](https://github.com/kevwillow/argus-db) describes
  equipment identifiers selected for surveillance-related categories. Its
  published artifact is an export; its canonical SQLite database is not shipped.
  Its scope and source credits do not establish a complete FCC filing dump.

Neither dataset was downloaded or imported. No denied FCC endpoint was retried.
This narrows these particular leads; it does not prove that a complete equipment
authorization bulk source cannot be obtained elsewhere.

## Further bulk screening and existing-capture extraction

The next public-source search still did not identify a verified full EAS filing
download. FCCIDSearch.com exposes an additional public mirror; its home page was
readable through the web tool, but `/grantees` and `/search` returned HTTP 403 on
direct retrieval. Both were stopped, with no alternate route or retry. No data
from those denied pages was saved or imported. Search results for ULS, CDBS and
FCC-rule text remain different datasets, not equipment-authorization substitutes.

Instead, the already archived Sporton report covers now have a separately checked
[applicant/manufacturer extraction](fcc-sporton-cover-roles.md). That adds useful
local evidence but does not increase the number of captured FCC IDs or establish
complete official FCC coverage. No original PDF availability is implied.
