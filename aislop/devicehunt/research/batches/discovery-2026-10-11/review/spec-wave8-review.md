# spec_results wave 8 supplement review (Sonnet 5.5), 2026-10-11

Reviewer did not author the handoff. Scratch: `.../scratchpad/review-spec/imp/w8-*`. Only this file was written. Live `data/devices.sqlite` sha256 `baa016d2...b2184`, unchanged. Candidate sha256 `19f14b18...f88d` confirmed. Manifest `d147c225...294e`, request log `161a7082...d37c` confirmed.

## Verdict: ACCEPT

No must-fix, no should-fix. Same rules as revision 3, applied cleanly to 59 further Quanta Cloud Technology CPU2017 results.

## Must-fix

None.

## Notes

- N1. The robots.txt re-fetch is the only URL that overlaps the earlier 244 (stated in the handoff, and byte-identical to the saved copy). No result URL repeats.
- N2. Same notes as before: two unparenthesised `... AMD EPYC 9755` models stay unstripped (6 results), as the rule says. The same model also exists under the separate brand `Quanta` (not touched here).

## Verified

- **Hashes and request log.** 60/60 manifest entries re-hash with matching bytes. The log equals the manifest (url and sha, 60 rows). Statuses are 60 x 200, `err` null on all, and all URLs are unique, so there are no retries. Spacing runs from 10.0 to 11.0 s, between 12:10:41Z and 12:20:54Z. Robots (Crawl-delay 10, Disallow `/cgi-bin/`, `/includes/`, `/auto/`, `*.txt.rev*`, `/jhtest/`): no fetched URL matches a Disallow rule. 244 + 60 = 304 requests, as stated. On disk there are 59 texts, robots and the log (61 files). No unlisted file.
- **Facts.** 59 sources, 59 devices, 954 facts. All source ids are new (0 overlap with the promoted candidate). Coverage sha256 equals the snapshot hash for 59/59. 895/895 non-URL quotes are found in the cited captures. Each fact value equals or lies inside its quote, line numbers are valid, and no fact has a value of None, `--`, `Not Run` or empty. Max fact size is 76 bytes. No email addresses. Independent label-based re-parse (CPU Name, Max MHz, Nominal, Enabled, Orderable, Memory, Storage, plus per-line metrics from the printed result lines): 479 value matches. The 3 non-matches (`2 TB (16 x 128 GB 4DRx4 PC4-3200AA-L)`, cpu2017-20210630-27691/27692, 20210704-27697) are my parser tripping on blank continuation lines. The values equal line 71 of the capture (`Memory: 2 TB (16 x 128 GB 4DRx4 PC4-3200AA-L)`). Title line equals vendor plus system name for all 59.
- **Base-model audit.** 49 of 59 stripped. The only removed text is one trailing ` (...)` group in 9 distinct patterns, all CPU/clock annotations: `(2.25 GHz,AMD EPYC 9754)`, `(2.40 GHz,AMD EPYC 9654)`, `(AMD EPYC 7742 64-Core, 2.25GHz)`, `(AMD EPYC 7763, 2.45 GHz)`, `(AMD EPYC 7773X, 2.2 GHz)`, `(Intel Xeon 6780E)`, `(Intel Xeon 6787P)`, `(Intel Xeon Platinum 8480+, 2.0GHz)`, `(Intel Xeon Platinum 8592+)`. None is a chassis variant. The 10 unstripped names carry no parenthesis (3 distinct: `QuantaGrid D53XQ-2U` and the two `AMD EPYC 9755` names). `spec_system_name` identifier equals the full printed name on all rows, and titles are `brand model`.
- **Brand, role, tier, kind.** All 59 results print Hardware Vendor `Quanta Cloud Technology` with Test Sponsor `Quanta Computer Inc.`. The brand stays as printed, because it is not a TARGETS alias (`target_for` gives None), and it is not mapped to Quanta. The role is `reported_system_vendor` on all 59, which matches the rule. Tier `vendor` on all 59, kind `server` on all 59. Recomputed identity rules give 0 mismatches. Coverage on every source states the identity rules, the role rule, the "not a manufacturer, OEM or operator claim" disclaimer and the omission rule.
- **Merges are exact.** After the copy import, devices 5042 to 5042: 0 new devices. All 59 new records land on 9 existing `Quanta Cloud Technology` identities: D43K-1U (17 new), D55Q-2U (10), D54Q-2U (9), S44NL-1U (6), D44N-1U (6), D53XQ-2U (4), `S44NL-1U AMD EPYC 9755` (3), `D44N-1U AMD EPYC 9755` (3), S43KL-1U (1). The sum is 59. The prior record counts match the handoff (6, 2, 2, 4, 4, 2, 1, 1, 1). Same brand, same model string, so the merge is correct on the project's exact brand+model rule. Kind and title of the existing devices are unchanged.
- **Owned-model check.** Zero hits for CS8200, CS8210, AZ4444, AS7760, standalone 7760, DMC-12, AS7772, SV1120/1121/2121/2221, ByteDance, B4020, B4010 and Oracle E4-2C/E5-2C/X7-2C/X10-2C over the 59 texts, robots, the log, the manifest and the candidate. Scope: the wave8 files only.
- **Temp import** into a scratchpad COPY of the CURRENT database: returned 59. Sources 1672 to 1731, records 6821 to 6880, affiliations 10178 to 10237, identifiers 6429 to 6488, facts 65930 to 66884, `search_index` 5042. `integrity_check` ok, `foreign_key_check` empty. An EXCEPT comparison of every non-FTS table against the pre-import copy shows 0 removed or changed rows. `vendor-inventory --limit 0` is byte-identical before and after for Quanta, Inspur, Oracle and Foxconn in both `manufacturer` and `all` scope. A reported-system-vendor role on a non-alias brand adds no target assertions.
