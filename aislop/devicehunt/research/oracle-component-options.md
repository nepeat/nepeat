# Oracle ZFS component part numbers

Three original Oracle service-manual pages are cached in
`data/raw/oracle-zfs-components`. The `oracle-components` importer reads their
NIC/HBA Options tables: [Sun 7420](https://docs.oracle.com/en/storage/zfs-storage/zfs-appliance/os8-8-x/service-manual/sun-zfs-storage-7420-pcie-io-cards.html),
[ZS3-2](https://docs.oracle.com/en/storage/zfs-storage/zfs-appliance/os8-8-x/service-manual/oracle-zfs-storage-zs3-2-pcie-io-cards.html),
and [ZS3-4](https://docs.oracle.com/en/storage/zfs-storage/zfs-appliance/os8-8-x/service-manual/oracle-zfs-storage-zs3-4-pcie-io-cards.html).
Original HTML and a checksummed bundle are retained; no login is required.

The tables provide **30 assertions for 15 marketing part numbers**, adding 14
previously absent Oracle identities. Each source record preserves the system,
description, row number, paragraph position, raw cells and exact table link.
Oracle has the `documentation_vendor` role; this is an Oracle documentation
namespace, not a claim that Oracle fabricated the components. No hyperscaler
operator or factory is inferred from documentation or compatibility.

The 7420 table explicitly maps nine marketing parts to manufacturing part
numbers. These use the distinct `oracle_mfg_part_number` identifier kind and
remain attached to the same source record. They are searchable but are not
global aliases, independent catalog identities or inventory-validation identity
fields. The two other selected tables do not supply that column, so mappings
are not propagated into them. Slot-assignment tables on the same pages are
outside this importer; slot placement and maximum counts are not inferred.

For example, the 7420 option row pairs `SG-XPCIE2FC-QF8-Z` with `371-4325-02`.
The marketing part is described as a two-port 8 Gb FC HBA on all three pages.
This follows the identifier found in the historical NTRA register while retaining
the independent Oracle documentation source.

## Source disagreement retained

The ZS3-2 table places two separate paragraphs in one row. Paragraph order pairs
`7103791` with a SAS-3 12 Gb/s description and `7110372` with a SAS-2 6 Gb/s
description. The ZS3-4 page describes `7103791` as SAS-2 6 Gb/s. Both statements
are retained per source and system, without choosing one, repairing a possible
documentation error, or concluding that revisions are interchangeable. Compare
the original table before making a compatibility decision. No live availability
or current support guarantee is established by these snapshots.

## Queries

```sh
python3 -m devicehunt lookup SG-XPCIE2FC-QF8-Z
python3 -m devicehunt search 371-4325-02 --vendor Oracle
python3 -m devicehunt match-listing --title 'Sun Oracle SG-XPCIE2FC-QF8-Z FC HBA' --vendor Oracle
```

The listing matcher excludes purely numeric identifiers by default. Enable
`--include-numeric-identifiers` for the seven-digit marketing parts in this
collection, or use `lookup` and SQL. Printed manufacturing-part mappings remain
discovery fields and are not promoted to listing-match aliases by that option.
Do not interpret an absent listing match as absent catalog evidence.

```sql
SELECT d.model AS marketing_part_number, i.value AS printed_mfg_part_number,
       r.source_id, r.source_url
FROM records r
JOIN devices d ON d.id = r.device_id
JOIN identifiers i ON i.record_id = r.id
WHERE i.kind = 'oracle_mfg_part_number'
ORDER BY d.model, r.source_id;

SELECT model, name, value, source_url
FROM device_facts
WHERE model = '7103791'
  AND name IN ('documented_component_description', 'documented_system_option')
ORDER BY source_id, name;
```

`data/exports/oracle-component-options.json` retains all 30 assertions.
The importer checks all three captures before changing catalog evidence. Page
heading, exact table identity, headers, row counts, paragraph pairing, part
syntax and duplicate parts are checked. Unexpected merged cells or unparsed
text fail closed. Five tests cover mapping scope, differing descriptions,
multi-paragraph rows, ignored unrelated tables, malformed captures, dependency
checks and repeat imports.
