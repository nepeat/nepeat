import {
  extractJsonLd,
  findByType,
  isRecord,
  normalizeStatus,
  num,
  scanHvac,
  scanNumber,
  scanString,
  splitAddress,
  str,
} from '../parse';
import type { Provider, Snapshot } from '../types';

/**
 * Shared LD+JSON reader. Both providers publish schema.org Residence/Product
 * markup on public pages; the provider adapters layer their own fallbacks on
 * top of whatever this recovers.
 */
export function snapshotFromJsonLd(
  html: string,
  provider: Provider,
  listingId: string,
  canonicalUrl: string,
  fetchedAt: number,
): Partial<Snapshot> {
  const nodes = extractJsonLd(html);
  const place =
    findByType(nodes, [
      'SingleFamilyResidence',
      'Residence',
      'House',
      'Apartment',
      'Product',
      'Place',
      'Offer',
    ]) ?? undefined;

  const out: Partial<Snapshot> = { provider, listingId, sourceUrl: canonicalUrl, fetchedAt };
  if (!place) return out;

  const addr = place['address'];
  if (isRecord(addr)) {
    out.address = str(addr['streetAddress']);
    out.city = str(addr['addressLocality']);
    out.state = str(addr['addressRegion']);
    out.zip = str(addr['postalCode']);
  } else if (typeof addr === 'string') {
    Object.assign(out, splitAddress(addr));
  }

  const floor = place['floorSize'];
  if (isRecord(floor)) out.sqft = num(floor['value']);

  const beds = place['numberOfBedrooms'] ?? place['numberOfRooms'];
  out.beds = num(isRecord(beds) ? beds['value'] : beds);

  const baths =
    place['numberOfBathroomsTotal'] ??
    place['numberOfBathrooms'] ??
    place['numberOfFullBathrooms'];
  out.baths = num(isRecord(baths) ? baths['value'] : baths);

  out.yearBuilt = num(place['yearBuilt']);

  const offers = place['offers'];
  const offer = Array.isArray(offers) ? offers[0] : offers;
  if (isRecord(offer)) {
    out.price = num(offer['price']);
    const avail = str(offer['availability']);
    if (avail) {
      out.statusRaw = avail;
      out.status = normalizeStatus(avail);
    }
  }
  return out;
}

/** Loose hydration-blob fallbacks common to both providers. */
export function snapshotFromScan(html: string): Partial<Snapshot> {
  const out: Partial<Snapshot> = {};
  out.price = scanNumber(html, 'price') ?? scanNumber(html, 'listPrice');
  out.beds = scanNumber(html, 'bedrooms') ?? scanNumber(html, 'beds');
  out.baths = scanNumber(html, 'bathrooms') ?? scanNumber(html, 'baths');
  out.sqft =
    scanNumber(html, 'livingArea') ??
    scanNumber(html, 'sqFt') ??
    scanNumber(html, 'squareFeet');
  out.lotSqft = scanNumber(html, 'lotSize') ?? scanNumber(html, 'lotSquareFeet');
  out.yearBuilt = scanNumber(html, 'yearBuilt');
  const hvac = scanHvac(html);
  if (hvac) out.hvac = hvac;
  const rawStatus =
    scanString(html, 'homeStatus') ??
    scanString(html, 'mlsStatus') ??
    scanString(html, 'listingStatus');
  if (rawStatus) {
    out.statusRaw = rawStatus;
    out.status = normalizeStatus(rawStatus);
  }
  return out;
}

/** Merge partials, preferring earlier (higher-confidence) sources. */
export function mergeSnapshot(
  ...parts: Array<Partial<Snapshot>>
): Partial<Snapshot> {
  const out: Record<string, unknown> = {};
  for (const p of parts) {
    for (const [k, v] of Object.entries(p)) {
      if (v === undefined || v === null || v === '') continue;
      if (out[k] === undefined) out[k] = v;
    }
  }
  return out as Partial<Snapshot>;
}

/** Promote a partial to a full Snapshot, or null if identity is missing. */
export function finalize(
  partial: Partial<Snapshot>,
  provider: Provider,
  listingId: string,
  canonicalUrl: string,
  fetchedAt: number,
): Snapshot | null {
  // Identity always comes from the URL, never from the page.
  const hasAnyFacts =
    partial.price !== undefined ||
    partial.address !== undefined ||
    partial.sqft !== undefined ||
    partial.beds !== undefined ||
    partial.status !== undefined;
  if (!hasAnyFacts) return null;

  return {
    ...partial,
    provider,
    listingId,
    listingKey: `${provider}:${listingId}`,
    sourceUrl: canonicalUrl,
    status: partial.status ?? 'unknown',
    fetchedAt,
  } as Snapshot;
}

/** og:title is a decent last-resort address on both providers. */
export function addressFromMeta(html: string): Partial<Snapshot> {
  const m =
    /<meta[^>]+property=["']og:title["'][^>]+content=["']([^"']+)["']/i.exec(html) ??
    /<meta[^>]+name=["']twitter:title["'][^>]+content=["']([^"']+)["']/i.exec(html);
  const title = m?.[1];
  if (!title) return {};
  // "400 Cedar Ave S, Renton, WA 98057 | MLS #123 | Zillow"
  const head = title.split('|')[0]?.trim() ?? '';
  if (!head) return {};
  return splitAddress(head);
}
