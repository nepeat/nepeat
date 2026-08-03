import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { describe, expect, it } from 'vitest';
import { normalizeStatus, splitAddress } from '../src/listing/parse';
import { redfinSource } from '../src/listing/providers/redfin';
import { zillowSource } from '../src/listing/providers/zillow';

function fixture(name: string): string {
  return readFileSync(fileURLToPath(new URL(`./fixtures/${name}`, import.meta.url)), 'utf8');
}

const ZURL = 'https://www.zillow.com/homedetails/400-Cedar-Ave-S-Renton-WA-98057/49059541_zpid/';
const RURL = 'https://www.redfin.com/WA/Bellevue/1234-NE-8th-St-98004/home/173510';

describe('zillow parser', () => {
  it('parses an active listing fixture', () => {
    const snap = zillowSource.parse(fixture('zillow-active.html'), ZURL, '49059541', 1000);
    expect(snap).not.toBeNull();
    expect(snap).toMatchObject({
      provider: 'zillow',
      listingId: '49059541',
      listingKey: 'zillow:49059541',
      sourceUrl: ZURL,
      status: 'active',
      price: 725000,
      address: '400 Cedar Avenue S',
      city: 'Renton',
      state: 'WA',
      zip: '98057',
      beds: 4,
      baths: 2,
      sqft: 4670,
      yearBuilt: 1998,
      fetchedAt: 1000,
    });
    expect(snap?.hvac).toBe('Forced air, Natural gas');
  });

  it('parses a sold listing as closed', () => {
    const snap = zillowSource.parse(fixture('zillow-sold.html'), ZURL, '49059541', 1000);
    expect(snap?.status).toBe('closed');
    expect(snap?.price).toBe(699000);
  });

  it('returns null on a page with no listing data', () => {
    expect(zillowSource.parse(fixture('blocked.html'), ZURL, '49059541', 1000)).toBeNull();
  });

  it('claims only zillow urls', () => {
    expect(zillowSource.matches(new URL(ZURL))).toBe(true);
    expect(zillowSource.matches(new URL(RURL))).toBe(false);
    expect(zillowSource.identify(new URL(RURL))).toBeNull();
    expect(zillowSource.identify(new URL(ZURL))?.listingId).toBe('49059541');
  });
});

describe('redfin parser', () => {
  it('parses an active listing across split ld+json nodes', () => {
    const snap = redfinSource.parse(fixture('redfin-active.html'), RURL, '173510', 2000);
    expect(snap).toMatchObject({
      provider: 'redfin',
      listingKey: 'redfin:173510',
      status: 'active',
      price: 1495000,
      address: '1234 NE 8th St',
      city: 'Bellevue',
      state: 'WA',
      zip: '98004',
      beds: 3,
      baths: 2.5,
      sqft: 2100,
      yearBuilt: 1975,
    });
    expect(snap?.hvac).toBe('Heat pump');
  });

  it('claims only redfin urls', () => {
    expect(redfinSource.matches(new URL(RURL))).toBe(true);
    expect(redfinSource.matches(new URL(ZURL))).toBe(false);
  });
});

describe('status normalization', () => {
  it('maps provider vocabularies onto the lifecycle enum', () => {
    expect(normalizeStatus('http://schema.org/InStock')).toBe('active');
    expect(normalizeStatus('FOR_SALE')).toBe('active');
    expect(normalizeStatus('RECENTLY_SOLD')).toBe('closed');
    expect(normalizeStatus('SOLD')).toBe('closed');
    expect(normalizeStatus('http://schema.org/Discontinued')).toBe('closed');
    expect(normalizeStatus('PENDING')).toBe('pending');
    expect(normalizeStatus('Contingent')).toBe('contingent');
    expect(normalizeStatus('gibberish')).toBe('unknown');
    expect(normalizeStatus(undefined)).toBe('unknown');
  });
});

describe('splitAddress', () => {
  it('splits a full single-line address', () => {
    expect(splitAddress('400 Cedar Avenue S, Renton, WA 98057')).toEqual({
      address: '400 Cedar Avenue S',
      city: 'Renton',
      state: 'WA',
      zip: '98057',
    });
  });

  it('falls back to a single line when it cannot split', () => {
    expect(splitAddress('somewhere odd')).toEqual({ address: 'somewhere odd' });
  });
});
