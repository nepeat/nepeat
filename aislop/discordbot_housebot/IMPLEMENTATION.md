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

**Split tsconfigs.** `@types/node` and `@cloudflare/workers-types` disagree about
`fetch`/`CryptoKey`. `tsconfig.json` typechecks `src/` against workers-types only
(what actually runs); `tsconfig.test.json` adds node types for tests and scripts.
`npm run typecheck` runs both.

## Known limitations

- **Parsers are fixture-tested, not provider-guaranteed.** The fixtures in
  `test/fixtures/` are hand-authored to match the *shape* of Zillow/Redfin public
  markup (schema.org LD+JSON + a hydration blob), not captured from live pages.
  If a provider changes markup, the adapter returns `unparseable` and nothing is
  written — but no test here will have warned you first.
- **No live fetch has been performed** against Zillow or Redfin from this repo.
  Bot protection is likely on real traffic from Cloudflare egress; the `blocked`
  path exists because that is the expected steady state, not a rare case.
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
- **Enrichment is interfaces only** (photos, commute, transit, ISP). See
  `docs/ENRICHMENT.md` for the concrete integration options and their costs.

## Verification performed

```
npm run typecheck   # tsc src + tsc tests/scripts — clean
npm test            # vitest: 8 files, 96 tests, all passing
npm run build       # wrangler deploy --dry-run --outdir=dist — 56.97 KiB
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
