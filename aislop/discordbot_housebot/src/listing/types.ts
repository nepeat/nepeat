export type Provider = 'zillow' | 'redfin';

/** Coarse lifecycle status normalized across providers. */
export type ListingStatus =
  | 'active'
  | 'pending'
  | 'contingent'
  | 'closed'
  | 'unknown';

/** Terminal states: the house is off the market. */
export const CLOSED_STATUSES: ReadonlySet<ListingStatus> = new Set(['closed']);

/** A normalized listing observation. Every field is optional except identity. */
export interface Snapshot {
  provider: Provider;
  listingId: string;
  /** `${provider}:${listingId}` -- the canonical dedupe key. */
  listingKey: string;
  /** Honest, normalized source URL. Never fabricated. */
  sourceUrl: string;

  status: ListingStatus;
  /** Raw provider status string, kept for auditability. */
  statusRaw?: string;

  price?: number;
  address?: string;
  city?: string;
  state?: string;
  zip?: string;
  beds?: number;
  baths?: number;
  sqft?: number;
  lotSqft?: number;
  yearBuilt?: number;
  /** Parsed from listing text when present. Always unverified. */
  hvac?: string;

  /** Unix seconds when this observation was taken. */
  fetchedAt: number;
}

/** Fields whose change is worth announcing in the thread. */
export const MATERIAL_FIELDS = [
  'status',
  'price',
  'beds',
  'baths',
  'sqft',
  'address',
  'yearBuilt',
  'hvac',
] as const;

export type MaterialField = (typeof MATERIAL_FIELDS)[number];

export interface FieldChange {
  field: MaterialField;
  from: string | null;
  to: string | null;
}

/** Result of a source fetch. Failures are values, not exceptions. */
export type FetchResult =
  | { ok: true; kind: 'snapshot'; snapshot: Snapshot; etag?: string; lastModified?: string }
  | { ok: true; kind: 'not-modified' }
  | { ok: false; kind: 'error'; reason: FetchFailureReason; detail: string };

export type FetchFailureReason =
  | 'network'
  | 'timeout'
  | 'http-status'
  | 'blocked'
  | 'unparseable'
  | 'unsupported-url';

export interface FetchOptions {
  userAgent: string;
  timeoutMs: number;
  etag?: string | null;
  lastModified?: string | null;
  /** Injected for tests; defaults to globalThis.fetch. */
  fetchImpl?: typeof fetch;
}

/**
 * A provider adapter. Public property pages are a best-effort source: adapters
 * must not log in, must not use private/internal APIs, and must not attempt to
 * evade anti-bot controls. If the page does not hand us structured data, the
 * adapter reports `unparseable` and the caller degrades gracefully.
 */
export interface ListingSource {
  provider: Provider;
  /** True if this adapter owns the URL. */
  matches(url: URL): boolean;
  /** Canonical, tracking-free URL + listing id, or null if not recognizable. */
  identify(url: URL): { listingId: string; canonicalUrl: string } | null;
  /** Parse an already-fetched page body into a snapshot. Pure + testable. */
  parse(html: string, canonicalUrl: string, listingId: string, fetchedAt: number): Snapshot | null;
}
