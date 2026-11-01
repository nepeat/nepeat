# ubuntu_certified independent evidence review (Sonnet 5.5), 2026-10-11

Reviewer did not author the handoff. Scratch: `.../scratchpad/review-ubuntu/` (v1-v3 verifiers, copy.sqlite, varA/varB.sqlite, inventory JSONs). Nothing under `data/` or `research/` was touched except this file; `data/devices.sqlite` sha256 `0646f4f5...c47c8f0` before and after.

## Verdict: ACCEPT with notes

The captured evidence is honest and the facts are literally correct. Two statements in the handoff are false and must be corrected. The brand/affiliation convention (brand as printed, `affiliations: []`) departs from project precedent and hides 63 records from target lookups. I recommend fixing that before import (item M2). It is a small candidate change and needs no re-capture.

## Must-fix

**M1. Two handoff claims are wrong.** Import behaviour disagrees with both.
- Handoff says "no device ids collide" and "74 distinct devices". A scratch import of the unmodified candidate adds **72 new devices**, not 74. Devices went 4361 to 4433. `Wiwynn SV7220G3` and `Inventec K885G7` use the same brand string as the existing rows, so they merge into existing device ids. Existing `Wiwynn SV7220G3` already has 3 records (`wiwynn-sv7220g3-pr..`, `wiwynn-sv7220g3-da..`, plus the new cert record) and `Inventec K885G7` has 2. This is the project's exact brand+model identity rule and is acceptable. The handoff must say so, because it lists both models under "different brand" leads and "no merge". The evidence policy's SV7220G3 vs `SV7220G3-V+` caution is untouched: the plain string merges only with the plain string.
- Handoff says "`target_for` would not match these strings anyway". It does match 63 of 89 records. `catalog.TARGETS` has the aliases `quanta computer inc` and `inspur electronic information industry co ltd`. `target_for('QUANTA Computer Inc')` is `Quanta` and `target_for('Inspur Electronic Information Industry Co., Ltd.')` is `Inspur`. `research/bytedance-inspur-2026-10-04.md` "Exact target mapping" reviews the Inspur alias explicitly. `IEIT SYSTEMS Co.,Ltd.` stays unassigned there, which is correct. `Wiwynn` and `Inventec` are target names too.

**M2. Resolve the affiliation convention before import (details in the section below).** With empty affiliations, none of the 89 records can be found through target-aware tools. In `vendor-inventory` they appear under no target. In `search --vendor Inspur`, `NF5180M6` finds no `Inspur Electronic...` device even though one now exists. Plain `search` shows them, so they are visible as extra brands. Every existing certification-tier or program-listing source I checked records the listing company as a sourced affiliation: UL uses `listing_holder`, ENERGY STAR uses `program_partner`. Only the role is non-manufacturer. This makes the existing role data consistent without inventing a role.

## Should-fix

**S1. Make/model provenance is not in the cited snapshot.** Each source's snapshot is `detail/<cert>.html`. That page never prints a Vendor field. It has no `/certified/vendors/` link; the make and model appear only as the joined `<title>`/h1 string `QUANTA Computer Inc QuantaGrid D52BQ-2U certified with Ubuntu`. The Vendor / Configuration Name split comes from the saved listing tables in `pages/all-q-*.html` (columns "Vendor", "Configuration Name", "Category").
- I re-derived make, model and category from those listing rows for all 89: 0 differences. In every case `make + " " + model + " certified with Ubuntu"` is a literal substring of the detail page, so there is no hidden mismatch.
- Add one sentence to each source's coverage, or to the handoff, naming the listing page (path and sha256) that carries the Vendor column. Otherwise `ubuntu_cert_make_as_printed` cannot be reproduced from the source's own snapshot alone.
- The same applies to the whitespace-collapsed `QuantaGird  D43N-3U` (double space only in the listing HTML, 202208-30544).

**S2. `hw_*` rows describe the test bench, and the labelling is thin.** `ubuntu_cert_hw_usb` (all 55), `hw_mouse` (5), `hw_keyboard` (1), `hw_cardreader` (2) and `hw_disk` (test drives, e.g. `Union Memory (Shenzhen) ST8000NM0055-1RM112`; USB sticks such as `Lexar JumpDrive V10`) are peripherals attached during the certification run. They are not system specification.
- The source coverage says "components described below", but nothing says USB/mouse/disk rows are test peripherals.
- The hw fact names carry no release; the release is a separate fact, `ubuntu_cert_hardware_details_release` (first release tab only). The coverage text does state this.
- Suggest a coverage sentence that hw rows are run-time detections for one release, including attached peripherals and drives, and a fact-name or value note for `hw_usb`/`hw_mouse`/`hw_keyboard`/`hw_cardreader`.

**S3. Process lapse on the 5xx run, and the handoff mislabels it.** The manifest shows a 504 at 09:40:30, then 15 requests answered with 500, then 200 again at 09:42:26 with 12 further successes.
- The handoff says "a throttling-like pattern". The resumption of 200s at the same cadence contradicts that. The 500 pages are one 60,127-byte server-error body, apparently per-certificate/release errors, not a throttle.
- Nothing was retried or rerouted, and the 15+1 errors are recorded once, which is compliant. But the batch loop did not stop or checkpoint at the first 5xx, which AGENTS.md's stop-on-throttling rule asks for. The handoff already admits this. Reword it as "per-page server errors, not throttling", not as a throttling signature.

## Notes

- **N1. Missing lead.** `QuantaGrid D53XQ-2U` is the same model as existing `Quanta D53XQ-2U` (Bahrain TRA), with the family prefix dropped in the catalog. This is a loose, not exact, lead (13 exact matches confirmed, plus this one). Add it to the lead list. No merge.
- **N2. Inspur vs IEIT.** 12 model strings appear under both brands in this source (for example `NF5280M6`: 4 IEIT certs; `NF3180A6`, `NF3280A6`, `NF8260M6`, `NF5180M6`, `NS3160A5` and so on). Keeping them separate is right: IEIT is an unassigned name in the registry. The coverage and handoff should say that the pair is an unresolved relationship, not two products.
- **N3. Kind on merge.** The new `server` kind is dropped on merges with existing `server platform` and similar devices, since `INSERT OR IGNORE` keeps the first title and kind. Harmless.
- **N4. Request accounting.** 192 requests, which is the cap's accounting including the unsaved probe, and 191 manifest rows. The probe has no timestamp, so its spacing to the next request cannot be audited. All 190 saved gaps are at least 2.0 s (min 2, max 53).
- **N5. Counts.** Handoff says "90 certificate IDs listed". The listing pages contain 98 distinct cert ids: 90 Quanta/Inspur/IEIT/Inventec/Wiwynn certs plus 8 Dell desktops from the `QCT` and `EC2` probes. The excluded Dell desktops are described correctly.

## Verified

**Manifest and capture rules.**
- 191/191 files re-hash to the manifest sha256 and bytes. 0 missing and 0 mismatches. The only on-disk files not in the manifest are the manifest and the candidate (193 on disk).
- Every URL is unique. Status counts: 172x200, 15x500, 2x504, 2x302. URL and UTC are recorded per file, strictly increasing.
- Robots: `ubuntu.com/robots.txt` was saved with 200. Under `User-agent: *` it disallows `/search*`, `/*/search*`, `/account*`, `/pro/*` and `/devices*`, with Crawl-delay 1. `/certified` is explicitly allowed in the AI-bot group and not disallowed under `*`. No fetched path matches a Disallow.
- `certification.canonical.com/robots.txt` returned 302 with an empty body. It was recorded and not followed, and that host was never used again.
- The `?category=Server` 302 listing was recorded, not followed, and replaced by `/certified/servers?q=Quanta`.
- Every 5xx is recorded exactly once with no retry. No 429 appears.
- Candidate: 89 sources = 89 devices-by-record, all tier `certification_body`, all snapshots exist. The snapshot sha256 stated in each source's coverage string matches its file (89/89).

**Facts, 1656 total.**
- I verified every factual value literally against the saved HTML by whitespace-insensitive substring match (1,948 value parts including list splits): 0 failures except cert ids, which sit in the URL and not in page text, and the derived `network_entry_count` row.
- I independently re-parsed the release tabs. The release list, the kernel per release, and the BIOS per release (present only where printed) match for all 89 pages.
- Each `hw_*` row occurs in its own `hwdetail/*.html` capture. The capture sha256 stored in `hardware_details_capture` matches the file for all 55. The `hardware_details_release` is the page's first release tab in all 55.
- Spot examples (all literal): 201709-25792 `QuantaGrid D52BV-2U`, Ubuntu 18.04 LTS, kernel `4.15.0-34-generic`, `Intel Corp. Intel(R) Xeon(R) Gold 6134 CPU @ 3.20GHz`, `Intel Corp. Ethernet Connection X722 for 10GBASE-T pci (8086:37d2)`, `Broadcom / LSI MegaRAID SAS-3 3108 [Invader] (1000:005d)`; 202206-30404 `QuantaGrid D52B-1U` with two releases and distinct kernels `5.4.0-121-generic` and `4.15.0-187-generic`; 202208-30544 `QuantaGird D43N-3U` kept as the page's own misspelling. Max fact size is 885 bytes.
- Multiple certificates per model: five certificates share `QuantaGrid D52B-1U` and four share IEIT `NF5280M6`. Each cert is its own source and record, so facts are scoped to the certificate's source. No conflicts get merged. Release coverage differs across certificates and is preserved. Honest.
- The BIOS and hw rows are absent where the page prints none, as claimed (absent, not zero).
- A 504 detail page (202103-28876) made no source or device, which is correct.

**Exact-name catalog leads.** 13 matches confirmed by case-insensitive exact model equality against `data/devices.sqlite` (read-only). They are:
- Existing `QCT`: `QuantaPlex T42S-2U`, `QuantaGrid D52BV-2U`, `QuantaGrid D52BQ-2U`, `QuantaGrid D51B-2U`.
- Existing `Inspur`: `NF5180M5`, `NF5280M5`, `NF5280M6`, `NF5468M5`, `NF8260M5`, `NF8480M5`, `NS5162M5`.
- Existing `Wiwynn`: `SV7220G3`, which merges.
- Existing `Inventec`: `K885G7`, which merges.
- Punctuation-insensitive matching found no additional model equalities, apart from N1.

**No personal data.** There are no emails (0 in the candidate, and 0 across all captured HTML), no IPs, MACs, serials or hostnames. Cert ids and public vendor/model strings only.

**Owned-model check.** Case-insensitive over every path and content in the 193 files (including candidate and manifest): CS8200, CS8210, AZ4444, 7760, AS7760, DMC-12, AS7772, SV1120, SV1121, SV2121, SV2221, ByteDance, B4020, B4010. Result: 0 hits. Scope: this directory only.

**Temp import** (scratchpad COPY of `data/devices.sqlite`, `pipeline.import_source('curated')`).
- Returned 89. Counts, before to after: sources 490 to 579 (+89), devices 4361 to 4433 (+72), records 5625 to 5714 (+89), identifiers +89, facts 46904 to 48560 (+1656), affiliations 9290 unchanged (0 new). The FTS `search_index` row count is 4433, equal to devices.
- `integrity_check` ok, `foreign_key_check` empty.
- Every row of the prior sources, devices, records, facts, affiliations and identifiers tables is present and unchanged in the copy (EXCEPT count 0). No new non-ubuntu-cert sources or records.
- **Target lookups unchanged**: `vendor-inventory` for Quanta, Inspur, Wiwynn and Inventec, in both `manufacturer` and `all` scope, gives byte-identical JSON to the original DB. So the 89 new records are invisible to target-vendor inventory, including the two that merge into existing Wiwynn and Inventec devices. Plain `search` does show them: `search D52B` lists `QCT` and `QUANTA Computer Inc` D52BQ-2U/D52BV-2U as two devices, and `search NF5180M6` lists the `IEIT` and `Inspur Electronic...` twins.

## Brand and affiliation judgment

**Does brand-as-printed create misleading duplicates?** Yes, mildly. In the scratch import 23 model strings sit under two or more brands:
- 4 Quanta models: `QCT` vs `QUANTA Computer Inc`.
- 7 Inspur models vs existing catalog: `Inspur` vs `Inspur Electronic Information Industry Co., Ltd.` (NF5180M5, NF5280M5, NF5468M5, NF8260M5, NF8480M5, NS5162M5) and `Inspur` vs `IEIT SYSTEMS Co.,Ltd.` (NF5280M6).
- 12 models within this source only: `Inspur Electronic...` vs `IEIT` (4+7+12 = 23).
Search results are honest but noisy. A user searching `NF5180M6` sees two devices for what is probably one platform generation.

**Is it consistent with precedent?** No. The project's convention for third-party listings is canonical target brand plus an as-printed affiliation:

| Source | Brand | Affiliation (company as printed, target, role) |
|---|---|---|
| `ul-accton-platforms` (certification_body) | `Accton` | `ACCTON TECHNOLOGY CORPORATION`, Accton, `listing_holder` |
| `ul-amazon-platforms` | `Amazon Data Services` | `Amazon Data Services, Inc.`, `listing_holder` |
| `energystar-server-archive-v2` | `Inspur` | `Inspur`, brand; `Inspur Electric Information Industry Co, Ltd.`, target None, `program_partner` |
| `bsmi-reviewed-targets` | `Oracle` | `Oracle`, `regulatory_applicant` |

The precedent also keeps an unreviewed spelling as printed, with a null target (the ENERGY STAR partner string). It keeps role claims out of the brand column. A certification listing's make string is therefore treated as a sourced company statement, stored as an affiliation with a non-manufacturer role.

**Recommendation (least surprising).**
1. For `QUANTA Computer Inc` and `Inspur Electronic Information Industry Co., Ltd.` (both reviewed exact aliases) use canonical `Quanta` / `Inspur` as the brand, and add `[[<make as printed>, "listing_holder"]]` as the affiliation. The target then resolves automatically via `target_for`, and the source tier stays `certification_body`.
2. Keep `IEIT SYSTEMS Co.,Ltd.` as the printed brand with a `listing_holder` affiliation whose target is None; this matches the ENERGY STAR and registry treatment of unassigned names. Add no new IEIT-to-Inspur claim.
3. `Wiwynn` and `Inventec` already equal the target names, so keep them and add the same `listing_holder` affiliation.
4. Do not touch `QCT`. That brand is the vendor's own and is separate from `Quanta` in this catalog already.
5. Keep the coverage text that this is not a manufacturer, ODM, deployment or support claim; `listing_holder` is not a manufacturer-scope role.

If you want zero identity change, use the fallback of keeping brand as printed but adding the same affiliations. Then duplicates remain, but the lookups work.

I tested both options on scratch DB copies (curated import, integrity ok, foreign_key_check empty):

| Option | Devices after import | Quanta `all` | Quanta `manufacturer` | Inspur `all` | Inspur `manufacturer` |
|---|---|---|---|---|---|
| baseline (current DB) | 4361 | 73 | 41 | 141 | 3 |
| as submitted | 4433 | 73 | 41 | 141 | 3 |
| A: canonical brand + `listing_holder` | 4427 | 108 | 41 | 154 | 3 |
| B: printed brand + `listing_holder` | 4433 | 108 | 41 | 160 | 3 |

Manufacturer-scope counts are unchanged in both. Brand and listing roles alone never qualify as a manufacturer claim, which fits the policy.

A caveat on option A: Inspur-brand devices merge with existing Inspur rows with the same model (NF5180M5, NF5280M5, NF8260M5, NF8480M5, NF5468M5, NS5162M5). This follows the exact brand+model rule, and the facts stay record-scoped to the Ubuntu cert source. It is the same merge Wiwynn SV7220G3 and Inventec K885G7 already undergo.

Changing the candidate changes its sha256, so re-hash and update the handoff.

## re-review

Re-review verdict: ACCEPT (no must-fix). Candidate sha256 `054f90fc...d6a7` and live `data/devices.sqlite` sha256 `1cfd84ca...ba0c` (unchanged by me). Capture directory and manifest unchanged since the first pass (manifest sha in handoff still `8629c979...`).

Verified:
- **Brand/affiliation per record (89/89).** Brand is `Quanta` for 41 `QUANTA Computer Inc` records, `Inspur` for 22 `Inspur Electronic Information Industry Co., Ltd.`, `Wiwynn` 2, `Inventec` 2, and `IEIT SYSTEMS Co.,Ltd.` as printed for 22. Each record has exactly one affiliation `[make as printed, listing_holder]`; title is `<brand> <model>`. After import the affiliation targets are Quanta 41, Inspur 22, Wiwynn 2, Inventec 2, and IEIT null (22). `target_for('IEIT SYSTEMS Co.,Ltd.')` is None, so there is no IEIT-to-Inspur mapping anywhere. `ubuntu_cert_make_as_printed` still carries the original make string.
- **Merges.** In the current-catalog copy exactly 8 new records land on existing device ids, all by identical brand+model: Wiwynn `SV7220G3`, Inventec `K885G7`, and Inspur `NF5180M5`, `NF5280M5`, `NF5468M5`, `NF8260M5`, `NF8480M5`, `NS5162M5`. Each merged device gains exactly one ubuntu record, and no `ubuntu_cert_*` fact is attached to a non-ubuntu record. The existing `QCT` devices and IEIT NF5280M6/Inspur NF5280M6 stay separate. Device rows (kind, title) are unchanged for merged identities. The handoff's merge list is correct.
- **S1 listing coverage.** All 89 coverage strings name a listing file and its sha256 (`all-q-quanta` 41, `all-q-Inspur` 22, `all-q-IEIT` 22, `all-q-Wiwynn` 2, `all-q-Inventec` 2). Every sha256 matches the file, every cert row is present in the named listing, and the page-capture sha256 in each coverage matches its detail snapshot (0 failures). Coverage also states that the IEIT and Inspur strings are separate with an unresolved relationship.
- **S2.** `hw_testbench_{disk,usb,mouse,keyboard,cardreader}` (55/55/5/1/2) replace the old names and carry unchanged values. All 1948 value parts still match their saved HTML, and the release/kernel/BIOS re-parse still matches. The coverage text explains these are run-time peripherals and drives, not system spec.
- **S3 and handoff.** The 5xx note now says per-page errors and not throttling, and admits the loop did not stop. The lead list includes the loose `QuantaGrid D53XQ-2U` lead and the revised merge text. Counts in the handoff are reproduced exactly: +66 devices, plus 8 merged, equals 74.
- **Temp import into a copy of the current DB.** The import returned 89. sources 776 to 865, devices 4650 to 4716 (+66: Quanta 35, Inspur 13, IEIT 16, Wiwynn 1, Inventec 1), records 5925 to 6014, identifiers 5445 to 5534, facts 51850 to 53506, affiliations 9290 to 9379 (+89, all on the new ubuntu records; no existing record gained one), search_index 4716. `integrity_check` ok, `foreign_key_check` empty. All old rows in sources, devices, records, identifiers, facts and affiliations are present and unchanged (0 differences each), and no other source or record appeared.
- **vendor-inventory before to after** (`--limit 0`): Quanta manufacturer 41 to 41, all 73 to 108; Inspur manufacturer 3 to 3, all 141 to 154; Wiwynn manufacturer 2 to 2, all 23 to 24; Inventec manufacturer 44 to 44, all 82 to 83. Manufacturer scope is unchanged and the new rows appear only in `all`, as intended.

Notes (non-blocking):
- `Inspur NS5162M5` (existing kind `compute node`), `Inspur NF5468M5` (`GPU server`) and `Wiwynn SV7220G3` (`server family`) have a differently kinded existing identity. The merge is exact-name, and the Ubuntu facts are record-scoped, so the policy's `i24` vs NS5162M5 separation is not affected. Consumers should not read the Ubuntu server-level facts as describing the compute-node role specifically.
- The first-pass N2, N4 and N5 still apply and are handled in the handoff text.
