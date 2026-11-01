# networking primary independent code review

reviewer: networking_primary_review; 2026-10-04. **pass** for the frozen code
and tests below. source acceptance is separately frozen in `review.md`.

## change and behavior

- reviewed actual `devicehunt/buying.py` and `tests/test_buying.py`, engineer
  handoff, pre/post hashes and focused/full logs. actual change is one explicit
  mapping block and three tests. no matching, identifier, role, claim-status or
  rendering algorithm changes.
- each SM variant's chassis field maps to chassis_physical; memory/PCIe/bays and
  processor maxima map to compute_network_storage. processor maxima never map
  to power. complete literal variant sections and limit/context fields remain
  unclassified and visible. no parsing, value-based classification or variant
  inference. SM 1U and SM 2U labels remain distinct.
- four artifact declaration fields map to firmware_boot_software. record tier,
  URL, hash, company roles and full context survive. these fields do not create
  identifiers or affiliations. empty-brand community record cannot be borrowed
  by Accton filtering; compatible/model text is not a matching alias.
- new tests exercise two variant labels, processor exclusion from power,
  multi-category literals staying unclassified, isolated part-number source
  scope, nearby X7-2 exclusion, community provenance, regulatory identifier
  exclusion and vendor-filter isolation. these verify evidence boundaries,
  not only membership in implementation sets.
- existing CLI matching remains exact and record-local. use full
  `buying-report 'Oracle Server X7-2C'` to include the new VPAT; a short alias
  held by another record does not carry that record's match into the VPAT.

## reproducible verification

post hashes independently verified:

| path | SHA-256 |
| --- | --- |
| `devicehunt/buying.py` | `16197bf80ea24183a75f43ab9fea4cdbedee03f2a5a6f1ea039c2e1ba24c5c53` |
| `tests/test_buying.py` | `800c6610f612e529fe7efceec47b8e588c7319a2bc181b6e21fd98161a8376fb` |

focused log: 10 tests, OK; full log: 427 tests, OK. no unnecessary rerun.
log hashes: focused `674dd7d77f918239ef12051ed1f46fce1ffd1b3e93fb04448c26f5e2ffb1e68c`;
full `33bc5f74ec982f94596caa907e9ade43d96eb64b93406ea56b3f1baad5d044fa`.

initial engineering `code.diff` was empty because both code paths are untracked.
independently reconstructed preimages in memory by removing exactly the mapping
block (from `# Oracle's named SM variants` through the line before `LIMITS`)
and the three added tests (from `test_oracle_variants_preserve_labels` through
the line before `test_matched_identity_with_no_practical_facts`). resulting hashes
equal both recorded pre hashes:
`82d5c1f70847979b5b7d6a18bdf9587f469045af547e560e6b1bf695dc4a67e5` and
`0ae4a94f9d05a61bccaf3f5ee2cee3e7120e15ccdc66c40968bbfc3d537ef97b`.
this proves the bounded actual change without relying on the empty diff.
artifact finding closed: engineer retained hash-verified reconstructed preimages
and repaired the real before/after diff. independently regenerated that diff:
equal, 7,779 bytes, SHA-256
`14325a0be365ae2ae677a792c0e3fbfc02f282e33270a1654faa07c6653c222a`.
all 86 added lines pass explicit trailing-whitespace checks. frozen post hashes
remain unchanged. `engineering/diff-verification.json` and `reconstruct-diff.py`
preserve the repair method. no code/test changes or reruns needed.

no code/source/shared-state writes or new acquisition performed by reviewer.
integration, preservation, rebuild and current exports remain separate checks.
code review is final and frozen; no outstanding code findings.
