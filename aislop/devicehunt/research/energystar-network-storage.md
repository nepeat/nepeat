# ENERGY STAR network and storage registers

Five complete published files are now searchable in `energystar_datacenter`:

| Dataset kind | Published file | Rows | Original fields |
|---|---|---:|---:|
| `network` | Current large network equipment CSV | 96 | 223 |
| `storage-file` | Current file-I/O storage CSV | 35 | 144 |
| `storage-block` | Current block-I/O storage CSV | 83 | 114 |
| `storage-file-v1` | Historical version 1 file-I/O storage workbook | 9 | 282 |
| `storage-block-v1` | Historical version 1 block-I/O storage workbook | 229 | 289 |

The official catalog links the public [network export](https://catalog.data.gov/dataset/energy-star-certified-large-network-equipment),
[file-I/O export](https://catalog.data.gov/dataset/energy-star-certified-data-center-storage-file-i-o)
and [block-I/O export](https://catalog.data.gov/dataset/energy-star-certified-data-center-storage-block-i-o).
The [storage partners page](https://www.energystar.gov/products/data_center_storage/partners)
links the two historical workbooks as the last lists before the March 15, 2021
specification change. These five files total **452 qualification rows**, not
452 unique physical models.

## Coverage finding

None of these snapshots names Accton, Edge-Core, Celestica, Oracle or Amazon
Data Services in the partner/brand fields; a supplementary literal scan of all
exported field values also found no target-company names. Consequently they add
**zero target catalog assertions**. A complete import with zero target matches
is recorded as a coverage result, not a failed import or an inferred OEM link.

The all-brand data remains useful for searching a surplus device's model,
regulatory label, controller, module, firmware or configuration details.
For example, `E21W` occurs in several Dell qualification rows. Their IDs and
full configurations stay separate; the catalog does not collapse them into one
physical design or infer a retail-model alias.

```sh
python3 -m devicehunt lookup E21W
python3 -m devicehunt lookup VDS2249R
python3 -m devicehunt import energystar-datacenter data/raw/energystar-network/bundle.json
python3 -m devicehunt import energystar-datacenter data/raw/energystar-storage-file/bundle.json
python3 -m devicehunt import energystar-datacenter data/raw/energystar-storage-block/bundle.json
python3 -m devicehunt import energystar-datacenter data/raw/energystar-storage-archive/file-bundle.json
python3 -m devicehunt import energystar-datacenter data/raw/energystar-storage-archive/block-bundle.json
```

```sql
SELECT dataset_kind, product_id, partner, brand, model_name, model_number,
       model_identifier, source_url
FROM energystar_datacenter
WHERE model_number = 'E21W'
ORDER BY dataset_kind, product_id;

SELECT dataset_kind, count(*) AS qualification_rows
FROM energystar_datacenter GROUP BY dataset_kind;

SELECT model_number,
       json_extract(raw_json, '$."Storage Controller Manufacturer Name"')
         AS controller_manufacturer,
       json_extract(raw_json, '$."Storage Controller Model Number"')
         AS controller_model
FROM energystar_datacenter
WHERE dataset_kind = 'storage-file';
```

Every original field remains in `raw_json`, alongside mapped identity/date
columns. Current CSV row numbers are one-based data-row positions; historical
rows retain the worksheet name and original Excel row number. The `*_date_raw`
columns contain extracted source values, not qualification-validity judgments.
Original workbooks and stored-cell extractions are retained; XLSX dates in
the extraction carry ISO values, while the workbook preserves formatting.

## Identity and measurement limits

The file-I/O CSV formats numeric PD_ID values with thousands separators, such
as `2,382,932`. The raw value remains in JSON. The mapped `product_id` and
detail URL use `2382932`, after checking the source metadata declares a numeric
ID and the commas form valid three-digit groups. Duplicate checks use the
normalized ID, so `2382932` and `2,382,932` cannot silently become different
records in one source. This formatting rule is enabled only for that dataset;
other imports retain strict digit-only ID checks. Models are never subjected
to numeric-ID normalization.

Brand and program partner are separate from hardware manufacturer, factory and
operator. Names in controller, module, PSU or configuration fields do not become
chassis makers or aliases. The importer can catalog a future exact target-brand
row, but no such row is present in these captured files. Grouped model cells and
placeholder templates are not split or expanded.

Storage fields such as **GB/W** or **MiBPS/W** are efficiency ratios, not watts.
Network measurements retain minimum/maximum/typical configurations, full/half
port population and load level in their original field names. No ratio or PSU
rating is presented as measured chassis consumption, and no auction-unit power
estimate is derived from these fields.

## Validation and reproduction

For each current CSV, before/after metadata has matching update timestamps and
column definitions. Exact CSV headers, row widths, non-null primary-ID totals,
unique IDs and required identity fields are checked before source replacement.
The historic files are checked against their original workbook hash, complete
stored-cell extraction, source-index link, sheet/layout, reviewed header hash
and row count. Historical counts come from the workbook, not an independent
registry total. These checks prove coverage of the captured files, not all
historical qualifications or all devices made by the listed companies.

`research/extract_energystar_archives.py` reproduces the two historical XLSX
extractions with optional openpyxl 3.1.5. Import/rebuild and search need only the
standard library. All five sources and their dependencies are included in the
offline rebuild manifest. Schema 20 adds the table; unified lookup covers
22 datasets. All-brand exports are `data/exports/energystar-datacenter-register.csv`
and `.json`; counts are in `energystar-datacenter-import-audit.json`.

This expansion does not close the owned baseline's exact-label gaps for
`7760-32X-A-AC-F` or bare `AZ4444-52X`.
