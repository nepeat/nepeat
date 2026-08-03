export const InteractionType = {
  PING: 1,
  APPLICATION_COMMAND: 2,
  MESSAGE_COMPONENT: 3,
  APPLICATION_COMMAND_AUTOCOMPLETE: 4,
  MODAL_SUBMIT: 5,
} as const;

export const InteractionResponseType = {
  PONG: 1,
  CHANNEL_MESSAGE_WITH_SOURCE: 4,
  DEFERRED_CHANNEL_MESSAGE_WITH_SOURCE: 5,
} as const;

export const MessageFlags = {
  EPHEMERAL: 1 << 6,
} as const;

export const ApplicationCommandOptionType = {
  SUB_COMMAND: 1,
  STRING: 3,
  BOOLEAN: 5,
} as const;

export const ChannelType = {
  GUILD_TEXT: 0,
  PUBLIC_THREAD: 11,
} as const;

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
    (o) => o.type === ApplicationCommandOptionType.SUB_COMMAND,
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
