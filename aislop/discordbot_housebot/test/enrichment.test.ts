import { describe, expect, it, vi } from 'vitest';
import {
  estimateCommutes,
  formatCommute,
  nextTuesday8am,
  parseDuration,
} from '../src/enrichment/commute';
import { classifyHvac, formatHvac } from '../src/enrichment/hvac';

describe('classifyHvac', () => {
  it('classifies the real listing text from the test house', () => {
    const c = classifyHvac('Fireplace, Heat Pump, Natural Gas')!;
    expect(c.kinds).toContain('heat-pump');
    expect(c.kinds).toContain('forced-air-gas');
    expect(c.disliked).toEqual([]);
  });

  it('flags oil', () => {
    const c = classifyHvac('Oil furnace')!;
    expect(c.kinds).toEqual(['oil']);
    expect(c.disliked).toEqual(['oil']);
    expect(formatHvac(c)).toContain('⚠️');
    expect(formatHvac(c)).toContain('oil');
  });

  it('flags steam radiators', () => {
    const c = classifyHvac('Steam radiators, boiler')!;
    expect(c.disliked).toEqual(['radiator-steam']);
  });

  it('does NOT confuse radiant floor heat with radiators', () => {
    const c = classifyHvac('Radiant floor heat')!;
    expect(c.kinds).toEqual(['radiant-floor']);
    expect(c.disliked).toEqual([]);
    expect(formatHvac(c)).not.toContain('⚠️');
  });

  it('flags a mixed system that includes a disliked one', () => {
    const c = classifyHvac('Heat pump upstairs, oil furnace in basement')!;
    expect(c.kinds).toContain('heat-pump');
    expect(c.kinds).toContain('oil');
    expect(c.disliked).toEqual(['oil']);
  });

  it('does not fire on "oil rubbed bronze" fixtures', () => {
    const c = classifyHvac('Oil rubbed bronze fixtures, forced air electric')!;
    expect(c.disliked).toEqual([]);
  });

  it('handles electric baseboard and none', () => {
    expect(classifyHvac('Baseboard')!.kinds).toEqual(['baseboard-electric']);
    expect(classifyHvac('None')!.kinds).toEqual(['none']);
  });

  it('returns null for missing text and unknown for unrecognized', () => {
    expect(classifyHvac(undefined)).toBeNull();
    expect(classifyHvac('   ')).toBeNull();
    expect(classifyHvac('sunshine and hope')!.kinds).toEqual(['unknown']);
  });

  it('always marks the value unverified and quotes the source text', () => {
    expect(formatHvac(classifyHvac('Heat Pump')!)).toContain('unverified');
    expect(formatHvac(classifyHvac('Heat Pump')!)).toContain('"Heat Pump"');
  });
});

describe('parseDuration', () => {
  it('parses the protobuf duration strings Routes returns', () => {
    expect(parseDuration('1115s')).toBe(1115);
    expect(parseDuration('1115.4s')).toBe(1115);
    expect(parseDuration('nope')).toBeUndefined();
    expect(parseDuration(undefined)).toBeUndefined();
  });
});

describe('nextTuesday8am', () => {
  it('always lands on a future Tuesday at 08:00 Pacific', () => {
    for (const day of ['2026-08-03', '2026-08-04', '2026-08-08', '2026-12-25']) {
      const iso = nextTuesday8am(new Date(`${day}T12:00:00Z`));
      const d = new Date(iso);
      expect(d.getTime()).toBeGreaterThan(new Date(`${day}T12:00:00Z`).getTime());
      const pacific = new Intl.DateTimeFormat('en-US', {
        timeZone: 'America/Los_Angeles',
        weekday: 'short',
        hour: 'numeric',
        hour12: false,
      }).format(d);
      expect(pacific).toContain('Tue');
      expect(pacific).toMatch(/\b0?8\b/);
    }
  });

  it('never returns today even when today is Tuesday', () => {
    const iso = nextTuesday8am(new Date('2026-08-04T12:00:00Z')); // a Tuesday
    expect(new Date(iso).getUTCDate()).toBe(11);
  });
});

describe('estimateCommutes', () => {
  const okBody = {
    routes: [{ duration: '1115s', staticDuration: '1189s', distanceMeters: 18083 }],
  };

  it('is unavailable without a key', async () => {
    const r = await estimateCommutes(47.4779, -122.20163, {});
    expect(r).toMatchObject({ status: 'unavailable' });
    expect(r.provenance).toContain('GOOGLE_MAPS_API_KEY');
  });

  it('is unavailable without coordinates, and makes no call', async () => {
    const fetchImpl = vi.fn() as unknown as typeof fetch;
    const r = await estimateCommutes(null, null, { apiKey: 'k', fetchImpl });
    expect(r).toMatchObject({ status: 'unavailable' });
    expect(r.provenance).toContain('coordinates');
    expect(fetchImpl).not.toHaveBeenCalled();
  });

  it('requests both destinations with the Pro-tier traffic preference', async () => {
    const bodies: Array<Record<string, unknown>> = [];
    const headers: Array<Record<string, string>> = [];
    const fetchImpl = vi.fn(async (_u: unknown, init: RequestInit | undefined) => {
      bodies.push(JSON.parse(String(init?.body)));
      headers.push(init?.headers as Record<string, string>);
      return new Response(JSON.stringify(okBody), { status: 200 });
    }) as unknown as typeof fetch;

    const r = await estimateCommutes(47.4779, -122.20163, {
      apiKey: 'test-key',
      departureIso: '2026-08-11T08:00:00-07:00',
      fetchImpl,
    });

    expect(fetchImpl).toHaveBeenCalledTimes(2);
    expect(bodies[0]).toMatchObject({
      travelMode: 'DRIVE',
      routingPreference: 'TRAFFIC_AWARE_OPTIMAL',
      departureTime: '2026-08-11T08:00:00-07:00',
      origin: { location: { latLng: { latitude: 47.4779, longitude: -122.20163 } } },
    });
    // The key travels in a header, never in the URL or body.
    expect(headers[0]?.['X-Goog-Api-Key']).toBe('test-key');
    expect(JSON.stringify(bodies)).not.toContain('test-key');

    expect(r.status).toBe('ok');
    if (r.status === 'ok') {
      expect(r.value).toHaveLength(2);
      expect(r.value[0]).toMatchObject({
        label: 'Bellevue office (nep)',
        driveSeconds: 1115,
        freeFlowSeconds: 1189,
        distanceMeters: 18083,
        provider: 'google-routes',
      });
    }
    expect(r.provenance).toContain('2026-08-11T08:00:00-07:00');
  });

  it('keeps a partial result when one destination fails', async () => {
    let call = 0;
    const fetchImpl = vi.fn(async () => {
      call += 1;
      return call === 1
        ? new Response(JSON.stringify(okBody), { status: 200 })
        : new Response('nope', { status: 500 });
    }) as unknown as typeof fetch;

    const r = await estimateCommutes(1, 2, { apiKey: 'k', fetchImpl });
    expect(r.status).toBe('ok');
    if (r.status === 'ok') expect(r.value).toHaveLength(1);
    expect(r.provenance).toContain('partial');
  });

  it('reports unavailable (never throws) when everything fails, and redacts keys', async () => {
    const fetchImpl = vi.fn(async () =>
      new Response('bad key AIzaSyREALKEYVALUE123', { status: 403 }),
    ) as unknown as typeof fetch;
    const r = await estimateCommutes(1, 2, { apiKey: 'k', fetchImpl });
    expect(r.status).toBe('unavailable');
    expect(r.provenance).not.toContain('REALKEYVALUE');
    expect(r.provenance).toContain('AIza***');
  });

  it('survives a network throw', async () => {
    const fetchImpl = vi.fn(async () => {
      throw new Error('dns');
    }) as unknown as typeof fetch;
    expect((await estimateCommutes(1, 2, { apiKey: 'k', fetchImpl })).status).toBe('unavailable');
  });
});

describe('formatCommute', () => {
  it('shows traffic time, free-flow comparison and distance', () => {
    const out = formatCommute([
      {
        label: 'Bellevue office (nep)',
        destination: 'x',
        driveSeconds: 1115,
        freeFlowSeconds: 1189,
        distanceMeters: 18083,
        provider: 'google-routes',
      },
    ]);
    expect(out).toContain('Bellevue office (nep): **19 min**');
    expect(out).toContain('20 min free-flow');
    expect(out).toContain('11.2 mi');
  });
});
