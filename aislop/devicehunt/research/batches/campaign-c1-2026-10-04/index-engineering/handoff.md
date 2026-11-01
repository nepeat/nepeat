# c1b archive core handoff

author `campaign_index_engineer`, 2026-10-04. application ownership stayed within
artifacts/archive/derivations and their assigned tests. [interfaces](interfaces.md)
is the API basis; [checks and file hashes](checks.json) freeze this coherent core.
independent reviewer `campaign_contract_review` found the previously raised core
blockers resolved. combined state integration/full-suite acceptance belongs to
`/root`; no live archive migration or production dispatch has occurred.

implemented: exact-byte receipt intent, immutable publication, transactional index,
durable head and acknowledgement; idempotent prefix recovery; recoverable explicit
generation replacement; new-only create; legacy readonly audit; explicit legacy
upgrade and missing/corrupt/rolled-back index rebuild. corrupt indexes retain a
SHA-addressed forensic copy. valid unsupported newer versions are refused.
stable IDs reuse original wrappers and do not consume another publication sequence.
objects without registrations remain reported orphans. newly created object-prefix
directory entries and mutable head replacement receive parent-directory fsync.

normal opens validate bounded metadata/head and never scan/reindex. handles bind
their generation; another reindex requires an explicit reopen. selected proof
checks only known required artifact/receipt closure, excluding code/config/request
hashes. indexed capture/policy/safety/derivation lookup rechecks the selected
projection against immutable wrapper bytes. derivation reuse uses recipe plus
ordered-input fingerprint and verifies outputs before reuse.

`publication_guard` uses campaign-state's shared publication flock and can be
acquired before operational SQL admission. proof/readiness never acquire that
lock or write operational state. archive code never writes the operational DB.
state imports exact generation-bound safety references separately; core readiness
alone does not authorize HTTP or imply intact quota authority.

the shared safety envelope adds `selector` to the original note's fields.
`safety_row(wrapper, selector)` performs canonical legacy adaptation;
`safety_status(row)` conservatively recognizes restrictive legacy error codes even
beside a nonrestrictive status. core fixtures cover legacy `http_status=403` and
contradictory `status=200/error=403`. these helpers are pure, with no filesystem,
lock or state side effects.

39 affected tests pass: artifacts 8, indexed recovery 17, derivations 4, archive
release 3 and lineage 7. real processes crash after every receipt protocol boundary,
compete for one stable ID, and prove rebuild/writer exclusion. fixtures also cover
intent/phase contradictions, corrupt/missing/rolled-back indexes, bad/missing heads,
newer schemas, generation recovery, selected projection corruption and lock-free
readonly verification. the old manually bypassed publication race fixtures were
replaced by public-API process tests preserving their original receipt-retention
and frozen-winner intent.

[bounded benchmark](benchmark.json): 5,005 synthetic receipts, 1,901,207 logical
bytes; normal open reads zero wrappers, capture lookup one, derivation lookup plus
proof three. all make zero directory scans and use SQL indexes. the real legacy
archive has 374 receipts/146 artifacts, was opened strictly readonly, and its
index/operational database hashes stayed unchanged. timings are local observations,
not an environment-independent performance guarantee. the benchmark explicitly
boots a temporary fixture, then uses normal indexed APIs; it never upgrades live
data. crash fixtures do not simulate physical power loss; durability relies on
supported local POSIX fsync and SQLite FULL-commit semantics.

remaining coordinator gates: freeze peer capture/state integration, independently
review combined safety/admission/restore/reconciliation behavior, run the full
offline suite, verify protected hashes and update practical/current documentation.
future live migration must be a separate explicit integration assignment.
