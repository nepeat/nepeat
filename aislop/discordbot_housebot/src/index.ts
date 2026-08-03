import { Repo } from './db/repo';
import { handleInteraction, jsonResponse } from './discord/interactions';
import { DiscordRest } from './discord/rest';
import type { Interaction } from './discord/types';
import { verifyDiscordRequest } from './discord/verify';
import { DEFAULT_USER_AGENT, intVar, type Env } from './env';
import { formatError } from './http';
import { runScheduledRefresh } from './scheduled/refresh';
import { HouseService } from './service/house';

export const FETCH_TIMEOUT_MS = 8000;

function build(env: Env) {
  const repo = new Repo(env.DB);
  const rest = new DiscordRest({
    botToken: env.DISCORD_BOT_TOKEN,
    applicationId: env.DISCORD_APPLICATION_ID,
  });
  const service = new HouseService({
    repo,
    rest,
    config: {
      houseChannelId: env.HOUSE_CHANNEL_ID,
      userAgent: env.USER_AGENT ?? DEFAULT_USER_AGENT,
      refreshIntervalSeconds: intVar(env.REFRESH_INTERVAL_MINUTES, 720) * 60,
      fetchTimeoutMs: FETCH_TIMEOUT_MS,
    },
    airtable: {
      token: env.AIRTABLE_TOKEN,
      baseId: env.AIRTABLE_BASE_ID,
      table: env.AIRTABLE_TABLE,
      fieldMapJson: env.AIRTABLE_FIELD_MAP_JSON,
    },
  });
  return { repo, rest, service };
}

export default {
  async fetch(request: Request, env: Env, ctx: ExecutionContext): Promise<Response> {
    const url = new URL(request.url);

    if (request.method === 'GET' && url.pathname === '/health') {
      return new Response('ok', { status: 200 });
    }
    if (request.method !== 'POST' || url.pathname !== '/interactions') {
      return new Response('not found', { status: 404 });
    }

    // Signature verification happens before ANY parsing or side effect.
    const body = await request.text();
    const valid = await verifyDiscordRequest(
      body,
      request.headers.get('x-signature-ed25519'),
      request.headers.get('x-signature-timestamp'),
      env.DISCORD_PUBLIC_KEY,
    );
    if (!valid) return new Response('invalid request signature', { status: 401 });

    let interaction: Interaction;
    try {
      interaction = JSON.parse(body) as Interaction;
    } catch {
      return new Response('bad request', { status: 400 });
    }

    const { repo, rest, service } = build(env);
    try {
      return await handleInteraction(interaction, {
        repo,
        service,
        houseChannelId: env.HOUSE_CHANNEL_ID,
        waitUntil: (p) => ctx.waitUntil(p),
        editOriginal: (token, content) => rest.editOriginalResponse(token, content),
      });
    } catch (err) {
      console.error('interaction handler failed', {
        id: interaction.id,
        error: err instanceof Error ? (err.stack ?? err.message) : String(err),
      });
      // Still answer Discord so the user does not see "application did not respond".
      return jsonResponse({
        type: 4,
        data: {
          content: `internal error handling that command:\n\`\`\`\n${formatError(err)}\n\`\`\``,
          flags: 1 << 6,
        },
      });
    }
  },

  async scheduled(_event: ScheduledController, env: Env, ctx: ExecutionContext): Promise<void> {
    const { repo, service } = build(env);
    ctx.waitUntil(
      runScheduledRefresh({
        repo,
        service,
        batchSize: intVar(env.REFRESH_BATCH_SIZE, 10),
      }).then((report) => {
        console.log('scheduled refresh', report);
      }),
    );
  },
};
