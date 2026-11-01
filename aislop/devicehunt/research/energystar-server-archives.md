# Historical ENERGY STAR server qualification lists

Two original workbooks linked from the [official server partners page](https://www.energystar.gov/products/enterprise_servers/partners)
are now collected locally:

| Archive | Published scope | Qualification rows |
|---|---|---:|
| [Version 2 XLSX](https://www.energystar.gov/sites/default/files/asset/document/EnterpriseServers_V2_Archived.xlsx) | Last list before the June 17, 2019 specification change | 279 |
| [Version 1.1 XLS](https://www.energystar.gov/sites/default/files/asset/document/EnterpriseServers_V1.1_Archived.xls) | Last list before the December 16, 2013 specification change | 594 |

The older workbook contains 148 single-configuration rows and 446
family-configuration rows. These are **873 qualification rows**, not 873 unique
physical models. All brands, repeated configurations, original sheet/row
locations and fields are retained in `energystar_server_archives`. Blank rows
are excluded; definition/criteria sheets remain in the complete cell extraction
and original workbook. This is coverage of these two published files, not every
historical qualification or manufactured server.

The version 2 archive contributes seven Oracle assertions: Oracle Server
X6-2, X6-2L, X7-2, X7-2L, X8-2, X8-2L, and the printed `X5-4`.
The version 1.1 archive contributes eight Oracle program-partner assertions
across three printed Sun Fire models: X4170 M2, X4270 M2 and X4470. The
family sheet's **Model Name** supplies the catalog model; its **Model Number**
and minimum/maximum configuration IDs remain row-specific facts. The printed
brand `Sun Fire` stays separate from Oracle's program-partner role. This source
does not establish the physical manufacturer, factory or previous operator.
Existing names in other brand namespaces are not automatically merged.

## Search and SQL

```sh
python3 -m devicehunt lookup 'Sun Fire X4170 M2'
python3 -m devicehunt specs 'Sun Fire X4170 M2' --exact-model
python3 -m devicehunt import energystar-server-archive data/raw/energystar-servers-archive/v2-bundle.json
python3 -m devicehunt import energystar-server-archive data/raw/energystar-servers-archive/v1_1-bundle.json
```

```sql
SELECT source_id, sheet, row_number, partner, brand, model_name, model_number,
       json_extract(fields_json,
         '$."Idle Power Draw @ 230V (W) | Minimum Config."') AS min_idle_230v_w,
       json_extract(fields_json,
         '$."Idle Power Draw @ 230V (W) | Maximum Config."') AS max_idle_230v_w,
       source_url
FROM energystar_server_archives
WHERE partner = 'Oracle Corporation'
ORDER BY source_id, sheet, row_number;
```

For example, version 1.1 family-sheet row 434 lists Sun Fire X4170 M2,
Energy Star Family 1: 161 W minimum-configuration and 229 W maximum-configuration
idle power under the 230 V column headings. Its full-load values are 345 W
and 511 W, with a separate printed test supply of **220 V / 60 Hz**. The source
also lists 760 W PSU output. These are different measurements/ratings, not one
universal consumption estimate. The apparent 230 V versus 220 V source wording
is preserved rather than silently corrected.

Configuration, voltage and family label remain attached to catalog power
statements. `N/A` values and power allowance/efficiency/score fields are not
converted into measured consumption. Low-end, minimum-power, typical,
maximum-power and high-end configurations in the later file retain their
original field names. Duplicate idle fields remain distinct source statements.

## Reproducibility and limits

Original workbooks, the source index, complete stored-cell extractions and
bundles are under `data/raw/energystar-servers-archive/`. The XLSX extraction
uses openpyxl 3.1.5 and rejects formula/error cells; XLS uses xlrd 2.0.2 stored
values. No macros are run. Date cells retain interpreted ISO values, and XLS
dates additionally retain the original Excel serial and date system. The
original workbook preserves the source formatting. SQL `fields_json` contains
resolved text fields; `cells_json` contains the original extracted cell array,
including date objects. The date columns are extracted source values, not
current-validity claims.

The importer checks workbook/extraction/index hashes, extraction-to-workbook
binding, extraction method, sheet presence, fixed widths, reviewed header
hashes, required identity fields, per-sheet row counts, and unique version 2
PD_ID values before replacing its own source. It resolves multi-level headers
only using explicit merged cells. Model patterns are not expanded; additional
model lists, configuration IDs and components do not become aliases.

`research/extract_energystar_archives.py` reproduces the cell extractions with
the optional workbook packages. Normal import, search and offline rebuild use
the hashed extractions and standard library only. The row counts are reviewed
workbook counts; no independent registry total is available for these archives.

Exports: `data/exports/energystar-server-archives.csv` and `.json`,
`energystar-archive-target-facts.csv` and `.json`, and
`energystar-archive-import-audit.json`. Schema 19 adds the archive table, and
unified lookup now searches 21 datasets. This expansion does not close either
owned-hardware exact-label gap.
