export interface Env {
  DB: D1Database;

  // vars (wrangler.jsonc)
  HOUSE_CHANNEL_ID: string;
  AIRTABLE_BASE_ID?: string;
  REFRESH_INTERVAL_MINUTES?: string;
  REFRESH_BATCH_SIZE?: string;
  USER_AGENT?: string;

  // secrets (.dev.vars / wrangler secret put)
  DISCORD_PUBLIC_KEY: string;
  DISCORD_BOT_TOKEN: string;
  DISCORD_APPLICATION_ID: string;
  DISCORD_GUILD_ID?: string;

  AIRTABLE_TOKEN?: string;
  AIRTABLE_TABLE?: string;
  AIRTABLE_FIELD_MAP_JSON?: string;
}

export function intVar(value: string | undefined, fallback: number): number {
  const n = value === undefined ? NaN : Number.parseInt(value, 10);
  return Number.isFinite(n) && n > 0 ? n : fallback;
}

export const DEFAULT_USER_AGENT =
  'housebot/0.1 (+https://github.com/nepeat/discordbot_housebot)';
