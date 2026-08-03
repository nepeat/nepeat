import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { describe, expect, it } from 'vitest';
import { hvacFromSection, normalizeStatus, splitAddress } from '../src/listing/parse';
import { buildThreadTitle } from '../src/listing/format';
import { factsFromMetaDescription, photoFromMeta } from '../src/listing/providers/common';
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

describe('zillow live markup (captured 2026-08-03, zpid 49024254)', () => {
  const LIVE = 'https://www.zillow.com/homedetails/400-Cedar-Ave-S-Renton-WA-98057/49024254_zpid/';

  it('reads facts nested under offers.itemOffered plus the meta description', () => {
    const snap = zillowSource.parse(fixture('zillow-live-2026.html'), LIVE, '49024254', 500);
    expect(snap).toMatchObject({
      listingKey: 'zillow:49024254',
      status: 'active',
      price: 725000,
      address: '400 Cedar Avenue S',
      city: 'Renton',
      state: 'WA',
      zip: '98057',
      beds: 4,
      baths: 2,
      sqft: 4670,
      yearBuilt: 1908,
      lat: 47.4779,
      lon: -122.20163,
    });
  });

  it('produces the canonical thread title from live markup', () => {
    const snap = zillowSource.parse(fixture('zillow-live-2026.html'), LIVE, '49024254', 500);
    expect(buildThreadTitle(snap!, { closed: false })).toBe(
      '$725K - 4,670ft - 4b2b - 400 Cedar Avenue S, Renton, WA 98057',
    );
  });

  it('extracts heating from the rendered section, not from a JSON key', () => {
    const snap = zillowSource.parse(fixture('zillow-live-2026.html'), LIVE, '49024254', 500);
    expect(snap?.hvac).toBe('Fireplace, Heat Pump, Natural Gas');
  });
});

describe('factsFromMetaDescription', () => {
  it('parses beds/baths/sqft/year/price out of the description', () => {
    const html =
      '<meta name="description" content="Zillow has 36 photos of this $1,250,000 3 beds, 2.5 baths, 2,100 sqft single family home located at 1 A St built in 1975." />';
    expect(factsFromMetaDescription(html)).toEqual({
      price: 1250000,
      beds: 3,
      baths: 2.5,
      sqft: 2100,
      yearBuilt: 1975,
    });
  });

  it('returns nothing when there is no description', () => {
    expect(factsFromMetaDescription('<html></html>')).toEqual({});
  });
});

describe('hvacFromSection', () => {
  it('stops at the next facts label', () => {
    const html = '<h6>Heating</h6><ul><li>Heat Pump, Natural Gas</li></ul><h6>Cooling</h6><ul><li>Window Unit(s)</li></ul>';
    expect(hvacFromSection(html)).toBe('Heat Pump, Natural Gas');
  });

  it('returns undefined when there is no heating section', () => {
    expect(hvacFromSection('<h6>Cooling</h6><ul><li>Central</li></ul>')).toBeUndefined();
  });
});

describe('listing photo', () => {
  it('takes og:image from the live zillow fixture', () => {
    const snap = zillowSource.parse(
      fixture('zillow-live-2026.html'),
      'https://www.zillow.com/homedetails/x/49024254_zpid/',
      '49024254',
      1,
    );
    expect(snap?.photoUrl).toMatch(/^https:\/\/photos\.zillowstatic\.com\//);
  });

  it('ignores non-https and missing images', () => {
    expect(photoFromMeta('<meta property="og:image" content="http://insecure/x.jpg" />')).toEqual({});
    expect(photoFromMeta('<html></html>')).toEqual({});
    expect(
      photoFromMeta('<meta name="twitter:image" content="https://ok/x.jpg" />'),
    ).toEqual({ photoUrl: 'https://ok/x.jpg' });
  });
});
