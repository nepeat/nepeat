# Celestica DS4000/DS4001 chassis ordering codes

Eight full chassis SKUs are now linked to the existing DS4000 and DS4001 model
families using Celestica's [original installation guide](https://www.celestica.com/uploadedFiles/Site/our-expertise/hardware-platform-solutions/celestica-documentation-portal/DS4000_Installation_Guide.pdf).
The original 49-page PDF is archived locally. Physical pages 7 and 8 (printed
pages 4 and 5) were rendered and visually reviewed against the extracted tables.

| Model family | Complete chassis SKU | Printed airflow token | Physical PDF page |
|---|---|---|---:|
| DS4000 | R4023-F9102-A0 | F2B | 7 |
| DS4000 | R4023-F9103-A0 | F2B | 7 |
| DS4000 | R4023-F9104-A0 | F2B | 7 |
| DS4000 | R4023-F9003-A0 | B2F | 7 |
| DS4000 | R4023-F9004-A0 | B2F | 7 |
| DS4000 | R4023-F9005-A0 | B2F | 8 |
| DS4001 | R3240-F9102-A0 | F2B | 8 |
| DS4001 | R3240-F9103-A0 | B2F | 8 |

These are eight ordering configurations, not eight newly discovered model
families. Each SKU has its own source record under
`celestica-ds4000-chassis-skus`, a `part_number` identifier, the complete printed
configuration, and a page URL. The source supports the Celestica brand, not an
independent physical-factory or hyperscaler claim. No relationship to the owner's
AWS hardware is inferred.

The DS4000 table continues onto the next page: R4023-F9005-A0 remains a chassis
row because its description explicitly names the DS4000 assembly. The power
cables, console cable and brackets in the same tables are excluded from chassis
identifiers. They remain discoverable in the full PDF archive.

Power/cooling facts repeat the full SKU alongside its configuration so six
different DS4000 configurations cannot be mistaken for one unit's specifications.
PSU ratings are not measured consumption, and an auction unit may have different
installed components. The page does not establish current stock or firmware
entitlement. SKU suffixes are never decoded into features by convention.

```sh
python3 -m devicehunt lookup R4023-F9102-A0
python3 -m devicehunt specs R4023-F9102-A0 --exact-part-number --format json
python3 -m devicehunt match-listing --title 'Celestica R3240-F9103-A0 used switch'
python3 -m devicehunt documents R4023-F9005-A0 --source reference-pdf-celestica-ds4000-guide
```

The `specs --exact-part-number` mode returns categorized statements from the
same records that identify the complete SKU. It retains original matching
identifier strings, record IDs, revisions, source URLs and evidence tiers.
Vendor filtering also applies on those records. It does not borrow statements
from another configuration or fill missing specifications with family-level
claims. A plain model search still provides broader family context. For the
uncategorized full configuration field, use SQL:

```sql
SELECT d.model, r.source_key AS chassis_sku, f.value AS configuration, r.source_url
FROM records r
JOIN devices d ON d.id = r.device_id
JOIN facts f ON f.record_id = r.id
WHERE r.source_id = 'celestica-ds4000-chassis-skus'
  AND r.source_key = 'R4023-F9102-A0'
  AND f.name = 'ordering_configuration';
```

The reviewed input is `data/celestica-chassis-skus-reviewed.json`; the original
PDF and page text are covered by the offline manifest. Re-extraction of all
eight table rows agrees with the reviewed cells. Local lookup and auction-title
matching resolve each complete SKU to the documented family and exact source
record; adding an unknown suffix remains a variant lead. This is label matching,
not proof that a seller's unit has the published configuration.

The targeted 7760-32X-A-AC-F search again returned seller descriptions. It did not
produce qualifying independent exact-code evidence, and the baseline remains
9/10 corroborated. No AS7760 family alias was added to change that result.
