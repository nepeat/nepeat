import type { CommuteValue } from '../enrichment/commute';
import { formatCommute } from '../enrichment/commute';
import { formatHvac, type HvacClassification } from '../enrichment/hvac';
import { formatIsp, type IspOffer } from '../enrichment/isp';
import { formatTransit } from '../enrichment/transit';
import { formatAddress, formatBeds, formatPrice, formatSqft, statusLabel } from '../listing/format';
import type { ListingStatus, Snapshot } from '../listing/types';

import type { APIEmbed } from './types';

/** Discord caps: 25 fields, 1024 chars per field value, 4096 per description. */
const FIELD_VALUE_MAX = 1024;

export type Embed = APIEmbed;

const COLOR_ACTIVE = 0x2ecc71;
const COLOR_PENDING = 0xf1c40f;
const COLOR_CLOSED = 0xe74c3c;
const COLOR_INFO = 0x5865f2;

export function colorFor(status: ListingStatus, closed: boolean): number {
  if (closed || status === 'closed') return COLOR_CLOSED;
  if (status === 'pending' || status === 'contingent') return COLOR_PENDING;
  if (status === 'active') return COLOR_ACTIVE;
  return COLOR_INFO;
}

function field(name: string, value: string, inline = false) {
  return { name, value: value.slice(0, FIELD_VALUE_MAX), inline };
}

export interface HouseEmbedInput {
  snapshot: Snapshot;
  closed: boolean;
  commute?: CommuteValue | null;
  commuteProvenance?: string | null;
  hvac?: HvacClassification | null;
  isp?: IspOffer[] | null;
  ispProvenance?: string | null;
}

/**
 * THE house embed: listing facts and every enrichment in one card.
 *
 * There used to be two messages — a plain-text snapshot and a separate
 * enrichment embed — which split one house across two disaggregated blobs. This
 * is the single view, used as the thread's starter message and re-posted
 * whenever something material changes.
 */
export function buildHouseEmbed(input: HouseEmbedInput): Embed {
  const s = input.snapshot;
  const fields: Embed['fields'] = [];

  // Inline trio: the numbers you scan first.
  const price = formatPrice(s.price);
  if (price) fields.push(field('Price', price, true));
  const bb = formatBeds(s.beds, s.baths);
  if (bb) fields.push(field('Beds/Baths', bb, true));
  const sqft = formatSqft(s.sqft);
  if (sqft) fields.push(field('Size', sqft, true));

  const detail: string[] = [`**Status:** ${statusLabel(s.status)}`];
  if (s.yearBuilt) detail.push(`**Built:** ${s.yearBuilt}`);
  if (s.lotSqft) detail.push(`**Lot:** ${formatSqft(s.lotSqft)}`);
  fields.push(field('Details', detail.join(' · ')));

  if (input.commute?.drive?.length) {
    fields.push(field('🚗 Driving', formatCommute(input.commute.drive)));
  }
  for (const itinerary of input.commute?.transit ?? []) {
    fields.push(field(`🚆 Transit — ${itinerary.label}`, formatTransit(itinerary)));
  }
  if (input.hvac) fields.push(field('🔥 Heating', formatHvac(input.hvac)));
  if (input.isp?.length) fields.push(field('🌐 Internet', formatIsp(input.isp)));

  const footer = [input.commuteProvenance, input.ispProvenance]
    .filter((x): x is string => Boolean(x))
    .join(' · ');

  return {
    title: `${input.closed ? '❌ ' : ''}${formatAddress(s) ?? s.listingKey}`,
    url: s.sourceUrl,
    color: colorFor(s.status, input.closed),
    fields,
    // Hotlinked provider preview image, same one a link unfurl would show.
    ...(s.photoUrl ? { image: { url: s.photoUrl } } : {}),
    ...(footer ? { footer: { text: footer.slice(0, 2048) } } : {}),
  };
}

export interface StatusEmbedInput {
  title: string;
  sourceUrl: string;
  listingKey: string;
  status: ListingStatus;
  forceClosed: boolean;
  lastCheckedAt: number | null;
  lastChangedAt: number | null;
  failCount: number;
  lastError: string | null;
  snapshot: Snapshot | null;
  commute?: CommuteValue | null;
  commuteProvenance?: string | null;
  commuteStatus?: string | null;
  hvac?: HvacClassification | null;
  isp?: IspOffer[] | null;
  ispProvenance?: string | null;
}

/** `/house status` — everything we know, without fetching anything. */
export function buildStatusEmbed(i: StatusEmbedInput): Embed {
  const closed = i.forceClosed || i.status === 'closed';
  const fields: Embed['fields'] = [];

  fields.push(
    field(
      'Status',
      `${statusLabel(i.status)}${i.forceClosed ? ' _(force-closed)_' : ''}`,
      true,
    ),
  );
  if (i.snapshot?.price) {
    fields.push(field('Price', `$${Math.round(i.snapshot.price).toLocaleString('en-US')}`, true));
  }
  if (i.snapshot?.sqft) {
    fields.push(field('Size', `${Math.round(i.snapshot.sqft).toLocaleString('en-US')} ft²`, true));
  }

  if (i.commute?.drive?.length) {
    fields.push(field('🚗 Driving', formatCommute(i.commute.drive)));
  }
  for (const itinerary of i.commute?.transit ?? []) {
    fields.push(field(`🚆 Transit — ${itinerary.label}`, formatTransit(itinerary)));
  }
  // Say *why* it is missing rather than silently omitting the section.
  if (!i.commute?.drive?.length && !i.commute?.transit?.length) {
    fields.push(
      field(
        '🚗 Commute',
        i.commuteStatus === 'unavailable' && i.commuteProvenance
          ? `_not available — ${i.commuteProvenance}_`
          : '_not computed yet — run `/house update`_',
      ),
    );
  }

  if (i.hvac) fields.push(field('🔥 Heating', formatHvac(i.hvac)));
  if (i.isp?.length) fields.push(field('🌐 Internet', formatIsp(i.isp)));

  const times = [
    i.lastCheckedAt ? `Checked <t:${i.lastCheckedAt}:R>` : 'Never checked',
    i.lastChangedAt ? `Changed <t:${i.lastChangedAt}:R>` : 'No changes recorded',
  ];
  if (i.failCount > 0) {
    times.push(`⚠️ ${i.failCount} consecutive failure(s): ${i.lastError ?? 'unknown'}`);
  }
  fields.push(field('History', times.join('\n')));

  return {
    title: `${closed ? '❌ ' : ''}${i.title}`,
    url: i.sourceUrl,
    color: colorFor(i.status, closed),
    fields,
    // Thumbnail rather than a full image: status is a dense readout, not a
    // listing card, and a hero shot would push the fields off-screen.
    ...(i.snapshot?.photoUrl ? { thumbnail: { url: i.snapshot.photoUrl } } : {}),
    footer: { text: i.listingKey },
  };
}
