# documentation review (independent, Claude Sonnet 5.5), 2026-10-11 discovery batch

Verdict: **ACCEPT with notes after two must-fix edits.** Everything below is read-only verified; nothing outside this file was written. Two PROGRESS.md statements are factually wrong against the live files (facts total; accepted-manifest job count) and one more contradicts the promoted state (latest rebuild). All three are one-line fixes. The rest is wording/scope.

Scope: PROGRESS.md (whole), README.md (lines 50-125, 180-236), research/discovery-2026-10-11.md (whole), research/ops-2026-10-10.md, research/brand-family-proposal-2026-10-11.md; checked against `data/` (SQLite opened `?mode=ro`), integration logs, suite-result JSONs, review files and handoffs. The suite was not re-run.

## Verified (no action)
- `data/devices.sqlite` sha `d78b0b7ea0ed...` = `devices-candidate-discovery-2026-10-11-e.sqlite`; `data/import-manifest.json` sha `f1ccce0e0e94...` = `import-manifest-candidate-discovery-2026-10-11-e.json`. user_version 29. devices 5,084 / records 7,076 / sources 1,927; 61 documents / 3,716 pages; 146 artifacts / 374 receipts / 4 statements; 22 hyperscaler_evidence rows.
- Manifest: 144 imports, 3,756 files, `campaign_gate` present. Chain of promoted shas, each = the next wave's `devices-before-*` and `devices-candidate-*`: `1cfd84ca` (a) -> `ab8ed2e3` (b) -> `baa016d2` (c) -> `09891954` (d) -> `d78b0b7e` (e); `0646f4f5` (hunt) = `candidate-hunt-2026-10-10-b` = `devices-before-discovery-2026-10-11.sqlite`. The manifest pairs match the same way (`ea81e645`, `5b54fcc8`, `d5799672`, `a2623161`, `f1ccce0e`; `63254199` = candidate-hunt-b manifest = before-discovery manifest; `5ddb1579` = candidate-c3b = before-hunt manifest).
- Wave deltas (from the backup DBs): waves 1-2 +289/+300/+286 identities/records/sources, +4,946 facts; 3-4 +365/+701/+701, +11,853; 5-6 +27/+195/+195, +2,227; 8 +25/+105/+105, +1,573; 9 +17/+150/+150, +2,548. All match PROGRESS 32 and discovery note per-wave "Integration" lines. Per-input record sums match (e.g. 61+183+14+12+11+19 = 300; 89+33+237+342 = 701; 158+29+8 = 195; 21+25+59 = 105).
- Suites: 653 / 658 / 669 / 669 / 671 pre-promotion (suite-result JSONs `discovery-2026-10-11-pre[-b..e]-suite-result.json` and logs), 0 blocked, passed; 644 (ops), 618 (hunt final), 614 (hunt pre-2) also match. Post-promotion gate logs: OK (b: 15 tests = gate + lookup; others 1 test). compare/compare-b..e: "All 52 tables/indexes and baseline results match".
- All-associations 4,579 pairs / 4,441 identities (from 4,344 / 4,206 before wave 1; +235; unchanged by wave 9: d = e = 4,579); manufacturer scope byte-identical before/after (`vendor-inventory --scope manufacturer` output md5 equal); 2,142 pairs / 2,141 identities.
- New-source affiliation roles (live DB): ubuntu/redhat/netbox/linuxhw = `listing_holder` only; SPEC = `brand` 351 + `reported_system_vendor` 95; software-reference sources have no affiliations. So the README and discovery-note "non-manufacturer roles only / no company role" claims hold.
- Storage line: 28 `data/*.sqlite` files, 10.68 GB apparent size, `data/archive` 65.9 MB apparent. Every one of the 28 is named in PROGRESS; every `data/...sqlite|json` path named in PROGRESS exists. Backup pairs `devices-before-discovery-2026-10-11[-b..e]` are byte-identical to `devices-candidate-discovery-2026-10-11[-b..e]` shifted by one wave exactly as claimed; `candidate-e` pair is byte-identical to the live pair; `candidate-hunt-2026-10-10-b` + its manifest are byte-identical to the hunt-promoted pair (and to `devices-before-discovery-2026-10-11.sqlite`).
- Review verdicts quoted for the discovery inputs (onie, bmc, sonic, fb, ubuntu, netbox, spec, redhat, firmware, linuxhw, ieit, model-collisions, lookup-context, lookup-citations, bundle-sha, nos2, candidates a-e, wave5/8/9) and the ops S4 (REJECT -> ACCEPT with notes) / queue (ACCEPT with notes -> re-review ACCEPT) all match the review files. "lookup matched_in scope fixes and linuxhw should-fixes coordinator-checked, not re-reviewed" and "S4 re-review notes 1-2 coordinator-applied, not re-reviewed" are correct (no re-review section exists for those).
- Relative links: every markdown link in the five docs resolves. Only backtick path that does not exist is `data/brand-families.json` in the proposal (lines 54, 103), correctly a proposed path. Tables: no blank line inside any table; column counts consistent in all five files.
- Brand-family proposal numbers (351 groups = 326/18/7; skipped 445; Accton 236, Edge-Core 170, Inspur 51, Oracle 26, Quanta 18) reproduce exactly on `devices-before-discovery-2026-10-11-c.sqlite` (= candidate b, 5,015 identities).
- IEIT wording (discovery note 125-131) matches the ieit-source handoff: first-party = FY2024 report (P1), Articles Dec 2025 (P2), 2023-08 Articles draft (P7), FY2023 report (P8); old name and 2023-08 amendment = Sina/Xueqiu mirrors, provisional; effective date unestablished; scoped to the one legal entity, not `Inspur Corporation`/Group.
- Wave-9 detail (150 results, 64 IEIT + 86 Inspur names, 17 new / 13 merged, 244 unfetched) and wave-5/8 request counts (161, 21; Red Hat 195+161+21 = 377) verify.

## Must-fix
1. **PROGRESS.md:33 facts total is stale/wrong.** Says `facts | 67,503 (... +2,548 wave 9)`. 67,503 is the count *before* wave 9; the live catalog has **70,051** facts (67,503 + 2,548; also candidate-e review/handoff-wave9 "67503 -> 70051"). Fix: `70,051`.
2. **PROGRESS.md:58 accepted-manifest row is stale.** `data/import-manifest.json` (127 jobs, gated by the c3b release)` -> the manifest now has **144 jobs / 3,756 inputs** (PROGRESS:41 already says so, so the document contradicts itself). Fix: `(144 jobs, 3,756 hashed inputs, gated by the c3b release)`.
3. **PROGRESS.md:64 "latest verified rebuild" is wrong.** It names `data/rebuilt-hunt-2026-10-10-b.sqlite`; the latest verified rebuild is `data/rebuilt-discovery-2026-10-11-e.sqlite` (rebuild-e.log, compare-e.log: all 52 tables match, 5,084/7,076/1,927). That file is simultaneously listed as cleanup-pending on line 65, so the table currently implies the only recorded "latest" rebuild is an older one. Fix: point the row at `-e` and say the older rebuilds on line 65 are superseded (keeping `rebuilt-discovery-*-e` out of the cleanup list or marking it "latest, keep until next rebuild").

## Should-fix
1. **discovery-2026-10-11.md:42** "All six research inputs and the engineering change": the table below has five research inputs (onie, entity-manager, bmc, sonic-pddf, fb-openbmc) plus the bundle-sha engineering change. Fix "six" -> "five".
2. **discovery-2026-10-11.md:44** "entity-manager REJECT-conditional -> fixed -> re-review ACCEPT": the first-pass verdict was "ACCEPT with notes, conditional on fixing M1 and M2" (entity-manager-review.md:3), not REJECT. Fix: "ACCEPT conditional on M1/M2 -> fixed -> re-review ACCEPT".
3. **discovery-2026-10-11.md:35** ieit_source "(<=25 requests)": the handoff records round 1 = 19 (cap 25) plus a round-2 follow-up of 11 (cap 20 additional), **30 requests total**, and also notes the robots-before-fetch ordering slip for Sina/cnstock/inspur.com. State "<=25 + a <=20 follow-up; 30 used" so the cap is not misread.
4. **discovery-2026-10-11.md:94 and :122 and proposal:4,13** "351 groups on the promoted catalog": true only for candidate b. The current catalog gives **355 groups** (323/18/14; 478 software-kind rows skipped). Scope the number: "351 on candidate b (355 on the catalog after wave 9)". The proposal already says "candidate b" on line 4 but its title line 13 and the discovery note do not; add the catalog date there and a one-line "numbers superseded" pointer.
5. **brand-family-proposal:71-74 and :99 (decision 4)** still say no IEIT primary source exists / "authorise me to look". The same-day ieit-source handoff found one (first-party current-name leg; old-name leg provisional). Add a status note under "evidence already in the catalog" and reword decision 4 to "accept/record the IEIT leg, and who reviews it" (decision 7 already covers reviewer). Otherwise a reader following the proposal alone re-opens a closed lead; discovery note 125 says "7 decisions listed" which is still accurate.
6. **discovery-2026-10-11.md:96-97** Wi-Fi "5 requests, nothing captured": raw dir holds two robots.txt, a 301 stub, `pf-q-Accton.html` and `productfinder-app.js`. Say "no product data captured (robots, finder page and app JS only)".
7. **discovery-2026-10-11.md:139 and :150** "SPEC total 304" / "SPEC total 454" are cumulative **requests** (244+60, +150), but sit next to result counts; result records are 237+59 = 296 and 446 (live DB: 446 `spec-*` sources). Write "SPEC requests to date 304" / "454 (446 result records)". Line 137 "Red Hat total 377" is also requests (records 521).
8. **PROGRESS.md:3,7-9** "updated 2026-10-10" and the milestone list omit the 2026-10-11 discovery batch note (it is linked only from the catalog table). Update the date and add `[discovery batch](research/discovery-2026-10-11.md)` plus the [brand-family proposal](research/brand-family-proposal-2026-10-11.md) (item 6 line 191 mentions a "later reviewed brand-family/alias layer" with no link).
9. **PROGRESS.md:65** cleanup list: each "(byte-identical to the promoted pair)" is true only at that wave's promotion (only `-e` still equals the live pair). Reword "byte-identical to the pair promoted at that wave". Also the list says "keep" for each wave's `devices-before-...` while the last sentence says only the newest is needed; pick one ("keep newest = -e; older four are cleanup candidates"). Cross-note that `devices-before-discovery-2026-10-11.sqlite` (kept) is byte-identical to `candidate-hunt-2026-10-10-b` (listed for cleanup), so only one of the two needs keeping.
10. **PROGRESS.md:32** "+235 from `listing_holder` / `brand` / `reported_system_vendor` listings": the 235 new target/identity pairs come from `listing_holder` (367 role rows) and `brand` (30) only; `reported_system_vendor` on non-alias brands contributes no target pair (confirmed by the wave-8 handoff). Drop the third role or say "(reported_system_vendor adds none)".
11. **PROGRESS.md (run-time line after the table of next work, "about 12-15 minutes")**: the discovery full-suite runs took 708-1,192 s (12-20 min). Say "about 12-20 minutes".

## Notes
- README:59-67 matches `lookup.py` wording ("not values") but omits that names hit only inside a citation locator are reported separately as `cited-path only: N fact names` (wave-8 change). One clause would prevent the old "reads as corroboration" misreading (candidate-c should-fix 1). Discovery note 116-118 lists that candidate-c note without saying it was fixed in wave 8 (141-144); add "fixed in wave 8".
- README `model-collisions` subsection (flags `--target/--limit/--all-kinds/--min-key-length/--format`, reasons, skip rule) matches `--help` and the output. The c4 `reconcile` and c3b baseline-job sentences match the ops note, S4/queue handoffs and reviews, and correctly say the driver phase is only help-checked.
- ops-2026-10-10.md: all claims verify (644 OK, 0 blocked; catalog `0646f4f5`, manifest `63254199` at that time, now superseded by later promotions - the note is date-scoped and fine).
- PROGRESS item 4 "no change in these 7 declared manufacturer queries" is correctly scoped ("not a regulator-wide claim"); no company-role claim appears for software-reference, listing or SPEC sources anywhere in the five documents.
- Hyperscaler: PROGRESS says 22 rows on 18 devices; `hyperscaler-inventory` prints 20 assertions on 18 identities (default eligibility filter). Pre-existing, unchanged this session; no action.
- Reviews are absent for kernel-psus, energystar-check, wifi-alliance and brand-family-proposal; the docs correctly do not claim any.

## coordinator follow-up

- must-fixes 1–3 applied in PROGRESS (facts 70,051; accepted manifest 144 jobs; latest verified rebuild
  `data/rebuilt-discovery-2026-10-11-e.sqlite`, removed from the cleanup list).
- should-fixes applied: batch note (five research inputs; entity-manager wording; IEIT 30 requests; 351 groups scoped to
  the wave 3–4 catalog, 355 now; Wi-Fi Alliance wording; "SPEC requests so far"); proposal IEIT paragraph and decision 4
  updated with the same-day source; PROGRESS header date and milestone link; cleanup row rewritten (candidate copies
  byte-identical at their own promotion, only `-e` equals the current pair; keep only the newest backup pair and rebuild);
  all-associations role wording; suite runtime 12–20 minutes. not independently re-reviewed.

## re-review (wave 10 / candidate f), 2026-10-11

Verdict: **ACCEPT with notes.** No must-fix against the facts; two should-fix wording/staleness items. Read-only; suite not re-run.

Verified:
- Live `data/devices.sqlite` sha `e7cc01a1e39f...` = `devices-candidate-discovery-2026-10-11-f.sqlite`; `data/import-manifest.json` `c287a21c1a19...` = `import-manifest-candidate-discovery-2026-10-11-f.json`. `devices-before-discovery-2026-10-11-f.sqlite` = `d78b0b7e...` and its manifest = `f1ccce0e...` (the wave-9 pair). Rebuild `-f` is a different file by bytes but compare-f.log: "All 52 tables/indexes and baseline results match".
- Counts 5,107 / 7,225 / 2,076 / facts 74,074 (wave 10 = +23 / +149 / +149 / +4,023; matches handoff-wave10); manifest 145 jobs / 3,906 files. Batch totals (0646f4f5 -> e7cc01a1; 4,361 -> 5,107; 5,625 -> 7,225; 490 -> 2,076; 46,904 -> 74,074; 127 -> 145) all correct.
- Suites: pre-promotion-suite-f.log and `discovery-2026-10-11-pre-f-suite-result.json` = 671 OK, 0 blocked; post-promotion-gate-f OK. "653 -> 671" correct. SPEC requests 454 + 150 = 604; 149 results = 7+21+40+81; SPEC sources 595 = 446 + 149.
- All-associations 4,581 / 4,443 (+237 since wave 1; roles listing_holder + brand only); manufacturer-scope output md5 identical before and after wave 10.
- spec-wave10 and candidate-f reviews exist, both "ACCEPT with notes", as claimed. All links in the five docs resolve; no broken or blank-line tables.
- Earlier fixes landed: facts, manifest row, latest-rebuild row, "five research inputs", entity-manager wording, IEIT 30 requests, SPEC "requests so far", 12-20 minutes, proposal IEIT update and decision 4, header and milestones.
- Cleanup row: candidate `-f` pair equals the current pair; others equal the pair promoted at their own wave; keep-list (`-f` before-pair, `-f` rebuild) correct. Storage line: 31 SQLite files, 11.87 GB, archive 65.9 MB: correct.

Should-fix:
1. **research/discovery-2026-10-11.md:94**: "355 on the current catalog" is stale after wave 10; `model-collisions` on the live catalog now reports **362 groups** (320 / 19 / 23). Write "362 after wave 10" (or drop the current-catalog figure).
2. **PROGRESS.md:211** says every SQLite file is "listed in the role table", but `data/rebuilt-hunt-2026-10-10-b.sqlite` (formerly "latest verified rebuild") now appears nowhere. Add it to the superseded list on the cleanup row (all other files are covered by the `{,-b,...}` brace forms).

Notes:
- PROGRESS.md cleanup row still says `candidate-hunt-2026-10-10-b.sqlite` / its manifest are "byte-identical to the promoted ..." without the "at that wave" qualifier (true only for the hunt promotion; it also equals the kept `devices-before-discovery-2026-10-11.sqlite`, which is on the cleanup list).
- PROGRESS.md:208-210 item 7 ends with a dangling `;` and blank line before the storage line (cosmetic).
- Proposal line 4 still says numbers are from candidate b (5,015 identities): accurate and dated; leave as is.

## coordinator follow-up 2

- re-review should-fixes applied: model-collisions count now "362 after wave 10"; `data/rebuilt-hunt-2026-10-10-b.sqlite`
  added to the superseded list; hunt candidate "byte-identical" wording scoped to 2026-10-10; item 7 punctuation.
