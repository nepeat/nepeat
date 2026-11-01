# curated import atomicity — bounded design

status: **design only; implementation not authorized here**. author `/root/handoff_audit`. production files were read, not edited. all probes wrote tiny temporary catalogs; only this design directory persists. no network, live writes, rebuild or benchmark.

## confirmed problem and compatibility evidence

the coordinator's `curated-input-probe/result.json` demonstrates a two-source bundle where source 1 commits, then source 2's missing model fails; caller rollback cannot restore source 1. `curated()` calls `Catalog.finish()` once per declared source; finish updates count, deletes orphan identities, rebuilds all FTS and commits. pipeline simply dispatches; CLI rollback arrives too late. offline rebuild avoids publishing failed staging files, but direct imports remain partially committed.

`accepted-input-profile.json` covers **every curated job in held accepted manifest** SHA `6ab359e22929bdb6a55e47c2a816c2c78fc3e5d0f90c8b252129f8fafea3aac5`, observed after PSU promotion. manifest bytes stayed unchanged during profile:

- 48 jobs, 1,853 source declarations, 2,201 device-row occurrences; all 48 JSON input hashes match manifest. 1,849 sources have snapshot strings; every referenced snapshot exists. four sources omit snapshot and must remain valid. two declared sources intentionally have no rows.
- all current source required fields and consumed identity fields are strings; affiliation lists are present on every row. optional identifier/fact/hyperscaler arrays may be absent. all entries have current pair/triple arity. all accepted fact/identifier values are strings, including **three empty fact strings** which current add skips.
- no repeated source ids, repeated source/key pairs or unreferenced rows occur within these files; no curated source id repeats across the held manifest. their absence does **not** justify new uniqueness rules.
- source extras `sha256`, `original_sha256`, capture/final-URL metadata are currently ignored by curated; row extras include nested evidence/raw tables, reviewed cells/pages, numeric indices and scope limitations. top-level candidate/review/scope/unresolved fields are ignored. retain these semantics, not a fixed allowed-key list.

`probe-results.json` records six tiny legacy fixtures:

1. **final-cleanup-only changes successful output.** moving M1 from source A to later source B yields new title/kind under legacy per-source cleanup. deferring orphan removal to bundle end retains old title/kind. keep per-source orphan cleanup; reject that optimization.
2. repeated source declarations and same-key rows are accepted: two declarations × two rows returns 4; final source records_seen 2, one surviving record, last source metadata/last raw record, but first inserted identity title/kind. no sorting/deduplication or count reinterpretation.
3. row revision stays in raw_json but is not passed to add; device revision remains default empty. an unreferenced row with missing model/affiliations is ignored.
4. facts use `value is not None and str(value)` and preserve Python `str()` representation, including 0/False/empty list/dict. identifiers use truthiness before str conversion, skipping those false values. don't replace these with JSON serialization, new string-only validation or unified skip logic.
5. legacy finish commits a caller's unrelated pending work. the new transaction contract should fix this deliberately, preserving caller transaction ownership rather than that unsafe side effect.
6. empty sources is a no-op returning 0, even when devices is absent; no new cleanup/reindex/commit should occur.

## recommended implementation

**keep per-source cheap cleanup/count updates; use one bundle savepoint and one final FTS rebuild.** a literal single final orphan cleanup is incompatible with current successful identity metadata ordering. the newest coordinator instruction prioritizes preserving that ordering over the cleanup optimization.

1. add a pure full-input preparation helper in importers.py. load JSON once; keep original dictionaries/order for raw_json. prepare **all declared sources and all matching rows** before any database mutation. diagnostic ValueErrors name input/source/row/field for malformed consumed structures; I/O errors propagate normally.
2. validate root/source/row containers, required consumed fields, pair/triple arity, existing operation/bindability requirements, model's string API and truthy snapshot path shape. preserve empty text, exact identifiers/case/suffixes, arbitrary role/tier names, URLs without new scheme rules, numeric/scalar SQL coercions where they previously succeeded and all `str()` fact/identifier semantics. do not introduce finite-number/content restrictions incidentally.
3. validate only consumed portions: require a row's source selector when sources are present, then use the same equality/filtering semantics as today. unreferenced rows remain ignored rather than imported/rejected because their other fields are incomplete. absent optional arrays retain default empty; extra/unused fields (including revision) remain raw and ignored. don't let hashing an unreferenced list/dict source selector invent a new TypeError. empty-source no-op must return before demanding unused devices.
4. pre-read every referenced snapshot and capture its current hash/mtime/cache-path metadata **before writes**. do not newly trust/require payload `sha256`/original_sha256 fields, restrict snapshots to sibling names or normalize their stored paths; this generic importer currently supports nested paths. reuse the trusted prepared metadata for Catalog.source so execution doesn't read a later different snapshot or duplicate all I/O. a small optional `prepared_snapshot` keyword backed by a Catalog metadata helper is appropriate; ordinary Catalog.source callers retain exactly their old default behavior. metadata must come from local reads, never be passed directly from JSON.
5. begin a fixed-name SAVEPOINT after successful preflight. iterate source declarations and row occurrences in their original order, including repeated ids/keys. source replacement/add behavior stays unchanged. extend finish with keyword-only `reindex=True, commit=True` defaults; curated calls finish with both false **while retaining count update and orphan deletion at each source boundary**. then call reindex once. no schema changes or global transaction-mode switch.
6. release the savepoint on success. a savepoint started outside a caller transaction commits the whole bundle on release; nested release leaves caller work uncommitted. on any execution/reindex/release failure or interruption, roll back the savepoint and release it, then propagate the original exception. never roll back or commit caller work outside that savepoint. keep release inside the protected block; cleanup must not obscure an original exception if SQLite itself fails.

source order determines identity first-writer metadata and source replacement; input row order determines last same-key record. preserve the returned row-occurrence count and source records_seen counts, not distinct SQL row counts. don't enable the previously ignored row revision, sort raw dictionaries, merge roles/values, canonicalize brand names or rebuild unrelated document FTS.

optional array validation should preserve skipped values: false company means no affiliation is inserted; null/empty fact value and false identifier value may prevent other tuple fields from reaching SQL. don't turn formerly ignored tuple fields into new metadata policies. unexpected SQL/programming failures still roll back atomically; preflight need not guess every constraint dependent on current database state.

## precise file scope

implementation proposal: **devicehunt/importers.py**, **devicehunt/catalog.py**, new **tests/test_curated_import.py**. Catalog changes are limited to trusted snapshot metadata preparation/reuse and deferred finish flags with unchanged defaults. no pipeline/CLI/rebuild/schema/manifest/database/export changes are required. existing tests are read-only dependencies unless review identifies a concrete needed transfer. preserve original production bytes before editing this untracked project.

## meaningful tests and acceptance

- reproduce the coordinator's late missing-model bundle: preflight fails before any source/add/finish write; old full logical state survives. later missing/unreadable snapshot likewise mutates nothing.
- inject late SQL error, add programming error, final reindex error and interruption after earlier source work: all source/device/record/fact/identifier/role/FTS state restored without caller rollback; error propagates. no catch converting programming/I/O errors into source outcomes.
- success leaves standalone connection committed; nested success remains in caller transaction and caller rollback removes both its own work and imported rows. nested failure preserves prior pending caller work; separate reader visibility changes only when caller commits.
- preserve the A→B metadata-move case, first identity versus last duplicate raw record, repeated-source metadata/row counts, duplicate pair INSERT OR IGNORE behavior, zero-row sources, unrelated records and empty-source no-op.
- preserve facts/identifier None/empty/0/False/True/list/dict/number conversion and different skip rules, optional missing arrays, raw nested extras, ignored revision and unreferenced incomplete rows. schema validation should pass every currently accepted curated input via pure preparation, without importing them.
- instrument reindex/commit traces on a tiny multi-source fixture: one final FTS rebuild/bundle boundary; per-source cheap orphan/count work preserved. compare final FTS rows/search behavior to legacy with fixed observation timestamps. no large benchmark needed: statically, current curated jobs could reduce **1,853 FTS rebuilds to 48**, not a measured runtime claim.
- run affected curated/catalog/rebuild/listing/specification/gate tests, then full guarded suite because this is an importer change. independent review before promotion; clean accepted-manifest rebuild into a new path and logical comparison, preservation/counts/baseline/integrity/FK/FTS checks remain coordinator integration work. rerun the pure input profile/preflight against the then-current accepted manifest; this profile is point-in-time, not a permanently frozen catalog.

stop condition met: concrete design, all accepted inputs profiled and compatibility edge outcomes recorded. next: independent design review, then explicit implementation ownership/authorization after current production freeze is lifted.
