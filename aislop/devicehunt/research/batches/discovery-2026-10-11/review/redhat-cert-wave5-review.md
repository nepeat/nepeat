# redhat_certified wave 5 supplement independent review (Sonnet 5.5), 2026-10-11

Reviewer did not author the handoff. Scratch: `.../scratchpad/review-redhat/` (`v5.py` verifier, `impE.py`, `before5.sqlite`, `E.sqlite`). Nothing under `data/` or `research/` changed except this file. Live `data/devices.sqlite` sha256 `ab8ed2e3...1b78` before and after. Supplement sha256 `33cc6cc23569c6489a4a696a923ea31fe8ae9dfdfb944c7a9a8f909ee6d5745b` confirmed; wave-5 manifest `c4d14e83...8d69f` confirmed.

## Verdict: ACCEPT

No must-fix. No should-fix. Notes only.

## Must-fix

None.

## Notes

- **N1. Wording of coverage.** Every supplement source says it supplements `redhat-cert-<id>` ("listing record, no detail page captured then"), carries only detail-page facts, and that listing facts stay with the listing source. Accurate. Because the supplement attaches a second source to each device, `redhat_cert_catalog_id`, `product_name_as_printed` and `detail_company_name` appear on two records per device; they are record-scoped and equal, so no conflict.
- **N2. 21 Inspur Power listing-only products still have no detail page.** Declared in the handoff as outside this wave's list. Not a defect.
- **N3. Sentry key.** The page config again contains the public front-end error-reporting address `8184a196...@sentry.io` in every HTML capture. Third-party boilerplate, not personal data; not in the supplement.
- **N4. Max fact size 1,245 bytes** (the capped certification list), well under 4 KB. Cert lists are capped in text but `certification_count` is the page's real count in all 158.

## Verified

**Capture and request log.**
- 161/161 wave-5 files re-hash to the manifest sha256 and bytes; no extra or missing files on disk (set of ids equals the 161 listing-only Quanta 53 + Inspur 67 + IEIT 41 products; no overlap with the 160 run-1 detail ids).
- 161 distinct URLs, all HTTP 200, `curl_err` null on every row, 0 403/429/5xx, no URL repeated (also none repeated across the two manifests). UTC strictly increasing, gaps min 2.0 s, max 17 s, 0 under 2 s. First wave-5 request is 3,624 s after the last run-1 request. Total requests 195 + 161 = 356, as stated. UA unchanged. Robots reused from run 1 (`Allow: /`), so the detail path is permitted.

**Facts (4,197 checks, 0 failures).** For all 158 supplement devices against the new HTML:
- `companyName` and `"name"` literal in the embedded data and equal to the accepted candidate's `partner_name_as_printed` / `product_name_as_printed`.
- Every certification row (`cert id|product|start-end|arch|publishedOn|level`): `"id":N,"product":...`, `architecture`, `certificationLevel` and non-null `publishedOn` all present in the page; `certification_count` equals the page's certification entry count and is at least the number of listed rows.
- Form factors literal; every CPU certification name present; detail-file path and sha256 match the file.
- 0 `None` values.
- Example, id 2065 `QSSC S4R` (Quanta): HTTP 200, `"id":"2065","name":"QSSC S4R","type":"System","formFactors":["Server"],"platforms":["Red Hat Enterprise Linux"]`.

**Identity reuse, 0 new devices.** Each supplement device equals the accepted candidate's record for the same catalog id in brand, model, title, kind, affiliations and identifiers (0 differences over all six fields). Each brand+model exists in the promoted DB, and each `redhat-cert-<id>` listing source is present there. Brands: Inspur 67, Quanta 50, IEIT SYSTEMS Co.,Ltd. 41 (IEIT unmapped, as before).

**Facts are detail-only and source-scoped.** Fact keys are limited to `catalog_id, supplements_source, product_name_as_printed, detail_form_factors, detail_company_name, detail_platforms, certifications, certification_count, cpu_certifications, detail_file`; `supplements_source` points at the matching listing source for all 158; no listing field (make, model field, category, listing dates) is repeated.

**The 3 empty-data Quanta pages (6378, 6548, 6592).** HTTP 200 and byte-identical size (180,373) shells whose data block reads `"id":"$undefined","name":"","formFactors":[]`, no certifications. No supplement source or device exists for them; their listing-only sources are unchanged (their listing docs also lack certification level/version/arch). Handled honestly: absent, not zero.

**Owned-model check** (case-insensitive over the 161 pages, supplement and manifest; CS8200, CS8210, AZ4444, 7760/AS7760, DMC-12, AS7772, SV1120/1121/2121/2221, ByteDance, B4020, B4010): 0 hits. **No personal data:** no emails or IPs in the supplement; the only address in the pages is N3.

**Temp import** (scratch copy of the current DB, `pipeline.import_source('curated')`):
- Returned 158. sources 1477 to 1635 (+158), devices 5015 to 5015 (**0 new**), records 6626 to 6784 (+158), identifiers 6175 to 6333 (+158), facts 63703 to 65190 (+1487), affiliations 9991 to 10149 (+158). `integrity_check` ok, `foreign_key_check` empty.
- All rows of sources, devices, records, identifiers, facts, affiliations present and unchanged (EXCEPT count 0 for each).
- vendor-inventory unique identities before to after: Quanta all 170 to 170, manufacturer 41 to 41; Inspur all 220 to 220, manufacturer 3 to 3. Manufacturer scope unchanged. All counts match the handoff.
