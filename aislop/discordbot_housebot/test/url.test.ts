import { describe, expect, it } from 'vitest';
import { extractListingUrl, identifyUrl, parseUrl } from '../src/listing/url';

describe('identifyUrl', () => {
  it('normalizes a full zillow homedetails url', () => {
    const r = identifyUrl(
      'https://www.zillow.com/homedetails/400-Cedar-Ave-S-Renton-WA-98057/49059541_zpid/?utm_source=x',
    );
    expect(r).toEqual({
      provider: 'zillow',
      listingId: '49059541',
      listingKey: 'zillow:49059541',
      canonicalUrl:
        'https://www.zillow.com/homedetails/400-Cedar-Ave-S-Renton-WA-98057/49059541_zpid/',
    });
  });

  it('handles a bare zpid zillow url and mobile host', () => {
    const r = identifyUrl('https://m.zillow.com/homedetails/49059541_zpid/');
    expect(r?.canonicalUrl).toBe('https://www.zillow.com/homedetails/49059541_zpid/');
    expect(r?.listingKey).toBe('zillow:49059541');
  });

  it('normalizes a redfin url and drops the query', () => {
    const r = identifyUrl('https://www.redfin.com/WA/Bellevue/1234-NE-8th-St-98004/home/173510?a=1');
    expect(r).toEqual({
      provider: 'redfin',
      listingId: '173510',
      listingKey: 'redfin:173510',
      canonicalUrl: 'https://www.redfin.com/WA/Bellevue/1234-NE-8th-St-98004/home/173510',
    });
  });

  it('accepts scheme-less and punctuation-wrapped input', () => {
    expect(identifyUrl('<www.zillow.com/homedetails/1_zpid/>')?.listingId).toBe('1');
    expect(identifyUrl('https://www.redfin.com/WA/X/y/home/42.')?.listingId).toBe('42');
  });

  it('rejects unsupported hosts and non-listing paths', () => {
    expect(identifyUrl('https://www.realtor.com/realestateandhomes-detail/x')).toBeNull();
    expect(identifyUrl('https://www.zillow.com/renton-wa/')).toBeNull();
    expect(identifyUrl('https://www.redfin.com/city/16163/WA/Renton')).toBeNull();
    expect(identifyUrl('not a url at all')).toBeNull();
  });

  it('does not treat a lookalike host as zillow', () => {
    expect(identifyUrl('https://zillow.com.evil.example/homedetails/1_zpid/')).toBeNull();
  });
});

describe('parseUrl', () => {
  it('returns null for garbage', () => {
    expect(parseUrl('http://')).toBeNull();
  });
});

describe('extractListingUrl', () => {
  it('finds the first supported url inside free text', () => {
    const text = 'look at https://example.com/x and https://www.redfin.com/WA/A/b/home/7 pls';
    expect(extractListingUrl(text)?.listingKey).toBe('redfin:7');
  });

  it('returns null when nothing matches', () => {
    expect(extractListingUrl('no links here')).toBeNull();
  });
});
