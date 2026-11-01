# spec_results wave 10 review: SPECjbb2015 HTML parser (Sonnet 5.5), 2026-10-11

Reviewer did not author the handoff. Scratch: `.../scratchpad/review-spec/jp.py` (my own `html.parser` row extractor, written independently of `jbbparse.py`) and `imp/w10-*`. Only this file was written. Live `data/devices.sqlite` sha256 `d78b0b7e...9438` before and after. Candidate sha256 `cafe0f28...5ba75`, manifest `d1fef666...b8ae` and request log `bdf16e12...5b18` confirmed.

## Verdict: ACCEPT with notes

The parser is sound. Every extracted value and every locator re-derives from the captured HTML with my own parser, brand comes from the right block, and the import is exact and non-disruptive. There is no must-fix. Two small should-fix items concern the benchmark mode and one multi-hardware page, and can be applied without re-capture.

## Must-fix

None.

## Should-fix

**S1. The jOPS facts omit the benchmark mode.** `spec_result_max_jops` and `spec_result_critical_jops` hold bare numbers (for example `52482`, `19614`). The mode only appears inside the evidence quote (`52482 SPECjbb2015-MultiJVM max-jOPS`). The 149 results split into MultiJVM 125, Composite 23 and Distributed 1. These modes are not comparable with each other, and `spec_benchmark` is just `SPECjbb2015`. A consumer reading the fact table cannot tell which mode a number belongs to. Add a `spec_benchmark_mode` fact (value `MultiJVM`/`Composite`/`Distributed`, from the same header cell), or put the mode in the fact name or value. Also add a coverage sentence that max-jOPS and critical-jOPS are throughput scores of that run, not hardware specifications. The fact names themselves are correctly labelled (max-jOPS vs critical-jOPS match the header cell on all 149).

**S2. The handoff claim that coverage names extra hardware blocks is not true for the one page that has two.** `jbb2015-20160705-00111` (Oracle `SPARC S7-2`, `SPECjbb2015-Distributed`) lists `Hardware hw_1` = `SPARC S7-2` and `Hardware hw_2` = `Sun Server X4-2L` (with its own CPU, memory and PSU rows). The candidate correctly uses hw_1 (it matches the page title), but its coverage says only "The Hardware block hw_1 is used" and never mentions that a second system (hw_2) is listed. Zero of the 149 coverage strings contain `hw_2`. Add one sentence to this source (the other 148 pages have a single hardware block). Do not read the X4-2L as part of the S7-2.

## Notes

- **N1.** The first page (`jbb2015-20150904-00007.html`) is the earlier `.txt` probe's HTML twin, and is a normal result page. The request count is 150 = robots + 149 HTML pages (the handoff text "148 more" plus the probe adds up).
- **N2.** Brand-prefixed model strings (`Inspur NF5466M6`, `Inspur NF8480M6`) are not normalised, as in earlier waves (review note N2). Several jbb pages print a header title that differs from hw_1 Vendor + Name (47 pages): `Inspur Corporation NF5280M6` for hw_1 Name `Inspur NF5280M6`, `Quanta Cloud Technology, QuantaGrid D43K-1U` (comma) for Name `QuantaGrid D43K-1U`, and `Inspur PowerSystem FP5280G2` for Vendor `Inspur PowerSystem Corporation`. In all 47 the hw_1 fields are right and the title is a cosmetic variant. Identity from hw_1 is correct. The coverage mentions the title difference as the handoff says.
- **N3.** `Inspur PowerSystem Corporation` (`FP5280G2`, `FP5290G2`, 2 pages) is kept as printed with role `brand`. It is not a TARGETS alias, so there is no target affiliation. Fine.
- **N4.** The 6 oldest Inspur jbb pages were not fetched (budget). The handoff lists them as a gap.

## Verified

**1. Capture and request log.**
- 150/150 manifest entries re-hash with matching bytes. The log equals the manifest (url and sha, 150 rows). 151 files are on disk (149 HTML, robots, log). Statuses are 150 x 200, `err` null.
- All 150 URLs are unique, so there are no retries. The only overlap with the 453 earlier URLs (original, wave 8, wave 9) is the robots.txt re-fetch, which is byte-identical to the saved copy. Robots was re-fetched first (13:50:12Z).
- Spacing is 10 to 34 s (min 10.0). No URL touches `/cgi-bin/`, `/includes/`, `/auto/`, `/jhtest/` or `.txt.rev*`. All 149 result URLs are static `.html` under `/jbb2015/results/`.

**2. Independent re-parse of all 149 pages (more than the 30 asked).**
- With my own `html.parser` table-row extractor, I split each page into the header rows, an overall SUT block, and `Hardware hw_N` sections.
- **Facts.** 3427 fact values compared against my parse: header title, test sponsor, tested by, hardware availability, max-jOPS, critical-jOPS, and 18 hw_1 fields (Vendor, Name, Model, Form Factor, CPU Name, CPU Characteristics, CPU Frequency, Nodes/Systems/Chips/Cores/Cores per chip/Threads, Memory Amount, `# and size of DIMM(s)`, Memory Details, Power Supply Quantity and Rating, Available). 0 mismatches and 0 unmapped fact names.
- **Locators.** All 2682 `SUT Description > Hardware hw_1 > <label>` locators resolve to a row with that label whose cell text equals the quote exactly (0 failures). Another 447 header locators (`page header > Tested by`, `Test Sponsor`, `Hardware Availability`) and 298 jOPS locators (`row 0 cell 1`) and 298 title/page-title locators are consistent with rows 0 to 2. The 3725 quotes are all found.
- Examples, all literal: `jbb2015-20150904-00006` Sun Server X4-2: `Intel Xeon E5-2697 v2`, `16 x 8 GB`, PSU `2 x 600W AC power supply`, `128` GB, max-jOPS `52482`, critical-jOPS `19614`; `jbb2015-20160705-00111` SPARC S7-2: `2 x 1200W redundant power supplies`, `16 x 64 GB`, `1024` GB, `66612` / `36922` (Distributed).
- **Layouts.** I checked all 149 for multi-system, multi-node, enclosure or shared cells. Every page has `Number of Systems` 1, `Nodes Per System` 1, `Total Systems` equal to hw_1, `Cabinet/Housing/Enclosure` None and `Shared Description`/`Shared Comment` None (so the omitted shared facts are correctly absent, not dropped). The overall `Vendor` row equals hw_1 `Vendor` on all 149. 148 pages have one hardware block. The only multi-block page is S2 (`hw_2` is a different system, and hw_1 matches the title). No result mixes systems in the extracted facts. 142 pages are `Server Rack` and 7 are `Rack Mount Chassis`. Modes are MultiJVM 125, Composite 23, Distributed 1.
- Coverage sha256 equals the snapshot hash for 149/149. No `None`, `--` or empty fact values. Max fact is 78 bytes. No email addresses in the candidate.

**3. Identity, brand, role.**
- 149 sources/devices/3874 facts. Source ids are new (0 overlap with the promoted, wave 8 and wave 9 candidates). Tier `vendor`, kind `server` on all 149.
- **Base-model stripping.** 0 names stripped: no hw_1 Name has a parenthesised group, and no model contains a CPU word. `spec_system_name` equals the model on all rows. Titles are `brand model`.
- **Brand from the right block.** `brand = target_for(hw_1 Vendor) or hw_1 Vendor as printed` holds for all 149 (0 mismatches). Only aliases are canonicalised: `Oracle Corporation` to Oracle (7), `Inspur Electronic Information Industry Co., Ltd.` to Inspur (4), `Quanta Computer Inc.` to Quanta (2). As printed: `Inspur Corporation` 68, `IEIT Systems Co., Ltd.` 40 (never mapped), `Quanta Cloud Technology` 19, `Inspur ... (IEI)` 7, `Inspur PowerSystem Corporation` 2. None of these last five is an alias.
- **Role.** `brand` when jbb `Test Sponsor` equals hw_1 Vendor, else `reported_system_vendor`. Re-derived for all 149 with 0 mismatches: 19 `Quanta Cloud Technology` (sponsor `Quanta Computer Inc.`) and one `Quanta Computer Inc.` page (sponsor `Quanta Cloud Technology`) get `reported_system_vendor`, the rest get `brand`. The rule maps sensibly onto jbb's Test Sponsor field. `Tested by` stays a fact.

**4. Added and merged.** Devices went from 5084 to 5107: 23 added, 37 merged, with 149 new records across 60 identities. All merges are the same brand string plus the same model string. The Oracle ones: `Sun Server X4-2` (1 new / 6 existing records), `Oracle Server X5-2L` (1/6), `SPARC S7-2` (2/1), `SPARC T5-2` (1/3), `SPARC T7-1` (1/4), `SPARC T8-1` (1/2). They land on existing Oracle devices of kind `server` (sources include `oracle-sparc`, `oracle-eol`, `oracle-previous`, `ntra-2026-01`). Those are the same model names in substance (the X4-2/X5-2L pages print exactly `Sun Server X4-2` and `Oracle Server X5-2L`). Facts stay record-scoped under `spec-jbb2015-*` sources. Merged devices keep their kind and title. The other merges match the handoff list (Quanta Cloud Technology D43K-1U 4/23, D44N-1U 4/10, S44NL-1U 4/10, D53XQ-2U 2/6, D55Q-2U 2/4, D54Q-2U 3/3; Quanta D51B-2U 1/5, D52B-2U 1/1; Inspur TS860G7, NF5280A7; Inspur Corporation NF5466M6 2/13, NF5180M6 8/19, NF8260M5 2/1, NF8480M6 2/6, NF5280M6 14/16, NF5280M5 6/9, NF8260M6 4/4, NF5180M5 4/2; the seven IEI and six IEIT identities). No wrong merge found.

**5. Metrics labelling.** The max-jOPS and critical-jOPS facts take the first and second number of the same header cell, and I re-parsed all 149 header cells (`<n> SPECjbb2015-<mode> max-jOPS <n> SPECjbb2015-<mode> critical-jOPS`): 0 mismatches or swaps. They are named as results, not hardware. See S1 for the mode gap.

**6. Owned-model check and import.**
- Owned-model check: zero hits for CS8200, CS8210, AZ4444, AS7760, standalone 7760, DMC-12, AS7772, SV1120/1121/2121/2221, ByteDance, B4020, B4010 and Oracle E4-2C/E5-2C/X7-2C/X10-2C over the 149 pages, robots, log, manifest and candidate. The only Oracle X-series pages are `Sun Server X4-2` and `Oracle Server X5-2L`; the other five Oracle pages are SPARC. Scope: wave10 files only.
- Temp import into a scratchpad COPY of the CURRENT database: returned 149. Sources 1927 to 2076, devices 5084 to 5107, records 7076 to 7225, affiliations 10408 to 10557, identifiers 6684 to 6833, facts 70051 to 73925, `search_index` 5107. `integrity_check` ok, `foreign_key_check` empty. An EXCEPT comparison of every non-FTS table against the pre-import copy shows 0 removed or changed rows.
- Vendor inventory (`--limit 0`): `--scope manufacturer` is byte-identical before and after for Quanta, Inspur, Oracle and Foxconn. `--scope all` (identities/source_records/sources/role_assertions): Oracle 1281/1610/99/2453 to 1281/1617/106/2460, Quanta 175/289/202/295 to 175/291/204/297, Inspur 221/380/238/510 to 223/384/242/514, Foxconn unchanged. This matches the handoff exactly. The deltas come from the alias-canonicalised brand strings (Oracle 7, Quanta 2, Inspur 4 pages), which create `brand`-role records only.
