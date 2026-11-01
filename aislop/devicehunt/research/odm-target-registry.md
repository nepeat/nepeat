# Expanded exact target registry, 2026-10-04

`devicehunt.catalog.TARGETS` now accepts the thirteen requested target groups.
Matching still requires a complete normalized name, not substring matching.
Normalization removes punctuation/spacing and case differences; it does not remove
legal suffixes, expand abbreviations, split composite cells or resolve parents.

| Target | Newly accepted names beyond canonical target | Evidence / limits |
|---|---|---|
| Inventec | Inventec Corporation | Exact DGI registration in the saved FCC grantee snapshot; original Inventec corporate/products HTML confirms the enterprise product business. Inventec Appliances and geographic subsidiaries remain unassigned. |
| Hyve | Hyve Solutions; Hyve Solutions Corporation | Original TD SYNNEX 2023-10-17 Hercules release names the company and product; exact 2AUSZ grantee registration. Hyve Group remains unassigned. |
| Foxconn | None | Only the requested exact brand is added. Hon Hai, FIT, Ingrasys, Foxconn International and other companies need separate entity/role review; substring hits are not aliases. |
| Wiwynn | Wiwynn Corporation | Original company/product HTML prints the legal company name. Wistron is not an alias. |
| TD SYNNEX | TD SYNNEX Corporation; SYNNEX; SYNNEX Corporation | Requested legacy/current names. SEC-hosted 2021 merger-completion exhibit was readable through the web tool; direct original download returned 403. Its selected facts are preserved separately. Names remain printed per source and date; no parent manufacturing is inferred. |
| Quanta | Quanta Computer Inc | Exact HFS/PZ5 names in saved FCC grantees. QCT/Quanta Cloud Technology remains a separate brand/entity, with nine reviewed historical portfolio associations below. Quanta Storage, Quanta Semi and Quanta Corporation remain unassigned. |

The original five target mappings remain unchanged. In particular, Amazon consumer
entities do not become Amazon Data Services.

`Synnex Technology International Corp` (MDO) is not an accepted TD SYNNEX alias.
`Quantam Telematics Limited` and `SEQUANTA` are substring false positives and are
excluded. Full printed manufacturer/applicant/brand names remain in each record;
a canonical search target does not rewrite the legal entity or transfer its role.

The FCC coverage helper additionally recognizes DGI, 2AUSZ, HFS and PZ5 from the
saved exact registrations, without turning grantee registrations into models or
adding any sitemap URL as a filing. Its original detail collector remains scoped
to its original captures. Hyve 2AUSZ had no URL in the cached global sitemap.

## Original capture evidence

ByteDance and Inspur were added in a subsequent 2026-10-04 pass. Exact ByteDance
and Inspur names plus Inspur Electronic Information Industry Co., Ltd. qualify.
See [their mapping review, exclusions and example gaps](bytedance-inspur-2026-10-04.md).
Parent/subsidiary and manufacturer distinctions above continue to apply.

Original public HTML is saved under `data/raw/odm-primary-2026-10-04/` with a
fetch report and SHA-256 hashes: Inventec about/products, Wiwynn company/products,
TD SYNNEX Hercules release, and Foxconn's dated Ingrasys release. These pages
support separate claims; no one document qualifies every group member.
The SEC merger original returned 403 and was not retried through another route.
A selected factual web extract records the evidence that had already been read.

The expanded source rebuild also parses the already-cached Inventec and Quanta
ONL sections. The old table's original CPU/forwarding text stays unchanged even
when it is incomplete (for example, Inventec's `on` forwarding cell). These are
historical platform-support associations, not OEM or present support guarantees.

## Product index boundaries

The current curated product capture adds twelve front-facing Wiwynn card models
and Hyve Hercules 9120. They are brand/platform claims, not automatic manufacturer
or deployment assertions. The full source coverage and facts flag:

- Wiwynn front card `EP100G2` has reverse heading `EP200G2`; the reverse heading
  remains a fact, not an alias or additional independently supported model.
- Wiwynn `ST7000G4` links to an `SV7000G4` URL; both printed model identities are
  kept separate and the link discrepancy remains unresolved.
- Hercules 9120's 5U / 96-drive and rack-level values are dated announced platform
  capacities, not measured specifications of a particular auction unit.
- The Hyve-to-TD-SYNNEX parent relationship is dated 2023-10-17 and uses the
  `historical_subsidiary_portfolio` role, separate from Hyve's platform-vendor role.
- Eight Ingrasys server/storage names have an explicit, separately sourced
  `parent_company_portfolio` association with Foxconn as observed 2026-10-04.
  The company page prints Foxconn Technology Group; that name is retained in
  facts, with each model source URL/hash alongside the company source URL/hash.
  This reviewed cross-source inference does not add Ingrasys or Foxconn Technology
  Group to global aliases, transfer manufacturer claims or establish deployment.
  See [the Ingrasys review](ingrasys-servers-2026-10-04.md).
- Nine full QuantaGrid/QuantaPlex names from QCT's original WSSD datasheet have
  separately sourced `historical_subsidiary_portfolio` associations with Quanta.
  The QxVDI reference architecture names Quanta Computer Inc. as QCT's parent;
  its copyright date is 2020–2021. The reviewed cross-source inference preserves
  both source URLs/hashes and does not add QCT as a global alias or manufacturer.
  Bare model names in other registers are not silently merged with these names.
- Inventec's English server index contributes 28 exact public headings, including
  four whose detail pages require login and were not requested. These index names
  carry brand/platform roles. A separate dated 2020-11-04 Inventec announcement
  explicitly attributes development/manufacture of Entei, Horsea, Golduck, E850G4
  and FPGA SmartNIC C5020X to its EBG division. The article's separate IAC-made
  vehicle device remains excluded. See [the Inventec/QCT review](inventec-qct-2026-10-04.md).

## Verification

Tests cover new exact names, excluded subsidiaries/near-matches, parent versus
manufacturer filters and ONL's complete model suffixes/section boundaries. The
initial expanded imports preserved all original assertions/raw rows and were
promoted after a second clean rebuild matched all 42 checked tables/indexes.
That initial milestone passed 395 tests; subsequent verification is recorded in
PROGRESS.md. The ten-device baseline remains 9 document hits plus one label-photo hit.
Current counts and remaining gaps are in PROGRESS.md.
