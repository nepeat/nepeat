# sun6000 handoff — 2026-10-10 (worker sun6000, Claude Sonnet 5.5)

## Outcome

- Saved RRA pages for `SUN-6000` are byte-identical in both bundles; the record itself names the product
  "Sun Blade 6000 Chassis" (a chassis), so `6000` is the Sun Blade 6000 chassis per the RRA, not a guess.
- Public Oracle docs (8 captures, all HTTP 200) confirm the "Sun Blade 6000 Modular System" exists as an
  Oracle-hosted product library with a chassis service manual, site-planning pages and chassis/PSU/CMM/midplane
  part numbers. They say nothing about Celestica, Netra 6000, or "Chassis M2".
- A proposed curated input (not imported into the authoritative DB) validates in a temp catalog.
- Budget used: 4 web searches (cap 12), 8 captures (cap 8), 0 failures, no 401/402/403/429.

## Saved-evidence review (no network)

`data/raw/rra-celestica-official/detail-090.html` and `data/raw/rra-oracle-official/detail-215.html`:
SHA-256 `3c01977f51bc25c4865c1c7e59c4fe7024d6e7cb4c8a46f20ceee5aeedfa870b`, 4483 bytes each, identical.
Both match `rra_equipment` (detail_sha256 equal; rows 90 of `rra-official-query-41e4ca912b33`, 215 of
`rra-official-query-73009f906e4e`; source_url `https://www.rra.go.kr/ko/license/A_b_popup_keyno.do?key_no=SUN-6000`).
HTML `<title>`: `ORACLE AMERICA. INC | Sun Blade 6000 Chassis 인증현황` (translation: "certification status").

| Label (Korean) | Translation (mine) | Value, literal |
| --- | --- | --- |
| 상호 | company/applicant | `ORACLE AMERICA. INC` |
| 기기명칭 | equipment name | `Sun Blade 6000 Chassis` |
| 모델명 | model name | `6000` |
| 파생모델명 | derived model names | `Netra 6000 AC,Netra 6000 Chassis,Oracle Sun Blade 6000 Chassis M2,Sun Blade 6000 Chassis` |
| 인증번호 | certificate number | `SUN-6000` |
| 제조자 | manufacturer | `Celestica de Monterrey S.A. de C.V.,Celestica Kladno s.r.o.` + line break + `(Oracle America, Inc.)` |
| 제조국가 | country of manufacture | `멕시코` (= Mexico); a single country although two factories are named (Kladno is a Czech city; the page does not say so) |
| 인증연월일 | certification date | `2007-05-15` |
| 기타 | other | empty |

Table caption (translation): "certification status result provided as company, equipment name, model, derived model,
certificate number, manufacturer, country, certification date, other". The page also has an Excel-download link
(`excel_keyno.do?key_no=SUN-6000`), not followed.
Related certificates in `rra_equipment` (not re-read from HTML): `SUN-6000StorageM2` (row 165, "Blade Storage Module",
main model `6000StorageM2`, derived `Oracle Sun Blade 6000 Storage Module M2`, manufacturer `ORACLE AMERICA. Inc`, 미국,
2010-05-28) and `SUN-6000-Storage` (row 207, "Sun Blade 6000 Storage Module", `6000-STORAGE`, 2008-03-12, 미국).

Interpretation limits: the derived-model list is the RRA's; it makes `Netra 6000 AC/Chassis` and
`Oracle Sun Blade 6000 Chassis M2` RRA-asserted derived names of this certificate only. The parenthesised
"(Oracle America, Inc.)" is unlabelled in the cell; do not read it as a role.

## Files + hashes

Raw dir `data/raw/hunt-2026-10-10-sun6000/` (ledger `capture-ledger.json`, sha256 `2ecaf6a4...faa064`; ledger was rebuilt once
from the printed capture results after parallel curl calls raced on it; values are unchanged curl output):

| file | URL (docs.oracle.com/cd/E19938-01/...) | UTC | bytes | sha256 |
| --- | --- | --- | --- | --- |
| sbm6000-library-index.html | index.html | 10:38:52 | 9890 | c9745836b7f8dc5c4015e959868ee4c853dcd6872da2e2f56e1092baed016127 |
| sbm6000-product-notes-index.html | E23069/index.html | 10:39:03 | 33029 | f2c2b651e2556f79ab53f1b5f70aca101e0a8eb627fd66d3dfd82049ff991077 |
| sbm6000-installation-guide-index.html | 820-0050-11/index.html | 10:38:25 | 19838 | 5c4d02323cf5b3c1101bfb71568580747e083698eea5f3b04f97514cb9b3e817 |
| sbm6000-e20918-z40000472304.html | E20918/z40000472304.html | 10:38:25 | 21064 | b1eddfc4777cec98a5461f07a58063ccfa4b606b1f53843bf6a3a7cdd2f8d3c4 |
| sbm6000-service-manual-chassis-part-numbers.html | E20918/z400010c1426713.html | 10:38:25 | 20106 | 6183960f6792a5454c9f6e1f94f905a52bdee68110e04c8d14ea635fbd9c23dc |
| sbm6000-service-manual-replacement-components.html | E20918/z400001b1048274.html | 10:38:25 | 23119 | caa4f0ea5f023daa772524726e0d8bbccf1aac4e412577ed874a51cc3cca71a3 |
| sbm6000-site-planning-z40001dc1393255.html | E20776/z40001dc1393255.html | 10:39:03 | 13238 | 7e721d930fb42f97546990758683b4eb21cec73531adb905f12814e5a09ab659 |
| sbm6000-site-planning-z400000a1394304.html | E20776/z400000a1394304.html | 10:39:03 | 13114 | 8e1507de995a1d5c387459a0a5528a20ec5c1b5b71aaaafe0e3b7c839ac91885 |
| sun6000-facts-2026-10-10.json | proposed curated input | | | 5cfa10f38e95904b8d20f0402aa53f748d7019e0829a95ee76b509c1746cc03a |

Scripts and temp catalog (outside owned dirs, scratchpad `.../scratchpad/sun6000/`): `cap.py`, `gen.py`, `val.py`, `imp.py`, `temp.sqlite`.

## Supported exact facts (all in the facts JSON, one source per capture, one device row `Oracle` / `Sun Blade 6000 Modular System`)

- Library title "Sun Blade 6000 Modular System Product Library Documentation"; lists Product Notes, Overview, Site Planning Guide
  (6000 and 6048), Installation Guide, Service Manual, Safety and Compliance Guide, firmware guides (library index, copyright 2012, 2015 Oracle).
- Printed chassis names: "Sun Blade 6000 Chassis" (Installation Guide chapter 2 title; Service Manual "Click Sun Blade 6000 Chassis.").
- Service Manual chassis part numbers with PCIe midplane PNs: A90-A/541-1983, A90-B/541-3789, A90-D/541-4239, 7105379/7045529;
  A90-B, A90-D, 7105379 have PCIe 2.x midplanes; 7105379 "functionally equivalent to A90-D".
- Replacement components per chassis (power supply 300-1802 / 300-2190 / 300-2259 or 7060590 / same; PSU fan tray, rear fan,
  CMM, midplane PNs) with the Service Manual's ordering and EN 60950 notes.
- Site Planning: minimum four AC inputs; "2N power redundancy at a 5600W consumption level ... should never exceed 6250W";
  grid-redundancy Line A / Line B statement.
- Product Notes contents list "Power Supplies Shipped to Korea" and "Identifying Chassis Models" (titles only, body not captured).
- Existing catalog already holds Oracle `Sun Blade 6000 Modular System` and `Sun Blade 6000 Chassis` (Oracle EOL index); this input adds
  evidence records to the former (same brand+model identity), no new identity.

## Excluded claims, gaps

- No Celestica/factory/Mexico/Czech statement appears in any captured Oracle page; none was added. RRA `6000` = Sun Blade 6000 Chassis is an
  RRA statement; no public document here prints the certificate number or the RRA names. The RRA record and Oracle docs are linked only by the
  exact printed product name (a name match, not a certificate match).
- `Netra 6000 AC/Chassis` and `Oracle Sun Blade 6000 Chassis M2`: not confirmed from public docs (Netra 6000 docs exist at
  docs.oracle.com/cd/E19945-01/E21622 from search results, not captured). Product notes mention "M2" only for NEM/Storage Module.
- Which chassis variant (A90-A/B/D/7105379) the 2007 certificate covers is unknown; the 2007 date predates the 2013 A90-D/7105379 notes.
- Weights/dimensions/PSU rating/cooling spec pages (Site Planning "Specifications") not captured; the 5600 W figure is the only PSU-related number and
  is a redundancy-budget sentence, not a PSU rating. PSU wattage 5600 W per supply came only from search snippets of a third-party mirror: not used.
- Product Notes body, Overview, Safety/Compliance not captured. Table pairing in the replacement-components page is by flattened row order; the
  Pass-through NEM row was garbled and not extracted.

## Checks

- Quote check (`val.py`): every `literal_quote` found in its saved HTML after whitespace and cell-separator normalisation (0 misses, after fixing 2 ellipsis quotes).
- Temp import: `Catalog` + `pipeline.import_source(c, 'curated', ...)` returned 8; 8 sources, 8 records, 1 device; integrity ok, foreign-key check empty.
- Nothing written to `data/devices.sqlite` or other shared state.

## Decisions needed (policy proposal — not implemented)

Current behaviour: `rra.company_target` returns None for the cell (the cell is pure ASCII, but it joins two entities with a comma plus a
parenthesised third name, and Celestica de Monterrey / Kladno are not among the reviewed legal names), so only `rra_equipment` rows exist.

Proposal A (preferred, narrowest): a per-certificate reviewed entry in a new optional company-evidence kind, e.g.
`rra-composite-manufacturer-cells`, keyed by (certificate number, exact raw cell sha256), listing split parts:
`Celestica de Monterrey S.A. de C.V.` -> role `registry_manufacturer`, target Celestica; `Celestica Kladno s.r.o.` -> same role; trailing
`(Oracle America, Inc.)` -> recorded as unlabelled parenthetical, not a role (or `registry_applicant_reference` only if the RRA documents the convention).
Each part keeps the raw cell, split basis, and evidence = the RRA detail hash. Existing `rra_manufacturer_lists` extraction is the precedent
for reviewed splitting of composite cells; reuse its guard style (exact cell match, no pattern-based splitting).
Effects: catalog record for main model `6000` and derived models become Celestica `registry_manufacturer` claims with Oracle applicant, as with SPARC T1000/T2000.

Risks: (1) legal-name evidence: "Celestica de Monterrey" and "Kladno" are not among the five names in the saved reviewed Exhibit 21.1 extract (`data/raw/rra-celestica-official/celestica-subsidiaries-2025.json`, checked 2026-10-10; that file is itself a reviewed text extract, not original HTML); a registry-held
name containing "Celestica" is still not proof of physical manufacture (the cell lists two factories; the country field names only Mexico),
so the role must stay `registry_manufacturer` with no factory/country-per-factory claim; (2) entity-relationship evidence would need a new
reviewed source (company registry/filing), otherwise the mapping is name-only; (3) splitting conflicts with the stated rule "composite companies are
not split" unless limited to explicit reviewed entries; (4) the `(Oracle America, Inc.)` parenthetical could be misread as a third manufacturer or
as brand; (5) the same four derived models (Netra 6000 ..., Chassis M2) would gain manufacturer claims from one 2007 certificate, though the Oracle docs
only show chassis variants through 2013; (6) the same cell in two bundles must stay one certificate, not two independent corroborations (identical hash);
(7) `company_target` is global in `rra.py`, so a generic rule could mislabel other composite cells — keep it certificate-scoped; (8) frozen
projection hashes in the campaign release: any importer change needs the engineering owner.
Alternative B: leave raw-only and add a hand-curated record from the RRA page in the curated JSON (no importer change) — cheaper but duplicates the claim outside the RRA importer.
Decision for the user: A, B, or raw-only; and whether to ingest the facts JSON as-is (source ids and brand `Oracle` reviewed).

## Next step

If approved, integrator imports `sun6000-facts-2026-10-10.json` via the manifest; optional follow-ups within a finite batch: capture Site Planning
"Specifications / AC Power Requirements / Chassis dimensions" and Product Notes "Identifying Chassis Models" bodies (same library, ids above), and Netra 6000 E21622.

## Fixes after review (2026-10-10)

1. `ordering_note_as_printed` split into two cited facts: "If a second part number ... for that component." and "Use the second part number when ordering a replacement part." (each value equals its quote).
2. Every source coverage and every `specification_scope` fact now states the facts attach to the document-named `Sun Blade 6000 Modular System` identity, are not merged with the RRA "Sun Blade 6000 Chassis" record or certificate SUN-6000, and that the chassis variant covered by the 2007 certificate is unknown.
3. Capture times for the product-notes and both site-planning pages corrected to the ledger (10:39:03 UTC).
4. Exhibit 21.1 statement verified against the saved extract (five names, none is Celestica de Monterrey or Kladno); no new fetch.
Re-validated: quote check 0 misses; temp-catalog import returned 8, 1 device, 8 records, 41 facts, integrity ok. New facts JSON sha256 `5cfa10f38e95904b8d20f0402aa53f748d7019e0829a95ee76b509c1746cc03a`; captures and ledger hashes unchanged.
