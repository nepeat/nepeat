import type { ListingStatus } from './types';

/** Extract every <script type="application/ld+json"> payload. */
export function extractJsonLd(html: string): unknown[] {
  const out: unknown[] = [];
  const re =
    /<script[^>]+type=["']application\/ld\+json["'][^>]*>([\s\S]*?)<\/script>/gi;
  let m: RegExpExecArray | null;
  while ((m = re.exec(html)) !== null) {
    const body = (m[1] ?? '').trim();
    if (!body) continue;
    try {
      const parsed: unknown = JSON.parse(body);
      if (Array.isArray(parsed)) out.push(...parsed);
      else out.push(parsed);
    } catch {
      // Providers ship malformed LD+JSON often enough; skip quietly.
    }
  }
  // Flatten @graph containers.
  const flat: unknown[] = [];
  for (const node of out) {
    const graph = isRecord(node) ? node['@graph'] : undefined;
    if (Array.isArray(graph)) flat.push(...graph);
    else flat.push(node);
  }
  return flat;
}

export function isRecord(v: unknown): v is Record<string, unknown> {
  return typeof v === 'object' && v !== null && !Array.isArray(v);
}

export function typesOf(node: unknown): string[] {
  if (!isRecord(node)) return [];
  const t = node['@type'];
  if (typeof t === 'string') return [t];
  if (Array.isArray(t)) return t.filter((x): x is string => typeof x === 'string');
  return [];
}

/** First LD+JSON node whose @type matches any of `wanted` (case-insensitive). */
export function findByType(
  nodes: unknown[],
  wanted: string[],
): Record<string, unknown> | null {
  const want = new Set(wanted.map((w) => w.toLowerCase()));
  for (const n of nodes) {
    if (typesOf(n).some((t) => want.has(t.toLowerCase()))) {
      return n as Record<string, unknown>;
    }
  }
  return null;
}

/**
 * Find `"key": <number>` in an embedded hydration blob. Deliberately shallow:
 * we only read what the public page already renders, and a miss is fine.
 */
export function scanNumber(html: string, key: string): number | undefined {
  const re = new RegExp(`"${key}"\\s*:\\s*(-?\\d+(?:\\.\\d+)?)`);
  const m = re.exec(html);
  if (!m) return undefined;
  const n = Number(m[1]);
  return Number.isFinite(n) ? n : undefined;
}

/** Find `"key": "value"` in an embedded hydration blob. */
export function scanString(html: string, key: string): string | undefined {
  const re = new RegExp(`"${key}"\\s*:\\s*"((?:[^"\\\\]|\\\\.)*)"`);
  const m = re.exec(html);
  if (!m) return undefined;
  try {
    return JSON.parse(`"${m[1]}"`) as string;
  } catch {
    return m[1];
  }
}

export function num(v: unknown): number | undefined {
  if (typeof v === 'number' && Number.isFinite(v)) return v;
  if (typeof v === 'string') {
    const cleaned = v.replace(/[$,+]/g, '').trim();
    const n = Number.parseFloat(cleaned);
    if (Number.isFinite(n)) return n;
  }
  return undefined;
}

export function str(v: unknown): string | undefined {
  return typeof v === 'string' && v.trim() ? v.trim() : undefined;
}

const STATUS_TABLE: Array<[RegExp, ListingStatus]> = [
  [/sold|closed|off_?market|discontinued|soldout|no_?longer/i, 'closed'],
  [/pending|under_?contract|underoffer|accepting_?backup/i, 'pending'],
  [/contingent/i, 'contingent'],
  [/for_?sale|active|instock|in_?stock|available|coming_?soon|new/i, 'active'],
];

/** Normalize a provider status/availability string to our lifecycle enum. */
export function normalizeStatus(raw: string | undefined): ListingStatus {
  if (!raw) return 'unknown';
  const s = raw.replace(/^https?:\/\/schema\.org\//i, '');
  for (const [re, status] of STATUS_TABLE) {
    if (re.test(s)) return status;
  }
  return 'unknown';
}

/** Labels that mark the end of the heating section in a rendered facts list. */
const HVAC_STOP_WORDS =
  /\b(?:Cooling|Appliances|Features|Utilities|Parking|Interior|Flooring|Heating type|Has heating)\b/i;

/**
 * Pull the heating facts out of a rendered `<h6>Heating</h6><ul>…` block.
 * Zillow renders these as markup, not as JSON keys, so tag-stripping a bounded
 * window after the heading is the only honest way to read it.
 */
export function hvacFromSection(html: string): string | undefined {
  const heading = /<[^>]*>\s*Heat(?:ing| Source| Type)\s*<\/(?:h\d|dt|span|strong)>/i.exec(html);
  if (!heading) return undefined;

  const window = html.slice(heading.index, heading.index + 1200);
  const text = window
    .replace(/<!--.*?-->/gs, '')
    .replace(/<[^>]+>/g, ' ')
    .replace(/\s+/g, ' ')
    .trim();

  const after = text.replace(/^Heat(?:ing| Source| Type)\s*:?\s*/i, '');
  const stop = HVAC_STOP_WORDS.exec(after);
  const value = (stop ? after.slice(0, stop.index) : after).trim().replace(/[,;]$/, '');
  if (value.length < 3 || value.length > 120) return undefined;
  return value;
}

/** Grab an HVAC-ish phrase from listing text. Always treated as unverified. */
export function scanHvac(html: string): string | undefined {
  const direct =
    scanString(html, 'heating') ??
    scanString(html, 'heatingSystem') ??
    scanString(html, 'heatSource');
  if (direct && direct.length <= 80 && !/^https?:/i.test(direct)) return direct;

  return hvacFromSection(html);
}

/** Split "400 Cedar Avenue S, Renton, WA 98057" into parts. */
export function splitAddress(full: string): {
  address?: string;
  city?: string;
  state?: string;
  zip?: string;
} {
  const parts = full.split(',').map((p) => p.trim()).filter(Boolean);
  if (parts.length === 0) return {};
  const last = parts[parts.length - 1] ?? '';
  const m = /^([A-Za-z]{2})\s+(\d{5}(?:-\d{4})?)$/.exec(last);
  if (parts.length >= 3 && m) {
    return {
      address: parts.slice(0, parts.length - 2).join(', '),
      city: parts[parts.length - 2],
      state: m[1],
      zip: m[2],
    };
  }
  return { address: full.trim() };
}
