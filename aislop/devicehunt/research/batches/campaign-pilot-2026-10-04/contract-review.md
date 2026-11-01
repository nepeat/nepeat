# independent campaign contract review

reviewer: `campaign_contract_review`; date: 2026-10-04. coordinator owns the
contract and fixes. this reviewer owns this report only. scope is the offline c0
inventory and c1 design gate, not implementation acceptance or live collection.
all source captures and databases were inspected read-only; no network, installs,
imports, export refreshes or further delegation were performed.

status: **accepted for a separately assigned c1a/c1b implementation**. final
contract and inventory review passed, including the coordinator's b1 fix.
no implementation, live collection or live-state migration approval is implied.

## initial findings and must-settle decisions

1. **durable quota and uncertain requests.** `capture.py:85,194–208` replenishes
   an instance-local budget on every collector construction, subtracts before
   receipt publication, and stores no committed reservation. two different hosts
   share no admission key. specify the persistent campaign/run/host budget scopes,
   nonrefundable uncertain spend, dispatch eligibility, and restoration behavior.
   an attempt receipt is not proof that a request was sent, and a missing result
   is not proof that it was not sent. preserve legacy attempts without invented
   reservation links. reservations without receipts must block fresh dispatch
   until reconciled, rather than becoming free quota.
2. **cross-store publication and stale safety state.** `artifacts.py:143–153`
   publishes immutable receipt bytes before indexing. `Archive.__init__:114`
   performs full reindex and audit on each writable open; `capture.py:_rows` and
   `derivations.py:56` repeatedly scan receipt files. removing these scans needs a
   durable intent journal plus an exclusive writer protocol, not a file-count
   comparison. define recovery for an intent without bytes, published bytes
   without an index row, an index row without completion acknowledgement,
   immutable ID collision, missing/corrupt index, and restored state older than
   receipts. no stale denial lookup may authorize dispatch. read-only status must
   fail or report stale state without replay, migration or file creation.
3. **lossless lease migration and clock fencing.** `capture.py:88–118` has one
   `leases(key, owner, expires)` store; request/host keys and active foreign owners
   are already meaningful. choose compatible migration and schema rejection
   behavior before adding operational tables. a wall-clock forward jump must not
   reclaim a live request. an expired writer heartbeat alone must not let a
   second publisher run. preserve owner-checked release after transfer, and define
   conservative restart with uncertain requests instead of automatic retries.
4. **status restrictions before risky body work.** `capture.py:211–243` stores
   response bytes before deriving outcomes. oversized 429/503 is labelled
   `invalid` and misses the throttle stop. an uncaught archive error/interrupt may
   lose a newly observed 402/403/429/503 entirely. oversized 403 still has its
   status and remains blocked if the result receipt successfully publishes; the
   gap is failure before publication. define immediate durable restriction
   observation and conservative failure ordering. source/resource restrictions
   must survive URL, session and run changes; exact URL denial and host throttle
   scopes must be preserved. resume a throttle only with a saved eligible policy
   decision and remaining quota; never clear a denied resource in that operation.
5. **metadata redaction and session identity.** `capture.py:195–198,211` records
   supplied request and response headers literally. define case-insensitive
   credential-header filtering for attempts, results, historical metadata,
   failures, restriction diagnostics and request descriptors. exact original
   saved bodies remain evidence. do not equate redacted request identity with
   cache equivalence. ISED form/export steps require an explicit ephemeral session
   scope; stale execution IDs/tokens do not resume a workflow. redirect targets,
   form actions and source-derived URLs need validation before each dispatch.
6. **stable source/occurrence/claim identity and release boundaries.** current
   RRA source IDs hash the full query (`rra.py:173`) and record keys use certificate
   plus exact model (`rra.py:208`). ISED query IDs normalize company spelling
   (`ised.py:249`) and its importer preserves row-level occurrences. freeze these
   actual source semantics; do not use snapshot row positions as durable claims.
   a complete exact-scope revalidation replaces that source only after review;
   overlapping probes remain observations. cross-query certificates are separate
   observations, not independent corroboration. `archive.freeze` currently audits
   and includes the entire registry, and `lineage.prepare:77` rejects empty
   artifacts. c3 must select validated closure and use an independent receipt-only
   coverage manifest for all-failed campaigns. the complete proposed projection,
   or a deterministic accepted-claim selector, must gate source replacement.

7. **legacy API bypass.** require all nonzero-budget `Collector` dispatch and all
   writable `Archive` APIs to enter the same durable admission/publication
   protocol. legacy live transport cannot continue resetting instance budgets
   beside the campaign path. preserving a budget-zero saved-only compatibility
   path does not authorize live bypass. cover direct constructor/API use, not
   only the future campaign CLI.

## executable acceptance criteria for the implementation handoff

these are tests to require, not tests run by this design review.

| gate | fixture/action | required assertion |
| --- | --- | --- |
| shared request cap | two processes reserve against a quota of one on different hosts; reconstruct collectors and restart | at most one admitted dispatch; durable spent/uncertain total never decreases |
| shared byte/time caps | multiple individually legal requests consume cumulative limits; restart before next admission | per-request success does not bypass cumulative admission; exhaustion checkpoints without deleting evidence |
| attempt ordering | inject faults before reservation, after reservation commit, before/after attempt publication, before/after dispatch marker and transport | no transport without committed reservation and published attempt; uncertain units remain spent and cannot automatically retry |
| response restriction | return 403/429 with oversized bodies; fail body/object/receipt publication; interrupt after response status | restriction or conservative uncertain stop survives restart; zero subsequent forbidden transport calls |
| result/checkpoint ordering | publish result then crash before job/checkpoint commit; crash after checkpoint commit | replay derives the same completed unit and raw occurrence set, without a new observation or dispatch |
| writer exclusion | two publishers compete; old heartbeat expires while first holds writer lock | second refuses/serializes; it never writes an immutable receipt or index concurrently |
| pending publication | faults at object publication, receipt publication, index commit and intent acknowledgement | isolated restore/recovery gives matching immutable wrappers/index; no pending safety state permits HTTP |
| migration | existing legacy lease DB with live/expired owners and known key strings; interruption during migration | row values survive exactly; rollback or fully versioned migration; old-owner release cannot delete a new owner's lease |
| clock changes | forward/backward wall-clock changes and process/boot changes around leases and resume | no overlapping live request; early eligibility is not inferred from a jump; uncertain state stays checkpointed |
| missing/corrupt stores | remove/corrupt index or operational DB in fixture copies | ordinary read-only open creates nothing; normal writer refuses until explicit recovery; lost budget/restriction authority never auto-resets |
| redaction | mixed-case Cookie/Set-Cookie/Authorization/Proxy-Authorization plus source-declared sensitive header in success, 403, malformed, timeout and saved metadata paths | persisted metadata contains no credential values; literal approved bodies remain unchanged; sessions are cache-separated |
| no network | patch socket/HTTP/subprocess network entrypoints to fail during offline replay/rebuild/tests | no real network call; mocked transport has explicit recorded calls only |
| lookup scale | compare real archive read-only and declared larger synthetic corpus; count file reads/globs and record timings | normal open and point capture/derivation lookup do not scan whole receipts/archive; only relevant verified bytes are read; full audit is explicit |
| source coverage | explicit zero, all-failed, mixed, partial detail, changed total and malformed source fixtures | zero has its proof bytes; failed is accounted-for not successful coverage; partial cannot publish complete replacement |
| stable claims | reorder repeated certificates, amend one field, overlap query spellings, reject a candidate | occurrence duplicates/conflicts survive; claims do not retarget by position; rejected/unreviewed projection does not enter accepted catalog |
| closure selection | select finite real evidence; damage unrelated pending object; remove a required dependency | unrelated damage does not poison selected closure; missing required dependency fails before replacement |
| boundedness/preservation | exact pinned seven RRA/five ISED set; second saved replay and one fault rehearsal | no expanding source set; source/manifest/catalog/export byte hashes unchanged; synthetic inputs stay outside evidence |

## observations about existing test coverage

`tests/test_capture.py` already covers exact request fingerprints, same-instance
leases, cache validation, 304 dependencies, immutable saved registration, denied
URLs, post-lease races, throttle stops, saved-unavailable behavior and no redirect
following. its interruption case leaves an attempt without a result but expects
lease release; it does not establish durable quota or safe restart dispatch.

`tests/test_artifacts.py` covers immutable publication, orphan detection,
reconstruction, symlink rejection and read-only nonmutation. its concurrent
reindex test allows a second receipt publisher and tests eventual index
visibility; that does not satisfy the new single-writer recovery protocol.

`tests/test_derivations.py` verifies recipe identity, output verification,
consumer reuse and duplicate order. `tests/test_archive_release.py` verifies a
frozen release is independent of later registry state, but currently freezes all
then-existing entries. neither establishes selected-closure isolation or the
receipt-only coverage manifest.

## final cross-review

reviewed the coordinator's draft `docs/campaign-state-contract.md`, the full
pinning helper and generated inventory/definition, measurement and projection
artifacts. the following draft decisions settle the safety findings above:

- durable accounts/reservations charge requests and worst-case response-body
  allocation; response status/restrictions persist before body reads; uncertain
  attempts stop and never auto-refund or auto-dispatch;
- operational state retains safety authority, while the index is recoverable from
  exact intent bytes and durable head sequencing; missing/rolled-back/restored
  state requires explicit quarantine/recovery;
- OS locks accompany SQL fences, with explicit legacy migration and no legacy
  live API bypass; clocks cannot steal a held lock;
- request/response metadata redaction, ISED session separation, finite RRA
  observation slots, stable source identity and later c3 projection/coverage
  boundaries remain explicit;
- legacy registration time cannot outrank new observations; live cache reuse
  requires compatible request context/scope, and new ordering uses durable
  admission sequence. S2 explicitly tests unknown saved context registered after
  captures with backward wall times while preserving offline replay;
- c2 must replace saved `store_bytes` copying with verified in-place registration
  before replay, preserving the no-new-source-bytes limit and historical copies;
- Q/R/L/M/P are c1a, I/S/indexed reuse are c1b, and source replay/coverage/claim
  review are later c2/c3 gates. no test passage or implementation is claimed.

### blocker b1: repeat registration of a stable immutable receipt ID

c1b publication step 3 permits a same-ID collision only for identical wrapper
bytes. the current API intentionally reuses a preexisting frozen wrapper if
`kind` and canonical `payload` match even when the new caller would generate a
different timestamp (`artifacts.py:137–142` and
`tests/test_artifacts.py:test_receipts_freeze_and_distinct_attempts`). saved
captures, recipe registration and consumer links depend on that behavior for
second-run idempotence. requiring freshly generated timestamps to match would
break those existing flows or lead an author to change stable IDs.

accepted contract fix, reread after coordinator edit: under publication lock,
before generating/committing a
new intent, look up and validate any existing requested ID. identical kind and
canonical payload must reuse the original frozen wrapper bytes/timestamp and
complete any prior recovery, with no new receipt/sequence. conflicting payload
fails. once a new intent is committed, exact wrapper bytes remain mandatory
through every recovery/collision path. add repeat registration of legacy stable
IDs and new recipe/saved IDs to I1/C1 acceptance.

the corrected c1b step 1 and I1 now state this behavior explicitly. b1 is
resolved; no further safety-critical design blocker was found. remaining work is
the concrete implementation and its independently reviewed acceptance tests.

### verified c0 inventory

independently ran the final helper's **read-only** `verify` command successfully:
12 units (seven RRA, five ISED), 1,121 source paths, 1,093 unique byte hashes,
7,764,220 logical bytes by path and 7,160,749 unique-content bytes. no source
copies were made. canonical inventory SHA-256:
`85ac532ab1ae457440c4ae9760ff76ef8d919737955b632694ca41534e1a8e2e`;
exact inventory file SHA-256:
`0b891f166c67093515fd7dc6ebe4c41281a2db5501aeb20aa5e4b6ee2083629a`.
the command rehashed the accepted manifest/release/catalog, implementation/policy
and context references, source closure, and selected existing PDF fixtures;
deterministic source validation and definition regeneration matched saved outputs.

also independently compared the inventory's **duplicate-sensitive projected
source-key lists** against all 12 accepted sources through SQLite
`mode=ro&immutable=1`. every list matched, including Amazon's schema-1 CSV ordinal
origin, the 162 target rows within Accton's 167 HTML rows, and both explicit-zero
sources. this is an identity/count cross-check, not a fresh import or new claim.

the inventory explicitly preserves repeated ISED certificates, 15 Accton HTML
rows without exact CSV matches, cross-query RRA certificate overlap, separate
manufacturer/applicant associations, unknown historical collection versions/times,
and saved restrictions. selected accepted Golduck lineage is regression context,
not pilot PDF evidence. available disk and registration measurement time are
separate from upstream observation times. source-specific validator restrictions
and complete sibling dependencies remain intact.

the final helper now rejects symlink root/output ancestors before resolving them,
preflights changed frozen outputs, publishes files by exclusive atomic links and
preserves the first measurement. exact-byte repeated publication is idempotent;
an interrupted multi-file build can finish only with compatible bytes. it makes
no cross-file transaction claim. its cap includes existing helper/draft/review
bytes under the output directory and proposed payload growth. both superseded
in-progress drafts are retained and are clearly historical. the owner's recorded
18 fixture checks were inspected; this reviewer did not rerun their `/tmp`-writing
self-test because the assigned write boundary is this report only. subsequently
inspected [coordinator proof](acceptance/coordinator-checks.json): two identical
network-guarded verifications and all 18 fixture checks passed, with zero forbidden
entrypoint attempts and unchanged catalog, manifest, accepted release and complete
reference-PDF collection hashes. this is c0 helper acceptance, not c1 test passage.

checked all local Markdown links across the contract, campaign plan, batch
note, [inventory findings](inventory/findings.md) and this report; every target
resolves. the coordinator owns changing the current batch/status handoff from
in-progress to accepted after this verdict. no pinned source/reference bytes need
to change for that status update.

accepted checkpoint hashes reverified through the inventory helper:

- manifest: `27ff5fc4a6a0d170dfb1128e17c1a452aa6bdc725c3d3c7060584ca165bd3540`
- release: `2eadea0c31d9ffdfc1b763b26b8500ebddcb78c94d2706900ed382549fb33736`
- catalog: `62ab16eb8ab562ebbd6eaf6951987800fa8f4482879007d1713d0b8529ce0e4a`

### remaining phase gates

with b1 settled, the design can gate a separately assigned, finite c1a then
c1b implementation; it does not authorize live acquisition, mutation of accepted
catalog/manifest/exports or automatic migration of the live archive. implementation
must prove the fault/multiprocess/redaction/recovery tests in the contract and this
report. selected closure and claim-review enforcement remain c3 work, not current
archive functionality. source transports remain unimplemented and unvalidated.
