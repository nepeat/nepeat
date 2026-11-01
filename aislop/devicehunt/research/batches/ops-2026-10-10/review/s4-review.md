# s4_reconcile review

Verdict: REJECT (one must-fix: window accounts can gain time on a same-boot backward wall step; otherwise sound)

Tests: `python3 -m unittest tests.test_campaign_state tests.test_campaign` -> Ran 69, OK (verified, ~80 s).

## must-fix

1. campaign_state.py:1245-1252 `reconcile_clock` can EXTEND remaining window time (verified by probe).
   `start_mono = mono - wall_elapsed` uses wall elapsed unconditionally. On the same boot with valid
   `mono_elapsed >= 0`, a backward wall step (NTP) makes `wall_elapsed < mono_elapsed`; reconcile then
   moves consumed time DOWN to the smaller wall figure. Probe (scratchpad/probe_s4.py): window=100, 80s
   consumed (20 left), wall stepped back 50s, same boot -> `_eligible` refuses, reconcile accepted,
   remaining becomes 70. A second backward step of 20s and another reconcile gives 90. Repeatable, so
   time can be gained repeatedly. This violates the spec ("shrinking, never extending") and the
   handoff claim "remaining time never grows". Window semantics are elapsed seconds, so the
   known monotonic consumption must be kept.
   Fix: when `self.boot == row['boot'] and mono_elapsed >= 0`, use `consumed = max(wall_elapsed, mono_elapsed)`
   and set `start_mono = mono - consumed`. Across a boot change mono is meaningless and wall is the only
   evidence; a backward wall step there can still understate consumption (cannot be fixed, see note 1).
   Add a test: same-boot backward wall step must not increase remaining (this is the missing mutation test;
   `test_S4_reconcile_after_drift_same_boot...` only covers a FORWARD step, where wall > mono, so it passes
   with either formula).

## should-fix

1. Test gap: no test for backward wall step on the same boot (see must-fix), nor for repeated reconcile
   after a second anomaly not increasing remaining time.

## notes

1. Deadline accounts: safe. `deadline`/`start_wall` unchanged (verified at :1252 UPDATE touches only
   start_mono, boot). After reconcile `start_wall+elapsed == wall`, so `_eligible` (:828) reduces to the
   absolute `wall >= deadline` check. A wall step back extends a wall deadline inherently; that is the
   documented semantics, not a reconcile defect. Forward wall step: consumes more than real (fail-closed). OK.
2. `_eligible` (:814-846) unchanged (read; git diff scope per handoff). It never writes `accounts.stopped`
   (verified: only writers are `uncertain` :974, cancel, resume_account; grep).
3. Refusal rules all present at :1233-1251: empty decision (ValueError), missing, stopped, untimed, open
   reservation (global `_OPEN_RESERVATIONS`), negative wall elapsed, non-anomalous. Healthy-account faking:
   requires a real drift >2 s or boot change, so not abusable beyond the must-fix case. Repeated calls on the
   same anomaly: second is refused as healthy (tested for throttle, implied for clock).
4. Guard pattern matches `settle_unobserved`: `archive.publication_guard()`, `self.db(True)`,
   `_require_archive` (:1232-1234); a `recovery`/scope account event with decision, policy hash and reason is
   recorded (:1253). It does not take `self.locks.held` like settle_unobserved, acceptable since no reservation
   fence is touched. Single transaction so refusal leaves nothing written.
5. `reconcile_throttle` (:1051-1080): original row untouched (insert-only event); wall delay never shortened
   (`eligible_wall = max(old, now)`, remaining = max(0, old - now)); new `eligible_mono = mono + remaining` keeps the
   resume drift check consistent (verified algebraically; test confirms). `_throttle_timing` (:1036) picks the newest
   by `ORDER BY sequence DESC` and keys on `target = event['id']` plus scope/provenance, so it cannot be pointed at
   another throttle's reconcile. policy_hash on the resume check still enforced (:1109, uses the original
   `policy_hash` which the reconcile does not alter). Scope `throttle` recovery events do not match
   `_restrictions` wanted scopes (url/host/source/resource), so they are not treated as restrictions (read).
   reconcile_throttle takes no archive guard or open-reservation check; matches `resume_throttle` (read, minor).
6. `_boot()` fallback (:38-43) is `process-<pid>-<per-interpreter uuid>`; /proc path identical. Test uses
   spawn Process with patched pid (verified in code). Every restart on a host without /proc forces a reconcile,
   which is the intended conservative behavior.
7. `reservations.boot` is write-only: grep of `boot` across devicehunt, scripts, tests shows reads only at
   campaign_state.py:825 (accounts), :1109 (events), :1250/:1073 (new code); the reservations INSERT at :888-890
   is the only use (verified).
8. Cross-process test uses `multiprocessing.get_context('spawn')` (not subprocess) for both workers; the real
   boot test simulates reboot by rewriting the stored boot through sqlite, acceptable. It skips without /proc.
9. Tests otherwise bite: the boot-change window test asserts exact consumption (99 admitted, 100 refused) and a
   later restart; mutation to `start_mono = mono` (reset) would fail it. The forward-step test would not catch
   the backward-step defect (must-fix).

## re-review

Verdict: ACCEPT with notes (previous must-fix resolved; no new must-fix)

Verified: probe_s4.py now prints remaining 20 then 20 (was 70, 90). `python3 -m unittest tests.test_campaign_state tests.test_campaign` -> Ran 73, OK.
Mutation check: reverting to `consumed = wall_elapsed` (old formula) makes 4 tests fail
(`test_S4_same_boot_backward_wall_step_never_gains_time` and three subtests of `test_S4_repeated_reconciles_never_gain_time`); source restored afterwards (verified, diff unchanged).

start_wall deviation (campaign_state.py ~1252-1260), read plus algebra:
- New `start_wall = wall - consumed`, `start_mono = mono - consumed`. Same clock: consumed >= wall_elapsed, so new start_wall <= old start_wall (never later). Otherwise consumed = wall_elapsed, so start_wall is unchanged up to float rounding (see note 1).
- Deadline accounts: `_eligible` :827 checks `wall >= deadline` (absolute, unaffected) and `start_wall + elapsed >= deadline`; after reconcile start_wall + elapsed == wall, so the check is identical to the wall check. The deadline cannot be extended; moving start_wall earlier only matters for the drift cross-check.
- Window accounts: elapsed == consumed after reconcile, and consumed >= prior monotonic consumption on the same boot, >= wall elapsed always. Remaining never grows on the same boot. Repeated reconciles: each step sets consumed to a max over evidence that already includes the prior consumed (start_wall and start_mono are re-baselined consistently), so the sequence is monotone; the tests cover backward/forward/backward and forward-then-reboot.
- Other readers of `accounts.start_wall`: grep of devicehunt and scripts shows only `_eligible` (:825, :827). `activations.start_wall` (:717), `campaign_runs.start_wall` (campaign.py) and `reservations.start_wall` (capture.py:350) are different tables. `account()` just returns the row. Nothing changes meaning.

Notes:
1. Float rounding: `wall - (wall - start_wall)` can differ from start_wall by an ulp, so "never later" is exact in math but not guaranteed bitwise on the boot-change path. Harmless (consumption error ~1e-13 s). If desired, keep `start_wall` verbatim when `consumed == wall_elapsed`.
2. Stale text: the recovery event reason still says "(start_wall kept)" and the event does not record old/new start_wall, though start_wall may now move. Should record both for audit. Should-fix, low severity.
3. Remaining limit unchanged: a backward wall step combined with a reboot cannot be detected (only wall evidence exists). Documented in the docstring.
4. `test_S4_repeated_reconciles_never_gain_time` has a dead `step is None` reboot branch (no reboot case in the loop), so the repeated-reboot sequence is only covered by the separate forward-then-reboot test. Cosmetic.
