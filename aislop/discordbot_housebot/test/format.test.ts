import { describe, expect, it } from 'vitest';
import {
  buildChangeMessage,
  buildSnapshotMessage,
  buildThreadTitle,
  formatBeds,
  formatPrice,
  formatPriceShort,
  formatSqft,
  THREAD_NAME_MAX,
} from '../src/listing/format';
import type { Snapshot } from '../src/listing/types';

const base: Snapshot = {
  provider: 'zillow',
  listingId: '49059541',
  listingKey: 'zillow:49059541',
  sourceUrl: 'https://www.zillow.com/homedetails/49059541_zpid/',
  status: 'active',
  price: 725000,
  address: '400 Cedar Avenue S',
  city: 'Renton',
  state: 'WA',
  zip: '98057',
  beds: 4,
  baths: 2,
  sqft: 4670,
  fetchedAt: 1_700_000_000,
};

describe('scalar formatters', () => {
  it('formats price, sqft, beds/baths', () => {
    expect(formatPrice(725000)).toBe('$725,000');
    expect(formatPrice(0)).toBeNull();
    expect(formatPrice(undefined)).toBeNull();
    expect(formatSqft(4670)).toBe('4,670ft');
    expect(formatBeds(4, 2)).toBe('4b2b');
    expect(formatBeds(3, 2.5)).toBe('3b2.5b');
    expect(formatBeds(undefined, undefined)).toBeNull();
  });
});

describe('formatPriceShort', () => {
  it('abbreviates thousands, trimming trailing zeros', () => {
    expect(formatPriceShort(725000)).toBe('$725K');
    expect(formatPriceShort(749900)).toBe('$749.9K');
    expect(formatPriceShort(1000)).toBe('$1K');
    expect(formatPriceShort(999000)).toBe('$999K');
  });

  it('abbreviates millions', () => {
    expect(formatPriceShort(1000000)).toBe('$1M');
    expect(formatPriceShort(1250000)).toBe('$1.25M');
    expect(formatPriceShort(1495000)).toBe('$1.5M');
    expect(formatPriceShort(12750000)).toBe('$12.75M');
  });

  it('leaves sub-thousand prices alone and rejects junk', () => {
    expect(formatPriceShort(950)).toBe('$950');
    expect(formatPriceShort(0)).toBeNull();
    expect(formatPriceShort(undefined)).toBeNull();
  });

  it('does not change the exact formatter used everywhere else', () => {
    expect(formatPrice(749900)).toBe('$749,900');
  });
});

describe('buildThreadTitle', () => {
  it('matches the canonical format with an abbreviated price', () => {
    expect(buildThreadTitle(base, { closed: false })).toBe(
      '$725K - 4,670ft - 4b2b - 400 Cedar Avenue S, Renton, WA 98057',
    );
  });

  it('keeps one decimal when the price is not a round thousand', () => {
    expect(buildThreadTitle({ ...base, price: 749900 }, { closed: false })).toBe(
      '$749.9K - 4,670ft - 4b2b - 400 Cedar Avenue S, Renton, WA 98057',
    );
  });

  it('prefixes closed houses', () => {
    expect(buildThreadTitle(base, { closed: true })).toBe(
      '❌ $725K - 4,670ft - 4b2b - 400 Cedar Avenue S, Renton, WA 98057',
    );
  });

  it('omits missing segments instead of leaving holes', () => {
    expect(buildThreadTitle({ price: 500000, address: 'Somewhere' }, { closed: false })).toBe(
      '$500K - Somewhere',
    );
  });

  it('is deterministic', () => {
    expect(buildThreadTitle(base, { closed: false })).toBe(buildThreadTitle(base, { closed: false }));
  });

  it('never exceeds the Discord thread-name cap, even when closed', () => {
    const long: Partial<Snapshot> = {
      ...base,
      address: 'A'.repeat(200),
    };
    const open = buildThreadTitle(long, { closed: false });
    const closed = buildThreadTitle(long, { closed: true });
    expect(open.length).toBeLessThanOrEqual(THREAD_NAME_MAX);
    expect(closed.length).toBeLessThanOrEqual(THREAD_NAME_MAX);
    expect(closed.startsWith('❌ ')).toBe(true);
    expect(open.endsWith('…')).toBe(true);
  });

  it('falls back when the snapshot is empty', () => {
    expect(buildThreadTitle({}, { closed: false, fallback: 'zillow:1' })).toBe('zillow:1');
    expect(buildThreadTitle({}, { closed: false })).toBe('house');
  });
});

describe('messages', () => {
  it('renders an initial snapshot message with the EXACT price, not the short one', () => {
    const msg = buildSnapshotMessage(base, false);
    expect(msg).toContain('**Price:** $725,000');
    expect(msg).not.toContain('$725K');
    expect(msg).toContain('**Beds/Baths:** 4b2b');
    expect(msg).toContain('**Status:** Active');
    expect(msg).toContain(base.sourceUrl);
    expect(msg).not.toContain('❌');
  });

  it('marks hvac as unverified', () => {
    const msg = buildSnapshotMessage({ ...base, hvac: 'Forced air' }, false);
    expect(msg).toContain('HVAC (unverified, from listing text):** Forced air');
  });

  it('prefixes closed snapshots and change notices', () => {
    expect(buildSnapshotMessage({ ...base, status: 'closed' }, true).startsWith('❌ ')).toBe(true);
    const notice = buildChangeMessage(
      [{ field: 'status', from: 'Active', to: 'Closed / Sold' }],
      { ...base, status: 'closed' },
      true,
    );
    expect(notice.startsWith('❌ ')).toBe(true);
    expect(notice).toContain('**Status:** Active → Closed / Sold');
  });

  it('change notices keep exact prices', () => {
    const notice = buildChangeMessage(
      [{ field: 'price', from: '$725,000', to: '$699,000' }],
      base,
      false,
    );
    expect(notice).toContain('$725,000 → $699,000');
  });
});
