# independent c1b review

reviewer: `campaign_contract_review`; coordinator: `/root`. write ownership is
this report only; [c1a review](review.md) stays frozen. inputs: the current
[ownership note](../../campaign-c1-2026-10-04.md), accepted
[state contract](../../../docs/campaign-state-contract.md) and
[index interfaces](index-engineering/interfaces.md).

status: independent c1b code review passes on the final author-pinned snapshot;
reported blockers are resolved. combined acceptance remains contingent on the
coordinator's final full offline suite and protected-data proof. no live readiness,
migration or collection is claimed. existing
data/source captures remain read-only; no network, installation or delegation.

## review gates

| gate | required inspection/proof |
| --- | --- |
| immutable preservation | legacy wrapper bytes/IDs/timestamps, object paths and registered locations survive explicit upgrade; stable repeat IDs do not create a new sequence; conflict fails |
| I1 publication | durable frozen intent precedes immutable receipt; exact wrapper/index/head/ack prefixes recover idempotently; one writer and no recursive lock bypass; dependency/object/hash contradictions stop |
| generation changes | reindex validates all inputs before replacement; generation transition intent survives index/head crash; stale handles fail, missing intent never means guessed adoption |
| I2 restore/loss | missing/corrupt/rolled-back index/head/state fails closed; explicit restore quarantine preserves quotas/stops; read-only opens neither create nor recover; core never writes operational state |
| selected proof | lock-free `verified_receipt` checks exact indexed vs immutable bytes and selected dependency/artifact closure, with acknowledged generation/sequence; arbitrary caller proof tokens cannot authorize transitions |
| safety admission | publication OS lock acquired before reserve/admit state SQL and released before HTTP; exact generation/reference checkpoint plus newly published safety receipts cannot race admission; generic writer paths create unresolved refs |
| result recovery | explicit reconciliation binds original frozen attempt/fences and acknowledged result/dependencies, uses fresh recovery ownership, retains spend/stop history and never dispatches |
| S1 indexed lookup | normal open/point capture/derivation/restriction lookup has no directory/full receipt scan; only selected wrappers/artifacts read; actual SQL indexes and declared real/synthetic benchmark instrumentation |
| S2 ordering/reuse | admission sequence governs modern compatible observations despite backward wall clocks; unknown saved context is offline-only; session/observation mismatch excluded; repeated recipe reuse adds no redundant run/output |
| conditional evidence | attempt fingerprint/metadata records actual If-None-Match/If-Modified-Since wire headers; 304 requires verified prior representation; callbacks preserve known status before body work |
| final P1 | native HTTPMessage/repeated credentials sanitize after durable status; short cookie fragments do not rewrite IDs/hashes/URLs; saved/diagnostic paths remain safe |
| saved adapter compatibility | coordinator added minimal `research/fetch_fcc_sitemaps.py` edit to state-author ownership: fresh roots initialize, existing modern archives open without recreate, and damaged/legacy evidence cannot be reset by `create=True`; repeated saved-only fixtures stay offline |
| bounded acceptance | actual affected and final combined offline suite on frozen code, coordinator protected byte/membership proof, accurate practical docs; no c2/c3 or live deployment claim |

## coordination boundaries

core author owns artifacts/archive/derivations; integration author owns capture/
state. the coordinator has approved a short publication-to-state admission guard,
with OS lock obtained before state SQL. readonly readiness/receipt verification
never takes the publication lock. archive core does not open/write operational
state. all dependent interface changes need the owners and coordinator to agree.

## findings and disposition

no remaining concrete code blocker was found in the final coherent implementation.
the coordinator owns final combined regression and protected-data proof; the final
verdict below distinguishes review acceptance from deployment or migrated state.

## initial coherent core findings

these were reported against the first coherent draft and are now resolved;
line numbers referred to the
first coherent core draft.

- coordinator findings: explicit reconstruction must support a corrupt disposable
  index while retaining damaged forensic bytes and leaving safety unbound; unknown
  newer schemas must refuse downgrade instead of blindly dropping tables. intent
  phase requires exact previous/next metadata pairing, and an indexed receipt's
  exact frozen bytes/publication sequence must match the intent, not merely agree
  with its own index row/file.
- reviewer finding: `readiness` does not bind the generation observed when an
  `Archive` handle opens. explicit reindex can therefore make an old handle adopt
  a new generation silently. require stale-handle refusal; explicit maintenance
  can return/reopen a fresh acknowledged handle.
- reviewer finding: `safety_references` validates selected wrapper bytes but trusts
  the indexed projected row, selector/reference identity and sequence. bind selected
  safety projection fields to the immutable selector-derived payload before state
  can acknowledge them. likewise selected capture/derivation candidates must
  recheck raw request/context or recipe/exact ordered inputs against their query.
  this is selected validation, not a full-archive scan or a claim to detect all
  arbitrary external index replacement.

the coordinator's offline acceptance harness was sanity-checked: socket/DNS,
urllib and external subprocess guards run on parent/spawn import, trace only
entrypoint/PID, and a caught forbidden call still fails the final result. current
spawn fixtures use queues rather than network Manager IPC. tests may replace guards
with explicit mocks. no kernel-level network-isolation claim is made.

## coherent fix rereview

the current core now binds opened generations, checks intent phase against exact
previous/next sequence and indexed frozen bytes, validates selected safety/capture/
derivation projections, preserves corrupt-index forensic bytes during explicit
reconstruction, and refuses valid newer schemas. the associated fixtures include
real process exits at all five publication boundaries, competing stable-ID writers,
serialized reindex/writer exclusion, exact legacy whitespace preservation,
generation recovery, selected corruption and unrelated-file scan guards. the
core author froze six application/test paths with 39 affected tests passing.

the integration uses one canonical selector/status interpretation for legacy
`http_status` and contradictory saved error text; known observations now compare
interpreted safety status to the durable observed status. actual conditional wire
headers and their separate wire fingerprint are persisted while compatible
representation identity remains reusable after 304.

resolved state blockers sent to coordinator/state author:

- lost operational authority after a committed reservation but before its first
  receipt can leave an empty receipt directory. `initialize` must use an independent
  durable state marker/binding so it cannot recreate forgotten quota. publish the
  marker before admission and preserve it through index maintenance; exercise
  lost authority with an empty receipt set.
- a generic matching `capture_result` can add `restriction_scope='source'` to a
  known observed 403. observation persisted URL/resource scopes, so exact status/
  reservation matching alone must not acknowledge the broader source reference.
  require exact represented scope semantics or explicit import; exercise a second
  URL under that source.

the benchmark separates actual legacy read-only point access from modern synthetic
measurement: 5,005 synthetic receipts, zero directory scans in ordinary open and
selected queries, one selected capture wrapper, and three wrappers/two artifact
hashes for selected derivation proof. query plans use capture, derivation and safety
indexes. timing is a bounded local measurement, not production throughput proof.

the final state implementation now publishes an independent immutable authority
marker before allocation, binds its UUID to SQL, and requires an immutable ready
seal before any use. marker presence prevents empty-receipt authority reinitialization;
the ready seal prevents reusing an old legacy/c1a migration backup after spend.
failed transactional migration remains retryable from its exact backup before
that seal exists. index maintenance does not write these state-owned files.
the lost-DB/pre-receipt and completed-migration replay fixtures exercise the two
distinct loss cases. initialization validates lock paths before writing either
authority file.

known observations now reject any extra declared restriction scope; the matching
403 plus broader-source fixture blocks admission until explicit synchronization
imports the second scope, then blocks another URL under that source. canonical
legacy selectors, conflicting status/error text and exact generation/reference
acknowledgements have corresponding fixtures. operational safety checkpoint,
publication guard and state SQL ordering close the generic-writer admission race.

result reconciliation re-verifies frozen original attempt owner/fences and selected
acknowledged result/artifacts under fresh recovery ownership. it preserves reservation
identity, allocation and account stop. account resume is a separate recorded decision;
ambiguous observations stay charged and blocked. quarantine does not offer automatic
reactivation or infer missing quota. fresh production requires ready generation,
exact safety binding and an explicitly activated finite account; positive production
transport tests use isolated fixtures and mocked native openers only.

the saved FCC adapter initializes only an absent archive path, opens existing modern
archives, and refuses damaged existing state. its repeated fixture keeps wrappers
and sequence exact without network. c2 in-place saved registration, source identity,
coverage manifest/candidate boundaries and c3 release redesign remain later gates.

## final pinned review verdict

accepted by independent review for the bounded c1b code gate, contingent on final
combined offline regression and coordinator protected-byte/membership verification.
no unresolved safety-critical transition was found in the reviewed paths; this is
not a live-ready verdict or permission to migrate/activate an existing archive.
any later code change reopens this pinned review gate.

- [core handoff](index-engineering/handoff.md) reports 39 affected tests, including
  all 17 I/S gates. [core code pins](index-engineering/checks.json) match all six
  current application/test files independently. proof-file SHA-256:
  `2d191a157025882ff8fa7b34e7740e017123cbb7d4d9a1fa0a9659184869f0f9`.
- [state integration handoff](state-integration/handoff.md) reports 52 affected
  tests and syntax checks. [state/adapter code pins](state-integration/test-results.json)
  match all five current owned files independently. proof-file SHA-256:
  `9b33881ae7f31b792216686be43cc8ea4e45b8f6d58093446c5d67f92164e039`.
- local links resolve in both handoffs, this report, current ownership note,
  PROGRESS, archive storage instructions and the accepted state contract. practical
  status distinguishes c1a baseline regression, pending final combined acceptance
  and absence of live migration/activation. coordinator updates remain separate.
- the earlier 514-test c1a suite is a regression baseline; it does not validate
  this c1b snapshot. this reviewer inspected code, executable fixtures, author proof,
  benchmark and hashes read-only; the coordinator owns running final regression.

remaining limits are explicit: process exits exercise committed crash prefixes,
not physical power loss; durability uses local POSIX fsync and SQLite FULL commits.
coherent external rollback of every head/index or DB/authority marker cannot be
detected by ordinary open and requires explicit restore quarantine. missing or
rolled-back quota and unfinished allocation/seal are blocked rather than guessed.
no automatic unquarantine/reset API exists. macOS without a shared boot identifier
conservatively blocks timed resume across processes. no live source transport,
source-refresh coverage, accepted-catalog mutation or later adapter/release work
was performed by this review.

## practical documentation and preservation follow-up

read-only follow-up passes for [archive usage](../../../docs/archive-storage.md),
[current progress](../../../PROGRESS.md) and the [c1 ownership/status note](../../campaign-c1-2026-10-04.md).
all local links resolve. the new-archive example now consistently specifies
`--root research/my-new-archive` for init, registration and reindex; the coordinator
fixed the initial mixed-root example. live `data/archive/` is explicitly labelled
unmigrated, with legacy audit/catalog evidence still usable and writes deferred
until separate exclusive rehearsal/upgrade. code acceptance, operational identity,
quota/safety activation and later c2–c4 work remain distinct.

the documented commands match the current CLI: root precedes subcommand; upgrade/
reindex require explicit decision and quiescence; recover requires a decision;
new-only init never resets existing state. documentation names operational DB plus
both immutable authority files and head/index backup, and preserves blocked
allocation interruption/restore limits instead of offering a quota reset.

[coordinator final preservation proof](acceptance/c1-final-preservation.json)
reports all 1,549 protected entries unchanged and the exact 424-file live archive
membership unchanged, with no migration/activation/catalog or manifest changes.
this reviewer independently matched its 11 code pins to the current frozen files.
proof-file SHA-256:
`148da1848a874645b9a3732047af280fdd6fcabe617b730aaa017e7422522a06`.
the source/archive protection result is coordinator evidence; this follow-up did
not repeat a live full-file audit or mutate any data. final combined regression
is still running at this follow-up, so the pending-suite gate remains in force.

## final acceptance crosscheck

combined c1 code acceptance now passes; this supersedes the pending-suite statuses
recorded above. [final guarded result](acceptance/final-suite-result.json),
[actual unittest log](acceptance/full-suite-c1-final.log) and
[post-suite coordinator acceptance](acceptance/c1-acceptance.json) agree:
542 tests, zero failures/errors/skips or forbidden network/external-process calls;
unittest time 339.896s, harness elapsed 339.95s. the post-suite proof records all
1,549 protected entries and exact 424-file archive membership unchanged. all 11
frozen code hashes still independently match current files, and its embedded suite
result exactly matches the standalone result. no rerun or data mutation occurred
in this crosscheck.

proof SHA-256: suite
`c074be8c4c72e0195a40e92c6cdd75875c5a9667271238ba06eee32adc74ef47`;
post-suite acceptance
`8d012eb90dff148361915f6d2e5e36e27ba75c54f23247dde14157e5c9448fc7`;
actual log
`f57fcffa5c61fd1249b024f3a7aa32e322c68e6b4395d90f8d6a24bd4fc18fdd`.

current PROGRESS and batch note now report accepted code and distinguish the
514-test historical baseline from completed final regression; the coordinator
fixed the last stale baseline phrase. local links resolve. the acquisition registry
remains unmigrated, no live account is activated, and its separate offline rehearsal/
upgrade plus c2–c4 remain pending. this accepts the reviewed code milestone only.
