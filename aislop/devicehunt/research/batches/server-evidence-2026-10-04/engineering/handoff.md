# buying-report engineering handoff

outcome: complete and frozen pending independent QA. author: assigned engineering worker.

production change: only explicit `cmvp_tested_configuration` fact-name membership in
`firmware_boot_software`. no prefix/value-keyword matching, field splitting,
alias/role/schema/import changes. two dated E4-2C environments stay literal,
including with/without PAA and `(single-user mode)`.

new test uses the sixteen literal candidate facts in a temporary local fixture.
it verifies exact two statements, full context equality including module version,
Historical status, reason, caveat and both history rows; record-local
`tested_platform_vendor` role, regulatory tier and provenance; all other categories
remain not_collected. module facts remain unclassified/full context. other Oracle
model hardware and other-vendor same-model configuration/current-OS facts do not
leak through the exact-model Oracle-filtered report. short model/configuration
text is not an inferred alias. no source data/import/shared DB/manifest/export or
project documentation writes were made.

checks: focused tests 11/11 pass; full offline suite 428/428 pass.
`test-results.json` records exact bundled runtime commands and exit codes;
`focused.log` and `full-suite.log` retain complete output. deleting only the new
membership in memory makes the new regression test fail at its exact statements
assertion, with no fixture errors (`mapping-removed-regression.log`). code on disk
remained frozen; after hashes verified after this negative check.

proof: before byte copies `before-buying.py`, `before-test_buying.py`;
`before-hashes.json`; actual unified `change.diff`; `after-hashes.json`.
self-review of diff confirms just the explicit membership/comment and one test.
reviewers should compare frozen hashes before running their independent checks.

next bounded step: independent code/test QA; coordinator alone accepts/integrates
source data and updates current documentation. no further engineering edits until
review findings are assigned back to this owner.
