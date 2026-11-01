# Inventec and QCT model evidence — 2026-10-04

Current follow-up: [the Inventec practical milestone](inventec-practical-batch-2026-10-04.md)
adds reviewed Entei announcement and Golduck original-brief specifications to the
existing exact identities. Generic redirected Entei content and search-only Golduck
per-CPU memory wording are excluded. Current status/proofs live in
[PROGRESS](../PROGRESS.md); the body below records the earlier Inventec/QCT stage.

Earlier follow-up: [the vendor-discovery batch](vendor-discovery-batch-2026-10-04.md)
reviews model-specific D51B/D52BQ specifications separately from the WSSD
configuration claims below. Its source handoff records mirror provenance,
download restrictions and the excluded T41S reseller document.

This batch combines a reproducible Inventec server-index importer, two detailed
Inventec platform pages, five dated manufacturer claims, nine older QCT models
and separately sourced Quanta parent-company associations. New raw captures are
under `data/raw/inventec-qct-2026-10-04/`; the late Inventec manufacturer article
and its fetch report are separate sibling files under `data/raw/`.

## Inventec

[Original English server index](https://ebg.inventec.com/en/product): 28 public
model headings, all retained verbatim after whitespace normalization:

P9500AG7; P9000G7(AC); P9000AG7 (AC); P9000IG6(AC); P9000G6 (LC);
P600G7 (LC); P5800G7; P5000AG7; P90G6; NX-H1331; NX-H3321; K885G7;
EdgePro 2U; C805G7; C805DG7; C800G7; K888G7; K709G6; K885G6; K900G7;
K880G6; K905G6_G6P; K900G6; Seadra; Horsea; EdgePro 1U; NX-H1313; NX-H1323.

The combined heading `K905G6_G6P` remains one name. Parentheses, spaces, suffixes
and underscores are not dropped or split into inferred commercial SKUs.
Four cards (NX-H1331, NX-H3321, NX-H1313, NX-H1323) visibly say login is required
for full details. Their public names and access status are retained; no detail
requests or authentication attempts were made. These are index/platform claims,
not manufacture, current availability, release-date or operator assertions.
The page is the server category; this is not a complete Inventec product archive.

`devicehunt/inventec.py` imports this snapshot through `inventec-server-index`.
It validates all cards before source replacement, rejects empty/malformed or
duplicate/conflicting cards, ignores navigation and external links, and never
fetches linked resources. Five tests cover names, roles, login limitations,
failed replacement and idempotency. The full original HTML remains the snapshot.

The separately captured Chinese Horsea and Seadra product pages provide useful
power/rack/management facts. Their statements remain separate from the index:

| Platform | Selected printed specs |
| --- | --- |
| Horsea | Dual AMD EPYC 7003/7002, 32 DDR4 DIMMs, 2U, 447.6 × 87 × 780 mm; IPMI 2.0 + KVM; 1+1 redundant Platinum PSU alternatives 800/1300/1600/2000 W, input printed as 100–220 VAC |
| Seadra | Dual third-generation Xeon Scalable, 32 DDR4 DIMMs, 2U, 438 × 87.9 × 780 mm; AST2500; 1+1 CRPS Platinum PSU alternatives 800/1300/1600/2000 W, input printed as 110–220 VAC |

Both pages list six 6056 hot-swappable fans with N+1 redundancy. Chinese power
statements are retained alongside English translations. Values are alternatives,
not a description of a specific auction unit; no PSU derating or SKU A/B/C order
code mapping is inferred. No firmware availability is verified.

[The original 2020-11-04 manufacturer announcement](https://ebg.inventec.com/cn/news/%E6%96%B0%E9%97%BB%E4%B8%8E%E5%85%AC%E5%91%8A/2020/37)
explicitly attributes development and manufacture of **Entei, Horsea, Golduck,
E850G4 and FPGA SmartNIC C5020X** to Inventec's EBG division. These five records
receive the `Inventec Corporation: manufacturer` role, with the printed division
name and Chinese claim retained. The article identifies the corporation and its
EBG division; this does not require a subsidiary alias. The separately IAC-made
5G V2X OBU Box, Tianjin software products, unnamed custom servers and Intel's
C5000X underlying platform are excluded. No physical factory or operator assigned.
Source ID: `inventec-dongguan-manufacturer-2020`.

## QCT and Quanta

[Original WSSD datasheet](https://go.qct.io/wp-content/uploads/2020/11/QCT-Window-Server-2019-Datasheet.pdf)
has nine table-header model names:

| Model as printed | Configuration form factor | Printed CPU family |
| --- | --- | --- |
| QuantaPlex T21P-4U | 4U, 1 node | Xeon E5-2600 v4 |
| QuantaGrid D51B-2U | 2U, 1 node | Xeon E5-2600 v4 |
| QuantaGrid D52BQ-2U | 2U, 1 node | Xeon Scalable |
| QuantaPlex T41S-2U | 2U, 4 nodes | Xeon E5-2600 v4 |
| QuantaPlex T42S-2U | 2U, 4 nodes | Xeon Scalable |
| QuantaGrid D52BV-2U | 2U, 1 node | Xeon Scalable |
| QuantaGrid D51PH-1ULH | 1U, 1 node storage | Xeon E5-2600 v4 |
| QuantaGrid D52T-1ULH | 1U, 1 node storage | Xeon Scalable |
| QuantaGrid S31A-1U | 1U, 1 node | Xeon E3-1200 v6 |

Pages 2–3 were visually checked to avoid assigning specifications to the wrong
table columns. These are particular WSSD solution configurations, not maximum
platform capabilities. The footer says copyright 2018, PDF metadata is dated
2018-11-21, filename includes 2019, and URL path is 2020/11; no product release
date is selected from those dates. Microsoft software compatibility and solution
participation do not establish Microsoft deployment or unit provenance.

Page 40 of the original [QCT QxVDI reference architecture](https://go.qct.io/wp-content/uploads/2020/05/QxStack-QxVDI-OAHC-High-Density-Optimized-SKU-Reference-Architecture-EN.pdf)
expands QCT as Quanta Cloud Technology and explicitly names Quanta Computer Inc.
as parent. Its footer says copyright 2020–2021. It supports a separately dated
`historical_subsidiary_portfolio` record for each QCT model. Both model and
corporate source URLs/hashes are retained; the join is explicitly labelled an
inference across sources. No manufacturer claim transfers to Quanta.

QCT remains the product brand; full QuantaGrid/QuantaPlex prefixes stay in model
fields. A QCT QuantaPlex T41S-2U identity is not silently merged with a bare
T41S-2U under Quanta in another register. QCT and Quanta Cloud Technology remain
excluded from global Quanta company aliases.

Quanta's original [2025 affiliated-company report](https://www.quantatw.com/quanta/english/about/organize25en.pdf)
is also archived, with chart page 1 visually reviewed. It supports further legal
entity research; it does not itself identify these models or establish their
manufacturer. Detailed subsidiary paths and percentages were not generalized.

## Rebuild and access outcomes

Three original PDFs add 62 pages. The complete reference collection is
`data/raw/reference-documents-with-inventec-qct-2026-10-04.json`; never replace it
with a partial three-PDF bundle. Curated facts are in
`data/inventec-qct-facts-2026-10-04.json` and
`data/inventec-manufacturer-facts-2026-10-04.json`.

The first candidate review covers the index/product batch; the final review adds
five manufacturer claims through a second full preservation audit. Both stages
and the original live database have preserved snapshots/manifest checkpoints.
Final counts and verification are recorded in PROGRESS.md.

Two distinct QCT web resources returned 403: the Broadwell CPU-upgrade download
index and the 2016 European-office press page. Neither was retried or retrieved
through an alternate route. The independently accessible WSSD and QxVDI documents
support different claims; no firmware package was fetched or verified.

ByteDance B4020/B4010 remain unresolved. This search found no new primary
affiliation evidence. Trade-data preview claims remain unverified, and IMPERX
B4020/ICL-B4020 camera results are model-string collisions. The known Eximpedia
402 was not retried. Neither user example is imported as evidence.

## Queries

```sh
python3 -m devicehunt search Horsea --vendor Inventec --role manufacturer
python3 -m devicehunt specs Horsea
python3 -m devicehunt search K905G6_G6P --vendor Inventec
python3 -m devicehunt search 'QuantaPlex T41S-2U' --vendor Quanta
python3 -m devicehunt vendor-inventory Quanta --scope all --limit -1
```

Use [the bundled Python in README](../README.md) if system Python is unavailable.
QCT's nine additions appear in all associations, while Inventec's five explicit
manufacturer claims also appear in the default manufacturer view.
