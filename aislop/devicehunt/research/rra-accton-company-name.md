# Accton Singapore company spelling in Korean approvals

Reviewed 2026-09-27 UTC. The cached Accton RRA query has 200 approval rows.
Fourteen identify the manufacturer as `ACCTON TECHNOLOGY CORP. PTE LTD.`.
The search-index extract of Accton's [2013 consolidated report](https://www.accton.com/wp-content/uploads/2017/09/2013-Consolidated-Report.pdf)
names `Accton Technology Corp. Pte Ltd (Singapore)` among Edgecore Networks'
wholly owned subsidiaries. This supports a reviewed whole-name match with a
geographic qualifier; it does not establish ownership at the 2001–2002 approval
dates or today. Direct PDF opening returned HTTP 402. No retry or alternate
retrieval followed; the original PDF and its visual layout were not inspected.

The separately hashed factual search extract is
`data/raw/rra-accton-official/accton-singapore-2013.json`. Its report year is
explicitly a year, not an invented exact reporting date. Other subsidiary names
are retained as context and do not enable aliases. The reviewed mapping is
optional and scoped to this RRA bundle; removing it withdraws its extra claims.
Global aliases, composite company handling and the baseline are unchanged.

The target namespace is Accton, matching the named manufacturer. The raw legal
manufacturer remains `ACCTON TECHNOLOGY CORP. PTE LTD.`; Accton parent-company
manufacturing is not inferred. The reported Edgecore parent, report year,
mapping interpretation, URL, capture method and dependency hash are facts on
each affected source record. No Edge-Core device alias, factory or hyperscaler
deployment claim is created.

Thirteen new source assertions cover twelve identities, ten new to the catalog.
Examples include `AC-IG2004W`, `ES3526L`, `WA3001-PoE` and `AC-MC110-SC`.
Two approvals describe `AC-IG2004W`. The fourteenth row's main-model cell,
`EN1207F-TX/WOL` (certificate `E-G021-01-2328`), remains raw searchable evidence:
the current conservative model parser does not interpret slash-qualified names.
It is neither split into aliases nor silently discarded from the registry.

At this company-name checkpoint the Accton query contributed 176 model assertions.
The later [main-list parser](rra-main-model-lists.md) adds 12 more, for 188.
Thirty-six approval rows
still have unmapped company names, including `ACCTON TECHNOLOGY INC.` and
several composite cells. The [review audit](../data/exports/rra-accton-company-review.json)
lists their exact names and row counts. The 2024/2025 Accton filings found during
research do not verify all these historical spellings. They also describe
changing China investments, which is a reason not to collapse all name matches
into one timeless corporate identity.

## Local use

```sh
python3 -m devicehunt search AC-IG2004W --vendor Accton --role registry_manufacturer
python3 -m devicehunt lookup EN1207F-TX/WOL
python3 -m devicehunt vendor-inventory Accton --limit -1
```

```sql
SELECT d.model, a.company, a.role, r.source_url
FROM records r JOIN devices d ON d.id=r.device_id
JOIN affiliations a ON a.record_id=r.id
WHERE r.source_id='rra-official-query-e5984834207f'
  AND a.company='ACCTON TECHNOLOGY CORP. PTE LTD.'
  AND a.role='registry_manufacturer'
ORDER BY d.model;
```

Exports: `accton-manufacturer-additions.csv` and `.json` under `data/exports`.
Tests verify evidence-dependent inclusion and withdrawal, exact whole-company
matching, preserved raw names and dated parent facts, no global/SMC/composite
alias expansion, and rejection of changed evidence before record replacement.
