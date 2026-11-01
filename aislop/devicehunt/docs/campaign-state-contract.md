# campaign state and recovery contract

Accepted c0 design gate, 2026-10-04, after independent GPT-6.1 Sol high-effort
[review](../research/batches/campaign-pilot-2026-10-04/contract-review.md).
Implements design decisions from the
[reviewed campaign plan](campaign-plan.md); this file specifies behavior; c1 and
offline c2 implement the parts noted below, and live-transport gates remain open. [Batch and ownership](../research/campaign-pilot-2026-10-04.md).
The accepted hardware catalog remains unchanged. This contract gates c1a/c1b;
c2/c3 interfaces below constrain later work without claiming it is complete.

## scope and pinned pilot

Use campaign ID `rra-ised-saved-2026-10-04`, definition version 1. The c0 inventory
is derived from the exact `rra` and `ised` jobs of the pinned accepted manifest,
including their recursive declared dependencies and company-name evidence.
Evidence pins, accepted checkpoints, implementation/policy references, and local
measurement times are separate categories. Neither mtime nor current runtime
supplies a missing historical acquisition/extractor timestamp.

The initial execution mode is offline, request quota **zero**. The pilot's new
metadata/candidate-output cap is 64 MiB with a 2 GiB free-space floor; the pinning
measurement records available disk. Existing registered source bytes do not count
as new allocation and are not copied. Every write must check the applicable cap;
overflow checkpoints instead of omitting a dependency or deleting old data.
Integration DB copies need a separate space assignment, outside this small cap.

Register full query snapshots, not a new all-country scope. Source unit IDs are
the actual accepted importer source IDs; immutable unit revisions bind exact
query fields and input-manifest hashes. Inventory occurrences retain manifest
job ordinal, source file hash and row/page ordinal. They are not claim identities.
Two occurrences sharing bytes or certificates remain distinct observations.
For c2 saved registration use verified existing locations (`copy=False`). The
current `Collector.register_saved` copies through `store_bytes`; adapting that
behavior is required before replay, not permission to duplicate the pinned input
set. Existing accepted object copies remain untouched historical locations.

## authority and storage

| store | authority and mutation rules |
| --- | --- |
| campaign definition/inventory | immutable scope, limits, policy/code/input pins; changing one creates a new revision |
| `data/archive/capture-leases.sqlite` | extend the existing operational DB; durable budget admission, lease fencing, stops, pending attempts and explicit control decisions |
| archive receipt/object files | immutable evidence of attempts, results and derivations; old IDs, wrappers and byte paths stay valid |
| `data/archive/index.sqlite` | disposable indexed projection plus recoverable pending publication journal; never the only surviving copy of a published receipt |
| `data/archive/index-head.json` (implemented; present in the live archive) | small mutable operational generation/sequence marker for detecting index rollback or unfinished commit; not accepted evidence or a release pointer |
| accepted manifests/catalog | only the exclusive integrator writes these after review/rebuild; campaign execution never does |

No database per source/run, no scheduler service, no receipt-path sharding in c1.
Operational schema/version belongs to a new `devicehunt/campaign_state.py`; it
does not change `devicehunt/catalog.py` or the hardware catalog version. Any
later accepted-catalog table change still belongs in `catalog.py` and its rebuild
and comparison routing. c2 (2026-10-05) implemented the offline `devicehunt.campaign` commands
and operational schema 3 run/job/execution checkpoints; live-transport gates remain open.

## c1a: admission, budgets and restrictions

Add constrained operational tables, preserving existing `leases(key,owner,expires)`
values and key strings. Minimum logical records:

| record | required fields/invariants |
| --- | --- |
| operational metadata | schema version, archive identity, restore/recovery state; unknown newer schemas refused |
| budget account | immutable account ID, definition/policy hash, request ceiling, response-payload-byte ceiling, deadline/window and stopped state |
| reservation | UUID, account/run/unit revision, request fingerprint, source/resource/host scope, attempt receipt ID, max body allocation, owner/fence, state, immutable start observations and optional known response status |
| lease fence | existing lease key, monotonic generation, owner token; adjunct record preserves legacy row contents on migration |
| restriction/control event | append-only event ID, deny/throttle/resume/cancel kind, exact scope, observed status, provenance, eligible time, reason and decision identity |

Runs in c2 reference the same budget account when resumed or replayed. Creating a
different run ID cannot replenish that account. A future refresh cycle requires a
new explicitly bounded account allocation with recorded scope/policy; it does not
clear any source restriction. Host limits and the campaign account are checked
in the same short admission transaction. Count committed reservations, including
uncertain ones, against the request ceiling. Repeated use of a reservation UUID
is idempotent metadata access, never permission for another HTTP dispatch.

Reserve worst-case body allocation (`max_bytes + 1` for the over-limit detection
byte) before HTTP. This is a bound on bytes read from response bodies, not total
wire/TLS/header bytes. Initially keep this conservative allocation charged even
when fewer bytes arrive; record actual observed/stored bytes separately. Uncertain
attempts never free quota. No implicit refund, quota increase or deadline extension.

There is no parallel legacy live path: every nonzero-budget `Collector` dispatch
must use an explicitly allocated durable account and the upgraded publication
protocol. `Policy.budget` alone cannot create/replenish an account. Until both c1a
and c1b pass, nonzero legacy transport is disabled with an actionable error;
budget-zero saved-only use remains compatible. Tests allocate isolated fixture
accounts explicitly. Every writable archive API uses the same publication lock
and intent protocol; an old helper cannot bypass it with a private write path.

Required sequence for new campaign attempts:

1. Validate the frozen request descriptor, source restrictions, archive readiness,
   offline flag and capacity. Acquire request/host locks and check admission.
2. Commit a reservation and its quota charge, with a predetermined attempt receipt
   ID. No network or archive-file publication occurs inside this SQL transaction.
3. Publish and index the sanitized attempt receipt, referencing that reservation.
4. In a short transaction recheck stop/policy/deadline/ownership state and mark
   dispatch admitted. If validation fails, stop with the reservation still charged.
5. Dispatch once. The dispatch-admission transaction is the control linearization
   point: a later stop forbids subsequent admissions but cannot undo in-flight HTTP.
6. On receiving response headers, persist known status and any deny/throttle
   restriction **before reading/validating/storing the body**. The production
   transport must expose this event; mocked transports must exercise it too.
7. Save any retained bytes, publish a result with response/validation/storage
   outcomes distinguished, then mark the reservation complete and checkpoint.

States are `reserved → attempt_published → dispatch_admitted → response_observed
→ complete`; exceptions may checkpoint `uncertain` from any nonterminal state.
Not every state proves HTTP was sent. A reservation lacking a receipt, an attempt
lacking a result, or a dispatch admission interrupted before/after the call is
uncertain, charged and ineligible for automatic redispatch. Reconciliation may
finish from verified durable bytes/receipts; otherwise an explicit decision keeps
it stopped or authorizes a distinct bounded attempt if source policy permits.
Legacy attempts stay legacy; never invent past reservations or successful calls.

Any 402/403 remains a denial even if its body is truncated, unreadable or fails
publication. Any 429/503 remains a host throttle under the current source policy.
Body errors add diagnostics; they cannot downgrade these to generic `invalid`.
If operational persistence itself fails after observing headers, no further
network work is allowed; the already committed uncertain reservation causes the
same fail-closed stop after restart. Do not rely on publishing an error body or
result receipt to remember a restriction.

Denials preserve exact URL identity plus known source/resource identities from
the adapter and saved policy evidence. A URL change cannot route around the same
denied item; do not guess resource equivalence from a filename. Host throttle and
broader documented source restrictions are independent scopes. A throttle resume
event names the exact triggering event(s), host, policy revision, eligible time,
reviewer/decision identity and remaining quota. It does not delete history or
clear denial events. A later throttle needs a later explicit resume decision.
Saved unavailable results stay ineligible for automatic retry under current rules.

## migration, exclusion and clocks

Migration is explicit and transactional after quiescing old collectors. Preserve
all legacy lease values and restriction evidence. A legacy owner cannot be shown
safe merely because its wall-clock expiry is past: active owners block migration
until stopped, or an explicit recovery decision is recorded. New schemas reject
old unsupported mutation paths; old collectors must not run beside new writers.
Keep one small verified pre-migration operational backup, not another catalog DB.
Read-only opens never create, migrate, recover, checkpoint or delete files.

Use OS advisory locks on supported Linux/macOS hosts in addition to SQL fences:
one archive-publication writer lock and deterministic request/host lock files.
Do not unlink lock files while processes can hold them. If locking is unavailable,
fail rather than silently use an expiry-only fallback. Request/host locks stay held
through the network attempt, while SQLite transactions remain short. Acquire keys
in a fixed order; never wait for an OS lock while holding an SQL transaction.
Archive publication uses its own short lock after admission, not a lock across HTTP.
Two runners may be refused or serialized; they do not concurrently publish index
changes. A live OS lock cannot be stolen because a wall-clock lease expired.

Every release/finalization checks owner and generation; an old owner cannot clear
a replacement lease or finalize its work. Within a process use monotonic time for
elapsed limits. Record wall and monotonic observations for clock-anomaly detection;
uncertain jumps/restart/restore block time-based resume until reconciled. Wall time
alone is never proof that another process is dead or a retry is now eligible.

## c1b: indexed lookup with recoverable publication

The first upgrade/recovery is an explicit full read of existing receipts under
exclusive ownership. Validate immutable wrapper IDs, bytes, paths and registration
consistency; import known safety outcomes into operational restrictions before
network can be enabled. Publish a new index generation and matching head marker.
Do not run full reindex/audit on every normal `Archive` construction afterward.

All new receipt publications use the single-writer protocol:

1. Acquire publication lock, compare index generation/sequence with the head
   marker and recover any earlier pending intent. No caller bypasses this step.
   Before making a new intent, resolve an existing stable receipt ID and validate
   its original wrapper. If kind and canonical payload match, return that exact
   frozen wrapper/time without a new intent or sequence. A different payload is
   a conflict. Do not generate a replacement timestamp for repeat registration.
2. Commit an intent containing the exact frozen wrapper bytes/hash, receipt ID,
   expected previous sequence and next sequence. Time/ID are frozen here.
3. Publish the immutable receipt atomically, with file and directory durability.
   A same-ID conflict is accepted only for identical bytes; otherwise stop.
4. In one index transaction materialize the wrapper and all lookup rows, advance
   sequence, and mark the intent indexed. Keep the intent until acknowledgement.
5. Atomically publish/fsync the matching `index-head.json` generation/sequence.
6. Clear the acknowledged intent. Only then report publication complete.

Recovery repeats these steps idempotently under the lock. Intent/no file publishes
the frozen wrapper; file/no indexed row validates and indexes it; indexed/no head
advances the head; head/no acknowledgement clears the intent. Required artifact
bytes must exist and validate before completing a referencing receipt. An object
published without a registration receipt remains an orphan to report/reconcile,
not a valid input or something to delete automatically.

A missing/corrupt index, index behind its durable head, mismatched generation or
unexplained sequence is not a cache miss. Normal opens refuse mutation/dispatch and
require explicit recovery/reindex. Read-only queries may report `needs_recovery`
without changing anything. Pending critical publication or unreconciled attempt
blocks new dispatch. A same-generation index ahead of its head is recoverable only
through its intact intent; no guessed adoption of arbitrary receipt files.

The supported threat model is cooperating exclusive writers and explicit backup
restoration, not undetectable arbitrary external file replacement. Restore always
marks the store quarantined and performs full reconciliation before HTTP; restoring
both an old index and old head while newer receipts exist must not be treated as a
normal open. Losing the operational DB cannot reconstruct spent quota exactly from
receipts (reservations may precede receipts); restore it or keep acquisition blocked.

Index receipt kind/ID, capture request/resource/host and result order, derivation
recipe/input fingerprint, and restriction references. Point lookups must use these
indexes and verify only selected immutable wrappers/artifacts. Multiple capture
attempts sharing request identity remain separate.
Legacy registration time does not rank an old/unknown observation ahead of a new
network capture. Cache eligibility requires adapter-declared compatible request
context and observation scope; unknown saved context supports offline replay,
not automatic live reuse. New attempt ordering uses a durable admission sequence
within that scope, not an assumed monotonic wall clock.
Restriction lookup reads the
operational safety state, including known status whose body/result never published.
Audit/reindex and explicitly requested whole-history enumeration may scan all files;
ordinary open/cache/derivation lookup may not. Receipt count equality is not used
as a consistency test. Selected release validation stays independent of unrelated
pending artifact integrity in c3.

## request metadata and source adapter boundary

Keep conservative exact URL, method, body hash, representation context and an
explicit session scope in request identity. Persist only sanitized header metadata:
case-insensitively exclude Cookie, Set-Cookie, Authorization, Proxy-Authorization
and source-declared credential headers. Apply this to attempts, results, failures,
policy diagnostics and legacy registration wrappers. Existing original captures
stay byte-identical and may be referenced by hash/locator; do not rewrite them.
If an original report contains sensitive headers, new receipts link it rather
than embedding those values. Tests scan persisted metadata, not immutable originals.

ISED execution URLs/form bodies remain exact source context. They are session-bound
and cannot authorize cache reuse across sessions. Cookies remain in memory; an
interrupted live session starts new eligible setup steps under quota, not a guessed
resume with old tokens. Saved ISED snapshots can be replayed offline with their
unknown request context explicitly labelled; they are not live session cache entries.

RRA's before/after exports are separate observation slots with separate reservations
and actual requests when live. The identical POST must not return the earlier cache
entry as the second observation. Preserve the bundle's declared comparison mode,
including exact certificate-keyed fields with row-order changes allowed where stated.
Detail requests come only from validated CSV certificate IDs and approved endpoints;
derive an exact finite allowlist within preset caps. No automatic increase from the
number of newly discovered rows. Missing details cannot publish a complete bundle.

## later c2/c3 contract constraints

The c2 adapter boundary is `units(definition)`, `register_saved(unit)`,
`next_work(unit, checkpoint)`, `validate(unit, saved_results)` and
`bundle(unit, validated_results)`. Offline c2 implements these responsibilities in
`devicehunt/campaign_sources.py` (`RraSavedAdapter`, `IsedSavedAdapter`) and
`devicehunt/campaign.py` (`Runner`); live descriptors through shared capture are c2b. Work descriptors carry unit revision, source/resource IDs, step and
observation-slot IDs, method/exact URL/body reference, validator/recipe identity,
session/reuse policy and all limits. No adapter calls curl, urllib or browsers
outside shared capture. Checkpoints bind unit/config/adapter/session identities;
incompatible checkpoints fail without silently restarting with fresh budget.

Job identity binds unit revision + stage + recipe; execution IDs are separate.
A result receipt is durable before a checkpoint advances. On restart, a result
published before a checkpoint crash completes that same job without another
network observation. Raw occurrence IDs bind unit revision + artifact + selector;
candidate entities and accepted claims have separate stable IDs and review history.

Coverage fields distinguish declared units, successfully observed units, validated
complete units, partial/failed/blocked/unknown units, observed upstream totals and
whether that denominator is known. Accounted-for work includes failure; successful
coverage does not. Explicit zero includes its source proof. Query stability is not
worldwide coverage or a modification feed. A revalidation projects only a complete
original query/date scope; narrower overlapping probes stay observation-only.

Freeze coverage as a versioned immutable JSON manifest with definition/input pins,
unit revision/outcome/observation references, selected receipt payloads and their
required artifact closure, unknowns and an optional evidence-release hash. Compute
its identity from canonical contents excluding its own identity. Validate references,
acyclic receipt dependencies and every required artifact before exclusive publish.
All-failed output can have zero artifacts/claims and is labelled coverage-only.
It does not go through today's nonempty-artifact lineage validator or assert device
absence. Mixed/explicit-zero outcomes are separate fixture cases. The evidence
release freezes selected reviewed roots and closure, never the whole growing archive.

Candidate source imports precede lineage projection validation in an isolated DB.
Review gates complete touched-source projections or a deterministic tested accepted
selection; it does not re-review the whole historical catalog. Preserve old accepted
claims and explicit withdrawals/conflicts. Only the exclusive integrator can promote
after independent review, full tests and separate offline rebuild/comparison.

## implementation ownership and acceptance tests

c1a owner: `campaign_state.py`, capture admission/transport callbacks, and affected
tests; c1b follows with `artifacts.py`, indexed capture/derivation queries and tests.
One owner edits shared `capture.py` at a time. Do not migrate live acquisition state
until isolated migration/recovery tests pass and an integration assignment is named.

| ID | acceptance test |
| --- | --- |
| Q1 | two processes/different hosts share quota one; restart/new collector cannot increase admitted dispatches |
| Q2 | cumulative worst-case byte/deadline limits stop admission despite legal individual requests; uncertain spend never resets |
| Q3 | fault each reservation/receipt/dispatch boundary; zero HTTP without committed reservation and published attempt; no automatic uncertain retry |
| R1 | 402/403/429/503 then oversize/read timeout/object-write/result-write failure; durable restriction or uncertain stop blocks subsequent HTTP |
| R2 | explicit throttle resume respects scope, remaining budget and trigger events; it never clears denied resources or new throttles |
| L1 | two processes compete for writer/request/host locks; clock jumps do not steal a live lock; old fence cannot release/finalize new owner |
| M1 | legacy lease values survive atomic migration; interruption rolls back or completes; unknown schema/read-only missing DB fails without mutation |
| I1 | fault each intent/file/index/head/ack boundary; recovery preserves exact wrapper bytes and no critical pending state permits dispatch; repeated legacy/saved/recipe stable IDs reuse frozen timestamps without a new sequence, conflicting payloads fail |
| I2 | missing/corrupt/rolled-back index/head and restored operational state fail closed; explicit rebuild preserves all legacy receipts |
| P1 | mixed-case cookie/credential headers on success/error/saved-report paths never enter new persisted metadata; session cache entries stay separate |
| S1 | normal open and point lookup use no full directory scan; instrument file reads/globs and benchmark real read-only plus declared larger synthetic set |
| S2 | two compatible mocked network captures have backward wall times; later legacy registration cannot supersede the later admission sequence, unknown saved context cannot satisfy live reuse, and offline saved replay still works |
| A1 | RRA after-export cannot reuse before; reordered fields pass only declared comparison; zero, missing detail and changed totals keep distinct outcomes |
| A2 | ISED duplicate/reordered rows, zero, stale tokens and HTML–CSV discrepancies preserve exact rows and no guessed aliases |
| C1 | result-before-checkpoint crash resumes logically once; two offline runs add no redundant capture/derivation, while explicit reuse events are allowed |
| E1 | all-failed/zero/mixed coverage manifests validate honestly; missing selected dependency fails, unrelated pending corruption does not poison selection |
| N1 | socket/HTTP/subprocess network entrypoints disabled in all tests/offline jobs; real source and accepted-state hashes unchanged |

Q/R/L/M/P gates belong to c1a; I/S plus indexed reuse belong to c1b. A/C/E are
later c2/c3 gates, not prerequisites falsely reported as already passing c1.
Use real multi-process quota/lock tests, deterministic fake clocks and injected
faults. Run affected tests during implementation; full suite before combined code
acceptance and whenever importer/catalog schema behavior changes. No network,
package installation or system configuration changes are required.

**Design stop:** independent review must accept the inventory, state transitions,
publication recovery and test assignment before c1 implementation. c0 produces no
new device assertions and does not change the accepted manifest or original bytes.
