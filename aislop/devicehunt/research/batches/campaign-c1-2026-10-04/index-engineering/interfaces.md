# c1b archive index interfaces and ownership proposal

2026-10-04; author `campaign_index_engineer`, GPT-6.1 Sol high. read-only
application/test inspection; this note is the only deliverable. acceptance and
implementation ownership remain with `/root`. inputs: [c1 note](../../../campaign-c1-2026-10-04.md),
[state contract](../../../../docs/campaign-state-contract.md), `artifacts.py`,
`archive.py`, `capture.py`, `campaign_state.py`, `derivations.py` and affected tests.
no network, packages, live migration, accepted-data mutation or delegation.

## proposed ownership

| writer | paths after explicit transfer | boundary |
| --- | --- | --- |
| index engineer | `devicehunt/artifacts.py`, `devicehunt/archive.py`, `devicehunt/derivations.py`, `tests/test_artifacts.py`, `tests/test_archive_index.py`, `tests/test_derivations.py`, assigned bounded benchmark script | archive core, indexed derivation reuse and explicit CLI recovery/upgrade |
| c1a engineer or coordinator | `devicehunt/capture.py`, `devicehunt/campaign_state.py`, `tests/test_capture.py`, `tests/test_campaign_state.py` | indexed capture lookup, admission readiness, safety synchronization and result reconciliation |
| coordinator | current milestone/PROGRESS/practical documentation and acceptance proofs | combined review/full tests; no live migration in this code gate |

the core must land with capture's outer lock removal in the same accepted batch.
`FileLocks(root / 'capture-locks').held(['publication'])` is the one writer lock;
do not add another lock key, operational database or service. lazily import
`FileLocks` in artifacts to avoid the existing campaign-state/artifacts import
cycle. private `_..._locked` core methods can share a lock; public writers never
recursively invoke another public writer while holding it.

## minimal public API

existing `receipt`, `store_bytes`, `register_file`, `get_receipt`, `resolve`,
`receipts`, `audit` and `close` keep their return shapes. old receipt IDs,
timestamps, payloads, original paths and object bytes remain exact. recipe code
changes naturally create a new recipe ID; old recipe wrappers are preserved.

| API | behavior |
| --- | --- |
| `Archive(root, workspace=None, create=False, readonly=False)` | ordinary open validates only bounded schema/meta/head/pending state; no recovery, reindex, mkdir, checkpoint or full scan. `create=True` initializes only a truly new archive and refuses existing evidence/index/head/state, including a damaged existing archive. writable legacy open requires explicit upgrade; readonly legacy audit remains supported. |
| `readiness()` | returns `ready`, `legacy_readonly` or `needs_recovery`, generation/sequence and a precise reason; does not mutate or take publication lock. unknown schema, corrupt/missing index/head, unexplained mismatch, active intent and quarantine are visible failures. |
| `require_ready()` | raises a named archive recovery error unless modern acknowledged publication state is ready. readiness never means budget/policy authorization. |
| `verified_receipt(receipt_id, *, require_ack=True)` | selected immutable wrapper plus generation, publication sequence (unknown for legacy), raw-wrapper SHA-256 and acknowledged head sequence. verifies selected bytes against indexed bytes/hash and required registered artifact closure. no OS lock and no state writes. operational transitions call this themselves, never trust a supplied proof token. |
| `capture_candidates(request_id, *, source, session_scope, observation_scope, live=False)` | indexed exact-context candidates; live excludes unknown saved context. validates only selected wrappers; deterministic ordering uses admission sequence for compatible modern network observations, never registration timestamps. |
| `derivation_candidates(recipe_id, inputs)` | successful runs via recipe + canonical ordered-input fingerprint; verify selected wrapper, full exact inputs and outputs before reuse. saved unknown recipes cannot match. |
| `safety_references(...)` | indexed status/scope/provenance references, including pending-import references; no directory scan. source/resource remain adapter-declared, never filename guesses. |
| `upgrade(root, workspace, *, quiesced, decision, ...)` | explicit legacy-to-modern validation and full index preparation under publication lock, preserving files; emits safety-import checkpoint for state integration. no automatic initialization of lost quota authority. |
| `recover(root, workspace, *, decision, ...)` | completes intact publication intent under lock; revalidates selected dependencies; never adopts arbitrary unindexed files or dispatches. |
| `reindex(root, workspace, *, quiesced, decision, ...)` | explicit full immutable-file validation/new index generation, after pending intent recovery where possible. invalid input does not destroy the prior usable projection; changed generation invalidates state safety binding until explicitly synchronized. |

the following concrete seam is proposed for both authors; coordinator acceptance
freezes it before ownership transfer:

```python
archive.readiness() -> dict
# {status, reason, generation, sequence, acknowledged_sequence, pending}
archive.require_ready() -> dict  # same shape; raises ArchiveRecoveryError
archive.verified_receipt(receipt_id, *, require_ack=True) -> dict
# {wrapper, wrapper_sha256, generation, sequence, acknowledged_sequence}
# sequence is None for imported legacy wrappers; wrapper is the existing shape.
archive.capture_candidates(request_id, *, source=None, session_scope=None,
                           observation_scope=None, live=False) -> list[dict]
# existing wrapper shape, ordered oldest -> newest; caller uses row['payload'].
archive.derivation_candidates(recipe_id, inputs) -> list[dict]
# existing wrapper shape; exact normalized ordered inputs; deterministic order.
archive.policy_candidates(url) -> list[dict]
# capture_policy wrappers indexed by saved_failure.url, including unavailable
# policy outcomes, not just denial/throttle. no full-history scan.
archive.safety_references(*, after_sequence=None, generation=None, url=None,
                         host=None, source=None, resource=None) -> list[dict]
# {reference_id, receipt_id, wrapper_sha256, sequence, row}
# row is the exact legacy/saved status-bearing payload selected from the wrapper;
# consumer applies existing sanitization/provenance rules. generation mismatch
# raises; imported legacy refs have sequence 0. None returns all selected refs;
# numeric after_sequence means strictly greater. filters select applicable exact
# URL/resource/source denial or host throttle scopes, not guessed equivalences.
# reference_id=digest(canonical({receipt_id, wrapper_sha256, selector})); selector
# distinguishes payload versus payload.saved_failure. keep unsafe originals only
# in their immutable source wrapper, never repersist raw secrets into state.
Archive.upgrade(root, workspace=None, *, quiesced=False, decision=None,
                fault=None) -> dict
Archive.recover(root, workspace=None, *, decision=None, fault=None) -> dict
Archive.rebuild(root, workspace=None, *, quiesced=False, decision=None,
                fault=None) -> dict
archive.reindex(*, quiesced=False, decision=None, fault=None) -> dict
# classmethod operations return readiness plus recovered_receipt_ids and
# safety_checkpoint={generation, sequence}; instance reindex uses the same engine.
```

`ArchiveRecoveryError` subclasses `ValueError` for existing corruption handlers;
lock contention retains shared `LeaseBusy`. faults receive stable boundary names.
capture candidate ordering must put compatible modern observations ahead of
unknown legacy registration, and use admission sequence within modern context.
`verified_receipt` proves publication even when the optional artifact closure is
empty; callers verify semantic reservation/owner/status fields themselves.
readonly paths never synchronize safety references. core safety rows are immutable
index projections; the integration owner tracks pending/resolved references in
operational authority, keyed by generation/reference identity.

CLI `init` uses new-only create; `upgrade`, `recover`, `reindex`
use explicit entrypoints rather than `create=True`. legacy readonly audit may
inspect the old schema and enumerate immutable files. modern missing-index reads
must report recovery required, never an empty result. an explicit audit may scan
file authority even when normal indexed query is unavailable.

## index and publication protocol

keep `index.sqlite`. modern metadata contains schema, UUID generation and durable
sequence; receipts store ID/kind/canonical payload plus exact wrapper bytes/hash
and publication sequence. keep artifact/location projections. add one constrained
pending-intent slot (exact wrapper bytes/hash, ID, expected previous/next sequence,
phase), indexed capture fields, derivation fingerprints and safety references.
create real SQL indexes on selected predicates/order. legacy rows have unknown
admission sequence; indexing does not invent historical HTTP/reservations.

`index-head.json` contains schema/generation/sequence. head writes use fsynced
private staging + atomic replacement + directory fsync; immutable `_publish`
continues exclusive link publication and byte comparison. SQLite uses explicit
durability settings. normal readiness compares a bounded head/SQL/head snapshot;
an in-flight mismatch fails closed or returns a bounded retry-needed result, never
waits for the writer lock. no receipt-count consistency heuristic or `quick_check`
of every page on each open. a stale handle after generation change must fail.

all public writers acquire publication lock before any index write and recover
the existing intent before doing new work:

1. validate head/meta/intent. resolve an existing stable ID first: identical kind
   and canonical payload return its frozen wrapper without a new sequence; conflict
   fails. indexed bytes and actual immutable bytes must agree.
2. freeze ID/time/complete wrapper bytes once; commit intent and its expected next
   sequence. validate explicit receipt dependencies before completion; registration
   validates the referenced location, size and hash.
3. fsync/publish immutable file; an existing pathname must have identical bytes.
4. index wrapper and every lookup row in one transaction, advance sequence, mark
   intent indexed. keep the exact intent through head publication.
5. atomically fsync matching head; acknowledge by clearing intent in durable SQL.
6. return only after acknowledgement. callers do not treat merely existing receipt
   files as proof of completed publication.

fault hooks cover each committed boundary. recovery replays the frozen wrapper:
intent/no file -> publish; file/no row -> verify/index; row/no head -> head;
head/no ack -> clear. sequence/generation/hash contradictions stop. same-generation
index ahead of head is recoverable only with intact matching intent. missing index,
index behind head or unsupported schema require explicit full recovery/reindex.
an object without registration remains a reported orphan, never a valid input or
automatic deletion target. `store_bytes` may publish an object then its registration
within one lock; interrupted registration is recovered through its receipt intent.

explicit generation replacement also needs a durable transition intent, so failure
between committing the new projection and replacing head is recoverable rather
than guessed. stage/validate all full-scan inputs before replacing old rows; retain
the generation transition until head acknowledgement. do not discard a pending
receipt intent as a shortcut to reindex.

## integration requirements for capture/state owner

- remove `Collector._receipt` and `_store_bytes` outer publication locks; archive
  owns them. remove constructor full safety scans and `_rows` use in point lookup.
- admission checks modern archive readiness before reservation and again before
  dispatch. `_verify_receipt` uses `verified_receipt` inside its current state SQL
  transaction; this archive path stays strictly lock-free/read-only. core publication
  never opens/writes operational state while holding publication lock.
- explicit upgrade/reindex enumerates indexed safety references and imports known
  denials/throttles through the existing idempotent state APIs before enabling HTTP.
  state records the acknowledged index generation and safety-import checkpoint.
  new saved/legacy restrictions added through generic receipt APIs create indexed
  pending safety references until imported or proven to match a durable observed
  reservation/status. unresolved refs conservatively block dispatch. synchronization
  is explicit and outside state transactions that perform archive verification;
  acknowledgement must bind exact references/generation, not counts.
- operational restrictions remain authoritative for known headers whose body/result
  never publishes. new status evidence must not resurrect explicitly resumed old
  throttles. offline archived restrictions use indexed references without scanning.
- live capture candidates require known compatible request/source/session/observation
  context. saved replay stays supported. two network observations rank by durable
  admission sequence despite backward wall times; later legacy registration cannot
  displace them. RRA before/after slots remain different contexts.
- `reconcile_result` obtains request/host recovery locks before SQL, retains old
  quota and original attempt fence evidence, verifies acknowledged attempt/result
  and artifacts, records a decision, and completes without redispatch. do not
  rewrite a frozen attempt to a new owner/fence. incomplete/ambiguous observations
  remain charged and stopped; clearing an account stop needs a distinct justified
  decision, not an automatic reconciliation side effect.
- restore is an explicit operation: quarantine operational authority first, perform
  full reconciliation and rebind index generation only with verified authority.
  restoring both old head and old index cannot be detected by a normal open alone
  under the stated threat model; restore must therefore invoke quarantine. lost or
  rolled-back operational quota cannot be recovered exactly from receipts because
  reservations can precede receipt files. keep acquisition blocked without a
  coherent restored authority; never infer or reset spend.

## finite acceptance work

I1: kill real worker processes after intent/file/index/head/ack commits; recover
each prefix, compare exact raw wrapper bytes and stable timestamp, verify one
sequence only and no dispatch while pending. stable registration/saved/recipe IDs
repeat without sequence changes; conflicts fail. missing referenced bytes halt
recovery. test object orphans and interruption during generation replacement.

I2: missing/corrupt index/head, rolled-back index versus current head, unsupported
schema, lost intent, restore quarantine and missing operational authority fail
closed. explicit upgrade preserves a frozen legacy fixture byte-for-byte and
imports all known safety scopes; readonly legacy audit causes no writes.

S1: instrument `Path.glob/rglob/iterdir`, wrapper reads and artifact hashes on
normal open/capture/derivation lookup. unrelated receipts/artifacts must not be
read. measure real archive **readonly** and declared larger synthetic fixture
with candidate count, reads, SQL query plan, elapsed time and dataset size.
real legacy audit is labelled a scan; synthetic modern open measures the new
normal path. do not migrate live archive just to benchmark.

S2: deterministic backward wall times with increasing admission sequences; a later
saved registration retains unknown context, offline reuse works, session mismatch
and observation-slot mismatch cannot satisfy live cache. repeated offline recipe
reuse uses indexed candidates and adds no redundant run/output.

replace obsolete tests with stronger protocol tests, preserving their intent:
`test_identical_concurrent_publication_uses_frozen_winner` currently fabricates an
uncoordinated winner by monkeypatching `_publish`; replace with two real processes
and one shared stable ID, exact winning wrapper bytes and one sequence.
`test_reindex_snapshot_serialized_with_writer` manually publishes a file and writes
SQL outside the public writer; replace with real reindex/writer processes using the
public lock protocol and deterministic barrier. assert serialization/refusal,
receipt preservation and acknowledged head agreement. `test_reconstruct_and_restore`
must explicitly reindex/restore rather than recreate an existing archive with
`create=True`. retain corruption/reuse, permission and failure preservation checks.

stop after a bounded core + disjoint state/capture integration, independent review,
affected tests and one full offline suite. no c2 adapter, c3 release redesign,
collection, or live upgrade belongs in this assignment.
