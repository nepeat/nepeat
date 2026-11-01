# c3b — review queue, selected evidence release and import gate

2026-10-10, coordinator/integrator/documentation editor `/root-claude` (Claude Code, Opus 5.5).
The user chose c3b on 2026-10-10. Plan: [c3](../docs/campaign-plan.md) (review queue + gate half).
Orientation: [projection map](batches/campaign-c3b-2026-10-10/orientation/projection-map.md).

## design decisions

- **Whole-projection review per unit.** Importers replace whole sources, and ISED v1
  (`ised-rel-25869`) has no occurrence→record key map, so per-claim selection would need importer
  filters and still could not cover every unit. A reviewer accepts or rejects one unit's
  complete candidate projection; partial acceptance is a rejection. Per-claim selection stays
  a possible later RRA-only refinement.
- **Projection** = one source's `sources` row, its `records` with device brand/model/revision/
  title/kind, raw JSON, affiliations, identifiers, facts and hyperscaler evidence, plus its raw
  `rra_equipment`/`ised_equipment` rows. Environment metadata (`fetched_at`, `cache_path`,
  `observed_at`) is excluded from the hash. The candidate projection comes from the existing
  importer into an isolated empty scratch catalog; the baseline comes read-only from the live catalog.
- **Diffs match by content identity, never by `source_key`** (brand, model, revision and
  identifiers; duplicates paired in order). Changes are added / removed / changed / moved;
  "moved" = same content, different positional key.
- **Immutable chain**: queue item → decision (binds the item id) → content-addressed release.
  `lineage.py` never reads the mutable queue, and the 2026-10-04 lineage release is untouched.
- **Gate** in `rebuild`: the import manifest declares `campaign_gate` naming a release file under
  `data/` (hashed in `files`). Before importing, every release entry's job must be present with
  exactly the allowed input hashes. After importing, every entry's projection must equal the
  allowed projection sha. Units not accepted stay at their baseline inputs and projection.
  Only the 12 campaign unit sources are gated; `rra-reviewed`, `rra-sun-portfolio` and
  `rra-manufacturer-lists` are not. Rebuild never opens `data/archive`.
- **Re-pin**: a new inventory/definition revision beside c0 (same `campaign_id`, `version` 1),
  built with the c0 `pin_inventory.py build`; a trial build reproduced all 12 units and 1,121
  file hashes, differing only in accepted checkpoints and policy-reference hashes.

## ownership

| owner | writable paths |
| --- | --- |
| `/root-claude` | this note, `devicehunt/review_queue.py`, `devicehunt/rebuild.py` (gate hook), `devicehunt/campaign.py` (defaults), `tests/test_review_queue.py`, `research/batches/campaign-c3b-2026-10-10/` (except `review/`), new `data/campaign-reviews/` (items), `data/campaign-releases/` and `data/coverage/` files, `data/import-manifest.json` + catalog promotion (integrator), PROGRESS/README/docs |
| independent reviewers (Claude Sonnet 5.5) | `research/batches/campaign-c3b-2026-10-10/review/` only; the live decision reviewer writes only `data/campaign-reviews/<coverage id>/decisions/` via the `decide` CLI |

Stop when: the gate is tested (the c3 acceptance cases), the 12 live units are queued and decided
by an independent reviewer, the release is frozen, a gated clean rebuild equals live, the full suite
passes, and the code review is accepted.

## results

- **Code**: `devicehunt/review_queue.py` (queue / show / decide / release / check-release CLI),
  gate hook in `devicehunt/rebuild.py`, `tests/test_review_queue.py` (including a
  read-only promotion check that the accepted manifest gates every current campaign unit).
  Release entries are `accepted`, `baseline` (rejected / unreviewed / not reviewable) or
  `excluded` (no accepted baseline: the gate refuses importing it).
- **Code review** (Claude Sonnet 5.5, [report](batches/campaign-c3b-2026-10-10/review/c3b-code-review.md)):
  ACCEPT with notes → one must-fix (release not bound to the full unit set: a deleted item file
  escaped the gate) plus five should-fixes, all fixed and verified → ACCEPT. Open notes: the gate is
  opt-in in `rebuild` (enforced at promotion by `AcceptedManifestGateTests`); `decide` has a
  check-then-write race that fails closed; the candidate bundle sha is recorded, not cross-checked.
- **Re-pin**: `research/batches/campaign-c3b-2026-10-10/inventory/` (definition `0f095a42…`): units
  and 1,121 pinned files identical to c0; checkpoints = manifest `984624e6…` (pre-gate) + catalog `4d525902…`.
  `DEFAULT_DEFINITION`/`DEFAULT_INVENTORY` point here; the c0 files are unchanged.
- **Live offline run** `run-61cc2607-7966-4835-a440-5c35a4addf97`: 36 jobs executed, 12/12 validated
  (2 explicit zero), all `matches_pinned`, 1.39 MB archive growth
  ([report](batches/campaign-c3b-2026-10-10/acceptance/live-run.json)). Coverage manifest
  `coverage-d2bc5c40…` (36 receipts, 1,117 artifacts).
- **Projection check**: for all 12 units, an isolated scratch import projects byte-identically to the
  live catalog (after switching from first-writer device spellings, e.g. `CUBSCOUT`/`CubScout`, to `device_id`).
- **Queue** `data/campaign-reviews/coverage-d2bc5c40…/`: 12 items, all `no_changes`. Conflicts are
  known c0 facts: repeated certificates with distinct hardware versions (e.g. `25869-TE1A001`, three
  HVINs), the 15 accton HTML/CSV discrepancies (= c0 review samples) and the `SUN-6000` cross-query
  overlap (Celestica ↔ Oracle RRA queries, `independent_corroboration: false`).
- **Decisions** by an independent Claude Sonnet 5.5 reviewer
  ([report](batches/campaign-c3b-2026-10-10/review/live-decisions.md)): 12 accepted, 0 rejected, after
  scripted checks of every input hash against the accepted manifest. The report first said the SUN-6000
  flag was absent from the items; the reviewer then verified and appended a correction. It is present in both items ([check](batches/campaign-c3b-2026-10-10/acceptance/sun-6000-overlap-check.txt)).
- **Release** `data/campaign-releases/campaign-release-2a5b2f0c…json`: 12 accepted entries.
- **Gate promoted**: `data/import-manifest.json` now declares `campaign_gate` (sha `5ddb1579…`; previous
  bytes in `data/import-manifest-before-c3b-gate-2026-10-10.json`). Gated clean rebuild
  `data/rebuilt-c3b-gate-2026-10-10.sqlite` equals live in all 52 tables/indexes + baseline; the catalog
  is unchanged (`4d525902…`). Live negative checks: a tampered input hash and a dropped gated job are
  refused ([output](batches/campaign-c3b-2026-10-10/acceptance/live-gate-negative.txt)).
- **Order of verification** (disclosed; it inverts the "full suite before promotion" rule): targeted
  tests (17) → gated clean rebuild + compare → promote the manifest (pre-gate bytes kept) → full
  guarded suite. `AcceptedManifestGateTests` cannot pass before the gate is live, so a fully green
  pre-promotion suite was impossible by construction. Next integrator: run the suite before promotion
  excluding that class, then that class after promotion.
- **Full suite after promotion**: OK under the guarded offline runner, 0 blocked calls
  ([proof](batches/campaign-c2-2026-10-05/acceptance/c3b-final-suite-result.json),
  [log](batches/campaign-c3b-2026-10-10/acceptance/c3b-final-suite.log)).
- **Projection gate on live data**: passes for all 12 entries on both the gated rebuild and the live
  catalog, and refuses a flipped projection
  ([output](batches/campaign-c3b-2026-10-10/acceptance/live-gate-projections.txt)).
- **Not new evidence**: on today's data c3b adds the gating machinery and zero new catalog claims.

## leads noticed (not acted on)

- `SUN-6000` (main model `6000`) lists manufacturer "Celestica de Monterrey S.A. de C.V.,Celestica
  Kladno s.r.o. (Oracle America, Inc.)" in both the Celestica and Oracle RRA queries (raw
  `rra_equipment` rows 90 and 215). That cell does not resolve to a target even with the importer's
  reviewed company-name evidence (`rra.company_target` → None), so only raw rows exist and no
  catalog record. A certificate-specific lead for Celestica factories on an Oracle `6000` product;
  making it a record is a company-name/composite-cell policy decision for a separate reviewed batch.
- `ised-rel-query-edgecore` holds two `27052-EAP101N` rows with the same HVIN: preserved as two
  records, not listed as a conflict (duplicate occurrences are not flagged yet).
