# IP Infusion OcNOS compatibility snapshot

Captured September 26, 2026 (local date). Source:
[OcNOS HCL](https://www.ipinfusion.com/resources/ocnos-hardware-compatibility-list/)
and its publicly linked [platform API](https://www.ipinfusion.com/wp-json/ipi/v1/platforms).

The old `/wp-content/uploads/OcNOS-HCL-v2.pdf` URL now redirects to the HTML HCL.
Search-index text still describes an October 2022 PDF with older Celestica
platforms. That historical PDF was **not** downloaded or imported. The original
HTML response is saved as HTML, not as a mislabeled PDF.

The API and HTML each contain 44 platforms: 20 Edgecore, 23 UfiSpace, one
Celestica. All 44 identities, commercial names, vendor, NOS variant, ASIC,
capacity, version, form factor and port profiles agree across the two captures.
Only the 21 exact target-vendor entries enter the device catalog. They add
21 source assertions and 13 previously absent brand/model identities; existing
commercial aliases and differently branded records are not merged.

## Evidence scope

- The source is a software vendor's compatibility list. Target companies have
  the `compatibility_partner` role. No physical manufacturer or hyperscaler
  relationship is inferred.
- Commercial aliases explicitly agree between the API's `commercialName` and
  the HTML model heading. Revision/firmware labels remain contextual facts,
  never model aliases or minimum-version requirements.
- `OcNOS 7.0.1`, the listed SKUs, and firmware labels are separate scoped
  assertions. No per-SKU firmware/version matrix or entitlement is inferred.
- Power and weight retain approximation signs. Power has no stated test
  conditions and is not treated as a PSU rating. The source recommends the
  linked hardware datasheets for full mechanical/electrical specifications.
- AS5915-18X has an announcement date of 2026-09-08, end of sale 2027-02-28,
  and end of life 2030-08-28 in the HCL API. These are retained as
  `ocnos_hardware_*` facts with source attribution. The raw API also labels it
  `active`; we do not turn that into a general support claim. An empty lifecycle
  object/list does not imply indefinite support.
- The raw `maxSpd` field sometimes disagrees with the port profile (for example,
  AS5915-18X says `100G` while its ports are 6x10G plus 12x1G). This field is not
  promoted into a normalized device specification. All original API values stay
  available in `records.raw_json`.
- Identical repeated firmware entries in the API remain in the raw record;
  SQL facts deduplicate identical name/value pairs.

## Local use

```sh
python3 -m devicehunt import ocnos-hcl data/raw/ocnos-bundle.json
python3 -m devicehunt search 'OcNOS' --vendor Edge-Core --limit -1
python3 -m devicehunt specs DS1000 --exact-model
python3 -m devicehunt specs CSR200 --exact-model
```

Example SQL (the full assertions export is `data/exports/ocnos-platform-facts.csv`):

```sql
SELECT d.brand, d.model, f.name, f.value, r.source_url
FROM facts f JOIN records r ON r.id = f.record_id
JOIN devices d ON d.id = r.device_id
WHERE r.source_id = 'ipinfusion-ocnos-hcl'
  AND f.name IN ('compatibility_asic', 'compatibility_ports',
                 'compatibility_sku', 'compatibility_firmware_context',
                 'ocnos_hardware_end_of_sale', 'ocnos_hardware_end_of_life')
ORDER BY d.brand, d.model, f.name, f.value;
```

The importer uses only Python's standard library and never executes page
JavaScript. Both inputs have checked hashes; all rows are validated before
the prior source is replaced. Truncation, changed columns, duplicate identities,
API/table disagreement, invalid dates, and malformed flags fail the import.
The owned-hardware baseline remains 10/10 searchable and 8/10 corroborated;
this source does not close either outstanding full-label coverage gap.
