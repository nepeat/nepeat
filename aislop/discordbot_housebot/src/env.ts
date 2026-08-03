export interface Env {
  DB: D1Database;

  // vars (wrangler.jsonc)
  HOUSE_CHANNEL_ID: string;
  AIRTABLE_BASE_ID?: string;
  REFRESH_INTERVAL_MINUTES?: string;
  REFRESH_BATCH_SIZE?: string;
  USER_AGENT?: string;
  /**
   * "false" makes command replies ephemeral (caller-only). Default is public:
   * this is a shared house-hunting channel, so the answers are for the room.
   * Note that error tracebacks go public too.
   */
  PUBLIC_REPLIES?: string;

  // secrets (.dev.vars / wrangler secret put)
  DISCORD_PUBLIC_KEY: string;
  DISCORD_BOT_TOKEN: string;
  DISCORD_APPLICATION_ID: string;
  DISCORD_GUILD_ID?: string;

  AIRTABLE_TOKEN?: string;
  AIRTABLE_TABLE?: string;
  AIRTABLE_FIELD_MAP_JSON?: string;

  /** Google Maps Platform key, Routes API enabled. Enables commute estimates. */
  GOOGLE_MAPS_API_KEY?: string;
  /**
   * Pinned departure time for commute ETAs, ISO-8601 with offset. A commute
   * number with no stated departure assumption is a number that lies, so this
   * is stored and echoed into the thread message alongside the ETA.
   * Defaults to the next Tuesday 08:00 America/Los_Angeles when unset.
   */
  COMMUTE_DEPARTURE_ISO?: string;
  /**
   * Pinned transit departure, ISO-8601 with offset. Defaults to the next
   * Tuesday 10:00 America/Los_Angeles — off-peak enough that the itinerary
   * reflects ordinary service, not commuter-only runs.
   */
  TRANSIT_DEPARTURE_ISO?: string;
}

export function intVar(value: string | undefined, fallback: number): number {
  const n = value === undefined ? NaN : Number.parseInt(value, 10);
  return Number.isFinite(n) && n > 0 ? n : fallback;
}

export function boolVar(value: string | undefined, fallback: boolean): boolean {
  if (value === undefined || value.trim() === '') return fallback;
  return !/^(false|0|no|off)$/i.test(value.trim());
}

export const DEFAULT_USER_AGENT =
  'housebot/0.1 (+https://github.com/nepeat/discordbot_housebot)';
