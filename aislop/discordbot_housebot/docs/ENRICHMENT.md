# Enrichment: what is scaffolded and why nothing brittle shipped

`src/enrichment/index.ts` defines the adapter interface and ships **declared-
unavailable** implementations for photos, commute, transit, and ISP. Each one
returns `{ status: 'unavailable', provenance: <why> }` rather than a guess.

That is deliberate. Every item below either needs a paid API, a self-hosted
service, or an address→parcel/fabric join. Scraping a listing page for them
produces numbers that *look* authoritative and are quietly wrong — which is
worse than an empty field when you are deciding where to live.

The one exception is HVAC, described at the bottom.

```ts
interface EnrichmentAdapter<T> {
  readonly name: string;
  readonly configured: boolean;
  run(snapshot: Snapshot): Promise<EnrichmentResult<T>>;
}
type EnrichmentResult<T> = {
  status: 'unavailable' | 'unverified' | 'ok';
  provenance: string;   // always populated
  value?: T;
};
```

To implement one: write the adapter, add its secret(s) to `.dev.vars.example` and
the README table, and call it from `HouseService` after a successful snapshot —
wrapped like the Airtable sync so a failure cannot break the Discord path.

## Commute to the two destinations

Targets: `551 Boren Ave N, Seattle, WA 98109` and `601 108th Ave NE, Bellevue, WA
98004` (already in `COMMUTE_DESTINATIONS`).

A car ETA needs geocoding + routing, and a *useful* one needs historical traffic
for a departure time. Options, cheapest-honest first:

| Option | Cost | Notes |
| --- | --- | --- |
| **Self-hosted OSRM or Valhalla** on an OSM PNW extract | server time only | Free-flow ETAs, no traffic model. Fine for relative comparison between houses, misleading as an actual 8am commute. Valhalla can apply speed profiles if you feed it OSM-based traffic tiles. |
| **OpenRouteService** hosted free tier | free, rate-limited | Same free-flow caveat; ToS limits commercial use. |
| **Mapbox Directions** (`driving-traffic`) | pay-as-you-go, small free tier | Real traffic, one call per house per destination. Best value for this use case. |
| **Google Routes API** with `departureTime` | paid, most expensive | Best traffic model; `TRAFFIC_AWARE_OPTIMAL` with a fixed weekday-8am departure gives a stable, comparable number. |
| **HERE Routing** | paid | Comparable to Mapbox. |

Geocoding the listing address first: the **US Census geocoder**
(`geocoding.geo.census.gov/geocoder/locations/onelineaddress`,
`benchmark=Public_AR_Current`) is keyless and adequate for US street addresses —
it is what the prior `house-lookup` project used. Note it often snaps to the
street centerline, which is fine for routing but not for parcel joins.

Recommendation: cache the geocode on the property row (addresses do not move),
compute commutes once per house at `/house add` time with a fixed departure
time, and never recompute on the cron — traffic changes, but not enough to
justify N calls per hour.

## Transit stop proximity and drive time

Use a real transit dataset, not the listing page:

- **GTFS static feeds** — King County Metro and Sound Transit both publish them
  (also aggregated on **Transitland** and **Mobility Database**). Parse
  `stops.txt` once into a spatial lookup; nearest-stop is then a local query with
  zero per-house API cost. This is the right answer for a small tracked
  portfolio.
- **OneBusAway** REST API (`api.pugetsound.onebusaway.org`) — `stops-for-location`
  returns nearby stops and routes with a free API key. Good for live-ish data
  without hosting GTFS yourself.
- Drive time *to* the stop reuses whatever routing provider you picked above;
  walk distance can be straight-line as a first cut, clearly labeled as such.

## ISP availability / symmetrical service

**A generic listing page has no canonical ISP data.** Availability is keyed on a
specific address, and the FCC's current data model (Broadband Data Collection)
is keyed on **Location IDs from the Broadband Serviceable Location Fabric**, not
on lat/lon alone.

- **FCC BDC API** (`broadbandmap.fcc.gov/api`) needs registered credentials and
  an address→Location ID resolution step. This is the only authoritative source.
- **FCC Form 477** (Socrata `opendata.fcc.gov/resource/hicn-aujz.json`, keyed on
  **2010** census blocks via `geo.fcc.gov/api/census/block/find?censusYear=2010`)
  is keyless but frozen at **December 2020** and block-level, so it over-reports
  ("someone in this block can get fiber" ≠ "this house can"). Usable as a
  *hint*, never as an answer. Symmetrical gigabit ≈ a provider reporting both
  `maxaddown` and `maxadup` ≥ 900.
- Practical fallback: emit a deep link to
  `https://broadbandmap.fcc.gov/location-summary/fixed?lat=..&lon=..` and let a
  human read it. Cheap, honest, zero credentials.

## Listing photos

Photo URLs are provider-hosted assets governed by provider terms. Rehosting or
hotlinking them is a licensing question, not a technical one. Options: link out
to the listing (what housebot does today), or let a human attach photos to the
thread. If you ever want automated photos, get an MLS/IDX feed with explicit
redistribution rights.

## HVAC — the one thing we do attempt

`scanHvac` reads a `heating` / `heatingSystem` / `heatSource` field, or a
`Heating: …` label, from the public listing text when the provider rendered one.
It is surfaced as **"HVAC (unverified, from listing text)"** everywhere and is
treated as a material field for change detection.

It is unreliable on purpose-built listing copy: agents write "forced air" for a
heat pump, omit the field entirely, or describe the fireplace. For anything
load-bearing, the **King County Assessor eReal Property** detail page exposes a
`Heat Source` field per parcel (keyless HTML, reachable via parcel PIN) — that is
the verification path, and it needs the address→parcel join that the enrichment
adapters above would provide.
