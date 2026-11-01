# spec_results independent evidence review (Sonnet 5.5), 2026-10-11

Reviewer did not author the handoff. Scratch: `.../scratchpad/review-spec/` (v1/v2 verifiers, `imp/before.sqlite`, `imp/after.sqlite`, inventory JSONs). Nothing under `data/` or `research/` was touched except this file. Live `data/devices.sqlite` sha256 `1cfd84ca...ba0c` before and after. Candidate sha256 `1e4afb4e...e2f180` confirmed.

## Verdict: ACCEPT with notes

No must-fix. The capture is honest, every fact is literally in the captured text, the base-model stripping only ever removes a trailing CPU annotation, brand canonicalisation touches exactly the three reviewed aliases, and the affiliations do not change manufacturer scope. Five should-fix items are cheap and mostly about wording and two edge identities. They need no re-capture.

## Must-fix

None.

## Should-fix

**S1. Source `coverage` text is still the revision-1 wording, so it now contradicts the data.** All 237 coverage strings say the brand is "not a mapping to other catalog names". Revision 2 maps three printed strings to catalog targets (Quanta 12, Inspur 29, Oracle 2 records). The coverage also never mentions that `model` is a stripped base model (the full printed name lives in `spec_system_name` and the identifier). It never explains the role split `brand` (201) versus `reported_system_vendor` (36). Reword before import. Suggested: "brand is the reviewed target alias when the printed Hardware Vendor equals one, otherwise as printed; `model` drops only a trailing parenthesised CPU/config group, full name kept as identifier `spec_system_name`; role is `brand` when Test Sponsor equals Hardware Vendor, else `reported_system_vendor`; neither is a manufacturer claim". The candidate sha256 changes, so re-hash it and update the handoff. The handoff body (outcome/identities sections) also still describes revision 1 ("brand = Hardware Vendor exactly", "205 devices", "brand-only affiliations"). Mark those lines superseded.

**S2. `1-Chip VM with SPARC M7` (Oracle, 2 records) is a virtual-machine configuration, not a hardware identity.** The result prints `Orderable: 1-16 CMIOU (on host)`. It is imported as kind `server` under brand `Oracle` with a `brand` affiliation, and it adds an Oracle identity to `vendor-inventory Oracle --scope all` (1268 to 1269 identities). Either drop the two results, or set kind to something like `virtual machine configuration`. Do not describe it as a server.

**S3. Multi-node enclosure result `power_ssj2008-20170926-00785` (Quanta QuantaGrid T42S-2U) loses its shared-hardware PSU.** The result has a SHARED HARDWARE block (Enclosure `T42S-2U`, `1 x 2200` W, `ARTESYN MC2200B4-3-4R-02`, line 69) and a HARDWARE PER NODE block that repeats `1 x 2200` (line 105) but prints `Power Supply Details: None` (line 107). The candidate stores per-node `spec_power_supply_quantity_and_rating_w = 1 x 2200` and `spec_power_supply_details = None`. Both are literal. But a reader sees "None" and a 2200 W PSU for a 4-node chassis. The ARTESYN part never appears, and `spec_number_of_identical_nodes = 4` is not tied to the PSU. Add the shared-block PSU as its own fact (for example `spec_shared_power_supply_details`), or at least note the shared block in the coverage. This is the only multi-node result in the 57 (`Test Method: Multi Node`). The other 56 are single node.

**S4. `spec_result_metrics` `literal_quote` is a constructed string, not a literal.** Example: `SPECrate(R)2017_fp_base 111; SPECrate(R)2017_fp_peak 118`. The two parts come from separate printed lines (51, 52) with the column padding collapsed. Every component is present in the text and the values are right (all 180 checked, see below). But the evidence field claims "literal_quote". Either split into base and peak facts, one per printed line, or state the join in the coverage. The handoff's "3962/3962 literal" is true only after splitting on `; `.

**S5. `Sun Fire V490` CPU2017 results are the SPEC reference-machine calibration runs, so their metrics are all 1.00.** In `cpu2017-20161026-00001..00004` every ratio is `1.00` (`SPECrate(R)2017_fp_energy_base 1.00; SPECrate(R)2017_fp_base 1.00`, text lines 40 to 43). Peak is `Not Run`/`--`. Those are normalisation values, not a performance claim. This is my inference from the all-1.00 ratios plus the unusually old 2007 hardware (Hardware availability Feb-2007), not stated in the file. Either add a coverage note or omit `spec_result_metrics` for these four. Do not read "1.00" as a benchmark score of the V490.

## Notes

**N1. Same model under several brands.** Not wrong, but noisy. After import `NF5280M7` exists as Inspur (10 spec plus the existing incloud-rail record), `IEIT Systems Co., Ltd.` (6) and the unmapped `Inspur Electronic Information Industry Co., Ltd. (IEI)` (14). `QuantaGrid D43K-1U` is split as `Quanta` (1) and `Quanta Cloud Technology` (6). The existing `QCT` QuantaGrid D52BQ-2U/D51B-2U/S31A-1U devices stay separate from the new `Quanta` ones. This is the adopted ubuntu-cert convention (IEIT never mapped to Inspur, QCT never mapped to Quanta). Keep it.

**N2. Brand-prefixed model strings are not normalised.** Under brand `Inspur Corporation` the models are `Inspur NF5280M5`, `Inspur NF5180M5` and the like. `Inspur Electronic ... (IEI)` has both `NF5180A7` (9) and `Inspur NF5180A7` (1), and both `NF5280A7` (8) and `Inspur NF5280A7` (1). That gives two identities for the same printed model within one brand string. Only a trailing parenthesised group is stripped, which is the coordinator rule, so this is as designed. Flag it for a later normalisation pass.

**N3. Unparenthesised CPU text stays in two models.** `QuantaGrid D44N-1U AMD EPYC 9755` and `QuantaGrid S44NL-1U AMD EPYC 9755` (QCT, 1 each) are separate from `QuantaGrid D44N-1U` and `QuantaGrid S44NL-1U`. The handoff names this and it follows the rule.

**N4. `--` is stored as a value** for `spec_form_factor` on `NF290D2` (line 69). It is as printed, so it is acceptable. Consumers should treat it as "not stated".

**N5. Request spacing.** Gaps are 10 s minimum (max 118 s). The first gap, from robots.txt, is 14 s. The early Python urllib redirect guesses (the 302 and 301) are recorded in the manifest and not followed again.

**N6. Rated, not measured.** PSU values such as `1 x 2200`, `1 x 650` and `2 x 1448 W (redundant)` are submitter-printed ratings. The Sun Fire V490 text also prints measured Max/Idle watts (1143.2 W / 1080.48 W). Those are correctly not extracted.

## Verified

**1. Capture and request log.**
- 244/244 manifest entries re-hash with matching sha256 and bytes. The 244 URLs are unique. The only on-disk files not in the manifest are the manifest, `requests.jsonl` and the candidate. `requests.jsonl` equals the manifest `files` list (url and UTC, 244 rows).
- Hashes match the handoff: manifest `8644364f...`, requests `391b0bec...`, candidate `1e4afb4e...`.
- Timestamps are strictly increasing, 09:25:17Z to 10:10:42Z. Min gap 10.0 s, max 118 s, 0 gaps under 10 s.
- Status counts: 241x200, 1x302, 1x301, 1x404. `err` is null on every row. No 403/429/5xx. No URL occurs twice, so there are no retries. The 404 is the single jbb2015 `.txt` probe, recorded once.
- Robots content (saved copy): `Crawl-delay: 10`, `Disallow: /cgi-bin/ /includes/ /auto/ *.txt.rev* /gwpg/*eadme.txt /jhtest/`. I matched all 244 URLs against every Disallow rule: 0 hits. No `cgi-bin` URL was requested.
- UA is plain and identifies the project.

**2. Facts literally in the captures.**
- All 4199 facts have an evidence entry. The 3962 non-URL quotes (`spec_result_url` has line 0) are all found in the cited capture's text (whitespace-insensitive). Each fact value equals its quote modulo whitespace.
- Line numbers are valid. Every integer `line` points at (or within 3 lines of) the quote. For CPU2017 vendor/system the evidence uses ranges like `2-4` for wrapped title lines.
- Each source's coverage sha256 equals the snapshot file hash (237/237).
- Independent label-based re-parse (not using the builder's quotes): 2592 fact values compared against the captured text by field label (`Hardware Vendor`, `Model`, `Form Factor`, `CPU Name`, `CPU(s) Enabled`, `Memory Amount (GB)`, `# and size of DIMM`, `Memory Details`, `Power Supply Details`, `# of Identical Nodes`, `Test Method`, CPU2017 `Max MHz`/`Nominal`/`Enabled`/`Orderable`/`Memory`/`Storage`/`Power Supply`, etc.). The 242 non-matches are my parser's limits: wrapped Test sponsor lines, the two-column header and the multi-block PSU case in S3. I inspected the types. No real value mismatch.
- Line-level sample of 80 random facts (PSU details and ratings, CPU, memory, node count, headline, metrics, form factor, enabled, DIMM, storage, availability), seed 7: 80/80 verified at the cited line. Examples, all literal:
  - `power_ssj2008-20230522-01258` FOXCONN (FII) Mt. Collins: Form Factor `2U`, `256 cores, 2 chips, 128 cores/chip`, `16 x 16 GB`, PSU `1 x 650`, `3Y Power Technology P/N: YM-2651YA10R`, nodes `1`, availability `Feb-2023`.
  - `power_ssj2008-20170926-00784` Quanta D52BQ-2U: PSU details `ARTESYN MC800B6-3-4R-02` (line 81).
  - `power_ssj2008-20181031-00865` Quanta D52BQ-2U: PSU details `Murata MPS8146`.
  - `power_ssj2008-20240923-01439` IEIT meta brain NF5280G8: PSU `1 x 1600`. `power_ssj2008-20240924-01460` Quanta D44N-1U: PSU `1 x 1600`.
  - `power_ssj2008-20180606-00820` Inspur NF5180M5: headline `SPECpower_ssj2008 = 12,167 overall ssj_ops/watt`.
  - `power_ssj2008-20200519-01037` and `-20200617-01038` Inspur: memory `192` GB.
  - `cpu2017-20250221-46471` IEIT: enabled `256 cores, 2 chips`. `cpu2017-20250113-45913` IEIT: `144 cores, 1 chip`. `cpu2017-20230828-38731` Inspur: `96 cores, 2 chips, 2 threads/core`.
  - `cpu2017-20240603-43698` IEIT: CPU `AMD EPYC 9174F`. `cpu2017-20241104-45162` IEIT: `Intel Xeon 6980P`.
  - `cpu2017-20161026-00001` Sun Fire V490: `2 x 1448 W (redundant)`, `Type A187 1448 Watt AC Input Power Supply`.
- CPU2017 title line equals `hardware_vendor + " " + system_name` for all 180.

**3. Base-model rule audit (all 237 results, 205 distinct printed-name/model/brand tuples, 172 stripped).**
- For every stripped tuple, the printed name starts with the stored model and the removed remainder is exactly one trailing ` (...)` group with one `(`. No odd strips.
- All 76 distinct removed groups are CPU annotations only (`(Intel Xeon ...)`, `(AMD EPYC ...)`, `(2.25 GHz,AMD EPYC 9754)`, `(AMD EPYC 7742 64-Core, 2.25GHz)`, `(Intel Xeon Platinum 8480+, 2.0GHz)`, `(2.5 GHz, Intel Xeon Processor L5420)`). Each includes a CPU SKU or clock. None is a chassis variant, form factor, drive-bay count or revision. No parenthesised name was kept. The only remaining CPU words in models are the two unparenthesised `... AMD EPYC 9755` names (N3).
- `spec_system_name` identifier equals the full printed name on all 237 rows, so no information is lost. Titles are `brand model` on all rows.
- Merges into existing identities (confirmed by the copy import): exactly `Inspur NF5280M7` (existing incloud-rail record plus 10 spec), `Inspur NF5280M6` (plus 2) and `Inspur NF8480M7` (plus 1). All 13 spec records print the same model string followed by only a CPU group (for example `NF5280M7 (Intel Xeon Platinum 8458P)`, `NF5280M6 (Intel Xeon Gold 5318N)`, `NF8480M7 (Intel Xeon Platinum 8450H)`) under test sponsor `Inspur Electronic Information Industry Co., Ltd.`. In substance these are the same model families with different CPU options. Not wrong. Kind/title of the merged devices is unchanged (`server`, `Inspur NF5280M7`). Facts stay record-scoped (`spec_*` on `spec-*` records only).

**4. Brand and affiliation.**
- `catalog.target_for` gives Quanta, Inspur and Oracle for exactly `Quanta Computer Inc.`, `Inspur Electronic Information Industry Co., Ltd.` and `Oracle Corporation`. It gives `None` for `Quanta Cloud Technology`, `Inspur Corporation`, `Inspur Electronic Information Industry Co., Ltd. (IEI)`, `Inspur(Beijing) ...`, `IEIT Systems Co., Ltd.`, `FOXCONN (FII)`, `Sun Microsystems` and `Sun Microsystems, Inc.`.
- All 237 rows satisfy `brand = target_for(HW vendor) or HW vendor as printed` and `affiliations == [[HW vendor as printed, role]]` (0 mismatches). Counts: Quanta 12, Inspur 29, Oracle 2 canonical. IEIT 84 stays `IEIT Systems Co., Ltd.` and is never mapped to Inspur.
- Role rule re-derived from the facts: `brand` when `spec_test_sponsor == spec_hardware_vendor`, else `reported_system_vendor`. 201 `brand` and 36 `reported_system_vendor`: 30 `Quanta Cloud Technology` (sponsor `Quanta Computer Inc.`), 4 `Sun Microsystems` (sponsor `Oracle Corporation`) and 2 `FOXCONN (FII)` (sponsor `Infobell IT Solutions Pvt. Ltd.`). 0 mismatches. `reported_system_vendor` is an existing role (1 use, the Geekbench source). Rule is sound, if a little coarse (the QCT label printed by the hardware vendor field is still the vendor's own label). Tester names (Infobell) stay as facts, not roles.
- **Manufacturer scope unaffected.** `vendor-inventory` (`--limit 0`), copy before versus after: `--scope manufacturer` is byte-identical for Quanta, Inspur, Oracle and Foxconn. 0 `spec-` rows in the manufacturer listings. `--scope all`: Quanta 73/97/10/103 to 81/109/22/115, Inspur 141/152/10/282 to 149/181/39/311, Oracle 1268/1560/49/2403 to 1269/1562/51/2405 (identities, source_records, sources, role_assertions). Foxconn `all` is unchanged (the FII string is not an alias). This matches the handoff.

**5. Tier `vendor`: acceptable.**
- SPEC full-disclosure reports are vendor-submitted and reviewed by SPEC before publication. That is closer to a vendor document than to the anonymous Geekbench community upload that is the sole `benchmark_submission` source in the catalog. The policy file defines no tiers. `benchmark_submission` is outside `validation.EXTERNAL_TIERS`, which excludes it from snapshot-eligible inventory. `vendor` is inside it.
- Risk: inventory counts a source as external only for roles that qualify as manufacturer-type, and no role here is. So `vendor` has no effect on manufacturer scope today. A future importer adding a `declared_oem`-type role to these sources would count it. The coverage disclaimers (S1) matter for that reason.
- Caveat: for the FOXCONN (FII) results the submitter is Infobell, a test house, and the QCT results are sponsored by Quanta Computer Inc. Treat `vendor` as "vendor-attested, SPEC-published", not "authored by the named brand".

**6. Owned-model check.** Case-insensitive over every path and every byte of the 247 files in the capture directory (including the candidate, manifest, log and the three index pages): CS8200, CS8210, AZ4444, AS7760, DMC-12, AS7772, SV1120/SV1121/SV2121/SV2221, ByteDance, B4020, B4010 give 0 hits. `7760` appears only inside result IDs and URLs (for example `cpu2017-20190903-17760`) and hex hashes, never as a model. The Oracle absence claim holds: in the three saved indexes, no system named E4-2C, E5-2C, X7-2C or X10-2C, nor any X8-2/X9-2. The only Oracle X-series names are SPECjbb2015 `Sun Server X4-2` and `Oracle Server X5-2L` (HTML-only, not captured). Other Oracle/Sun rows are `Sun Netra X4250`, `Sun Fire V490` and SPARC systems. Scope: static index pages captured on 2026-10-11. The pattern `E5-2xxx` matches Intel CPU names (E5-2699) in the candidate. Those hits are CPUs, not Oracle servers.

**7. Temp import (scratchpad copy of the CURRENT `data/devices.sqlite`).**
- `python3 -m devicehunt --db <copy> import curated <candidate>` returned `{"curated": 237}`.
- Counts before to after: sources 776 to 1013 (+237), devices 4650 to 4718 (+68 new, 3 merged), records 5925 to 6162 (+237), identifiers 5445 to 5682, facts 51850 to 56049 (+4199), affiliations 9290 to 9527 (+237), `search_index` 4718 (equals devices). `integrity_check` ok, `foreign_key_check` empty.
- Row-level EXCEPT of every other table (sources, devices, records, affiliations, identifiers, facts and all registry tables): 0 removed or changed rows. Existing rows are unchanged, including the three merged devices.
- Matches the handoff's numbers exactly.

**8. Sizes and personal data.**
- Largest fact value is 308 bytes. The largest device record JSON is 8.6 KB. The largest coverage string is 1.3 KB. No fact approaches the 4 KB cap.
- The candidate has 0 email addresses. `Tested By` and `Test Sponsor` values are companies only (IEIT, Inspur strings, Quanta, Oracle, Sun, Infobell). No person names, contact details, serials or MACs appear in any fact. Notes, controller and analyzer sections, test locations and licence numbers are not extracted. Test location (for example `San Jose, CA, USA`) is not stored.
- Rights: the source license strings keep SPEC and submitter rights. This is consistent with the evidence policy.

## Suggested handoff corrections for the integrator
1. Mark the revision-1 body lines as superseded by Revision 2, or fold the Revision 2 text into the main sections.
2. After fixing S1 to S5, re-hash the candidate and record the new sha256. Capture files and the manifest are unaffected.
3. If S2 drops the two VM results, expect 236 sources, 66 or 67 new devices, and no change to the three merges.

## re-review

Re-review verdict: ACCEPT (no must-fix). Candidate sha256 `428c78f8...cc4c` (4359 facts). Captures and manifest unchanged (manifest `8644364f...`). Live `data/devices.sqlite` sha256 `1cfd84ca...ba0c`, untouched.

- **S1 coverage (fixed).** All 237 coverage strings now state the identity rules: brand is the TARGETS alias only when the printed vendor is a reviewed alias, else as printed; model drops only a trailing CPU/config parenthetical, with the full name in `spec_system_name`; role is `brand` or `reported_system_vendor` by Test Sponsor; not a manufacturer, OEM or operator claim. Each has its per-result before/after values (for example `'Oracle Corporation' -> 'Oracle'`, `'Quanta Cloud Technology' as printed`). It also says fields printed as None are omitted. The per-result values agree with the data. I recomputed brand, affiliation role, identifier and model-prefix rule for all 237: 0 issues. 71 distinct (brand, model), same as before.
- **S2 VM kind (fixed).** `1-Chip VM with SPARC M7` (2 records) is kind `virtual machine configuration`. The other 235 are `server`. Coverage says it is a VM (`Orderable: 1-16 CMIOU (on host)`), not hardware. It still adds one Oracle identity in the `--scope all` inventory, as the handoff states. That is acceptable.
- **S3 shared enclosure (fixed).** `power_ssj2008-20170926-00785` now has `spec_shared_enclosure` `T42S-2U` (line 65), `spec_shared_form_factor` `2U` (66), `spec_shared_power_supply_quantity_and_rating_w` `1 x 2200` (67) and `spec_shared_power_supply_details` `ARTESYN MC2200B4-3-4R-02` (69). I checked all four against the capture. The per-node `1 x 2200` (line 105) is kept and the per-node `None` is gone. Coverage notes the multi-node case. No fact value is empty or "none" anywhere in the candidate, and none in the imported spec facts either.
- **S4 metrics (fixed).** `spec_result_metrics` is replaced by 343 per-line facts named `spec_result_<metric label>`, for example `spec_result_specrate_r_2017_fp_base` = `111`. Each cites its own printed line. For all 343, the whitespace-normalised quote equals the whole cited line (0 mismatches). Example: line 51 `SPECrate(R)2017_fp_base 111`, line 52 `...fp_peak 118`. `Not Run` and `--` lines are omitted.
- **S5 V490 wording (fixed).** Coverage now says only that all printed ratios are 1.00 as printed, that the text does not state why, and that it is not a performance claim. It makes no reference-machine claim. (My earlier inference stays out of the data.)
- **Quote check.** 4122/4122 non-URL quotes are found in the cited captures. Each fact value is equal to, or inside, its quote. Line numbers are valid for all integer lines. Coverage sha256 equals the snapshot hash for 237/237. Max fact size is 308 bytes. All 237 sources are tier `vendor`.
- **Temp import** into a scratchpad copy of the CURRENT database: returned 237. Sources 776 to 1013, devices 4650 to 4718 (+68 new, 3 merged), records 5925 to 6162, affiliations 9290 to 9527, identifiers 5445 to 5682, facts 51850 to 56209. `search_index` 4718. `integrity_check` ok, `foreign_key_check` empty. An EXCEPT comparison of every non-FTS table against the pre-import copy shows 0 removed or changed rows. Inventory with `--limit 0`: `--scope manufacturer` is byte-identical to before for Quanta, Inspur, Oracle and Foxconn. `--scope all` is byte-identical to the revision-2 import (Quanta 81, Inspur 149, Oracle 1269 identities), and Foxconn `all` is unchanged.
- Open notes N1 to N3 stand, as the worker states. The first-pass notes N4 to N6 are unaffected.
