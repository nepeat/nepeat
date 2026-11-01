# PICOS Edgecore/Accton hardware table

The original public HTML of Pica8's [EdgeCore/Accton Switch table in its PICOS 4.5 guide](https://pica8-fs.atlassian.net/wiki/spaces/PicOS45/pages/258605211/EdgeCore+Accton+Switch)
was captured on 2026-09-27 UTC. The table has 26 rows with Product, Model,
Configuration, Switch ASIC and CPU columns. Every row is imported, including
nine parenthetical DCS/EPS codes. Those codes are explicit same-row names,
not equivalents inferred from similar specifications or model families.

The hashed original HTML and bundle live under `data/raw/picos45/`. Importer
`picos-hardware` checks the page title, heading, sole renderer table, headers,
cell spans, captured row count and complete model-cell syntax before replacing
the source. Navigation/sidebar names do not create records. New or ambiguous
model qualifiers, duplicate models, changed headers and incomplete captures fail
before mutation.

## Evidence scope

The joint EdgeCore/Accton heading establishes software-vendor partner context
for both named companies. Records use an Edge-Core identity namespace and
`compatibility_partner` affiliations; no manufacturer, retail-brand, physical
factory or hyperscaler deployment role is inferred. Original portfolio, model
cell, ASIC, CPU and port text remain row-scoped facts. The spelling `Trident 3×7`
is preserved without silently normalizing it to another chip name.

The containing guide is PICOS 4.5; its hardware table does not establish a
per-release support matrix, required firmware, available download or transferable
license. Model names missing from this table are not proven unsupported. The
snapshot describes documentation, not the configuration or provenance of a
particular auction unit.

Twenty-six assertions add two catalog identities in this namespace:
`AS7312-54XS` and `AS9737-32DB`. Other-brand records are not merged.
These counts describe new identity rows, not newly discovered physical designs:
existing commercial-name records may already carry these strings as aliases.
For example, the exact AS9737-32DB specifications query also returns the existing
vendor-sourced DCS511 record through its independently documented alias.
The ASIC/CPU facts can help compare listings: the table describes AS9737-32DB with Tomahawk 4
and Intel x86, and AIS800-32O with Tomahawk 5 and Intel x86. No live purchase,
price or availability claim is made.

## Local use

```sh
python3 -m devicehunt import picos-hardware data/raw/picos45/bundle.json
python3 -m devicehunt search 'Tomahawk 5' --vendor Edge-Core --role compatibility_partner
python3 -m devicehunt lookup AS9737-32DB
python3 -m devicehunt specs AS9737-32DB --exact-model
```

```sql
SELECT d.model, f.name, f.value, r.source_url
FROM records r JOIN devices d ON d.id=r.device_id
JOIN facts f ON f.record_id=r.id
WHERE r.source_id='picos45-edgecore-accton'
  AND f.name IN ('compatibility_ports','compatibility_asic','compatibility_cpu')
ORDER BY d.model, f.name;
```

`data/exports/picos45-platforms.csv` and `.json` provide the complete row table;
`picos45-import-audit.json` records additions. Saved example lookups and an exact
model software-specification result are in the same export directory. The
importer uses only the Python standard library and replays offline.

## Owned-label follow-up

Neither owned-label gap occurs in this table. The new exact-code searches again
returned seller descriptions for `7760-32X-A-AC-F`, and regulatory sources for
suffixed AZ4444 names. The separate UL BR4384 search result names Openglobe as
listing holder and only suffixed AZ4444 entries; it does not establish the bare
Edge-Core label. Those leads were not converted into aliases or additional
manufacturer claims. The baseline remains 10/10 searchable and 8/10 corroborated.
