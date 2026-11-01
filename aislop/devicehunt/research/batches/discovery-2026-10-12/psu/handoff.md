# exact PSU vendor discovery — 2026-10-12

outcome: complete bounded discovery; three exact models have useful primary vendor
evidence, two custom-suffix models remain unresolved. no catalog imports or
shared-state writes. this directory is discovery evidence, not an accepted PDF
text bundle.

## identities and practical findings

| exact existing string | supported result and source location | limits |
| --- | --- | --- |
| `PFE1100-12-054NA` | Bel series HTML JSON-LD explicitly includes this exact SKU/MPN with brand “Bel Fuse”. `bel-pfe1100-series.pdf`, revision `BCD.00012_AP`, p. 1: 1100 W class, 90–264 VAC, 12 V main output, programmable 3.3/5 V standby; p. 5: main nominal power **1080 W**, 90 A; p. 9 names this exact NA model's input/temperature derating chart. | brand evidence, no separately captured manufacturer certificate. preserve 1100 W class versus 1080 W main rating. chart curves/current-limit columns need visual QA before numerical low-line assertions. |
| `PFE1500-12-054NAC` | `bel-pfe1500-series.pdf`, `BCD.00733_J`, p. 2 explicitly orders this exact code: C16 AC socket, 90–264 VAC / 180–350 VDC input; normal airflow is output connector → input socket. `bel-pfe1500-nemko.pdf` p. 1 explicitly lists this model, manufacturer **Bel Fuse Inc.**, applicant Bel Fuse Inc., factory **BPS Asia Pacific Electronics (Shenzhen) Co., Ltd.**; main 12 V / 125 A and selectable standby 5 V / 3.3 A or 3.3 V / 5 A. | certificate issue date 07-01-2022, no claim of present certification status or identity of an auction unit's factory. factory and manufacturer roles remain separate. region/ambient/low-line limits below. |
| `FSP550-29FM` | `fsp550-29fm.pdf` p. 1, printed `2020.05 spec`: exact model, 550 W CRPS 2.0 module, PMBus 1.2, 90–264 VAC / 47–63 Hz input; +12 V main at 45.8 A maximum for 100–240 V, +12 V standby 2.1 A maximum; 185 × 73.5 × 39 mm, 0–55°C. primary FSP product page carries **FSP TECHNOLOGY INC.**, which is vendor/publisher context. | page 2 heading says **FSP550-20FM** despite the -29FM filename and page-1 identity; don't silently attach its mechanical/airflow drawing to -29FM. no exact per-model manufacturer certificate captured. |
| `D1U54P-W-1500-12-HC4TC-AF` | exact suffix searched in vendor and unrestricted queries; only base `D1U54P-W-1500-12-HC4TC` / family datasheets surfaced. | vendor PIM PDF returned **403**; no retry and no alternate copy fetch. base ratings cannot be transferred to the -AF model. |
| `D1U74T-W-2600-12-HB4C-QNT` | exact vendor search yielded no qualifying vendor document; broader exact-family search surfaced already-known SPEC configuration and a Murata brochure with **2700**, a different rating/model. | existing QCT SPEC test configuration remains a lead, not universal fitment or vendor electrical specification. no voltage/derating assertion accepted for this suffix. |

## PFE1500 low-line/ambient scope

`bel-pfe1500-csa.pdf` p. 4, issue 2022-01-10, explicitly lists NAC.
For Canada/US, its main-output current derates linearly across 100–200 VAC:
83.4–125 A at 45°C, 69–104 A at 60°C. At 200–240 VAC or 200–350 VDC:
125 A at 45°C, 104 A at 60°C. Outside Canada/US, the 100–200 VAC
row is 66.5–125 A at 45°C and 66.5–104 A at 60°C.
Preserve both regional scopes and temperatures; “1500 W at any mains voltage”
is unsupported. visual review of the table is still pending.

## conflicts, failures and exclusions

- FSP sheet p. 1 says MTBF 500,000 h at 55°C/100% load; the primary product
  page says 250K h at 45°C/75% load. keep different conditions and statements.
- Bel's old /resources/datasheets URL returned its HTML Datasheet Library.
  this was a redirect/content mismatch, not a denial. the actual permitted
  /media PDF URL came from the captured series HTML.
- Murata denied URL:
  https://pim.murata.com/asset/pim4/1UFrontEnd/D1U54P-W-1500-12-HXXTC_PDF_1UFRONTEND?lastModifiedDatetime=20250707193551
  returned 403; don't retry this item or retrieve its alternate copy.
- no model alias expansion, PSU↔chassis fitment, OEM/customer deployment,
  current support, availability, price, or standalone/wiring recommendation.
  software literals and parent benchmark configurations remain discovery context.

## artifacts, request limits and checks

- `fetch-report.json`: six successful raw GETs with exact URLs, UTC timestamps,
  content types, byte sizes and SHA-256: five PDFs and one series HTML.
- `web-captures.json` and `web-pfe1100-capture.json`: saved search/open tool
  responses; explicitly typed web extracts, abridged rather than full PDF text.
- `bel-pfe1100-html-jsonld.json`: stdlib extraction bound to HTML hash.
- `capture-manifest.json`: per-file capture hashes. raw PDFs have valid
  %PDF- headers; local hashes verified. no importer preparation attempted.
- eight searches; fifteen primary retrieval attempts (nine web opens, six
  raw GETs). additionally, three screenshot inspection requests against
  already-open PDFs: CSA p. 4 yielded no visible image in the flattened tool
  response; FSP p. 2 and Bel PFE1100 p. 9 timed out. these extra requests
  are disclosed separately; no retry. `screenshot-outcomes.json` preserves them.
- prior failure check: relevant research handoffs plus saved fetch/outcome/
  access/ledger JSONs and research/source-access were searched for the target
  vendor domains/models; no existing vendor-specific denial found.
- project python lacks pypdf and available PDF CLI extractors. no installs;
  no misleading “pypdf embedded text” envelope. no code tests needed for captures.

acceptance dependency: independent review of exact claims, especially PDF table
columns/graphs and manufacturer/factory roles. integration would require a
complete validated PDF text extraction using an already available authorized
runtime; keep these as useful discovery captures meanwhile.

next bounded step: review the three successful exact matches offline. don't
resume Murata requests or expand the source set from this handoff.
