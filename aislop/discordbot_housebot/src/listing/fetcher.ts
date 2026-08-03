import { boundFetch } from '../http';
import { redfinSource } from './providers/redfin';
import { zillowSource } from './providers/zillow';
import type { FetchOptions, FetchResult, ListingSource } from './types';
import { parseUrl } from './url';

export const SOURCES: ListingSource[] = [zillowSource, redfinSource];

export function sourceFor(rawUrl: string): ListingSource | null {
  const u = parseUrl(rawUrl);
  if (!u) return null;
  return SOURCES.find((s) => s.matches(u)) ?? null;
}

export const DEFAULT_TIMEOUT_MS = 8000;

/**
 * Best-effort snapshot fetch of a *public* listing page.
 *
 * Ground rules, enforced here so no caller can opt out:
 *  - identify + honest User-Agent, one request, no retries in-band
 *  - hard timeout via AbortController
 *  - conditional GET (If-None-Match / If-Modified-Since) when we have validators
 *  - a 403/429 is reported as `blocked` and never retried harder; we do not
 *    rotate agents, solve challenges, or touch private/login endpoints
 */
export async function fetchListing(
  rawUrl: string,
  opts: FetchOptions,
  now: number = Math.floor(Date.now() / 1000),
): Promise<FetchResult> {
  const u = parseUrl(rawUrl);
  const source = u ? SOURCES.find((s) => s.matches(u)) : null;
  if (!u || !source) {
    return { ok: false, kind: 'error', reason: 'unsupported-url', detail: rawUrl };
  }
  const ident = source.identify(u);
  if (!ident) {
    return { ok: false, kind: 'error', reason: 'unsupported-url', detail: u.toString() };
  }

  const doFetch = boundFetch(opts.fetchImpl);
  const headers: Record<string, string> = {
    'User-Agent': opts.userAgent,
    Accept: 'text/html,application/xhtml+xml',
    'Accept-Language': 'en-US,en;q=0.9',
  };
  if (opts.etag) headers['If-None-Match'] = opts.etag;
  if (opts.lastModified) headers['If-Modified-Since'] = opts.lastModified;

  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), opts.timeoutMs);
  let res: Response;
  try {
    res = await doFetch(ident.canonicalUrl, {
      headers,
      redirect: 'follow',
      signal: controller.signal,
      // Edge cache dedupes bursts across invocations for free.
      cf: { cacheTtl: 300, cacheEverything: true },
    } as RequestInit);
  } catch (err) {
    const aborted = controller.signal.aborted;
    return {
      ok: false,
      kind: 'error',
      reason: aborted ? 'timeout' : 'network',
      detail: aborted ? `timeout after ${opts.timeoutMs}ms` : errText(err),
    };
  } finally {
    clearTimeout(timer);
  }

  if (res.status === 304) return { ok: true, kind: 'not-modified' };
  if (res.status === 403 || res.status === 429) {
    return {
      ok: false,
      kind: 'error',
      reason: 'blocked',
      detail: `HTTP ${res.status} from ${source.provider}`,
    };
  }
  if (!res.ok) {
    return { ok: false, kind: 'error', reason: 'http-status', detail: `HTTP ${res.status}` };
  }

  let html: string;
  try {
    html = await res.text();
  } catch (err) {
    return { ok: false, kind: 'error', reason: 'network', detail: errText(err) };
  }

  const snapshot = source.parse(html, ident.canonicalUrl, ident.listingId, now);
  if (!snapshot) {
    return {
      ok: false,
      kind: 'error',
      reason: 'unparseable',
      detail: `no structured data on ${source.provider} page`,
    };
  }

  const etag = res.headers.get('etag') ?? undefined;
  const lastModified = res.headers.get('last-modified') ?? undefined;
  return { ok: true, kind: 'snapshot', snapshot, etag, lastModified };
}

function errText(err: unknown): string {
  return err instanceof Error ? err.message : String(err);
}

export const FAILURE_TEXT: Record<string, string> = {
  network: "couldn't reach the listing page",
  timeout: 'the listing page took too long to answer',
  'http-status': 'the listing page returned an error',
  blocked: 'the provider blocked this request (bot protection) — try again later',
  unparseable: "the page loaded but didn't expose structured listing data",
  'unsupported-url': 'that link is not a Zillow or Redfin listing URL',
};

export function explainFailure(reason: string, detail: string): string {
  return `${FAILURE_TEXT[reason] ?? 'listing fetch failed'} (${detail})`;
}
