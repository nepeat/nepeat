# accepted PSU integration

Coordinator `/root` completed local promotion and export refresh. The previous
staging checkpoint is retained verbatim in `handoff-staging.md`; its running
handles are historical. No PSU suite/rebuild/export process remains running.

- `stage-report.json`: reviewed six-source / six-record / three-identity /
  42-fact additions, five complete PDFs / 63 pages. All old logical rows survive;
  the 50 old reference sources have the expected complete-bundle hash change.
  Integrity/FK, baseline 10/10 and campaign inputs/projections pass.
- `full-suite.log` and the acceptance directory's
  `discovery-2026-10-12-poppler-psu-suite-result.json`: 769 tests pass, zero
  failures/errors/skips/blocked calls. `code-under-test.json` binds 111 hashes.
- `rebuild.log`, `compare.log`, and
  `data/exports/psu-2026-10-12-{comparison,validation}.json`: clean rebuild from
  the candidate manifest matches all 52 tables/indexes and the baseline.
- `../review/psu-integration-review.md`: frozen independent ACCEPT, hash bound
  by `promotion-acceptance.json` together with the exact proof set.
- `promotion-journal.json`, `promotion-result.json`: old live pair preserved as
  `data/devices-before-psu-2026-10-12.sqlite` and
  `data/import-manifest-before-psu-2026-10-12.json`; validated candidate copies
  became authoritative. Current exact counts/hashes belong in PROGRESS.md.
- `promoted-gate-check.log`: accepted manifest gate test passes after promotion.
- `exports-report.json`: current catalog, targets, specifications, reference
  document inventory, owned validation and dated PSU results refreshed through
  a read-only catalog connection; database hash unchanged. Previous overwritten
  exports are byte-identical in `exports-before/`, with provenance hashes.

The original `stage.py` imported the isolated candidate, then its audit failed
because Catalog's connection did not support the attached file URI. That script
and `stage.log` remain unchanged. `finalize_stage.py` used URI-enabled read-only
connections to validate the already staged inputs/catalog and create the new
manifest/report; it performed no re-import. Reviewer repeated preservation
independently. Do not rerun completed helpers or overwrite their output paths.

`candidate-smoke.json` records 24 practical statements across the three exact
PSU models, document-search hits, unchanged target inventories and unchanged
candidate hash. No live archive edits, manufacturer-role transfers, target
expansion or parent-system fitment inference occurred.
