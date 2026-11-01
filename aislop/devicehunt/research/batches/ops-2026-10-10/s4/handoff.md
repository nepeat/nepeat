# s4_reconcile handoff

Scope: c1 follow-up S4, explicit clock/boot reconciliation. Files touched: `devicehunt/campaign_state.py`,
`tests/test_campaign_state.py` (new tests only), this file. `_eligible` is unchanged.

## Changes (devicehunt/campaign_state.py)
- `:35-43` `_PROCESS_UUID` (once per interpreter) and `_boot()` fallback now `process-<pid>-<uuid hex>` when
  `/proc/sys/kernel/random/boot_id` is unreadable. The /proc path is identical.
- `:1036` `_throttle_timing(db, event)` returns the throttle event with the timing of its newest
  `explicit-throttle-clock-reconcile` recovery event, else the row unchanged.
- `:1051` `reconcile_throttle(event_id, *, decision)`.
- `:1108` `resume_throttle` now passes each active throttle through `_throttle_timing` before its existing
  boot/policy/eligible/drift checks (checks themselves unchanged).
- `:1220` `reconcile_clock(account_id, *, archive, decision)`.

## reconcile_clock rules
Guard pattern as `activate`/`settle_unobserved`: `archive.publication_guard()`, `self.db(True)`,
`self._require_archive`. Refuses (ValueError for empty decision, else StateError, nothing written) when: decision
empty; account missing; `stopped` set; no deadline and no window; any open reservation (the global
`_OPEN_RESERVATIONS` predicate, i.e. non-complete and not settled by `unobserved-settlement`); wall elapsed
(`now_wall - start_wall`) negative; timing healthy (same boot, mono elapsed >= 0, |wall elapsed - mono elapsed| <= 2).
Otherwise (revised after review REJECT): `consumed = max(wall_elapsed, mono_elapsed)` when the boot is unchanged
and `mono_elapsed >= 0`, else `wall_elapsed` (boot change, or monotonic below the account start: monotonic is then
meaningless). `start_mono = now_mono - consumed`, `boot = self.boot`, and `start_wall = now_wall - consumed` (this
only moves `start_wall` earlier, and only when monotonic consumption exceeds wall elapsed, i.e. after a backward wall
step; without it `_eligible`'s drift cross-check would still fail). `deadline`, `window`, quotas untouched. On the
same boot remaining time never grows (backward wall step cannot refund time; repeated reconciles are monotone).
LIMIT: after a boot change only wall time is evidence, so a wall step back across a reboot (or earlier uncorrected
steps) can understate consumption and cannot be detected. Consumed time is kept; an already-exhausted account stays
exhausted. Suspend time counts as consumed (conservative). Records `recovery`/scope `account`/target = account,
provenance `explicit-clock-reconcile`, the decision, policy hash, reason with old/new boot, drift, wall and mono elapsed.
Quota-wise it makes no request and refunds nothing.

## reconcile_throttle rules
Events are append-only (immutable rows), so a reconcile is a new `recovery` event, scope `throttle`, target = throttle
event id, provenance `explicit-throttle-clock-reconcile`, carrying the re-stamped `eligible_wall`, `eligible_mono`,
`boot` and the original policy hash. `resume_throttle` reads the newest such event as the effective timing.
Re-stamp: `remaining = max(0, eligible_wall - now_wall)`; `eligible_mono = now_mono + remaining`;
`eligible_wall = max(eligible_wall, now_wall)` (wall delay never shortened; when it already elapsed the wall stamp
advances to now so the existing drift check in resume stays consistent; otherwise resume would hit the >2 s check).
Refuses: empty decision (ValueError); unknown event or not a host throttle; already resumed (has an `event_triggers`
row); no timing; timing not anomalous (current boot and |drift| <= 2). It does not resume; `resume_throttle` with its own
decision, quota, policy and "not yet eligible" checks still applies. No archive argument (matches `resume_throttle`).

## Is the `_eligible` StopCollection persisted into `accounts.stopped`? No.
Writers of `accounts.stopped`: `uncertain()` (reasons 'response/transport failure' / 'interrupted attempt', called
from capture.py:422/437 after a reservation exists), `cancel()` ('cancelled: ...'), and `resume_account` clears it.
`_record_status` writes only `events`. `reserve()` runs `_eligible` before inserting a reservation, and nothing in
capture.py/campaign*.py converts an `_eligible` StopCollection into `uncertain`/`stopped`. Decision: reconcile does
not touch `stopped` and refuses a stopped account (tested). Residual: a reboot mid-flight leaves an open reservation;
that is `uncertain` + stopped via the existing path and needs the existing settle/reconcile_result first.

## reservations.boot grep
`grep -rnE "(row|res|reservation|r)\['boot'\]" devicehunt scripts` and `grep -rn boot devicehunt scripts tests | grep -i reserv`:
`reservations.boot` is written only (campaign_state.py `reserve` INSERT, ~:888-890) and never read or compared anywhere.
The only `['boot']` comparisons are `_eligible` (`accounts.boot`, :825), `resume_throttle` (`events.boot`) and the new code.

## Tests (tests/test_campaign_state.py, all prefixed test_S4_)
boot change preserves remaining window (and later restart needs its own reconcile); drift same boot; negative
monotonic elapsed; deadline account (absolute wall deadline kept); refusals leave state unchanged (empty decision,
healthy, unknown, untimed, wall backward); stopped; open reservation; recovery event contents and `stopped` untouched;
throttle reconcile after boot change through resume (immutable original row, second reconcile refused, resumed refused);
throttle with elapsed wall delay + same-boot drift; `_boot()` fallback unique per process with identical faked pid
(spawn child); real `_boot()`/real clocks cross-process (spawn, skipped without /proc boot_id; simulates reboot by
rewriting the stored boot via sqlite, then reconciles). Helpers `_fallback_boot_worker`, `_real_boot_worker` are top level.

## Verification
- `python3 -m unittest tests.test_campaign_state tests.test_campaign`: Ran 73, OK (15 new S4 tests; the pre-existing
  count was 57).
- `python3 -m unittest discover -s tests`: Ran 638 tests, 1 error, 871 s. Error is not from this change:
  `test_review_queue.MovedBundleBaselineTests.test_manifest_without_a_gate_falls_back_to_the_path_match`
  (`ReviewError: excluded entries carry no inputs`, devicehunt/review_queue.py:50). review_queue.py and its tests are
  owned by another worker who was editing them concurrently, so this looks like their in-flight state; I did not touch it.

## Notes / risks
- `reconcile_clock` takes the global `_OPEN_RESERVATIONS` (same as `_eligible(new=True)`), so another account's open
  reservation also blocks it; conservative, and admission would be blocked anyway.
- Reconcile only after a human confirms no other process is using the account (the open-reservation check does not
  see a process that has not reserved yet; admission afterwards is re-checked by `_eligible` as usual).
- campaign_state.py changed, so per PROGRESS the next offline campaign run re-executes its 36 jobs (already expected).
- PROGRESS item 5 S4 can be marked done by the coordinator; the new methods are not wired to any driver/CLI.

## Review fix (s4-review must-fix)
Added tests: same-boot backward wall step never gains time (80 s consumed, wall -50: 20 s left after reconcile),
repeated reconciles (backward, forward, backward) never increase remaining, forward step then reboot, and boot change
using wall elapsed. Verification after the fix: `tests.test_campaign_state tests.test_campaign` Ran 73, OK; the full
suite was not rerun (coordinator).

## coordinator follow-up (after re-review ACCEPT with notes)

- re-review notes 1 and 2 applied by `/root-claude`: `start_wall` is written back unchanged unless
  `consumed > wall elapsed` (no float round-trip on the boot-change path), and the recovery event
  reason now records `start_wall old -> new` and `consumed` instead of the stale "(start_wall kept)".
  `python3 -m unittest tests.test_campaign_state tests.test_campaign`: 73 OK.
- driver: `research/batches/hunt-2026-10-10/c4/live_rra.py reconcile ACCOUNT --label L --decision D`
  (no HTTP; report records the account row before and after).
- note 4 (dead reboot branch in `test_S4_repeated_reconciles_never_gain_time`) left as is; the reboot
  sequence is covered by the separate forward-then-reboot test.
