# independent review — owned buying import, 2026-10-04

## stage 1: label input — accepted

independently inspected the original label pixels with `view_image`, read the
previous networking evidence note and curator artifacts, and ran a separate
standard-library structural comparison and SHA-256 check. no findings.

accepted candidate: `data/label-evidence-with-input-marking-2026-10-04.json`
SHA-256: `da337224625fc56240c387d579bbc5e9818edd1524064589384c3f28989c1de9`.
original input SHA-256: `cb03db8c48a15a857a05dd1803c84e1a3315b472e1d8c6349e38e69216a1d4c9`.
original photo SHA-256: `3c85ec72cf8e8c69283c562a87a579a519f2a4a0ae1fa7072bdddac78e8082b5`.

checks passed:

- one original source and one device remain; every source field is identical
  except append-only coverage text. source URL, tier, license and snapshot remain.
- every device field, brand affiliation, identifier and exact full ordering code
  remains identical. all nine original facts remain an exact ordered prefix.
- exactly two facts were added: `label_observed_input_voltage_range` with
  `100-240~, 50 - 60Hz, 6A Max`, and its adjacent-label reference.
- original pixels show Model `AS7760-32X` above the Input Voltage Range line on
  the adjacent compliance label. the visible marking supports the transcription.
- source coverage explicitly limits the observation to the photographed marking;
  no generic model input requirement, measured consumption, installed PSU
  inventory, wattage, seller claim or deployment claim is added.
- serial/MAC are not transcribed into candidate searchable fields. unchanged
  image hash matches the retained original image hash fact.
- snapshot still resolves to the original sibling-relative archived photograph.

this acceptance covers the label candidate only. code, BIOS extraction,
integration, exports and final documentation await their assigned review stages.
no network, imports, exports, package installs or writes outside this report.

## stage 2: BIOS importer and buying mapping — accepted

reviewed `devicehunt/broadcom.py`, `devicehunt/buying.py`, both affected test
modules, frozen engineering handoff/hashes/logs, previous Oracle evidence note,
and existing pipeline/source/rebuild primitives. no code findings.

independent checks passed:

- ran `tests.test_broadcom` and `tests.test_buying`: 18 tests pass. reviewed the
  hash-verified engineer full-suite log: 424 tests pass; full suite not repeated.
- extracted arrays directly from every detail capture with separate traversal,
  checked their registered capture hashes, then compared all complete ordered
  dictionaries against temporary SQL import: 24 exact records / 206 literal rows.
  only Oracle / Oracle / compatibility_partner affiliations appeared.
- parser preserves duplicate rows/certIds, raw null/empty/numeric scalars,
  unknown/nested fields and source row order. absent tables yield no fact; present
  empty tables yield `[]`. BIOS rows require bios and certId scalar-or-null fields.
- relevant header drift, multiple BIOS tables, malformed rows and nonfinite
  numeric JSON fail before source replacement. late malformed non-target records
  also preserve the previous full source snapshot, covered by passing tests.
- pipeline is a dispatcher; importer validates every capture before `c.source`.
  caller transaction semantics still apply to database write failures.
- BIOS JSON arrays stay on exact compatibility records; no OS-release pairing,
  physical manufacturer, entitlement or current firmware recommendation is added.
  label mapping remains on exact record with source coverage and context intact.

frozen reviewed module/test SHA-256:

- broadcom.py: `29d34ab28c3035f53475454af854dc03cc27559b901964dd9f4caa54d64341d0`
- buying.py: `82d5c1f70847979b5b7d6a18bdf9587f469045af547e560e6b1bf695dc4a67e5`
- test_broadcom.py: `d2fd840d6818d23e474f72d84b516dc35b9f37110868f5c6cdf556a4926d14ca`
- test_buying.py: `0ae4a94f9d05a61bccaf3f5ee2cee3e7120e15ccdc66c40968bbfc3d537ef97b`

integration-plan dependency, not a code blocker: step 7 still mentions deriving
expected facts for manifest validation. keep fact-count checks in the independent
batch audit, since rebuild compares a fixed-key actual dictionary against
manifest.expected. the coordinator already instructed the integrator accordingly;
revise that plan wording before accepting integration. preservation must compare
all ordinary columns (including record title/kind/observed_at), not only the
obvious identifiers/facts; only the reviewed label raw_json and exact allowed
source metadata changes may differ. use Counter equality for duplicate-sensitive
ordinary and FTS content. integration itself is not yet reviewed or accepted.

no live import, network or export refresh; temporary fixture databases were
created in an independent temporary directory and removed on completion.
