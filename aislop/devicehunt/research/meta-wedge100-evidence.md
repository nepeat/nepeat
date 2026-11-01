# Accton manufacturing and Facebook Wedge 100 evidence

Reviewed September 27, 2026 against the saved original HTML and the live
[Facebook engineering article dated October 18, 2016](https://engineering.fb.com/2016/10/18/data-center-engineering/wedge-100-more-open-and-versatile-than-ever/).

| Printed model | Manufacturer | Other relationship | Hyperscaler evidence |
|---|---|---|---|
| Wedge 100 | Accton Technology | Facebook platform design | Facebook reports production deployment |
| Wedge 100-32X | Accton Technology | Edgecore commercial brand | Wedge 100 OCP design lineage; commercial-model deployment not explicitly established |

These are historical platform claims. They do not identify the prior owner of
an auction unit or guarantee present support, software access, stock or condition.
The Accton namespace for Wedge 100 represents the manufacturer, not a retail
brand. Existing `WEDGE100-32X` and differently branded entries remain separate
source identities; their count is not a count of proven distinct physical designs.
No aliases or ordering-code suffixes were inferred.

Only Wedge 100 receives the article's rack-adapter and bus-bar-module statements.
The commercial 32X record receives its own historical shipping-software statement.
These are source-specific descriptions, not verified configurations of used units.

## Offline use

```sh
python3 -m devicehunt search 'Wedge 100' --vendor Accton --role manufacturer --hyperscaler
python3 -m devicehunt lookup 'Wedge 100-32X'
python3 -m devicehunt specs 'Wedge 100' --exact-model
python3 -m devicehunt match-listing --title 'Edgecore Wedge 100-32X empty chassis' --with-specs --format json
```

The last command identifies model mentions in supplied text, not live offers.
Spaced names with added or omitted dash suffixes now produce variant leads.
Those leads receive no specifications. Whitespace normalization does not remove
spaces or make `Wedge 100-32X` an alias of `WEDGE100-32X`.

```sql
SELECT d.brand, d.model, a.company AS manufacturer,
       h.operator, h.relationship, h.detail, r.source_url
FROM records r
JOIN devices d ON d.id = r.device_id
JOIN affiliations a ON a.record_id = r.id AND a.role = 'manufacturer'
JOIN hyperscaler_evidence h ON h.record_id = r.id
WHERE r.source_id = 'meta-wedge100-2016'
ORDER BY d.model;
```

## Reproduction and checks

The original article and fetch metadata are under `data/raw/meta-wedge100/`.
`review-notes.json` records publication metadata, paragraph indexes and text
hashes. `data/meta-wedge100-reviewed.json` contains the two reviewed records.
The import manifest hashes all four inputs and replays them through the existing
curated importer. Source ID: `meta-wedge100-2016`.

The addition brings the catalog to 3,532 identities, 4,651 records and 215 sources.
Audit outputs under `data/exports/` use the `meta-wedge100-` prefix. Preservation
checks compare all prior source rows and indexed document text. Search checks
cover roles, distinct deployment relationships, exact model specifications,
unchanged identities, auction-title matches and unknown-suffix isolation.
The offline rebuild is `data/rebuilt-with-wedge100.sqlite`; its 39 tables and
indexes are compared with the live database. The owned-hardware baseline remains
10/10 searchable and 9/10 corroborated. Complete historical/FCC coverage and
independent corroboration of `7760-32X-A-AC-F` remain unresolved.
