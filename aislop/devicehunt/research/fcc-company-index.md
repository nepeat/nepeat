# Local FCC-ID discovery index

Current combined discovery index: **344 source rows / 210 literal FCC IDs**.
The original HTML captures below contain 171 rows; a separately labelled two-row
[Celestica factual extract](fcc-celestica-supplement.md) and 169 distinct IDs from
[Accton company-table extracts](fcc-accton-company.md) supplement them.
Two [Amazon company rows](fcc-amazon-company.md) add CubScout index coverage.
Duplicate IDs across mirrors remain separate source rows. The original mirror’s
not-found results remain intact.

Original public HTML from [FCCID.co](https://fccid.co/) is cached for six
grantee-code queries. This is a third-party mirror, not the official FCC service.
The snapshot adds **171 index entries** to `fcc_id_index`; it does not add
171 models or complete authorization applications to the catalog.

| Grantee | Listed organization | Captured rows | Pages | Capture state |
| --- | --- | ---: | ---: | --- |
| HED | Accton Technology Corp | 146 | 6 | Listed |
| YZK | Edgecore Networks Corporation | 20 | 1 | Listed |
| A4H | Oracle America, Inc | 4 | 1 | Listed |
| 2AVPL | Amazon Data Services, Inc. | 1 | 1 | Listed |
| MRF | Celestica Incorporated | 0 | 1 | Company not found in this mirror |
| NVN | Celestica Limited | 0 | 1 | Company not found in this mirror |

The grantee codes and legal names were checked against the existing FCC grantee
snapshot. Each cached listing is checked against its own company heading,
grantee code, reported total, 25-row pagination, page number and unique FCC IDs.
All listed pages were captured. SHA-256 hashes are in
`data/raw/fcc-index/bundle.json`; the original HTML includes the two not-found
responses. Those responses are **coverage gaps, not evidence of zero filings**.

## Known omissions and date discrepancies

The public FCCID.io index reports 192 IDs for HED, 37 for YZK, 11 for A4H and
two for 2AVPL. Its MRF page exposes two Celestica rows. These exceed the counts
above; the two mirrors do not establish each other's completeness. FCCID.io
page access was uneven: Accton's second page failed to open, Oracle was available
as a search result but direct opening returned 403, and a direct Accton HTML
download returned 403. No access restrictions were bypassed.

The reviewed comparisons and retrieval limits are saved in
`data/raw/fcc-index/mirror-comparison.json`. Reference indexes:
[Accton](https://fccid.io/HED), [Edgecore](https://fccid.io/YZK),
[Oracle](https://fccid.io/A4H), [Amazon Data Services](https://fccid.io/2AVPL),
[Celestica Incorporated](https://fccid.io/MRF).

For example, HEDEAP112 is dated `2024-10-14` in the FCCID.io company index and
`Oct 13, 2024` on the cached FCCID.co card. The latter is retained verbatim as
`listed_date`, without guessing a time zone, correcting it, or asserting a grant
date. The local Amazon index contains only TE1A001; the existing reviewed
application evidence also contains CubScout. These demonstrate that a fully
captured mirror query is not a full FCC filing history.

## Search and SQL

```sh
python3 -m devicehunt lookup 'HEDEAP115'
python3 -m devicehunt lookup 'Oracle America' --format json
python3 -m devicehunt sql "SELECT fcc_id, grantee_name, product_description, listed_date, source_url FROM fcc_id_index WHERE grantee_code = 'HED' ORDER BY fcc_id"
python3 -m devicehunt sql "SELECT id, records_seen, coverage FROM sources WHERE id LIKE 'fcc-index-%'"
python3 -m devicehunt import fcc-index data/raw/fcc-index/bundle.json
```

`data/exports/fcc-id-index.csv` exports all 171 rows. Unified `lookup` reports
them as a separate dataset with all six source scopes, including not-found
company queries. Missing tables in older databases remain `not_loaded` without
migration. Counts must not be added to model counts or to the historical grantee
registration count.

Fields preserve FCC ID, grantee code/name, displayed date, product description,
equipment class, application purpose, detail URL and originating page URL.
The suffix after a grantee code is **not inferred to be a model name**. Grantee
identity is not physical manufacturer or hyperscaler deployment evidence. These
index rows do not create model aliases, grant rows or inventory-validation passes.
The existing `fcc_applications` and `fcc_exhibits` tables retain their separate,
more detailed evidence.

## Parser and validation

Some cached pages stream HTML fragments after their placeholders. The importer
reconnects only explicitly named static `$RS(source,target)` fragments; it never
executes page JavaScript. Duplicate, cyclic, missing or reused fragments are
rejected. Each card's identity, company and fields must remain within that card,
and unresolved placeholders cause rejection. Scripts cannot contribute apparent
product rows. This handles Oracle's cards arriving after its company-info block.

All six query captures are parsed and verified before any source is replaced.
Tests cover page truncation, repeated pages/IDs, mismatched company fields,
malformed dates, broken streamed fragments, corrupt hashes, escaping paths,
not-found ambiguity and the separation of identifiers from models and grants.

This source is a useful local discovery index. Complete official FCC equipment
authorization coverage and full exhibit archives remain open. The complete
`7760-32X-A-AC-F` identity now has [reviewed label-photo evidence](edgecore-7760-label.md). The former AZ4444 gap was resolved when the owner
confirmed the complete label AZ4444-52X-DC-10; existing Anatel evidence matches it.
