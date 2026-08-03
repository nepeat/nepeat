/**
 * Classify the raw heating text a listing renders into something decision-shaped.
 *
 * The listing text is agent-written prose, so this is always `unverified`. The
 * verification path is the King County Assessor's per-parcel `Heat Source`
 * field (keyless HTML, reached via lat/lon -> parcel PIN), which is not wired
 * up yet -- see docs/ROADMAP.md.
 */

export type HvacKind =
  | 'heat-pump'
  | 'forced-air-gas'
  | 'forced-air-electric'
  | 'baseboard-electric'
  | 'radiant-floor'
  | 'radiator-steam'
  | 'oil'
  | 'none'
  | 'unknown';

export interface HvacClassification {
  kinds: HvacKind[];
  /** Kinds the operator has said they do not want. Drives the ⚠️ prefix. */
  disliked: HvacKind[];
  raw: string;
}

/** Operator preference: oil furnaces and steam radiators are dealbreakers. */
export const DISLIKED_KINDS: ReadonlySet<HvacKind> = new Set(['oil', 'radiator-steam']);

export const HVAC_LABEL: Record<HvacKind, string> = {
  'heat-pump': 'heat pump',
  'forced-air-gas': 'forced air (gas)',
  'forced-air-electric': 'forced air (electric)',
  'baseboard-electric': 'electric baseboard',
  'radiant-floor': 'radiant floor',
  'radiator-steam': 'radiators (steam/hydronic)',
  oil: 'oil',
  none: 'none',
  unknown: 'unknown',
};

/**
 * Order matters: the first pattern that matches a segment wins, so the more
 * specific phrasing has to come first. In particular `radiant floor` (hydronic
 * underfloor, modern, desirable) must be tested BEFORE `radiator`, and
 * "radiant" alone must not be treated as a radiator -- collapsing those two
 * would flag a nice house as a dealbreaker.
 */
const RULES: Array<[RegExp, HvacKind]> = [
  [/\bheat\s*pump|mini[- ]?split|ductless|hvac heat pump\b/i, 'heat-pump'],
  [/\bradiant\s*(?:floor|heat(?:ing)?|slab)|in[- ]?floor|hydronic\s*floor\b/i, 'radiant-floor'],
  [/\bradiator|steam\s*heat|hot\s*water\s*(?:baseboard|radiator)|boiler\b/i, 'radiator-steam'],
  [/\boil\b(?!\s*rubbed)|fuel\s*oil|oil\s*(?:furnace|tank|heat)/i, 'oil'],
  [/\bbaseboard|wall\s*heater|cadet\b/i, 'baseboard-electric'],
  [/\b(?:forced\s*air|central|furnace|gas\s*heat)\b[^,;]*\b(?:gas|propane)\b/i, 'forced-air-gas'],
  [/\b(?:gas|propane)\b[^,;]*\b(?:forced\s*air|furnace|central)\b/i, 'forced-air-gas'],
  [/\b(?:natural\s*gas|propane)\b/i, 'forced-air-gas'],
  [/\b(?:forced\s*air|central)\b[^,;]*\belectric\b/i, 'forced-air-electric'],
  [/\belectric\b/i, 'forced-air-electric'],
  [/\bforced\s*air|central\s*heat|furnace\b/i, 'forced-air-gas'],
  [/\bno\s*heat|none\b/i, 'none'],
];

/**
 * A listing usually lists several systems ("Fireplace, Heat Pump, Natural Gas").
 * Classify each comma/slash-separated segment so a heat pump paired with a gas
 * backup reports both rather than whichever pattern happened to match first.
 */
export function classifyHvac(raw: string | undefined | null): HvacClassification | null {
  const text = (raw ?? '').trim();
  if (!text) return null;

  const kinds: HvacKind[] = [];
  for (const segment of text.split(/[,;/]|\band\b/i)) {
    const piece = segment.trim();
    if (!piece) continue;
    for (const [re, kind] of RULES) {
      if (re.test(piece)) {
        if (!kinds.includes(kind)) kinds.push(kind);
        break;
      }
    }
  }
  if (kinds.length === 0) kinds.push('unknown');

  return {
    kinds,
    disliked: kinds.filter((k) => DISLIKED_KINDS.has(k)),
    raw: text,
  };
}

/** One line for the thread message. Leads with the warning when relevant. */
export function formatHvac(c: HvacClassification): string {
  const labels = c.kinds.map((k) => HVAC_LABEL[k]).join(', ');
  const warn = c.disliked.length
    ? `⚠️ **${c.disliked.map((k) => HVAC_LABEL[k]).join(' + ')}** — `
    : '';
  return `${warn}${labels} _(unverified, from listing text: "${c.raw}")_`;
}
