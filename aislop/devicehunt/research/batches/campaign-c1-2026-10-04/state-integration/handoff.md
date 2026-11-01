# c1b state/capture integration handoff

owner: `campaign_state_engineer`, GPT-6.1 Sol high. 2026-10-04.
status: coherent implementation and affected offline tests pass; frozen for
independent review and coordinator combined acceptance. c1a engineering artifacts
remain unchanged. no live archive upgrade, account activation or HTTP occurred.

owned paths: `devicehunt/campaign_state.py`, `devicehunt/capture.py`,
`tests/test_campaign_state.py`, `tests/test_capture.py`, and the coordinator's
minimal compatibility expansion `research/fetch_fcc_sitemaps.py`. the index
engineer owns artifacts/archive/derivations; no overlapping writes occurred.
all state, captures, backups, activation records and recovery tests were isolated
under temporary directories. protected data/source/manifest/export paths remained
read-only. [final tests and code hashes](test-results.json).

## state and admission

Operational schema **2** adds exact archive-generation/safety-sequence binding,
reference IDs with receipt/wrapper-hash/publication-sequence, indexed event scopes
and resume-trigger links, account activation records and reservation transport mode.
existing quotas/reservations/events remain safety authority, never reconstructible
from archive evidence alone. old catalog schema is unchanged.

`CampaignState.initialize(root)` is a new-authority-only action. before creating
its DB it publishes the immutable `capture-authority.json` UUID/identity marker.
after SQL schema commit it publishes immutable `capture-authority-ready.json`.
normal/read-only operational opens validate both against SQL authority identity.
a lost DB cannot be recreated even when reservation commit preceded every receipt.
missing/invalid/unfinished marker or seal fails closed. no quota reconstruction or
implicit allocation recovery is provided.

Explicit `migrate` preserves original legacy lease rows; explicit
`upgrade(root, quiesced=True, decision=...)` upgrades accepted schema1 state to2
without reallocation. each retains one verified pre-migration backup. migration
markers bind the exact backup hash; transaction interruption permits retry with
the same binding. the immutable ready seal forbids completed migration from being
replayed with that old snapshot after spend. restoring operational authority must
use a coherent backup and explicit quarantine/reconciliation; no reset or generic
unquarantine API exists. arbitrary external/double-state restoration is outside
normal-open detection and must follow the documented explicit restore protocol.

`reserve(..., archive=archive, transport_mode='mock'|'production')` and
`transition(..., archive=archive)` acquire `Archive.publication_guard()` **before**
operational SQL, then perform readiness, safety, ownership and receipt checks.
this prevents a generic receipt writer from introducing a restriction between
safety validation and dispatch admission. the guard ends before HTTP. receipt proof
and archive readiness themselves acquire no OS lock or operational transaction,
so publication -> state lock ordering is retained.

`archive.verified_receipt` supplies acknowledged exact immutable-wrapper proof,
including required artifact closure. state self-calls it, verifies semantic
reservation/attempt/result/owner/fence/status fields, and rejects forged transition
edges or conflicting status outcomes. original request/host OS locks remain held
through dispatch and result completion. stale owner/generation cannot finalize or
release a replacement holder. every unfinished/uncertain reservation still blocks
fresh reservations globally, including a newly allocated account.

## safety synchronization and activation

`synchronize_archive(archive, decision=..., policy_hash=...,
allow_generation_change=False)` explicitly imports indexed references and binds
exact generation/checkpoint/ref IDs/hashes. legacy sequence0 references are included
on initial/generation synchronization. `Archive.safety_row` and `safety_status` are
shared pure interpretation helpers: legacy `http_status` selectors and restrictive
error text alongside nonrestrictive raw statuses are preserved conservatively.
a reference cannot be acknowledged without interpreted restriction handling.

Normal admission may advance the checkpoint only across new references proven to
match an existing durable observed reservation/status. a generic wrapper with
additional source-wide scope or contradictory status/error is unresolved; it needs
explicit synchronization and its own durable restriction. new generic saved/policy
references block HTTP until synchronization. repeat/generation synchronization does
not resurrect already-resumed restrictions from the same reference. known captured
results acknowledge existing observed status without reissuing a throttle event.

Restrictions are queried through SQL indexes on scope/target and event triggers;
no full event/receipt directory scan participates in ordinary capture. exact URL,
source-scoped resource, broader documented source and canonical hostname stops stay
separate. hostname grouping spans all allowed schemes/ports, preserves exact URLs,
rejects malformed/userinfo URLs and never infers DNS aliases.

Production needs `require_production_ready`: acknowledged modern archive, exact
safety binding, eligible immutable account and generation/policy-bound activation.
`activate(archive, account_id, policy_hash=..., decision=...)` records a **separate
coordinator decision for an already authorized finite live scope**; it makes no
request and is not a blanket bypass. reindex generation change invalidates old
activation even after safety synchronization. cancellation, uncertainty, pending
publication and unknown references continue blocking admission. constructor and
capture reject implicit nonzero-budget accounts and ambiguous injected transports.

**Only synthetic temporary accounts were activated** to exercise the positive
readiness branch, with network guards and a mocked native HTTP opener. no live
archive/data account was activated; this batch does not authorize collection.

## recovery and capture

`reconcile_result(id, archive=..., result_id=..., decision=...)` holds fresh
request/host recovery OS locks, then publication -> SQL, verifies acknowledged
attempt/result/bytes, advances recovery fences and completes that existing charged
reservation. frozen attempt ownership/fences, wall times, admission sequence and
quota remain literal. no HTTP, refund, new attempt or frozen-wrapper rewrite occurs.
ambiguous/missing result remains stopped. the account stop remains after successful
reconciliation. `resume_account(archive, account_id, decision=...)` is a distinct
reviewed decision for an interruption whose work is fully reconciled and whose
original quota/time limits permit further work. cancellation and transport-failure
stops are not cleared by that helper.

Capture no longer wraps public archive writers in a second publication flock or
scans receipts at construction/cache/policy lookup. it delegates to
`capture_candidates`, `policy_candidates` and indexed safety references. mock/live
cache excludes unknown saved context; source/session/observation scopes remain
exact. admission sequence orders modern captures despite backward wall times;
later unknown saved registration cannot supersede them. saved-only replay remains
possible. repeated policy wrappers now use stable content IDs and frozen timestamps.

Conditional request headers are built **before attempt publication**. attempt
metadata records the sanitized actual wire headers, `wire_request_id`, explicit
prior-capture conditional context and validator context. reusable base request ID
stays separate, so a refreshed 304 remains visible to subsequent ordinary reuse.
response status remains durable before any body read or native-header normalization.
all repeated HTTPMessage credential values are gathered before collapse; credential
header filtering and diagnostic-only fragment redaction preserve structural IDs,
hashes, exact URLs, representation metadata and original body bytes.

The existing saved FCC adapter now creates only an absent archive root; it opens
modern existing roots normally and fails on legacy/damaged roots rather than
resetting them. repeated saved entry preserves identical receipt bytes/sequence.
this is compatibility only; in-place registration and campaign adapters remain c2.

## validation

```
python3 -m unittest tests.test_campaign_state tests.test_capture
```

**52 tests pass in 33.047s** (34 campaign-state and18 capture tests).
syntax checks pass for the five owned Python paths. all tests retain c1a Q/R/L/M/P
fault/multiprocess/redaction gates, with new coverage for:

- reserve-before-receipt DB loss and immutable authority reset rejection;
- completed legacy migration -> spend -> exact old-backup replay refusal;
- schema1->2 interruption rollback and accounts/reservations/events preservation;
- generic safety-reference gating, exact ref/hash/checkpoint synchronization,
  legacy selectors, contradictory statuses and broader source scope;
- known-result acknowledgement and nonresurrection of resumed throttles;
- positive **fixture-only** activation, mocked native transport and generation
  invalidation/reviewed reactivation without further HTTP;
- acknowledged result reconciliation preserving charges and frozen attempts,
  followed by a distinct reviewed account-resume decision;
- backward clock capture ordering, late saved context, offline replay and separate
  before/after observation slots;
- ordinary open/cache/dispatch without glob/rglob/iterdir or unrelated receipt reads;
- exact conditional wire-header attempt evidence and 304/base-cache compatibility;
- repeated saved FCC adapter entry and missing/corrupt existing-index refusal.

Socket connections, urllib opener and subprocess entrypoints are guarded; mocked
native opener intentionally substitutes the guarded boundary. spawned workers use
explicit mocked transport and disjoint temporary state, with no inherited flock
ownership. core publication/index/recovery tests and S1 larger-corpus measurements
belong to the index engineer. final combined full suite, independent acceptance,
protected hash proof and practical documentation belong to `/root`; they are not
claimed by this engineering test result.

Remaining limits: restoration cannot reconstruct unknown lost/rolled-back spend;
normal open cannot detect a user externally replacing every operational marker and
DB coherently with an older snapshot. such restore requires quarantine and coherent
authority reconciliation. an allocation interruption after SQL commit but before
ready-seal publication remains explicitly blocked; no automatic seal repair/reset
is provided. macOS without shared boot identity remains conservative for timed
resume across processes. this code gate adds no live source transport validation,
c2 adapter, c3 selected-closure release or claim assertion.
