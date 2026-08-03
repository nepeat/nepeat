# Roadmap: enrichment features

Written 2026-08-03 after probing every API involved. Findings marked ✅ were
verified with a live request; ⚠️ means "documented but not yet proven here".

## The unlock: we already have coordinates

Zillow's LD+JSON ships `offers.itemOffered.geo` ✅:

```json
{ "@type": "GeoCoordinates", "latitude": 47.4779, "longitude": -122.20163 }
```

That removes the geocoding step from every feature below — no Census geocoder
round trip, no street-centerline snapping problem, no API key. Redfin's geo is
unverified (⚠️ its adapter has never seen a live page); fall back to the Census
geocoder there.

**Phase 0 (prerequisite, ~30 min):** parse `geo` into the snapshot, add
`lat`/`lon` columns, backfill on next `/house update`. Everything else depends
on this and nothing else does. Do it first regardless of what we build.

---

## 1. Transit proximity 🚆 — *easiest, do this one*

**Effort: ~half a day. No API key, no runtime dependency, no recurring cost.**

Sound Transit and King County Metro publish GTFS static feeds. We do not need a
live API — station locations change roughly never. Preprocess `stops.txt` once
into a D1 table and the runtime query is a bounding-box scan over a few hundred
indexed rows.

```sql
CREATE TABLE transit_stops (
  id TEXT PRIMARY KEY, name TEXT NOT NULL,
  kind TEXT NOT NULL,          -- link | sounder | streetcar | rapidride | transit_center
  routes TEXT, lat REAL NOT NULL, lon REAL NOT NULL
);
CREATE INDEX idx_stops_bbox ON transit_stops (lat, lon);
```

Scope it to what actually matters: **Link light rail (~45 stations), Sounder
(12), streetcar, RapidRide stops, transit centers.** That is hundreds of rows,
not the ~9,000 bus stops in the region — and it matches "we like trains" rather
than "there is a bus pole somewhere".

Runtime: bbox filter on ±0.05° then haversine in JS, return the nearest rail
station and the nearest frequent-bus stop separately. Straight-line distance
ships first (clearly labeled); drive/walk time needs §2.

**Alternative considered:** OneBusAway PugetSound
(`api.pugetsound.onebusaway.org`). The endpoint shape is right — a probe with
`key=TEST` returned `429 rate limit exceeded` ✅, so it exists and the demo key
is exhausted. A free registered key would give live arrivals and headways. Worth
adding later for "trains every N minutes", but it is a runtime dependency that
GTFS-in-D1 does not need. **Recommendation: GTFS first, OBA later if we want
frequency data.**

---

## 2. Drive time to work 🚗

**Effort: ~1 day. Needs a routing provider — this is the one that costs money or
a server.**

Destinations (from `COMMUTE_DESTINATIONS`, labels corrected):

| Label | Address |
| --- | --- |
| Bellevue office (nep) | 601 108th Ave NE, Bellevue, WA 98004 |
| Seattle office (partner) | 551 Boren Ave N, Seattle, WA 98109 |

Options, honestly compared:

| Option | Cost | Traffic model | Verdict |
| --- | --- | --- | --- |
| **Mapbox Directions** `driving-traffic` | free ≤100k req/mo, then ~$2/1k | live traffic | **Recommended MVP.** Two calls per house, one-time. Well within free tier forever at our volume. |
| **Self-hosted OSRM/Valhalla** on `10g.warc.zip`, exposed via Cloudflare Tunnel | server time only | none (free-flow) | Great fit for the existing lab host + Cloudflare account, and zero per-request cost. But free-flow ETAs understate a real commute badly on I-90/520. Best as a *comparison* number, not the headline. |
| **Google Routes API** with `departureTime` | paid, priciest | historical + live, departure-time aware | The only one that answers "how bad is this at 8am Tuesday". Worth it if the commute number is actually load-bearing for a purchase decision. |
| OpenRouteService free tier | free, rate-limited | free-flow | ToS is restrictive; no advantage over self-hosting. |

**Key design point: compute once at `/house add`, not on refresh.** Addresses do
not move. Recomputing hourly would burn quota to produce noise. Store
`commute_seconds_bellevue` / `commute_seconds_seattle` + the provider and the
assumed departure time, and show provenance in the thread.

I would ship Mapbox with a fixed 8:00am-Tuesday framing, and note in the message
that it is live-traffic-at-fetch-time unless we pay for Google.

---

## 3. FCC ISP speeds 🌐 — *surprisingly good news, with a catch*

**Effort: ~half a day for the free path.**

Two datasets, and the difference matters:

**Form 477 (Socrata, keyless) ✅ — works right now.** Verified end to end against
400 Cedar Ave S:

```
geo.fcc.gov/api/census/block/find?...&censusYear=2010  ->  block 530330257012004
opendata.fcc.gov/resource/hicn-aujz.json?$where=blockcode='...' and consumer='1'
```

returned 6 real rows:

| Provider | Tech | Down | Up |
| --- | --- | --- | --- |
| Comcast | 43 (cable) | 1000 | 35 |
| CenturyLink | 11 (DSL) | 15 | 0.75 |
| T-Mobile | 70 (fixed wireless) | 25 | 3 |
| ViaSat / HNS / VSAT | 60 (satellite) | 2–35 | 1.3–3 |

So for that house: **no symmetrical service, no fiber** as of the data date.
That is exactly the signal you want — and it took zero credentials.

**The catch: this data is frozen at December 2020 and is block-level.**
Block-level means "someone in this block can get it", not "this house can". And
Lumen has trenched a lot of fiber in Renton since 2020. So Form 477 is a
*strong negative signal* (if it showed no cable in 2020, it is probably still
thin) and a *weak positive* one.

**BDC (current, address-level) ⚠️ needs free registration.** A probe of
`broadbandmap.fcc.gov/api/public/...` returned `401 Unauthorized` ✅ — confirmed
it needs a username + token from broadbandmap.fcc.gov. It is also keyed on
**Location IDs from the Broadband Serviceable Location Fabric**, so it needs an
address→Location ID resolution step, not just lat/lon.

**Plan:** ship Form 477 now, labeled *"FCC Form 477, data as of Dec 2020,
block-level"*, with `symmetrical = down ≥ 900 && up ≥ 900` and a deep link to
`broadbandmap.fcc.gov/location-summary/fixed?lat=…&lon=…` for the human check.
Add BDC as a drop-in second source once you register (~15 min of signup, then
~2h of work).

---

## 4. HVAC classification 🔥 — *you hate oil and radiators, so let's make that loud*

**Effort: ~2h for the classifier, ~1 day for county verification.**

We already extract the raw text (`Fireplace, Heat Pump, Natural Gas` on the test
house). Add a classifier over it:

```ts
type HvacKind = 'heat-pump' | 'forced-air-gas' | 'forced-air-electric'
              | 'baseboard-electric' | 'radiator-steam' | 'radiant-floor'
              | 'oil' | 'none' | 'unknown';
```

Keyword → kind, plus a **`dislike` flag** for `oil` and `radiator-steam` so the
thread message can lead with `⚠️ oil heat` instead of burying it in a list. Note
that "radiant floor" (hydronic, modern, nice) and "radiator" (steam, old, on your
hate list) are different things and must not collapse — worth being careful in
the keyword table.

**Verification path (phase 2):** King County Assessor eReal Property exposes a
per-parcel `Heat Source` field, keyless HTML. Chain is
`lat/lon → KC GIS parcel PIN → assessor detail page`. That turns HVAC from
"unverified listing prose" into a county record. It only works in King County,
which is fine for now. This is the single biggest honesty upgrade available and
it costs nothing but code.

---

## 5. Photos 📷 — *ship the cheap version, skip the rest*

**Effort: ~1h for the defensible version.**

Zillow ships `og:image` ✅ — one URL, the same one any link preview shows:
`photos.zillowstatic.com/fp/…-cc_ft_1536.jpg`. Posting that single image is
ordinary link-preview behavior and is easy to defend.

Scraping the full 36-photo gallery and rehosting it is a licensing question, not
a technical one, and I would not ship it. If you want real galleries, that wants
an MLS/IDX feed with redistribution rights.

**Recommendation:** include `og:image` in the initial snapshot message. Done.

---

## Suggested order

1. **Phase 0 — geo columns** (~30 min). Unblocks everything.
2. **Transit** (~half a day). No keys, no cost, no dependency, and it is the
   thing you actually care about.
3. **HVAC classifier + oil/radiator warning** (~2h). Pure code, immediate value.
4. **FCC Form 477** (~half a day). Free, real data, honest caveats.
5. **`og:image`** (~1h). Trivial.
6. **Commute** (~1 day). Needs a Mapbox account first.
7. *Later:* King County Assessor HVAC verification, FCC BDC, OneBusAway
   frequencies.

Items 1–5 are ~2 days total, need **zero** credentials, and add no recurring
cost. Item 6 is the only one that needs an account, and its free tier is far
above our volume.

## Cross-cutting design notes

- **Enrichment runs once, at `add`/`bind` time**, not on refresh. Static facts
  about a location do not change between price cuts, and re-running them would
  burn quota to produce noise. Add a `/house enrich` to force recomputation.
- Store results in a separate `enrichment` table keyed by `property_id` + `kind`,
  with `provenance` and `computed_at`. Never merge them into `snapshot_json` —
  that is the *listing observation*, and mixing derived data into it would
  corrupt the change-detection diff.
- Every value carries its provenance string into the Discord message. A commute
  ETA with no departure-time assumption stated is a number that lies.
- All of it hangs off the existing `EnrichmentAdapter<T>` interface in
  `src/enrichment/index.ts`, which already returns
  `{ status: 'unavailable' | 'unverified' | 'ok', provenance }` — so a missing
  key degrades to a labeled gap instead of a silent absence.
