# Inspur hardware and Inventec download follow-up — 2026-10-04

This bounded batch adds 34 identities and 37 source records. All are Inspur
brand/platform associations; no manufacturer, factory, FCC-ID or hyperscaler
deployment assertion is added. The 1,064 raw support-menu entries remain intact
and do not become model assertions automatically. Inventec's two remaining
download-index pages were captured for review; this batch adds no Inventec record.

## Original sources and boundaries

- [InCloud Rail compatibility list](https://partner.inspur.com/eportal/fileDir/lcjtww/resource/cms/2024/03/2024032014450874883.pdf):
  64 original pages, cover printed December 2023. PDF modification metadata is
  March 2024; neither is a product release date. Original SHA-256:
  `9f340f567f5603150a9c82781e1fc34656c3f2177e68b3f259ba842a32d16150`.
  Full embedded text is searchable through `documents`; selected tables were
  visually checked on PDF pages 9, 12, 17, 20 and 51. The document also includes
  third-party cards, disks, guest operating systems and applications. Those are
  not imported as Inspur devices. The table's vendor heading does not establish
  which entity physically manufactured an item.
- [NE5260M5 launch article](https://partner.inspur.com/qudao/2392089/2392123/2025021410101280931/index.html):
  printed article date 2019-02-27; launch event 2019-02-25. The migrated URL and
  current footer do not change those dates. This adds 2U/19-inch/430 mm chassis,
  wall-mount, memory, slot and drive information. The 2019 text describes two
  forthcoming Intel scalable processors without an exact SKU. OTII collaboration
  with telecom companies is design context, not evidence of deployment.
- [Existing i24 technical white paper](https://www.inspur.com/eportal/fileDir/defaultCurSite/resource/cms/2020/08/2020081316525840458.pdf):
  revision 3.0, January 2020, already archived. PDF pages 5 and 8 were visually
  checked. The i24 chassis supports four NS5162M5 compute nodes, sharing power
  and cooling through side-card connections. Each node supports two Xeon
  Scalable processors and up to 16 DDR4 DIMMs. The additional records preserve
  a chassis/node relationship, not an alias or proof of independent rack use.
  Original i24 source records remain unchanged. No new download was necessary.

Nine hyperconverged appliance/server-node mappings from PDF page 9 are stored
as facts on separate identities:

| Appliance model | Server node model | Printed group |
| --- | --- | --- |
| IR5280M7 | NF5280M7 | M7 |
| IR8480M7 | NF8480M7 | M7 |
| IR5280M6 | NF5280M6 | M6 |
| IR5266M6 | NF5266M6 | M6 |
| IR8480M6 | NF8480M6 | M6 |
| IR5468M6 | NF5468M6 | M6 |
| 600S-M5 | NE5260M5 | M5 |
| 4010S-M5 | NF8480M5 | M5 |
| 1010S-M5 | NF5280M5 | M5 |

NF5280M5 was already present. Its first-imported title remains `INSPUR NF5280M5`.
NE5260M5 also gains the separate launch-article statement. These are historical
platform mappings, not proof that an auction unit includes a particular software
license or configuration.

Other selected hardware names:

- SSDs: NS8600G1, NS8500G2, NS8500G1, NS6510 (page 12). U.2 is the interface
  printed in this table. Firmware values and Intel+HG applicability are retained
  as dated source facts, not instructions to flash anything.
- Storage controllers: 3108_2GB, 3108_4GB, 3008IT, 3008IMR, PM8204_2GB under
  section 7.1 Inspur (page 17). The separate LSI section is not relabeled Inspur.
- Switches: SC5630EL, CN9008-48YC-S, **S6550V2-48TQ-AC/D**, S6820-24XQ-E,
  CN61108PC-V-H (page 20). AC/D remains a single printed identifier; no fabricated
  AC or DC suffix variants. The switch-type column is not a complete port map.
  Ruijie rows remain reference text only.
- FPGA accelerators: F10A, F37X (page 51).

The NIC tables span merged cells and include similar Intel/Inspur identifiers.
They remain archived text pending a separate naming/row-scope review. Software
such as Oracle Linux, TencentOS and SQL databases does not establish hardware
ownership or deployment by those companies.

## Inventec pagination

The previously archived first page links to public
[page 2](https://ebg.inventec.com/en/support/download-center?page=2) and
[page 3](https://ebg.inventec.com/en/support/download-center?page=3). Both succeeded,
each with ten download rows. All three displayed pagination destinations have
now been captured; this does not establish a complete historical Inventec archive.

Page 2 models: NX-H1331, NX-H3321, K885G7, C805G7, C805DG7, C800G7, K888G7,
K885G6, K880G6, K905G6_G6P.

Page 3 models: K905G6_G6P, K900G6, Seadra, Horsea, NX-H1313, NX-H1323,
Metagross, Artemis II Rack, Inventec X680m CXL Memory Accelerator,
Inventec FPGA IPU C5020X.

Important source distinctions:

- NX-H1313's row links to a document whose printed title contains NX-H1331.
  No equivalence, correction or extra details were inferred. Previously gated
  NX product-detail pages were not accessed through an alternate route.
- K905G6_G6P stays grouped. A linked white-paper title containing K905G6 is not
  enough to split or alias the product-model field.
- Prior page 1 paired P9000IG6(AC) with a title containing P9000G6 (AC). Preserve
  this mismatch too; review a permitted original document before connecting them.
- Artemis II Rack and X680m are useful follow-up product leads; manufacturer
  claims, precise commercial model codes and hardware specifications are not yet
  established by this pagination review.

All four new original requests succeeded using ordinary public GETs with TLS
validation. No retries, known-denial workarounds, private serial/configuration,
warranty, activation, newsletter, license or contact actions were performed.

## Local files and queries

Original captures and outcome log: `data/raw/inspur-rail-2026-10-04/`.
Reviewed facts: `data/inspur-rail-facts-2026-10-04.json`.
Complete replacement PDF bundle:
`data/raw/reference-documents-with-inspur-rail-2026-10-04.json`.
Never import only the newly added PDF as a replacement collection.

Use the bundled Python when system Python is unavailable. Run from this folder:

```sh
python3 -m devicehunt vendor-inventory Inspur --scope all --limit -1
python3 -m devicehunt specs NS5162M5 --vendor Inspur --exact-model
python3 -m devicehunt specs i24 --vendor Inspur --exact-model
python3 -m devicehunt specs NE5260M5 --vendor Inspur --exact-model
python3 -m devicehunt documents CN61108PC-V-H
python3 -m devicehunt lookup 'i24&NS5162M5'
```

The exact specification modes keep node/chassis statements separate; broad
searches may find both because the relationship text explicitly names both.
Query appliance mappings with SQL:

```sql
SELECT d.model AS appliance, f.value AS server_node, r.source_url
FROM facts f
JOIN records r ON r.id = f.record_id
JOIN devices d ON d.id = r.device_id
WHERE r.source_id = 'inspur-incloud-rail-hardware-2023'
  AND f.name = 'server_node_model'
ORDER BY d.model;
```

The promoted, rebuilt catalog has 3,976 identities / 5,180 records / 257 sources,
47 PDFs / 3,587 pages. Inspur has 47 associated identities / 3 manufacturer-backed
identities; Inventec remains 79 / 44. The owned baseline remains 10/10 with its
9-document/1-label distinction. ByteDance B4020/B4010 remain 0/2 evidence gaps.
The 113-job rebuild from 2,130 hashed inputs matched all 43 tables/indexes.
All 408 Python tests and six report tests pass. All 98 lookup reports and current
exports were refreshed; all 47 PDF hashes, SQLite integrity/foreign keys and the
unchanged live database hash passed final verification. See `PROGRESS.md` and
`data/exports/inspur-rail-final-verification-2026-10-04.json`. Full historical or
worldwide coverage remains unproven.
