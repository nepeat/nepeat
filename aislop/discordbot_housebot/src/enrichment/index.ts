import type { Snapshot } from '../listing/types';

/**
 * Enrichment adapters are INTERFACES ONLY in this MVP.
 *
 * Everything here needs either a paid API, a self-hosted service, or an address
 * -> fabric/parcel join that a generic listing page cannot provide honestly.
 * Shipping a scraped guess would be worse than shipping nothing, so the default
 * implementations declare themselves unavailable and say what would be needed.
 * See docs/ENRICHMENT.md for the integration options.
 */

export type EnrichmentStatus = 'unavailable' | 'unverified' | 'ok';

export interface EnrichmentResult<T> {
  status: EnrichmentStatus;
  /** Why it is unavailable, or where the value came from. Always populated. */
  provenance: string;
  value?: T;
}

export interface EnrichmentAdapter<T> {
  readonly name: string;
  readonly configured: boolean;
  run(snapshot: Snapshot): Promise<EnrichmentResult<T>>;
}

function stub<T>(name: string, provenance: string): EnrichmentAdapter<T> {
  return {
    name,
    configured: false,
    async run() {
      return { status: 'unavailable', provenance };
    },
  };
}

/** Commute destinations the household actually cares about. */
export const COMMUTE_DESTINATIONS = [
  { label: 'Bellevue office (nep)', address: '601 108th Ave NE, Bellevue, WA 98004' },
  { label: 'Seattle office (partner)', address: '551 Boren Ave N, Seattle, WA 98109' },
] as const;

export interface CommuteEstimate {
  label: string;
  destination: string;
  driveSeconds: number;
  provider: string;
}

export interface TransitStop {
  name: string;
  routes: string[];
  driveSeconds?: number;
  walkMeters?: number;
}

export interface IspOffer {
  provider: string;
  technology: string;
  downMbps: number;
  upMbps: number;
  symmetrical: boolean;
}

export const photosAdapter = stub<string[]>(
  'photos',
  'listing photo URLs are provider-hosted assets under their terms; needs an explicit licensed feed or manual upload',
);

export const commuteAdapter = stub<CommuteEstimate[]>(
  'commute',
  'needs a routing/geocoding provider (Google/Mapbox/HERE) or self-hosted OSRM+Valhalla; no free reliable car ETA exists',
);

export const transitAdapter = stub<TransitStop[]>(
  'transit',
  'needs a GTFS / OneBusAway / Transitland source for nearby stops and headways',
);

export const ispAdapter = stub<IspOffer[]>(
  'isp',
  'FCC availability is keyed on address/fabric ids (BDC API credentials), not on a listing page',
);

/**
 * HVAC is the one thing we do attempt, and only from listing text that the
 * provider already rendered. It is always marked unverified.
 */
export const hvacAdapter: EnrichmentAdapter<string> = {
  name: 'hvac',
  configured: true,
  async run(snapshot) {
    if (!snapshot.hvac) {
      return {
        status: 'unavailable',
        provenance: 'listing page did not expose a heating/HVAC field',
      };
    }
    return {
      status: 'unverified',
      provenance: 'parsed from public listing text; not confirmed against county records',
      value: snapshot.hvac,
    };
  },
};

export const ADAPTERS = {
  photos: photosAdapter,
  commute: commuteAdapter,
  transit: transitAdapter,
  isp: ispAdapter,
  hvac: hvacAdapter,
};
