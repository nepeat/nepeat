# Celestica FCC discovery supplement

The public [FCCID.io Celestica Incorporated index](https://fccid.io/MRF) exposes
two rows absent from the previously cached FCCID.co company query. They are now
stored in `fcc_id_index` under the separate source `fcc-index-reviewed-mrf`.

| FCC ID | Displayed date | Product description | Displayed purpose |
|---|---|---|---|
| MRF13316C2 | 1997-09-22 | Wireless LAN Card Maximum Output Power: 153.5 mW | Class II Permissive Change |
| MRF13316C | 1997-05-16 | WLAN PCMCIA Card Maximum output power: 58 mW | Class II Permissive Change |

The source is a **reviewed factual web-tool table extract**, not original HTML,
an official FCC download or a complete historical filing list. The capture is
`data/raw/fcc-reviewed-index/mrf.json`; its SHA-256 is checked by the associated
bundle and rebuild manifest. Grantee code/name agree with the reviewed target
registration mapping. The suffixes `13316C` and `13316C2` are FCC product codes,
not inferred device model names.

Equipment class is absent from the captured company table and stays blank. The
description, power text, displayed purpose and date remain verbatim mirror
claims. The importer does not reinterpret the date as an independently verified
grant/application date or create a new detailed application record.

The [MRF13316C detail page](https://fccid.io/MRF13316C) was separately accessible;
it displays a DSS class and a copied grant/application form. Those fields have
not been mixed into the company-table extract. The MRF13316C2 detail retrieval
returned a cache miss, and the NVN company page was unavailable through the web
tool. Neither response establishes zero filings. No private or gated source was
used to fill missing fields.

## Local use

```sh
python3 -m devicehunt lookup MRF13316C
python3 -m devicehunt lookup 'Celestica Incorporated'
python3 -m devicehunt import fcc-reviewed-index data/raw/fcc-reviewed-index/bundle.json
```

```sql
SELECT source_id, fcc_id, grantee_name, listed_date, product_description,
       equipment_class, application_purpose, source_url
FROM fcc_id_index WHERE grantee_code = 'MRF' ORDER BY source_id,fcc_id;
```

The earlier `fcc-index-mrf` source remains at zero rows with its explicit
not-found-in-mirror scope. The supplement records two visible rows from another
mirror. This preserves the disagreement rather than overwriting source history.
The discovery index now has 173 rows. No manufacturer, factory, operator, model
alias, detailed application or exhibit record is inferred from the supplement.

## Parser and dependent collectors

The importer checks capture method, source URL, registered grantee name/code,
review date, three-line row shape, displayed date syntax, reviewed row count,
unique FCC IDs and dependency hashes before mutation. Refresh replaces only
the corresponding reviewed source and leaves other mirrors intact.

The FCCID.co detail importer and its download helper now select only their own
original company-index sources. An additional mirror's rows cannot overwrite
an ID's detail URL or force that parser to consume another site's layout. The
new regression case covers an overlapping FCC ID as well as a new grantee row.

Four new tests bring the suite to 247 passes. Coverage includes mirror
disagreement, absent equipment class, no inferred model/grant records, malformed
captures and hashes, scoped refresh and isolation of the existing detail parser.
The owned-device baseline remains 10/10 searchable and 8/10 corroborated.
