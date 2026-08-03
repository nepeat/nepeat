import { describe, expect, it, vi } from 'vitest';
import {
  estimateCommutes,
  formatCommute,
  isCurrentCommuteShape,
  nextTuesdayAt,
  normalizeCommuteValue,
  parseDuration,
} from '../src/enrichment/commute';
import { classifyHvac, formatHvac } from '../src/enrichment/hvac';
import {
  emojiForVehicle,
  formatTransit,
  formatTransitRoute,
  hopsFromSteps,
} from '../src/enrichment/transit';

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

describe('nextTuesdayAt', () => {
  it('always lands on a future Tuesday at the requested Pacific hour', () => {
    for (const day of ['2026-08-03', '2026-08-04', '2026-08-08', '2026-12-25']) {
      const iso = nextTuesdayAt(new Date(`${day}T12:00:00Z`), 10);
      const d = new Date(iso);
      expect(d.getTime()).toBeGreaterThan(new Date(`${day}T12:00:00Z`).getTime());
      const pacific = new Intl.DateTimeFormat('en-US', {
        timeZone: 'America/Los_Angeles',
        weekday: 'short',
        hour: 'numeric',
        hour12: false,
      }).format(d);
      expect(pacific).toContain('Tue');
      expect(pacific).toMatch(/\b10\b/);
    }
  });

  it('never returns today even when today is Tuesday', () => {
    const iso = nextTuesdayAt(new Date('2026-08-04T12:00:00Z'), 10); // a Tuesday
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

    // 3 destinations x 2 travel modes
    expect(fetchImpl).toHaveBeenCalledTimes(6);
    expect(bodies[0]).toMatchObject({
      travelMode: 'DRIVE',
      routingPreference: 'TRAFFIC_AWARE_OPTIMAL',
      departureTime: '2026-08-11T08:00:00-07:00',
      origin: { location: { latLng: { latitude: 47.4779, longitude: -122.20163 } } },
    });
    // routingPreference is DRIVE-only; sending it with TRANSIT is an API error.
    expect(bodies[1]).toMatchObject({ travelMode: 'TRANSIT' });
    expect(bodies[1]).not.toHaveProperty('routingPreference');
    // The key travels in a header, never in the URL or body.
    expect(headers[0]?.['X-Goog-Api-Key']).toBe('test-key');
    expect(JSON.stringify(bodies)).not.toContain('test-key');

    expect(r.status).toBe('ok');
    if (r.status === 'ok') {
      expect(r.value.drive).toHaveLength(3);
      expect(r.value.drive[0]).toMatchObject({
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
    // only the first of six calls succeeds

    const r = await estimateCommutes(1, 2, { apiKey: 'k', fetchImpl });
    expect(r.status).toBe('ok');
    if (r.status === 'ok') expect(r.value.drive).toHaveLength(1);
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

describe('transit itineraries', () => {
  const transitBody = {
    routes: [
      {
        duration: '3900s',
        legs: [
          {
            steps: [
              { travelMode: 'WALK', staticDuration: '360s' },
              {
                travelMode: 'TRANSIT',
                transitDetails: {
                  stopCount: 12,
                  transitLine: { nameShort: '535', name: 'Bellevue', vehicle: { type: 'BUS' } },
                },
              },
              {
                travelMode: 'TRANSIT',
                transitDetails: {
                  transitLine: {
                    nameShort: '1 Line',
                    name: 'Link 1 Line',
                    vehicle: { type: 'LIGHT_RAIL' },
                  },
                },
              },
              { travelMode: 'WALK', staticDuration: '240s' },
            ],
          },
        ],
      },
    ],
  };

  it('collapses steps into boarded vehicles and sums walking', () => {
    const { hops, walkSeconds } = hopsFromSteps(
      transitBody.routes[0]!.legs[0]!.steps,
      parseDuration,
    );
    expect(hops).toEqual([
      { line: '535', vehicle: 'BUS', emoji: '🚌', stopCount: 12 },
      { line: '1 Line', vehicle: 'LIGHT_RAIL', emoji: '🚈' },
    ]);
    expect(hops.every((h) => h.seconds === undefined)).toBe(true);
    expect(walkSeconds).toBe(600);
  });

  it('maps vehicle types to distinct emoji — rail is not light rail', () => {
    expect(emojiForVehicle('BUS')).toBe('🚌');
    expect(emojiForVehicle('LIGHT_RAIL')).toBe('🚈');
    expect(emojiForVehicle('HEAVY_RAIL')).toBe('🚆');
    expect(emojiForVehicle('FERRY')).toBe('⛴️');
    expect(emojiForVehicle('SUBWAY')).toBe('🚇');
    expect(emojiForVehicle(undefined)).toBe('🚉');
    expect(emojiForVehicle('SOMETHING_NEW')).toBe('🚉');
  });

  it('prefers nameShort but falls back to the long name', () => {
    const { hops } = hopsFromSteps(
      [{ travelMode: 'TRANSIT', transitDetails: { transitLine: { name: 'Sounder S Line' } } }],
      parseDuration,
    );
    expect(hops[0]!.line).toBe('Sounder S Line');
  });

  it('formats the route as a vehicle chain', () => {
    const it = {
      label: 'Bellevue office (nep)',
      destination: 'x',
      totalSeconds: 3900,
      walkSeconds: 600,
      hops: [
        { line: '535', vehicle: 'BUS', emoji: '🚌', seconds: 1080 },
        { line: '1 Line', vehicle: 'LIGHT_RAIL', emoji: '🚈', seconds: 720 },
        { line: '8', vehicle: 'BUS', emoji: '🚌', seconds: 360 },
      ],
    };
    expect(formatTransitRoute(it)).toBe(
      'House → 535 🚌 18m → 1 Line 🚈 12m → 8 🚌 6m → Bellevue office (nep)',
    );
    const out = formatTransit(it);
    expect(out).toContain('**65 min**');
    expect(out).toContain('2 transfer(s)');
    expect(out).toContain('10 min walking');
  });

  it('returns a transit itinerary from the API alongside driving', async () => {
    const fetchImpl = vi.fn(async (_u: unknown, init: RequestInit | undefined) => {
      const body = JSON.parse(String(init?.body)) as { travelMode: string };
      return body.travelMode === 'TRANSIT'
        ? new Response(JSON.stringify(transitBody), { status: 200 })
        : new Response(
            JSON.stringify({ routes: [{ duration: '1115s', distanceMeters: 18083 }] }),
            { status: 200 },
          );
    }) as unknown as typeof fetch;

    const r = await estimateCommutes(47.4779, -122.20163, { apiKey: 'k', fetchImpl });
    expect(r.status).toBe('ok');
    if (r.status === 'ok') {
      expect(r.value.drive).toHaveLength(3);
      expect(r.value.transit).toHaveLength(3);
      expect(r.value.transit[0]!.hops.map((h) => h.line)).toEqual(['535', '1 Line']);
    }
  });

  it('keeps driving when transit has no usable itinerary', async () => {
    const fetchImpl = vi.fn(async (_u: unknown, init: RequestInit | undefined) => {
      const body = JSON.parse(String(init?.body)) as { travelMode: string };
      return body.travelMode === 'TRANSIT'
        ? new Response(JSON.stringify({ routes: [] }), { status: 200 })
        : new Response(JSON.stringify({ routes: [{ duration: '1115s' }] }), { status: 200 });
    }) as unknown as typeof fetch;

    const r = await estimateCommutes(1, 2, { apiKey: 'k', fetchImpl });
    expect(r.status).toBe('ok');
    if (r.status === 'ok') {
      expect(r.value.drive).toHaveLength(3);
      expect(r.value.transit).toHaveLength(0);
    }
    expect(r.provenance).toContain('no usable itinerary');
  });
});

describe('legacy stored commute values', () => {
  // Exactly what is sitting in production D1 from before transit existed.
  const LEGACY = [
    {
      label: 'Bellevue office (nep)',
      destination: '601 108th Ave NE, Bellevue, WA 98004',
      driveSeconds: 2043,
      provider: 'google-routes',
    },
  ];

  it('reads the pre-transit array shape instead of showing nothing', () => {
    const v = normalizeCommuteValue(LEGACY)!;
    expect(v.drive).toHaveLength(1);
    expect(v.transit).toEqual([]);
    expect(formatCommute(v.drive)).toContain('34 min');
  });

  it('reads the current shape unchanged', () => {
    const v = normalizeCommuteValue({ drive: LEGACY, transit: [] })!;
    expect(v.drive).toHaveLength(1);
  });

  it('rejects junk', () => {
    expect(normalizeCommuteValue(null)).toBeNull();
    expect(normalizeCommuteValue('nope')).toBeNull();
    expect(normalizeCommuteValue({ nope: 1 })).toBeNull();
  });

  it('does NOT treat the legacy shape as settled, so update upgrades it', () => {
    expect(isCurrentCommuteShape(LEGACY)).toBe(false);
    expect(isCurrentCommuteShape({ drive: LEGACY })).toBe(false);
    expect(isCurrentCommuteShape({ drive: LEGACY, transit: [] })).toBe(true);
  });
});

describe('per-leg times and split departures', () => {
  it('carries each leg duration into the hop and the chain', () => {
    const { hops } = hopsFromSteps(
      [
        { travelMode: 'WALK', staticDuration: '300s' },
        {
          travelMode: 'TRANSIT',
          staticDuration: '1080s',
          transitDetails: { transitLine: { nameShort: '545', vehicle: { type: 'BUS' } } },
        },
        {
          travelMode: 'TRANSIT',
          staticDuration: '720s',
          transitDetails: { transitLine: { nameShort: '1 Line', vehicle: { type: 'LIGHT_RAIL' } } },
        },
      ],
      parseDuration,
    );
    expect(hops[0]).toMatchObject({ line: '545', seconds: 1080 });
    expect(hops[1]).toMatchObject({ line: '1 Line', seconds: 720 });
    expect(
      formatTransitRoute({ label: 'Hackerspace', destination: 'x', totalSeconds: 2100, hops }),
    ).toBe('House → 545 🚌 18m → 1 Line 🚈 12m → Hackerspace');
  });

  it('rounds a sub-minute leg up to 1m rather than showing 0m', () => {
    const { hops } = hopsFromSteps(
      [
        {
          travelMode: 'TRANSIT',
          staticDuration: '20s',
          transitDetails: { transitLine: { nameShort: 'SLU', vehicle: { type: 'TRAM' } } },
        },
      ],
      parseDuration,
    );
    expect(
      formatTransitRoute({ label: 'X', destination: 'x', totalSeconds: 20, hops }),
    ).toContain('SLU 🚊 1m');
  });

  it('defaults both driving and transit to 10:00 Pacific', async () => {
    const bodies: Array<{ travelMode: string; departureTime: string }> = [];
    const fetchImpl = vi.fn(async (_u: unknown, init: RequestInit | undefined) => {
      bodies.push(JSON.parse(String(init?.body)));
      return new Response(JSON.stringify({ routes: [{ duration: '600s' }] }), { status: 200 });
    }) as unknown as typeof fetch;

    await estimateCommutes(47.6, -122.3, { apiKey: 'k', fetchImpl }, new Date('2026-08-03T12:00:00Z'));

    const hourIn = (iso: string) =>
      new Intl.DateTimeFormat('en-US', {
        timeZone: 'America/Los_Angeles',
        hour: 'numeric',
        hour12: false,
      }).format(new Date(iso));

    const drive = bodies.filter((b) => b.travelMode === 'DRIVE');
    const transit = bodies.filter((b) => b.travelMode === 'TRANSIT');
    expect(drive).toHaveLength(3);
    expect(transit).toHaveLength(3);
    expect(drive.every((b) => hourIn(b.departureTime) === '10')).toBe(true);
    expect(transit.every((b) => hourIn(b.departureTime) === '10')).toBe(true);
  });

  it('honours explicit departure overrides independently', async () => {
    const bodies: Array<{ travelMode: string; departureTime: string }> = [];
    const fetchImpl = vi.fn(async (_u: unknown, init: RequestInit | undefined) => {
      bodies.push(JSON.parse(String(init?.body)));
      return new Response(JSON.stringify({ routes: [{ duration: '600s' }] }), { status: 200 });
    }) as unknown as typeof fetch;

    await estimateCommutes(47.6, -122.3, {
      apiKey: 'k',
      fetchImpl,
      departureIso: '2026-08-11T07:30:00-07:00',
      transitDepartureIso: '2026-08-11T11:15:00-07:00',
    });
    expect(bodies.find((b) => b.travelMode === 'DRIVE')!.departureTime).toBe(
      '2026-08-11T07:30:00-07:00',
    );
    expect(bodies.find((b) => b.travelMode === 'TRANSIT')!.departureTime).toBe(
      '2026-08-11T11:15:00-07:00',
    );
  });

  it('includes the hackerspace as a third destination', async () => {
    const fetchImpl = vi.fn(async () =>
      new Response(JSON.stringify({ routes: [{ duration: '900s' }] }), { status: 200 }),
    ) as unknown as typeof fetch;
    const r = await estimateCommutes(47.6, -122.3, { apiKey: 'k', fetchImpl });
    expect(r.status).toBe('ok');
    if (r.status === 'ok') {
      expect(r.value.drive.map((d) => d.label)).toEqual([
        'Bellevue office (nep)',
        'Seattle office (partner)',
        'Hackerspace',
      ]);
      expect(r.value.drive[2]!.destination).toBe('1517 12th Ave Suite 207, Seattle, WA 98122');
    }
  });

  it('nextTuesdayAt honours the requested local hour', () => {
    const iso = nextTuesdayAt(new Date('2026-12-25T12:00:00Z'), 10); // PST window
    const label = new Intl.DateTimeFormat('en-US', {
      timeZone: 'America/Los_Angeles',
      weekday: 'short',
      hour: 'numeric',
      hour12: false,
    }).format(new Date(iso));
    expect(label).toContain('Tue');
    expect(label).toMatch(/\b10\b/);
  });
});
