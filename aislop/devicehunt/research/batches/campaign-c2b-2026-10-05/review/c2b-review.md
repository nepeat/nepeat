# c2b review: RRA live path through mocked transport

reviewer: `c2b_review` (Claude Sonnet 5.5), 2026-10-05. Wrote only this file.
Ran `python3 -m unittest tests.test_campaign_rra_live tests.test_capture -q`: 33 tests OK.
All experiments ran in temp dirs and the session scratchpad. `data/archive` was only read.
Tags: verified = reproduced by running something; read = from code reading only.
Note: `fcc_shenanigans/` is untracked, so there is no git baseline. Pre-change capture.py could not be diffed.

## summary

No blocker for the mocked milestone. A1, R1, P1, Q-gates and C1 hold for the paths tested.
The B1, S2 and S3 fixes are correct and do not weaken live behaviour. Three should-fix items
(F1-F3) should be closed before c4, F1 preferably now.

## findings

### F1 should-fix: validator exceptions other than ValueError strand the account with no result
`campaign_rra_live.py:78-79` (`_csv`), `:103` (detail lambda), `:107` (empty result);
mechanism at `capture.py:411` (`except (ValueError,SyntaxError)`) and `:433`.
- Scenario: a 200 CSV body with a malformed quote, or a field over 131072 bytes, makes
  `csv.reader(strict=True)` raise `_csv.Error`, which is not a ValueError. Deeply nested HTML makes
  `parse_detail` or `validate_empty_result` raise `RecursionError`.
- The exception escapes the validator and `Collector.capture`. The outer `except BaseException`
  marks the reservation `uncertain`, no `capture_result` is published, and `RraRefresh.capture` raises.
- Reservation state is `uncertain` with `result_id NULL`, account `stopped='interrupted attempt'`, and
  `reconcile_result` has nothing to reconcile. The received bytes are never stored.
- A hostile or garbled upstream response therefore halts the shared account instead of yielding
  `invalid`. Safe-fail, but dishonest to the contract and operationally a trap.
- Verified: e3 script (`"abc` CSV through `RraRefresh`: raised `Error`, states `[complete, uncertain]`).
  Direct validator calls: `_csv` gives `_csv.Error`; `parse_detail` and `validate_empty_result` give `RecursionError`.
- Fix: wrap each validator in this module so any `Exception` other than ValueError is re-raised as
  `ValueError(type(exc).__name__)`. Or widen the catch at `capture.py:411` (the module-local wrap is smaller).

### F2 should-fix: reservations are not bound to the unit revision
`campaign_rra_live.py:141-157`.
- `collector_for` is caller-built, and `Collector.unit_revision`/`run_id` default to `capture/1`/`capture`.
  The slot scope carries `self.unit_revision`, but the durable reservation does not.
- Verified: reservations show `unit_revision='capture/1'` while the refresh was `u`; the tests also build collectors this way.
- Contract c1a: a reservation records account/run/unit revision. Quota audits per unit become impossible.
- Fix: after `collector_for(policy)`, assert `collector.unit_revision == self.unit_revision` (raise otherwise).
  Make the test's `collector_for` pass it.

### F3 should-fix before c4: no request pacing
`campaign_rra_live.py:147-169`.
- `research/capture_rra_details.py` sleeps 0.15 s between details.
- The new runner issues up to `max_details + 3` requests back to back. `Policy.throttle_seconds` only sets a
  resume time after a 429/503, and `campaign_state` has no minimum inter-request interval (grep: none).
- Scenario: 200 details become a burst heavier than the curl flow, so the first sign of trouble is the throttle.
- Fix: an injectable `pause(seconds)` (real `time.sleep` in c4, a no-op in tests) with a declared per-scope
  gap, called between steps. Add a test that pause runs between dispatches and not on reuse.
- Read only.

### F4 note: resumed failed slots change outcome name
`campaign_rra_live.py:124-126, 167-168`.
- A slot that originally ended `invalid` (or `empty`) comes back as `saved_unavailable` and maps to
  `unavailable` when the same cycle is rerun. Original `failed` becomes `unavailable`.
- No double dispatch. Verified: e4, `failed invalid 4` then `unavailable saved_unavailable`, calls still 4.
- Fix: map by `prior_capture_id`'s receipt outcome; fall back to `unavailable` only if that is unreadable.
- The cycle is also permanently dead for that slot until a new `cycle_id`, which is acceptable but should be documented.

### F5 note: `timeout` and `unavailable` hide that the reservation is uncertain
`campaign_rra_live.py:124-126, 165-168`.
- A transport exception publishes `timeout` or `unavailable` with the reservation `uncertain` and the account stopped.
  An ordinary HTTP 404 `unavailable` ends `complete`. The step dict does not distinguish them.
- Verified: S3 correct post-status. Pre-status `BadStatusLine` publishes `unavailable` with `status=None`, reservation
  `uncertain`, and `reconcile_result` refuses (`ambiguous request cannot finish from guessed result`).
  There is no automatic exit, the same as pre-existing OSError/URLError behaviour.
- Fix: include `status`, `error` and `reservation_id` in each step from the result, and return
  `outcome='interrupted'` when the reservation is uncertain.

### F6 note: slot identity and cycle freshness are caller conventions
`campaign_rra_live.py:141-142, 144-169`.
- `unit_revision + ':' + cycle_id + ':' + slot` is ambiguous when either contains ':' (an ISO timestamp cycle
  id is likely): `('a:b','c')` and `('a','b:c')` collide, so cross-cycle reuse is possible.
- Nothing enforces a fresh `cycle_id`. Rerunning with a reused id gives `captured` with zero new
  observations and a bundle whose before/after are old.
- Fix: serialize the scope as JSON `[unit, cycle, slot]` or reject ':'. Report `new_observations=N` (or
  outcome `captured_from_reuse` when 0). Read only for the collision; the all-reused path is verified by the c2b test.

### F7 note: `register_saved` cannot declare headers, and slot/resource do not enter identity
`capture.py:458-462`; test at `tests/test_capture.py:226-229`.
- A saved POST registered with context is found only if the replaying `capture()` passes no headers.
  `RraRefresh` sends `Content-Type`, which is in `fingerprint`.
- Verified (e2): offline `capture(POST, body, headers=None)` returned `reused`; with the live form headers it
  returned `offline_unavailable`. The B1 test calls `_cached(request_id)` directly, so it bypasses this.
- `slot`/`resource`/`unit_revision` given without any scope or non-GET/body leave `declared` False: the legacy
  request_id is used and two slots would still collide.
- Fix: add `headers=None` to `register_saved`'s fingerprint, or document that replay must omit headers. Require a
  scope whenever `slot` is supplied. Add a `capture()`-level offline test with the live headers.

### F8 note: `_stop_outcome` matches free text
`campaign_rra_live.py:114-121`.
- `'cancelled: <decision>'` or a `stopped` reason containing "quota", "window" or "throttl" mis-maps.
  `durable source restriction` and `restriction arrived before dispatch` map to `blocked` but can be throttle events.
- Both are reachable only by races. Read only.
- Fix: map exact known strings, else `stopped` with `reason`.

### F9 note: provenance for resumed slots
`campaign_rra_live.py:176`.
- On resume, `capture_receipts` records the `capture_reuse` receipt id, not the original `capture_result`. Fix:
  record `prior_capture_id` (from the reuse receipt) plus whether the slot was a new observation.

### F10 note: fidelity gaps that need a c4 smoke, not testable offline
- No redirects, whereas curl used `--location`. User-Agent is `Python-urllib/x` versus `curl/x`. Either could
  change the real server's answer; the mock cannot show it.
- `candidate.write_text(...)` at `:202` and `rra.validate`'s `read_text()` use the default encoding with
  `ensure_ascii=False`. It is consistent in-process; pass `encoding='utf-8'` for portability.
- Detail manufacturer matching (`rra.validate`, `rra.py:164`) runs only at `bundle()`, after every request is
  spent. Checking it inside the detail validator fails fast. The run also never checks remaining account
  quota against `3 + n` before the first detail.

## focus-question answers

1. **A1: holds.** Each slot has its own `observation_scope`, which enters the request_id. csv-before and
   csv-after therefore have distinct request_ids and distinct reservations. Older cycles and the before-export
   cannot satisfy it. Saved rows are `context_known=0`, so they never satisfy live reuse
   (`artifacts.py:612`). Verified by tests (two CSV calls, cycle-2 makes 4 more calls). The only gaps are F6
   (id collision, reused cycle id).
2. **Detail allowlist/cap: holds.**
   - Details come only from `rra.csv_rows(observed['csv-before'])`. The cap is checked before the first detail
     request, and `bundle()` refuses a non-`captured` set.
   - A partial set cannot write `bundle.json`: the rename is after `rra.validate`, and a failure leaves
     only `candidate.json`.
   - URL encoding is identical to the helper and `rra.validate` (`quote(safe='')`).
   - Body bytes verified (e3): `A&B Co+Ltd 한국 ~x*` becomes `A%26B%20Co%2BLtd%20%ED%95%9C%EA%B5%AD%20~x%2A`
     with the helper's field order and empty fields. That matches `curl --data-urlencode` rules (unreserved
     `A-Za-z0-9-._~`, spaces as `%20`, UTF-8). Read, not run against curl, to stay offline.
   - File names `search-form.html`, `before.csv`, `after.csv`, `empty-result.html`, `detail-%03d.html`, request
     order form, before, [details | empty], after, and detail ordering all match the helper.
3. **Outcome mapping.** Mostly honest. `reused` and `not_modified` are only reachable from a prior success.
   `not_modified` has no refresh here (`refresh` is False), so it is dead but harmless. `saved_unavailable`,
   `offline_unavailable`, `blocked`, `throttled`, `invalid`, `empty` and `timeout` are all non-success. See F4, F5, F8.
4. **Resume/crash.** Same-cycle resume reuses published slots with no dispatch. An interrupted reservation
   blocks (`unreconciled charged reservation`) until `reconcile_result` and `resume_account`, then
   resumes without re-requesting. I found no double dispatch and no lost durable result. The F1 route
   (no result published) is the only loss case: a validator crash drops bytes and strands the account.
5. **capture.py.**
   - B1: verified. 53 real saved `capture_result` receipts in `data/archive/receipts` (41 success, 12 error),
     replayed through the new `register_saved` in a fresh archive, gave 53/53 identical `saved-` ids.
   - S2: the throttle stop is `self.policy.budget and ...` (both branches), so budget>0 is unchanged. Budget 0
     never dispatches (`capture.py:313`). The test covers it.
   - S3: verified for `IncompleteRead`, `BadStatusLine`, `RemoteDisconnected`, `LineTooLong`, `InvalidURL`,
     `NotConnected`. A result is published each time and the reservation is `uncertain`, never `complete`
     without a status (the `complete` transition is guarded by `if not uncertain`).
   - `scoped=True`: only affects budget-0 lookups. `request_id` already hashes the context, so honest data
     is unaffected. Live (`live or scoped`) is unchanged.
6. **Metadata safety.** `Set-Cookie` is dropped by `_response_metadata` and the test asserts the secret is
   absent from all receipts. The `acquisition` block holds only caller strings, `capture_receipts` ids and a method
   string; no headers, paths, hosts or accounts. Residual: a `Location` header (for example a
   `jsessionid` URL) is not in the blocked set and would persist; declare it in `Policy.sensitive_headers` at
   c4. The test scans only `receipts/*.json`, not `bundle.json`, `index.sqlite` or the state DB.
7. **Fidelity.** The schema is accepted by `rra.validate` (run in `bundle()`) and by `import_bundle`, which
   reads only known keys. The extra `acquisition` key is ignored by both. `RraSavedAdapter.records` reproduces
   the pinned record-set hashes for both pinned units (tests). `import_bundle` itself was not run.
   (A real import replaces that query's `rra_equipment` rows; that is a c4 import gate, not c2b.)

## missing offline tests (each is quick)

- Validator hardening: malformed-quote CSV and deep-HTML detail responses yield `failed` and a published
  `invalid` result, with the reservation `complete` (F1).
- Reservation `unit_revision` equals the refresh's (F2).
- Pacing hook between dispatches (F3).
- POST body golden test (`&`, `+`, space, non-ASCII, `~`, `*`) against `RraScope.body`.
- Scope-id collision across `('a:b','c')` and `('a','b:c')` (F6).
- Resumed failed slot keeps its original outcome (F4); `status`/`reservation_id` appear for transport faults (F5).
- A 503 on a detail throttles and stops, with no further dispatch.
- Secret scan extended to `bundle.json`, `index.sqlite` and the state DB; repeated and mixed-case
  `Set-Cookie` headers.
- Offline `capture()` replay of a saved scoped POST with the live form headers (F7).

VERDICT: ACCEPT with notes

## fix verification

2026-10-05, same reviewer. `python3 -m unittest tests.test_campaign_rra_live tests.test_capture -q`: 37 tests OK.
Extra experiments in the scratchpad only (temp archives, mocked transport); no `data/` writes.

- **F1 fixed.** `_strict()` wraps every validator (`campaign_rra_live.py:73-83`, applied in `next_work`).
  Verified (e5): a 20000-deep `<div>` detail gives `failed`/`invalid`, no uncertain reservation and
  `accounts.stopped` NULL. The new test covers the malformed-quote CSV (reservations all `complete`, resume keeps
  `invalid`). `_csv` and `parse_detail` were the two failure routes; both now yield results. The fail-fast
  manufacturer check works: an `Edge-Core` replaced by `Other` stops at detail-001 as `failed` (3 calls).
  Minor: the wrapped type name is lost, because Collector stores only `type(exc).__name__`, so `error` always reads
  `ValueError`.
- **F2 fixed.** `capture()` raises `ValueError` unless `collector.unit_revision == self.unit_revision`, before any
  dispatch. The test asserts all reservations carry `unit/fixture` and that a mismatched collector raises.
- **F3 fixed, with a small gap.** `min_interval=0.15` with an injectable `pause`. The test shows `[0.15]*9` over
  ten dispatches and `[]` on a fully reused run. Gap (note): pacing keys off "the previous step dispatched",
  not elapsed time since the last dispatch. In a dispatch, reuse, dispatch sequence the second dispatch is
  not paced. Not reachable in the normal flow, only after a partial resume. Fix if wanted: track a last-dispatch
  monotonic time.
- **F4 fixed.** `_prior_outcome` follows `prior_capture_id`. Verified by the new test: the resumed
  invalid slot reports `failed`/`invalid`, with 2 calls total and no re-request.
- **F5 fixed, one nuance.** Steps carry `status`, `error`, `reservation_id`, `observation_id` and
  `new_observation`. The OSError test returns `interrupted` with `status=None`. Nuance (note): an S3 case such as
  403 then `IncompleteRead` also returns `interrupted` (reason `blocked`, step `status=403`) rather than
  `blocked`. That is honest, since the reservation is uncertain and needs reconcile, and the denial is durable.
  Callers must read `reason`/`status`.
- **F6 fixed.** Scope is now `json.dumps([unit, cycle, slot])`. The colon-identity test passes (it mutates
  attributes after construction, which is fine for a pure function). `new_observations` is reported in every result.
  Not enforced: reuse of a `cycle_id` still returns `captured` with `new_observations=0`; callers must check it.
- **F7 fixed.** `register_saved(headers=)` enters the fingerprint only (values not stored). Verified (e2 rerun):
  saved with the form headers, offline `capture(headers=hdr)` returns `reused` and with `None` returns
  `offline_unavailable`. `slot` without a scope raises `ValueError('a saved observation slot needs an explicit
  observation scope')`. The legacy IDs are stable: 53/53 real saved receipts recompute identically after the change.
  Note: `resource`/`unit_revision` alone still do not enter identity; they are documented as metadata.
- **F8 fixed, one nuance.** Exact string sets plus classification from durable `restrictions`. Nuance (note): when
  both deny and throttle events exist the result is `throttled`. A deny is stricter, and `blocked` would
  be the safer label. Edge case only; reason is preserved.
- **F9 fixed.** `capture_receipts` records `observation_id` (`prior_capture_id` when reused, else the
  result id). Read plus the passing bundle tests; the `acquisition` block is still ignored by `rra.validate`.
- **F10 partial, as expected.** utf-8 candidate fixed (`:265`) and the manufacturer fail-fast is done. Still open and
  untestable offline: no redirect following and the User-Agent differ from the curl flow, so a c4 smoke is
  needed. No quota-versus-`3+n` precheck. A `Location` header is not in the sensitive set; declare it in
  `Policy.sensitive_headers` at c4. The secret scan still covers only `receipts/*.json`.
- Side effect to remember: `validator_id` for details is now `rra-detail/2`. Cached success is validated at reuse by
  the current validator, so a pre-fix cached detail that fails the manufacturer rule would raise `ValueError`
  out of `capture()` (not mapped). No such cache exists yet.

Nothing is blocking. Remaining items are notes for c4 readiness: the F3 gap, F8 deny-versus-throttle, the F10 smoke
items, and the unmapped `ValueError` on re-validating a stale cached slot.

VERDICT: ACCEPT with notes
