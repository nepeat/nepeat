# t1 trade documentation qualification

consulted 2026-10-04. author: `campaign_trade_feasibility`; acceptance belongs to
`/root` and an independent reviewer. scope: exactly UN Comtrade, US Census
international trade API and ImportYeti. this is documentation research, with
zero API/data calls, shipment downloads, company queries, accounts or credentials.

**recommendation: no qualifying sustainable free row-level feed established in
this finite check.** census is aggregate-only context. comtrade is also aggregate
context, with access/retention questions unresolved. importyeti is a commercial
third-party publisher whose possible shipment feed remains unqualified.
this is neither a global absence claim nor a claim that a key requirement means
payment. do not start t2 or design an adapter from this result.

| publisher | grain and source origin | identifiers / exact-model usefulness | access, retention and automation | dates / coverage / disposition |
| --- | --- | --- | --- | --- |
| UN Comtrade | the reviewed 2019 methodology describes country-reported commodity/partner statistics, annual/monthly merchandise data and annual services data; quantities can be estimated and classifications converted. | commodity, partner, transport and procedure classifications are statistical dimensions. they do not identify a shipment, party or device model. | the homepage open returned no extracted body; its free/API marketing was a search lead. the opened agreement dated **2012-08-07**, section 5, restricts copying and automated browsing/downloading without permission. current applicability is unverified; acquisition qualification stops here. | reporting and optional fields vary by country. earliest dates, current release coverage, key requirements and current free quotas remain unknown. **aggregate-only context; automated/local reusable feed not qualified.** |
| US Census international trade API | official statistical publisher. the trade page describes compilation from EEI/ACE and import documentation. published examples expose commodity/country/district value totals. | HS, NAICS, end-use and related classifications support statistical context. no model, named company or bill-of-lading evidence was established. | the current trade page explicitly requires a key for **all data queries**. no key was sought. a key is not a paywall; a paid requirement and current free quota were not established here. terms allow API retrieval/analysis and require attribution, accurate representation and no identifying or linking respondents. | monthly January 2010–present; monthly press-release updates and annual revisions with April statistics are documented. latest observed dataset month is untested. **aggregate-only context; no row-level hardware feed.** |
| ImportYeti | provider's about page describes organizing public container-shipment trails; it is a third-party business, **not a regulator or government publisher**. original official records were not consulted. | the API search result describes BOL-number lookup, parties, ports and product descriptions. this is a documentation lead; the opened API page returned only metadata in the bounded tool output. row vs line-item structure, durable keys and actual model yield remain unvalidated. | FAQ answers were not exposed, including free searches/exports/API limits. the API search result describes accounts, keys, data credits and paid calls; trial/free credits remain unknown. free manual UI permission does not establish a permitted sustainable automated feed or reusable local dataset. | search-extracted API docs suggest US/Mexico imports and exports, not independently body-validated. historical span, delays, update frequency, retention rights and original-record completeness remain unknown. **unqualified row-level candidate.** |

citations for the table:
[comtrade 2019 methodology, pp. 2–6](https://comtrade.un.org/data/MethodologyGuideforComtradePlus.pdf);
[comtrade dated usage agreement, section 5](https://comtrade.un.org/licenseagreement.html);
[census current trade documentation](https://www.census.gov/data/developers/data-sets/international-trade.html);
[census API terms, use/attribution/limits](https://www.census.gov/data/developers/about/terms-of-service.html);
[importyeti about](https://www.importyeti.com/about);
[importyeti FAQ questions](https://www.importyeti.com/faqs);
[importyeti API documentation lead](https://docs.importyeti.com/docs/getting-started).

## consulted set and proof limits

all URLs below were attempted on the consulted date. successful documentation
extraction is distinct from validated API availability, entitlement or data access.
saved `*.tool-response.json` files preserve **derived web-tool representations**,
not original HTTP bodies; no HTML/PDF-original SHA-256 is claimed.
[request ledger](request-budget.json) records the exact requests, outcomes and
returned-content byte accounting; [qualification](qualification.json) is the
machine-readable decision; [artifact hashes](artifact-hashes.json) binds local files.

| URL | outcome |
| --- | --- |
| [comtrade homepage](https://comtrade.un.org/) | open returned zero extracted lines; cached search text remains a lead |
| [comtrade agreement](https://comtrade.un.org/licenseagreement.html) | body reviewed; prominently dated 2012, current applicability unknown |
| [comtrade methodology](https://comtrade.un.org/data/MethodologyGuideforComtradePlus.pdf) | partial extracted text reviewed; 2019 guide, 14-page PDF reported; no original downloaded |
| [census trade page](https://www.census.gov/data/developers/data-sets/international-trade.html) | body reviewed; revised 2026-05-20 |
| [census trade guide](https://www.census.gov/foreign-trade/reference/guides/Guide_to_International_Trade_Datasets.pdf) | PDF metadata only in returned open output; 27 pages reported; body not independently reviewed |
| [census query limits](https://www.census.gov/data/developers/guidance/api-user-guide.Query_Limits.html) | metadata only in returned open output; current numerical limits not established |
| [census terms](https://www.census.gov/data/developers/about/terms-of-service.html) | first click metadata-only, second open body reviewed; revised 2026-04-14 |
| [importyeti FAQ](https://www.importyeti.com/faqs) | questions rendered, answers unavailable in extracted representation |
| [importyeti API getting started](https://docs.importyeti.com/docs/getting-started) | search-extracted documentation text only; opened body not exposed in returned output |
| [importyeti pricing](https://www.importyeti.com/pricing) | tool reported 404; unavailable, not empty or free |
| [importyeti about](https://www.importyeti.com/about) | body reviewed; provider origin story, no original-regulator records validated |

## restrictions and stop

preflight searched saved research/docs. the saved
[Eximpedia 402](../../../bytedance-inspur-2026-10-04.md) remains blocked, including
alternate routes to that same company item. it was not queried or opened.
Datadory's paid path stays closed. no prior failure for the three named publishers
was found in that search; this does not prove unrestricted access.

budget exhausted at **3 search queries / 12 page-open attempts** (11 distinct
URLs, including the second census-terms view), across four web-tool invocations.
returned JSON representations totaled **109,123 UTF-8 bytes**, below the pre-pinned
200,000-byte cap. the four saved files total 109,127 bytes: each adds one trailing
newline to its serialized response, accounting for the four-byte difference.
upstream HTTP bytes are unavailable to this tool and unknown;
no direct downloads were made. the initial search response was saved from session
memory after a local UTF-8 helper error; no search was repeated.

no more comtrade requests followed observation of its restrictive dated agreement.
the agreement and methodology were requested together; the restriction became
visible after that batch completed. no pricing retry or alternate pricing route
followed the importyeti 404. search results outside the named primary/provider
documentation were ignored.

unmet questions: current comtrade permissions and limits; census current cost/quota
and key authorization if aggregate context ever becomes useful; importyeti current
free manual/export allowances, free or trial API credits, automated/local retention
rights, row/line-item keys, coverage and actual exact-model yield. answering these
needs a separately assigned finite documentation check. a shipment sample would
need a positive free-access/retention qualification first. no commercial party name,
commodity category or consignee could establish manufacture, operation, deployment
or the origin of an auction unit.
