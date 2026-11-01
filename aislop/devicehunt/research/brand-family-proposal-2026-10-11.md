# brand-family / alias proposal (2026-10-11) — decision needed

status: proposal only, nothing implemented. numbers from `python3 -m devicehunt model-collisions --format json --limit -1`
on `data/devices.sqlite` (candidate b, 5,015 identities; read-only, db untouched).

## the problem

the same hardware shows up as several identities because identity = `normalize(brand)` + `model.casefold()` + revision
(`catalog.py` `add()`), and listings (Red Hat, Ubuntu, SPEC) print whatever vendor string they use. nothing is wrong per
record, but a buyer looking at one identity sees only part of the evidence. `Quanta` / `Inspur` target counts also
exclude the other brand strings (`target_for` returns None for QCT, IEIT, `Inspur Corporation`, `Sun Microsystems`...).

## how big is it (351 groups, 445 software-kind rows skipped)

| reason | groups |
| --- | --- |
| same_model_different_brand | 326 |
| brand_token_prefix (same brand, `Oracle SPARC S7-2` vs `SPARC S7-2`) | 18 |
| brand_token_prefix_different_brand | 7 |

by target (`--target`, a group counts if any member is tied): Accton 236, Edge-Core 170, Inspur 51, Oracle 26, Quanta 18.
by family, groups whose members are all in the family:

| family | groups | identities involved | what splits them |
| --- | --- | --- | --- |
| Quanta / QCT / Quanta Cloud Technology | 16 | 32 | 9 `QCT` twins, 7 `Quanta Cloud Technology` twins of `Quanta` rows |
| Inspur / IEIT SYSTEMS / IEIT Systems / `...(IEI)` / Inspur Corporation / Inspur Power Commercial | 52 | 111 | 46 pure brand, 6 with an `Inspur ` model prefix |
| Oracle / Sun | 23 | 46 | only 5 are cross-brand (Sun Microsystems 4, `Sun Fire` 1); 18 are same brand `Oracle`, prefix only |
| Accton vs Edge-Core / SMC / IgniteNet / 3Com / Nortel... | ~235 | n/a | OEM / relabel relationships, not brand spellings |

important: the Accton/Edge-Core/SMC groups (the biggest block, 157 Accton-Edge-Core pairs) are manufacturer-to-relabel
relationships. a brand-family table must not absorb them; they need role-aware handling and stay review leads.
also: the 18 Oracle groups are not a brand problem at all (same brand, model prefix only), so brand tables cannot fix them.

## worked examples (ids shortened to 6 chars; all tiers are from `source_tiers`)

| # | identities | sources / tiers | note |
| --- | --- | --- | --- |
| 1 | `QCT QuantaGrid D51B-2U` dd133b (4 src: qct-wssd-models, qct-quanta-portfolio-2020, qct datasheet mirror, netbox; vendor+community) vs `Quanta QuantaGrid D51B-2U` 841b1a (4 src: ubuntu-cert, 2 SPEC power, redhat-cert; certification_body+vendor) | disjoint, 8 total | QCT's own datasheets are on one id, third-party certs on the other |
| 2 | `Quanta Cloud Technology QuantaGrid D43K-1U` 297908 (vendor, SPEC reported_system_vendor) vs `Quanta` 43a124 (certification_body+vendor) | 7 such twins | SPEC sponsor is Quanta Computer Inc., printed vendor is QCT |
| 3 | `QCT QuantaPlex T41S-2U` eb1827 (2 src) vs `Quanta QuantaPlex T41S-2U` 4c16d1 (1 src) | | bare `Quanta T41S-2U` 0944273 is a third identity (model-level, see D) |
| 4 | `Inspur NF5280M7` 3ad1c9 (11 src) / `IEIT Systems Co., Ltd. NF5280M7` d9934a (6) / `Inspur Electronic ... (IEI) NF5280M7` 3127b4 (14) | all vendor tier | 31 source records over 3 ids; worst case |
| 5 | `IEIT SYSTEMS Co.,Ltd. NF5280M6` 0401e1 (cert) vs `Inspur NF5280M6` 5b0977 (cert+vendor) | | 38 of the 52 Inspur families are this Inspur+IEIT pair, mostly Red Hat/Ubuntu both listing |
| 6 | `Inspur Corporation Inspur NF5280M5` 896c25 (8 src) vs `Inspur NF5280M5` 5d2e68 (6 src: cert, regulatory, vendor) | | brand AND model-prefix differ; needs B and D together |
| 7 | `IEIT SYSTEMS NF5280-M7-A0-R0-00` 52ff51 vs `Inspur NF5280-M7-A0-R0-00` e3f2c1 vs `NF5280M7` (above) | | Red Hat's IEIT-era order string is a different model string; no collision key at all today |
| 8 | `Oracle Oracle SPARC S7-2` 496977 (regulatory) vs `Oracle SPARC S7-2` 49fd1a (vendor) | | same brand; model prefix only (18 such groups) |
| 9 | `Oracle Crossbow USB` / `Sun Microsystems Crossbow USB` (regulatory both); `Oracle ultra 60` vs `Sun Microsystems ULTRA 60` | 4 cross-brand groups | Sun became an Oracle subsidiary 2010-01-26 (8-K in sun-legacy-portfolio.md) |
| 10 | `Oracle X4470` 4d5de1 vs brand `Sun Fire` model `Sun Fire X4470` f7a19a | regulatory | brand field holds a product line; neither brand nor prefix rule alone connects them |

## options

| | A status quo + leads | B reviewed brand-family table | C canonicalise at import | D model-level aliases |
| --- | --- | --- | --- | --- |
| what changes | docs only (add the "not merged by design" note) | new reviewed file (e.g. `data/brand-families.json`) listing explicit (brand string -> family, relation, evidence, reviewer, date); `lookup`/`show` group results by family; manifest input so rebuild is reproducible | `TARGETS` / import rules map e.g. `QCT`->`Quanta` before id derivation | explicit pairs `(brand?, model A) <-> (brand?, model B)` with evidence, shown as "also known as" in lookup/show |
| ids / counts | none | none (no identity merge); family counts shown beside, target counts unchanged | device ids change (hash of normalised brand); the 9 QCT + 7 QCT-Cloud pairs collapse, `INSERT OR IGNORE` keeps first title/kind; Quanta target and `vendor-inventory` counts change, any QCT-sourced affiliation gets a target | none; adds links only. fixes the cases B cannot (examples 6, 7, 8, 10) |
| evidence needed | none | per mapping, a primary statement that the strings name the same brand/entity; relation typed (`same_entity`, `abbreviation`, `subsidiary_brand`, `renamed`), never `manufacturer` | same as B, plus a decision that identities should truly be one; stricter because irreversible without rebuild | per pair, a source linking the two model strings (datasheet, order-code table); for prefix-only cases the printed product name already is the evidence (full name is kept as an identifier) |
| reversible | yes | yes, delete a row | only by rebuild from manifest; old ids vanish from exports/bookmarks | yes |
| effort | tiny | medium: loader + validator + lookup/show grouping + tests + rebuild-compare | medium-large: importer routing, rebuild, tests, exports refresh, full suite | small-medium per rule class; mechanical for `Oracle `/`Inspur ` prefixes, manual for order strings |
| false-merge risk | none | low, as long as it only groups display and never feeds target/manufacturer counts | highest: pulls QCT (subsidiary) evidence under Quanta's target; hides role distinction (policy: company roles stay separate) | low if pairs are explicit; medium if done by pattern |

## evidence already in the catalog

- QCT = Quanta Cloud Technology: yes. `research/inventec-qct-2026-10-04.md` cites the original QCT QxVDI reference architecture
  (p.40) expanding QCT as Quanta Cloud Technology and naming Quanta Computer Inc. as parent (hashed). it is a
  parent/subsidiary statement, not "same entity"; the registry (`odm-target-registry.md`) deliberately keeps QCT out of the
  Quanta alias. so B can record `QCT` and `Quanta Cloud Technology` as `abbreviation` of one another and `subsidiary_brand`
  of Quanta Computer. that the *`Quanta`-branded Red Hat/Ubuntu rows* mean the QCT product is an inference from the shared
  QuantaGrid/QuantaPlex model strings; SPEC does show sponsor Quanta Computer Inc. printing vendor `Quanta Cloud Technology`.
- Sun Microsystems -> Oracle: yes, Oracle's Jan 2010 Form 8-K (sun-legacy-portfolio.md): wholly owned subsidiary, dated. same
  caveat: lineage, not same brand string; Sun-era units were not made by Oracle.
- IEIT SYSTEMS <-> Inspur: no primary source saved. the catalog only has the observation that Red Hat and Ubuntu list the same
  models under both names (39 models in Red Hat alone) and a note that IEIT "remains unassigned". that is circumstantial and
  policy says to review dated entity relationships first. needs a primary source (company filing, press release or the
  company's own site) before any mapping. no network was used here, so none was fetched.
  **update (same day, coordinator):** a follow-up lead-only search found one —
  [ieit-source handoff](batches/discovery-2026-10-11/ieit-source/handoff.md),
  [review](batches/discovery-2026-10-11/review/ieit-source-review.md) ACCEPT with notes. first-party filings (CNINFO / SZSE:
  FY2023 and FY2024 annual reports, Articles of Association) give `IEIT SYSTEMS Co., Ltd.` as the English name of
  浪潮电子信息产业股份有限公司 (000977); the earlier English name `Inspur Electronic Information Industry Co.,Ltd` and the
  2023 amendment rest on mirror copies (provisional). it covers that legal entity only, not `Inspur Corporation` or
  other strings. nothing is mapped yet; decision 4 below now asks whether to accept this evidence.
- `Inspur Corporation`, `...(IEI)` and `Inspur(Beijing) ...`: no source in the catalog on the relationship; `(IEI)` and the
  reviewed legal name `Inspur Electronic Information Industry Co., Ltd.` look like one entity but that is not yet shown.
- `Inspur Power Commercial Systems Co., Ltd.` is a separate (Power/IBM-platform) company: keep out (1 group, FP5280G2).

## recommendation

A now, then B limited to families with evidence, then D for prefix rules. skip C for now.

1. keep A (write the not-merged note in PROGRESS; model-collisions is the lead list).
2. build B as display grouping only: Quanta group = {Quanta, QCT, Quanta Cloud Technology} (typed relations, evidence already
   in hand) and Oracle group = {Oracle, Sun Microsystems} (8-K). start with those two; add Inspur rows only after a primary
   IEIT source. covers 16 + 5 groups at no id churn. validator: every row needs evidence url/sha, relation type, reviewer, date.
3. D, narrowly: mechanical own-brand model-prefix aliases (`Oracle SPARC S7-2` -> `SPARC S7-2`, `Inspur NF5280M5`) for 18+6
   groups, since the printed name is itself the evidence; hand-curated pairs only for order strings (`NF5280-M7-A0-R0-00`).
4. C only if B proves insufficient. it is the only option that changes ids and Quanta target counts, so it needs a full rebuild
   and the user should see before/after counts first. note `tests/test_catalog.py` mentions Quanta Cloud; check before C.
5. keep Accton/Edge-Core/SMC/IgniteNet etc. out of all of this; they are role relationships.

## decisions for the user

1. go with A + B (display-only family grouping), or want C? (recommend: B, no C)
2. is `QCT`/`Quanta Cloud Technology` -> Quanta group acceptable as a typed *subsidiary/abbreviation* relation? **update
   (same day):** first-party evidence now exists — Quanta Computer's affiliated-companies list (as of 2025-12-31) holds
   the Taiwan entity `Quanta Cloud Technology Inc.` 100%, and QCT's own (undated) About pages name Quanta Computer as
   parent ([family-evidence](batches/discovery-2026-10-11/family-evidence/handoff.md), review ACCEPT with notes). it
   supports a display-only brand/group row, not a tie from catalog rows to one QCT legal entity.
3. Sun Microsystems -> Oracle family, using the 8-K as evidence: ok?
4. IEIT <-> Inspur: a primary source now exists (see the update above). accept a family row for the legal-entity pair
   `IEIT SYSTEMS Co., Ltd.` ↔ `Inspur Electronic Information Industry Co., Ltd.` on the first-party current-name evidence,
   wait for a first-party copy of the 2023 rename, or leave it unmapped?
   (also: a 2025 US Federal Register BIS rule lists `Inspur Information` and `IEIT Systems Co. Ltd.` as aliases of
   `Inspur Electronic Information Industry Co., Ltd.` — government corroboration; `Inspur (Beijing) …` is a separate
   100%-held subsidiary; no primary source names `Inspur Corporation` or `(IEI)`, so those stay unmapped.)
5. should family grouping ever feed `Quanta`/`Inspur` target counts and vendor-inventory? recommend no; family hits show
   beside but separate, manufacturer roles untouched.
6. model prefix aliases (D): pattern rule for own-brand prefixes, or explicit pair list only?
7. who is reviewer of record for the family table, and where does it live (suggest `data/brand-families.json` in the
   manifest)?

## implemented 2026-10-11 (option B display-only + D rule)

user decision 2026-10-11: "Display-only table (Recommended)". built; all 7 rows signed `accepted` by the independent reviewer 2026-10-11 ([review](batches/discovery-2026-10-11/review/brand-family-table-review.md), ACCEPT with notes).
the IEIT <-> Inspur row is NOT included (the user chose the option without it); `Inspur Corporation`, `(IEI)`,
`Inspur(Beijing) ...`, `Sun Fire`, `Quanta Microsystems` and `Quanta Computer Inc.` (not a catalog brand) are not members.

- `data/brand-families.json` (schema_version 1, the only data file written): families `quanta` (group entity Quanta
  Computer Inc.: `Quanta` own_brand; `QCT` abbreviation + subsidiary_brand; `Quanta Cloud Technology`,
  `Quanta Cloud Technology Inc.` subsidiary_brand) and `oracle` (group entity Oracle Corporation: `Oracle` own_brand;
  `Sun Microsystems`, `Sun Microsystems, Inc.` acquired_company). brand strings match on `catalog.normalize`, so the
  catalog's `ORACLE` / `SUN MICROSYSTEMS` spellings resolve without extra rows. every brand string was checked against the
  catalog (read-only) and the counts stored per row.
- evidence per row: Quanta group = Quanta Computer's affiliates list as of 2025-12-31 (dating anchor, sha `068ff9f9...`),
  QCT About page (undated, `as_of` null with a note, sha `5e53264c...`), QxVDI 2020 (sha `32fcf545...`); caveat kept in
  the file: the 100% holding is of QCTTW only, the relation is group-level, no claim about which legal entity a catalog
  row means and no manufacturer claim. Oracle group = Form 8-K item 2.01 factual extract
  (`data/raw/rra-sun-official/oracle-sun-acquisition.json`, sha `37ae069c...`; the sha is of the reviewed extract, not the
  original HTML, which returned 403), dated 2010-01-26, lineage only (Sun-era units were not made by Oracle).
- all 7 rows: `review_status: accepted`, signed by the independent reviewer (originally written pending/null; see
  [brand-family-table handoff](batches/discovery-2026-10-11/brand-family-table/handoff.md)). every output reports
  `rows_pending_review` (now 0). post-review edits touched only the QxVDI corroborating evidence item (real go.qct.io URL,
  `as_of` null with a date note) and text fields, not the signed fields.
- D rule: `model_prefix_aliases` stores the rule (`own_brand_leading_token` = `model_collisions.stripped_key`, same
  normalised brand only) and its generated group list (25 groups at generation: 18 Oracle, 7 Inspur-string) for audit;
  regenerate with `python3 -m devicehunt.brand_families regenerate-aliases --write`. the code is authoritative, the stored
  list may go stale as the catalog grows. the 7 Inspur-string groups are same-brand audit entries for strings the table
  deliberately does not map; they imply no IEIT/Inspur relation, and no output consumes aliases (D exists as a rule,
  function and audit list, not an "also known as" display).
- display only: `lookup` catalog rows gain `brand_family` (+ `family_members_hit` in JSON); `match-listing` candidates
  gain `brand_family` and `same_family_candidates` (other candidates in the family, counted before the limit);
  `model-collisions` groups gain `family {ids, unmapped_members, same_family}` and the result `groups_same_family`
  (live: 39 of 362 groups fully inside one family, Quanta 17, Oracle 22; 6 more are partial). each output carries
  `brand_families {path, sha256, rows_pending_review, ...}`. the library functions default to no family table; the CLI loads
  the file when it exists. `vendor-inventory`, `TARGETS`, ids and target counts do not import or read it; a test byte-compares
  `vendor-inventory` output (json/csv/table) with and without the file.
- not done: decision 7 (manifest input / reviewer of record) is open; the file is not yet in the import manifest.
