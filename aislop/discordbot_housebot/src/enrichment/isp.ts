import { boundFetch } from '../http';

/**
 * Fixed-broadband availability from the FCC.
 *
 * Uses **Form 477**, which is keyless — verified live 2026-08-03 against the
 * Renton test house. Two hard caveats, both surfaced in the provenance string
 * because they change how much the answer is worth:
 *
 *  1. The data is frozen at **December 2020**. Fiber built since is invisible.
 *  2. It is **census-block level**: "somebody in this block can get it", not
 *     "this house can". So it is a strong negative signal and a weak positive.
 *
 * The current, address-level dataset is BDC, which needs registered credentials
 * AND an address -> Fabric Location ID resolution step. See docs/ROADMAP.md.
 */

const BLOCK_URL = 'https://geo.fcc.gov/api/census/block/find';
const FORM477_URL = 'https://opendata.fcc.gov/resource/hicn-aujz.json';

/** Symmetrical gigabit: both directions at or above 900 Mbps. */
export const SYMMETRIC_MBPS = 900;

const TECH_LABEL: Record<string, string> = {
  '10': 'DSL',
  '11': 'DSL',
  '12': 'DSL',
  '20': 'copper',
  '30': 'copper',
  '40': 'cable',
  '41': 'cable',
  '42': 'cable',
  '43': 'cable',
  '50': 'fiber',
  '60': 'satellite',
  '70': 'fixed wireless',
  '90': 'other',
};

export function techLabel(code: string | undefined): string {
  return TECH_LABEL[String(code ?? '').trim()] ?? 'other';
}

export interface IspOffer {
  provider: string;
  technology: string;
  downMbps: number;
  upMbps: number;
  symmetrical: boolean;
}

export interface IspConfig {
  fetchImpl?: typeof fetch;
  timeoutMs?: number;
  blockUrl?: string;
  form477Url?: string;
}

export type IspResult =
  | { status: 'unverified'; provenance: string; value: IspOffer[] }
  | { status: 'unavailable'; provenance: string };

interface RawRow {
  providername?: string;
  techcode?: string;
  maxaddown?: string | number;
  maxadup?: string | number;
}

export async function lookupIsp(
  lat: number | null | undefined,
  lon: number | null | undefined,
  cfg: IspConfig = {},
): Promise<IspResult> {
  if (typeof lat !== 'number' || typeof lon !== 'number') {
    return {
      status: 'unavailable',
      provenance: 'listing page did not publish coordinates, so no census block to look up',
    };
  }
  const doFetch = boundFetch(cfg.fetchImpl);
  const timeoutMs = cfg.timeoutMs ?? 8000;

  // Form 477 is keyed on 2010 blocks, NOT 2020 ones -- querying with a 2020
  // block silently returns zero rows.
  let block: string;
  try {
    const url =
      `${cfg.blockUrl ?? BLOCK_URL}?latitude=${lat}&longitude=${lon}` +
      `&censusYear=2010&format=json`;
    const res = await withTimeout(doFetch, url, timeoutMs);
    if (!res.ok) {
      return { status: 'unavailable', provenance: `fcc block lookup HTTP ${res.status}` };
    }
    const json = (await res.json()) as { Block?: { FIPS?: string } };
    const fips = json.Block?.FIPS;
    if (!fips) return { status: 'unavailable', provenance: 'fcc returned no census block' };
    block = fips;
  } catch (err) {
    return { status: 'unavailable', provenance: `fcc block lookup failed: ${errText(err)}` };
  }

  try {
    const where = encodeURIComponent(`blockcode='${block}' and consumer='1'`);
    const url = `${cfg.form477Url ?? FORM477_URL}?$where=${where}`;
    const res = await withTimeout(doFetch, url, timeoutMs);
    if (!res.ok) {
      return { status: 'unavailable', provenance: `fcc form 477 HTTP ${res.status}` };
    }
    const rows = (await res.json()) as RawRow[];
    const offers = dedupeOffers(rows);
    if (offers.length === 0) {
      return {
        status: 'unavailable',
        provenance: `no FCC Form 477 rows for census block ${block}`,
      };
    }
    return {
      status: 'unverified',
      provenance:
        `FCC Form 477, census block ${block} — data as of Dec 2020 and block-level, ` +
        `so it is a strong negative signal and a weak positive one`,
      value: offers,
    };
  } catch (err) {
    return { status: 'unavailable', provenance: `fcc form 477 failed: ${errText(err)}` };
  }
}

/** One row per provider, keeping its fastest offering. */
export function dedupeOffers(rows: RawRow[]): IspOffer[] {
  const best = new Map<string, IspOffer>();
  for (const row of rows) {
    const provider = String(row.providername ?? '').trim();
    if (!provider) continue;
    const downMbps = toNumber(row.maxaddown);
    const upMbps = toNumber(row.maxadup);
    if (downMbps === undefined || upMbps === undefined) continue;

    const offer: IspOffer = {
      provider,
      technology: techLabel(row.techcode),
      downMbps,
      upMbps,
      symmetrical: downMbps >= SYMMETRIC_MBPS && upMbps >= SYMMETRIC_MBPS,
    };
    const key = provider.toLowerCase();
    const existing = best.get(key);
    if (!existing || offer.downMbps > existing.downMbps) best.set(key, offer);
  }
  return [...best.values()].sort((a, b) => b.downMbps - a.downMbps);
}

export function formatIsp(offers: IspOffer[]): string {
  const symmetric = offers.filter((o) => o.symmetrical);
  const head = symmetric.length
    ? `✅ **symmetrical available** — ${symmetric.map((o) => o.provider).join(', ')}`
    : '⚠️ **no symmetrical service on record**';

  const lines = offers.slice(0, 6).map((o) => {
    const mark = o.symmetrical ? '✅ ' : '';
    return `• ${mark}${o.provider} (${o.technology}) — ${fmt(o.downMbps)}↓ / ${fmt(o.upMbps)}↑`;
  });
  return [head, ...lines].join('\n');
}

function fmt(mbps: number): string {
  return mbps >= 1000 ? `${mbps / 1000} Gbps` : `${mbps} Mbps`;
}

function toNumber(v: string | number | undefined): number | undefined {
  if (v === undefined) return undefined;
  const n = typeof v === 'number' ? v : Number.parseFloat(v);
  return Number.isFinite(n) ? n : undefined;
}

async function withTimeout(
  doFetch: typeof fetch,
  url: string,
  timeoutMs: number,
): Promise<Response> {
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), timeoutMs);
  try {
    return await doFetch(url, { signal: controller.signal });
  } finally {
    clearTimeout(timer);
  }
}

function errText(err: unknown): string {
  return err instanceof Error ? err.message : String(err);
}
