# Celestica storage manufacturer evidence

Reviewed September 27, 2026. Celestica's original product pages explicitly say
that it designs and manufactures SC6100, SD4100 and SD6200. These were already
catalog identities, but their prior records established brand and listing-holder
associations only. Three additional evidence records now include a manufacturer
claim, raising Celestica's manufacturer inventory from 190 to 193 identities.

| Exact model | Original product page | Source-stated chassis description |
|---|---|---|
| SC6100 | [Celestica SC6100](https://cls.celestica.com/hardware-platforms/sc6100/) | 2U, 24 SSD, high-availability all-flash PCIe Gen 5 storage controller |
| SD4100 | [Celestica SD4100](https://cls.celestica.com/hardware-platforms/sd4100/) | 2U, 12 HDD, SAS-4 storage expansion system |
| SD6200 | [Celestica SD6200](https://cls.celestica.com/hardware-platforms/sd6200/) | 4U, 90 HDD, dense SAS-4 storage expansion system |

Each statement belongs to its model-specific page. Original HTML, retrieval URLs,
HTTP status, timestamps, SHA-256 hashes and reviewed text are preserved under
`data/raw/celestica-storage-manufacturer/`. The checked title, first model heading,
second product-description heading and manufacturer paragraph identify the scope.
The source tier is `vendor`; the company is the printed name Celestica. This does
not identify a subsidiary, a particular factory, or the provenance of an auction
unit. Chassis descriptions are platform specifications, not installed-drive counts.

SD6200's hyperscale-workload marketing is not evidence of deployment at a named
hyperscaler. No hyperscaler evidence, SKU, alias, suffix interpretation, or new
device identity was added. Existing records remain intact.

## Offline queries

```sh
python3 -m devicehunt vendor-inventory --vendor Celestica --scope manufacturer --limit -1
python3 -m devicehunt lookup SC6100
python3 -m devicehunt specs SD6200 --exact-model --vendor Celestica
python3 -m devicehunt match-listing --title 'Celestica SD4100 empty chassis' --vendor Celestica --format json
```

```sql
SELECT d.brand, d.model, a.company, a.role, r.source_url,
       f.value AS manufacturer_statement
FROM records r
JOIN devices d ON d.id = r.device_id
JOIN affiliations a ON a.record_id = r.id
JOIN facts f ON f.record_id = r.id
WHERE r.source_id GLOB 'celestica-storage-manufacturer-*'
  AND a.role = 'manufacturer'
  AND f.name = 'manufacturer_statement'
ORDER BY d.model;
```

Replay uses the existing reviewed-data importer:

```sh
python3 -m devicehunt import curated data/celestica-storage-manufacturer-reviewed.json
```

The import manifest hashes the review file and all five associated inputs. It
reproduces the three additional records during a complete offline rebuild.
Search audits check exact-model specifications, source-linked auction-title
mentions and unknown-suffix handling. A title such as `SD6200-UNPROVEN` stays a
variant lead; it does not become an exact model match or a supported ordering code.

The owner baseline remains 10/10 searchable and 9/10 corroborated. The complete
`7760-32X-A-AC-F` label still lacks non-marketplace corroboration; this addition
does not close that gap or establish a complete worldwide inventory.
