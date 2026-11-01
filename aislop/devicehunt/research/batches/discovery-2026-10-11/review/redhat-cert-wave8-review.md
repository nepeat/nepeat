# redhat_certified wave 8 supplement independent review (Sonnet 5.5), 2026-10-11

Reviewer did not author the handoff. Scratch: `.../scratchpad/review-redhat/` (`v8.py`, `impF.py`, `before8.sqlite`, `F.sqlite`). Nothing under `data/` or `research/` changed except this file. Live `data/devices.sqlite` sha256 `baa016d2...b2184` before and after. Supplement sha256 `32d077a6dc134d444dff826407c7c54dc4522c5f7f97b8f6a56b888857793ceb` and manifest `41d63fee...6654` confirmed.

## Verdict: ACCEPT

No must-fix, no should-fix.

## Must-fix

None.

## Notes

- **N1.** Inspur Power Commercial Systems is not a target alias, so no target inventory moves (verified). The supplement keeps the unmapped brand; no mapping to Inspur is implied.
- **N2.** With this wave every System listing has a detail capture except the 3 empty-data Quanta pages (6378, 6548, 6592) and `5864` (render error), as the handoff says.
- **N3.** Pages again carry the public front-end Sentry address in site config; not personal data and not in the supplement.

## Verified

- **Captures.** 21/21 files re-hash to the manifest sha256 and bytes; the 21 files on disk equal the manifest; the ids equal exactly the 21 Inspur Power listing-only products of the promoted candidate. All 21 HTTP 200, `curl_err` null, 21 distinct URLs, none repeated from the two earlier manifests (total 195 + 161 + 21 = 377). UTC strictly increasing, gaps min 3.0 s, max 13 s. Wave 8 starts about 43 minutes after the last wave-5 request. Robots reused (`Allow: /`).
- **Facts: 481 checks, 0 failures** over the 21 devices against the HTML: `companyName`/`name` literal and equal to the promoted facts, every certification row (id, product, architecture, level, non-null `publishedOn`) present, `certification_count` equals the page's entry count, CPU names present, form factors literal, detail-file hash correct, 0 `None` values, max fact 521 bytes.
- **Identity reuse, 0 new devices.** Brand `Inspur Power Commercial Systems Co., Ltd.`, model, title, kind, `listing_holder` affiliation and `redhat_certified_product` identifier equal the accepted candidate's record for the same catalog id in all 21; every brand+model exists in the current DB and each `redhat-cert-<id>` listing source is present.
- **Scoping.** Fact keys limited to the detail-only set; `supplements_source` points at the listing source; coverage names the supplemented source and says listing facts stay there.
- **Owned-model check** (pages, supplement, manifest): 0 hits. No emails or IP addresses in the supplement.
- **Temp import** (scratch copy of the current DB, `pipeline.import_source('curated')`): returned 21. sources 1672 to 1693, devices 5042 to 5042 (0 new), records 6821 to 6842, identifiers 6429 to 6450, facts 65930 to 66119 (+189), affiliations 10178 to 10199. `integrity_check` ok, `foreign_key_check` empty, all old rows in the six tables unchanged (EXCEPT count 0). vendor-inventory before to after: Quanta all 175 / manufacturer 41, Inspur 221 / 3, Oracle 1281 / 859, identical. Counts match the handoff.
