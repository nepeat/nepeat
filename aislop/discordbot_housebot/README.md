# housebot

A Discord bot that tracks Zillow / Redfin property listings as threads under one
channel. It runs entirely on **Cloudflare Workers + D1** using **HTTP
interactions** (no gateway connection, no always-on process, no AI calls).

Each tracked house gets a public thread whose name is the listing at a glance:

```
$725,000 - 4,670ft - 4b2b - 400 Cedar Avenue S, Renton, WA 98057
```

Sold/closed houses get an `❌ ` prefix on the thread name and on the change
notice.

## Commands

| Command | Where | What it does |
| --- | --- | --- |
| `/house add <link>` | house channel or any of its threads | Normalizes the URL, fetches one snapshot, dedupes by canonical listing key, creates the thread, posts the first snapshot. |
| `/house bind <link>` | inside an existing thread | Binds **that** thread to a listing instead of creating a new one, renames it to the canonical title, and posts the first snapshot. |
| `/house update [link] [reenrich]` | inside a house thread (or the channel with an explicit `link` to an already-tracked house) | Refetches, updates the snapshot/title/status, posts a change notice **only** when a tracked material field moved, and fills in any enrichment that is still blank. `reenrich:true` forces a full recompute. |
| `/house close` | inside a house thread | Force-closes the house (no fetch). Prefixes the title with `❌ ` and removes it from the cron. |
| `/house open` | inside a house thread | Re-opens **only** if the live listing is not sold/closed. Explains cleanly otherwise. |
| `/house status` | inside a house thread | Rich embed built from D1 — status, price, size, driving + transit commutes, heating, history. Posted **publicly**; makes no network calls. |

Material fields: `status`, `price`, `beds`, `baths`, `sqft`, `address`,
`yearBuilt`, `hvac`.

## Enrichment

Location-derived facts, stored in the `enrichment` table with a provenance
string. Computed in full at add/bind time; after that **`/house update` fills in
only what is still blank**, so a routine refresh costs zero API calls when
everything already succeeded — and self-heals anything that previously failed
(routing down, coordinates missing) with no manual step. There is no separate
enrich command: `/house update reenrich:true` forces a full recompute.

- **Commute** — Google Routes API, driving *and* transit, departing at a pinned
  time (`COMMUTE_DEPARTURE_ISO`, default next Tue 08:00 Pacific — rush hour, so
  the number means something). Driving uses `TRAFFIC_AWARE_OPTIMAL` and reports a
  free-flow comparison; transit renders the itinerary as a vehicle chain:

  ```
  House → 101 🚌 → South Lake Union Streetcar 🚊 → Seattle office (partner)
  ```

  Light rail (🚈), heavy rail (🚆), tram (🚊), bus (🚌) and ferry (⛴️) get distinct
  glyphs. **4 calls per house** (2 destinations × 2 modes) against the Compute
  Routes free tier, so ~1,250 house-adds/month before it costs anything. Origin
  coordinates come from the listing page's own `geo` markup — housebot never
  geocodes.
- **Heating** — classified from listing text into heat pump / forced air (gas or
  electric) / baseboard / radiant floor / radiators / oil / none. **Oil and steam
  radiators are flagged with ⚠️.** Always labeled unverified; `radiant floor` and
  `radiator` are deliberately distinct. Reclassifies whenever the listing's
  heating text changes, since it costs nothing.

Results are posted as a Discord embed, colored by listing status (green active,
yellow pending, red closed). Both adapters degrade to a recorded `unavailable`
with a reason rather than failing the command, and `/house status` says *why* a
section is missing instead of silently omitting it. Transit, ISP and photos are still scaffolded interfaces — see
[docs/ROADMAP.md](docs/ROADMAP.md).

## Bootstrap

```bash
git clone <this repo> && cd discordbot_housebot
npm install

# 1. Create the D1 database, then paste the printed database_id into wrangler.jsonc
npx wrangler d1 create housebot

# 2. Apply migrations
npm run db:migrate:local     # local dev
npm run db:migrate:remote    # production

# 3. Local secrets
cp .dev.vars.example .dev.vars   # fill in the Discord values

# 4. Verify
npm run typecheck
npm test
npm run build                # wrangler deploy --dry-run --outdir=dist
```

`wrangler.jsonc` already points at the live D1 database
(`7d1aa078-b83c-448e-8aed-f62f175de3c8`, region WNAM). A fresh environment
should run `wrangler d1 create` and swap that id.

## Discord Developer Portal setup

1. **Create the application** at <https://discord.com/developers/applications>.
2. **General Information** → copy the **Application ID** and **Public Key**.
3. **Bot** → *Reset Token* → copy it. The bot needs no privileged intents (HTTP
   interactions only).
4. **OAuth2 → URL Generator** → scopes `bot` + `applications.commands`, bot
   permissions: **View Channel**, **Send Messages**, **Create Public Threads**,
   **Send Messages in Threads**, **Manage Threads** (needed to rename/archive).
   Invite the bot to the guild.
5. **General Information → Interactions Endpoint URL** →
   `https://<your-worker>.workers.dev/interactions`. Discord immediately sends a
   signed PING; the Worker must already be deployed with `DISCORD_PUBLIC_KEY` set
   or the save will fail.
   ⚠️ This is **not** the *Webhooks* page. That page's "Endpoint URL" is for
   Event Webhooks (`APPLICATION_AUTHORIZED` etc.) — housebot consumes none of
   them, so leave it blank with the **Events** toggle off. Putting the
   interactions URL there produces "The application did not respond" with no
   traffic ever reaching the Worker.

### Register the command

```bash
npm run commands:print      # dry run: prints the exact PUT + JSON body, no network
npm run commands:register   # actually registers (guild-scoped, instant)
```

Reads `DISCORD_APPLICATION_ID`, `DISCORD_GUILD_ID`, `DISCORD_BOT_TOKEN` from the
environment or from `.dev.vars`.

## Secrets and vars

Vars live in `wrangler.jsonc` (non-secret). Secrets go in `.dev.vars` locally and
`wrangler secret put <NAME>` in production.

| Name | Kind | Required | Purpose |
| --- | --- | --- | --- |
| `HOUSE_CHANNEL_ID` | var | yes | Parent channel for all property threads (`1533771541106655423`). |
| `REFRESH_INTERVAL_MINUTES` | var | no (720) | Base staleness window before a property is due. |
| `REFRESH_BATCH_SIZE` | var | no (10) | Max properties refreshed per cron tick. |
| `USER_AGENT` | var | no | Identifying UA sent to listing pages. |
| `AIRTABLE_BASE_ID` | var | no | Airtable base (`appc5LQi7Uo9Y75yN`). |
| `DISCORD_PUBLIC_KEY` | secret | yes | Ed25519 signature verification. |
| `DISCORD_BOT_TOKEN` | secret | yes | REST calls (threads, messages). |
| `DISCORD_APPLICATION_ID` | secret | yes | Follow-up webhook routing. |
| `DISCORD_GUILD_ID` | secret | registration only | Guild-scoped command registration. |
| `AIRTABLE_TOKEN` | secret | no | Enables the optional Airtable sync. |
| `AIRTABLE_TABLE` | secret | no | Table id or name. **No default is assumed.** |
| `AIRTABLE_FIELD_MAP_JSON` | secret | no | Field mapping — see [docs/AIRTABLE.md](docs/AIRTABLE.md). |
| `GOOGLE_MAPS_API_KEY` | secret | no | Routes API key; enables commute ETAs. Restrict by API, **not** by IP. |
| `COMMUTE_DEPARTURE_ISO` | secret | no | Pinned departure time for ETAs. Default: next Tue 08:00 Pacific. |

```bash
for s in DISCORD_PUBLIC_KEY DISCORD_BOT_TOKEN DISCORD_APPLICATION_ID; do
  npx wrangler secret put "$s"
done
```

## Scheduled refresh

**Currently disabled.** `wrangler.jsonc` sets `triggers.crons = []` — repeated
automated hits are exactly what gets an IP blocked by Zillow/Redfin, so refreshes
happen only when you run `/house update`. The handler, its batching/backoff
logic, and its tests all remain in place.

To re-enable, restore `"crons": ["17 * * * *"]`. A tick then selects at most
`REFRESH_BATCH_SIZE` properties whose `next_check_at` is due, refreshes each
**independently**, and backs off exponentially (capped at 24h) on consecutive
failures. Force-closed houses are excluded in SQL, so they cost nothing.

Test a tick locally:

```bash
npx wrangler dev --test-scheduled
curl "http://localhost:8787/__scheduled?cron=17+*+*+*+*"
```

## Deploy

```bash
npm run build      # dry run, no account needed
npm run deploy     # requires CLOUDFLARE_API_TOKEN / wrangler login
```

Deployed at **<https://housebot.butt.workers.dev>** — interactions endpoint
`https://housebot.butt.workers.dev/interactions`, cron `17 * * * *`, D1 migration
`0001_init.sql` applied remotely.

## Operational caveats

- **Public pages are best effort.** Zillow and Redfin ship bot protection. A
  `403`/`429` is reported as `blocked` and backed off; housebot never rotates
  user agents, solves challenges, logs in, or touches private APIs. Expect
  refreshes to fail sometimes — the stored snapshot simply stays stale, and
  `/house status` shows the failure.
- **Parsers read four sources, in confidence order:** schema.org LD+JSON
  (including the residence nested under `offers.itemOffered`, which is how Zillow
  ships it), the `<meta name="description">` facts line, hydration-blob JSON
  keys, and `og:title`. The Zillow path is verified against markup captured live
  on 2026-08-03 (`test/fixtures/zillow-live-2026.html`). **Redfin is still
  fixture-only** — its adapter has never seen a live page. When markup changes,
  the result is `unparseable` — never invented data.
- **HVAC is unverified.** It is parsed from listing text when present and always
  labeled as such. Everything else in
  [docs/ENRICHMENT.md](docs/ENRICHMENT.md) (photos, commute, transit, ISP) is a
  scaffolded interface only.
- **Airtable is advisory.** Absent or broken Airtable never blocks a command; the
  result is recorded in the `airtable_sync` table.
- **Thread names cap at 100 chars.** Titles are truncated with `…` after the
  price/size/beds segments, so the numbers always survive.
- `/house` is refused outside the house channel and its threads.
- **Errors reply with the full traceback**, ephemerally. This is deliberate for a
  single-operator bot; if you add other users, trim `formatError` usage in
  `src/discord/interactions.ts` and `src/index.ts` back to a generic message.

## Docs

- [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) — design, economics, and limits
- [docs/AIRTABLE.md](docs/AIRTABLE.md) — field-map discovery and configuration
- [docs/ENRICHMENT.md](docs/ENRICHMENT.md) — commute / transit / ISP / photos options
- [docs/ROADMAP.md](docs/ROADMAP.md) — costed plan for the enrichment features, with live API probe results
- [IMPLEMENTATION.md](IMPLEMENTATION.md) — decisions and known limitations
