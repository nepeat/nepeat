# ieit_source independent evidence review (Sonnet 5.5), 2026-10-11

Reviewer did not author the handoff. Offline only. Scratch: `.../scratchpad/review-ieit/` (extracted text). Text extraction used the existing nix-store `pdftotext` (poppler-utils 26.06.0, `-layout`; nothing installed); HTML decoded with `python3 -I` (Sina page is GB18030, cnstock UTF-8).

## Verdict: ACCEPT with notes (2 must-fix wording/gating items, no change to the core finding)

The core claim holds: in the issuer's own 2024 annual report (CNINFO) and its December 2025 Articles of Association (SZSE disclosure host), the registered Chinese name 浪潮电子信息产业股份有限公司 (000977) is printed with English name `IEIT SYSTEMS Co., Ltd.`. The only evidence that the previous English name was `Inspur Electronic Information Industry Co.,Ltd.` is a Sina Finance copy of the 2023 charter-amendment bulletin, so the "renamed" leg is primary content on a mirror host, not first-party host evidence. The handoff discloses this itself, but its "meets the bar" table row is stronger than that.

## 1. Capture and request log

- Hashes: all 19 files in `capture-manifest.json` re-hash to the recorded sha256 and byte count (0 mismatches). All `.err` files are empty.
- Count and order: 19 requests (cap 25). Timestamps (UTC, 12:05:32 to 12:08:14) have a minimum gap of 3 s (several exactly 3-4 s); no host is hit twice in under 3 s. szse.io: robots, then the page 111 s later (Crawl-delay 10 respected; only 2 requests). Statuses in the manifest equal `manifest.tsv`. No URL is requested twice (no retries, no reroutes). The 7 WebSearch queries are listed in the manifest and not counted in the cap; fine.
- Robots obeyed, with two gaps (S1 below): robots fetched and honoured for static.cninfo.com.cn (404), disc.static.szse.cn (404), szse.io (allow `/`), www.ieisystem.com (allow all), en.ieisystem.com (404, a JSON 404 body). inspur.com: the PDF was requested before robots (ordering slip, disclosed); robots returned 403, host dropped, nothing further. That handling is conservative and matches the brief (403 recorded once, never rerouted). Under RFC 9309 a 4xx robots response means "unavailable", so stopping was stricter than required; fine.
- Observation on the inspur.com pair: the "404" PDF capture and the "403" robots capture are byte-identical (same sha256 `20c55aff...`, 157,915 bytes): both are the same HTML 404 page (title `404`, Inspur Group product navigation). The file `ar2020-full-inspur.com.pdf` is therefore an HTML page with a `.pdf` name. The "different Inspur-brand site" remark in the handoff is consistent with that nav, but is inference, not stated by any capture.
- Not checked: `money.finance.sina.com.cn` (plain http) and `paper.cnstock.com` have no robots fetch in the log, and the handoff/manifest `robots_notes` are silent on them. Neither request was blocked (200s), but "robots obeyed" cannot be claimed for these two hosts.

## 2. Quoted text verification

Verified directly from the saved captures.

| claim | result |
| --- | --- |
| P1 `ar2024-1222950880.pdf`, PDF page 6 (printed page 6), 第二节 一、公司信息: `公司的中文名称 浪潮电子信息产业股份有限公司`, `公司的外文名称（如有） IEIT SYSTEMS Co., Ltd.`, `公司的外文名称缩写（如有） IEIT`, `公司网址 www.ieisystem.com`, `股票简称 浪潮信息`, `股票代码 000977` | exact (the abbreviation label wraps over two lines in the PDF; content as quoted). Header line: `浪潮电子信息产业股份有限公司 2024 年年度报告全文`. Section 四 states no change in main business; no name-change history is given in this report. |
| P2 `articles-2025-12-szse.pdf` (78 pages, cover `公 司 章 程 二○二五年十二月`): `第四条 公司注册名称：浪潮电子信息产业股份有限公司` ends PDF page 4; `英文名称： IEIT SYSTEMS Co., Ltd.` is the first line of page 5 | exact; page split as stated. The doc mentions events through 2025-12-04, consistent with a Dec 2025 text. Cover says only "December 2025"; whether this exact text was the adopted/registered version is not shown (low relevance to the name). |
| P3 `amendment-2023-sina.html`: 修订前 `第四条  公司注册名称：浪潮电子信息产业股份有限公司  英文名称： Inspur Electronic Information Industry Co.,Ltd.`, 修订后 `... 英文名称：IEIT SYSTEMS Co., Ltd.`; closing `该议案尚需提交公司2023年第二次临时股东大会审议，且需经市场监管部门核准。本次《公司章程》的修订最终以市场监管部门核准登记为准。` signed `浪潮电子信息产业股份有限公司 2023年8月25日` | exact. Locator "line 257 of the stripped text" is not reproducible (my stripping gives line 44); cite the table row (修订前/修订后 comparison, 第四条) instead. Two details the handoff omits: the Sina header shows `公告日期:2023-08-26` (signed 08-25), and the capture links the original attachment `file.finance.sina.com.cn/.../2023-08-26/9457677.PDF` (not fetched; still a Sina host). Printed typo `20223年6月2日` in the page text is irrelevant to the name. Also note the closing sentence says the amendment is "ultimately subject to market-supervision registration approval", i.e. P3 alone is a proposal. |
| P4 `resolution-2023-09-12-cnstock.html` (title `...2023年第二次临时股东大会决议公告|上海证券报`, `来源：上海证券报`, 2023-09-12, 公告编号 2023-056, meeting 2023-09-11) item 3 `审议《关于增加注册资本暨修订〈公司章程〉的议案》；表决结果：同意509,804,887股，占...99.9903%` then `表决结果：通过。该议案为特别决议事项，获得出席本次股东大会有效表决权股份总数的 2/3 以上通过。` | exact. The handoff calls the item-3 to P3 link "inferred"; it is firmer than that: P3 names this meeting, item 3 is the charter amendment with the capital increase (P3 also changes registered capital), and it is the only charter item. Still no statement that the English name was in it. Handoff says "China Securities Journal / Shanghai Securities News"; the page says 上海证券报 only. |
| P5 sites: `Copyright © 2024 浪潮电子信息产业股份有限公司` (www), `Copyright © 2024 IEIT SYSTEMS CO.,LTD. All Rights Reserved.` (en), `IEIT SYSTEMS (stock code SZ000977) is a global leader in IT infrastructure products, solutions, and services` ; zero occurrences of "Inspur" in all four pages | exact |
| S1 szse.io: JSON-LD `name IEIT SYSTEMS Co., Ltd.`, `legalName 浪潮信息`, publisher `SZSE Releases`, text `also known as 浪潮信息`; titles `Inspur Information 2025 Annual ...`, `...Shareholders' Meeting of Inspur Information` | exact. Not in the handoff: the same page's machine-translated summaries (2026 items) render the issuer as `Inspur Electronic Information Industry Co., Ltd.` ("Annual ... Q1 Report for 2026", performance forecast, trading-fluctuation notice), i.e. the old English string is also produced as a translation of the Chinese name. That is a reason not to read the string's appearance in non-primary sources as a former official name (see S3). |
| `ar2020-szse.pdf` line 352 `Branch of Inspur Electronic Information Industry Co.,Limited` | exact; a subsidiary string (Limited, not Ltd.), correctly excluded. No issuer English name in that summary. |

English glosses: faithful. One presentation issue: the P1 gloss writes `Company Chinese name: Inspur Electronic Information Industry Co., Ltd. [the Chinese string]`. That is the reviewer-visible translation of a Chinese name and must never be recorded as a printed English name of the 2024 report (P1 prints no "Inspur" at all). No other overreach in the glosses.

## 3. Tier classification

| capture | tier | note |
| --- | --- | --- |
| P1 CNINFO annual report | primary (issuer filing on a designated disclosure platform; static.cninfo.com.cn, URL dated 2025-03-29) | states the pairing in the issuer's own words |
| P2 Articles, disc.static.szse.cn | primary (issuer document on the exchange disclosure host) | independent of P1 |
| P3 Sina Finance | secondary-hosted copy of a primary bulletin | content is the issuer's signed text; provenance rests on Sina's labelling (`公告日期`, stockid 000977, bulletin id 9457677). Handoff's label "secondary-hosted primary" is right. |
| P4 cnstock/上海证券报 | secondary host, designated disclosure media; supporting | |
| P5 company sites | issuer's own, brand-use evidence only | |
| S1 szse.io | secondary; translations are machine output, not primary | operator not established |

So: the current English name <-> legal entity pairing is primary (P1, P2). The rename itself (old name -> new name) is evidenced only through the Sina mirror (P3), and the old-name leg has no first-party host copy anywhere in the batch. Nothing here independently shows the old English name was used in any earlier primary filing; the 2020 summary, the only earlier filing captured, does not print it.

## 4. Precision of the conclusion

Good. The bottom line says one legal entity, English name only changed (Chinese legal name identical before and after), and explicitly excludes Inspur-brand products, `Inspur Information` as an official English name, Inspur Group/Software and others. I agree with all exclusions. Items to keep out of any row: `Inspur Corporation`, `Inspur Information` (only MT titles), `Inspur Electric Information Industry Co, Ltd.` (ENERGY STAR spelling, similarity only), `...(IEI)`, Inspur Power Commercial Systems, `Branch of Inspur Electronic ... Co.,Limited` (a 2020 subsidiary), and the Inspur brand string itself. Effective date is correctly left open (proposal 2023-08-25/26, meeting 2023-09-11, registration approval not seen; new name seen only in 2024 AR onward).

Two precision nits: (a) the suggested typed row lists `Inspur Electronic Information Industry Co., Ltd.` (with a space; the amendment prints `Co.,Ltd.` without one) and punctuation variants as brand strings; keep each as printed and map by explicit string, not normalisation; (b) the bare `IEIT SYSTEMS` brand and `IEIT` abbreviation rest on P1 (abbreviation) and P5 (site copy), which is fine for an `abbreviation` relation but not for a manufacturer claim.

## 5. Proposal evidence bar (option B: primary statement per mapping, typed relation, url/sha, reviewer, date)

- `IEIT SYSTEMS Co., Ltd.` -> legal entity 浪潮电子信息产业股份有限公司 (000977): MEETS (P1 + P2, first-party hosts, hashed, dated). Relation: `same_entity`/English name; a company-role claim only, never `manufacturer`.
- `Inspur Electronic Information Industry Co., Ltd.` (old English name) -> same entity, relation `renamed`: MEETS IN SUBSTANCE, but its only support is P3 on a mirror; I would record it as provisional (evidence tier "primary content on secondary host") until a CNINFO/SZSE copy is hashed. Note the policy file already treats `Inspur Electronic Information Industry Co., Ltd.` as the legal-name string of the existing Inspur target (bytedance-inspur note), so the family row ties an already-assigned string to IEIT; that is the point of the work and the evidence supports it.
- Bare `IEIT SYSTEMS` / `IEIT`: abbreviation-grade, as the handoff says.
- `Inspur` brand, `Inspur Information`, `Inspur Corporation`, `(IEI)`, Group and others: NOT shown, correctly left out; the catalog's Red Hat/Ubuntu "same models under both names" observation stays circumstantial.
- Still missing: (i) first-party host copy of the 2023-08-25 amendment (the Sina attachment 9457677.PDF is Sina-hosted; look for the CNINFO/SZSE original, not the QQ mirror); (ii) an earlier first-party filing showing the old English name (2022 annual report front page 外文名称, or the pre-2023 Articles); (iii) the first filing using the new name and any registration-completion notice for the effective date (2023 annual report / Q3 2023 report front page); (iv) reviewer of record and date for the row (decision 7 in the proposal, unresolved).

## Findings

### must-fix
- **M1. Gate the `renamed` leg on its tier.** The handoff table says the old->new mapping "meets the bar". Before it is used in a family row, record the evidence for the old-name leg as "primary content on a secondary host (Sina)" and either hold that leg as provisional or fetch a CNINFO/SZSE copy of the 2023 amendment. The current-name leg (P1/P2) may proceed independently.
- **M2. Correct the request-log statements.** State in the handoff/manifest `robots_notes` that no robots.txt was fetched for `money.finance.sina.com.cn` and `paper.cnstock.com`, so "robots obeyed" applies only to the six hosts listed. Also note that the saved `ar2020-full-inspur.com.pdf` is an HTML 404 page (byte-identical to the inspur.com robots capture) and not a PDF.

### should-fix
- S1. Fetch (one request each, with robots checked first) a first-party copy of the 2023 amendment and of the 2023 annual report front page (effective date), and ideally an earlier filing printing the old English name. Hash, add to the manifest.
- S2. Rewrite the P1 gloss so the Chinese company name is shown as the Chinese string with a bracketed translation, never as if the filing printed the English name; keep `IEIT SYSTEMS Co., Ltd.` the only English name attributed to P1/P2.
- S3. Add the S1 finding that szse.io's machine-translated summaries use `Inspur Electronic Information Industry Co., Ltd.` for the issuer in 2026 items: the old string is also a translation of the Chinese name, so only P3 (and any first-party copy) counts for old-name usage.
- S4. Fix small facts: Sina's bulletin date is 2023-08-26 (signed 2023-08-25); P4 source is 上海证券报; P3 locator should be the 第四条 table row, not "line 257".
- S5. P4 linkage can be stated as reasoned (P3 names the meeting; item 3 is the only charter item) rather than merely inferred; still corroboration only.

### note
- N1. P2's cover is dated December 2025 only; the name pairing does not depend on adoption status.
- N2. The 7 searches are not hit-counted in the cap and no snippet is used as evidence; consistent with the policy.
- N3. Everything here keeps the evidence policy's role separation: company-role/name claim only, no manufacture, brand or deployment assertion; no `sources/` or database touched.
