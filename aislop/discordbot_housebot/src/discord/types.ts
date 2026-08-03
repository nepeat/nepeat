/**
 * Discord wire types.
 *
 * Enums and payload shapes come from `discord-api-types/v10` rather than being
 * hand-rolled — hand-rolled constants are exactly how the forum-channel bug got
 * shipped: `{name, type: PublicThread}` is a valid *text-channel* thread body,
 * and nothing checked that a forum channel needs a starter `message`.
 */
export {
  ApplicationCommandOptionType,
  ApplicationCommandType,
  ChannelType,
  InteractionResponseType,
  InteractionType,
  MessageFlags,
} from 'discord-api-types/v10';

export type {
  APIEmbed,
  APIInteractionResponseCallbackData,
  RESTPostAPIChannelMessageJSONBody,
  RESTPostAPIChannelThreadsJSONBody,
  RESTPostAPIGuildForumThreadsJSONBody,
} from 'discord-api-types/v10';

import { ApplicationCommandOptionType, ChannelType } from 'discord-api-types/v10';

/** Channels whose threads must be created with a starter message. */
export function requiresStarterMessage(type: number | undefined): boolean {
  return type === ChannelType.GuildForum || type === ChannelType.GuildMedia;
}

/** The thread type to request for a given parent channel. */
export function threadTypeFor(parentType: number | undefined): ChannelType {
  return parentType === ChannelType.GuildAnnouncement
    ? ChannelType.AnnouncementThread
    : ChannelType.PublicThread;
}

export interface InteractionOption {
  name: string;
  type: number;
  value?: string | number | boolean;
  options?: InteractionOption[];
}

export interface InteractionChannel {
  id: string;
  type: number;
  name?: string;
  parent_id?: string | null;
}

/**
 * A narrowed view of APIInteraction: only the fields housebot reads. The full
 * union carries every interaction kind and would obscure what we depend on.
 */
export interface Interaction {
  id: string;
  type: number;
  token: string;
  application_id: string;
  guild_id?: string;
  channel_id?: string;
  channel?: InteractionChannel;
  member?: { user?: { id: string; username?: string } };
  user?: { id: string; username?: string };
  data?: {
    id?: string;
    name?: string;
    options?: InteractionOption[];
  };
}

export function subcommandOf(interaction: Interaction): {
  name: string | null;
  options: Map<string, string>;
} {
  const sub = interaction.data?.options?.find(
    (o) => o.type === ApplicationCommandOptionType.Subcommand,
  );
  const options = new Map<string, string>();
  for (const o of sub?.options ?? []) {
    if (o.value !== undefined) options.set(o.name, String(o.value));
  }
  return { name: sub?.name ?? null, options };
}

export function actorId(interaction: Interaction): string | null {
  return interaction.member?.user?.id ?? interaction.user?.id ?? null;
}
