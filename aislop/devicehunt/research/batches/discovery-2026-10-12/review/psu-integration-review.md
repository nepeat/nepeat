# PSU integration independent review — 2026-10-12

final verdict: **ACCEPT the reviewed candidate pair for coordinator promotion**. reviewer `/root/bytedance`; coordinator `/root` is sole integrator. final checkpoint 2026-10-13 03:54:31 UTC. no stage/import/rebuild/promotion execution by this reviewer, no network/installs/shared-state writes. candidate and manifest inspected read-only. this report is frozen for the hash-bound promotion receipt.

reviewed staging script hash: `ba2fd43eb795f642c592e9f1eacf8d513e6f65c5c32efaa8002c164f64254e7e` (`integration/stage.py`).

the script preserves all 50 original reference entries byte-for-byte as JSON values and adds the five reviewed PSU PDFs to a new 55-entry bundle. existing PDFs/text remain siblings beside the new bundle. the previous manifest/files remain intact; the reference import job switches to the new complete bundle and one curated job is appended. 18 new hashed inputs close the new dependencies: curated JSON, six source snapshots, five PDF/text pairs and the complete bundle. no old capture is replaced. new destinations are exclusive/new; authoritative catalog and manifest are hashed against the accepted baseline, read/copied read-only and rechecked at completion.

expected deltas are correct: **+3 identities, +6 records, +42 facts, +11 sources, +5 PDFs and +63 pages**, reaching 5,115 / 7,236 / 74,171 / 2,087 / 66 / 3,779. old reference source bundle hashes legitimately change on all 50 original reference sources; the script requires exactly that source-ID set and exact old/new bundle hashes rather than ignoring arbitrary source-hash changes. source-preservation comparison excludes the bundle hash only after this separate guard. old ordinary rows and their multiplicities are checked, excluding volatile timestamps/cache paths and FTS internals; candidate/rebuild comparison must still verify FTS.

the existing campaign gate stays unchanged. `gate_inputs` runs before candidate imports and `gate_projections` after; no campaign-source input is replaced and no new campaign release is needed. integrity/foreign keys, exact expected counts and owned baseline are checked. the two recommended guards are now present: hard pin of final curated candidate **2c55934a430e423c861f04e107e121ff801e9a36e32985daa032308eab4ef756**, and exact baseline summary `total=10, hits=10, gaps=0`.

independent pre-stage checks passed:

- all 50 original entries' PDF/text dependencies are already declared with matching hashes in the accepted manifest.
- six proposed source snapshots match candidate original hashes; six rows contain 42 facts; source IDs do not collide with live sources.
- all five revised sidecar/PDF hashes match r2 report, and `read_capture` validates **63/63 pages** under the honest Poppler method/version/options.
- current live read-only counts equal the accepted 5,112 / 7,230 / 74,129 / 2,076 / 61 / 3,716 baseline.
- facts/roles/visual evidence are covered by the independent PSU fact review. reviewer `/root/psu` has independently verified the cosmetic source-coverage deduplication and bound final acceptance to 2c559… before staging.

## interrupted audit and read-only recovery

the first stage imported into its isolated candidate, then failed ATTACH because Catalog's connection had URI handling disabled. this connection limitation was missed in the initial static review; `stage.py` and its traceback log are retained unchanged. the coordinator's separate `finalize_stage.py` opens both candidate and attached live catalog with explicit `mode=ro, uri=True`, reruns hash/input/bundle/gate/preservation/count/baseline checks and creates only new manifest/report outputs. it performs no import and requires unchanged candidate/live byte hashes. finalizer reviewed hash: `1c5c119bc48e02812d2cfbc9162eed060a0e0f9bd7869f0254e86d03c0b1bfeb`.

## staged candidate proof checked independently

`integration/stage-report.json` hashes match the actual files: candidate catalog `53a9a0eabaef5656b38ad922f96d963b76839eeda2da3b28a18b22359d857dee`; candidate manifest `6ab359e22929bdb6a55e47c2a816c2c78fc3e5d0f90c8b252129f8fafea3aac5`. read-only comparisons confirm all old manifest pins/gate preserved, exactly the reference job replacement plus one curated job, **146 jobs / 3,928 inputs**, and exactly 18 newly hashed files, all matching disk.

independently reran complete `prepare` (55 entries), gate input/projection checks and candidate counts. integrity is ok and FK violations zero. owned validation is **10/10**, with **nine document snapshots plus one label photo**; ByteDance remains **0/2**. the five new reference documents persist the Poppler method and total 63 pages. additionally reran SQL multiset preservation across **all 50 ordinary tables** and the explicit source-hash comparison: zero missing old groups and exactly 50 permitted old-reference bundle-hash changes, matching the report. accepted live byte hashes still match the baseline at this review checkpoint.

## full-suite and publication helper review

full guarded suite proof `acceptance/discovery-2026-10-12-poppler-psu-suite-result.json` and `integration/full-suite.log` agree: **769 passed, zero failures/errors/skips/blocked calls, 916.163 s**. independently rechecked all **111** code-under-test hashes; unchanged.

reviewed `promote.py` and `refresh_exports.py` without executing them. promotion requires accepted/hash-bound review, candidate pair, passed suite, unchanged tested code, 52-table comparison, explicit candidate/rebuild validation paths, stage counts, unchanged accepted live pair and absence of live journal/WAL/SHM files. backups and complete pending copies use exclusive creation; a saved journal precedes two per-file atomic replacements. those two replacements are not a filesystem-wide atomic pair; both old backups and the journal provide recovery if interrupted. no re-import occurs. the final acceptance receipt will bind the exact reviewed proof/helper/rebuild hashes.

export refresh opens Catalog read-only, verifies the promoted DB hash, refreshes standard and three exact-PSU reports, checks the baseline and confirms the database hash is unchanged. its final preservation refinement exclusively creates `integration/exports-before/`, byte-copies each existing overwritten export and records original hashes before refresh; inspected without execution. reviewed helper hashes: promote.py `3284ad73e8ab081bbf65a5078d55cb539b618424c8d156222d00f2905b149bbb`; refresh_exports.py `a7009c64d370189a2bfddce00f5cf02b0c3d23419771616d6b387705b3522f84`.

## final clean-rebuild acceptance

clean offline rebuild completed with **3,928 verified inputs**. `integration/compare.log` reports all **52 tables/indexes and baseline results match**. independently checked the comparison JSON: exact row counts and zero directional multiset differences on every table, including `search_index`/`document_search`; all six staged counts match. validation proof explicitly names the reviewed candidate as reference and `data/rebuilt-psu-2026-10-12.sqlite` as rebuilt output, with 10/10 baseline equality. source bundle hashes are retained in this comparison; only normal timestamps/cache paths are excluded by the comparison tool.

final candidate/manifest hashes remain the accepted pair above; live catalog/manifest remain the original baseline hashes; all 111 tested code hashes and both reviewed helper hashes still match. no unresolved material finding remains. final evidence SHA-256:

- clean rebuild: `ff5832c60de0d4fbc94a3c1de10157bcad1935c0f5c6b7874e3e1f16cb773fd4`.
- full-suite proof: `c8f96195efa30bb46d1bdda822a6412968b636d6b920916256b5b44bc921095b`.
- code-under-test.json: `b4eca3e430b3a485b456cc1e58bb5c07ae0c7974a41f301cae2114d7884e288f`.
- stage-report.json: `0eb4cdf09fbe1bef681a383616ca777fec9a8175cc3b465344f810d035c30f7c`.
- 52-table comparison: `0b10d8f97023d9d63eea75938d83b7c27e20f868beb0f260ca0f21d09720a391`.
- comparison validation: `e6ae319c48189666dc883bfba40feafc5b5cbd364bbaccf31ee14c5af4022d50`.

coordinator may now bind this frozen report and the exact proof/helper hashes, preserve the old live pair, promote validated copies and refresh preserved exports. perform required post-promotion gate checks and documentation updates afterward. this acceptance establishes source-scoped catalog facts and preserved prior evidence, not current certification/support, unit provenance, chassis compatibility or resolution of the ByteDance gap.
