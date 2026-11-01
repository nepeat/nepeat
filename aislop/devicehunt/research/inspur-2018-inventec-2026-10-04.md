# Enterprise hardware from the Inspur 2018 catalog and Inventec brochures

Reviewed 2026-10-04 for the revised liquidation-hunting goal. This batch has 93
source records: 91 Inspur hardware identities and two Inventec platforms. Five
Inspur identities already existed, so the batch adds 88 identities (86 + 2).
All new affiliations are brand/platform roles. Manufacturer counts do not change.

## Sources and reproduction

- [Inspur 2018 Chinese product catalog](https://en.inspur.com/eportal/fileDir/lcjtww/resource/cms/article/2448179/2448543/2019032710162840541.pdf):
  50 PDF pages; most are two printed pages per spread. Copyright 2018 and PDF
  metadata 2018-08-07 establish document context, not individual release dates.
- [Inventec Artemis II Rack brochure](https://ebg.inventec.com/en/tool-download/product_files/file/206?note=):
  two pages, copyright 2025, PDF metadata 2025-08-15.
- [Inventec X680m brochure](https://ebg.inventec.com/en/tool-download/product_files/file/136?note=):
  two pages, copyright 2024, PDF metadata 2024-05-15.

Original captures and request hashes remain under
`data/raw/inspur-2018-inventec-2026-10-04/`. The complete reference collection is
`data/raw/reference-documents-with-inspur-2018-inventec-2026-10-04-v2.json`.
The archive importer requires sibling filenames, so byte-identical PDF/page-text
copies are beside this manifest. The rejected nested-path v1 bundle and untouched
first candidate remain historical artifacts; neither is authoritative.

Curated input: `data/inspur-2018-inventec-facts-2026-10-04.json`.
Every record cites one or more PDF pages and retains the original source URL.
`research-outcomes.json` lists the selected models and exclusions. The offline
rebuild uses hashed inputs and does not rerun the one-off research scripts.

## Practical liquidation details

| Model | Source detail that helps identify or evaluate an auction unit |
| --- | --- |
| OB5161M4 / OB5162M5 | Separate Open19 full-width storage / half-width compute nodes; power comes from an Open19 power shelf. PDF p15. |
| ON5263M5 | Compute node uses centralized rack bus-bar power. PDF p14. |
| SN3116J4 | 1U JBOD node, 12V DC through a power clip; separate from the adjacent unnamed SAS switch. PDF p13. |
| NS5482JD | i48 JBOD node explicitly requires another compute node. PDF p8. |
| GX4 | GPU expansion enclosure used with a dual-CPU head node. PDF p18. |
| FS5900 | 1U Fibre Channel switch; AC 85–264V, 48W typical / 57W at the stated maximum configuration. PDF p30. |
| FS6500/6600 | Printed model grouping retained; separate port increments and power figures remain in the source fact. PDF p31. |
| 1010S / 1010E | Historical InCloud Rail 2U1-node appliances. Table lists several input-power options; installed PSU variant remains unknown. PDF p47. |
| 1020S / 1020E | Historical InCloud Rail 2U4-node appliances; table lists 200–240V, 50–60Hz. PDF p47. |
| Artemis II Rack | Liquid-cooled GB300 NVL72 rack with 18 compute trays and nine NVLink switch trays; L2L or L2A cooling. Brochure pp1–2. |
| X680m | Full-height, three-quarter-length, single-width PCIe Gen5 x16 CXL accelerator; two M.2 22110 slots. Brochure pp1–2. |

These are dated platform options, not verified auction-unit configurations,
current firmware availability, software entitlement, seller inventory or prices.

## Identity and evidence boundaries

- i48 and its nodes, I9000 and its blades, I8020 and NX5440M5, SR racks and their
  nodes, and i24/NS5162M5 remain separate identities. No chassis/node aliases.
- NF5288M5 is explicitly paired with AGX-2 on PDF pp16 and 33. That is the only
  new source-scoped alias. It does not connect unrelated ordering-code suffixes.
- `AS13000-M1/M3/M5/Rack` and `FS6500/6600` remain grouped source names. No
  AS13000-M3 or FS6600 identifier is manufactured by expanding the shorthand.
- Printed prefixes/spaces remain in `天梭 M13`, `天梭 TS860G3`, `天梭 TS860M5`,
  `SSC 1900`, `SSC 2900` and `SSC 3900`. General search finds partial model strings;
  exact-model queries require the recorded spelling.
- The 2018 `1010S` is not an alias for the later `1010S-M5`, and `NF5280M5-V`
  is not an alias for NF5280M5.
- Software-only names (DPS-M1, BCP, ClusterEngine, InCloud OS, KDB), unnamed
  expansion modules and third-party parts do not become curated hardware.
- PDF p19 explicitly describes joint Inspur/Baidu development of the SR-AI rack.
  One `documented_design_lineage` assertion applies to the rack. It establishes
  neither deployment nor an individual surplus unit's origin. Separate node/box
  records receive no inherited Baidu assertion.
- Cloud-backup interoperability with AWS, software compatibility with Oracle,
  or the Inventec corporate manufacturing boilerplate creates no extra company
  or per-model manufacturer assertions.

Unresolved source conflicts are stored explicitly: M13's 48TB introduction versus
24T table; NF5288M5's platinum versus titanium PSU descriptions; X680m's page-one
64GB memory statement versus page-two 8GB statement. X680m's separate ARM HPS
memory row is retained independently. No capacity or PSU configuration is guessed.

## Local queries

Run from the project directory, using the bundled Python runtime if system Python
is unavailable. Use all associations to include brand/platform evidence.

```sh
python3 -m devicehunt vendor-inventory Inspur --scope all --limit -1
python3 -m devicehunt specs OB5162M5 --vendor Inspur --exact-model
python3 -m devicehunt specs SN3116J4 --vendor Inspur --exact-model
python3 -m devicehunt specs AGX-2 --vendor Inspur --exact-model
python3 -m devicehunt documents X680m
```

This deliberately illustrative title tests the offline auction-text matcher; it
is not a collected listing:

```sh
python3 -m devicehunt match-listing --title 'Inspur OB5162M5 bare node untested' --vendor Inspur --with-specs
```

SQL for the three source-specific conflicts:

```sql
SELECT d.brand, d.model, f.name, f.value, r.source_url
FROM facts f JOIN records r ON r.id=f.record_id
JOIN devices d ON d.id=r.device_id
WHERE r.source_id IN ('inspur-products-2018-reviewed','inventec-x680m-2024')
  AND (f.name LIKE '%memory%' OR f.name='specification_conflict')
  AND d.model IN ('天梭 M13','NF5288M5','X680m')
ORDER BY d.model, f.name;
```

## Remaining coverage

ByteDance B4020/B4010 still lack qualifying external ByteDance affiliation evidence.
The original ten owned devices remain the separate 9-document + 1-label baseline.
The Inspur document contains more detailed specifications than this curated pass;
unselected text is locally searchable but not converted into assertions. InCloud
Rail NIC tables, older Hyve/Wiwynn systems and original-vendor product families
remain useful next work. Historical source coverage and current marketplace stock
are not complete.
