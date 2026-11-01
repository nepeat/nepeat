# SUPERSEDING NOTE (rev 2 of phase 2, 2026-10-11)

The queue below (coverage-0db21eac…) is **superseded**; see its `SUPERSEDED.md`. Cause: the `composite_coverage` source-coverage sentence
overstated the Kladno-only reading (review S1); it now says a part carries `registry_manufacturer` only where reviewed legal-name evidence
exists, other parts and the parenthetical are unmapped literal context (no role/target/affiliation), scoped to the keyed certificate/cell.
Only `devicehunt/rra_company_names.py::composite_coverage` changed (the sentence in `prototype.diff` is the older text).

| item | value |
| --- | --- |
| pin | `inventory-r2/` (batch-local script re-run, gate-less pin base unchanged): inventory file sha `b8e380b7…1afd`, definition sha `39747f315cb0392984af14cef882f47030ad3100652dc0a8ba6046ffb602dbfa`, 12 units, 1,122 paths, verify OK; same pins as before except code-reference hashes |
| run | `run-9f2db863-e585-4ae8-a5f9-c6174b3a9b1e`: complete, 36 executed, 12/12 validated, all matches_pinned, +1,388,507 bytes, 0 network (`logs/r2/`) |
| coverage manifest | `coverage-b8ee5df9984de206d250cf144da8d40ac60535b6caaee2d84fc07506dfdcbacd` (validate OK) |
| **new queue** | `data/campaign-reviews/coverage-b8ee5df9984de206d250cf144da8d40ac60535b6caaee2d84fc07506dfdcbacd/` (12 items, no decisions) |
| tests | targeted (composite, rra, company_names, celestica_historical, manufacturer_lists): 40 OK after the edit |

New items (11 no_changes, 1 changed):

| unit | item id |
| --- | --- |
| ised-rel-25869 | review-item-183042d486a9dc1da3f8ca2a681a7e80c88a584eb83ae9ba0640bf3b686fff57 |
| ised-rel-query-accton | review-item-3b048fa5a07b2acaa4f98fabc67528cc2cd4a27b9b783348a3479473d6acbd01 |
| ised-rel-query-celestica | review-item-4851f8870d3984577e1251299940045aee00167e408aeecfeb7f2827a80cbe4f |
| ised-rel-query-edgecore | review-item-b008f6320a151f736636465a94083b7435b708ebbadb8ae93377cb77b37a84ab |
| ised-rel-query-oracle | review-item-0e8990f24c5dc9285a3ebbc349fb1f4bfbcef50c4840dfea26e197aba870b78d |
| rra-official-query-33f7d154085d | review-item-e95cf9e2e0ba042856bdb8897a22935ba5c247e2907dba957feb59b9ba68e765 |
| **rra-official-query-41e4ca912b33 (Celestica, CHANGED)** | review-item-6fe3b47a8dad7ca7824dd2506d10d81143b9223ad8cdf6f7f1346ed07a9403b3 |
| rra-official-query-5827f2fe7092 | review-item-7f24684f40d979866b0b78758d21cd8925b9507cf3db475de7194f6ff89d9e6d |
| rra-official-query-63cb0b9098c7 | review-item-4d61233c3e8a639cd17000fb6a893ccd25730144a148bce9aecd50278f91017d |
| rra-official-query-73009f906e4e | review-item-6297155c0d3b4ca502477fd92b97e163cedda155b200ba890331d5488d93ee38 |
| rra-official-query-78fc6b2e95f6 | review-item-22a58547c70a4d72ab402ef1668d6f3cfc37358b7b194eccec3d84b233410a0c |
| rra-official-query-e5984834207f | review-item-4c9b70e229f5dca1768936bf5de70a9d08b7456127af8c354c8097cd8e5589da |

Celestica diff (`logs/r2/celestica-item-summary.txt`): same as before: +5 records (the five `SUN-6000:*` keys, Kladno registry_manufacturer + Oracle applicant),
149 unchanged, 0 removed/changed/moved, raw rows identical, source fields changed `coverage` (new exception sentence) and `sha256`. Candidate projection
`5699fd41…4997` (was `064a32b5…`), baseline `a313914d…` unchanged; baseline job `bundle.json` (160 inputs), candidate job `bundle-composite-cells.json` (161).

In the remaining-commands block below, use `I=research/batches/discovery-2026-10-11/sun6000-a/inventory-r2`, the new `Q`/`C` (coverage-b8ee5df9…), and the new definition for `DEFAULT_*`.
Re-pin command used: `python3 -B …/inventory/pin_inventory.py build --root . --output …/inventory-r2`; run/freeze/queue with `--definition/--inventory` pointing at `inventory-r2`.

---

# SUN-6000 phase 2 handoff (worker sun6000_a, Claude Sonnet 5.5, 2026-10-11)

Design: `design.md` (rev 2, Kladno-only; Monterrey = unmapped literal context). Stopped before any decision, release, manifest,
catalog or `DEFAULT_*` change. `data/import-manifest.json` and `data/devices.sqlite` untouched (mtime 07:54:20, as before).
Zero HTTP. Reviewer must be someone other than me.

## What changed

| path | change |
| --- | --- |
| `devicehunt/rra.py`, `rra_company_names.py`, `campaign_sources.py` | `prototype.diff` applied unchanged (wrapper kind, `composite_claims`, `claim_target`, import facts/affiliations, raw-slice re-hash) |
| `data/raw/rra-celestica-official/rra-composite-manufacturer-cells-2026-10-11.json` | new, sha `6daf8c28102bbe160e4757a15216057d66bbf08de485697c83996ab2720d54b7` |
| `data/raw/rra-celestica-official/bundle-composite-cells.json` | new, sha `2e32bff9289d426a21e9e4e3e9c3c548a4416a7d6c15fea7376395513eb0b7fc`; differs from `bundle.json` (`6aacf7a8…`, untouched) only in `company_evidence` |
| `data/import-manifest-pinbase-sun6000-2026-10-11.json` | PIN BASE ONLY (no `campaign_gate`, job swapped, 2 files hashed); never `--accepted-manifest`, never rebuild; delete/supersede after promotion |
| `tests/test_rra_composite_cells.py` (new, 20 tests), `tests/test_campaign_rra_live.py` (+1 carry test) | see tests |
| `research/batches/discovery-2026-10-11/sun6000-a/inventory/` | batch-local `pin_inventory.py` (MANIFEST -> pin base, uses `rra.claim_target`/`composite_claims`, adds `campaign_sources.py` to references), `inventory.json` (file sha `277f143a…9826`), `campaign-definition.json` (definition sha `0175d3c8ad42e38423ff1343fc78c019861e5ef963ef99cca5bc5c1088332b0d`), receipt |
| `data/coverage/rra-ised-saved-2026-10-04/coverage-0db21eac…64c7.json` | new coverage manifest |
| `data/campaign-reviews/coverage-0db21eac…64c7/items/` | the queue (12 items, no decisions) |
| `data/archive/` | one new offline run (36 receipts, +1,521,895 bytes, cap 64 MiB) |

## Commands run (outputs in `logs/`)

1. `patch -p1 < research/batches/discovery-2026-10-11/sun6000-a/prototype.diff` (dry-run first, clean).
2. Step 0, mixed state (new code, c3b defaults, no new data): `python3 -B -m unittest tests.test_rra tests.test_rra_company_names tests.test_rra_celestica_historical tests.test_rra_manufacturer_lists tests.test_rra_sun tests.test_campaign tests.test_campaign_rra_live tests.test_review_queue tests.test_coverage` -> Ran 109, OK (`logs/step0-mixed-state-tests.log`).
3. Data: `cp -n` the two drafts into `data/raw/rra-celestica-official/`; `diff bundle.json bundle-composite-cells.json` = the 2-line `company_evidence` change; `rra.validate` -> 151 rows, claims `['SUN-6000']`.
4. Tests written; `python3 -B -m unittest tests.test_rra_composite_cells` OK (20); `tests.test_campaign_rra_live.RraLiveBundleCarryTests` OK (5).
5. Pin: `python3 -B …/inventory/pin_inventory.py build --root . --output research/batches/discovery-2026-10-11/sun6000-a/inventory` -> 12 units, 1,122 source paths, verified; `… verify` same (`logs/pin-build.log`). Versus c3b: only the Celestica unit changed (160 -> 161 paths, 106 eligible rows, 154 model assertions, five `SUN-6000:*` keys), `definition_version` 1, `max_referenced_source_paths` 1,122.
6. `python3 -B -m devicehunt.campaign --definition $I/campaign-definition.json --inventory $I/inventory.json plan` -> 36 unplanned.
7. `… run --offline` -> `run-56886670-4363-4b01-8e19-f9505bb4f3ea`, complete, 36 executed, 12/12 validated, 2 explicit zero, all `matches_pinned` (Celestica against the new pins), 0 network requests (`logs/run.json`).
8. `python3 -B -m devicehunt.coverage --definition $I/campaign-definition.json --inventory $I/inventory.json freeze run-56886670-… --output data/coverage/rra-ised-saved-2026-10-04` -> `coverage-0db21eac50f8f269fc5eaec881b5b15feb840fc2bfa70297175fb04464a664c7` (36 receipts, 1,118 artifacts); `… validate` OK.
9. `python3 -B -m devicehunt.review_queue queue $C --output data/campaign-reviews/coverage-0db21eac… --definition $I/campaign-definition.json --inventory $I/inventory.json --accepted-manifest data/import-manifest.json` -> 12 items (bundle receipts, archive bytes and re-derivation all passed with the final code).
10. Final targeted tests: `tests.test_rra_composite_cells test_rra test_rra_company_names test_rra_celestica_historical test_rra_manufacturer_lists test_rra_sun test_rra_empty test_rra_reviewed test_rra_capture test_review_queue.AcceptedManifestGateTests` -> 56 OK. (`AcceptedManifestGateTests` passes because `DEFAULT_*` still point at c3b.)

## Queue

`data/campaign-reviews/coverage-0db21eac50f8f269fc5eaec881b5b15feb840fc2bfa70297175fb04464a664c7/items/` (no `decisions/` yet).

| unit | item id | no_changes |
| --- | --- | --- |
| ised-rel-25869 | review-item-cdd8ec7c… | yes |
| ised-rel-query-accton | review-item-98d995c2… | yes |
| ised-rel-query-celestica (explicit zero) | review-item-3f0c2531… | yes |
| ised-rel-query-edgecore | review-item-0d9fd2e7… | yes |
| ised-rel-query-oracle | review-item-80f442b8… | yes |
| rra-official-query-33f7d154085d | review-item-c8353af0… | yes |
| **rra-official-query-41e4ca912b33 (Celestica)** | **review-item-8bcb3b81df797e6c48e94c58f4adefef980edaf2859d29b009a5df2eb6c395ff** | **no** |
| rra-official-query-5827f2fe7092 | review-item-06ed19b3… | yes |
| rra-official-query-63cb0b9098c7 | review-item-c1ee5190… | yes |
| rra-official-query-73009f906e4e (Oracle) | review-item-c0e3dc89… | yes (SUN-6000 overlap conflict kept, `independent_corroboration: false`) |
| rra-official-query-78fc6b2e95f6 (explicit zero) | review-item-f3a6e86c… | yes |
| rra-official-query-e5984834207f | review-item-08790824… | yes |

Celestica item (`logs/celestica-item-summary.txt`): candidate job `raw/rra-celestica-official/bundle-composite-cells.json` (161 inputs);
baseline job `raw/rra-celestica-official/bundle.json` with the 160 old inputs from the accepted release; baseline projection
`a313914d…` (149 records) -> candidate `064a32b5…` (154 records). Diff: records added 5 (`SUN-6000:6000`, `:Netra 6000 AC`,
`:Netra 6000 Chassis`, `:Oracle Sun Blade 6000 Chassis M2`, `:Sun Blade 6000 Chassis`; each affiliations = `Celestica Kladno s.r.o.` ->
Celestica `registry_manufacturer` + `ORACLE AMERICA. INC` -> Oracle `regulatory_applicant`), removed/changed/moved 0, 149 unchanged,
raw rows identical, source fields changed: `coverage` and `sha256` only. Conflicts: 38 `repeated_certificate_distinct_devices` (pre-existing)
and 1 `cross_query_certificate_overlap` for SUN-6000 (also in the Oracle query).

Reviewer notes (S4): the Celestica decision reason must state the Kladno-only judgement and counts (5 new Celestica identities, role
assertions +5, Monterrey unmapped context, Oracle-query row raw-only). Decide all 12 (11 `no_changes`).

## Remaining commands (after independent decisions)

```sh
I=research/batches/discovery-2026-10-11/sun6000-a/inventory
Q=data/campaign-reviews/coverage-0db21eac50f8f269fc5eaec881b5b15feb840fc2bfa70297175fb04464a664c7
C=data/coverage/rra-ised-saved-2026-10-04/coverage-0db21eac50f8f269fc5eaec881b5b15feb840fc2bfa70297175fb04464a664c7.json
# reviewer, once per item (12): 
python3 -m devicehunt.review_queue decide $Q/items/ITEM.json accepted|rejected --reviewer NAME --reason WHY --queue $Q
# release
python3 -m devicehunt.review_queue release $Q --coverage-manifest $C
python3 -m devicehunt.review_queue check-release data/campaign-releases/campaign-release-<new>.json
# candidate manifest: cp data/import-manifest.json data/import-manifest-before-sun6000-2026-10-11.json; then build
#   data/import-manifest-candidate-sun6000-2026-10-11.json = live manifest with: Celestica rra job path -> raw/rra-celestica-official/bundle-composite-cells.json;
#   files += bundle-composite-cells.json, the wrapper, campaign-releases/<new>.json, coverage/rra-ised-saved-2026-10-04/<new coverage>.json;
#   campaign_gate -> {release: campaign-releases/<new>.json, coverage_manifest: coverage/rra-ised-saved-2026-10-04/coverage-0db21eac….json};
#   expected.devices 5107->5112, expected.records 7225->7230 (sources[...41e4ca912b33] stays 151)
# candidate catalog: sqlite backup of data/devices.sqlite to data/devices-candidate-sun6000-2026-10-11.sqlite, then
python3 - <<'PY'   # integrate.py-style staging
from pathlib import Path; from devicehunt.catalog import Catalog; from devicehunt.pipeline import import_source
c=Catalog(Path('data/devices-candidate-sun6000-2026-10-11.sqlite')); import_source(c,'rra',Path('data/raw/rra-celestica-official/bundle-composite-cells.json')); c.db.commit()
PY
# expect devices 5112, records 7230, Celestica manufacturer identities 198 (role assertions 246), pairs 2147 / ids 2146
python3 -m devicehunt --db data/rebuilt-sun6000-2026-10-11.sqlite rebuild --manifest data/import-manifest-candidate-sun6000-2026-10-11.json   # runs gate_inputs + gate_projections
# repeat c3b's check with data/archive and $Q moved away
PYTHONPATH=. python3 scripts/compare_rebuild.py data/rebuilt-sun6000-2026-10-11.sqlite --database data/devices-candidate-sun6000-2026-10-11.sqlite --output-prefix sun6000-2026-10-11
python3 -m devicehunt --db data/devices-candidate-sun6000-2026-10-11.sqlite validate-inventory
FCC_C1_ACCEPTANCE_LABEL=sun6000-pre python3 research/batches/campaign-c2-2026-10-05/acceptance/run_offline_suite.py   # green BEFORE promotion
# promote (together): cp backup data/devices-before-sun6000-2026-10-11.sqlite; candidate -> data/devices.sqlite; candidate manifest -> data/import-manifest.json;
#   repoint DEFAULT_DEFINITION/DEFAULT_INVENTORY in devicehunt/campaign.py to $I/campaign-definition.json and $I/inventory.json
# rollback = restore the three together (catalog backup, manifest backup, previous DEFAULT_* constants)
python3 -m unittest tests.test_review_queue.AcceptedManifestGateTests tests.test_campaign tests.test_coverage
FCC_C1_ACCEPTANCE_LABEL=sun6000-final python3 research/batches/campaign-c2-2026-10-05/acceptance/run_offline_suite.py
# then: exports/lookups/collection reports, PROGRESS/README/docs (vendor-inventory totals; Celestica-brand vs Oracle-brand 'Sun Blade 6000 Chassis' distinct; '6000' literal), mark/delete the pin-base manifest
```

Note: if the reviewer rejects the Celestica item, build the release anyway (Celestica stays baseline with `candidate_job` forbidden) and the code/data above stay inert for the catalog; nothing else changes.
