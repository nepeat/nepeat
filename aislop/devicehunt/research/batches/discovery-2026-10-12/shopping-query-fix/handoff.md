# F17 shopping-query fix — ready for independent code review

author `/root/psu`; **implemented and focused tests pass**, not independently
accepted or exported yet. only assigned code/tests and this directory written.
all three existing owned files were copied/hash-bound before editing; new
`tests/test_shopping.py` was initially absent. `original-files.json` and `before/`
preserve those originals. final four-file hashes are in `code-manifest.json`;
`changes.diff` shows the complete patch. files are stable pending root review.

## resulting behavior

`shopping_links` retains nonblank title and existing approved identifier queries,
adds the selected identity's **exact model literal**, and returns `[]` when
existing `listings.is_software_reference` matches. query literals retain case,
spacing, prefixes/suffixes and punctuation; only blank/non-string candidates
are omitted. exact-set de-duplication, sorted order, query/ebay/web row keys,
URL roots/quote_plus bytes remain unchanged for retained queries. no brand
shortening, model-prefix stripping, corporate-family lookup, guessed alias,
fact-derived query or classifier-policy change.

CLI software output is exactly
`Software reference; no hardware shopping links.`. generated report data
retains `shopping: []`; report JS hides empty shopping details and renders
actual rows with their original labels/URLs. ordinary literal hardware names
including wedge/Switch/S1 remain query choices under the existing classifier.
legacy title/identifier-only helper inputs still work. hardware CLI syntax and
table columns remain compatible; some hardware gains a row and software loses
hardware URLs, as intended. no first-row canonical-query/ranking claim.

## meaningful verification

actual command/results in `focused-tests.json`, clearly labelled summary in
`focused-tests-summary.txt` (not mislabelled raw subprocess bytes):

- `python3 -B -m unittest tests.test_shopping tests.test_readonly_catalog_commands tests.test_listing_software_refs`:
  **15 passed /7.474s**, including six new shopping tests, three readonly tests,
  six existing software-reference tests. new cases exercise retained title/all
  approved identifier URL bytes, rejection of fact/family/unapproved identifiers,
  exact duplicates/blanks/spacing/missing new fields, full suffix/plus/slash/comma/
  case encoding, brand/kind software rules versus literal hardware, actual CLI
  explanation/table, actual report embedded arrays and fixture DB byte preservation.
- `node --test tests/report.test.cjs`: **7 passed**, zero failures/skips;
  TAP duration56.68884ms. new case runs actual inline report script and checks
  literal query/link labels+hrefs and absent groups for empty shopping arrays.
- combined shell exit0. existing command/classifier paths emitted non-failing
  unclosed-SQLite ResourceWarnings; all checks completed. no unrelated warning fix.

no full suite or rebuild was run. tests use isolated temporary fixtures; no
current export/report, authoritative database/manifest or live archive write,
network/click/install/delegation. prior design's eight live examples remain
saved unchanged under `shopping-query-design/`; no new live example scan here.

stop condition met: stable patch, original recovery copies, exact diff/hash
provenance and focused results ready for independent root review. root owns
combined full-suite acceptance and normal export/docs refresh. existing HTML
exports keep their old embedded query arrays until that authorized refresh;
this implementation does not overwrite them or historical snapshots.
