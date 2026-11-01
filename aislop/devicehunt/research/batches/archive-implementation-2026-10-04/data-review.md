# independent frozen release acceptance

reviewer: archive_engine; result: pass before catalog integration. reviewer did not
author coordinator archive/reference/freezer or lineage modules. read-only data
checks performed; only this report was written. no network, imports, catalog,
archive receipts or generated export writes.

accepted release: `data/archive-release-2026-10-04.json`.
SHA-256 `2eadea0c31d9ffdfc1b763b26b8500ebddcb78c94d2706900ed382549fb33736`;
release identity `c468ef0721b90f5935615f37dd9619dad6fc1bc2159a107cbf054ede6aad5b79`.
contains 146 artifacts, 374 frozen receipts, four statements/reviews/links.
`lineage.prepare(release, db)` passed using an immutable read-only SQLite connection
to `data/devices.sqlite`; every selected projection already exists. database hash
before/after remained
`a098c390431efd77d25f162acb039d2ddd938ff5effeffd7e93fa9b0bc34ff16`.

independent checks:

- pilot inventory SHA-256 matches the accepted milestone-0 pin
  `d7be503676cb832824d4e0fd33e5390417cc7af55c476027563d75391c11b327`.
  complete reference bundle `reference_documents.prepare` passes all 47 entries.
- all 47 reference-document receipts equal the pinned entry dictionaries verbatim;
  PDF and text hashes, saved derivation input roles/order, output roles/hashes and
  receipt dependency references match. `read_pages` validates every PDF/text
  envelope and complete ordered page array: 3,589 reference pages. saved recipes,
  tool/config/code versions, extraction time/status/schema version remain unknown.
- Ingrasys reconstructed PDF retains malformed original hash and both capture
  receipts, saved capture ledger, transformation metadata, reconstruction derivation
  and page-text derivation. exact original span `[4128654,8355366)` plus suffix
  `460d0a` reproduces derivative SHA-256
  `83cb825a5ff3a087fd2f0706297f640f1b1778fc4ec72d6daf0ac3876465131e`.
  capture start/end remain null; HTTP200 ledger does not establish a valid original
  PDF or independent second edition.
- four Golduck facts are precisely `form_factor`, `power_supply`,
  `cooling_description`, `pcie_lanes_as_printed`; raw values exactly equal existing
  curated facts and historical fact-provenance literal quotes. all links identify
  Golduck's exact PDF hash/receipt and PDF page 2. `lineage.prepare` confirms literal
  excerpts against page2 text using whitespace normalization only. normalized
  values remain null. printed width `435 x 43.2 x 800 mm` and
  `(17.3 x 1.7x 31.5 inch)` conflict remains literal; 162-lane wording is retained.
  PSU alternatives/input and N+1/eight dual-rotor fan facts gain no auction-unit
  or consumption inference.
- curated input, prior source-review and fact-provenance artifacts are pinned in
  review-basis closure. prior source-review expressly reports visible-page checks;
  this acceptance rechecks saved text/bindings rather than claiming new rendering.
  review and link IDs independently match their canonical payload hashes.
- every release receipt's id/kind/created_at/payload equals its original immutable
  JSON wrapper in `data/archive/receipts`. registration time remains distinct from
  unknown historical acquisition/extraction times. current review timestamps
  describe the new local review decision, never historic capture time.
- all accepted statements belong to Golduck and use vendor_specification fact
  projections. XML recipes/captures remain discovery artifacts and receipts: zero
  XML-derived accepted device/manufacturer/deployment statements. complete finite
  release checksums and dependency DAG pass; no mutable registry lookup is needed
  by `lineage.prepare`.

acceptance scope: frozen finite lineage metadata and exact mapping of four already
reviewed hardware projections. this review does not newly validate marketplace
stock, all historical source coverage, the owned hardware baseline or unresolved
ByteDance associations. coordinator may proceed with candidate lineage import and
independent rebuild/preservation checks; live promotion still requires those checks.
