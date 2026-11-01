# wave 3 integration — promoted

/root is sole integrator and current-document editor. **Promoted and exports refreshed.**
Independent stage/code/final integration reviews ACCEPT; 791 full-suite tests pass.
`promotion-result.json` and `exports-report.json` bind actual results. Live pair
equals the frozen candidate pair; prior PSU pair and seven exports are preserved.

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

## completed acceptance and promotion

Full guarded suite43695 finished:791passed, zero errors/failures/skips/blockedcalls.
All209files in code-under-test.json remain unchanged. Final independent review:
`../review/wave3-integration-review.md`, SHA256
`df355f8d163ad00fab707ef6a5937e0a3bec8b3f0532948136e147c1f027f485`.
Root receipt `promotion-acceptance.json` binds mandatory and supporting proofs.
`promote.py` completed session45892, preserving the prior live pair before
replacing it; journal/result bind exact old/new/recovery hashes.

`refresh_exports.py` completed session24026, preserving seven earlier exports
and writing eight outputs including dated four-model results. Specifications
contain1285rows; all output hashes verified and authoritative DB hash unchanged.
`promoted-gate-check.json` confirms current campaign input/projection gates pass.
Prior dated PSU export remains untouched. Current docs updated; independent
completion documentation audit accepted: `../review/wave3-completion-doc-review.md`. No wave3 process remains running.

## stage history

`stage.py`/`stage.log` stopped on an uppercase-only acceptance-text assertion
(Oracle review used lowercase), after first-group copies and before DB creation.
`stage-r2.py`/`stage-r2.log` verified those copies byte-identical, fixed verdict
case and completed isolated staging. Stage session22102 exited0. No captures
were overwritten. `old-code-under-test.json` binds the accepted old importer/
catalog to the design profile. Earlier detailed staging note is preserved in
`handoff-before-acceptance.md`; its process/state assertions are historical.
