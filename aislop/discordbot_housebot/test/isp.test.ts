import { describe, expect, it, vi } from 'vitest';
import { dedupeOffers, formatIsp, lookupIsp, techLabel } from '../src/enrichment/isp';

// Verbatim from the live FCC response for the Renton test house (2026-08-03).
const FORM477_ROWS = [
  { providername: 'ViaSat, Inc.', techcode: '60', maxaddown: '35', maxadup: '3' },
  { providername: 'COMCAST CABLE COMMUNICATIONS, LLC', techcode: '43', maxaddown: '1000', maxadup: '35' },
  { providername: 'CenturyLink, Inc.', techcode: '11', maxaddown: '15', maxadup: '0.75' },
  { providername: 'T-Mobile USA, Inc.', techcode: '70', maxaddown: '25', maxadup: '3' },
];

function fcc(rows: unknown, blockOk = true): typeof fetch {
  return vi.fn(async (url: unknown) => {
    if (String(url).includes('block/find')) {
      return blockOk
        ? new Response(JSON.stringify({ Block: { FIPS: '530330257012004' } }), { status: 200 })
        : new Response('nope', { status: 500 });
    }
    return new Response(JSON.stringify(rows), { status: 200 });
  }) as unknown as typeof fetch;
}

describe('techLabel', () => {
  it('maps FCC tech codes to human words', () => {
    expect(techLabel('50')).toBe('fiber');
    expect(techLabel('43')).toBe('cable');
    expect(techLabel('11')).toBe('DSL');
    expect(techLabel('60')).toBe('satellite');
    expect(techLabel('70')).toBe('fixed wireless');
    expect(techLabel('999')).toBe('other');
    expect(techLabel(undefined)).toBe('other');
  });
});

describe('dedupeOffers', () => {
  it('keeps the fastest row per provider, fastest first', () => {
    const offers = dedupeOffers([
      ...FORM477_ROWS,
      { providername: 'CenturyLink, Inc.', techcode: '50', maxaddown: '940', maxadup: '940' },
    ]);
    expect(offers[0]!.provider).toBe('COMCAST CABLE COMMUNICATIONS, LLC');
    expect(offers.filter((o) => o.provider.startsWith('CenturyLink'))).toHaveLength(1);
    expect(offers.find((o) => o.provider.startsWith('CenturyLink'))).toMatchObject({
      technology: 'fiber',
      downMbps: 940,
      symmetrical: true,
    });
  });

  it('flags symmetrical only when BOTH directions clear 900', () => {
    expect(dedupeOffers(FORM477_ROWS).every((o) => !o.symmetrical)).toBe(true);
    expect(
      dedupeOffers([{ providername: 'X', techcode: '50', maxaddown: '1000', maxadup: '35' }])[0]!
        .symmetrical,
    ).toBe(false);
  });

  it('skips rows with no provider or unusable speeds', () => {
    expect(
      dedupeOffers([
        { providername: '', techcode: '50', maxaddown: '1000', maxadup: '1000' },
        { providername: 'Y', techcode: '50', maxaddown: 'n/a' },
      ]),
    ).toEqual([]);
  });
});

describe('formatIsp', () => {
  it('leads with a warning when nothing symmetrical is on record', () => {
    const out = formatIsp(dedupeOffers(FORM477_ROWS));
    expect(out).toContain('⚠️');
    expect(out).toContain('no symmetrical service on record');
    expect(out).toContain('COMCAST CABLE COMMUNICATIONS, LLC (cable) — 1 Gbps↓ / 35 Mbps↑');
  });

  it('leads with a tick when symmetrical exists', () => {
    const out = formatIsp(
      dedupeOffers([{ providername: 'Lumen', techcode: '50', maxaddown: '940', maxadup: '940' }]),
    );
    expect(out).toContain('✅');
    expect(out).toContain('symmetrical available');
  });
});

describe('lookupIsp', () => {
  it('resolves the 2010 census block, then queries Form 477', async () => {
    const urls: string[] = [];
    const fetchImpl = vi.fn(async (url: unknown) => {
      urls.push(String(url));
      return String(url).includes('block/find')
        ? new Response(JSON.stringify({ Block: { FIPS: '530330257012004' } }), { status: 200 })
        : new Response(JSON.stringify(FORM477_ROWS), { status: 200 });
    }) as unknown as typeof fetch;

    const r = await lookupIsp(47.4779, -122.20163, { fetchImpl });
    // Form 477 is keyed on 2010 blocks; a 2020 block silently returns nothing.
    expect(urls[0]).toContain('censusYear=2010');
    expect(urls[1]).toContain('530330257012004');
    expect(urls[1]).toContain("consumer%3D'1'");

    expect(r.status).toBe('unverified');
    if (r.status === 'unverified') expect(r.value).toHaveLength(4);
    expect(r.provenance).toContain('Dec 2020');
    expect(r.provenance).toContain('block-level');
  });

  it('needs coordinates', async () => {
    const fetchImpl = vi.fn() as unknown as typeof fetch;
    const r = await lookupIsp(null, null, { fetchImpl });
    expect(r.status).toBe('unavailable');
    expect(fetchImpl).not.toHaveBeenCalled();
  });

  it('reports unavailable (never throws) when the FCC is down', async () => {
    expect((await lookupIsp(47, -122, { fetchImpl: fcc(FORM477_ROWS, false) })).status).toBe(
      'unavailable',
    );
    const thrower = vi.fn(async () => {
      throw new Error('dns');
    }) as unknown as typeof fetch;
    expect((await lookupIsp(47, -122, { fetchImpl: thrower })).status).toBe('unavailable');
  });

  it('reports unavailable when the block has no rows', async () => {
    const r = await lookupIsp(47, -122, { fetchImpl: fcc([]) });
    expect(r.status).toBe('unavailable');
    expect(r.provenance).toContain('no FCC Form 477 rows');
  });
});
