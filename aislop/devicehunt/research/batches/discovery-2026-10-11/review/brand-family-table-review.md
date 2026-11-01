# brand-family table independent review (Sonnet 5.5), 2026-10-11

Reviewer did not author the table or code. Offline only; nothing installed or fetched; `data/archive` and campaign code untouched; catalog read-only. Scratch: `.../scratchpad/rev/` (`code/` = copy of `devicehunt/` with `brand_families.load_default()` patched to return `None`; `mut/` = mutation copy; outputs `out/off`, `out/on`).

## Verdict: ACCEPT with notes (2 must-fix test-hygiene items, no change to the core finding; 4 should-fix; all 7 rows signed `accepted`, see "Sign-off")

The display-only guarantee holds on the real catalog; the evidence behind every row re-hashes and says what the rows say; the matching resolves exactly the intended catalog brands and nothing else. The two must-fix items are about the tests (an order-dependent failure when the new tests run with the neighbouring modules, and a ranking-mutation gap); neither is a defect in the table or in the display code.

## 1. Display-only guarantee: verified

Catalog: `data/devices.sqlite` sha256 `e7cc01a1e39f9140d8e8209699c65707ae62b62d45f504aa06f57126015e75da`, mtime 07:54:20, 5,107 identities. Re-checked at the end of the review: same sha and mtime (the live RRA refresh writes `data/archive`, not the catalog).

- **vendor-inventory**: scratch code (family loading disabled) vs real code, 13 targets (Accton, Edge-Core, Celestica, Oracle, Amazon Data Services, Inventec, Hyve, Foxconn, Wiwynn, TD SYNNEX, Quanta, ByteDance, Inspur) x scopes manufacturer / factory / all, `--limit -1 --format json` = 39 files, plus the all-targets `--scope all` in json (11.1 MB), csv (8.3 MB) and table: `diff -r out/off out/on` empty, byte-identical, all stderr empty. A first harness run was vacuous (wrong `--db` position, empty outputs); I fixed it and the result above is from the corrected run with non-empty outputs.
- **lookup / match-listing / model-collisions row sets**: 18 invocations, full `--limit -1` JSON, family keys stripped from the real-code output (`brand_family`, `family_members_hit`, `same_family_candidates`, `brand_families`, `family`, `groups_same_family`) == scratch-code output, identical in content AND order, and the scratch output contains no family key at all: lookup `QuantaGrid D51B-2U`, `SPARC S7-2`, `NF5280M7`, `Sun Fire X4470`, `QCT`, `Oracle` (1,247 labelled rows), `Sun Microsystems`, `Crossbow USB`; model-collisions (all, `--target Quanta`, `--target Oracle`, `--all-kinds`); match-listing for QCT D52B-1U, Quanta D51B-2U, Oracle SPARC S7-2, `Sun Fire X4470 Oracle` (mixed family/unmapped), Quanta T41S-2U at `--limit 3`, Inspur NF5280M7. 0 differences. Table and CSV renderings were eyeballed only (they add the `brand_family` column and a banner by design).
- Code read: `vendor_inventory.py`, `catalog.py` (TARGETS, `target_for`) do not import or reference `brand_families`; the three `__main__.py` call sites only pass `load_default()` and print the banner; lookup/listings/model-collisions annotate after sorting, limiting and counting (listings computes the hint over all candidates before the limit; model-collisions annotates after `groups.sort` and `counts`). `lookup.run_many` and `lookup-batch` do not pass a table (stay unannotated; fine).

## 2. Evidence per row

All four distinct captures exist and re-hash to the recorded sha256 (my own hashing, plus `python3 -m devicehunt.brand_families check`: `ok 4, absent [], mismatch []`, `missing []`):

| path | sha256 prefix | bytes |
| --- | --- | --- |
| `data/raw/discovery-2026-10-11-family-evidence/quanta-affiliates-2026-organize26en.pdf` | 068ff9f9 | 861,028 |
| `data/raw/discovery-2026-10-11-family-evidence/qct-company-aboutqct.html` | 5e53264c | 52,498 |
| `data/raw/inventec-qct-2026-10-04/qct-qxvdi-2020.pdf` | 32fcf545 | 3,471,706 |
| `data/raw/rra-sun-official/oracle-sun-acquisition.json` | 37ae069c | 891 |

Content re-read independently (existing nix `pdftotext`, stdlib HTML strip under `python3 -I`): Q2 cover `Summaries of Affiliated Companies / December 31, 2025`; PDF p.5 `Quanta Cloud Technology Inc. (QCTTW) 02/08/2007 ... Taoyuan City ... NT$ 482,000 ... Sale of computers and peripherals`; PDF p.12 `Quanta Cloud Technology Inc. (QCTTW)  Quanta Computer Inc.  48,200  100.00%`, chairman Barry Lam. QCT About: `The parent company of QCT is Quanta Computer, Inc.`; `in May of 2012 QCT was launched under the name Quanta Cloud Technology as a subsidiary of Quanta Computer Inc.`; footer `QCT, the QCT logo, Rackgo, Quanta, and the Quanta logo are trademarks or registered trademarks of Quanta Computer Inc.`; body `more than 14-years`. QxVDI PDF p.40 `The parent of QCT is Quanta Computer Inc.`, pages 2-3 `Copyright 2020-2021 Quanta Cloud Technology Inc.`. All statements in the JSON match, including the caveats (100% is QCTTW only; QCT page undated, Q2 is the anchor).

Relation labels and dates:
- `Quanta` own_brand: honest, resting on the Q1 trademark footer; the row date 2025-12-31 is the family dating anchor (Q2 does not itself speak about the `Quanta` brand string, Q1 does, undated). Acceptable for a display grouping.
- `QCT` abbreviation + subsidiary_brand (`abbreviation_of` Quanta Cloud Technology): honest. "subsidiary" is Q1's word; Q2 says "affiliated company" plus 100% direct (QCTTW). The scope note and statement keep the group-level, no-legal-entity, no-manufacturer limits. Caveat present on all 4 rows.
- `Quanta Cloud Technology`, `Quanta Cloud Technology Inc.` subsidiary_brand: honest; the second is the exact Q2 legal-name string and its note says no claim that a catalog row means that entity.
- Oracle / Sun Microsystems / Sun Microsystems, Inc.: own_brand and acquired_company, lineage only, `scope_note` says Sun-era units were not made by Oracle.

**8-K row (cites a reviewed extract, not the original HTML): acceptable.** The JSON says in the row itself that the sha256 is of the extract, the original returned 403 and is not cached; the extract keeps Item 2.01 (completion 2010-01-26, "wholly owned subsidiary") and Item 8.01 (announcement 2010-01-27) apart and scopes itself to corporate acquisition only. The claim is narrow and not contested, the user's decision 3 already named the 8-K as the evidence, and the project already accepts saved reviewed extracts as a standard (Celestica rows, see sun6000-a-design-review). Limits that stay: I could not check the extract against the filing (no network), so the day-level date rests on the earlier reviewed capture; the sha anchors a derived file, not SEC text. It should be re-captured from the SEC original (declared UA, polite) when convenient, and the hash swapped then.

Evidence defects (non-blocking):
- S1. The third QCT evidence item (QxVDI) has `url: "https://www.qct.io/ (QCT QxVDI reference architecture 2020, p.40; see research/inventec-qct-2026-10-04.md)"`, which is not a URL (the validator only checks the `http` prefix). The real URL is in `research/inventec-qct-2026-10-04.md` line 91: `https://go.qct.io/wp-content/uploads/2020/05/QxStack-QxVDI-OAHC-High-Density-Optimized-SKU-Reference-Architecture-EN.pdf`. It is the corroborating third item on three rows; the dated anchor and the first-party statements (Q2, Q1) carry the bar by themselves. Fix the field, put page/context in `statement`, and make the validator reject whitespace in `url`.
- S2. The QxVDI `as_of` `2020-01-01` is a synthetic first-of-year date (disclosed in `date_note`). Prefer `as_of: null` with the note, so a date field never holds an invented day.

## 3. Normalisation matching: no unintended brands

`family_of` = `catalog.normalize(brand)` equality against member brands (not substring). Catalog brands resolving (live catalog, 5,107 identities):

| family | catalog brand strings | identities |
| --- | --- | --- |
| quanta | `Quanta` 162, `QCT` 9, `Quanta Cloud Technology` 11, `Quanta Cloud Technology Inc.` 3 | 185 |
| oracle | `Oracle` 1,217, `ORACLE` 2, `Sun Microsystems` 32, `SUN MICROSYSTEMS` 2, `Sun Microsystems, Inc.` 1 | 1,254 |

I also scanned every brand whose normalised form contains `quanta`, `qct`, `oracle` or `sun`: the only unresolved ones are `Quanta Microsystems` (1) and `Sun Fire` (3), both deliberately out. No Inspur/IEIT/Edge-Core/Accton string resolves (also asserted in `RealFileTests`). The `Quanta` brand does carry odd FCC-type products (`GO920`, `HSTNN`, `TPN`...), which is the catalog's own brand assignment, not the family table's doing, and is display-only. Note for readers: model-collisions `same_family` for `brand_token_prefix` groups (18 Oracle) is trivially true because both members share one brand; the flag does not mean a cross-brand relation there.

## 4. Alias rule and drift

- Rule: `alias_key` -> `stripped_key` from `model_collisions` (strip ONE leading own-brand token or first word of a multi-word brand; `test_alias_rule_is_the_model_collisions_rule` asserts equality). Same brand only (`catalog.normalize`).
- Reproducibility: `python3 -m devicehunt.brand_families check` reports `alias_list_current: true`; my own `generate_alias_pairs` on the live catalog gives 25 groups == the stored list (`generated_from` digest corresponds to the current 5,107-identity catalog). Composition: 18 Oracle, 5 `Inspur Corporation`, 2 `Inspur Electronic Information Industry Co., Ltd. (IEI)`. The proposal's "18+6" and the handoff's "7 Inspur-string" both describe this (catalog grew from the proposal's 5,015 to 5,107 before the table was built).
- Drift vs `model-collisions`: all 19 model-collisions `brand_token_prefix` groups are in the alias list; the 6 extra alias groups sit inside `brand_token_prefix_different_brand` groups (the same-brand pair inside a multi-brand group). The alias grouping compares `catalog.normalize(brand)` (alphanumerics only) while model-collisions compares `casefold + collapsed whitespace`, so brands differing only by punctuation would group in the alias list but count as different brands in model-collisions; no such pair exists today.
- S3. The 7 Inspur-string pairs are for brand strings the family table deliberately does not map (`Inspur Corporation`, `(IEI)`). They are same-brand pairs only and imply no IEIT/Inspur relation, but a reader can mistake them for one; the rule description should say so. Also no output consumes the aliases: D exists as a function, a rule and an audit list, with no "also known as" display (model-collisions already shows them as `brand_token_stripped`). Say so in README/proposal so nobody expects an alias lookup.

## 5. Tests

Run: `tests.test_brand_families` (19), `test_lookup`, `test_lookup_batch`, `test_lookup_match_context`, `test_listings`, `test_listing_software_refs`, `test_listing_specifications`, `test_model_collisions`, `test_readonly_catalog_commands`, `test_vendor_inventory`.
- The 9 neighbouring modules alone: 70 tests OK. `tests.test_brand_families` + `test_readonly_catalog_commands` alone: OK. All 10 modules in one process (89 tests, about 55 s): **1 failure in 3 of 3 runs** (`test_readonly_catalog_commands...preserve_database_and_return_actual_records` asserts empty stderr and receives `ResourceWarning: unclosed database in sqlite3.Connection`). Cause: the new tests call `main()` about 20 times in-process and `__main__` uses `with sqlite3.connect(...)` (which does not close); the leaked connections are finalised during a later test inside its stderr capture. Passing in the handoff's runs is GC-timing luck. Verified fix: calling `gc.collect()` (under `warnings.simplefilter('ignore')`) at the end of `DisplayOnlyTests.cli()` makes all 89 pass (scratch copy). The full suite order (`discover -s tests`, alphabetical, brand_families early) is exposed to the same thing; I did not run the 12-20 min full suite.
- Mutants (scratch copy, each against `tests.test_brand_families`):
  - `catalog.target_for` consults the family table (QCT -> Quanta): caught (byte-compare + `target_for('QCT') is None`).
  - `vendor_inventory` feeds on family via obfuscated import (evades the `'brand_families' not in source` grep) and merges family brands into targets: caught by the byte-compare (identities 10 vs 9).
  - model-collisions re-sorts groups by `same_family`: caught.
  - match-listing `same_family_candidates` counted over shown candidates only: caught.
  - **match-listing candidates re-sorted so unmapped come first: survived.**
  - **lookup rows re-sorted so unmapped come first: survived.** Cause: the fixtures never mix a mapped and an unmapped brand in one result (the lookup queries and the listing title all hit only Quanta or only Oracle rows), so a family-aware sort is a no-op. The real-data check above covers it today (`Sun Fire X4470 Oracle` mixes both), but the test suite does not.
- Meaningfulness otherwise good: the byte-compare for vendor-inventory (json/csv/table, 4 arg sets) and the strip-equality tests for all three surfaces, plus the CLI no-write check, are real guards.

## Findings

### Must-fix
- M1. Test isolation: the combined run above fails (readonly-catalog test) because the new CLI tests leak sqlite connections whose `ResourceWarning`s surface in another test. Add `gc.collect()` under `warnings.simplefilter('ignore')` after each `main()` in `cli()` (verified), or close connections in `__main__`.
- M2. Add one mixed fixture to the display-only tests: one query/title returning a mapped and an unmapped brand together (e.g. `Inspur Corporation`/`Inspur` row beside a `Quanta` row, or a `Sun Fire` model beside Oracle), asserting order equality for lookup rows and match-listing candidates with and without the table. Both ranking mutants above survive today.

### Should-fix
- S1. QxVDI evidence `url` is not a URL; use the go.qct.io URL; validator should reject whitespace.
- S2. QxVDI `as_of 2020-01-01` is synthetic; use null + date_note.
- S3. State that the 7 Inspur-string alias pairs are same-brand audit entries only, and that no output consumes aliases.
- S4. After sign-off these now-stale texts need updating (outside my allowed edits): `data/brand-families.json` top-level `review_note` ("All rows are review_status pending"), proposal "implemented" section ("all 7 rows pending"), brand-family-table handoff ("Rows (all pending)"). Also correct the handoff statement that "the catalog gained identities during the live refresh": it did not. `devices.sqlite` (sha `e7cc01a1...`, mtime 07:54, 5,107) is unchanged, has 5,107 identities against 5,084 in `devices-before-discovery-2026-10-11-f.sqlite`; the mtime 07:54 is consistent with the candidate-f import (inferred from the snapshot name and mtimes, not observed) and falls about two hours BEFORE `brand-families.json` was created (10:06). Verified across the whole review window: sha and mtime unchanged. The numbers the handoff quotes (5,107, 25 alias groups, 39 same-family groups of 362) all match the unchanged catalog.

### Notes
- Reviewer-of-record is this review; the file is still not in the import manifest (decision 7 open, unchanged).
- The IEIT row is absent as decided; `Inspur`, `IEIT`, `Inspur Corporation`, `(IEI)`, `Sun Fire`, `Quanta Microsystems` all unresolved.
- Output counts verified live: model-collisions 362 groups, 39 fully inside one family (Oracle 18 prefix + 4 cross-brand = 22, Quanta 17), 6 more partial (Oracle 4 including the `Sun Fire` X4470 group, Quanta 2).

## Sign-off

Rows meeting the bar (primary/first-party statement or accepted reviewed extract, typed relation, date, url+sha, caveats present): all seven.
- quanta: `Quanta`, `QCT`, `Quanta Cloud Technology`, `Quanta Cloud Technology Inc.`: accepted.
- oracle: `Oracle`, `Sun Microsystems`, `Sun Microsystems, Inc.`: accepted, on the extract basis described in section 2 (re-capture of the original 8-K recommended).
Quanta rows are accepted on evidence[0] (Q2) + evidence[1] (Q1); S1/S2 concern only the corroborating QxVDI item, and this signature does not endorse that malformed field. Signed in `data/brand-families.json` (`review_status: accepted`, `reviewer: "Claude Sonnet 5.5 independent reviewer (devicehunt discovery batch, 2026-10-11)"`, `reviewed_on: "2026-10-11"`); validation re-run afterwards, result below.

## Post-sign validation

- Edit method: backup to scratchpad, Python load/rebuild/dump in the module's own writer format (`json.dumps(..., ensure_ascii=False, indent=2) + '\n'`; the pre-edit file round-tripped byte-identically). `diff` before vs after = 35 changed lines: per row 1 `review_status` pending -> accepted, 1 `reviewer` null -> name, 1 inserted `reviewed_on`, x 7 rows, nothing else.
- New `data/brand-families.json` sha256 `71d12ac0755efd11a7ef148034c76a57929a9d292ffbca8ccb287fa5ad8c72f9` (replaces `a3f1a9b8...`).
- `python3 -m devicehunt.brand_families check`: exit 0, `rows_pending_review: 0`, `missing: []`, evidence `ok 4 / absent [] / mismatch []`, `alias_list_current: true`.
- `python3 -m unittest tests.test_brand_families`: 19 tests OK (`RealFileTests` now exercises the accepted-row validator path).
- `lookup 'QuantaGrid D51B-2U'` banner now reports the new sha and `0 of 7 rows pending review`.
- Catalog sha `e7cc01a1...` and mtime 07:54:20 unchanged after all of the above; no other file outside this review was edited.

## re-review

## Verdict: ACCEPT (0 must-fix; all 7 signatures re-confirmed, none revoked)

Checked file sha256 `e1c70517a18b03b07a7a555875b7d2dc0f74ec70f8ffbc92e116d5bb9f9b237e`; catalog still `e7cc01a1...`.

- Field-level diff of the new file against my pre-sign backup (walked every key, not text diff): beyond my own sign-off edits, only (a) QxVDI evidence item on the 3 rows (QCT, Quanta Cloud Technology, Quanta Cloud Technology Inc.): `url` now the real `https://go.qct.io/wp-content/uploads/2020/05/QxStack-QxVDI-OAHC-High-Density-Optimized-SKU-Reference-Architecture-EN.pdf` (identical to `research/inventec-qct-2026-10-04.md` line 91), `as_of` null with `date_note` "document copyright 2020-2021; no exact day stated", `statement` adds "p.40 (see research/inventec-qct-2026-10-04.md)" and keeps the same quotation; `local_path` and `sha256` unchanged; (b) new `model_prefix_aliases.rule.audit_note` (S3 wording, accurate); (c) `review_note` rewritten (accurate; it discloses the post-signature edit). `review_status`, `reviewer`, `reviewed_on` on all 7 rows are exactly as I signed them.
- `brand_families check`: exit 0, 0 pending, missing [], evidence ok 4 / mismatch [], `alias_list_current: true`.
- Signatures on the 3 changed rows: the edits fix exactly S1/S2 on the corroborating item, are honest (no invented date, real URL, same quote), and do not touch the evidence that carried the bar (Q2 + Q1). Re-confirmed, not revoked. Oracle and `Quanta` rows unchanged.
- Mutants re-run against the new tests (scratch copy of current code): baseline OK; match-listing sort unmapped-first FAIL (caught by `test_mixed_mapped_and_unmapped_brands_keep_the_same_order`); lookup sort unmapped-first FAIL (caught); match-listing sort mapped-first FAIL (caught). M2 closed.
- Ten modules in one process (brand_families, lookup, lookup_batch, lookup_match_context, listings, listing_software_refs, listing_specifications, model_collisions, readonly_catalog_commands, vendor_inventory): 91 tests, OK. M1 closed (the `gc.collect()` is in the CLI helper).
- Remaining (not blocking): proposal "implemented" section and the table handoff still say all rows pending / catalog grew during the refresh; the 8-K original re-capture is still a recommendation.
