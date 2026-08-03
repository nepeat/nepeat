import { describe, expect, it, vi } from 'vitest';
import { syncListing } from '../src/airtable/client';
import { buildFields, parseFieldMap } from '../src/airtable/fieldmap';
import type { Snapshot } from '../src/listing/types';

const snap: Snapshot = {
  provider: 'zillow',
  listingId: '1',
  listingKey: 'zillow:1',
  sourceUrl: 'https://www.zillow.com/homedetails/1_zpid/',
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

const MAP = JSON.stringify({ key: 'fldKey', url: 'fldUrl', price: 'Price', bogus: 'fldNope' });

describe('parseFieldMap', () => {
  it('requires an explicit map', () => {
    expect(parseFieldMap(undefined).ok).toBe(false);
    expect(parseFieldMap('   ').ok).toBe(false);
    expect(parseFieldMap('{oops').ok).toBe(false);
    expect(parseFieldMap('[]').ok).toBe(false);
  });

  it('requires a key column for upsert', () => {
    const r = parseFieldMap(JSON.stringify({ price: 'Price' }));
    expect(r.ok).toBe(false);
    expect(r.problem).toContain('key');
  });

  it('accepts a valid map and reports ignored keys', () => {
    const r = parseFieldMap(MAP);
    expect(r.ok).toBe(true);
    expect(r.map).toEqual({ key: 'fldKey', url: 'fldUrl', price: 'Price' });
    expect(r.problem).toContain('bogus');
  });
});

describe('buildFields', () => {
  it('only emits mapped fields, using the operator field ids', () => {
    const fields = buildFields(snap, parseFieldMap(MAP).map);
    expect(fields).toEqual({
      fldKey: 'zillow:1',
      fldUrl: 'https://www.zillow.com/homedetails/1_zpid/',
      Price: 725000,
    });
  });

  it('adds a thread deep link only when both ids are known', () => {
    const map = { key: 'k', threadUrl: 't' } as const;
    expect(buildFields(snap, map, { guildId: '1', threadId: '2' })['t']).toBe(
      'https://discord.com/channels/1/2',
    );
    expect(buildFields(snap, map, {})['t']).toBeUndefined();
  });
});

describe('syncListing', () => {
  const cfg = {
    token: 'patTESTTOKEN123456',
    baseId: 'appc5LQi7Uo9Y75yN',
    table: 'tblHouses',
    fieldMapJson: MAP,
    apiBase: 'https://airtable.test/v0',
  };

  it('no-ops with a traceable reason when unconfigured', async () => {
    expect(await syncListing(snap, { ...cfg, token: undefined })).toEqual({
      status: 'skipped',
      detail: 'AIRTABLE_TOKEN not set',
    });
    expect(await syncListing(snap, { ...cfg, table: undefined })).toMatchObject({
      status: 'skipped',
    });
    expect(await syncListing(snap, { ...cfg, fieldMapJson: undefined })).toMatchObject({
      status: 'skipped',
    });
  });

  it('performs an idempotent upsert on the canonical key', async () => {
    let captured: { url: string; body: Record<string, unknown> } | null = null;
    const fetchImpl = vi.fn(async (url: unknown, init: RequestInit | undefined) => {
      captured = { url: String(url), body: JSON.parse(String(init?.body)) };
      return new Response(JSON.stringify({ records: [{ id: 'recABC' }], updatedRecords: ['recABC'] }), {
        status: 200,
      });
    }) as unknown as typeof fetch;

    const res = await syncListing(snap, { ...cfg, fetchImpl });
    expect(res).toEqual({ status: 'ok', recordId: 'recABC', detail: 'updated' });
    expect(captured!.url).toBe('https://airtable.test/v0/appc5LQi7Uo9Y75yN/tblHouses');
    expect(captured!.body['performUpsert']).toEqual({ fieldsToMergeOn: ['fldKey'] });
  });

  it('returns an error result (never throws) on http failure and redacts tokens', async () => {
    const fetchImpl = vi.fn(async () =>
      new Response('bad token patSECRETVALUE12345', { status: 401 }),
    ) as unknown as typeof fetch;
    const res = await syncListing(snap, { ...cfg, fetchImpl });
    expect(res.status).toBe('error');
    expect(res.detail).not.toContain('SECRETVALUE');
    expect(res.detail).toContain('pat***');
  });

  it('returns an error result on network failure', async () => {
    const fetchImpl = vi.fn(async () => {
      throw new Error('dns');
    }) as unknown as typeof fetch;
    expect((await syncListing(snap, { ...cfg, fetchImpl })).status).toBe('error');
  });
});
