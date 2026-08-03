import { afterEach, describe, expect, it } from 'vitest';
import { syncListing } from '../src/airtable/client';
import { DiscordRest } from '../src/discord/rest';
import { boundFetch, formatError } from '../src/http';
import { fetchListing } from '../src/listing/fetcher';
import type { Snapshot } from '../src/listing/types';

/**
 * workerd throws "Illegal invocation" when `fetch` is called with a `this` that
 * is not globalThis. Node's fetch does not care, so a detached `fetch` passes
 * every local test and fails only in production -- which is exactly what
 * happened. This stub reproduces workerd's strictness.
 */
const realFetch = globalThis.fetch;

function installStrictFetch(): void {
  const strict = function (this: unknown) {
    if (this !== globalThis && this !== undefined) {
      throw new TypeError(
        'Illegal invocation: function called with incorrect `this` reference.',
      );
    }
    return Promise.resolve(new Response('{}', { status: 200 }));
  };
  Object.defineProperty(globalThis, 'fetch', { value: strict, configurable: true, writable: true });
}

afterEach(() => {
  Object.defineProperty(globalThis, 'fetch', {
    value: realFetch,
    configurable: true,
    writable: true,
  });
});

const snap: Snapshot = {
  provider: 'zillow',
  listingId: '1',
  listingKey: 'zillow:1',
  sourceUrl: 'https://www.zillow.com/homedetails/1_zpid/',
  status: 'active',
  price: 1,
  fetchedAt: 1,
};

describe('boundFetch', () => {
  it('survives being called as a method (the workerd failure mode)', async () => {
    installStrictFetch();
    const holder = { go: boundFetch() };
    await expect(holder.go('https://example.test')).resolves.toBeInstanceOf(Response);
  });

  it('demonstrates the bug it prevents', async () => {
    installStrictFetch();
    const holder = { go: globalThis.fetch };
    expect(() => holder.go('https://example.test')).toThrow('Illegal invocation');
  });

  it('passes an injected impl through untouched in behavior', async () => {
    const impl = (async () => new Response('hi', { status: 200 })) as unknown as typeof fetch;
    const res = await boundFetch(impl)('https://example.test');
    expect(await res.text()).toBe('hi');
  });
});

describe('callers do not detach fetch', () => {
  it('DiscordRest works under strict fetch', async () => {
    installStrictFetch();
    const rest = new DiscordRest({ botToken: 't', applicationId: 'a' });
    await expect(rest.postMessage('123', 'hi')).resolves.toBeDefined();
  });

  it('fetchListing works under strict fetch', async () => {
    installStrictFetch();
    const res = await fetchListing('https://www.zillow.com/homedetails/1_zpid/', {
      userAgent: 'test',
      timeoutMs: 500,
    });
    // The stub returns `{}`, so this reaches the parser and reports unparseable
    // rather than blowing up on an illegal invocation.
    expect(res).toMatchObject({ ok: false, reason: 'unparseable' });
  });

  it('airtable sync works under strict fetch', async () => {
    installStrictFetch();
    const res = await syncListing(snap, {
      token: 'pat1',
      baseId: 'app1',
      table: 'tbl1',
      fieldMapJson: JSON.stringify({ key: 'fldKey' }),
    });
    expect(res.status).toBe('ok');
  });
});

describe('formatError', () => {
  it('prefers the stack and truncates long ones', () => {
    const err = new Error('boom');
    expect(formatError(err)).toContain('boom');
    expect(formatError(err)).toContain('at ');

    const long = new Error('x');
    long.stack = 'y'.repeat(5000);
    const out = formatError(long, 100);
    expect(out.length).toBeLessThan(200);
    expect(out).toContain('(truncated)');
  });

  it('handles non-Error throws', () => {
    expect(formatError('plain string')).toBe('plain string');
    expect(formatError({ weird: true })).toBe('[object Object]');
  });
});
