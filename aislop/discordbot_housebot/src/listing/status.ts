import { CLOSED_STATUSES, type ListingStatus } from './types';

export interface ClosureState {
  /** Operator forced the house closed via /house close. */
  forceClosed: boolean;
  /** Most recent listing status observed from the source. */
  listingStatus: ListingStatus;
}

/** A house shows as closed if the listing says so OR an operator forced it. */
export function isClosed(state: ClosureState): boolean {
  return state.forceClosed || CLOSED_STATUSES.has(state.listingStatus);
}

export type OpenResult =
  | { ok: true; nextForceClosed: false }
  | { ok: false; reason: 'listing-closed' | 'unknown-status' };

/**
 * `/house open` clears the force-close flag, but only when the live listing
 * actually supports being open. We never contradict the source.
 */
export function evaluateOpen(listingStatus: ListingStatus): OpenResult {
  if (CLOSED_STATUSES.has(listingStatus)) return { ok: false, reason: 'listing-closed' };
  if (listingStatus === 'unknown') return { ok: false, reason: 'unknown-status' };
  return { ok: true, nextForceClosed: false };
}

export const OPEN_FAILURE_TEXT: Record<'listing-closed' | 'unknown-status', string> = {
  'listing-closed':
    'the listing still reports sold/closed, so I left this house closed. Use `/house update` once the source flips back to active.',
  'unknown-status':
    "I couldn't determine a status from the listing page, so I left this house closed rather than guess.",
};

/** Force-closing is unconditional and does not require a fetch. */
export function applyForceClose(): { forceClosed: true } {
  return { forceClosed: true };
}
