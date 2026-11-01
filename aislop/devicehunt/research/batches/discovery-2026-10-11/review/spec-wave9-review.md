# spec_results wave 9 supplement review (Sonnet 5.5), 2026-10-11

Reviewer did not author the handoff. Scratch: `.../scratchpad/review-spec/imp/w9-*`. Only this file was written. Live `data/devices.sqlite` sha256 `09891954...79a4`, unchanged. Candidate sha256 `70f37903...24b1`, manifest `e3ab63eb...3051` and request log `2e1e4846...2646` all confirmed.

## Verdict: ACCEPT

No must-fix, no should-fix. Same revision-3 conventions, applied cleanly to 150 further IEIT and Inspur CPU2017 results.

## Must-fix

None.

## Notes

- **N1. Robots not re-fetched: acceptable.** The last fetch was wave 8 at 12:10:41Z. Wave 9 ran 12:53:07Z to 13:18:56Z, so the cached copy was 42 to 68 minutes old, with the same host and the same user agent. The wave 8 copy was byte-identical to the original one hours earlier. Caching a robots.txt for hours is normal practice, and nothing in this file's rules makes a refresh within the hour necessary. The manifest states the reason. No fetched URL matches any Disallow rule, so the outcome would not have changed.
- **N2. Brand-prefixed model strings remain un-normalised** (for example `Inspur NF5180M6`, `Inspur NF5280M6`, and `NF5180A7` versus `Inspur NF5180A7` under the IEI string). This is as designed (earlier note N2). The same model also exists under `Inspur Corporation`, IEIT and IEI as separate brands (earlier N1).
- **N3. Unfetched backlog.** 244 older Inspur CPU2017 names remain, and the handoff lists them as a lead. That is stated, not a defect.

## Verified

- **Hashes and request log.** 150/150 manifest entries re-hash with matching bytes. The log equals the manifest (url and sha, 150 rows), with 151 files on disk (150 texts plus the log). Statuses are 150 x 200 with `err` null. All 150 URLs are unique, so there are no retries. Spacing is 10.0 to 11.0 s. Every URL is a `.txt` under `/cpu2017/results/`; none touches `/cgi-bin/`, `/includes/`, `/auto/`, `/jhtest/` or a `.txt.rev*` path (robots disallows). There is zero URL overlap with the earlier 244 requests or with wave 8's 60 (303 earlier URLs checked). Combined total is 454, as stated.
- **Facts.** 150 sources, 150 devices, 2548 facts. The source ids are new (0 overlap with the promoted candidate or wave 8). Coverage sha256 equals the snapshot hash for 150/150. 2398/2398 non-URL quotes are found in the cited captures. Each fact value equals or lies inside its quote, and line numbers are valid. Zero fact values are None, `--`, `Not Run` or empty. Max fact is 91 bytes. No emails. The title line equals vendor plus system name for all 150. An independent label/printed-line re-parse (CPU Name, Max MHz, Nominal, Enabled, Orderable and the per-line metric values) matched 1048 values with 0 mismatches.
- **Base-model audit (150/150 stripped).** The removed text is exactly one trailing ` (...)` group with one `(` in every case, and the printed name starts with the stored model. There are 23 distinct patterns (digits masked), all CPU SKUs: `(Intel Xeon Gold ####)` 25, `(AMD EPYC ####)` 21, `(Intel Xeon Gold ####Y)` 20, `(Intel Xeon Silver ####Y)` 13, then Platinum, Bronze, `(AMD EPYC ####P/X/F)` and one `(Intel Xeon E-####G)`. None is a chassis variant, form factor or revision. The identifier `spec_system_name` equals the full printed name on every row, and titles are `brand model`. 30 distinct (brand, model) identities.
- **Brand and role.** Brands as printed: `Inspur Corporation` 74, `IEIT Systems Co., Ltd.` 64, `Inspur Electronic Information Industry Co., Ltd. (IEI)` 12. `catalog.target_for` is None for all three, so there is no canonicalisation, IEIT is never mapped to Inspur, and no target affiliation is created. Test Sponsor equals Hardware Vendor on all 150, so the role is `brand` on all 150, with 0 rule mismatches. Tier is `vendor` and kind is `server` on all 150.
- **Merges and additions are exact.** After the copy import, devices went from 5067 to 5084: 17 new, 13 merged, and the 150 new records are all accounted for across the 30 touched identities. The 13 merges (new/old records) match the handoff: IEIT NF5180M7 19/5, NF5280M7 19/6, NF5466M7 10/3, NF5180A7 4/2, NF5280A7 4/2, NF5688M7 1/2; IEI NF5280M7 4/14, NF5180M7 4/14, `Inspur NF5280A7` 1/1, `Inspur NF5180A7` 1/1; `Inspur Corporation` `Inspur NF5180M6` 16/3, `Inspur NF8480M6` 5/1, `Inspur NF5280M5` 1/8. All are same brand string and same model string, so each merge is exact under the project rule. The 17 additions are distinct (brand, model) pairs not previously in the catalog.
- **Owned-model check.** Zero hits for CS8200, CS8210, AZ4444, AS7760, standalone 7760, DMC-12, AS7772, SV1120/1121/2121/2221, ByteDance, B4020, B4010 and Oracle E4-2C/E5-2C/X7-2C/X10-2C over the 150 texts, the log, the manifest and the candidate. Scope: the wave9 files only.
- **Temp import** into a scratchpad COPY of the CURRENT database: returned 150. Sources 1777 to 1927, devices 5067 to 5084, records 6926 to 7076, affiliations 10258 to 10408, identifiers 6534 to 6684, facts 67503 to 70051, `search_index` 5084. `integrity_check` ok, `foreign_key_check` empty. An EXCEPT comparison of every non-FTS table against the pre-import copy shows 0 removed or changed rows. `vendor-inventory --limit 0` is byte-identical before and after for Quanta, Inspur, Oracle and Foxconn in both `manufacturer` and `all` scope. This is expected because none of the three brand strings is a TARGETS alias.
