# edgecore_ordering independent evidence review (Sonnet 5.5), 2026-10-10

Reviewer did not author the handoff. Scratch: .../scratchpad/review-edgecore/ (text extractions, page PNGs, temp catalog, 2 refetches).

## Verdict: ACCEPT with notes (no must-fix)

117/117 records verified against the printed Ordering Information tables (text extraction + visual check of all six pages). Zero code, description,
part-number, page, or column-misalignment errors. Four should-fix metadata items below should be settled by the integrator; none changes an identity.

## What was checked and found sound
- Ledger: 14 entries, sequential; all 10 kept files re-hash to the ledger sha256 and byte counts; JSON sha ff8d50b6... matches handoff. Four 404 URLs appear exactly
  once each (file null; sha/bytes are those of the deleted 404 HTML body, as the note says). Redirect r14 -> r15 recorded in final_url. No duplicate URLs.
- Upstream spot check (2 refetches, to scratch): 7726 R09 and AS4630-54PE R10 PDFs are byte-identical (sha256 equal) to the saved copies.
- Switch rows (75 = 8 + 12 + 10 + 18 + 20 + 7): independent re-parse of every printed row by cell split; ordering_description cells equal printed cells for all 75; code/model/key
  identical. Exact strings preserved (letter O vs zero, -F/-B, -UN/-US/-EU/-UK/-JP/-CN/-TW, -R-, -OZ-, bare -O-AC-F/-B, 7816 -O-48V-F-R, 4630 "AS" prefix only where printed).
  Part numbers match (7712 prints none, none invented). Base model matches: AS7816 -OZ rows -> "AS7816-64X-OZ series", -O rows -> "AS7816-64X-O series". Pages: 7712 p3, others p4; doc ids EC-DS-0716-03/0723-R06/0725-R10/0726-R09/0716-R15/1025-R10 match footers.
- Accessories (42 = 9 + 9 + 4 + 9 + 8 + 3): every code, part number, description and wrapped cell checked against text and rendered page. No column shift. Wrapped cells are joined correctly
  (5812 ORSA-1U; 4630 PSU/FAN/RKIT "EPS202/AS4630-54PE", "DCS501/AS7712-32X"; 7712 77xx-32X-SVC-RTF). 7816 FRU/FAN headers really print "Type" over a part-number-like cell
  (visually confirmed), stored unlabeled: correct, no relabeling. Printed oddities kept: "YM-2651Y, B01R", "YM-2851F, C01R", "DPS-850AB-5 A", "YM-2851J ER", 7712 en dash, FAN-1U-1x1C-B2.
- Identity rules: no AS prefix added (68 unprefixed switch models); catalog device id is brand+model+revision, no prefix stripping. Live data/devices.sqlite (opened read-only) has 0 exact collisions with the 115
  imported models; its AS5812-54X-O-48V/12V-F/B rows stay separate from the printed 5812-54X-O-48V/12V-F/B (12V and 48V remain separate printed variants; no -DC- decoding anywhere). The only
  merges are byte-identical strings: FAN-1U-1x1C-F (7712, 7726) and RKIT-100G-SLIDE (7726, 4630); both source records keep their own facts (117 records, 618 facts in the temp catalog).
  No compatibility/airflow meaning added beyond printed text; "for EPS202/..." lists remain table text only.
- Owned-code gap re-checked independently: 7760-32X-A-AC-F, AZ4444-52X-DC-10, DMC-12-48R-AS0, AS7772-32X (and looser 7760/7772/AZ4444/DMC-12/-A-AC patterns) have zero hits in
  pdftotext -layout and -raw text of all six PDFs (all pages), in the four HTML pages, and in the JSON. No "-A-" order code exists in any table. Gap claim holds.
- Import: snapshot resolves via Path(json).parent / snapshot (all six exist beside the JSON). Temp catalog (devicehunt.catalog.Catalog + pipeline.import_source 'curated'): returned 117;
  devices 115, sources 6, records 117, facts 618, integrity ok, foreign_key_check empty, all brand Edge-Core.

## Findings
1. should-fix, source coverage text stale (all six sources, e.g. 7712 p3): `coverage` still says "FRU/fan/PSU/accessory/service rows on the same page are not extracted" and "Rows transcribed
   literally by pdftotext -layout", but 42 accessory rows are now included and those were hand-keyed then word-checked. Correct the string (build_facts.py DOCS coverage) and re-hash.
2. should-fix, ligature inconsistency: 57 switch descriptions (7712 p3, 5812/7326/7816 p4, 4630 p4) keep the U+FB02 ligature in "Airﬂow"; accessory descriptions use plain "Airflow"; 7726 has none
   printed. Field labels are plain. Plain-text search for "airflow" will miss the 57 rows. Either normalize the ligature everywhere or state the policy in the handoff (handoff only flags it as an open question).
3. should-fix, brand on third-party-looking PSU codes: all 42 accessories carry affiliation (Edge-Core, brand), including YM-2651Y/YM-2851F/YM-2851J, DPS-850AB-5 A/-6 A, CPR-4011-4M11/4M21, UM400D01(-01),
   YPEB1200AM-1A03P10 (7326 p4, 7816 p4, 5812 p4, 4630 p4). The sheets only print Edge-Core as document brand; these strings are listed as orderable FRUs, not branded. Prefer a source-scoped
   "listed in Edge-Core datasheet" role, or drop the brand affiliation for these rows; do not let a brand/manufacturer claim be inferred.
4. note, base-model attribution is positional: accessory fact `listed_with_base_model_as_printed` = last switch table on the page. Rows are shared across tables where printed text says so (7816 FRU/FAN follow the -O table but
   the page also prints -OZ, flagged by grouping_note; 7712 77xx-32X-SVC-RTF says "AS77xx-32X series"; 4630 RKIT lists many models). Acceptable as "listed_with", never as compatibility.
5. note, merged devices: FAN-1U-1x1C-F device kind is the first import's "Fan Module FRU" (7726 prints "FAN FRUs"); 7726 prints FAN-1U-1x1C-F with part F0TAO6632000A, 7712 none. Same string only; keep records separate.
6. note, shared part numbers across different codes: F0TZZ5654019A/F0TZZ5654020A appear for 7326 "YM-2651Y, B01R/C01R" (p4) and 7726 PSU-AC-650B-F/-B (p4). Printed facts; no equivalence should be asserted from them.
7. note, 5812 doc id EC-DS-0723-R06 is the footer id of a file titled DS-R15 (p4): both are as printed. Document brand is "Edgecore Networks"; stored "Edge-Core" per task, as the handoff states. "developed and manufactured by Accton" (5812/7326/7816 p4) correctly not extracted as affiliation.
8. note, 404 ledger rows keep sha256/bytes of the deleted 404 HTML body next to file:null; harmless but a reader could mistake them for evidence of a saved capture.

## Fix verification (curated sha256 17781aab10bf...261ab)
- Baseline for the diff was my earlier scratch import (records.raw_json of the pre-fix JSON). All 117 records present with identical source/key sets; after replacing ligatures with plain letters in the baseline,
  the only differing fields are `brand` (42) and `affiliations` (42), exactly the accessory rows; no fact, model, key, title, kind or page value changed. No U+FB00-FB06 code points remain anywhere in the JSON.
- Brands: 75 switch records Edge-Core + [Edge-Core, brand]; 42 accessory records brand "Unattributed accessory" + [Edge-Core, documentation_vendor]. Findings 2 and 3 resolved.
- Coverage text: "not extracted" gone from all six sources, "hand-keyed" present (finding 1 resolved); each coverage still carries the ledger PDF sha256; snapshots exist beside the JSON, URLs equal ledger final_url; ledger file unchanged.
- Scratch re-import ('curated'): 117 records, 115 devices (75 Edge-Core + 40 Unattributed accessory; the two identical-string pairs still merge), 6 sources, 618 facts, integrity ok, FK check empty.
- Remaining notes 4-8 unchanged (non-blocking).
Updated verdict: ACCEPT (all should-fix items closed; notes only).
