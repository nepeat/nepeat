import { ChannelType } from './types';

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
    this.fetchImpl = opts.fetchImpl ?? fetch;
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
        throw new DiscordRestError(`${method} ${path} -> ${res.status}`, res.status, text);
      }
      if (res.status === 204) return undefined as T;
      return (await res.json()) as T;
    }
    throw new DiscordRestError(`${method} ${path} -> rate limited`, 429, '');
  }

  /** Public thread with no starter message, directly under a text channel. */
  createThread(channelId: string, name: string, autoArchiveMinutes = 10080) {
    return this.request<{ id: string; name: string }>(
      'POST',
      `/channels/${channelId}/threads`,
      { name, type: ChannelType.PUBLIC_THREAD, auto_archive_duration: autoArchiveMinutes },
    );
  }

  renameThread(threadId: string, name: string) {
    return this.request<{ id: string; name: string }>('PATCH', `/channels/${threadId}`, { name });
  }

  setThreadArchived(threadId: string, archived: boolean) {
    return this.request<{ id: string }>('PATCH', `/channels/${threadId}`, { archived });
  }

  postMessage(channelId: string, content: string) {
    return this.request<{ id: string }>('POST', `/channels/${channelId}/messages`, {
      content,
      allowed_mentions: { parse: [] },
    });
  }

  getChannel(channelId: string) {
    return this.request<{ id: string; type: number; parent_id?: string | null; name?: string }>(
      'GET',
      `/channels/${channelId}`,
    );
  }

  /** Replace the deferred "thinking" response. */
  editOriginalResponse(interactionToken: string, content: string) {
    return this.request<{ id: string }>(
      'PATCH',
      `/webhooks/${this.opts.applicationId}/${interactionToken}/messages/@original`,
      { content, allowed_mentions: { parse: [] } },
      'none',
    );
  }

  followUp(interactionToken: string, content: string, ephemeral = false) {
    return this.request<{ id: string }>(
      'POST',
      `/webhooks/${this.opts.applicationId}/${interactionToken}`,
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
