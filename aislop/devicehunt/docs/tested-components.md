# Search parts within tested configurations

```sh
python3 -m devicehunt tested-components PM883
python3 -m devicehunt tested-components 'PS-2162-6H2R' --format json
python3 -m devicehunt tested-components --vendor 'Amazon Data Services' --limit -1 --format csv
```

The command searches literal text within parent models, configuration IDs,
component types, manufacturers and Model / Description cells. It reports each
source row independently, including quantity and report page. A named component
does not become an alias for its parent server or a separate target-manufactured
device. Wildcard characters are literal. `--limit 0` returns counts only and
`--limit -1` returns all rows.

`--vendor` requires a target-company association on the parent **source record**.
It does not filter or infer component manufacturers, and cannot borrow company
roles from another record for the same device. JSON/CSV include the parent
company roles, record ID, source URL/tier/hash, capture method and original-file
availability. Table output is compact. Reads preserve the database and caller
transactions; missing database paths are not created.

## CubScout coverage

The selected factual extract of
[Sporton report FR181018AR](https://fcc.report/FCC-ID/2AVPL-CUBSCOUT/5661528.pdf),
issued January 19, 2022, contains cover fields from page 1 and 30 component rows
across configurations 1–3 on pages 6–7. Page numbers are report page references
from web-tool text. Original HTML, test-report PDF and label-PDF downloads all
returned HTTP 403 and were not retried. No original PDF is archived and no
rendered-page inspection is claimed. The saved source is explicitly a selected
factual extract, not the report itself; FCC PDF/detail coverage remains unchanged.

For example, the reported Samsung PM883 SSD quantity is two in configuration 2
and one in configuration 3. Their PSU cells also differ. These rows cannot be
combined into one default CubScout specification or used to promise the contents
of a particular auction unit.

The `component_model` output retains the source's combined Model / Description
cell; it may contain a generic component type, capacity or specification rather
than a validated orderable part number. Text typography has not been visually
verified. The report names Amazon Data Services as applicant and Foxconn
Interconnect Technology Limited Taiwan Branch as manufacturer. The source's
earlier ADS `fcc_grantee` affiliation is corrected to `regulatory_applicant`;
separate FCC application records retain their grantee roles.

No CPU family alias, component interchangeability, boot access, firmware
entitlement, current certification, physical factory or unit history is inferred.

## Plain SQLite queries

Data remains in the existing `facts` table as one versioned
`tested_configurations` JSON value, linked to its evidence record. No extension,
server or new database schema is needed. SQLite's JSON functions can expand it:

```sql
SELECT d.model AS parent_model,
       json_extract(cfg.value,'$.configuration_id') AS configuration,
       json_extract(cfg.value,'$.source_page') AS report_page,
       json_extract(part.value,'$.manufacturer') AS component_manufacturer,
       json_extract(part.value,'$.model') AS model_or_description,
       json_extract(part.value,'$.quantity') AS quantity,
       r.source_url
FROM facts f
JOIN records r ON r.id=f.record_id
JOIN devices d ON d.id=r.device_id,
     json_each(f.value,'$.configurations') cfg,
     json_each(cfg.value,'$.components') part
WHERE f.name='tested_configurations'
ORDER BY r.id, cfg.key, part.key;
```

The reader rejects malformed provenance, duplicate configuration IDs, invalid
pages, missing component fields and non-positive or Boolean quantities instead
of silently dropping data. Tests cover differing quantities, company-record
isolation, source-capture limits, literal searches, pagination, malformed input,
read-only CLI output and caller transactions. Saved exports:
`data/exports/cubscout-tested-components.json` and `.csv`.
