# Expanded ODM discovery — 2026-10-04

The active goal names Inventec, Hyve, Foxconn, Wiwynn, TD SYNNEX / SYNNEX, Quanta,
Accton, Edge-Core, Celestica, Oracle and Amazon Data Services. Related ODMs can
be captured as leads. This is the first source/identity screening pass, not an
import of new device assertions or a completed legal-entity mapping.

Hyve was subsequently promoted to a standalone requested target by the user
on 2026-10-04. It must remain separately searchable from TD SYNNEX/SYNNEX, with
its own brand/manufacturer/source records and dated group relationships.

## Initial cached registrations

`data/exports/odm-expansion-start-2026-10-04.json` preserves all rows found by
literal case-insensitive matching in the existing `fcc_grantees` and
`affiliations` tables. The database stayed byte-for-byte unchanged; the ten
owned-device checks still pass. These historical snapshots are not a fresh
FCC query and zero literal matches do not mean absence of filings or hardware.

Inventec has 9 name-matching registrations, Foxconn 5, SYNNEX 1 and Quanta 8;
Hyve has 1 grantee name matches and 0 existing affiliation matches;
Wiwynn and TD SYNNEX have none in that exact-name scan. The 24 raw matches are
not 23 reviewed company mappings. In particular, `Quantam Telematics Limited`
and `SEQUANTA` are substring false positives for the requested Quanta target and
must not become aliases. `Quanta Semi Co., Ltd`, `Quanta Storage Inc.` and
`Quanta Corporation` also need their own entity review. HFS and PZ5 print
Quanta Computer Inc; DGI prints Inventec Corporation.

The existing CubScout report has a manufacturer affiliation for **Foxconn
Interconnect Technology Limited Taiwan Branch**, currently with `target=NULL`.
Preserve that exact entity and its existing evidence. It must not acquire a
Foxconn parent-manufacturer attribution solely because of the shared string.

## Primary-source paths found

These notes paraphrase facts returned by web search/open tools on 2026-10-04.
No original HTML/PDF was downloaded in this pass. Links are research references,
not yet hashed import dependencies. Dates below belong to the cited statements;
parent/subsidiary statements are not unqualified current ownership warranties.

| Requested group | Source and observed scope | Next action |
|---|---|---|
| Inventec | [Official enterprise/cloud products page](https://www.inventec.com/en/products) describes EBG research, design and manufacturing of servers, switches, storage and rack solutions, including ODM/JDM/CM models. | Collect exact platforms and compliance identities; review each Inventec-named legal entity separately. |
| Foxconn | [Official 2023-12-25 release](https://www.foxconn.com/en-us/press-center/press-releases/latest-news/1172) calls Ingrasys a subsidiary and identifies AI-server/cloud-storage factory output. [Ingrasys about page](https://www.ingrasys.com/company/about/about/) also describes the group relationship. | Explore Ingrasys products and regulatory names. Keep parent affiliation distinct from the named device manufacturer; separately resolve FIT and Hon Hai entities. |
| Wiwynn | [Official company page](https://www.wiwynn.com/about-wiwynn/company-info) describes its ODM-direct model and historical Wistron backing. [Product index](https://www.wiwynn.com/products) names SV7100G4, SV302A, SV328R, SV310G4 and SV305A. | Capture product/detail sheets and exact ordering codes; Wistron stays a related-company lead, not an automatic alias. |
| TD SYNNEX / SYNNEX | [TD SYNNEX's 2023 Hercules 9120 release](https://ir.tdsynnex.com/news/news-details/2023/Hyve-Solutions-Announces-Ultra-High-Density-Mass-Storage-Platform/default.aspx) calls Hyve Solutions its wholly owned subsidiary and names the Hyve Hercules 9120 storage platform. | Prioritize Hyve product/compliance/OCP material. A Hyve relationship does not make TD SYNNEX the physical manufacturer. |
| Quanta | [QCT's dated 2015 branding announcement](https://qct.io/ja-JP/news/index/PR/Corporation/Quanta-Cloud-Technology-QCT-Unveils-New-Branding-to-Match-Business-Focus/2/) describes Quanta Computer Inc. as QCT's parent. This fact was available in search output; direct web-tool opening returned an internal error. | Review current and historical QCT/Quanta product/compliance sources; separate each legal entity, commercial brand and role. |

For SYNNEX's transition, [the SEC-hosted merger-completion announcement](https://www.sec.gov/Archives/edgar/data/1177394/000119312521264521/d213822dex991.htm)
reports the completed 2021 combination with Tech Data. A later annual-report
search extract gives November 3, 2021 as the effective corporate-name change,
which must not be conflated with the September merger completion. Capture and
review the underlying dated filings before encoding historical aliases. Do not
collapse every international company containing SYNNEX into this target.

## Next implementation checkpoint

1. Save accessible originals or explicitly labelled reviewed extracts and hashes.
2. Encode only reviewed exact name mappings, with tests rejecting substring,
   composite, similarly named and unreviewed subsidiary matches.
3. Expand the CLI target registry and source-specific mappings; backfill existing
   captures into a fresh rebuild rather than silently relabeling every parent.
4. Update manifest counts, regenerate current reports and rerun baseline/tests.
5. Import reviewed model-specific evidence, beginning with the product leads
   above. Keep registry/applicant, commercial vendor, legal manufacturer, physical
   factory, historical group affiliation and hyperscaler deployment distinct.

The live catalog and target filters still have their prior scope at this
checkpoint. The expanded research goal is active; no new devices were imported
by this note or the initial name-scan report.
