# wifi_alliance handoff — 2026-10-11 (Claude Sonnet 5.5)

## Outcome: STOPPED, no certification data captured, no candidate
Wi-Fi Alliance Product Finder has no server-rendered results. `https://www.wi-fi.org/product-finder?keywords=<term>` returns a React shell
(`/modules/productfinder/react/dist/productfinder-app.js`); results come only from a JSON API whose base URL is in the page's `drupalSettings.productFinder.certApiUrl`
= `https://prf.cert.wi-fi.org/` (endpoints in the app JS: `products/view/filtered?keywords=&companies=&categories=&start=&items=30&sort_by=certified&sort_order=desc`, `filters/companies/keywords`).
That host's `robots.txt` (HTTP 200, 39 bytes) is exactly `User-agent: *` / `Disallow: /products/view/`, which covers the results API. I obeyed it: **0 requests to prf.cert.wi-fi.org other than robots.txt**,
no workaround (no detail-page ID guessing, search engines or alternate hosts). The API itself needs no token, so this is a robots restriction, not a JS-token problem.
Detail/certificate pages (`product_url` / `certificate` links) are only exposed inside API items, so they are not discoverable without the disallowed API.
Request count: **5 of 150** (www robots 200; a wrong-guess `/product-finder-results` URL -> 301 to `/product-finder`, recorded not a restriction; product-finder page 200; app JS 200; prf robots 200). Gaps >= 3 s, one plain UA, no 403/429/5xx.
`www.wi-fi.org/robots.txt` saved (200): disallows `/search/`, `/search?`, admin/user paths; `/product-finder` is allowed.

## Files and hashes
`data/raw/discovery-2026-10-11-wifi-alliance/`: `robots/*.txt` (2), `pages/product-finder-search.html` (301 stub), `pages/pf-q-Accton.html`, `pages/productfinder-app.js`,
`capture-manifest.json` sha256 `30890ba18b84579404337d71ff7c1c3fcc033d30dfdc67b5d4519c41040a52f0` (url, status, UTC, sha256, bytes per file). Scripts in scratchpad `wifi-alliance/` (`get.sh`, `man.py`, `comp.py`, `js.py`). No curated candidate, no import.

## Identities/facts
None. Decision on one-identity-per-model vs per-cert is therefore moot; the proposed convention if the data is later obtained legitimately: one identity per printed model number, source `wfa-<cert id>`, facts `wfa_*`, identifier `wifi_alliance_model`, brand = canonical target (`Accton`, `Edge-Core`) when the printed company is a `TARGETS` alias, else as printed (`IgniteNet` is not in TARGETS; catalog already has brand `IgniteNet`, 24 devices), one affiliation `[company as printed, listing_holder]`, tier `certification_body`.

## Leads (from the page already captured; no extra request)
The page's embedded company list (919 companies, in `pf-q-Accton.html` drupal-settings JSON) has Wi-Fi Alliance company ids: `208` Accton, `2981` Edgecore Networks Corporation, `2983` IgniteNet (no "Accton Technology" or "Edge-Core" entries). Usable only if the Alliance permits API access (e.g. a written OK, or a robots change).
Existing catalog APs (read-only `data/devices.sqlite`): brand `Edge-corE` (EAP102, EAP111, ECW5110/5211/5410/7210, ECWO5211, OAP100, kind `access point`), `Accton` (MR3201A, WA3101/4101/5001/6101), `IgniteNet` ML-60 family; plus many FCC/Anatel/RRA rows under `Edge-Core`/`Accton`/`Edgecore` (e.g. EAP101/102/104, ECW5211-L, EAP111e, OAP6626A). Note existing brand spellings vary in case (`Edge-corE`, `Edge-CorE`, `Edgecore`): exact brand+model merges are case-sensitive, so a future import using canonical `Edge-Core` would not merge with `Edge-corE` rows (lead only).

## Excluded / gaps
No certification, company-as-printed, model, date or category data was obtained; absence is not zero. Alternative public routes (third-party mirrors, search engines, guessing detail URLs) were deliberately not used.
**Owned-model check** (case-insensitive over all 6 files in the raw dir: CS8200, CS8210, AZ4444, 7760/AS7760, DMC-12, AS7772, SV1120, SV1121, SV2121, SV2221, ByteDance, B4020, B4010): ZERO hits.

## Checks
Manifest 5 rows match the files on disk (bytes/sha256 recorded at fetch). No temp import (nothing to import; vendor-inventory unchanged by definition, manufacturer unchanged). `data/devices.sqlite`, staged candidate sqlite and import manifest untouched.

## Decisions needed
Whether to seek permission for the API (the only route), or drop this source. Next step if allowed: query `products/view/filtered?companies=208,2981,2983&sort_by=certified&sort_order=desc&start=N&items=30` (~ a few dozen requests) then detail pages.
