import { boundFetch } from '../http';
import { COMMUTE_DESTINATIONS, type CommuteEstimate } from './index';

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
  /** ISO-8601 with offset. Unset = next Tuesday 08:00 Pacific. */
  departureIso?: string;
  fetchImpl?: typeof fetch;
  apiUrl?: string;
  timeoutMs?: number;
}

export type CommuteResult =
  | { status: 'ok'; provenance: string; value: CommuteEstimate[] }
  | { status: 'unavailable'; provenance: string };

/**
 * Next Tuesday at 08:00 America/Los_Angeles, as an ISO instant.
 *
 * A commute ETA with no stated departure assumption is a number that lies: at
 * 05:00 the traffic-aware time comes in *below* free-flow. Pacific is UTC-7
 * (PDT) or UTC-8 (PST); we resolve the offset by asking Intl rather than
 * hardcoding, so this stays right across DST.
 */
export function nextTuesday8am(now: Date): string {
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
    8 - offsetHours,
    0,
    0,
  )).toISOString();
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

  const departure = cfg.departureIso?.trim() || nextTuesday8am(now);
  const doFetch = boundFetch(cfg.fetchImpl);
  const url = cfg.apiUrl ?? ROUTES_URL;
  const estimates: CommuteEstimate[] = [];
  const failures: string[] = [];

  for (const dest of COMMUTE_DESTINATIONS) {
    const controller = new AbortController();
    const timer = setTimeout(() => controller.abort(), cfg.timeoutMs ?? 8000);
    try {
      const res = await doFetch(url, {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
          'X-Goog-Api-Key': cfg.apiKey,
          'X-Goog-FieldMask': 'routes.duration,routes.staticDuration,routes.distanceMeters',
        },
        body: JSON.stringify({
          origin: { location: { latLng: { latitude: lat, longitude: lon } } },
          destination: { address: dest.address },
          travelMode: 'DRIVE',
          routingPreference: 'TRAFFIC_AWARE_OPTIMAL',
          departureTime: departure,
        }),
        signal: controller.signal,
      });

      if (!res.ok) {
        const body = await res.text().catch(() => '');
        failures.push(`${dest.label}: HTTP ${res.status} ${redact(body).slice(0, 120)}`);
        continue;
      }
      const json = (await res.json()) as {
        routes?: Array<{ duration?: string; staticDuration?: string; distanceMeters?: number }>;
      };
      const route = json.routes?.[0];
      const seconds = parseDuration(route?.duration);
      if (seconds === undefined) {
        failures.push(`${dest.label}: no route returned`);
        continue;
      }
      estimates.push({
        label: dest.label,
        destination: dest.address,
        driveSeconds: seconds,
        provider: 'google-routes',
        freeFlowSeconds: parseDuration(route?.staticDuration),
        distanceMeters: route?.distanceMeters,
      });
    } catch (err) {
      failures.push(
        `${dest.label}: ${controller.signal.aborted ? 'timeout' : redact(errText(err))}`,
      );
    } finally {
      clearTimeout(timer);
    }
  }

  if (estimates.length === 0) {
    return {
      status: 'unavailable',
      provenance: `google routes returned nothing — ${failures.join('; ') || 'unknown'}`,
    };
  }
  const partial = failures.length ? ` (partial: ${failures.join('; ')})` : '';
  return {
    status: 'ok',
    provenance: `google routes, TRAFFIC_AWARE_OPTIMAL, departing ${departure}${partial}`,
    value: estimates,
  };
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
