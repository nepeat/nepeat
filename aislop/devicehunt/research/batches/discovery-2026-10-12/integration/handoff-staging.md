# PSU integration checkpoint

Coordinator `/root` is sole integrator. Candidate and complete collection are staged, not promoted. `stage-report.json` records all old logical rows preserved, six new records/42 facts/three identities, five new PDFs/63 pages, unchanged target inventory, accepted gate, integrity/FK checks and 10/10 baseline. Candidate manifest has 146 jobs/3928 pinned files.

`stage.py` imported the isolated candidate then failed attaching a read-only URI through Catalog's URI-disabled connection. Original script/log are retained unchanged. `finalize_stage.py` completed all audit work using URI-enabled, read-only connections, without rerunning imports; it verified candidate/live byte hashes remained unchanged. `finalize-stage.log` and report bind this recovery. Do not rerun either helper against existing outputs.

Running handles at checkpoint: full guarded suite session60425, label `discovery-2026-10-12-poppler-psu`; clean rebuild session53044, destination `data/rebuilt-psu-2026-10-12.sqlite`, candidate manifest `data/import-manifest-candidate-psu-2026-10-12.json`. Candidate DB `data/devices-candidate-psu-2026-10-12.sqlite`. Wait for completion, compare all52 tables/indexes via scripts/compare_rebuild.py explicitly selecting candidate, and obtain independent integration acceptance before promotion. No live archive change.

`candidate-smoke.json` records exact-model specifications, searchable document hits and unchanged manufacturer/all target inventory counts, with unchanged candidate hash. `campaign-final-check.log`: ten final shape/gate tests pass against stable reviewed production code. `code-under-test.json` binds production changes and complete test files for suite acceptance.

Independent integration reviewer has additionally repeated all50 ordinary-table multiset preservation comparisons with zero old groups lost. `promote.py` and `refresh_exports.py` are prepared, not executed; promotion requires explicit hash-bound final acceptance JSON after fullsuite/rebuild comparison. Pre-stage script is preserved as a failed historical run, not a runnable continuation recipe.

Clean rebuild completed and exec session53044 closed. Explicit candidate comparison completed successfully: all52 tables/indexes and baseline match (`compare.log`, `data/exports/psu-2026-10-12-{comparison,validation}.json`). Full suite769passed. Final independent integration acceptance is pending before promotion; old running-handle paragraph above is historical.
