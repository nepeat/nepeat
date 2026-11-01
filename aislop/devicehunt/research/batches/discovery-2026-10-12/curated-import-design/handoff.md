# curated importer design handoff

outcome: **design complete, production unchanged**, ready for independent design review. author `/root/handoff_audit`; only this directory written. no implementation, network, live imports/writes, large benchmark or rebuild. six tiny temporary catalogs used for explicit compatibility probes.

read `design.md` for exact proposed implementation/file scope and meaningful checks. supporting artifacts:

- `accepted-input-profile.json` / `profile_inputs.py`: all48 curated jobs in held accepted manifest `6ab359e22929bdb6a55e47c2a816c2c78fc3e5d0f90c8b252129f8fafea3aac5`,1,853 declared sources,2,201 row occurrences; input hashes verified; source/value/optional-field/duplicate/snapshot profiles. all accepted snapshots present; four source snapshots omitted legitimately; two zero-row sources.
- `probe-results.json` / `probes.py`: A→B orphan-move metadata regression, duplicate declaration/key first-identity/last-record behavior, ignored row/revision behavior, distinct fact/identifier conversions, caller-commit side effect and empty-source no-op.
- `production-code-sha256.json`: read-only code identities; importer/catalog match coordinator's original atomicity-probe hashes. no production edits.

recommendation: pure full-input preflight and trusted snapshot metadata preparation; one bundle savepoint; **retain per-source orphan cleanup/count updates to preserve successful identity title/kind order**, defer only FTS/commit; one final reindex. literal final-cleanup-only optimization is disproved by a tiny fixture.

compatibility priorities: no raw/value normalization, no role/tier/content policies or uniqueness rules, no activation of ignored revision, no new import/rejection of unreferenced incomplete rows, preserved repeated source/key counts and ordering, no changes to default finish behavior for other importers. prepared snapshot metadata must come from real local reads, not ignored payload hash fields.

proposed transaction-contract improvement is explicit: standalone success commits bundle; existing caller transaction remains caller-owned on success/failure, with savepoint rollback affecting only importer work. legacy success committed unrelated caller work. independent reviewer/coordinator should confirm this deliberate contract choice before implementation authorization.

proposed production scope: `devicehunt/importers.py`, `devicehunt/catalog.py`, new `tests/test_curated_import.py`; no pipeline/CLI/rebuild/schema/data/manifest/export changes. preserve original code bytes before later authorized edits. input-profile refresh, affected/full guarded tests, independent review and clean accepted-manifest rebuild/preservation checks belong to implementation/integration, not this design milestone.

stop condition met. `/root/bytedance` notified that design is stable for independent review; coordinator decides acceptance/implementation ownership afterward.
