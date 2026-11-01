# Kenya CA equipment register, August 2026

The official [register index](https://www.ca.go.ke/node/255) links the
[August 2026 PDF](https://www.ca.go.ke/sites/default/files/2026-09/List%20of%20Type%20Approved%20Equipment%20August%202026.pdf).
Both original files were downloaded. The index explicitly describes the list as
nonexhaustive. The PDF has 203 physical pages; printed page numbers diverge later
in the document. All physical pages have embedded text, now searchable locally.
The source PDF SHA-256 is
`874900238c0cdde2ca3beeac372fd29928b38dfba31f84831678b8d8cf574e19`.

All embedded pages were screened for the five target company names. Seven
candidate rows appeared on five pages; those five complete pages were rendered
and visually reviewed to verify manufacturer, brand and model columns. This is
a selected factual extraction plus a complete document-text archive, not a
structured import of every table row or proof of exhaustive manufacturer coverage.

## Reviewed assertions

| Physical PDF page | Printed page | Row | Manufacturer target | Complete model cell |
|---:|---:|---:|---|---|
| 9 | 9 | 11 | Amazon Data Services | HOPPER26SF |
| 23 | 23 | 183 | Edge-Core | ECS TABLET PC (TR10RS1) |
| 180 | 183 | 21 | Edge-Core | ECS5550-30X |
| 186 | 189 | 1 | Amazon Data Services | TIMBERWLFL10 |
| 192 | 195 | 71 | Edge-Core | ECS5550-30X |
| 192 | 195 | 72 | Edge-Core | ACCTON-AS8060 |

The six assertions add three catalog identities. HOPPER26SF and ECS5550-30X
already existed; the others are new literal labels in their manufacturer
namespaces. New identities do not necessarily mean previously unknown physical
hardware models. ACCTON-AS8060 is retained whole under the source's Edgecore
manufacturer attribution; no Accton manufacturer claim or AS8060 alias is added.

Printed geographic qualifiers are retained in `registry_manufacturer` facts.
The reviewed affiliations use the named company's target spelling, without
introducing global alias rules. Brand, manufacturer and equipment category are
separate fields. Manufacturer assertions are regulator claims, not independent
verification of a physical factory, current approval status or unit provenance.

## Source ambiguities retained

- HOPPER26SF appears under a mobile-phone/tablet category. That heading is saved
  as source text, not used to claim that the device is a phone or tablet.
- The tablet row attributes the model to EDGECORE NETWORKS. This potentially
  questionable attribution is explicitly flagged; it is not independently
  confirmed as the physical manufacturer.
- Immediately after the Edgecore router row, another row prints ECS5550-30X
  under EXTREME NETWORKS, INC. The contradiction remains in the PDF and is noted
  on the imported Edgecore assertion rather than silently corrected.
- Server row 2 on physical page 186 prints CAMILLO and GOLGI on two lines in one
  model cell. It stays unresolved in the reviewed capture and searchable page
  archive. No split or equivalence to the existing `Camillo_Golgi` record is
  asserted.

No new AWS deployment or hyperscaler-use claim is inferred from these rows.
Neither full `7760-32X-A-AC-F` nor an equivalence for it was found. The corrected
owned-hardware baseline remains **10/10 searchable, 9/10 corroborated**.

## Local use and reproduction

```sh
python3 -m devicehunt search TIMBERWLFL10
python3 -m devicehunt search ACCTON-AS8060 --vendor Edge-Core
python3 -m devicehunt lookup TR10RS1
python3 -m devicehunt documents CAMILLO --source reference-pdf-kenya-2026-08
python3 -m devicehunt import curated data/kenya-approved-reviewed.json
python3 -m devicehunt import reference-documents data/raw/reference-documents-bundle.json
```

```sql
SELECT title, page_number, text, source_url
FROM document_text
WHERE source_id = 'reference-pdf-kenya-2026-08'
  AND text LIKE '%TIMBERWLFL10%';

SELECT d.model, a.company, a.role, r.source_url
FROM records r JOIN devices d ON d.id = r.device_id
JOIN affiliations a ON a.record_id = r.id
WHERE r.source_id = 'kenya-ca-reviewed';
```

The review file is `data/kenya-approved-reviewed.json`. Original index and
selected-page table captures live under `data/raw/kenya-ca/`; the original PDF
and full text capture are `data/raw/kenya-approved-2026-08.pdf` and
`data/raw/kenya-approved-2026-08-text.json`. All are checksummed in the offline
rebuild manifest. `data/exports/kenya-*` retains import, SQL and export audits.

The full offline rebuild in `data/rebuilt-with-kenya.sqlite` matches all 37
checked tables/search indexes and the ten-device baseline. It verifies 88 import
jobs and 1,919 hashed files. All prior device records, affiliations, identifiers,
facts and hyperscaler evidence are preserved. The six full source model cells
were checked against imported raw records; existing `Hopper26SF` display casing
is retained while the source assertion keeps its original `HOPPER26SF` spelling.
Six existing document-archive tests pass. Catalog totals are now 3,525 identities,
4,625 source assertions and 191 source entries. The archive has 34 PDFs and
3,290 pages; 3,282 pages contain some embedded text.
