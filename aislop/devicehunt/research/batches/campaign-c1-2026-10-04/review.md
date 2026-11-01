# independent c1a review

reviewer: `campaign_contract_review`; coordinator: `/root`; author:
`campaign_state_engineer`. date: 2026-10-04. write ownership is this report only.
the accepted [state contract](../../../docs/campaign-state-contract.md),
[c1 assignment](../../campaign-c1-2026-10-04.md) and frozen
[c0 review](../campaign-pilot-2026-10-04/contract-review.md) remain read-only.

status: **accepted for c1b interface work**, following final c1a code/fix/test
rereview. final combined c1 code acceptance still requires its full regression
gate. no live collection/migration is implied. source captures, accepted
catalog, manifest, archive state and exports stay read-only; no network,
installations or further delegation.

## checklist and required proof

| area | independent inspection/check |
| --- | --- |
| durable quota Q1/Q2 | true competing processes on different hosts share one explicit account; quota stays spent after first completion/restart independently of unfinished-reservation exclusion; restarts/new collectors cannot replenish it; cumulative request, worst-case body allocation and deadline limits are enforced atomically |
| reservation Q3 | state API itself enforces allowed transition edges and receipt/result proof, rather than trusting caller-supplied prior/next strings; reservation commits before attempt publication; sanitized immutable attempt references its reservation; admission commits before dispatch; UUID access never means another dispatch; every uncertain state remains charged and blocks automatic retry |
| header status R1 | production and mocks expose headers before reading body; 402/403/429/503 restrictions survive oversize/read timeout/object/result errors and interrupts; persistence failure leaves an uncertain reservation that prevents restart dispatch |
| policy R2 | exact URL, host, source and resource scopes are independent; URL/run/session changes cannot bypass saved denial; resume names triggering throttle(s), policy, eligibility, identity and remaining quota; later throttle/denial is not cleared |
| exclusion L1 | OS locks accompany SQL fences; no wait for OS lock inside SQL transaction; fixed lock order; live lock survives clock jumps; old owners cannot release or finalize replacement work; request/host locks cover the actual transport call |
| migration M1 | explicit quiescence and atomic schema migration; legacy key/owner/expiry values survive exactly; interrupted migration rolls back or completes; unknown schemas are refused; read-only/missing opens create nothing; no second lease authority |
| redaction P1 | mixed-case Cookie/Set-Cookie/Authorization/Proxy-Authorization/source-sensitive headers absent from every new persisted wrapper/diagnostic; original captured bytes remain literal; historical metadata is referenced rather than reembedded unsanitized |
| immutable compatibility | repeated stable saved/recipe IDs reuse the frozen existing wrapper/time; conflicts fail; no new accepted data is written |
| readiness boundary | default production transport and nonzero legacy `Policy.budget` cannot dispatch before c1b; explicit fixture admission exercises c1a without silently claiming archive publication safety |
| offline regression | meaningful affected/full tests use mocks/temporary archives; no real socket/HTTP/subprocess collection; old saved-only flows and existing immutable original/catalog hashes are preserved |

## interfaces to settle before c1b assignment

- one state-owned readiness check must fail closed for missing/restored/incomplete
  operational state and c1b index/head/pending publications. reservation and final
  dispatch admission need this gate; a caller's truthy readiness flag is not proof
  that production publication is safe. explicit temporary-fixture bypass must be
  visibly separate from production behavior.
- the operational API must expose committed reservation identity, predetermined
  attempt ID, owner/fence and durable admission order to capture. c1b needs those
  references for indexed attempt/result ordering and restriction linkage.
- capture must publish attempts/results through the archive-owned publication
  interface. c1a cannot independently establish c1b's intent/head guarantees while
  `artifacts.py` retains old writable-open reindex. production remains disabled
  until that interface and recovery protocol are reviewed.
- transport needs an explicit sanitized header/status callback, including HTTP
  error responses, before body reading. callback persistence failure must stop
  reading/dispatch progression; redirects are validated before following any
  target. the callback contract must remain usable for mocked ISED transport.
- c1b's one-time receipt migration must ingest saved denial/throttle evidence into
  the same operational restriction authority; normal indexed lookups cannot
  manufacture unrestricted state from a cache miss. legacy unknown contexts need
  offline-only eligibility, not later-registration priority in live cache.
- retain saved `register_saved` behavior for existing offline regression callers,
  but make the later c2 verified in-place registration requirement explicit; the
  c0 pilot cannot copy its full 1,121-file source set through `store_bytes`.

## findings and acceptance

initial blockers sent to the coordinator and author; line numbers refer to the
first coherent author draft, not necessarily the eventual revised files.

1. **state transition/proof boundary.** `campaign_state.py:383–397` accepts
   caller-supplied expected/next states without enforcing the allowed graph.
   `reserved → complete` is consequently possible; receipt publication and result
   proof can be skipped. state APIs must enforce only declared edges, validate the
   predetermined attempt receipt at publication/admission and a matching immutable
   result at completion, and refuse wrong-owner/fence/result references. this was
   independently flagged by the coordinator as well.
2. **lease/host identity admission.** `_ownership:302–306` accepts an empty fence
   dictionary; `reserve:356–373` accepts caller-supplied URL/host/request identity
   without requiring the exact request/host keys. a caller can reserve without
   held locks or supply a different host to evade its limit/restriction. require
   exact request and normalized URL-host keys, nonempty fences, and matching
   `lock_owners` tuples in addition to SQL rows/OS handles. test empty/extra/missing
   keys, changed host and stale-owner contexts directly through the state API.
3. **saved restrictions before publication, with full known scopes.**
   `capture.py:156–158` publishes a prior-failure wrapper before persisting the
   policy event. archive publication failure can thus lose a known denial.
   `_ingest_restrictions:160–182` records a denied legacy URL alone even when its
   source/resource identity is known. ingest authoritative safety first and
   preserve known resource/broader declared source scopes so changing a URL cannot
   bypass that policy. publication failure and changed-URL fixtures are required.
4. **cross-account uncertain work.** `_eligible:314–315` blocks unfinished units
   only in the same account. after status persistence failure, a distinct
   explicitly allocated account can dispatch the same uncertain request/resource.
   apply the accepted pending/unreconciled readiness gate across accounts, using
   the declared conservative archive/source scope, rather than letting a new
   allocation turn uncertainty into unrestricted access. test new-account
   admission after a header-observation crash and preserve charged quotas.
5. **credential fragments in diagnostics.** `capture.py:21–32,355–361` replaces
   whole sensitive header values but persists arbitrary validator exception text.
   `Authorization: Bearer abc123` plus `ValueError('token abc123 invalid')` leaks
   the token. cookie fragments have the same problem. use safe typed/category
   diagnostics or token-aware redaction on these paths; test request/response
   credentials in validator and saved metadata errors, while preserving literal
   original body bytes. operational event reason/provenance callers also need a
   documented safe metadata boundary.
6. **cosmetic host spelling.** the public capture API accepts any exact allowed
   URL but lowercases `netloc` for host scope. explicitly allowed
   `https://host/path` and `https://host:443/path` would therefore use different
   host throttle/quota keys. reject userinfo and unsupported schemes; share
   canonical hostname/default-port handling across requests, state admission,
   restrictions and host-limit declarations. preserve exact source URLs and do
   not infer DNS aliases. add equivalence/rejection fixtures. the coordinator
   independently identified this boundary.

the durable quota test must establish spend after completion/restart, separately
from the conservative same-account single unfinished reservation rule. otherwise
concurrent refusal could accidentally demonstrate only work exclusion, not quota.

the author owns fixes and the coordinator prioritizes them; final dispositions
are recorded below.
passing c1a does not mean c1b indexed publication, c2 source transports, c3 selected
closure or live readiness is implemented.

### first final-handoff rereview

the frozen handoff addressed initial findings 1–6 and lock-directory symlink
prevalidation. state transitions now validate exact immutable attempt/result
proof; required request/host fences match actual held owner/generation; unresolved
work blocks admission across accounts; saved status scopes ingest atomically
before wrapper publication; validator errors are typed; host safety scopes group
the exact hostname across allowed ports/schemes; and migration retains a verified
small legacy backup. tests separately check spent quota after completion/restart,
not only concurrent exclusion. the author reports 40 affected tests passing.

two remaining findings were sent before accepting that handoff:

- **native header object path:** the production transport passes real
  `HTTPMessage` headers to the callback before body read, but dictionary-only
  sanitization leaves those objects non-JSON and does not gather credential
  values. commit observed status first, then normalize the header mapping into
  sanitized plain metadata and gather repeated sensitive fields before collapsing
  keys. require mocked native-header success/403 fixtures through the actual
  collector callback without network.
- **short credential fragments corrupt structural identity:** extracting cookie
  token `1` and substituting it into every string changes SHA-256s, reservation
  IDs and literal URLs. limit fragment substitution to diagnostic/header text;
  keep structural identifiers/hashes/exact approved source context intact, or
  explicitly reject credential-bearing URLs. require a successful short-cookie
  fixture proving unmodified artifact/reservation identities while credentials
  stay out of metadata.

the coordinator authorized the necessary narrow fixes. the full-suite run on the
earlier snapshot is pre-fix proof; final acceptance requires final code/test review
and final verification. c1b is not being expanded into this assignment.

### final disposition

both final findings are resolved and reread. `_response_metadata` runs only after
the operational status/restriction transaction commits, handles native
`HTTPMessage.items()` and gathers every repeated sensitive-header value before
omitting those fields from plain JSON metadata. the new success/403 native-header
fixtures invoke the actual transport through a mocked opener and assert durable
status before the first body read, sanitized result JSON and absent credentials.

fragment substitution now applies to named diagnostic text, preserving structural
hashes/IDs/source fields. the short-cookie test proves successful completion,
unchanged artifact SHA-256, reservation/request/attempt identity, exact URL and
ETag, with credential headers absent. validator/transport exceptions remain typed.
all six initial findings and the symlink-lock boundary are resolved; no further
blocking c1a defect was found within this bounded review.

inspected the final [engineering handoff](engineering/handoff.md), test definitions
and [test result/code pins](engineering/test-results.json): **42 affected tests
passed in 18.750s** (25 campaign-state, 17 capture), with compilation passing.
this reviewer inspected the proof and did not write/run additional mutable
fixtures outside the assigned report path. test guards and real spawned-process
fixtures were reviewed; they demonstrate mocked dispatch/quota and held-lock
exclusion without real collection.

reviewed final code SHA-256:

| path | SHA-256 |
| --- | --- |
| `devicehunt/campaign_state.py` | `c9b28ec278cf170c17380165ccdee56ab74163569a39a247ae9a287ca85c0b41` |
| `devicehunt/capture.py` | `400313e89d2bcdeac84aed4b414c2502aa52dbc8486997d730b6e1aa5f39568c` |
| `tests/test_campaign_state.py` | `c276da95578ee06461b01f3be3c2f5be594fd2564e38671671d3f0be61eda75a` |
| `tests/test_capture.py` | `27b56207391a8688f8b8af767a8956a385574dc772f4a66bf4b5773d0633c864` |

practical documentation was checked: `docs/archive-storage.md` accurately says
production transport is disabled, a network-budget flag cannot enable it, and
saved registration remains available; PROGRESS and the c1 assignment describe
c1a as unaccepted during implementation and c1b as the next gate. coordinator
owns replacing that status after this interface verdict. no documentation claims
live migration or collection occurred. all checked local links across README, PROGRESS,
archive usage, the c1 assignment, engineering handoff and this report resolved.

inspected [coordinator proof](acceptance/c1a-checks.json): 1,549 protected entries,
including all 1,121 source pins, accepted checkpoints, the complete PDF bundle and
424 existing archive files, retain their bytes; archive file membership is
unchanged. the pre-final-P1 regression baseline passed **514 tests in 327.693s**.
its recorded ResourceWarnings concern existing catalog/importer SQLite test paths;
no tests failed. that run is explicitly distinguished from the final 42 affected
tests and does not substitute for final combined c1 regression. no additional
live archive audit was performed by this reviewer.

### c1b dependencies retained

- replace unconditional production refusal with state-owned archive/index/head/
  pending-publication readiness at reservation and dispatch; preserve visible
  fixture isolation and fail-closed lost/restored-state behavior;
- transfer publication locking into `Archive` without nested independent flocks,
  implement exact durable intent/head recovery and indexed safety/reuse lookups;
- implement the currently gated `reconcile_result` boundary with verified frozen
  attempt/result/artifact proof and fresh recovery fences, retaining spent quota
  and no HTTP redispatch; unreconciled results remain globally stopped meanwhile;
- maintain publication/state lock order when replacing read-only receipt proof;
  ordinary read-only opens may not acquire mutation/recovery locks;
- run the **final combined c1 full suite** before final code acceptance or any
  separately authorized live migration. the coordinator's earlier full-suite run
  preceded the last two P1 fixes and is regression-baseline proof only;
- keep c2 in-place saved registration, source/session transport, c3 selected
  closure/claim review and live source validation outside this c1a acceptance.
