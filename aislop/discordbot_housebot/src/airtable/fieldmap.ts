import { formatAddress } from '../listing/format';
import type { Snapshot } from '../listing/types';

/**
 * Logical keys housebot can export. The operator maps whichever of these they
 * care about onto real Airtable field ids/names; nothing is assumed about the
 * base schema, and unmapped keys are simply not sent.
 */
export const EXPORT_KEYS = [
  'key', // canonical listing key -- required for upsert
  'url',
  'provider',
  'listingId',
  'status',
  'price',
  'address',
  'city',
  'state',
  'zip',
  'beds',
  'baths',
  'sqft',
  'lotSqft',
  'yearBuilt',
  'hvacUnverified',
  'threadUrl',
  'lastCheckedIso',
] as const;

export type ExportKey = (typeof EXPORT_KEYS)[number];
export type FieldMap = Partial<Record<ExportKey, string>>;

export interface FieldMapResult {
  ok: boolean;
  map: FieldMap;
  problem?: string;
}

export function parseFieldMap(json: string | undefined): FieldMapResult {
  if (!json || !json.trim()) {
    return { ok: false, map: {}, problem: 'AIRTABLE_FIELD_MAP_JSON is not set' };
  }
  let parsed: unknown;
  try {
    parsed = JSON.parse(json);
  } catch {
    return { ok: false, map: {}, problem: 'AIRTABLE_FIELD_MAP_JSON is not valid JSON' };
  }
  if (typeof parsed !== 'object' || parsed === null || Array.isArray(parsed)) {
    return { ok: false, map: {}, problem: 'AIRTABLE_FIELD_MAP_JSON must be a JSON object' };
  }

  const map: FieldMap = {};
  const unknown: string[] = [];
  for (const [k, v] of Object.entries(parsed as Record<string, unknown>)) {
    if (!(EXPORT_KEYS as readonly string[]).includes(k)) {
      unknown.push(k);
      continue;
    }
    if (typeof v === 'string' && v.trim()) map[k as ExportKey] = v.trim();
  }
  if (!map.key) {
    return {
      ok: false,
      map,
      problem: 'field map must include "key" (the canonical listing key column) for upsert',
    };
  }
  return {
    ok: true,
    map,
    problem: unknown.length ? `ignored unknown keys: ${unknown.join(', ')}` : undefined,
  };
}

export interface ExportContext {
  guildId?: string | null;
  threadId?: string | null;
}

/** Build the logical record, then project it through the operator's map. */
export function buildFields(
  snapshot: Snapshot,
  map: FieldMap,
  ctx: ExportContext = {},
): Record<string, unknown> {
  const logical: Partial<Record<ExportKey, unknown>> = {
    key: snapshot.listingKey,
    url: snapshot.sourceUrl,
    provider: snapshot.provider,
    listingId: snapshot.listingId,
    status: snapshot.status,
    price: snapshot.price,
    address: formatAddress(snapshot) ?? undefined,
    city: snapshot.city,
    state: snapshot.state,
    zip: snapshot.zip,
    beds: snapshot.beds,
    baths: snapshot.baths,
    sqft: snapshot.sqft,
    lotSqft: snapshot.lotSqft,
    yearBuilt: snapshot.yearBuilt,
    hvacUnverified: snapshot.hvac,
    threadUrl:
      ctx.guildId && ctx.threadId
        ? `https://discord.com/channels/${ctx.guildId}/${ctx.threadId}`
        : undefined,
    lastCheckedIso: new Date(snapshot.fetchedAt * 1000).toISOString(),
  };

  const out: Record<string, unknown> = {};
  for (const [logicalKey, field] of Object.entries(map) as Array<[ExportKey, string]>) {
    const value = logical[logicalKey];
    if (value === undefined || value === null || value === '') continue;
    out[field] = value;
  }
  return out;
}
