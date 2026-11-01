# c1 follow-up review after the Linux move (c2 readiness)

reviewer: `c1_followup_review` (Claude Sonnet 5.5). date: 2026-10-05. write ownership:
this file only. prior reviews [c1a](../../campaign-c1-2026-10-04/review.md) and
[c1b](../../campaign-c1-2026-10-04/index-review.md) were read; nothing below repeats them.

scope and caveats

- code snapshot read: `devicehunt/campaign_state.py` as it was before the coordinator's
  concurrent SCHEMA 3 / campaign_* edit. line numbers for that file refer to that
  snapshot and may have shifted. the new campaign_* tables, `_SQL_V3` and
  `upgrade_campaign_jobs` were not reviewed, per the coordinator's instruction.
- the live `data/archive` is now upgraded (live-upgrade.json), so item 2 applies to it.
- no unit-test module was run (the coordinator warned campaign_state tests may be
  transiently red). every "verified" below is an isolated experiment under the
  session scratchpad (temp archives, mocks, loopback-free, no network); `data/` was
  only read (one `mode=ro` query of `index.sqlite` receipts).
- verified = reproduced by experiment; read = from code reading; suspected = reasoned only.

## ranked findings

### B1 (blocker for A1/c2 RRA) saved registration cannot express an observation slot, and offline lookup ignores scope
`capture.py:440-464` (`register_saved`), `capture.py:265-269` (`_cached`),
`artifacts.py:601-622` (`capture_candidates`).

- `register_saved` hard-codes `request_id=fingerprint('GET', url)`, no body, no source/
  session/observation scope, `request_context_known=False`, and takes no unit/slot
  reference. RRA before/after exports are the same POST, so both saved rows get one
  `request_id` and NULL scope columns in `capture_lookup`.
- `_cached` passes `live=bool(policy.budget)`; with budget 0 `capture_candidates` skips the
  source/session/observation filter entirely and orders NULL admission sequences by
  `receipt_id`.
- verified (scratch e6): two saved rows for one URL, slots before/after, share `request_id`;
  offline `capture(url)` returns whichever receipt id sorts last, regardless of slot.
  contract A1 ("after-export cannot reuse before") fails by construction on this path.
- fix (keeps existing IDs stable): give `register_saved` optional keyword args
  `method='GET'`, `body=b''`, `source`, `resource`, `session_scope`, `observation_scope`,
  `unit_revision`, `slot`; include a field in `result` only when the arg is supplied;
  build `request_id` with `fingerprint(method,url,body,context=...)` only when any is
  supplied. for offline replay, apply the scope filter whenever the Policy declares a
  scope, ignoring `context_known`. or: the c2 adapter never calls `Collector.capture` for
  replay and uses a dedicated indexed saved-unit lookup. either way document it.

### B2 (blocker per contract) `register_saved` always copies bytes via `store_bytes`
`capture.py:449-454`. contract requires verified in-place registration (`copy=False`).

- minimal safe change: `register_saved(self,row,path=None,locator=None,*,copy=True)`; when
  `copy` is false replace the `read_bytes()` + hash + `self._store_bytes(payload)` lines with
  `sha = self.archive.register_file(path, expected_sha256=row['sha256'])` (default
  `copy=False` on `register_file` already does the stat-stable hash, `_safe` workspace
  check, and stable `registration-<hash>` receipt). keep default `copy=True` so existing
  offline callers/tests are unchanged.
- receipt-ID stability: verified. `saved-<sha256(result)>` hashes `result`, which holds
  `artifact_sha256` but no location, so in-place and copy modes produce the same `saved-`
  ID (scratch e6: subclass registering in place returned the identical id and added no
  sequence). do NOT add a location/path field to `result`; that would fork every existing
  saved ID. do not put absolute or host-specific paths in `row`/`locator` either (they are
  hashed into the ID).
- `register_file` is idempotent (verified: second call adds no sequence). the new
  registration is a separate `registration-*` receipt, so replaying a row that already has
  a copied `saved-` receipt adds exactly one registration receipt, nothing else.
- `_resolve` returns locations ordered by (scope,path), so `archive` copies win over
  `workspace`; consumers get the archive copy when both exist. acceptable, but the adapter
  should not assume the returned path is the pinned source path.

### S1 (should-fix, c2 must handle) wrong or default workspace can poison the archive permanently
`artifacts.py:196-199` (`workspace=None -> root.parent`), `artifacts.py:692-712`
(`register_file`), `artifacts.py:780-860` (`_rebuild`).

- item 3 asked: does a wrong-workspace upgrade/rebuild write anything? verified safe.
  `Archive.upgrade` on a legacy index and `Archive.rebuild` on a modern one with
  `workspace=root.parent` fail with `ValueError: missing or corrupt registered artifact`
  and leave every file byte-identical, inode/mtime identical, tables intact, no head. the
  only new file is the publication lock file `capture-locks/<hash>.lock` (the coordinator's
  rehearsal saw the same). reason: the `DROP TABLE` / `CREATE` / `_index_row` /
  `_dependencies` all run inside one `with archive.db:` after `BEGIN IMMEDIATE`; the
  `ValueError` from `_dependencies` (`artifacts.py:839-840`) exits the context and rolls
  back before `_generation_intent`, head, or replace.
- exception (verified): a damaged-index `rebuild` with the wrong workspace fails cleanly
  too, but leaves `.pending-index-<uuid>.sqlite` plus the intended
  `index.corrupt-<sha>.sqlite` forensic copy (`artifacts.py:822-825`; no cleanup in the
  failure path). audit reports `.pending-` as staging, not an error. note-level.
- the real hazard is writes under the default workspace (verified): with
  `Archive(project/data/archive)` (workspace `project/data`) `register_file(project/data/raw/y.pdf)`
  succeeds and records `scope=workspace path=raw/y.pdf`. under the correct workspace that
  receipt is unresolvable: `audit()` reports an error and `Archive.rebuild(...,project)`
  fails with the same ValueError. receipts are immutable and rebuild validates every
  receipt, so one bad registration blocks all future upgrade/reindex with no remedy. c2
  adds many `copy=False` registrations, so this is the easiest way to wreck the live
  archive. sources outside `data/` are rejected (`path outside root`), data files are not.
- fix: in `register_file(copy=False)` refuse unless the Archive was opened with an explicit
  workspace (track `self._workspace_explicit`), and add a cheap witness check: if the index
  already has any `scope='workspace'` location, require that
  `workspace/<that path>` exists (existence only, no hashing). on the live archive the
  default would fail that check at once. the c2 runner should also assert
  `archive.workspace == <project root>` from a known marker (e.g. `GOAL.md`).

### S2 (should-fix for c2 replay) saved 429/503 on a host aborts offline replay of any URL on it
`capture.py:291-299`, ordering relative to `capture.py:303-310`.

- the throttle check precedes the cache lookup and does not look at `policy.budget`. verified
  (e6): after `register_saved` of a 503 row for one URL, offline `capture()` of a different
  URL on the same host raises `StopCollection('collection checkpointed after throttling')`
  instead of returning a saved success / `offline_unavailable`. with `state=None` it uses
  `archive.safety_references(host=...)` and behaves the same.
- fix: when `not self.policy.budget`, skip the throttle stop (nothing is dispatched) and
  go straight to cache / `offline_unavailable`; keep it for budget>0. a saved denial still
  returning `capture_reuse outcome=blocked` over a saved success for the same exact URL is
  by design; document it for adapters.

### S3 (should-fix before any live use; not a c2 offline issue) body errors outside `OSError` skip result publication and strand the reservation
`capture.py:416`.

- `except (TimeoutError, urllib.error.URLError, OSError)` omits `http.client.HTTPException`.
  `IncompleteRead` from a truncated chunked body is not an `OSError`. verified (e4): mock
  transport observes 403 then raises `IncompleteRead`. the 403 denial event is persisted
  (good, R1 holds), but no `capture_result` is published; the reservation is
  `uncertain` with `status=403, result_id=NULL`; account stopped `interrupted attempt`.
  `reconcile_result` needs a published result and `resume_account` needs every reservation
  in the archive complete, so there is no automatic exit and every account is globally
  stopped (`_eligible` `new=True`). fail-closed but unrecoverable without hand-authoring a
  result receipt.
- fix: catch `http.client.HTTPException` (and `ValueError`) in that clause so the status-based
  outcome and `body_error` are published and `reconcile_result` can finish it.

### S4 (should-fix, ops) time-based resume paths die after reboot and have no reconcile API
`campaign_state.py:615` (`_eligible` boot compare), `:851` (`resume_throttle`),
`:791-798` (`_record_status`), `:826-858`.

- item 1 conclusion: cross-process monotonic comparison inside one boot is correct and
  safe on Linux. verified (e4): a spawned process with the real `/proc` boot id and real
  clocks admitted a request on an account with `deadline`+`window` created by another
  process. CLOCK_MONOTONIC is system wide; the wall-vs-mono cross check (`abs(...)>2`) and
  `elapsed<0` check remain as defence in depth. leases are not time based at all (read:
  `expires` is written at `:564` and never read; any existing row means `LeaseBusy`), so
  they are immune to clock jumps and boot changes.
- fail-closed but unrecoverable: an account with `deadline`/`window` and any throttle event
  carry the creating boot id. after a reboot (or a suspend/resume, because
  CLOCK_MONOTONIC excludes suspend and then disagrees with wall by >2 s; suspected, the
  physical suspend was not exercised) `_eligible` raises `StopCollection` for every call and
  `resume_throttle` raises `unreconciled throttle timing/policy`. the contract says "block
  until reconciled" but no reconcile operation exists. on macOS this was masked because
  nothing could resume across processes anyway. a throttle ingested from a saved 429/503 row
  gets `boot=<current>` too (`ingest_legacy_status` -> `_record_status` stamps current time),
  so it also becomes unresumable after the next reboot.
- c2 itself is offline and quota zero, so this does not block c2. fix before c3/live: a
  state method, e.g. `reconcile_clock(account_id,decision)`, that records a recovery event
  and re-baselines `start_wall/start_mono/boot` only for a window/deadline account (shrinking,
  never extending, remaining time) and a decision-based throttle resume that accepts a
  boot change when a reviewer names the events. for deadline accounts a persistent wall
  step (NTP correction after allocation) also needs this.
- tests (read): every timed-account test injects `boot='fixture-boot'/'new-boot'` and
  fake clocks (`test_campaign_state.py:64,158,182`); the real `_boot()` is never used with a
  `deadline`/`window` account, and the spawned Q1 workers have no timed account. so the
  real Linux path was untested. add: a spawned-process test with a window account on real
  `_boot()` (skip if no `/proc`), and a test that `_boot()` falls back to `process-<pid>`
  when `/proc` is unreadable (that fallback is not unique across restarts in containers
  where pids repeat; add a per-process uuid. low risk because the wall cross check still
  trips, suspected).

### S5 (should-fix, c2 design) derivation recipe identity is whole-file and already forked
`derivations.py:18-25` (`code_sha256` of `derivations.py`), `:46-50` (`run` equality check).

- verified by hash: the 3 live `recipe-*` receipts record `code_sha256 d7d101e9a699…`; the
  current `derivations.py` hashes `33cd34486c…` (changed in c1; recorded in c1-acceptance.json).
  python is 3.14.7 in both, so the Linux move did not change that part. consequence (read):
  `Derivations.run(<old recipe id>)` raises `recipe does not match installed
  implementation/runtime`, and re-registering yields new recipe IDs, so old `derivation-run`
  results are never reused (lookup is by recipe_id). any further edit to `derivations.py`
  forks identity again and invalidates c2 work. keep c2 adapters/recipes in separate modules,
  or hash per-recipe code. also `run` writes a new `run-<uuid>` receipt on every
  failure (`:61-86`), so deterministic failures are not idempotent on replay (C1).

### N1 (note, item 2) `CampaignState.identity` binds the store to the absolute path
`campaign_state.py:211-213`, enforced at `:365-370`, plus marker/seal files (`:64-100`).

- verified (scratch e2): copying an initialized archive to another path makes
  `CampaignState(new)` (even `readonly=True`) fail with `unsupported/mismatched operational
  schema or archive identity`; `initialize`, `upgrade` and `migrate` all refuse
  (`existing/lost authority`, `completed authority cannot be remigrated`). `Archive` itself
  still opens and is ready, because the archive index/head carry no path. recovery path
  today: none through the API. mechanical workarounds only: edit
  `operational_meta.archive_id`, `capture-authority.json` and `capture-authority-ready.json`
  by hand (they are 0444 immutable and cross-bound), or a bind mount at the old path
  (symlinks are rejected by `_safe`).
- the property is useful: a rehearsal authority (scratch-path bound) can never be copied back.
  keep it. the user confirmed this path is long-term; document the limit in
  `docs/archive-storage.md`.
- what a future `relocate` needs: quiesced explicit call under the publication lock; verify
  old state (authority marker + seal + SQL meta + ready head/no pending intent + no
  unreconciled reservation); keep `authority_id` (quota history) and write a new marker/seal
  with the new `archive_id` and an immutable `previous_archive_ids` chain; update
  `operational_meta.archive_id` and append a `recovery` event naming old/new id, decision and
  reviewer in one SQL transaction; make the order crash safe with a pending-relocate record
  that the open path accepts for either old or new id until finished; backup the old
  marker/seal first; never regenerate quota. the workspace-scope receipts are project relative
  and survive a move unchanged (only S1's workspace must be passed correctly).

### N2 (note, item 5) `transport()` keeps status-before-body; HTTPError handling is fine on 3.14
`capture.py:124-149`.

- verified with mocked `OpenerDirector.open` (scratch e3): `HTTPError` with a body, with
  `fp=None` (3.14 substitutes an empty `BytesIO`), 301 under `_NoRedirect`, and 403/429
  all yield `observe_response(status,url,headers)` before the first `read1`, and the right
  `Response`. `response.url` is the request URL for `HTTPError`. a pathological `fp` without
  `read1` raises after status is observed, so status still survives. dependency: the fp=None
  behaviour is a newer-Python feature; pin it with a test.
- minor: (a) the `deadline` is checked between chunks only; a slow-drip server can use
  up to timeout + per-read socket timeout, then the Collector rechecks post hoc
  (`capture.py:377`). (b) `build_opener()` includes `ProxyHandler` from environment proxies;
  nothing records that a proxy was used, and the persisted `headers` omit the real
  `User-Agent`/`Accept-Encoding` urllib adds, so "actual wire headers" is not complete.
  consider `ProxyHandler({})` and recording UA. (c) `Retry-After` is ignored; delay is
  `policy.throttle_seconds`. (d) DNS resolution is not bounded by `timeout`.

### N3 (note) other c2-relevant behaviour of existing API
- `Collector.capture` publishes a new `capture_reuse` receipt with a random uuid on every
  call (`:301,308,310,328`). the contract allows reuse events, but replay runs grow the
  archive and the sequence each time. adapters that need a quiet second offline run should
  not route replay through `capture`.
- with `state=None`, `register_saved` cannot ingest saved 402/403/429/503 rows into
  operational state (`_ingest_restrictions` returns early); the receipts are still indexed
  into `safety_lookup`, so a later `synchronize_archive` picks them up. the c2 runner must
  run it before any live step and must not claim restrictions were ingested while offline.
- `register_file` takes the publication lock and fsyncs per file; registering the c0
  inventory (1,121 paths) is one receipt per path, fine at that size but run it in one
  batch under a single integrator.
- adapter contract mapping (units/register_saved/next_work/validate/bundle, bound
  checkpoints, result-before-checkpoint resume): no c1 API blocks it other than B1, B2, S1,
  S2 above. result receipts are already durable and acknowledged before the state
  transition (`capture.py:424-427`), `reconcile_result` implements "result before checkpoint"
  for network captures, and `verified_receipt` gives acknowledged generation/sequence
  proof for bound checkpoints. checkpoint storage itself is the coordinator's new campaign_*
  work and was not reviewed.

## summary table

| id | severity | status | file:line | one line |
| --- | --- | --- | --- | --- |
| B1 | blocker (A1 via Collector) | verified | capture.py:440-464, 265-269; artifacts.py:601-622 | saved rows cannot carry slot/method/body/scope; offline lookup ignores scope |
| B2 | blocker (contract) | verified | capture.py:449-454 | register_saved copies; add `copy=False` via `register_file`; saved IDs stay stable |
| S1 | should-fix | verified | artifacts.py:196-199, 692-712 | default workspace register_file poisons archive; rebuild with wrong ws itself is clean |
| S2 | should-fix | verified | capture.py:291-299 | saved 429/503 aborts offline replay of the whole host |
| S3 | should-fix (live) | verified | capture.py:416 | IncompleteRead after 403 strands an unreconcilable reservation |
| S4 | should-fix (ops) | read | campaign_state.py:615, 851 | no clock/boot reconcile; real `_boot` untested with timed accounts; cross-process monotonic itself is correct |
| S5 | should-fix (design) | verified | derivations.py:18-25, 46-50 | recipe identity = whole file hash; live recipes already unmatched |
| N1 | note | verified | campaign_state.py:211-213 | path identity: moved workspace is unusable; relocate sketch above |
| N2 | note | verified | capture.py:124-149 | transport status-before-body holds for HTTPError; minor items |
| N3 | note | read | capture.py various | reuse-event growth, offline restriction ingestion, batching |
