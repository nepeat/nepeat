import type { Provider } from './types';

export interface IdentifiedUrl {
  provider: Provider;
  listingId: string;
  listingKey: string;
  canonicalUrl: string;
}

const ZILLOW_HOSTS = new Set(['zillow.com', 'www.zillow.com', 'm.zillow.com']);
const REDFIN_HOSTS = new Set(['redfin.com', 'www.redfin.com']);

/** Parse loosely (bare hosts, angle-bracket wrapped, trailing punctuation). */
export function parseUrl(raw: string): URL | null {
  let s = raw.trim();
  if (s.startsWith('<') && s.endsWith('>')) s = s.slice(1, -1);
  s = s.replace(/[.,;)\]]+$/, '');
  if (!/^https?:\/\//i.test(s)) s = `https://${s}`;
  try {
    return new URL(s);
  } catch {
    return null;
  }
}

function host(u: URL): string {
  return u.hostname.toLowerCase();
}

/**
 * Recognize a supported listing URL and return its canonical form.
 * Canonical = https, canonical host, path only -- query strings on these
 * providers are tracking/UI state and never identity.
 */
export function identifyUrl(raw: string): IdentifiedUrl | null {
  const u = parseUrl(raw);
  if (!u) return null;

  if (ZILLOW_HOSTS.has(host(u))) return identifyZillow(u);
  if (REDFIN_HOSTS.has(host(u))) return identifyRedfin(u);
  return null;
}

function identifyZillow(u: URL): IdentifiedUrl | null {
  // /homedetails/<slug>/<zpid>_zpid/  or  /homedetails/<zpid>_zpid/
  const m = /(?:^|\/)(\d+)_zpid(?:\/|$)/.exec(u.pathname);
  if (!m) return null;
  const listingId = m[1] as string;
  const slug = /\/homedetails\/([^/]+)\/\d+_zpid/.exec(u.pathname)?.[1];
  const path = slug
    ? `/homedetails/${slug}/${listingId}_zpid/`
    : `/homedetails/${listingId}_zpid/`;
  return {
    provider: 'zillow',
    listingId,
    listingKey: `zillow:${listingId}`,
    canonicalUrl: `https://www.zillow.com${path}`,
  };
}

function identifyRedfin(u: URL): IdentifiedUrl | null {
  // /<ST>/<City>/<address-slug>/home/<id>
  const m = /\/home\/(\d+)(?:\/|$)/.exec(u.pathname);
  if (!m) return null;
  const listingId = m[1] as string;
  const path = u.pathname.replace(/\/+$/, '');
  return {
    provider: 'redfin',
    listingId,
    listingKey: `redfin:${listingId}`,
    canonicalUrl: `https://www.redfin.com${path}`,
  };
}

/** Pull the first supported listing URL out of free text (thread starters etc). */
export function extractListingUrl(text: string): IdentifiedUrl | null {
  const candidates = text.match(/https?:\/\/[^\s<>"']+/gi) ?? [];
  for (const c of candidates) {
    const id = identifyUrl(c);
    if (id) return id;
  }
  return null;
}
