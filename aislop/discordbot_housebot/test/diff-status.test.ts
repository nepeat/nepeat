import { describe, expect, it } from 'vitest';
import { computeChanges, hasMaterialChange } from '../src/listing/diff';
import { applyForceClose, evaluateOpen, isClosed } from '../src/listing/status';
import type { Snapshot } from '../src/listing/types';

const s = (over: Partial<Snapshot> = {}): Snapshot => ({
  provider: 'zillow',
  listingId: '1',
  listingKey: 'zillow:1',
  sourceUrl: 'https://www.zillow.com/homedetails/1_zpid/',
  status: 'active',
  price: 700000,
  beds: 4,
  baths: 2,
  sqft: 2000,
  address: '1 Main St',
  city: 'Renton',
  state: 'WA',
  zip: '98057',
  fetchedAt: 1,
  ...over,
});

describe('computeChanges', () => {
  it('returns nothing for the first observation', () => {
    expect(computeChanges(null, s())).toEqual([]);
  });

  it('detects a price change with rendered values', () => {
    const changes = computeChanges(s(), s({ price: 675000 }));
    expect(changes).toEqual([{ field: 'price', from: '$700,000', to: '$675,000' }]);
  });

  it('detects a status transition to closed', () => {
    const changes = computeChanges(s(), s({ status: 'closed' }));
    expect(changes).toEqual([{ field: 'status', from: 'Active', to: 'Closed / Sold' }]);
  });

  it('ignores non-material fields', () => {
    expect(computeChanges(s(), s({ fetchedAt: 999, statusRaw: 'FOR_SALE' }))).toEqual([]);
  });

  it('ignores data that disappeared from the page', () => {
    const next = s();
    delete (next as Partial<Snapshot>).price;
    expect(computeChanges(s(), next)).toEqual([]);
  });

  it('reports multiple changes at once', () => {
    const changes = computeChanges(s(), s({ price: 650000, status: 'pending' }));
    expect(changes.map((c) => c.field).sort()).toEqual(['price', 'status']);
    expect(hasMaterialChange(s(), s({ price: 650000 }))).toBe(true);
    expect(hasMaterialChange(s(), s())).toBe(false);
  });
});

describe('closure state', () => {
  it('is closed when the listing is closed or an operator forced it', () => {
    expect(isClosed({ forceClosed: false, listingStatus: 'active' })).toBe(false);
    expect(isClosed({ forceClosed: false, listingStatus: 'closed' })).toBe(true);
    expect(isClosed({ forceClosed: true, listingStatus: 'active' })).toBe(true);
    expect(isClosed({ forceClosed: false, listingStatus: 'pending' })).toBe(false);
  });

  it('force close needs no fetch', () => {
    expect(applyForceClose()).toEqual({ forceClosed: true });
  });
});

describe('evaluateOpen', () => {
  it('opens only when the listing supports it', () => {
    expect(evaluateOpen('active')).toEqual({ ok: true, nextForceClosed: false });
    expect(evaluateOpen('pending')).toEqual({ ok: true, nextForceClosed: false });
    expect(evaluateOpen('contingent')).toEqual({ ok: true, nextForceClosed: false });
  });

  it('refuses to contradict a sold listing', () => {
    expect(evaluateOpen('closed')).toEqual({ ok: false, reason: 'listing-closed' });
  });

  it('refuses to guess on unknown status', () => {
    expect(evaluateOpen('unknown')).toEqual({ ok: false, reason: 'unknown-status' });
  });
});
