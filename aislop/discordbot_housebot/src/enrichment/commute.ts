import { boundFetch } from '../http';
import {
  COMMUTE_DESTINATIONS,
  type CommuteEstimate,
  type TransitItinerary,
} from './index';
import { hopsFromSteps, type RawTransitStep } from './transit';

/**
 * Drive times via the Google Routes API.
 *
 * Billing note: `TRAFFIC_AWARE_OPTIMAL` puts this on the **Compute Routes Pro**
 * SKU (5,000 free calls/month). We make 2 calls per house, once, at add time --
 * houses do not move, so recomputing on every refresh would burn quota to
 * produce noise. Verified live 2026-08-03: 18,083 m / 1,115 s to Bellevue.
 */

const ROUTES_URL = 'https://routes.googleapis.com/directions/v2:computeRoutes';

export interface CommuteConfig {
  apiKey?: string;
  /** Driving departure, ISO-8601. Unset = next Tuesday 08:00 Pacific. */
  departureIso?: string;
  /** Transit departure, ISO-8601. Unset = next Tuesday 10:00 Pacific. */
  transitDepartureIso?: string;
  fetchImpl?: typeof fetch;
  apiUrl?: string;
  timeoutMs?: number;
}

export interface CommuteValue {
  drive: CommuteEstimate[];
  transit: TransitItinerary[];
}

/**
 * Read a stored commute value, tolerating the pre-transit shape.
 *
 * Before transit existed the value was a bare `CommuteEstimate[]`. Rows written
 * then still exist in D1, so parsing has to accept both -- otherwise an old row
 * silently renders as "not available" despite holding real drive times.
 */
export function normalizeCommuteValue(raw: unknown): CommuteValue | null {
  if (Array.isArray(raw)) return { drive: raw as CommuteEstimate[], transit: [] };
  if (raw && typeof raw === 'object') {
    const v = raw as Partial<CommuteValue>;
    if (Array.isArray(v.drive)) {
      return { drive: v.drive, transit: Array.isArray(v.transit) ? v.transit : [] };
    }
  }
  return null;
}

/**
 * True only for a value produced by the current adapter. A legacy row has drive
 * times but no `transit` key at all, and must be recomputed rather than treated
 * as settled -- otherwise `missing` mode would skip it forever.
 */
export function isCurrentCommuteShape(raw: unknown): boolean {
  return Boolean(raw && typeof raw === 'object' && !Array.isArray(raw) &&
    Array.isArray((raw as Partial<CommuteValue>).drive) &&
    Array.isArray((raw as Partial<CommuteValue>).transit));
}

export type CommuteResult =
  | { status: 'ok'; provenance: string; value: CommuteValue }
  | { status: 'unavailable'; provenance: string };

/**
 * Next Tuesday at a given local hour in America/Los_Angeles, as an ISO instant.
 *
 * A commute ETA with no stated departure assumption is a number that lies: at
 * 05:00 the traffic-aware drive time comes in *below* free-flow. Driving uses
 * 08:00 (the actual rush hour you would sit in); transit uses 10:00, where
 * headways are representative rather than peak-only. Pacific is UTC-7 (PDT) or
 * UTC-8 (PST); the offset is resolved via Intl rather than hardcoded, so this
 * stays correct across DST.
 */
export function nextTuesdayAt(now: Date, localHour: number): string {
  const day = now.getUTCDay();
  // 2 = Tuesday. Always land on a future Tuesday, never today.
  const ahead = ((2 - day + 7) % 7) || 7;
  const target = new Date(Date.UTC(
    now.getUTCFullYear(),
    now.getUTCMonth(),
    now.getUTCDate() + ahead,
    15, // 08:00 PDT == 15:00 UTC; corrected below if the date is in PST
    0,
    0,
  ));
  const offsetHours = pacificOffsetHours(target);
  return new Date(Date.UTC(
    target.getUTCFullYear(),
    target.getUTCMonth(),
    target.getUTCDate(),
    localHour - offsetHours,
    0,
    0,
  )).toISOString();
}

export const DRIVE_HOUR_LOCAL = 8;
export const TRANSIT_HOUR_LOCAL = 10;

/** Back-compat helper: the driving default. */
export function nextTuesday8am(now: Date): string {
  return nextTuesdayAt(now, DRIVE_HOUR_LOCAL);
}

/** -7 during PDT, -8 during PST. */
function pacificOffsetHours(at: Date): number {
  const fmt = new Intl.DateTimeFormat('en-US', {
    timeZone: 'America/Los_Angeles',
    timeZoneName: 'shortOffset',
  });
  const name = fmt.formatToParts(at).find((p) => p.type === 'timeZoneName')?.value ?? 'GMT-8';
  const m = /GMT([+-]\d+)/.exec(name);
  return m ? Number(m[1]) : -8;
}

export async function estimateCommutes(
  lat: number | null | undefined,
  lon: number | null | undefined,
  cfg: CommuteConfig,
  now: Date = new Date(),
): Promise<CommuteResult> {
  if (!cfg.apiKey) {
    return { status: 'unavailable', provenance: 'GOOGLE_MAPS_API_KEY not set' };
  }
  if (typeof lat !== 'number' || typeof lon !== 'number') {
    return {
      status: 'unavailable',
      provenance: 'listing page did not publish coordinates, so no origin to route from',
    };
  }

  const departure = cfg.departureIso?.trim() || nextTuesdayAt(now, DRIVE_HOUR_LOCAL);
  // Transit gets its own, later departure: 08:00 answers "my commute", 10:00
  // answers "can I actually get there on transit" without peak-only service
  // flattering the result.
  const transitDeparture =
    cfg.transitDepartureIso?.trim() || nextTuesdayAt(now, TRANSIT_HOUR_LOCAL);
  const doFetch = boundFetch(cfg.fetchImpl);
  const url = cfg.apiUrl ?? ROUTES_URL;
  const drive: CommuteEstimate[] = [];
  const transit: TransitItinerary[] = [];
  const failures: string[] = [];

  for (const dest of COMMUTE_DESTINATIONS) {
    // DRIVE: traffic-aware at the pinned departure time.
    const driveRoute = await callRoutes(doFetch, url, cfg, {
      lat,
      lon,
      destination: dest.address,
      departure,
      mode: 'DRIVE',
    });
    if (driveRoute.ok) {
      const seconds = parseDuration(driveRoute.route?.duration);
      if (seconds === undefined) {
        failures.push(`${dest.label} drive: no route returned`);
      } else {
        drive.push({
          label: dest.label,
          destination: dest.address,
          driveSeconds: seconds,
          provider: 'google-routes',
          freeFlowSeconds: parseDuration(driveRoute.route?.staticDuration),
          distanceMeters: driveRoute.route?.distanceMeters,
        });
      }
    } else {
      failures.push(`${dest.label} drive: ${driveRoute.detail}`);
    }

    // TRANSIT: note that top-level routingPreference is DRIVE-only and errors
    // here, so it is deliberately omitted.
    const transitRoute = await callRoutes(doFetch, url, cfg, {
      lat,
      lon,
      destination: dest.address,
      departure: transitDeparture,
      mode: 'TRANSIT',
    });
    if (transitRoute.ok) {
      const seconds = parseDuration(transitRoute.route?.duration);
      const steps = transitRoute.route?.legs?.flatMap((l) => l.steps ?? []) ?? [];
      const { hops, walkSeconds } = hopsFromSteps(steps, parseDuration);
      if (seconds === undefined || hops.length === 0) {
        failures.push(`${dest.label} transit: no usable itinerary`);
      } else {
        transit.push({
          label: dest.label,
          destination: dest.address,
          totalSeconds: seconds,
          hops,
          walkSeconds,
        });
      }
    } else {
      failures.push(`${dest.label} transit: ${transitRoute.detail}`);
    }
  }

  if (drive.length === 0 && transit.length === 0) {
    return {
      status: 'unavailable',
      provenance: `google routes returned nothing — ${failures.join('; ') || 'unknown'}`,
    };
  }
  const partial = failures.length ? ` (partial: ${failures.join('; ')})` : '';
  return {
    status: 'ok',
    provenance:
      `google routes — drive TRAFFIC_AWARE_OPTIMAL departing ${departure}, ` +
      `transit departing ${transitDeparture}${partial}`,
    value: { drive, transit },
  };
}

interface RawRoute {
  duration?: string;
  staticDuration?: string;
  distanceMeters?: number;
  legs?: Array<{ steps?: RawTransitStep[] }>;
}

type RoutesCall =
  | { ok: true; route: RawRoute | undefined }
  | { ok: false; detail: string };

const DRIVE_MASK = 'routes.duration,routes.staticDuration,routes.distanceMeters';
const TRANSIT_MASK =
  'routes.duration,routes.legs.steps.transitDetails,routes.legs.steps.travelMode,routes.legs.steps.staticDuration';

async function callRoutes(
  doFetch: typeof fetch,
  url: string,
  cfg: CommuteConfig,
  req: {
    lat: number;
    lon: number;
    destination: string;
    departure: string;
    mode: 'DRIVE' | 'TRANSIT';
  },
): Promise<RoutesCall> {
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), cfg.timeoutMs ?? 8000);
  try {
    const body: Record<string, unknown> = {
      origin: { location: { latLng: { latitude: req.lat, longitude: req.lon } } },
      destination: { address: req.destination },
      travelMode: req.mode,
      departureTime: req.departure,
    };
    if (req.mode === 'DRIVE') body['routingPreference'] = 'TRAFFIC_AWARE_OPTIMAL';

    const res = await doFetch(url, {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
        'X-Goog-Api-Key': cfg.apiKey as string,
        'X-Goog-FieldMask': req.mode === 'DRIVE' ? DRIVE_MASK : TRANSIT_MASK,
      },
      body: JSON.stringify(body),
      signal: controller.signal,
    });

    if (!res.ok) {
      const text = await res.text().catch(() => '');
      return { ok: false, detail: `HTTP ${res.status} ${redact(text).slice(0, 120)}` };
    }
    const json = (await res.json()) as { routes?: RawRoute[] };
    return { ok: true, route: json.routes?.[0] };
  } catch (err) {
    return {
      ok: false,
      detail: controller.signal.aborted ? 'timeout' : redact(errText(err)),
    };
  } finally {
    clearTimeout(timer);
  }
}

/** Routes API returns durations as protobuf strings: "1115s". */
export function parseDuration(v: string | undefined): number | undefined {
  if (!v) return undefined;
  const m = /^(\d+(?:\.\d+)?)s$/.exec(v.trim());
  if (!m) return undefined;
  const n = Number(m[1]);
  return Number.isFinite(n) ? Math.round(n) : undefined;
}

export function formatCommute(estimates: CommuteEstimate[]): string {
  return estimates
    .map((e) => {
      const mins = Math.round(e.driveSeconds / 60);
      const miles =
        e.distanceMeters === undefined
          ? ''
          : ` · ${(e.distanceMeters / 1609.344).toFixed(1)} mi`;
      const free =
        e.freeFlowSeconds === undefined
          ? ''
          : ` (${Math.round(e.freeFlowSeconds / 60)} min free-flow)`;
      return `• ${e.label}: **${mins} min**${free}${miles}`;
    })
    .join('\n');
}

function errText(err: unknown): string {
  return err instanceof Error ? err.message : String(err);
}

/** The key travels in a header, but never let a echoed URL or body leak it. */
function redact(text: string): string {
  return text.replace(/AIza[0-9A-Za-z_-]{10,}/g, 'AIza***');
}
