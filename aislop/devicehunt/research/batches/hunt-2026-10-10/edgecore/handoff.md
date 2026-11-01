# edgecore_ordering handoff (Sonnet 5.5), 2026-10-10

## Outcome
6 official edge-core.com datasheet PDFs captured; 117 records (75 switch order codes + 42 PSU/fan/accessory rows) transcribed into a curated candidate
(not imported). 14/14 captures used (4 returned 404, recorded once). 9/15 web searches used. No general part-number-convention
page found. Of the four owned codes, none is defined by any captured official source.

## Files + hashes (data/raw/hunt-2026-10-10-edgecore/)
- edgecore-ordering-facts-2026-10-10.json 17781aab10bf88afdb62417543dd777f6adf5e0aff7f76749b8fb9ce474261ab (6 sources, 117 records, 618 facts); capture-ledger.json (14 entries, sequential)
- as7712-as7716-32x-onie-ds-r03-20160707.pdf 21d08cb5... (EC-DS-0716-03, ordering PDF p3)
- dcs208-as5812-54x-ds-r15.pdf 701cfa82... (EC-DS-0723-R06, p4)
- dcs203-as7326-56x-ds-r10.pdf 23267a41... (EC-DS-0725-R10, p4)
- dcs204-as7726-32x-ds-r09.pdf c15ce7bf... (EC-DS-0726-R09, p4)
- dcs500-as7816-64x-ds-r15.pdf 0f768fc1... (EC-DS-0716-R15, p4)
- eps202-as4630-54pe-ds.pdf 70749f11... (EC-DS-1025-R10, p4)
- 4 download-page HTML files (wpdm link discovery only; not sources): dcs203/dcs204/dcs500/eps202
- full hashes in capture-ledger.json; generator: research/batches/hunt-2026-10-10/edgecore/build_facts.py + accessories.py (sibling, exec'd by it)

## Supported exact facts
Each row: model = printed code, ordering_description_as_printed = "Header: value; ..." with printed headers (Model Number, Part Number,
PSU, Airflow, Region/Power Cord), base_model_as_printed + full Base Model line, page_ref, part_number_as_printed.
Codes are printed WITHOUT the AS prefix except AS4630. Counts: 7712-32X 8 (US/EU/UK/JP x AC-F/AC-B, no part numbers, airflow
"port-to-power"/"power-to-port"); 5812-54X 12 (AC F/B x4 regions, 48V F/B, 12V F/B); 7326-56X 10 (AC F/B x4, 48V F/B);
7726-32X 18 (AC F/B x UN/US/EU/UK/JP/CN/TW + bare "-O-AC-F/-B" w/o cord, 48V F/B); 7816-64X 20 (two tables: "-OZ-" 10 and "-O-...-R-" 10);
AS4630-54PE 7 (AC-F bare + US/UK/JP/EU/TW/CN).
Literal text only: e.g. "7726-32X-O-AC-F-UN ... Dual AC PSUs; Front-to-Back; IEC 60320 C13-C14"; "7726-32X-O-48V-F ... Dual DC PSUs".
-F/-B lines print Front-to-Back / Back-to-Front (7712: port-to-power / power-to-port) per code; no document defines suffixes as a general rule.
No "-DC-" suffix appears (codes use "-48V-"/"-12V-"); -OZ- and -R- printed with no definition besides the table's base-model heading
(OZ: port/fan/PSU numbering start at zero; O ...-R-: start at one, as printed in the Base Model line).
Observation: 5812 R15 prints 5812-54X-O-12V-F/B; AS7326 R10 and 7726 R09 print only -48V DC variants.

## Excluded, failures, gaps
- Owned-code check: 7760-32X-A-AC-F, AZ4444-52X-DC-10, DMC-12-48R-AS0, AS7772-32X: ZERO hits in all 6 PDF texts and 4 HTML pages.
  No "-A-" order code appears in any table. Gap stays a gap (not captured: AS7772/7760 datasheets; no search for them was made).
- 404 (recorded once, no retry): DCS203 R09 /_upload PDF, DCS510 AS9716-32D R09 /_upload PDF, AS7312-54X R02 /_upload PDF,
  AS4630-54NPE R04 /_upload PDF (all URLs taken from search results; moved). Not captured: AS9716-32D R11, AS7312-54X/XS, AS5812-54X-EC,
  AS7712 current (DCS501) datasheet. Budget exhausted.
- Search-snippet claims uncaptured: AS6700/6701 manual says "-O-" = ONIE preloaded, "-D-" = NOS preloaded. Lead only; not in the data.
- (superseded) PSU/fan/accessory rows are now included, see Extension.
- Catalog already holds AS-prefixed names (e.g. AS7712-32X-O-AC-F, AS7326-56X-O-AC-F) from EOL notices/RRA; those are separate
  identities from these printed codes; no alias asserted. 7712-32X-O-AC-F (no region suffix) is not printed in the 7712 sheet; only -US/-EU/-UK/-JP.
- Docs are branded "Edgecore Networks"; brand stored as Edge-Core per task. 5812/7326 sheets print "developed and manufactured by Accton": not
  extracted as an affiliation.

## Checks
Ledger sha256 re-hash OK; snapshot (bare filename) resolves; temp-catalog import in scratchpad/edgecore/t.sqlite: 75 devices, 6 sources,
integrity ok, FK check empty. Text extraction: see Extension.

## Decisions
Printed codes kept as exact identities (no AS prefix added); one source per PDF; download-page HTML not sources; 404 bodies deleted
(ledger keeps status/hash-null).

## Next
Capture AS9716-32D R11, AS7312-54X(S), AS7712 DCS501, AS7772/7760 sources via current download pages (wpdm link then PDF); integrator
to decide FRU rows and review of ligature "ﬂ" in printed text.

## Extension (coordinator request): PSU / fan / accessory rows, no new network
- 42 accessory records (kind as printed: "Power Supply FRU", "Fan Tray FRU", "Fan Module FRU", "PSU FRUs", "FAN FRUs", "Accessories",
  "FRU", "FAN", "PSU"/"FAN"/"Accessories" for 4630; rows from the 7712 and 5812 service/cable/rack tables are kind
  "ordering item (type not printed)"). Per-source keys are `ACC:<code>`; facts: ordering_description_as_printed, listed_with_base_model_as_printed
  (+ _line_), table_heading_as_printed, page_ref, part_number_as_printed. Counts: 7712 9, 5812 9, 7326 4, 7726 9, 7816 8, AS4630-54PE 3.
- Totals: 117 records, 618 facts (accessory facts 251). Curated sha256 17781aab10bf88afdb62417543dd777f6adf5e0aff7f76749b8fb9ce474261ab.
- Transcription is hand-keyed, then machine-checked: every code appears in the page text and every description word appears on the page
  (wrapped cells re-joined; the 5812 ORSA-1U description is wrapped across the page's warranty text, joined as "into 21 inch Open Rack").
- Printed oddities kept: 7816 FRU/FAN tables print headers "FRU Model Number | Type | Airflow" but the middle cell is a part-number-like
  string (stored unlabeled, no relabeling); 7816 FRU tables follow the "-O series" table but the page also prints an "-OZ series" heading
  (grouping_note records this; base-model fact = the -O series line). 7326 codes "YM-2651Y, B01R"/"C01R" and 7816 "YM-2851F, C01R" keep the printed comma.
  7712 cable rows keep the printed en dash. No compatibility or airflow meaning added; "for EPS202/..." lists are table text only.
- Catalog merge note: temp-catalog shows 115 devices from 117 records because identical Edge-Core model strings in two datasheets merge
  (FAN-1U-1x1C-F in 7712 and 7726 sheets; RKIT-100G-SLIDE in the 7726 and AS4630 sheets). Same string is not a claim of identical parts; both
  records keep their own page, part number and description (7726 prints F0TAO6632000A for FAN-1U-1x1C-F, 7712 prints no part number).
- Text extraction tool: poppler `pdftotext` version 26.06.0 at
  /nix/store/wb8486c5l68f1shsbc217b7m4hbgrw3s-poppler-utils-26.06.0/bin/pdftotext, already present in the nix store; nothing installed.
  Run as `pdftotext -layout FILE.pdf -`; page numbers are form-feed-separated PDF pages (1-based). devicehunt/documents.py needs pypdf,
  which is not available here.
- Re-validation: ledger sha256 re-hash OK (10 files); snapshots resolve; scratch import 117 records / 115 devices, 618 facts, integrity ok, FK check empty.

## Fixes after review (independent review: ACCEPT with notes; captures/ledger unchanged, no network)
1. Source coverage text rewritten: switch rows by pdftotext -layout; the 42 PSU/fan/accessory/service rows were hand-keyed from the printed
   tables (PDF page 3 for the 7712 sheet, page 4 for the others), machine-checked, and verified by independent review. The old "not extracted" wording is removed.
2. Ligatures (fi fl ff ffi ffl ft st glyphs U+FB00-FB06) replaced with plain letters in all sources/devices values at write time; coverage says
   so ("Airflow" is the printed word). 0 ligature code points remain. (Includes the ft ligature in "Software" on the 5812 sheet.)
3. All 42 accessory records: brand "Unattributed accessory", affiliations [["Edge-Core","documentation_vendor"]]. The 75 switch order-code
   records keep brand Edge-Core + [["Edge-Core","brand"]]. listed_with_base_model_as_printed etc. unchanged.
- Re-validation: ledger hashes OK, snapshots resolve, scratch import 117 records / 115 devices (the two same-model pairs still merge on
   brand+model), 618 facts, integrity ok, FK empty. New curated sha256 17781aab10bf88afdb62417543dd777f6adf5e0aff7f76749b8fb9ce474261ab.
