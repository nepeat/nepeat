import type { FieldChange, ListingStatus, Snapshot } from './types';

export const THREAD_NAME_MAX = 100;
export const CLOSED_PREFIX = '❌ ';

export function formatPrice(n: number | undefined): string | null {
  if (n === undefined || !Number.isFinite(n) || n <= 0) return null;
  return `$${Math.round(n).toLocaleString('en-US')}`;
}

/**
 * Compact price for thread titles: 725000 -> $725K, 749900 -> $749.9K,
 * 1250000 -> $1.25M. The exact figure still appears in the snapshot message,
 * the embed and every change notice -- this abbreviation exists only to buy
 * back characters against Discord's 100-char thread-name cap.
 */
export function formatPriceShort(n: number | undefined): string | null {
  if (n === undefined || !Number.isFinite(n) || n <= 0) return null;
  if (n >= 1_000_000) return `$${trimZeros(n / 1_000_000, 2)}M`;
  if (n >= 1_000) return `$${trimZeros(n / 1_000, 1)}K`;
  return `$${Math.round(n)}`;
}

/** Round to at most `places` decimals, then drop trailing zeros. */
function trimZeros(value: number, places: number): string {
  return String(Number(value.toFixed(places)));
}

export function formatSqft(n: number | undefined): string | null {
  if (n === undefined || !Number.isFinite(n) || n <= 0) return null;
  return `${Math.round(n).toLocaleString('en-US')}ft`;
}

function trimNum(n: number): string {
  return Number.isInteger(n) ? String(n) : String(Number(n.toFixed(1)));
}

export function formatBeds(beds?: number, baths?: number): string | null {
  const parts: string[] = [];
  if (beds !== undefined && beds > 0) parts.push(`${trimNum(beds)}b`);
  if (baths !== undefined && baths > 0) parts.push(`${trimNum(baths)}b`);
  return parts.length ? parts.join('') : null;
}

export function formatAddress(s: Partial<Snapshot>): string | null {
  const line1 = s.address?.trim();
  const cityState = [s.city?.trim(), [s.state?.trim(), s.zip?.trim()].filter(Boolean).join(' ')]
    .filter(Boolean)
    .join(', ');
  const full = [line1, cityState].filter(Boolean).join(', ');
  return full || null;
}

/**
 * Deterministic thread title.
 * `$725K - 4,670ft - 4b2b - 400 Cedar Avenue S, Renton, WA 98057`
 * Closed houses get a leading `❌ `. Result always fits Discord's 100-char cap.
 */
export function buildThreadTitle(
  snapshot: Partial<Snapshot>,
  opts: { closed: boolean; fallback?: string } = { closed: false },
): string {
  const segments = [
    formatPriceShort(snapshot.price),
    formatSqft(snapshot.sqft),
    formatBeds(snapshot.beds, snapshot.baths),
    formatAddress(snapshot),
  ].filter((x): x is string => Boolean(x));

  let body = segments.join(' - ');
  if (!body) body = opts.fallback?.trim() || 'house';

  const prefix = opts.closed ? CLOSED_PREFIX : '';
  const budget = THREAD_NAME_MAX - prefix.length;
  if (body.length > budget) {
    // Trim from the tail (the address) and mark the truncation.
    body = `${body.slice(0, Math.max(1, budget - 1)).trimEnd()}…`;
  }
  return `${prefix}${body}`;
}

const STATUS_LABEL: Record<ListingStatus, string> = {
  active: 'Active',
  pending: 'Pending',
  contingent: 'Contingent',
  closed: 'Closed / Sold',
  unknown: 'Unknown',
};

export function statusLabel(s: ListingStatus): string {
  return STATUS_LABEL[s] ?? 'Unknown';
}

function fieldValue(field: string, s: Partial<Snapshot>): string | null {
  switch (field) {
    case 'price':
      return formatPrice(s.price);
    case 'sqft':
      return formatSqft(s.sqft);
    case 'beds':
      return s.beds === undefined ? null : trimNum(s.beds);
    case 'baths':
      return s.baths === undefined ? null : trimNum(s.baths);
    case 'status':
      return s.status ? statusLabel(s.status) : null;
    case 'address':
      return formatAddress(s);
    case 'yearBuilt':
      return s.yearBuilt === undefined ? null : String(s.yearBuilt);
    case 'hvac':
      return s.hvac ?? null;
    default:
      return null;
  }
}

/** The structured message posted when a property thread is created. */
export function buildSnapshotMessage(s: Snapshot, closed: boolean): string {
  const lines: string[] = [];
  lines.push(`${closed ? CLOSED_PREFIX : ''}**${formatAddress(s) ?? 'Unknown address'}**`);

  const facts: string[] = [];
  const price = formatPrice(s.price);
  if (price) facts.push(`**Price:** ${price}`);
  const bb = formatBeds(s.beds, s.baths);
  if (bb) facts.push(`**Beds/Baths:** ${bb}`);
  const sqft = formatSqft(s.sqft);
  if (sqft) facts.push(`**Size:** ${sqft}`);
  if (s.lotSqft) facts.push(`**Lot:** ${formatSqft(s.lotSqft)}`);
  if (s.yearBuilt) facts.push(`**Built:** ${s.yearBuilt}`);
  facts.push(`**Status:** ${statusLabel(s.status)}${s.statusRaw ? ` (${s.statusRaw})` : ''}`);
  lines.push(facts.join('\n'));

  if (s.hvac) lines.push(`**HVAC (unverified, from listing text):** ${s.hvac}`);

  lines.push(`**Source:** ${s.sourceUrl}`);
  lines.push(`-# ${s.provider} listing \`${s.listingId}\` · snapshot <t:${s.fetchedAt}:R>`);
  return lines.join('\n');
}

/** Concise change notice. Only called when there is at least one change. */
export function buildChangeMessage(
  changes: FieldChange[],
  next: Snapshot,
  closed: boolean,
): string {
  const head = `${closed ? CLOSED_PREFIX : ''}**Update** — ${formatAddress(next) ?? next.sourceUrl}`;
  const body = changes.map((c) => `• **${labelFor(c.field)}:** ${c.from ?? '—'} → ${c.to ?? '—'}`);
  return [head, ...body, `-# checked <t:${next.fetchedAt}:R>`].join('\n');
}

const FIELD_LABEL: Record<string, string> = {
  price: 'Price',
  status: 'Status',
  beds: 'Beds',
  baths: 'Baths',
  sqft: 'Size',
  address: 'Address',
  yearBuilt: 'Year built',
  hvac: 'HVAC (unverified)',
};

export function labelFor(field: string): string {
  return FIELD_LABEL[field] ?? field;
}

/** Human-readable rendering of a single field, for diffs and /house status. */
export function renderField(field: string, s: Partial<Snapshot>): string | null {
  return fieldValue(field, s);
}
