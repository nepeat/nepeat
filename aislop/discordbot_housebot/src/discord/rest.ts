import { boundFetch } from '../http';
import { Routes } from 'discord-api-types/v10';
import {
  ChannelType,
  requiresStarterMessage,
  threadTypeFor,
  type APIEmbed,
  type RESTPostAPIChannelThreadsJSONBody,
  type RESTPostAPIGuildForumThreadsJSONBody,
} from './types';

export const DISCORD_API = 'https://discord.com/api/v10';

export interface DiscordRestOptions {
  botToken: string;
  applicationId: string;
  fetchImpl?: typeof fetch;
  apiBase?: string;
}

export class DiscordRestError extends Error {
  constructor(
    message: string,
    readonly status: number,
    readonly body: string,
  ) {
    super(message);
    this.name = 'DiscordRestError';
  }
}

/**
 * Minimal Discord REST client. Only the endpoints housebot needs, with one
 * bounded retry on 429 so a rate limit doesn't lose a thread update.
 */
export class DiscordRest {
  private readonly fetchImpl: typeof fetch;
  private readonly base: string;

  constructor(private readonly opts: DiscordRestOptions) {
    this.fetchImpl = boundFetch(opts.fetchImpl);
    this.base = opts.apiBase ?? DISCORD_API;
  }

  private async request<T>(
    method: string,
    path: string,
    body?: unknown,
    auth: 'bot' | 'none' = 'bot',
  ): Promise<T> {
    const headers: Record<string, string> = { 'Content-Type': 'application/json' };
    if (auth === 'bot') headers['Authorization'] = `Bot ${this.opts.botToken}`;

    for (let attempt = 0; attempt < 2; attempt++) {
      const res = await this.fetchImpl(`${this.base}${path}`, {
        method,
        headers,
        body: body === undefined ? undefined : JSON.stringify(body),
      });

      if (res.status === 429 && attempt === 0) {
        const retry = await safeJson(res);
        const wait = Math.min(5000, Math.round(Number(retry?.['retry_after'] ?? 1) * 1000));
        await sleep(wait);
        continue;
      }
      if (!res.ok) {
        const text = await res.text().catch(() => '');
        // Discord puts the actual reason in the body ("Invalid Form Body" plus
        // the offending field). A bare status code is not debuggable.
        throw new DiscordRestError(
          `${method} ${path} -> ${res.status} ${text.slice(0, 600)}`,
          res.status,
          text,
        );
      }
      if (res.status === 204) return undefined as T;
      return (await res.json()) as T;
    }
    throw new DiscordRestError(`${method} ${path} -> rate limited`, 429, '');
  }

  /**
   * Create a thread under `channelId`.
   *
   * Forum and media channels REQUIRE a starter message and reject a bare
   * `{name, type}` body with `50035 / message: BASE_TYPE_REQUIRED`; text and
   * announcement channels require the opposite. The caller passes the parent
   * channel's type so the right body is built.
   */
  async createThread(
    channelId: string,
    name: string,
    opts: {
      parentType?: number;
      starter?: { content?: string; embeds?: APIEmbed[] };
      autoArchiveMinutes?: number;
    } = {},
  ): Promise<{ id: string; name: string; usedStarter: boolean }> {
    const autoArchive = opts.autoArchiveMinutes ?? 10080;

    if (requiresStarterMessage(opts.parentType)) {
      const body: RESTPostAPIGuildForumThreadsJSONBody = {
        name,
        auto_archive_duration: autoArchive,
        message: {
          ...(opts.starter?.content ? { content: opts.starter.content } : {}),
          ...(opts.starter?.embeds?.length ? { embeds: opts.starter.embeds } : {}),
          allowed_mentions: { parse: [] },
        },
      };
      const thread = await this.request<{ id: string; name: string }>(
        'POST',
        Routes.threads(channelId),
        body,
      );
      return { ...thread, usedStarter: true };
    }

    const body: RESTPostAPIChannelThreadsJSONBody = {
      name,
      type: threadTypeFor(opts.parentType) as
        | ChannelType.PublicThread
        | ChannelType.AnnouncementThread,
      auto_archive_duration: autoArchive,
    };
    const thread = await this.request<{ id: string; name: string }>(
      'POST',
      Routes.threads(channelId),
      body,
    );
    return { ...thread, usedStarter: false };
  }

  renameThread(threadId: string, name: string) {
    return this.request<{ id: string; name: string }>('PATCH', Routes.channel(threadId), { name });
  }

  setThreadArchived(threadId: string, archived: boolean) {
    return this.request<{ id: string }>('PATCH', Routes.channel(threadId), { archived });
  }

  postMessage(channelId: string, content: string, embeds?: APIEmbed[]) {
    return this.request<{ id: string }>('POST', Routes.channelMessages(channelId), {
      ...(content ? { content } : {}),
      ...(embeds?.length ? { embeds } : {}),
      allowed_mentions: { parse: [] },
    });
  }

  getChannel(channelId: string) {
    return this.request<{ id: string; type: number; parent_id?: string | null; name?: string }>(
      'GET',
      Routes.channel(channelId),
    );
  }

  /** Replace the deferred "thinking" response. */
  editOriginalResponse(interactionToken: string, content: string) {
    return this.request<{ id: string }>(
      'PATCH',
      Routes.webhookMessage(this.opts.applicationId, interactionToken, '@original'),
      { content, allowed_mentions: { parse: [] } },
      'none',
    );
  }

  followUp(interactionToken: string, content: string, ephemeral = false) {
    return this.request<{ id: string }>(
      'POST',
      Routes.webhook(this.opts.applicationId, interactionToken),
      {
        content,
        flags: ephemeral ? 1 << 6 : 0,
        allowed_mentions: { parse: [] },
      },
      'none',
    );
  }
}

async function safeJson(res: Response): Promise<Record<string, unknown> | null> {
  try {
    return (await res.json()) as Record<string, unknown>;
  } catch {
    return null;
  }
}

function sleep(ms: number): Promise<void> {
  return new Promise((r) => setTimeout(r, ms));
}
