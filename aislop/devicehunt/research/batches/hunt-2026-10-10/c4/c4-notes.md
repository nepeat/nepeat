# c4 — first supervised live RRA refresh (2026-10-10)

Authorization: the user asked "explain more about the RRA refresh? How expensive? Give it a shot,
see how expensive it is." The coordinator set the finite scope before any request: one form-GET
smoke, then the explicit-zero unit, then the 7-row unit. After those measured a small cost, the
coordinator extended the same authorization to the remaining five units (Amazon, then Celestica,
Edge-Core, Accton, Oracle) with the same per-phase caps — a coordinator judgment, recorded here; each phase on its own immutable account
(request cap = the unit's exact step count, byte cap = cap × (1 MB + 1), 30 min elapsed window,
same host limit for `www.rra.go.kr`), activated with a recorded decision. No retries; pacing 0.15 s.
Driver: [live_rra.py](live_rra.py). Reports: one JSON per phase in this directory.

## what happened

| phase | account | result |
| --- | --- | --- |
| smoke a | `c4-smoke-2026-10-10a` | allocated with `deadline=1800` (absolute wall time, i.e. 1970): refused before any request; immutable account left as a record |
| smoke b | `c4-smoke-2026-10-10b` | byte cap one byte short per request (`reserve()` charges `max_bytes + 1`): refused before dispatch |
| smoke c | `c4-smoke-2026-10-10c` | **form GET 200, 14.9 KB, no redirect, 19.7 s** — production transport works |
| zero unit, cycle c | `c4-rra-official-query-78fc6b2e95f6-2026-10-10c` | form 200; `csv-before` POST **timed out at the 30 s client bound before headers** → reservation `uncertain`, account stopped (`response/transport failure`), charged |
| settlement | (no HTTP) | new `CampaignState.settle_unobserved` (reviewed: [settle-review](../review/settle-review.md), ACCEPT after two fix rounds) recorded the explicit decision; row stays `uncertain` and charged, account stays stopped, a distinct account may follow |
| zero unit, cycle d | `c4-rra-official-query-78fc6b2e95f6-2026-10-10d` | **captured 4/4 (form, csv-before, empty-result, csv-after), explicit zero, `matches_pinned`**; 120 s, 15.6 KB archive growth; bundle `data/raw/rra-edge-core-space-official-live-2026-10-10d/` |
| 7-row unit, cycle d | `c4-rra-official-query-5827f2fe7092-2026-10-10d` | **captured 10/10 in 81.6 s (~8 s/request), 63.8 KB growth**; bundle `data/raw/rra-edge-core-hyphen-official-live-2026-10-10d/`. CSV and all 7 detail pages byte-identical to the 2026-09-26 capture; records hash equal. The adapter reports `differs_from_pinned` only because pinned occurrences embed each detail page's *path* and a live bundle lives in a new directory ([location-insensitive check](compare-7row-location-insensitive.txt)) — a documented no-change result |
| Amazon unit, cycle d | `c4-rra-official-query-33f7d154085d-2026-10-10d` | **captured 60/60 in 133 s (~2.2 s/request), 452.6 KB growth**, detail cap 80 (pinned 57); 57 rows, CSV byte-identical, records hash equal ([check](compare-amazon-location-insensitive.txt)): no new Amazon Data Services certificates since 2026-09-26 |
| Celestica unit, cycle d | `c4-rra-official-query-41e4ca912b33-2026-10-10d` | **captured 154/154 in 375 s** (1 s pacing), 151 rows, CSV byte-identical, records hash equal. Pinned comparison also differs on eligible rows (48 vs 105) and model assertions (72 vs 149) because the live bundle lacks the saved bundle's reviewed `company_evidence` (`celestica-historical-companies.json`) ([check](compare-celestica.txt)): upstream unchanged |
| Edge-Core unit, cycle d | `c4-rra-official-query-63cb0b9098c7-2026-10-10d` | **captured 113/113 in 298 s**, 110 rows, CSV byte-identical, records hash equal, eligible rows 101 = 101 ([check](compare-edgecore-accton.txt)) |
| Accton unit, cycle d | `c4-rra-official-query-e5984834207f-2026-10-10d` | **captured 203/203 in 505 s**, 200 rows, CSV byte-identical, records hash equal; eligible rows 150 vs 164 only because of the missing `company_evidence` ([check](compare-edgecore-accton.txt)) |
| Oracle unit, cycle d | `c4-rra-official-query-73009f906e4e-2026-10-10d` | **captured 519/519 in 1,097 s**, 516 rows, CSV byte-identical, records hash equal, eligible rows 509 = 509 ([check](compare-oracle.txt)) |
| rebundle (no HTTP) | — | after the reviewed carry fix ([live-bundle review](../review/live-bundle-review.md), ACCEPT), the Celestica and Accton steps were rebuilt from their archived capture receipts (each byte-checked against the live bundle) and bundled again with the saved unit's reviewed `company_evidence`: `data/raw/rra-{celestica,accton}-official-live-2026-10-10d-carried/` |

**Result: all seven RRA units `matches_pinned` under the location-insensitive `compare_live`**
([table](compare-live-all-units.txt)) — a documented no-change refresh of these seven declared
manufacturer queries (not a regulator-wide claim). Nothing was queued or imported:
an unchanged real source yields no new accepted claims. The pre-fix Celestica/Accton live bundles stay on
disk as the original captures (their `-carried` siblings are the importable form).

## measured totals (cycle d, all 7 units)

| measure | value |
| --- | --- |
| HTTP requests | **1,063** (+ 1 smoke, + 2 in the timed-out cycle c), all 200, no throttle/denial |
| wall time | **43.5 min** of request time (pacing 0.15 s for the small units, 1 s for the four large ones) |
| archive growth | **~18 MB** (sum of per-phase growth: receipt files + objects + index file) for 1,063 requests; the saved 2026-09 bundles' files total ~5.6 MB on disk, a rough proxy for payload |
| operational state | 11 accounts (2 refused before any request — smoke a before activation, smoke b at reservation — and 1 stopped by the settled timeout), 1,066 reservations: 1,065 complete, 1 settled-uncertain |
| tokens | negligible for the requests themselves (the driver runs unattended); the review/engineering around the first run was the real cost |

latency fell from ~20–30 s/request in the first phases to ~2–3 s/request later the same morning;
the early 2.5–9 hour estimate was superseded by the measurement above.

## follow-ups

- done: `RraRefresh.bundle(..., carry_from=)` carries reviewed `company_evidence` (hash-checked, nested
  exhibits included, query must match; `first_result_page` deliberately not carried) and `compare_live`
  compares `detail_path` by file name ([engineering-live handoff](../engineering-live/handoff.md));
- c4 plan's "second eligible cycle" is not run yet (expected ~45 min, same caps; needs the user's
  authorization for that new scope); periodic invocation stays off until it is;
- a live candidate at a NEW bundle path needs the c3b queue to carry the baseline job/inputs
  separately from the candidate job (today both equal the pinned path); only needed when a live
  refresh actually changes a unit.

## cycle 2 (2026-10-11, cycle id `2026-10-11a`)

Authorization: the user answered "Authorize cycle 2" to "the c4 plan calls for a second live RRA refresh … ~45 min,
~1,060 requests, same caps as the first one". Scope and caps were identical to cycle d: one form-GET smoke, then the seven
units in the same order, each on its own immutable account (request cap = 4 + detail cap: 4, 11, 84, 194, 144, 254, 654;
byte cap = cap × (1 MB + 1); 30 min window; 120 s timeout), 1 s pacing, no retries. The recorded account decision now
carries the cycle-2 authorization (`live_rra.py --authorization`, a driver-only change).

| phase | result |
| --- | --- |
| smoke | form GET 200, 16.9 s |
| zero unit (78fc6b2e95f6) | 4/4 captured, 118 s, explicit zero, `matches_pinned` |
| 7-row unit (5827f2fe7092) | 10/10, 90 s, 7 rows |
| Amazon (33f7d154085d) | 60/60, 180 s, 57 rows |
| Celestica (41e4ca912b33) | 154/154, 366 s, 151 rows; rebundled offline with carried `company_evidence` |
| Edge-Core (63cb0b9098c7) | 113/113, 297 s, 110 rows |
| Accton (e5984834207f) | 203/203, 465 s, 200 rows; rebundled offline with carried `company_evidence` |
| Oracle (73009f906e4e) | 519/519, 1,079 s, 516 rows |

Totals: 1,063 unit requests + 1 smoke, all HTTP 200, no throttle/denial/timeout, no settlement or reconcile needed;
~12.9 MB archive growth; ~43 min of request time. **Result: all seven units `matches_pinned` under the
location-insensitive `compare_live`** ([compare](compare-live-all-units-2026-10-11a.txt)): no change in these seven
declared manufacturer queries since 2026-09-26/27 (not a regulator-wide claim). Nothing to import; bundles at
`data/raw/rra-*-official-live-2026-10-11a[-carried]/`. With two consecutive no-change cycles the plan's precondition for a
periodic schedule is met, but any periodic invocation is still a new scope needing the user's authorization.
