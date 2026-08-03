import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { describe, expect, it, vi } from 'vitest';
import { explainFailure, fetchListing, sourceFor } from '../src/listing/fetcher';

const ZURL = 'https://www.zillow.com/homedetails/400-Cedar-Ave-S-Renton-WA-98057/49059541_zpid/';

function fixture(name: string): string {
  return readFileSync(fileURLToPath(new URL(`./fixtures/${name}`, import.meta.url)), 'utf8');
}

const opts = (fetchImpl: typeof fetch) => ({
  userAgent: 'housebot-test/0.1',
  timeoutMs: 500,
  fetchImpl,
});

describe('fetchListing', () => {
  it('fetches, parses and reports validators', async () => {
    const fetchImpl = vi.fn(async () =>
      new Response(fixture('zillow-active.html'), {
        status: 200,
        headers: { etag: 'W/"abc"', 'last-modified': 'Tue, 01 Jul 2025 00:00:00 GMT' },
      }),
    ) as unknown as typeof fetch;

    const res = await fetchListing(ZURL, opts(fetchImpl), 1234);
    expect(res.ok).toBe(true);
    if (res.ok && res.kind === 'snapshot') {
      expect(res.snapshot.price).toBe(725000);
      expect(res.snapshot.fetchedAt).toBe(1234);
      expect(res.etag).toBe('W/"abc"');
      expect(res.lastModified).toBe('Tue, 01 Jul 2025 00:00:00 GMT');
    }
  });

  it('sends an honest User-Agent and conditional headers', async () => {
    const seen: Record<string, string> = {};
    const fetchImpl = vi.fn(async (_url: unknown, init: RequestInit | undefined) => {
      Object.assign(seen, init?.headers as Record<string, string>);
      // 304 cannot be constructed via `new Response`; stub the shape we read.
      return {
        status: 304,
        ok: false,
        headers: new Headers(),
        text: async () => '',
      } as unknown as Response;
    }) as unknown as typeof fetch;

    const res = await fetchListing(ZURL, {
      ...opts(fetchImpl),
      etag: 'W/"abc"',
      lastModified: 'Tue, 01 Jul 2025 00:00:00 GMT',
    });
    expect(seen['User-Agent']).toBe('housebot-test/0.1');
    expect(seen['If-None-Match']).toBe('W/"abc"');
    expect(seen['If-Modified-Since']).toBe('Tue, 01 Jul 2025 00:00:00 GMT');
    expect(res).toEqual({ ok: true, kind: 'not-modified' });
  });

  it('canonicalizes the request url (no tracking params)', async () => {
    let requested = '';
    const fetchImpl = vi.fn(async (url: unknown) => {
      requested = String(url);
      return new Response(fixture('zillow-active.html'), { status: 200 });
    }) as unknown as typeof fetch;

    await fetchListing(`${ZURL}?utm_campaign=spam`, opts(fetchImpl));
    expect(requested).toBe(ZURL);
  });

  it('reports bot protection as blocked, not as a retry loop', async () => {
    const fetchImpl = vi.fn(async () =>
      new Response(fixture('blocked.html'), { status: 403 }),
    ) as unknown as typeof fetch;
    const res = await fetchListing(ZURL, opts(fetchImpl));
    expect(res).toMatchObject({ ok: false, reason: 'blocked' });
    expect(fetchImpl).toHaveBeenCalledTimes(1);
  });

  it('reports other http failures', async () => {
    const fetchImpl = vi.fn(async () => new Response('', { status: 500 })) as unknown as typeof fetch;
    expect(await fetchListing(ZURL, opts(fetchImpl))).toMatchObject({
      ok: false,
      reason: 'http-status',
    });
  });

  it('reports unparseable pages instead of inventing data', async () => {
    const fetchImpl = vi.fn(async () =>
      new Response(fixture('blocked.html'), { status: 200 }),
    ) as unknown as typeof fetch;
    expect(await fetchListing(ZURL, opts(fetchImpl))).toMatchObject({
      ok: false,
      reason: 'unparseable',
    });
  });

  it('times out instead of hanging', async () => {
    const fetchImpl = vi.fn(
      (_url: unknown, init: RequestInit | undefined) =>
        new Promise<Response>((_resolve, reject) => {
          init?.signal?.addEventListener('abort', () =>
            reject(new DOMException('aborted', 'AbortError')),
          );
        }),
    ) as unknown as typeof fetch;

    const res = await fetchListing(ZURL, { ...opts(fetchImpl), timeoutMs: 20 });
    expect(res).toMatchObject({ ok: false, reason: 'timeout' });
  });

  it('reports network errors', async () => {
    const fetchImpl = vi.fn(async () => {
      throw new Error('econnreset');
    }) as unknown as typeof fetch;
    expect(await fetchListing(ZURL, opts(fetchImpl))).toMatchObject({
      ok: false,
      reason: 'network',
      detail: 'econnreset',
    });
  });

  it('refuses unsupported urls without any network call', async () => {
    const fetchImpl = vi.fn() as unknown as typeof fetch;
    expect(await fetchListing('https://www.realtor.com/x', opts(fetchImpl))).toMatchObject({
      ok: false,
      reason: 'unsupported-url',
    });
    expect(fetchImpl).not.toHaveBeenCalled();
  });
});

describe('sourceFor / explainFailure', () => {
  it('resolves providers', () => {
    expect(sourceFor(ZURL)?.provider).toBe('zillow');
    expect(sourceFor('https://www.redfin.com/WA/A/b/home/1')?.provider).toBe('redfin');
    expect(sourceFor('https://example.com')).toBeNull();
  });

  it('explains every failure reason in plain language', () => {
    for (const reason of ['network', 'timeout', 'http-status', 'blocked', 'unparseable', 'unsupported-url']) {
      expect(explainFailure(reason, 'detail')).toContain('detail');
    }
  });
});
