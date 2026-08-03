# Architecture

## Shape

```
Discord ──POST /interactions──> Worker.fetch
                                  │  verify Ed25519 (before any parsing)
                                  │  claim interaction id in D1  (idempotency)
                                  │  reply type 5 (deferred, ephemeral) in <3s
                                  └─ ctx.waitUntil ─> HouseService
                                                       ├─ fetch listing page (1 GET, 8s cap)
                                                       ├─ D1 read/write (indexed, 2-4 stmts)
                                                       ├─ Discord REST (thread / rename / message)
                                                       ├─ Airtable upsert (optional, best effort)
                                                       └─ PATCH @original follow-up

cron "17 * * * *" ──> Worker.scheduled ──> runScheduledRefresh
                                             └─ listDue(limit=REFRESH_BATCH_SIZE)
                                                └─ per property: same HouseService.refresh
```

`HouseService.refresh` is the *only* refresh path. `/house update` and the cron
call it identically; the only difference is the `source` tag recorded in
`snapshots`. That is why the scheduled handler is a thin, separately testable
module (`src/scheduled/refresh.ts`) with no Discord-specific logic.

## Why this is cheap

**No tokens are spent at all.** There is no model in the loop — titles, change
notices, and status transitions are deterministic string building from a
normalized snapshot (`src/listing/format.ts`). Two runs of the same snapshot
produce byte-identical output, which is also what makes the tests meaningful.

The cost model is then just *requests × D1 rows × outbound fetches*:

| Operation | D1 statements | Outbound fetches |
| --- | --- | --- |
| `/house status` | 2 (idempotency insert, thread lookup) | 0 |
| `/house close` | 3 | 0 (+2 Discord REST) |
| `/house add` | 4 (claim, dedupe, insert, snapshot) | 1 listing (+2 Discord REST, +≤1 Airtable) |
| `/house update` | 3–4 | 1 listing (+0–2 Discord REST) |
| cron tick | 1 + 2–3 per due property | ≤ `REFRESH_BATCH_SIZE` |

Every lookup is index-covered:

- `idx_properties_listing_key` (unique) — dedupe on `/house add`
- `idx_properties_thread_id` (unique) — "which house is this thread?"
- `idx_properties_due (force_closed, next_check_at)` — the cron's due query, so a
  tick reads only the rows it will actually refresh

**Bounded fan-out is the important part.** A naive design refetches every tracked
house on every tick; that is O(houses) outbound requests per hour forever, and it
is also the fastest way to get rate-limited by a provider. Instead each property
carries its own `next_check_at`, the cron takes at most `REFRESH_BATCH_SIZE` due
rows ordered by due time, and failures push `next_check_at` out exponentially
(`backoffSeconds`, capped at 24h). With the shipped defaults — hourly cron, batch
10, 12h interval — a portfolio of 50 houses costs ~100 listing fetches/day and
stays inside the Workers free tier's 100k requests/day and D1's free row budget
with orders of magnitude to spare.

Conditional GETs (`If-None-Match` / `If-Modified-Since`) cut this further: a
`304` costs one round trip, zero parsing, and a single `UPDATE`. The Worker also
passes `cf: { cacheTtl: 300 }` so bursts collapse at the edge.

## Correctness properties

- **Signature first.** `verifyDiscordRequest` runs before `JSON.parse` and before
  any D1 access. An unsigned or tampered request gets a bare 401.
- **Exactly-once handling.** Discord retries webhook deliveries. Every command
  claims its `interaction.id` via `INSERT OR IGNORE`; a duplicate short-circuits
  before any side effect. The log is pruned by the cron (7-day retention).
- **Defer correctly.** Anything with a network call answers type 5 (deferred,
  ephemeral) immediately and finishes in `ctx.waitUntil`, then replaces the
  placeholder via the follow-up webhook. `/house status` does no I/O so it
  answers inline with type 4.
- **Dedupe on identity, not URL text.** The canonical key is
  `provider:listingId` derived from the URL path (`..._zpid/`, `/home/<id>`), so
  tracking params, mobile hosts, and slug changes all collapse to one house.
  `/house add` re-checks for a race after the slow network work.
- **Never contradict the source.** `/house open` refuses when the live listing
  reports sold/closed *or* an unknown status. `/house close` is an operator
  override that survives listing flapping (`force_closed`).
- **Lost data is not news.** `computeChanges` ignores a field going
  known → unknown, so a provider dropping a value from the page never produces a
  false "price removed" notice.

## Failure behavior

| Failure | Result |
| --- | --- |
| Listing 403/429 | `blocked`, exponential backoff, snapshot untouched, user told plainly |
| Listing timeout (8s) | `timeout`, same |
| Markup changed | `unparseable` — no snapshot written, nothing invented |
| Discord REST 429 | one bounded retry honoring `retry_after` |
| Airtable down/misconfigured | `skipped`/`error` recorded in `airtable_sync`; command still succeeds |
| One property fails in a cron batch | counted in the report; the rest of the batch continues |

## Data model

`properties` holds one row per tracked house (identity, Discord ids, latest
normalized snapshot as JSON, denormalized status/title for cheap reads, fetch
validators, scheduling, failure state). `snapshots` is append-only change
history — one row per observed change set plus one for the initial add — so the
thread's story is reconstructable without re-reading Discord.
