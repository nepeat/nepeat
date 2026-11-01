# brand_family_table handoff (2026-10-11)

Built per user decision "Display-only table (Recommended)": a reviewed brand-family table used ONLY to group lookup /
match-listing / model-collisions output. No identity, target-count or vendor-inventory change. No IEIT/Inspur row.

## Files
- `data/brand-families.json` (new; only data file written). sha256: see `python3 -m devicehunt.brand_families check` (changes on any edit).
- `devicehunt/brand_families.py` (loader/validator, `family_of`, `alias_key`, catalog and evidence checks, alias generator, CLI)
- display-only additions: `devicehunt/lookup.py`, `devicehunt/listings.py`, `devicehunt/model_collisions.py`; `devicehunt/__main__.py` (3 call sites pass `brand_families.load_default()` plus a one-line hash banner in table output; no new flag)
- `tests/test_brand_families.py` (19 tests), `README.md` (subsection), `research/brand-family-proposal-2026-10-11.md` (implemented section)
- not touched: campaign*/capture/artifacts/rra*, vendor_inventory.py, catalog.py, anything under data/ besides the new file.

## Rows (all 7 signed `accepted` by the independent reviewer 2026-10-11; originally written pending)
| family | brand string | relations | date | evidence |
| --- | --- | --- | --- | --- |
| quanta (Quanta Computer Inc.) | Quanta | own_brand | 2025-12-31 | Q2 affiliates list sha 068ff9f9...; Q1 QCT About (undated) sha 5e53264c... |
| quanta | QCT | abbreviation, subsidiary_brand (abbreviation_of Quanta Cloud Technology) | 2025-12-31 | Q2, Q1, QxVDI 2020 sha 32fcf545... |
| quanta | Quanta Cloud Technology | subsidiary_brand | 2025-12-31 | Q2, Q1, QxVDI |
| quanta | Quanta Cloud Technology Inc. | subsidiary_brand | 2025-12-31 | Q2, Q1, QxVDI |
| oracle (Oracle Corporation) | Oracle | own_brand | 2010-01-26 | 8-K item 2.01 extract `data/raw/rra-sun-official/oracle-sun-acquisition.json` sha 37ae069c... (extract, not original HTML) |
| oracle | Sun Microsystems | acquired_company | 2010-01-26 | same |
| oracle | Sun Microsystems, Inc. | acquired_company | 2010-01-26 | same |
Case variants (`ORACLE`, `SUN MICROSYSTEMS`) resolve via `catalog.normalize`. Left out on purpose: `Quanta Computer Inc.` (not a catalog brand), `Sun Fire`, `Quanta Microsystems`, all Inspur/IEIT strings.

## Post-review changes (2026-10-11)
- Reviewer signature fields (`review_status`, `reviewer`, `reviewed_on`) untouched. Evidence fields of signed rows DID change in the corroborating QxVDI item only (on QCT, Quanta Cloud Technology, Quanta Cloud Technology Inc.): `url` is now the real go.qct.io PDF URL, `as_of` null with a `date_note`, `statement` adds page context. Evidence sha256/path unchanged. Also `review_note` and the alias `rule.audit_note` text.
- Validator rejects whitespace in `url`. Tests: gc.collect in the CLI helper (M1), mixed mapped/unmapped order fixture (M2), URL test.
- The earlier statement that the catalog gained identities during the refresh was wrong: devices.sqlite was unchanged (5,107 identities).

## For the independent reviewer (done)
1. Check each row against the cited evidence (the family-evidence review already ACCEPTED the evidence with notes S1-S3; that is an evidence review, not row sign-off). Points to confirm: QCT page is undated, so Q2 is the dating anchor; the 100% holding is of QCTTW only; the relation is group-level; Oracle sha is of the reviewed extract.
2. Judge whether `QCT` -> `abbreviation` + `subsidiary_brand` and `Quanta` -> `own_brand` (trademark owner per Q1 footer) are the right relation labels, and whether to display pending rows (currently yes, flagged).
3. To sign: set `review_status` to `accepted`, `reviewer` to the reviewer's name, add `reviewed_on` (the validator enforces these); `rejected` rows are ignored by `family_of`. Do not hand-edit `model_prefix_aliases`; regenerate it.
4. Open: decision 7 (reviewer of record; whether the file joins the import manifest).

## Verification (read-only, live catalog, catalog unchanged, 5,107 identities)
- Stripping the new keys from the "after" JSON reproduces the "before" JSON exactly for: `lookup NF5280M7` (7 catalog rows, all `brand_family: null`, no family change), `lookup 'QuantaGrid D51B-2U'` (QCT + Quanta rows both `quanta`, `family_members_hit ['QCT','Quanta']`), `lookup 'SPARC S7-2'` (20 shown / 44 matches, Oracle rows `oracle`), `match-listing --title 'QCT QuantaGrid D52B-1U'` (1 candidate, Quanta, `quanta`, `same_family_candidates 0`; the catalog has no QCT-branded D52B-1U row), `model-collisions --target Quanta --limit 5` (groups, order, counts identical; 3 of the 5 shown flagged `same_family`).
- `vendor-inventory` (all, Quanta json, Oracle csv) byte-identical before vs after.
- Tests: `tests.test_brand_families` (19), lookup, lookup_batch, lookup_match_context, listings, listing_software_refs, listing_specifications, model_collisions, readonly_catalog_commands, vendor_inventory: all pass. Full-suite result is reported separately in the final message.
- One existing test constrains the lookup table layout (`matched_in` last, `identity_text_match` second to last), so the `brand_family` table column sits after `kind` (before `source_ids`); JSON keys are appended.
