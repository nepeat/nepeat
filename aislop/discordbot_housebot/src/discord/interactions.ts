import type { PropertyRow, Repo } from '../db/repo';
import type { HouseService } from '../service/house';
import {
  InteractionResponseType,
  InteractionType,
  MessageFlags,
  subcommandOf,
  type Interaction,
} from './types';

export interface InteractionDeps {
  repo: Repo;
  service: HouseService;
  houseChannelId: string;
  now?: () => number;
  /** Schedules deferred work; Worker passes ctx.waitUntil. */
  waitUntil: (p: Promise<unknown>) => void;
  /** Replaces the deferred "thinking" response via the follow-up webhook. */
  editOriginal: (token: string, content: string) => Promise<unknown>;
}

export function jsonResponse(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { 'Content-Type': 'application/json' },
  });
}

function deferEphemeral(): Response {
  return jsonResponse({
    type: InteractionResponseType.DEFERRED_CHANNEL_MESSAGE_WITH_SOURCE,
    data: { flags: MessageFlags.EPHEMERAL },
  });
}

function replyEphemeral(content: string): Response {
  return jsonResponse({
    type: InteractionResponseType.CHANNEL_MESSAGE_WITH_SOURCE,
    data: { content, flags: MessageFlags.EPHEMERAL, allowed_mentions: { parse: [] } },
  });
}

/** Is this interaction coming from the house channel or one of its threads? */
export function inHouseScope(interaction: Interaction, houseChannelId: string): boolean {
  const ch = interaction.channel;
  if (interaction.channel_id === houseChannelId) return true;
  if (ch?.parent_id && ch.parent_id === houseChannelId) return true;
  return false;
}

export async function handleInteraction(
  interaction: Interaction,
  deps: InteractionDeps,
): Promise<Response> {
  if (interaction.type === InteractionType.PING) {
    return jsonResponse({ type: InteractionResponseType.PONG });
  }
  if (interaction.type !== InteractionType.APPLICATION_COMMAND) {
    return replyEphemeral('unsupported interaction type.');
  }
  if (interaction.data?.name !== 'house') {
    return replyEphemeral('unknown command.');
  }

  const { name: sub, options } = subcommandOf(interaction);
  if (!sub) return replyEphemeral('missing subcommand.');

  const now = deps.now?.() ?? Math.floor(Date.now() / 1000);
  const fresh = await deps.repo.claimInteraction(interaction.id, `house ${sub}`, now);
  if (!fresh) {
    // Discord retried a delivery we already handled.
    return replyEphemeral('already handled that one.');
  }

  if (!inHouseScope(interaction, deps.houseChannelId)) {
    return replyEphemeral(
      `\`/house\` only works in <#${deps.houseChannelId}> or one of its property threads.`,
    );
  }

  const threadId = interaction.channel_id ?? null;
  const link = options.get('link') ?? null;

  switch (sub) {
    case 'add': {
      if (!link) return replyEphemeral('`/house add` needs a `link`.');
      deps.waitUntil(
        runDeferred(deps, interaction, async () => {
          const r = await deps.service.add({
            link,
            guildId: interaction.guild_id ?? null,
          });
          return r.message;
        }),
      );
      return deferEphemeral();
    }

    case 'bind': {
      if (!link) return replyEphemeral('`/house bind` needs a `link`.');
      const parentId = interaction.channel?.parent_id ?? null;
      if (!threadId || !parentId) {
        return replyEphemeral(
          'run `/house bind` inside the thread you want to bind, not in the channel itself.',
        );
      }
      deps.waitUntil(
        runDeferred(deps, interaction, async () => {
          const r = await deps.service.bind({
            link,
            guildId: interaction.guild_id ?? null,
            threadId,
            parentChannelId: parentId,
            currentName: interaction.channel?.name ?? null,
          });
          return r.message;
        }),
      );
      return deferEphemeral();
    }

    case 'update': {
      deps.waitUntil(
        runDeferred(deps, interaction, async () => {
          const row = await resolveProperty(deps, threadId, link);
          if (!row) return propertyMissingText(link);
          const outcome = await deps.service.refresh(row, 'manual', link ?? undefined);
          switch (outcome.kind) {
            case 'changed':
              return `updated — ${outcome.changes.length} field(s) changed; notice posted in the thread.`;
            case 'unchanged':
              return 'checked — nothing material changed.';
            case 'not-modified':
              return 'checked — the listing page reported no changes since last fetch.';
            case 'error':
              return `refresh failed: ${outcome.detail}`;
          }
        }),
      );
      return deferEphemeral();
    }

    case 'close': {
      deps.waitUntil(
        runDeferred(deps, interaction, async () => {
          const row = threadId ? await deps.repo.getByThreadId(threadId) : null;
          if (!row) return propertyMissingText(null);
          const r = await deps.service.close(row);
          return r.message;
        }),
      );
      return deferEphemeral();
    }

    case 'open': {
      deps.waitUntil(
        runDeferred(deps, interaction, async () => {
          const row = threadId ? await deps.repo.getByThreadId(threadId) : null;
          if (!row) return propertyMissingText(null);
          const r = await deps.service.open(row);
          return r.message;
        }),
      );
      return deferEphemeral();
    }

    case 'status': {
      // No fetch, so answer inline instead of deferring.
      const row = threadId ? await deps.repo.getByThreadId(threadId) : null;
      if (!row) return replyEphemeral(propertyMissingText(null));
      return replyEphemeral(deps.service.status(row).message);
    }

    default:
      return replyEphemeral(`unknown subcommand \`${sub}\`.`);
  }
}

async function resolveProperty(
  deps: InteractionDeps,
  threadId: string | null,
  link: string | null,
): Promise<PropertyRow | null> {
  if (threadId) {
    const byThread = await deps.repo.getByThreadId(threadId);
    if (byThread) return byThread;
  }
  if (link) {
    const { identifyUrl } = await import('../listing/url');
    const ident = identifyUrl(link);
    if (ident) return deps.repo.getByListingKey(ident.listingKey);
  }
  return null;
}

function propertyMissingText(link: string | null): string {
  return link
    ? "I don't have a house tracked for that link. Use `/house add` first."
    : 'run this inside a house thread (or pass a `link` I already track).';
}

/** Deferred work: always land a follow-up, even on unexpected errors. */
async function runDeferred(
  deps: InteractionDeps,
  interaction: Interaction,
  work: () => Promise<string>,
): Promise<void> {
  let message: string;
  try {
    message = await work();
  } catch (err) {
    console.error('house command failed', {
      interaction: interaction.id,
      error: err instanceof Error ? err.message : String(err),
    });
    message = `something broke while handling that: ${
      err instanceof Error ? err.message : 'unknown error'
    }`;
  }
  try {
    await deps.editOriginal(interaction.token, message);
  } catch (err) {
    console.error('follow-up failed', {
      interaction: interaction.id,
      error: err instanceof Error ? err.message : String(err),
    });
  }
}
