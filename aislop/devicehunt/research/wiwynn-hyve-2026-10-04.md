# wiwynn / hyve capture curation — 2026-10-04

source review complete and accepted after independent QA, including the HTML source-scope correction below. the integrator has promoted the batch; [PROGRESS](../PROGRESS.md) owns current export/audit status. the coordinator maintains this status pointer; source research and shared-state integration had separate owners.

## bounded inputs and checks

Reviewed the existing four Wiwynn HTML product originals, derived `wiwynn-product-tables.json`, five PDF originals and all eight PDF page renders in [visual review](../data/exports/nics-wiwynn-hyve-visual-review/). PDF text was extracted with the bundled Python/pypdf; table column assignments were checked visually. HTML visible text was read with the standard-library HTML parser. No packages installed and no network calls made.

All ten successful original capture hashes in [fetch report](../data/raw/nics-wiwynn-hyve-2026-10-04/fetch-report.json) match the stored bytes; the sitemap is discovery-only and excluded from curation. The existing Hyve Ambient Series-E HTTP 403 remains unavailable; it was not retried or retrieved through another route.

The [curated input](../data/curated-wiwynn-hyve-2026-10-04.json) contains **9 new source IDs, 15 source records, 11 distinct brand/model identities and 259 facts**. All source IDs were checked against `sources` in the authoritative database opened with SQLite `mode=ro`; none existed. Snapshot paths resolve relative to `data/`. The JSON parsed successfully and source/record keys are unique within each source.

## identities and layout decisions

| identity | scope / discriminating facts |
| --- | --- |
| SV7100G2 | 4U16N platform; 16 SoC nodes, four 1S server cards per tray; chassis-wide 80kg and 48 M.2 modules stay separate in scope from per-node memory and three M.2 modules. |
| SV7220G3 | Family heading; three nodes in a 2OU shelf. No family/chassis alias for its variants. |
| SV7220G3-V | First PDF p2 column; four SAS/SATA bays; two PCIe x16 HHHL slots. |
| SV7220G3-V+ | Middle PDF p2 column; visually superscript plus; two SAS/SATA bays; one PCIe x16 HHHL plus two PCIe x8 HHHL slots. Extracted text detaches the plus; identity preserves it. |
| SV7220G3-S | Last PDF p2 column; one 3.5-inch SAS/SATA bay; two PCIe x16 FHHL slots. |
| ST7000G2 | Disaggregated storage enclosure; processor/memory/node/IOM/mezzanine cells grey, no server specs copied into those gaps. SES/SMP and two external mini-SAS ports. |
| SV7000G2 | Two micro servers; eight DDR4 slots total, two M.2, two IOMs and two mezzanines; expansion ports literal N/A. |
| SV7000G2-L | One micro server; four DDR4 slots, one M.2, one IOM and one mezzanine; two external mini-SAS ports. Distinct suffix. |
| Hercules | May 2026 5U 112-drive platform, distinct from previous Hercules 9120 and Orion. |
| HS9124 | Explicit Hyve-designed motherboard named in the Hercules motherboard row; chassis facts are not transferred to it. |
| JBOF | Named May 2026 2U 24-drive NVMe enclosure; no numeric SKU invented. |

## preserved conflicts and practical limits

- Wiwynn rack power is centralized bus-bar/shelf power. Bare cards/sleds do not acquire an independent AC PSU assertion. `power_description`, `physical_description` and source-supported `cooling_description` categories make those constraints available to specs output.
- SV7100G2 HTML combines a 1OU one-third-width card/node form-factor row with 4U vCubby chassis dimensions and weight. PDF identifies the 4U16N platform. Both scopes remain explicit; no unnamed chassis/node model is invented.
- SV7220G3 PDF p2 calls the node half-width despite three nodes per shelf and 174mm node width. The printed wording is retained without physical-layout correction.
- SV7220G3 HTML describes 100GbE QSFP and a multi-host OCP 3.0 NIC shared by four nodes; PDF specifies 40GbE QSFP and three nodes per shelf. HTML OpenBMC/IPMI partial support/optional Cluster Manager differs from PDF IPMI v2.0/Cluster Manager. HTML family assertions are not copied to variant records.
- The HTML download label `SV7200G3 Datasheet` differs from page/PDF headings `SV7220G3`; it does not create an SV7200G3 alias or identity.
- ST7000G2/SV7000G2 PDF dimensions are 185(H) x 537(W) x 879(D), with units absent in the row. HTML uses 190(H) x 538(W) x 879(D), with handle depth 1004 for ST and 940 for SV. Printed dimensions and separate handle claims remain source-scoped.
- Hercules introductory prose explicitly names Orion, a 10U HGX GPU platform with air/liquid cooling. Hercules heading/features/table instead describe a 5U 112-drive air-cooled storage server. The prose is retained as a conflict, not Orion identity proof or Hercules GPU capability.
- Hercules memory wording `16x RDIMM 6400MT/s DDR5, 64GB, 2DPC` stays literal enough to avoid deciding whether 64GB is per DIMM or total. The LAN typo `1x 1G R45` also stays explicit.
- JBOF power literal `12V @ 11-220VAC` stays in the power fact with uncertainty. It is not repaired to 110V or offered as verified electrical guidance. TPM module in key features versus optional in the table also remains configuration-dependent.
- Hyve May 2026 footer gives TD SYNNEX a dated `historical_subsidiary_portfolio` role; it does not give TD SYNNEX or Hyve a physical-manufacturer/operator role. Only HS9124 has explicit Hyve designer evidence.

No manufacturer, operator/hyperscaler deployment, present marketplace stock, firmware access, licensing entitlement or installed auction configuration was inferred. Generic hyper-scale/OCP lineage prose remains context only. Footer copyrights, download dates and PDF filename dates are not product introduction dates.

## source inventory

| source ID | captured file | SHA-256 |
| --- | --- | --- |
| `wiwynn-sv7100g2-product-page-2026-10-04` | [wiwynn-sv7100g2.html](../data/raw/nics-wiwynn-hyve-2026-10-04/wiwynn-sv7100g2.html) | `bcd5775b3492af4e50396560b2a2db637130c00c24ad5ea56113178e7e6039f3` |
| `wiwynn-sv7220g3-product-page-2026-10-04` | [wiwynn-sv7220g3.html](../data/raw/nics-wiwynn-hyve-2026-10-04/wiwynn-sv7220g3.html) | `23a08a89d05c36503c7862823e05e259772aa29637c2179f6ad55dbb932b7473` |
| `wiwynn-st7000g2-product-page-2026-10-04` | [wiwynn-st7000g2.html](../data/raw/nics-wiwynn-hyve-2026-10-04/wiwynn-st7000g2.html) | `4befdd2ab27ac4541e1b39ef94fc9a99398631d7113293bfa5fabc2f8687b6cb` |
| `wiwynn-sv7000g2-product-page-2026-10-04` | [wiwynn-sv7000g2.html](../data/raw/nics-wiwynn-hyve-2026-10-04/wiwynn-sv7000g2.html) | `bbefa9bd0b584d2ff100ebd54748249b7a57e4db4d3deb38f080e179a8013cc2` |
| `wiwynn-sv7100g2-datasheet-reviewed-2026-10-04` | [wiwynn-sv7100g2.pdf](../data/raw/nics-wiwynn-hyve-2026-10-04/wiwynn-sv7100g2.pdf) | `079a515d8a1d6b86021912f2d9eabe4427bde46a680bb50962438b8c3377899e` |
| `wiwynn-sv7220g3-datasheet-reviewed-2026-10-04` | [wiwynn-sv7220g3.pdf](../data/raw/nics-wiwynn-hyve-2026-10-04/wiwynn-sv7220g3.pdf) | `703f19b6b8814c146d8c332221381836f0c285c6dc4de6e0ef583bd5d5546783` |
| `wiwynn-st7000g2-datasheet-reviewed-2026-10-04` | [wiwynn-st7000g2.pdf](../data/raw/nics-wiwynn-hyve-2026-10-04/wiwynn-st7000g2.pdf) | `4bbf3cba70869fff52ebed7b28aa397e46e0c5960d0553f76dc331984b7363f1` |
| `hyve-hercules-2026-datasheet-reviewed-2026-10-04` | [hyve-hercules-2026.pdf](../data/raw/nics-wiwynn-hyve-2026-10-04/hyve-hercules-2026.pdf) | `cbdb1dc60eab08e1ec15d70cb5e9373769156efd58bae942934acd108828cf03` |
| `hyve-jbof-2026-datasheet-reviewed-2026-10-04` | [hyve-jbof-2026.pdf](../data/raw/nics-wiwynn-hyve-2026-10-04/hyve-jbof-2026.pdf) | `77b9c9b5bfc75dbf90749bbf9dccbb8a910356676fea938077efb35297eade02` |

## handoff

Independent QA correction: ST7000G2 HTML storage fact now contains only its own 72 hot-plug 3.5-inch bays; the 12Gb SAS/6Gb SATA clause remains in PDF records and SV7000G2 HTML, where it is present. A second source-scope pass checked HTML facts against the captures; SV7000G2 storage and SV7100G2 network wording now explicitly says HTML feature prose instead of PDF page 1. Cross-source comparisons remain labelled conflicts, not merged specifications. Counts are unchanged.

System `git diff --check` / scoped status could not run because Apple Git requires Xcode license acceptance. No system configuration was changed. A direct whitespace check found no trailing whitespace in either owned artifact; JSON/hash/reference checks passed. No import or test-suite run is claimed by this research worker.

Independent QA checked V+/V/S columns, merged SV7000G2-L management/OS cells, source conflicts and company roles; the ST HTML source-scope finding was corrected and rechecked. Accepted hashes and integration proof are in the [batch record](nics-wiwynn-hyve-batch-2026-10-04.md). The five PDF originals/eight pages were added through the exclusive integrator.

The research worker edited only the curated JSON and this note; subsequent status pointers are maintained by the coordinator. Raw captures, sources, manifest, complete PDF bundle, authoritative database, exports, code, tests and other documentation were not changed. This finite capture set is the stop condition.
