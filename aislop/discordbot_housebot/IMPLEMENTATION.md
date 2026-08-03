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

**`/house info` is public.** Everything else stays ephemeral, but the house
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
because it is pure local computation. `/house update reenrich:true` forces a full recompute — a flag on the refresh
command rather than a separate verb, since "look at this house again" is one
user intention, not two.

**Coordinates come from the listing, not from a geocoder.** Zillow publishes
`offers.itemOffered.geo`, so there is no Geocoding SKU in the loop and no
street-centerline snapping problem. `lat`/`lon` live on `properties` (they are
part of the observation) and are backfilled with `COALESCE` on later fetches, so
a page that starts publishing geo upgrades an existing row.

**Transit is one extra Routes call per destination, not a GTFS pipeline.** The
roadmap proposed preloading GTFS stops into D1 for proximity; for *itineraries*
that is the wrong tool — Routes already knows the schedule, the transfers and the
walk legs. Two caveats the API forced: top-level `routingPreference` is
DRIVE-only and errors on TRANSIT, and the transit field mask must name
`routes.legs.steps.transitDetails` explicitly or the steps come back empty.

Walking legs are collapsed into a single "N min walking" figure rather than
appearing in the chain, because what a rider needs to remember is which vehicles
in what order. `nameShort` is preferred over `name` (riders say "1 Line", not
"Link 1 Line") with a fallback — verified live, where South Lake Union Streetcar
ships only the long name.

**Both modes depart at 10:00 Pacific.** 05:00 makes traffic-aware driving come in
*below* free-flow; 08:00 scores transit against peak-only service. 10:00 asks
what the trip normally looks like, and keeping both modes on one clock makes
drive-vs-transit directly comparable. Each is overridable independently, and both
departures are printed in the provenance — an ETA without its assumption is a
number that lies.

**A missing photo suspends conditional GETs.** A house stored before `og:image`
parsing existed can never acquire one from a 304, because a 304 has no body. So
while `photoUrl` is absent the refresh omits its validators and takes one full
GET; once acquired, validators resume. The newly-found photo is worth an embed on
its own, but an embed containing *only* a picture is useless — so that path
backfills commute and heating from D1 rather than recomputing them, and posts a
complete card for zero API calls.

**Abbreviated prices are title-only.** `$725K` reads better in a thread list and
buys characters against the 100-char cap, but a change notice saying
"$725K → $700K" would hide a $4,900 cut. `formatPriceShort` is therefore used by
`buildThreadTitle` alone; `renderField` — which feeds both the diff and the
embeds — still returns the exact figure, and a test asserts the snapshot message
never contains the short form.

**One embed, and it is the starter message.** The thread used to open with a
plain-text snapshot and then get a second embed for enrichment — one house split
across two half-cards. Enrichment now computes *before* the thread is created
(`computeEnrichment` touches neither D1 nor Discord), so `add` can hand the
complete card to `createThread` as the forum starter — or post it once in a text
channel. Refreshes re-post the same merged view, freshly computed values layered
over what D1 already holds.

**ISP is Form 477, with its caveats attached to the data.** Keyless and verified
live. But it is Dec-2020 and block-level, so the provenance string says exactly
that: a strong negative signal, a weak positive one. Two calls — the 2010 census
block (Form 477 is keyed on 2010 blocks; a 2020 block silently returns nothing)
then the Socrata query.

**Embeds over markdown.** Enrichment and `/house status` post embeds so commute,
transit and heating are separately labeled fields rather than one wall of text,
colored by listing status. Field values are truncated at Discord's 1024-char cap.

**Stored enrichment values are versioned by shape, not by a version column.**
Adding transit changed the commute payload from `CommuteEstimate[]` to
`{drive, transit}`. Rows written by the older adapter still sit in D1, so two
things were needed: `normalizeCommuteValue` accepts both shapes when reading (an
old row renders its drive times instead of vanishing), and `isCurrentCommuteShape`
decides whether `missing` mode considers the row settled. A legacy row is
deliberately *not* settled, so the next update upgrades it. Treating "non-null
value" as "settled" would have frozen those rows forever.

**A 304 still runs enrichment.** The refresh path used to return early on
not-modified, which stranded any house whose page reliably answers 304 — the
listing never changes, so blank enrichment never gets filled. It now backfills
from the stored snapshot before returning.

**Discord wire types come from `discord-api-types`, not from hand-rolled
constants.** The hand-rolled version is precisely how the forum bug shipped:
`{name, type: PUBLIC_THREAD}` is a valid *text-channel* thread body, and nothing
could tell me a forum channel needs a starter `message` instead. Enums,
`RESTPostAPIGuildForumThreadsJSONBody`, `APIEmbed` and `Routes` now come from the
library, so the two thread bodies are distinct types and endpoint paths are not
string-built. It costs bundle size (84 KiB → 296 KiB raw, 23 → 57 KiB gzipped,
against a 1 MB compressed limit) and is worth it.

`Routes.webhookMessage(...,'@original')` percent-encodes to `%40original`, which
Discord decodes server-side — the follow-up path still works, but a test matching
the literal `@original` had to learn about it.

**Thread creation asks the channel what it is.** One `GET /channels/{id}` per
`/house add`, then a forum/media body (with `message`) or a text/announcement
body (with `type`). Guessing wrong is a hard 400, and the failure is invisible
until someone actually adds a house.

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
- **ISP data is four years stale by construction.** Form 477 stopped in Dec 2020,
  so fiber built since is invisible. The BDC upgrade needs FCC credentials plus an
  address→Fabric Location ID step.
- **Photos are the single `og:image` only.** Hotlinked from the provider, so a
  provider that blocks hotlinking or rotates the URL breaks the image silently.
- **HVAC classification is still listing prose.** The King County Assessor
  `Heat Source` verification path is designed but not built, so a listing that
  says "forced air" about a heat pump will still say that.
- **Commute (drive + transit) is live-verified from the Worker**, via a
  temporary token-gated route that was removed afterwards: Bellevue 34 min drive
  (20 free-flow) / 42 min transit via the 566; Seattle 46 min drive / 74 min
  transit via 101 + South Lake Union Streetcar. It has not yet run through a real
  `/house add`.
- **Which SKU transit bills to is unconfirmed.** Google's docs name
  `TRAFFIC_AWARE_OPTIMAL` as a Pro feature but say nothing about TRANSIT. At 4
  calls per house it does not matter for cost, but the free-tier headroom
  estimate assumes the worst case (Pro).

## Verification performed

```
npm run typecheck   # tsc src + tsc tests/scripts — clean
npm test            # vitest: 11 files, 190 tests, all passing
npm run build       # wrangler deploy --dry-run --outdir=dist — 82.54 KiB
```

Coverage by area: URL normalization/dedupe, title + message formatting (incl.
the 100-char thread cap and `❌ ` prefix), change computation, status
transitions, Ed25519 verification (valid/tampered/replayed/wrong-key/malformed),
provider parsers against fixtures (active/sold/unparseable), fetcher behavior
(conditional headers, 304, 403-as-blocked, 500, timeout, network error,
unsupported URL), Airtable field map + client, and end-to-end interaction flows
(`add`/`bind`/`update`/`close`/`open`/`info`, wrong channel, idempotent retry, deferral)
plus scheduled-refresh batching, force-close exclusion, per-property failure
isolation, and interaction-log pruning — all against a mock Discord REST and an
in-memory D1. No test touches the network.
