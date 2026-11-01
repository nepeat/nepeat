# corpus stage helper — graceful spin-down checkpoint

2026-10-15 UTC. owner `/root/network_bulk_full`; user explicitly requested stop
when quota reset arrived. **draft only: unreviewed, untested, not executed.**

existing implementation: [stage.py](stage.py), SHA256 `be749d9b5eb929b07452e800588654048595f987acace4b0f0b80ca28d99f4ad`.
no accepted plan, test file, test result, stage report, candidate database,
manifest or complete70-entry bundle has been created. root has **not staged or
promoted breadth**. all writes for this task were confined to this directory.
production/shared code, catalog, manifest, original captures, existing complete
61-entry bundle and exports were untouched. no network/install/delegation or
publication occurred. no active tool/process handles remain.

read inputs already consulted: root's `edgecore-corpus-review/review.md` and
`reextraction.json`; `edgecore-bulk-documents/handoff.md` and accepted additions;
wave5 and breadth stage patterns; reference_documents/documents/rebuild and
campaign gate modules. additions acceptance SHA256 is
`ac3d89415b6036f0617bc9e6789dddc3a1dce20083e5c52f4998601969229534`.

helper intent (not validated): require a root-authored externally hash-pinned
accepted plan after breadth promotion; bind the post-breadth live pair, promotion
receipt, code freeze, existing complete bundle, exact accepted additions, review,
reextraction and owned baseline; append exactly9 entries/40 pages to the complete
61-entry/3730-page collection; copy new18 sibling inputs without overwrite;
validate complete70 entries/3770 pages; stage a new candidate backup/import and
manifest; preserve all old logical rows and old document text, with only expected
sha256/fetched_at/cache_path changes on the61 existing bundle sources. intended
catalog delta: +9 sources/+9 documents/+40 pages, zero devices/records/facts.
expected global document totals after stage:81 documents/3844 pages. root must
independently verify all of these claims against the implementation.

planned interface: `python3 -B stage.py --plan ROOT_PLAN --plan-sha256 PLAN_SHA`.
**do not invoke it yet.** plan schema currently expects `schema_version:1`,
`accepted:true`, scope `accepted-edgecore-text-corpus-only`, `helper_sha256`,
`live_hashes`, pinned path/hash objects `breadth_promotion`, `code_freeze`,
`old_bundle`, `additions`, `review`, `reextraction`, `baseline`, complete six-table
`expected_before`, and `outputs` keys bundle/candidate_database/candidate_manifest/
report. no populated plan or plan example has been authored.

exact resume step: after explicit resumption, inspect this draft and complete
isolated fixture probes/tests (none have run, including syntax/import checks),
then have root independently review helper+plan contract. only after breadth is
accepted/promoted may root pin its actual new live hashes/counts and promotion
receipt in an accepted plan. actual corpus stage/rebuild/combined acceptance is
later work. no promotion helper is authored or requested here.
