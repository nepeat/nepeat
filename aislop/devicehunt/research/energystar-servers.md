# ENERGY STAR enterprise servers

The [EPA enterprise-server dataset](https://data.energystar.gov/d/qifb-fcj2)
is now available locally as `energystar_servers`: **495 records across all
brands**, with all **196 original fields** preserved in `raw_json`. It is a
snapshot of the published dataset, not all historically qualified servers or
all devices made by its participants. The [official catalog entry](https://catalog.data.gov/dataset/energy-star-certified-enterprise-servers)
provides its public download links.

Original CSV and metadata before/after download are retained in
`data/raw/energystar-servers/`. Both metadata captures agree on the column
definitions, data-update timestamp (`rowsUpdatedAt=1790428612`), and 495 non-null
primary IDs. The importer verifies dependency hashes, exact headers, row width,
unique nonempty IDs, required identity fields and total count before replacing
the previous source. A changed or incomplete capture is rejected. These checks
establish completeness against this published snapshot's metadata, not against
an independent inventory of every server.

```sh
python3 -m devicehunt import energystar-servers data/raw/energystar-servers/bundle.json
python3 -m devicehunt lookup 'EPYC 9J25'
python3 -m devicehunt specs X10M --exact-model
```

Only three rows have a recognized target brand: Oracle `X9-2L`, `X10M` and
`X11M`. These become three exact model assertions, with separate **brand** and
**program_partner** roles. Neither role establishes the physical manufacturer
or previous operator of an auction unit. Longer names already in the catalog
stay separate; related names are not automatically merged into the shorter
ENERGY STAR labels. Model/configuration IDs and PSU part numbers remain facts,
not alternate identities. Placeholder model patterns are retained raw only.

## Power measurements

The source distinguishes low-end, typical/single, and high-end test
configurations. They remain separate in SQL and in specification search.

| Printed model | Low-end idle W | Typical/single idle W | High-end idle W |
|---|---:|---:|---:|
| X9-2L | 148.90 | 204.80 | 485.80 |
| X10M | 304.10 | 217.50 | 231.90 |
| X11M | 217.50 | 210.20 | 191.00 |

All three records report 1,400 W PSU rated output for each configuration.
That rating is separate from measured system consumption. Idle values describe
the reported test configurations, not expected consumption of an auction unit.
The source's SERT total power **scores** remain raw fields and are not presented
as instantaneous wattage. Processor, installed memory, CPU count, PSU count,
PSU model and configuration identifiers remain available alongside measurements.

```sql
SELECT brand, model_number, partner, source_url,
       json_extract(raw_json,
         '$."SERT Idle Measurement (Watts) Typical or Single Configuration"')
         AS typical_idle_watts,
       json_extract(raw_json,
         '$."Processor Model Name Typical or Single Configuration"')
         AS typical_processor,
       json_extract(raw_json,
         '$."Power Supply Rated Output Typical or Single Configuration (W)"')
         AS typical_psu_rated_output_watts
FROM energystar_servers
WHERE brand = 'Oracle'
ORDER BY model_number;
```

All-brand exports: `data/exports/energystar-server-register.csv` and `.json`.
Target facts: `data/exports/energystar-oracle-facts.csv` and `.json`.
Import audit: `data/exports/energystar-import-audit.json`.
The source is included in the hashed offline rebuild manifest and unified
lookup. Schema version 18 adds its raw table; read-only commands never migrate
older databases.

The [official partners page](https://www.energystar.gov/products/enterprise_servers/partners)
also links archived version 2 and version 1.1 qualification spreadsheets.
Both historical files are now imported separately, with 873 qualification rows
and their original configuration layouts preserved. See
[historical archive scope and SQL](energystar-server-archives.md).
The owned-device baseline remains 10/10 searchable and 8/10 corroborated;
the exact `7760-32X-A-AC-F` and bare `AZ4444-52X` gaps remain open.
