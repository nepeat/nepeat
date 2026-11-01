# c2b — RRA live request path, mocked transport only

2026-10-05, continuing the Claude Code session (coordinator `/root-claude`, Opus 5.5)
after c2 acceptance ([c2 note](campaign-c2-2026-10-05.md)). The user said "continue";
the scope is PROGRESS next-step 1. No real request is made: there is no allocated or
activated account, and every test uses a mocked transport with forbidden network
entrypoints. c4 (a live refresh) still needs separate authorization.

## scope

- capture.py fixes from the [c1 follow-up review](batches/campaign-c2-2026-10-05/review/c1-followup-review.md):
  - B1: saved rows can declare method/body/scope/slot, and offline lookup honours a
    declared scope;
  - S2: a saved 429/503 never stops offline replay;
  - S3: an `http.client.HTTPException` body error still publishes its status-based result.
- new `devicehunt/campaign_rra_live.py`:
  - a pure `next_work(scope, observed)` planner;
  - a runner that sends each step through `Collector.capture`, with per-cycle
    observation slots;
  - a finite detail allowlist derived only from the validated before-CSV, under a
    predeclared item cap;
  - separate before/after exports (contract A1);
  - blocked/throttled/budget/invalid outcomes that never publish a partial bundle;
  - a complete bundle is assembled and checked with the existing `rra.validate`.
- tests in `tests/test_campaign_rra_live.py`, replaying real saved RRA bytes through
  mocked transport into isolated archives.

## ownership

| owner | writable paths |
| --- | --- |
| `/root-claude` | this note, `devicehunt/capture.py`, `devicehunt/artifacts.py` (capture lookup only), `devicehunt/campaign_rra_live.py`, `tests/test_capture.py`, `tests/test_campaign_rra_live.py`, `research/batches/campaign-c2b-2026-10-05/`, PROGRESS/docs |
| `c2b_review` (Claude Sonnet 5.5) | `research/batches/campaign-c2b-2026-10-05/review/` only |

`data/` is read-only for this milestone: no archive, catalog, manifest or export writes.
Stop when the mocked gates pass and the review is accepted. Do not wire live mode into
the CLI, allocate an account or activate one.

## results

- **c1 follow-ups fixed in `capture.py`:**
  - B1: `register_saved(method, body, headers, source, resource, session_scope,
    observation_scope, slot, unit_revision)`. Only declared values are stored,
    header values never are, and a slot requires a scope. The independent reviewer
    recomputed all 53 real saved receipt IDs: they are unchanged.
  - Offline lookup honours a declared scope.
  - S2: offline replay ignores host throttles.
  - S3: body protocol errors publish a reconcilable result.
- **`devicehunt/campaign_rra_live.py` (`RraScope`, `next_work`, `RraRefresh`):**
  - same request order, body encoding and file names as the curl helper;
  - per-cycle JSON observation slots, so the before-export, the after-export and
    earlier cycles never share an observation;
  - details only from the validated before-CSV, under a predeclared cap;
  - each validator is strict: every validator failure becomes `invalid`;
  - fail-fast manufacturer check on details;
  - 0.15 s pacing between dispatches, with an injectable pause;
  - reservations bound to the unit revision;
  - honest outcomes: `blocked`, `throttled`, `budget_exhausted`, `failed`,
    `unavailable`, `interrupted`, `stopped`;
  - no bundle from a partial or failed step set, and `rra.validate` gates the final
    `bundle.json`.
- **Tests:**
  - `tests/test_campaign_rra_live.py`, 15 tests. Mocked replay of the real saved bundles
    reproduces the pinned c0 record hashes for the 7-row Edge-Core unit and for the
    explicit-zero unit.
  - Three new capture tests.
- **[Review](batches/campaign-c2b-2026-10-05/review/c2b-review.md): ACCEPT with notes.**
  F1–F9 were fixed and verified. Two small notes were closed afterwards: pacing is
  sticky after any dispatch, and a denial takes precedence over a throttle.
- **Live archive unchanged by c2b:** head sequence still 1181, 1,555 receipts,
  0 reservations, accounts or leases.
- **Guarded full suite and code hashes:**
  [acceptance](batches/campaign-c2b-2026-10-05/acceptance/c2b-acceptance.json).

## c4 preconditions (not testable offline)

1. Separate user authorization for one concrete finite scope: unit, request, byte and
   time limits. Then an explicitly allocated and activated account.
2. **A single-request smoke first: the form GET only.** Production `transport()`
   follows no redirects and sends a urllib User-Agent; the saved bundles came from
   `curl --location`. A redirect makes every step `invalid` ("unexpected final URL").
3. A quota pre-check of 3 + n requests against the remaining account budget before
   the first detail. Add `Location` to the sensitive-header set. Extend the secret
   scan beyond `receipts/*.json`.
4. Use a fresh `cycle_id` per refresh. A reused id returns `captured` with
   `new_observations=0`; treat that as no observation, never as a refresh.
5. A detail slot cached under the old `rra-detail/1` validator would not carry the
   manufacturer check. None exists; a new cycle avoids it.
