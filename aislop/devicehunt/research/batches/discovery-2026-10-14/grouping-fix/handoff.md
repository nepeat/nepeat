# grouping fix checkpoint — pivot to broader discovery

owner `/root/grouping_review`; checkpoint 2026-10-15 UTC / 2026-10-14 local. root relayed the user's urgent direction to stop further grouping checks and prioritize reusable datasource/bulk discovery work. **private fix is prepared; independent fix review and production application have not occurred.** no production code/tests/data were edited.

prepared artifacts:

- [implementation.patch](implementation.patch): production-applicable unified patch against the209-file production freeze. SHA-256 `7fcaaff208d454c06c22d66a444e613af7eb9c751464947d8390079fe6995e18`. it changes `devicehunt/specifications.py`, `devicehunt/__main__.py`, new `tests/test_specification_grouping.py` and new `tests/fixtures/specification-grouping-defaults.json`. [patch-files.json](patch-files.json) records before/after hashes.
- R1 fix in `overlay/devicehunt/__main__.py`: table headers include shared `raw_fact` when formatting changed the value. a same-record/two-DMI-values regression verifies distinguishable headers and both raw values once; existing curated shared-statement-once test remains.
- R2 portable tests: no private overlay guards, legacy imports or `originals/` paths. four fixed default API/JSON/CSV/table snapshots were generated from hash-verified original production modules, rather than from the new implementation. [fixture provenance](default-fixture-provenance.json), [private generator](generate_defaults.py), [fixture](tests/fixtures/specification-grouping-defaults.json).
- [cli_checks.py](cli_checks.py): help assertion normalizes whitespace. actual child CLI probes also verify distinguishable DMI raw table groups, complete JSON/CSV evidence, unchanged fixture bytes, missing DB failure and help at widths40/80/120. [CLI proof](cli-checks.json), [log](cli-checks.log) report PASS.
- a separate [normal-layout/](normal-layout/) contains209 frozen production/test files plus the applied patch, without private `overlay/` or `originals/`. [dry-run log](normal-layout-patch-dry-run.log) and [application log](normal-layout-patch-application.log) show clean application.

already-started checks and handles at checkpoint:

- overlay focused suite session69656 completed exit0. [focused-tests-result.json](focused-tests-result.json) and [log](focused-tests.log) retain the actual49-test result (12new+37existing). no additional grouping test was started after the pivot.
- normal-layout focused suite session75634 was still running at the original checkpoint. on the coordinator's explicit request, its existing handle was subsequently polled to terminal: **exit0,49passed,0failures/errors/skips/blocked calls**. [normal-layout-tests-result.json](normal-layout-tests-result.json) and [normal-layout-tests.log](normal-layout-tests.log) preserve its actual outcome. no restart or additional grouping check occurred. no grouping process remains running.
- default generation session29563 and CLI session71279 completed exit0; no other grouping process remains running.

original implementation and review artifacts were read/copied, not changed; all writes were confined to this new grouping-fix directory. [inputs-before.json](inputs-before.json) pins209 production files and334 protected implementation/review files. the focused runner independently checks the209 production hashes before/after its run. **a final complete334-file protected-artifact check and final artifact SHA256SUMS were not run before this requested checkpoint**; do not claim those final checks. no live DB queries, network, installs, delegation, import/rebuild/export or shared-state writes occurred.

next action is the coordinator's broader datasource/bulk-collector assignment. grouping can remain parked with this concrete patch. if resumed later, independently review the patch/fixture semantics and finish provenance verification before considering any production application. wave5 promotion and an explicit production-freeze release remain required.
