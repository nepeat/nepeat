# curated import design — independent review

reviewer: `/root/bytedance`; reviewed 2026-10-13 04:18 UTC. verdict: **ACCEPT design**, with the transaction contract explicitly confirmed by the coordinator. this accepts an implementation proposal, not implemented behavior or a live import. no production files, accepted inputs, databases or manifests were changed by this review.

## evidence binding

reviewed root's `../curated-input-probe/probe.py` and `result.json`, author's `../curated-import-design/design.md`, profile script/results and probe script/results, and the current `curated`, `Catalog.source/add/finish/reindex` implementations.

final checked SHA-256 values:

| item | sha256 |
| --- | --- |
| design.md | `19472fcbe664ca0b598f64172e5bdd2001ebef91df475242201723be29c938a4` |
| accepted-input-profile.json | `612057a5477c1dc9ecc1def0a9864773a7073eed558a532e1a8e88a2f72ad848` |
| probe-results.json | `f02183bbf9b9a0b63f6e47800ed2dcfbc25ae3451be14aaf02678a5d6182b7e2` |
| devicehunt/importers.py | `99e5051a5c09c0abee1606e2127bea88e7539ca65b704f503b44ccb7d3ba7d51` |
| devicehunt/catalog.py | `2c5731c8a1fbac7f7c6f3d57284cb0ae3448cd26277c5e1446e83f054192c8ef` |

i independently verified all 48 profiled curated input hashes against both disk and the held accepted manifest `6ab359e22929bdb6a55e47c2a816c2c78fc3e5d0f90c8b252129f8fafea3aac5`, and checked the totals: 1,853 source occurrences and 2,201 row occurrences. this is the post-PSU, pre-wave3 profile; implementation acceptance must rerun pure preparation against the then-current manifest. the older live hash in root's initial bug probe is historical evidence, not a claim about the current live catalog.

## material conclusions

the proposal fixes the demonstrated late-source partial commit by preparing the whole consumed input and snapshot metadata before writes, then enclosing mutation plus final FTS rebuild in one savepoint. default `Catalog.source`/`finish` behavior for other importers stays unchanged.

retaining **ordered per-source orphan cleanup** is necessary: deferring it to the final boundary changes first-writer identity title/kind when a model moves from an earlier source to a later one. counts and source replacement must remain per occurrence; repeated sources/keys cannot be sorted, deduplicated or recounted from surviving SQL rows. one final device FTS rebuild is compatible with that ordering. the 1,853-to-48 reduction is a static call-count opportunity, not a measured benchmark.

preflight must retain original dictionaries for raw_json, ignored optional metadata/revision, empty-source no-op, and equality-based source selection. malformed fields in an unreferenced row remain ignored except for the source selector the legacy filter consumes. fact and identifier conversion/skip rules differ and must stay distinct; false affiliation companies can make other tuple fields irrelevant. pair/triple checks must not introduce new string-only, URL, uniqueness, finite-number or snapshot-path policies. snapshot preparation must use locally read bytes/stat metadata rather than payload hash fields and preserve stored path spelling.

the coordinator explicitly confirmed the deliberate transaction contract fix: **nested successful imports remain uncommitted and caller-owned**. legacy successful finish committed unrelated pending caller work; preserving that side effect is not required. standalone savepoint release commits the bundle. execution, final reindex, release errors and interruptions must restore only importer writes, preserve pending caller work, propagate the original error and avoid hiding it behind cleanup failures.

an independent tiny temporary-catalog savepoint emulation verified these transaction claims: late failure restored old records/title and preserved an unrelated pending caller row; nested success remained invisible to a second reader until caller commit, and caller rollback restored the old state. this checks the proposed SQLite sequence, not future importer code. no persistent database was created.

## implementation acceptance requirements

ownership is limited to `devicehunt/importers.py`, `devicehunt/catalog.py` and new `tests/test_curated_import.py`. no schema, pipeline, manifest or live-data edit is needed. preserve original production bytes before implementation.

the design's proposed tests cover the material risks: late malformed row/snapshot with zero writes; injected SQL/programming/index/interruption failures with full logical rollback; standalone and caller-owned success/failure; per-source move/duplicate ordering and counts; raw extras/ignored revision/unreferenced rows; truthiness/stringification; zero-row and empty-source cases; one final FTS rebuild and legacy successful output equality. include protected release and unchanged defaults for non-curated callers. full guarded tests and independent implementation review remain required, followed by coordinator-owned clean rebuild, preservation, baseline, integrity/FK and FTS comparisons before promotion.

no remaining design correction blocks implementation. stop condition met; implementation is subject to the coordinator's separate authorization.
