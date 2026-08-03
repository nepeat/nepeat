# Implementation journal

Decisions and known limitations. Not a transcript.

## Relationship to the prior art

The source concept lives at `~/Documents/agents/houses/house-lookup` — a Python
CLI/MCP "dossier" tool for King County properties. **No code was copied**; it is
a different language, runtime, and product (one-shot research report vs. a
long-lived tracker). What was actually reused is knowledge, all of it from
`references/data_sources.md`:

- the FCC Form 477 (Dec 2020, 2010-census-block) vs. BDC (credentialed,
  fabric-keyed) distinction and the "symmetrical gigabit ≈ both directions ≥ 900
  Mbps" heuristic → `docs/ENRICHMENT.md`
- the Census geocoder as the keyless address→lat/lon hub, including its
  street-centerline snapping gotcha → `docs/ENRICHMENT.md`
- King County Assessor `Heat Source` as the *verification* path for HVAC →
  `docs/ENRICHMENT.md`
- GTFS / OneBusAway as the transit-proximity source (listed there as a candidate)

Nothing from the source's `.env`, virtualenv, or parent directories was read or
copied. (Several top-level files in that directory were unreadable from this
environment — `EDEADLK` on `SKILL.md`, `README.md`, `pyproject.toml` — so the
port drew on the module tree and the reference doc, which read fine.)

## Decisions

**HTTP interactions, not a gateway bot.** A gateway bot needs an always-on
process; Workers cannot provide one cheaply. Everything the product needs
(slash commands, thread management, scheduled refresh) is reachable through
signed webhooks + REST + cron.

**No model in the loop.** Every user-visible string is deterministic formatting
over a normalized snapshot. This is the actual cost story (see
`docs/ARCHITECTURE.md`) and it makes the output testable by equality.

**One refresh path.** `/house update` and the cron both call
`HouseService.refresh`. Divergence between "manual" and "scheduled" behavior is
the classic source of "it works when I run it by hand" bugs, so the only
difference is a `source` tag written to history.

**Identity from the URL, never from the page.** `provider:listingId` comes from
the path (`<zpid>_zpid`, `/home/<id>`). A page that parses weirdly can never
change which house a row refers to, and mobile/tracking/slug URL variants
collapse to one record. Canonicalization strips the query entirely — on both
providers it is UI state, never identity.

**Snapshot stored as JSON, hot fields denormalized.** `properties.snapshot_json`
carries the full observation; `status`, `title`, `next_check_at`, `force_closed`
are columns because they are what the indexes and the cron query need. This
avoids a wide table that must migrate every time a parser learns a new field.

**`force_closed` is separate from `status`.** `/house close` is an operator
override that must survive the listing flapping back to active; conflating it
with the observed status would let a refresh silently undo a human decision.
`/house open` clears it only when the live listing agrees (and refuses on
`unknown` rather than guessing).

**Deferred + follow-up for anything with I/O.** Discord's 3-second budget cannot
absorb an 8-second listing fetch. `/house status` is the exception — pure D1, so
it answers inline.

**Idempotency in D1, not in memory.** `INSERT OR IGNORE` on `interaction.id`
gives exactly-once handling across isolates and retries. Pruned at 7 days by the
cron so the table stays small.

**Lost data is not a change.** `computeChanges` skips any field that went
known → unknown. Providers drop fields from public markup routinely; announcing
that as "price removed" would be noise, and worse, would flap.

**Failures are values.** `fetchListing` returns a tagged `FetchResult` instead of
throwing, with a fixed reason vocabulary (`network`, `timeout`, `http-status`,
`blocked`, `unparseable`, `unsupported-url`) that maps to user-facing text in one
place. `blocked` (403/429) is explicitly *not* retried harder.

**Plain Vitest + a `node:sqlite`-backed D1 adapter for tests.**
`@cloudflare/vitest-pool-workers` would run tests in workerd, which is more
faithful, but it needs the workerd binary and a live D1 session and it makes the
suite slower and flakier. Instead `test/helpers/d1.ts` implements the slice of
the D1 API the repo uses on top of `node:sqlite`, applying the *real*
`migrations/0001_init.sql`. The SQL, the indexes, the unique constraints, and the
`INSERT OR IGNORE` idempotency path are therefore genuinely exercised. Node 22's
webcrypto supports Ed25519, so signature verification is tested against real
generated keypairs, valid and invalid.

**Never store a detached `fetch`.** `this.fetchImpl = opts.fetchImpl ?? fetch`
looks harmless and passes every local test, because Node's fetch does not care
about its `this`. workerd does: calling it as a method throws *"Illegal
invocation: function called with incorrect `this` reference"*, which took down
every outbound call in production while the whole suite stayed green. All three
call sites now route through `boundFetch()` in `src/http.ts`, and
`test/bound-fetch.test.ts` installs a strict `fetch` stub that reproduces
workerd's behavior so the regression cannot come back.

**Tracebacks go to Discord.** Single-operator bot, ephemeral responses — an
error reply carries the full stack (truncated to 1500 chars) instead of a vague
apology, so a failure is debuggable without catching it in a live
`wrangler tail`. Airtable details are token-redacted before they reach this
path. Revisit if the bot ever gains other users.

**The first live fetch broke two parser assumptions.** A real `/house add`
returned price/address/status but no beds/baths/sqft. The captured page showed
why: Zillow nests the residence under `offers.itemOffered` (the outer node is
`RealEstateListing`/`Product` and carries no facts), and the `__NEXT_DATA__`
hydration keys the scanner looked for (`bedrooms`, `livingArea`, `homeStatus`)
no longer exist anywhere in the document. The fix reads through `itemOffered`
and adds `<meta name="description">` — "…$725,000 4 beds, 2 baths, 4,670 sqft …
built in 1908" — as a second source, which also supplies `yearBuilt`. HVAC now
comes from tag-stripping a bounded window after the rendered `Heating` heading,
since it is markup rather than JSON. `test/fixtures/zillow-live-2026.html` is
trimmed verbatim from that live page so these stay honest.

**Scheduled refresh is off.** `triggers.crons` is `[]`: automated repeat traffic
is what earns an IP block, and the value of hourly polling did not justify the
risk to a bot whose only source is public pages. Manual `/house update` only.
The handler and tests stay so re-enabling is a one-line config change.

**`/house status` is public.** Everything else stays ephemeral, but the house
summary is for the room — it is the thing people actually want to point at in a
thread.

**Enrichment lives in its own table, never in `snapshot_json`.** The snapshot is
an *observation of the listing*; enrichment is *derived facts about the
location*. Merging them would corrupt `computeChanges`, which diffs observations
— a commute recomputation would surface as a listing change. `enrichment` is
keyed `(property_id, kind)` with a mandatory `provenance` string.

**Enrichment on refresh fills blanks only.** Recomputing everything on each
refresh would burn Google quota to produce noise; never recomputing means a house
enriched while routing was down stays permanently empty. So `/house update` runs
enrichment in `missing` mode: a kind holding a value is skipped, a kind recorded
`unavailable` is retried. That makes update self-healing — a row added before the
parser learned to read coordinates picks up its commute on the next update with
no manual step — while a steady-state update still costs zero API calls. HVAC is
the exception: it reclassifies whenever the listing's heating text changed,
because it is pure local computation. `/house enrich` forces a full recompute.

**Coordinates come from the listing, not from a geocoder.** Zillow publishes
`offers.itemOffered.geo`, so there is no Geocoding SKU in the loop and no
street-centerline snapping problem. `lat`/`lon` live on `properties` (they are
part of the observation) and are backfilled with `COALESCE` on later fetches, so
a page that starts publishing geo upgrades an existing row.

**Split tsconfigs.** `@types/node` and `@cloudflare/workers-types` disagree about
`fetch`/`CryptoKey`. `tsconfig.json` typechecks `src/` against workers-types only
(what actually runs); `tsconfig.test.json` adds node types for tests and scripts.
`npm run typecheck` runs both.

## Known limitations

- **Redfin has never seen a live page.** Its fixture is hand-authored to match
  the *shape* of the markup, not captured. The Zillow adapter is now verified
  against real markup (see below), but Redfin's remains a guess.
- **Zillow served Cloudflare egress fine** on 2026-08-03 — no bot protection on
  a single manual fetch. That is one data point, not a guarantee; the `blocked`
  path still exists and the cron is off precisely to avoid earning one.
- **Airtable is untested against a live base.** No token was available. The
  request shape, upsert merge field, skip reasons, and token redaction are unit
  tested against a mock. See `docs/AIRTABLE.md` — verify with one real
  `/house add` before trusting it.
- **Deployed, but the listing path is unproven in production.** The Worker is
  live at `housebot.butt.workers.dev` with D1 `7d1aa078-…` migrated and the cron
  armed; signature rejection and the interactions endpoint are verified against
  the real deployment. No successful live listing fetch has been observed yet.
- **`/house update` with an explicit `link` only works for an already-tracked
  house.** It is a refresh escape hatch, not a second `add`.
- **`/house bind` renames the thread it adopts.** Storing a canonical title while
  leaving the Discord name untouched would make D1 and Discord disagree, and the
  next refresh would rename it anyway — so bind does it up front. Both unique
  constraints still apply: one listing per thread, one thread per listing.
- **Threads are never auto-archived.** `DiscordRest.setThreadArchived` exists but
  is unused: archiving a closed house would hide the history that makes the
  thread useful. Wire it up if the channel gets noisy.
- **No per-user permissions.** Anyone who can see the channel can run `/house`.
  Add Discord command permissions in the Developer Portal if that matters.
- **Baths parsing is approximate.** Providers disagree on whether "baths" means
  full, total, or a `2.5` decimal; the adapters prefer
  `numberOfBathroomsTotal`/`bathrooms` and fall back through the LD+JSON
  vocabulary. Titles render `4b2.5b` for fractional values.
- **Transit, ISP and photos are still interfaces only.** See `docs/ROADMAP.md`
  for the costed plan; the FCC Form 477 path is verified working and unbuilt.
- **HVAC classification is still listing prose.** The King County Assessor
  `Heat Source` verification path is designed but not built, so a listing that
  says "forced air" about a heat pump will still say that.
- **The commute number is one live-verified call, not a proven feature.** The
  Google Routes key was validated end to end from the Worker (18,083 m / 1,115 s
  to Bellevue), but the adapter itself has only run against mocks. The first real
  `/house add` is the actual test.

## Verification performed

```
npm run typecheck   # tsc src + tsc tests/scripts — clean
npm test            # vitest: 10 files, 140 tests, all passing
npm run build       # wrangler deploy --dry-run --outdir=dist — 72.96 KiB
```

Coverage by area: URL normalization/dedupe, title + message formatting (incl.
the 100-char thread cap and `❌ ` prefix), change computation, status
transitions, Ed25519 verification (valid/tampered/replayed/wrong-key/malformed),
provider parsers against fixtures (active/sold/unparseable), fetcher behavior
(conditional headers, 304, 403-as-blocked, 500, timeout, network error,
unsupported URL), Airtable field map + client, and end-to-end interaction flows
(`add`/`bind`/`update`/`close`/`open`/`status`, wrong channel, idempotent retry, deferral)
plus scheduled-refresh batching, force-close exclusion, per-property failure
isolation, and interaction-log pruning — all against a mock Discord REST and an
in-memory D1. No test touches the network.
