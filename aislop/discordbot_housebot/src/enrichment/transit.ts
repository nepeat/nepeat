import type { TransitHop, TransitItinerary } from './index';

/**
 * Vehicle enum -> emoji. Light rail and heavy rail get different glyphs on
 * purpose: Link 1 Line and Sounder are not the same trip.
 */
const VEHICLE_EMOJI: Record<string, string> = {
  BUS: '🚌',
  INTERCITY_BUS: '🚌',
  TROLLEYBUS: '🚎',
  SHARE_TAXI: '🚐',
  LIGHT_RAIL: '🚈',
  TRAM: '🚊',
  SUBWAY: '🚇',
  METRO_RAIL: '🚇',
  MONORAIL: '🚝',
  HEAVY_RAIL: '🚆',
  RAIL: '🚆',
  COMMUTER_TRAIN: '🚆',
  HIGH_SPEED_TRAIN: '🚄',
  LONG_DISTANCE_TRAIN: '🚄',
  FERRY: '⛴️',
  CABLE_CAR: '🚡',
  GONDOLA_LIFT: '🚡',
  FUNICULAR: '🚞',
  OTHER: '🚉',
};

export function emojiForVehicle(vehicle: string | undefined): string {
  if (!vehicle) return VEHICLE_EMOJI['OTHER'] as string;
  return VEHICLE_EMOJI[vehicle.toUpperCase()] ?? (VEHICLE_EMOJI['OTHER'] as string);
}

/** Routes API transit step shape, narrowed to what we read. */
export interface RawTransitStep {
  transitDetails?: {
    stopCount?: number;
    transitLine?: {
      name?: string;
      nameShort?: string;
      vehicle?: { type?: string };
    };
  };
  staticDuration?: string;
  travelMode?: string;
}

/**
 * Collapse a Routes leg into the boarded vehicles. Walking legs become the
 * aggregate walk time rather than arrows, so the chain reads as the thing you
 * actually have to remember: which vehicles, in order.
 */
export function hopsFromSteps(
  steps: RawTransitStep[] | undefined,
  parseSeconds: (v: string | undefined) => number | undefined,
): { hops: TransitHop[]; walkSeconds: number } {
  const hops: TransitHop[] = [];
  let walkSeconds = 0;

  for (const step of steps ?? []) {
    const details = step.transitDetails;
    if (!details?.transitLine) {
      if (step.travelMode === 'WALK') walkSeconds += parseSeconds(step.staticDuration) ?? 0;
      continue;
    }
    const line = details.transitLine;
    // nameShort is the rider-facing designator ("535", "1 Line"); name is the
    // long marketing string. Prefer the short one, fall back to the long.
    const label = (line.nameShort ?? line.name ?? '?').trim();
    hops.push({
      line: label,
      vehicle: line.vehicle?.type ?? 'OTHER',
      emoji: emojiForVehicle(line.vehicle?.type),
      ...(details.stopCount !== undefined ? { stopCount: details.stopCount } : {}),
    });
  }
  return { hops, walkSeconds };
}

/** `House → 535 🚌 → 1 Line 🚈 → 8 🚌 → Bellevue office (nep)` */
export function formatTransitRoute(it: TransitItinerary): string {
  const chain = ['House', ...it.hops.map((h) => `${h.line} ${h.emoji}`), it.label];
  return chain.join(' → ');
}

export function formatTransit(it: TransitItinerary): string {
  const mins = Math.round(it.totalSeconds / 60);
  const walk =
    it.walkSeconds && it.walkSeconds > 60
      ? ` · ${Math.round(it.walkSeconds / 60)} min walking`
      : '';
  const transfers = it.hops.length > 1 ? ` · ${it.hops.length - 1} transfer(s)` : '';
  return `**${mins} min**${transfers}${walk}\n${formatTransitRoute(it)}`;
}
