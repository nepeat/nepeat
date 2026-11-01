# inspur nic table review — 2026-10-04

source review complete and accepted after independent QA. the integrator has
promoted this batch; see [PROGRESS](../PROGRESS.md) for current export/audit status.
the research worker changed only this note and the curated JSON; integration
was performed separately. the coordinator maintains this status pointer.

## source and method

original [incloud rail compatibility list](https://partner.inspur.com/eportal/fileDir/lcjtww/resource/cms/2024/03/2024032014450874883.pdf),
cover december 2023; existing immutable capture
`data/raw/inspur-incloud-rail-compatibility-2026-10-04.pdf`.
sha-256: `9f340f567f5603150a9c82781e1fc34656c3f2177e68b3f259ba842a32d16150`.
this matches the companion `pages.json` recorded pdf hash and earlier source note.
no new acquisition was needed. visual inspection used
`data/exports/nics-wiwynn-hyve-visual-review/inspur-nics-14.png`, `-15.png` and
`-16.png` (pdf pages 14–16; printed pages 7–9).

curated source id: `inspur-incloud-rail-nics-2023`, deliberately distinct from
`inspur-incloud-rail-hardware-2023` so integration does not replace older records.
snapshot is relative to the curated file's data directory. eight unique exact
names appear in ten literal inspur table rows, so ten records preserve both
storage and management/business contexts.

## literal rows

| pdf page | section | printed speed group | exact name | remarks as printed |
| --- | --- | --- | --- | --- |
| 14 | 6.1 存储网卡 | 10G | X710_2Port_LC_PCIe | 适用 Intel +HG 版本 |
| 14 | 6.1 存储网卡 | 10G | 82599_2Port_LC_PCIe | 适用 Intel +HG 版本 |
| 14 | 6.2 管理网卡/业务网卡 | 1G | Inspur_Vostok_I350_1G_RJ_PCIEx4-G3_2 | 适用 Intel +HG 版本 |
| 14 | 6.2 管理网卡/业务网卡 | 1G | I350_4Port_RJ45_PCIe | 适用 Intel +HG 版本 |
| 14 | 6.2 管理网卡/业务网卡 | 10G | 82599_2Port_LC_PCIe | 适用 Intel +HG 版本 |
| 15 | 6.2 管理网卡/业务网卡 | 10G | X710_2Port_LC_PCIe | 适用 Intel / +HG+ARM 版本 (two printed lines) |
| 15 | 6.2 管理网卡/业务网卡 | 10G | Inspur_Andes-M6_X710_10G_LC_OCP3x8_2 | 适用 Intel +HG 版本 |
| 15 | 6.2 管理网卡/业务网卡 | 10G | NIC_10Gbps_2Port_LC_Inspur_X710_PCIe_MM | blank |
| 15 | 6.2 管理网卡/业务网卡 | 10G | Inspur_Sbt_X722_10G_LC_PCIEx8-G3_2 | blank |
| 15 | 6.2 管理网卡/业务网卡 | 25G | E810_2Port_LC_PCIe | blank |

page 15 continues the merged inspur vendor and 10G cells from page 14.
model lines join without added characters: Andes ends its first line `OCP3x8`
then `_2`; NIC ends `PCIe` then `_MM`; Sbt ends `PCIEx8-G3` then `_2`.
remarks do not merge across the three blank cells. the x710 management remark
retains its printed newline in json, rather than inheriting storage applicability.
raw rows retain empty strings; `compatibility_table_remarks_state=blank` preserves
absence in normal facts because the catalog intentionally omits empty fact values.

## roles, conflict and exclusions

`brand: Inspur` is solely the catalog's identity/grouping namespace for names
under the inspur table-vendor cell. affiliation is exclusively
`Inspur / compatibility_table_vendor`: no product-brand, manufacturer, platform
vendor, factory, parent, operator or deployment affiliation is inferred.

page 14 explicitly prints `特别说明：intel X722 10-Gigabit 不兼容、易飞扬的光模块不兼容。`
(the general x722 and optical-module incompatibility warning). page 15 nonetheless
lists the specific inspur sbt x722 name in the management/business table with
blank remarks. both statements are preserved; scope/resolution is unstated.
no unqualified compatibility is asserted. these are historical table statements,
not current support guarantees or advice to flash an auction unit.

exclude all third-party rows: Mellanox storage/management rows, Intel X550/EX710
and E810, Broadcom and Wangxun. the separate Intel row prints the same
`E810_2Port_LC_PCIe` spelling; it neither transfers intel roles to the inspur
record nor establishes equivalence between vendor rows. page 16 contains no
inspur additions. no controller-chip specs, pci lane specs, port-count specs,
aliases, numeric ordering codes or firmware entitlements were inferred from names.

## checks and next step

- verified original pdf sha-256 against the companion extraction metadata and
  previously recorded expected hash.
- all ten row names occur in the matching extracted page after joining line wraps;
  visual review confirms eight exact names, merged vendor/speed scope and blanks.
- exercised `devicehunt.importers.curated` with a validating sink, without opening
  or writing a database: 8 models / 10 records / 1 source, ten unique record keys,
  three blank remarks, only the allowed affiliation and correct snapshot path.
- curated sha-256: `8f3da5d987e699fbce8888c9015061431340fd6da7831f37b31c639be12d4e51`.
- shell git checks were unavailable due to the existing xcode-license restriction;
  no license or system configuration was changed. local text whitespace and json
  checks were run using bundled python instead.

independent QA checked these artifacts against the three page renders; accepted
input hashes and integration proof are linked from the [batch record](nics-wiwynn-hyve-batch-2026-10-04.md).
the eight-name finite scope is complete; no broader NIC collection was started.
