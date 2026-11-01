# ByteDance and Inspur — scope and first evidence pass

Follow-up: the [bounded ByteDance search](batches/vendor-discovery-2026-10-04/bytedance/handoff.md)
found a community article describing B4020, but no cited primary ByteDance source.
Its model/port/deployment statements remain unimported; both examples remain gaps.

The [subsequent diagram trace](batches/networking-primary-2026-10-04/bytedance/handoff.md)
found no model or ByteDance identification in the image. Its watermark is only
an attribution lead; the original OCP agenda request returned 403 and stays
unavailable. No new claim qualifies for import, and the exhausted query sets
should not be repeated without a distinct new lead.
Seek independently available, primary model/affiliation evidence before another
ByteDance pass; continue other target vendors meanwhile.

Added at the user's request on 2026-10-04. The active project goal now has
thirteen named target groups. `validation/vendor-examples.json` records B4020
and B4010, preserving the supplied labels `BYTEDANCE B4020` and `BYTEDANCE B4010`.
These are research examples, not evidence that the user owns them. The original
ten-device owned baseline is unchanged.

## Exact target mapping

- `ByteDance`: exact requested name, with ordinary case/punctuation normalization.
  TikTok, Pico, ByteDance System Sdn Bhd and other legal entities are not aliases.
- `Inspur`: exact brand and `Inspur Electronic Information Industry Co., Ltd.`.
  The original i24 white paper displays the Inspur brand/product header and that
  legal company footer. This search grouping preserves each record's actual role.
  Inspur Software Group, the ENERGY STAR spelling `Inspur Electric Information
  Industry Co, Ltd.`, Qingdao LG Inspur, and IEIT Systems remain unassigned names.
  Records independently branded Inspur can qualify through their brand role.

The NTRA PDF page-screening expression previously contained only the first five
target names. It now derives from the active target registry. Positional table
parsing still requires exact brand matching; broader screening alone adds no
company assertion. The complete backfill also revisits the prior six ODM groups.

## B4010: genuine model evidence, unresolved ByteDance association

[Original Accton-hosted BTL PDF](https://www.accton.com.cn/wp-content/uploads/B4010-2302C014-CER_FCCISED.pdf)
has two statements naming Ethernet Switch model B4010, brand N/A, applicant
Accton Technology Corporation, and report BTL-FICE-1-2302C014. Page 1 concerns
FCC Part 15 Subpart B; page 2 concerns ICES-003. It supplies no FCC ID and no
ByteDance, manufacturer or deployment claim. The catalog preserves brand `N/A`
and Accton's applicant role. It must not pass the ByteDance coverage example.

The two ByteDance example checks therefore remain evidence gaps in this pass.
B4010 has the separately sourced applicant/model lead; B4020 has no imported
qualifying identity. Raw substring matches were unrelated: a Xiaomi NCC record,
FCC URL strings GU6WTB4010 / FBIVB402051 / BVCAMB4020, and KONICA MINOLTA B4020i.
Those are not promoted to ByteDance matches or aliases.

Public search previews from Volza and Eximpedia contained both ByteDance model
labels. They remain secondary discovery leads only: the Eximpedia company page
returned HTTP 402 on opening, so collection stopped. No paid data, original bill
of lading, importer/exporter identity, OEM role or deployment was inferred.
Lead URLs (not imported model evidence):

- https://www.volza.com/p/models/hsn-code-85176249/import-data/
- https://www.eximpedia.app/companies/accton-technology-co-limited/10894156

## Inspur sources

The cached Anatel register explicitly names `INSPUR ELECTRONIC INFORMATION
INDUSTRY CO LTD` as manufacturer of AS61-48E4T-F-LC, AS61-48X4T-F-LC and
AS71-128H-LC. Raw suspended-status fields remain snapshot claims, not a current
determination. They do not prove hyperscaler deployment.

Cached ENERGY STAR and NTRA entries provide further brand/model evidence;
roles and complete model strings, including prefixes and suffixes, remain intact.
The 51 literal NCC name hits mostly involve LG-related manufacturer cells;
substring matching does not justify mapping that collection to Inspur.

[Original i24 white paper](https://www.inspur.com/eportal/fileDir/defaultCurSite/resource/cms/2020/08/2020081316525840458.pdf)
is cached in full: 34 pages, cover revision 3.0, release 2020-01. It adds an
`i24` platform record and supports the reviewed legal-name mapping. Other node
and revision names in the document are searchable text, not automatic aliases.
The Inspur about page `https://inspur.com/en/2822784/index.html` returned 403;
it is recorded as unavailable and must not be retried through another route.

## Local evidence and commands

Original files and retrieval outcomes: `data/raw/bytedance-inspur-2026-10-04/`.
Structured assertions: `data/bytedance-inspur-facts-2026-10-04.json`.
The complete reference collection includes all prior PDFs plus these two;
embedded text uses pypdf without OCR. B4010 page 1 and i24 page 11 were visually
reviewed for model/company roles. No units or suffixes were inferred.

```sh
python3 -m devicehunt vendor-inventory Inspur --scope all --limit -1
python3 -m devicehunt vendor-inventory ByteDance --scope all --limit -1
python3 -m devicehunt search B4010
python3 -m devicehunt documents B4010
python3 -m devicehunt validate-inventory --inventory validation/vendor-examples.json
python3 -m devicehunt validate-inventory
```

Example validation intentionally returns exit 2 until external evidence connects
the full model to the expected target. A test protects the case where an exact
Accton applicant model would otherwise look like a ByteDance hit.

Next: obtain primary ByteDance-branded label/compliance/product evidence for
B4020/B4010; review dated operator/OEM relationships independently. Continue
Inspur model-specific server/switch evidence and original-vendor collection.
No complete vendor inventory or worldwide FCC coverage is claimed.
