# wave 3 integration — acceptance running

/root is sole integrator and current-document editor. **Not promoted.** Live pair
remains the accepted PSU checkpoint in PROGRESS.md. Independent stage and both
code reviews ACCEPT; full guarded suite and final integration review pending.

## verified candidate and rebuild

`data/devices-candidate-wave3-2026-10-12.sqlite` was staged with the old importer:
5117 identities /7241 records /74210 facts /2093 sources /68 PDFs /3784 pages.
`data/import-manifest-candidate-wave3-2026-10-12.json`:148jobs/3941hashedinputs.
Complete reference bundle `data/raw/reference-documents-with-wave3-2026-10-12.json`
retains all55oldentries and adds two complete PDFs/five pages (57total).
Both curated candidates also pin all referenced snapshot/text sidecars.

`stage-report.json` binds counts, input hashes, live/candidate hashes and source
preservation. All50ordinarylogicaltables retain old row multiplicities. Exactly55
old reference-source bundle hashes change; checked separately. Integrity ok,
no FK violations, owned10/10 and campaign input/projection gate pass.
Independent stage acceptance: `../review/wave3-stage-review.md`.

Clean new-code rebuild completed, session82624 exited0:
`data/rebuilt-wave3-2026-10-12.sqlite`, log`rebuild.log`. Comparator session84628
exited0: **all52tables/indexes and baseline match**, including bothFTSindexes.
Proofs `data/exports/wave3-2026-10-12-comparison.json` and `-validation.json`;
`compare.log` records result. This compares new importer output to the held
old-code candidate. No current shared state changed.

Read-only model checks in `model-query-check.json`:9SD6200/8Wedge/9Oracle7420/
1M2categorizedstatements. M2 has only its reviewed literal statement; no transfer
to registry spellings, and ordering rows are not identifiers. Candidate hash
unchanged. `check_queries.py` reproduces the checks.

## running and required next steps

Full guarded Python suite is session**43695**, label
`discovery-2026-10-12-curated-wave3`; log`full-suite.log`. Check the actual handle,
do not restart. `code-under-test.json` freezes209production/test/dependencyfiles;
none may change during acceptance. Curated and shopping code reviews are
`../review/curated-import-fix-review.md` and `shopping-query-fix-review.md`.
Independent shopping checks include15Python/7JS tests and eight saved examples.

After suite passes, verify all frozen hashes, supply actual complete proofs to
/psu for final `../review/wave3-integration-review.md`. Root then writes explicit
hash-bound `promotion-acceptance.json`; `promote.py` requires mandatory proof
paths, exact review path, confined relative paths, exact52logicaltablekeys and
bothFTSindexes, candidate/live hashes and currentmanifestpins. Original helper
before stronger guards is preserved as `promote-before-proof-guards.py`.
No promotion/helper execution yet. Recovery backups and pending paths use fresh
wave3names; overwrite historical snapshots is forbidden.

After accepted promotion, `refresh_exports.py` opens authoritative catalog
readonly, preserves seven existing exports with hash provenance, refreshes the
catalog including accepted shopping behavior, and writes a new dated four-model
specification/document export. It verifies unchanged database bytes. Earlier PSU
model export remains untouched. Finish PROGRESS/usage and independent doc review.

## stage history

`stage.py`/`stage.log` stopped on an uppercase-only acceptance-text assertion
(Oracle review used lowercase), after first-group copies and before DB creation.
`stage-r2.py`/`stage-r2.log` verified those copies byte-identical, fixed verdict
case and completed isolated staging. Stage session22102 exited0. No captures
were overwritten. `old-code-under-test.json` binds the accepted old importer/
catalog to the design profile. Earlier detailed staging note is preserved in
`handoff-before-acceptance.md`; its process/state assertions are historical.
