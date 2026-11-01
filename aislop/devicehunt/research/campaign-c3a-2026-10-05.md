# c3a — immutable coverage manifest for campaign runs

2026-10-05, coordinator `/root-claude` (Claude Code, Opus 5.5), continuing after c2b.
Implements the coverage-manifest half of plan c3. The review queue, selected
evidence release and import gate (c3b) are out of scope. c3a touches no catalog,
manifest, importer or export.

## design

`devicehunt/coverage.py`:
- `build`: read-only. Selects one terminal run's recorded bundle receipts as roots,
  then takes the exact closure bundle → validation → registration plus every
  referenced artifact.
- `validate`: checks the content identity, wrapper bytes, references, acyclicity,
  unit/receipt agreement, the recomputed counts and that the closure holds exactly
  what the roots require. When given roots, it also re-hashes every selected
  artifact on disk.
- `publish`: validates an unpublished sibling first, then exclusively links it as
  `<manifest_id>.json`.
- CLI: `python3 -m devicehunt.coverage freeze RUN_ID --output DIR` and `validate PATH`.

The manifest is claim-free:
- no hardware facts;
- failed, blocked, pending and unavailable units are accounted for, not observed;
- no device-absence or regulator-wide claim;
- `evidence_release` is null until c3b.

All-failed and all-pending runs freeze with zero receipts and zero artifacts,
without the nonempty lineage validator and without fake artifacts.

**Roots come from the run's recorded executions, not today's recipes.** c2b changed
`artifacts.py`, which is inside the campaign recipes' import closure. So
`c2-live-run-1`'s code identity no longer equals the current code, and a new run
would re-execute its jobs. The manifest records the run's own identities.

**Re-pin decision:** c3a needs no accepted-catalog checkpoint, so the definition
stays at version 1. Re-pinning the moved catalog/manifest checkpoints is deferred to
c3b, where import gating actually needs it. Bumping `version` changes every unit
revision and reruns all jobs; a new inventory with the same version only changes the
definition hash. That choice belongs to c3b.

## ownership

| owner | writable paths |
| --- | --- |
| `/root-claude` | this note, `devicehunt/coverage.py`, `tests/test_coverage.py`, `research/batches/campaign-c3a-2026-10-05/`, new `data/coverage/` manifest files only, PROGRESS/docs |
| `c3a_review` (Claude Sonnet 5.5) | `research/batches/campaign-c3a-2026-10-05/review/` only |

The live archive is opened read-only for freezing.

Stop when:
- the E1 gates pass;
- one real manifest for `c2-live-run-1` is published and validated;
- the review is accepted.

After that, report to the user before starting c3b.

## results

- **Tests:** `devicehunt/coverage.py` and `tests/test_coverage.py`, 9 tests. They cover the
  E1 cases:
  - a mixed run;
  - an explicit-zero-only run;
  - an all-pending run with zero receipts and artifacts;
  - a missing selected object that fails while an unrelated damaged object does not;
  - tampering and forgery, including a fully recomputed forged wrapper rejected against
    the archive bytes;
  - honesty overclaims;
  - strict canonical loading;
  - location determinism after later copies;
  - `not_reached` for an interrupted run;
  - the CLI.
- **[Review](batches/campaign-c3a-2026-10-05/review/c3a-review.md):** ACCEPT with notes;
  fixes verified. Afterwards, a unit's scope/source/comparison is cross-checked,
  required claim invariants were added, a half-rooted validate is refused, file names
  must equal the id, and NaN is rejected.
- **Real manifest:** `data/coverage/rra-ised-saved-2026-10-04/coverage-fc6fe1ca0a7844d057a1c38abcf0df62bfcaf50e915e62304e87d8ddd19d0eda.json`
  (mode 0444, 698,710 bytes). It covers `c2-live-run-1` with 36 receipts and 1,117
  artifacts. 12/12 units are validated complete, 2 of them explicit zero; failed,
  blocked and not-reached counts are 0. It validates against both roots, and the
  live archive is unchanged (head sequence 1181).
  [freeze](batches/campaign-c3a-2026-10-05/acceptance/freeze-c2-live-run-1.json),
  [validate](batches/campaign-c3a-2026-10-05/acceptance/validate-c2-live-run-1.json).
- **Open notes:**
  - artifact-registration receipts are not frozen; locations come from payloads;
  - run/definition metadata is identity-bound but not re-checkable offline;
  - an unrelated pending archive publication intent blocks `build()` (fails closed);
  - `build()` takes no run snapshot against a concurrent resume.
