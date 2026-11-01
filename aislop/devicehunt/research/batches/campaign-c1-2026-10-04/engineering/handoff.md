# c1a engineering handoff

owner: `campaign_state_engineer`, GPT-6.1 Sol high. 2026-10-04.
status: implementation and affected offline tests complete; coordinator acceptance
and independent review pending. code frozen for review. no c1b/live work started.

owned changes: `devicehunt/campaign_state.py`, `devicehunt/capture.py`,
`tests/test_campaign_state.py`, `tests/test_capture.py`, this engineering directory.
accepted sources, catalog, manifests, exports and existing archive were read-only.
all mutable fixtures used temporary directories. no installs or real HTTP.

## implementation and API

`CampaignState(root)` opens existing safety authority without creating/migrating it.
`readonly=True` permits observation only. missing/corrupt/newer/mismatched stores
fail closed. `initialize(root)` explicitly allocates an empty new authority only;
it refuses existing receipts so loss of an authority cannot silently replenish
quota beside surviving evidence. `migrate(root, quiesced=True,
recovery_decision=...)` is explicit legacy migration, under an OS publication lock.
all original lease key/owner/expiry values survive; any owner, including an expired
one, needs a recorded decision. a verified `capture-leases.before-c1a.sqlite`
operational backup is retained. schema changes and metadata commit together;
injected interruption rolls them back. no live migration was performed.

`allocate_account(id, definition_hash, policy_hash, request_limit, byte_limit,
deadline=None, window=None, host_limits=None)` freezes allocation. repeat identical
allocation is idempotent; differences fail. new run/collector IDs do not replenish
it. request and worst-case body bytes (`max_bytes+1`) are charged at reservation
commit, with no refund from short responses or uncertainty. host ceilings are
checked in the same transaction. deadline/window use wall plus monotonic/boot
observations; ambiguous clock movement or epoch change blocks timed admission.

`acquire(keys, owner)` holds sorted OS advisory locks and writes SQL generations.
`release(fences, owner)` checks instance ownership and SQL generation; stale release
cannot unlock a replacement holder. expiry alone never steals locks or old rows.
`recover_lease(key, decision=...)` requires an available OS lock and records an
explicit decision; releasing a stale lease does not reconcile a reservation.
lock files remain in place. symlink roots/ancestors/lock directories fail before
writes. this is cooperating exclusive-writer protection, not external adversarial
filesystem replacement detection.

`reserve(...)` requires exactly the held `request:<fingerprint>` and canonical
`host:<hostname>` fences, owner and generation; supplied host must match URL.
reservations record UUID, account/run/unit/request/source/resource, owner/fences,
frozen wall/monotonic/boot observations, predetermined attempt ID and durable
admission sequence. repeat UUID dispatch is rejected; `reservation(id)` is the
idempotent metadata-access API. **any unfinished or uncertain reservation blocks
new reservation globally**, including a different account. this deliberately
conservative initial sequential pilot does not introduce a parallel scheduler.

`transition(id, owner, fences, expected, next_state, ...)` accepts only the c1a
edges `reserved -> attempt_published`, `attempt_published -> dispatch_admitted`,
and `response_observed -> complete`. state-owned immutable receipt verification
proves attempt/result identity, reservation metadata, attempt fences, known status
and referenced artifact integrity. completion byte observations must match the
result. dispatch rechecks deadline, cancellation/stops and restrictions.
`observe(...)` is the separate atomic `dispatch_admitted -> response_observed`
operation, committing status and scoped restrictions before body read.
`uncertain(...)` is owner/fence-checked and retains all quota; reservation states
before its update already block admission after a crash.

`record_restriction`, `ingest_legacy_status`, `restrictions`, `resume_throttle`,
`cancel` and `quarantine` are the safety/control API. legacy ingestion is atomic
across exact URL, supplied source-scoped resource and explicit broader source
restrictions. saved reports are ingested before publishing their policy wrapper.
new 402/403 records exact URL plus known source-scoped resource; 429/503 records
host throttle. resume names the complete active host trigger set, policy, decision,
eligibility and remaining account/host quota. it preserves denials and cannot
clear subsequent throttles. no generic retry or uncertainty clearing API exists.

Canonical host safety scope is the lowercase hostname across schemes and **all
ports**, with bracketed IPv6. this conservative scope was accepted by coordinator
and reviewer; default-port spelling cannot bypass quota/throttle. exact URLs stay
request/evidence identity. unsupported schemes, userinfo and invalid ports fail;
there is no DNS or alias inference.

`Collector` retains budget-zero saved-only compatibility without creating an
operational DB or allocating quota. any nonzero collector requires explicit
`state`, `account_id`, `mock_transport=True` and an explicitly supplied transport.
using the production transport, omitting explicit fixture admission, or trusting
`Policy.budget` as allocation fails with the c1b readiness error.
`request_transport` must accept keyword `observe_response(status, final_url,
headers)` and invoke it before body work; missing/duplicate observations fail.
production transport itself demonstrates this ordering, including HTTPError
headers, through mocked opener tests, while collector production dispatch remains
disabled. denial/throttle outcomes survive oversize, timeout, object/result failure
or interrupt. exceptions without complete result remain charged/uncertain.

New attempts/results carry reservation ID and admission sequence for c1b ordering.
source/session/observation scope is bound into request/cache identity. unknown
saved context is eligible for offline replay but not mock/live compatible cache.
session policy requires an explicit ephemeral session ID; cookies remain only in
wire/in-memory handling. `reuse_policy='never'` supports distinct observation slots.
native HTTPMessage headers normalize after status commit and retain every repeated
sensitive value for diagnostic redaction before duplicate collapse. short cookie
fragments cannot rewrite structural hashes, IDs, URLs or representation headers.
new receipt metadata filters mixed-case Cookie/Set-Cookie/Authorization/
Proxy-Authorization and source sensitive headers, including nested legacy wrappers.
known credential fragments in diagnostic strings are redacted; transport/validator
errors use exception type, avoiding arbitrary credential echoes. original body
bytes remain unchanged. stable saved receipt IDs and their frozen timestamps retain
the existing archive idempotence behavior.

## verification

final affected command:

```
python3 -m unittest tests.test_campaign_state tests.test_capture
```

**42 tests passed in 18.750s** (25 campaign-state + 17 existing/adapted capture
checks). final `python3 -m py_compile devicehunt/campaign_state.py
 devicehunt/capture.py tests/test_campaign_state.py tests/test_capture.py` passed.
fixtures disable socket connection, urllib opener and subprocess entrypoints.
spawned workers explicitly inject mock transport; no inherited flock descriptor
is used to demonstrate cross-process exclusion.

| contract gate | proof tests |
| --- | --- |
| Q1 | spawned processes/different hosts share one account; exactly one dispatch; restart/new run remains exhausted; immutable allocation |
| Q2 | cumulative conservative bytes, deadline/window, host caps; timeout/uncertain charge; fake wall jump and boot change |
| Q3 | ten reservation/attempt/admission/transport/result fault boundaries; state/receipt proof; missing callback; cancellation at admission; fresh-account pending stop |
| R1 | 402/403/429/503 x oversize/timeout/object/result/interrupt matrix; operational persistence fault; restart + URL/resource change cannot dispatch |
| R2 | exact trigger set, wrong policy, time/clock eligibility, quota, new throttle, retained denied resource and broader source restriction |
| L1 | spawned request/host/publication contention despite wall jump; stale generation cannot release/finalize replacement; symlink lock boundary |
| M1 | exact live/expired legacy rows and key strings, verified backup, interrupted migration rollback, explicit recovery, missing/corrupt/newer/read-only stores |
| P1 | mixed-case request/response/error/saved/policy metadata, source-sensitive headers, bearer/cookie fragments, original bytes, session-separated reuse |

Existing capture checks retain PDF/empty/error distinctions, cache-integrity failure,
304 conditionals, no redirect acceptance, stable saved IDs, exact fingerprints,
saved restrictions and post-lock cache race behavior. timeout fixtures now use
separate authorities for independent oversize checks because uncertainty cannot be
bypassed by allocating a fresh account. test expectations no longer permit expiry
to steal a held lease or unknown saved request context to satisfy live reuse.

Full suite and protected live-data hash proof belong to coordinator acceptance;
they have not been represented as engineering results here.

## c1b integration boundaries and remaining limitations

Production gating is unconditional in c1a. c1b must replace it with state-owned
archive/index/head/publication readiness checks at **both reservation and dispatch**,
including legacy safety migration and restored/lost-authority quarantine. an
in-memory boolean or policy budget is insufficient. explicit fixture mocks must
stay visibly separate from that production protocol.

`reconcile_result(reservation_id, result_id=..., decision=...)` is an explicit
**gated stub**, raising `StateError` without mutation. c1b must verify the frozen
attempt/result/required artifacts against recovered indexed publication, acquire
recovery fences, finish that same charged reservation, and reconcile safety stops
without HTTP, quota refund or new observation. no raw SQL workaround is supported.
uncertain requests without a verified result remain stopped for explicit policy
review. c1a does not claim crash result/checkpoint recovery is already implemented.

`Collector._receipt` and `_store_bytes` presently hold a short publication flock
around legacy `Archive` calls for mocked c1a fixtures. once `Archive` owns locking,
these wrappers must delegate without reacquiring an independent flock. collector
legacy receipt scans likewise become explicit c1b recovery/indexed safety lookups.
ordinary writable `Archive` APIs still have legacy behavior in this phase; no
production network is enabled until every publication path is upgraded.

State receipt proof currently opens `Archive(readonly=True)` while holding a short
operational transaction. c1b readonly readiness must not acquire publication lock
and invert the publication -> state lock order. moving verified IO outside SQL
transactions with a post-read ownership/state recheck is a possible c1b refinement.
operational restriction lookup currently enumerates append-only events; c1b should
index exact scopes and trigger references for point lookup. ordinary capture/cache
lookups still scan legacy receipt wrappers pending c1b, so no S1/I1/I2 pass is claimed.

The new operational schema is local to campaign_state, not catalog.py. no catalog
schema/importer/rebuild change occurred. Linux flock/boot behavior was exercised;
macOS without a shared boot identifier conservatively treats epoch as process-local,
so timed resume across a restart requires future explicit reconciliation. saved
registration still copies through the existing archive object API; c2 owns verified
in-place registration. no c2 adapters, coverage, claims or live readiness are claimed.

Final code pins and final affected-test result: [test-results.json](test-results.json).
Production gating is rechecked at capture entry, so replacing an explicit mock
transport with the production function after construction still cannot dispatch.
