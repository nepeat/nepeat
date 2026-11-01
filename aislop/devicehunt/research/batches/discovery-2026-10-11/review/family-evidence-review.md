# family_evidence independent evidence review (Sonnet 5.5), 2026-10-11

Reviewer did not author the handoff. Offline only. Scratch: `.../scratchpad/review-family/` (extracted text). PDFs via the existing nix-store `pdftotext` (poppler-utils 26.06.0, `-layout`); HTML decoded with stdlib `html.parser` under `python3 -I`. Nothing installed, nothing fetched.

## Verdict: ACCEPT with notes (0 must-fix; 3 should-fix wording/disclosure items; core findings hold)

Both headline findings verify against the saved captures. (1) QCT's site says Quanta Computer is the parent and that QCT launched as its subsidiary; Quanta Computer's own "Summaries of Affiliated Companies" (December 31, 2025) lists `Quanta Cloud Technology Inc. (QCTTW)` with Quanta Computer Inc. holding 48,200 thousand shares = 100.00%. (2) The FY2024 000977 annual report prints 浪潮（北京）电子信息产业有限公司 as a directly 100%-held subsidiary and contains zero occurrences of `Inspur`; the 2025 BIS rule is a government corroboration only; no source names `Inspur Corporation` or `(IEI)`. The handoff discloses nearly every limit itself. The should-fix items are where its labels run slightly ahead of the evidence.

## 1. Capture and request log

- Hashes: all 10 file captures in `capture-manifest.json` re-hash to the recorded sha256 and byte count (0 mismatches). Three requests (en.inspur.com, www.inspursystems.com, www.businesswire.com robots) have no file, status 0, curl error recorded. The three reused prior captures (`ar2024-1222950880.pdf`, `quanta-affiliates-2025.pdf`, `qct-qxvdi-2020.pdf`) also match the sha256 values the handoff cites. `manifest.tsv` statuses and URLs match the JSON.
- Count and spacing: 13 requests (cap 30). Gaps between consecutive requests (UTC, 15:23:32 to 15:26:11): 5, 9, 5, 19, 5, 43, 5, 4, 4, 13, 12, 35 s. Minimum 4 s (handoff says ">= 4 s", correct), so >= 3 s everywhere. WebSearch queries are not counted in the cap; they are listed and no snippet is used as evidence.
- Robots: www.qct.io robots (req 1) precedes both QCT page fetches; www.quantatw.com robots (req 2) precedes the subsidiary page and the PDF; govinfo robots (9) precedes the FR page (11); spec.org robots (10) precedes the SPEC page (12). Content of the saved robots files agrees with the handoff (qct.io disallows `/solution/*`, `/*.pdf$` etc., not the About pages; quantatw.com blocks only MJ12bot/SemrushBot; spec.org Crawl-delay 10; govinfo Drupal rules, nothing relevant). The qct.io PDF-disallow does not touch the quantatw.com-hosted PDF.
- Failures: the three robots failures are recorded once each, hosts dropped, no retry, no reroute. The quantatw.com QCT subsidiary page returned HTTP 200 but is a 1,520-byte bot-protection stub (title `Quanta`, `__uzdbm` script, empty body); the handoff records that it was not read, not retried, and the search snippet not used. Correct handling.
- No URL requested twice; no login.

Request-log notes (see S1, S3): robots was saved for `www.qct.io` only, but request 3 went to the apex `qct.io`; and the handoff's "spec.org 11 s" is not what the log shows.

## 2. Quote, location and gloss verification

### Relation 1 (QCT <-> Quanta Computer)

| item | result |
| --- | --- |
| Q1 `qct-company-aboutqct.html` (title `Overview - About \| QCT- Quanta Cloud Technology`): `The parent company of QCT is Quanta Computer, Inc., a Fortune Global 500 corporation.` | exact (intro paragraph). |
| Q1 `Parent Company` heading: `The QCT parent company is Quanta Computer Inc. (Quanta), a Fortune Global 500 company with about 9,000 employees ...` | exact. |
| Q1 `History`: `... in May of 2012 QCT was launched under the name Quanta Cloud Technology as a subsidiary of Quanta Computer Inc. to offer ...` | exact; same sentence in both captures. |
| Q1 footer: `QCT, the QCT logo, Rackgo, Quanta, and the Quanta logo are trademarks or registered trademarks of Quanta Computer Inc.`; footers `(c) 2025` (www.qct.io) and `(c) 2022` (en-US) | exact. |
| Q1 en-US variant: `The parent of QCT is Quanta Computer Inc., a Fortune Global 500 technology engineering and manufacturing company.` | exact. |
| Q2 `quanta-affiliates-2026-organize26en.pdf` (20 pp.), cover `QUANTA COMPUTER INC. Summaries of Affiliated Companies / December 31, 2025` | exact. |
| Q2 PDF p.5, section "2. Summaries of Affiliated Companies", `Unit: thousands of dollars`: `Quanta Cloud Technology Inc. (QCTTW)  02/08/2007  No. 188, Wenhua 2nd Rd., Guishan Dist., Taoyuan City, Taiwan  NT$ 482,000  Sale of computers and peripherals` | exact; page correct. |
| Q2 PDF p.12, section "5. Directors and Presidents for Affiliated Companies", `(thousand shares)`: `Quanta Cloud Technology Inc. (QCTTW)  Quanta Computer Inc.  48,200  100.00%`, chairman Barry Lam | exact; page correct. The chart on p.2 also places the entity under Quanta Computer Inc. and p.4 notes `ownership is 100% unless otherwise specified`. |
| Q2 PDF p.11, section "4. Principle Activities": QCT-USA, QCT Korea Inc., QCTTW, QCJK, Cloud-Tech (Chongqing), QCTJ, Germany GmbH, Singapore Pte under "Trade and sales of electronic products" | exact (the handoff's "p.11" is right). |
| Q2 prior edition `quanta-affiliates-2025.pdf`: same QCTTW row, address `Dist., Taipei City, Taiwan`, capital 482,000, 48,200 / 100.00% | confirmed. |
| Q3 `qct-qxvdi-2020.pdf` PDF p.40: `The parent of QCT is Quanta Computer Inc., a Fortune Global 500 technology engineering and manufacturing company.` | exact. |
| Q3 `qct-product-portfolio-v14-2023.pdf`: `QCT is Quanta Computer's cloud computing division and a global data center solution provider.` | exact (text line 55). |
| Q4 stub page | matches the handoff description. |

Gloss check. The relation statements are precise. Q1 uses "parent"/"subsidiary" for QCT as a business and gives no percentage; Q2 says "affiliated company" and gives the 100.00% direct holding for the Taiwan legal entity only; the handoff says both. It correctly notes that "Quanta Cloud Technology" is a name shared by a family of legal entities and that no source ties a SPEC/Red Hat/Ubuntu/catalog row to a specific one. I added one fact the handoff leaves implicit: Q2 shows the holding path differs by entity (QCTTW and QCTS are held directly by Quanta Computer Inc., QCT-USA by QIL, QCT Korea by QCT Inc., Germany and Japan Inc. through other holders, and the QCTBJ row via QAL). So "Quanta Computer Inc. holds 100%" is true of QCTTW, not of every QCT-named entity.

### Relation 2 (Inspur strings)

| item | result |
| --- | --- |
| I1 `ar2024-1222950880.pdf` PDF p.21, 九、主要控股参股公司分析 (unit 万元): first row `浪潮（北京）电子信息产业有限公司`, 公司类型 `子公司`, main business `计算机软硬件及辅助设备的销售`, registered capital `5,250 万元` | exact. |
| I1 PDF p.158, 十、在其他主体中的权益 / 1、在子公司中的权益 / （1）主要子公司 (unit 元): `浪潮（北京）电子信息产业有限公司 52,500,000.00 北京 北京 计算机软硬件及辅助设备的销售 100.00% 0.00% 合并` (name wraps over several lines in the PDF) | exact; the handoff's 直接 100.00% / 间接 0.00% gloss is right. |
| I1 PDF p.111: `本公司之子公司浪潮（北京）电子信息产业有限公司，2023 年通过高新技术企业资格复审 ...` | exact. |
| I1 "no `Inspur` anywhere in the report" | confirmed: 0 occurrences in the full extracted text. |
| I2 `fr-2025-03-28-2025-05427.html`: FR Vol. 90 No. 59, pp. 14046-14052, FR Doc 2025-05427, effective March 25, 2025; summary sentence naming the six entities and "subsidiaries of ... Inspur Group, which was added to the Entity List on March 2, 2023" | exact. |
| I2 entries: `Inspur (Beijing) Electronic Information Industry Co., Ltd.` with four aliases (Langchao Beijing ...; Inspur Beijing Electronic Information Technology; Inspur Beijing Company; Langchao (Beijing) Electronic Information Industry Co., Ltd.); `Inspur Electronic Information Industry Co., Ltd.` with eight aliases (`Inspur Information`; `IEIT Systems Co. Ltd.`; Incumber; Incubic; Incorphor; Wave; Chao Chao; `IEII`) and Jinan addresses | exact. The `Inspur Electronics Information Industry Co., Ltd.` spelling occurs once in the amendatory text. The govinfo HTML still prints `90 FR [INSERT FR PAGE NUMBER` in the table, as stated. No `Inspur Corporation` or `(IEI)` anywhere in the rule. |
| I3 `spec-cpu2017-20190723-16416.html`: title `CPU2017 Floating Point Rate Result: Inspur Corporation Inspur NF5280M5 (Intel Xeon Gold 6136)`; `Test Sponsor: Inspur Corporation`; `Tested by: Inspur Corporation` | exact; page states no legal form, address or parent. |

## 3. Tiers and relation wording

- Q1 first-party (QCT's own site) and Q2 first-party (Quanta Computer's own site): tiers right. I1 is the issuer's own report, first-party. I2 is correctly tiered as a government record of a third party's names, with the handoff stating "corroboration, not first-party" for the alias tie. I3 is a first-hand printed string with no relation content, and the handoff says so.
- Precision is good: subsidiary vs affiliated vs brand is kept apart; `Inspur (Beijing)` is stated as a separate legal entity (a subsidiary) and "do not merge or alias"; no manufacturer claim is made; the QCT-row-to-legal-entity link and the Quanta-branded Red Hat/Ubuntu inference are explicitly "not established" / "not shown".
- The 浪潮（北京） to `Inspur (Beijing) ...` English match is described as the handoff author's transliteration. It can be made firmer from material already in hand: the BIS entry lists the alias `Langchao (Beijing) Electronic Information Industry Co., Ltd.`, which is the pinyin of the exact Chinese registered name in I1. Still a government alias, not a first-party English name, so the handoff's "flagged as not first-party English" stands.

## 4. Does each relation meet the proposal's bar (primary statement, typed relation, date, evidence url/sha)?

- QCT / Quanta Cloud Technology -> Quanta Computer Inc., `subsidiary_brand`: yes. Q2 gives a dated (2025-12-31) first-party ownership statement with hashes; Q1 gives the first-party parent/subsidiary/trademark statements (undated); Q3 is the older corroboration. `abbreviation` between `QCT` and `Quanta Cloud Technology`: meets in substance (Q1 title, Q1 History, Q3 `Quanta Cloud Technology (QCT)`).
- `Inspur (Beijing) ...` ~ 000977: handoff's conclusion (different entity, typed `subsidiary` at most) is supported.
- `Inspur Corporation`, `(IEI)`: no source; "do not map" is right. Absence is a search result, not proof, and the handoff says that (`www.inspur.com` / `en.inspur.com` unreachable).
- `Inspur Electronic Information Industry Co., Ltd.` ~ `IEIT SYSTEMS Co., Ltd.`: BIS is only corroboration; the table row correctly defers to the ieit-source evidence and its provisional old-name leg.

## Findings

### Must-fix
None.

### Should-fix
- S1. Q1 is labelled "current first-party evidence" and the bottom line says "no need to wait for a newer QCT page". The two QCT pages are undated, carry footers 2022 and 2025, and the body text is visibly old-copy: both say QCT is "Backed by Quanta, with more than 14-years of experience" (a 2000 start plus 14 gives about 2014) and the History paragraph says the same. The pages are first-party and consistent with Q2, but "current" should be dropped or qualified for Q1; make Q2 (dated 2025-12-31) the dating anchor and record Q1 as "undated, copy of unknown age, served 2026-10-11". The conclusion for decision 2 survives because Q2 is current and dated.
- S2. The "subsidiary" label rests on two different words: Q1 says "subsidiary"; Q2 says "affiliated company" plus the 100.00% direct holding. State in the suggested row that the 100% direct holding is of `Quanta Cloud Technology Inc.` (QCTTW) only, and that other QCT-named entities are held via other holders in Q2 (see section 2). Relation `subsidiary_brand` should be recorded between the brand strings and Quanta Computer Inc. (the group), not as an assertion about any single catalog row's legal entity.
- S3. Request-log wording: (a) robots was saved for `www.qct.io`, but request 3 fetched the apex `qct.io` (no redirect, `effective_url` unchanged); the apex origin has no robots capture of its own. The content fetched is not covered by any Disallow in the saved file, so no harm, but "robots saved before the first fetch for every host" should say the apex was treated as the same site. (b) "spec.org 11 s" does not match the log: spec robots (15:25:11) to the SPEC page (15:25:36) is 25 s (12 s after the preceding govinfo request). Compliant either way; fix the number.

### Notes
- N1. Q2's wording "ownership is 100% unless otherwise specified" (p.4 chart note) and the p.12 share table are consistent; paid-in capital / par arithmetic remains the handoff author's, as it says.
- N2. I2's "six subsidiaries of Inspur Group" is group-level and does not conflict with I1's direct 100% holding by 000977; the handoff says this. The rule's own entity count is inconsistent (summary names six, the list shows five China + Taiwan); immaterial.
- N3. The claim that the FY2024 principal-subsidiary table lists only five Chinese subsidiaries (I4) and the 36kr 59.86% figure were not re-checked; neither is used for any mapping.
- N4. Hash and file-name hygiene is good; the failed-capture records carry curl errors in the manifest.

## What the evidence supports for proposal decisions 2-4

Decision 2 (QCT group): yes. QCT and Quanta Cloud Technology can be recorded as typed `abbreviation` strings, both `subsidiary_brand` of Quanta Computer Inc., on Q2 (dated first-party ownership, 100.00% direct for the Taiwan entity) plus Q1 (first-party parent/subsidiary/trademark text); there is no need to wait for a newer QCT page, provided the row is a display-only group that makes no claim about which QCT legal entity a catalog row means and no manufacturer claim, and the Quanta-branded Red Hat/Ubuntu rows remain an unproven inference from shared model strings. Decision 3 (Sun -> Oracle) is outside this batch. Decision 4 (IEIT <-> Inspur): this batch adds only government corroboration (BIS lists `Inspur Information` and `IEIT Systems Co. Ltd.` as aliases of the entry `Inspur Electronic Information Industry Co., Ltd.`, in Jinan) and does not remove the earlier gap that the old English name rests on a mirror copy of the 2023 amendment; the legal-entity family row `IEIT SYSTEMS Co., Ltd.` <-> `Inspur Electronic Information Industry Co., Ltd.` therefore still stands on the ieit-source evidence with a government cross-check, while `Inspur Corporation` and `...(IEI)` must stay unmapped and `Inspur(Beijing) Electronic Information Industry Co., Ltd.` must be kept out of the row as a separate 100%-held subsidiary of 000977.
