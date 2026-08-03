import { ApplicationCommandOptionType } from './types';

/** Guild-scoped `/house` command definition. Registration is a separate script. */
export const HOUSE_COMMAND = {
  name: 'house',
  description: 'Track property listings in per-house threads',
  // Guild install, chat input.
  type: 1,
  options: [
    {
      type: ApplicationCommandOptionType.Subcommand,
      name: 'add',
      description: 'Track a Zillow or Redfin listing and open a thread for it',
      options: [
        {
          type: ApplicationCommandOptionType.String,
          name: 'link',
          description: 'Zillow or Redfin listing URL',
          required: true,
        },
      ],
    },
    {
      type: ApplicationCommandOptionType.Subcommand,
      name: 'bind',
      description: 'Bind THIS existing thread to a listing and post the first snapshot',
      options: [
        {
          type: ApplicationCommandOptionType.String,
          name: 'link',
          description: 'Zillow or Redfin listing URL',
          required: true,
        },
      ],
    },
    {
      type: ApplicationCommandOptionType.Subcommand,
      name: 'update',
      description: 'Refresh this house from its listing (run inside a house thread)',
      options: [
        {
          type: ApplicationCommandOptionType.String,
          name: 'link',
          description: 'Optional explicit listing URL to refresh instead',
          required: false,
        },
        {
          type: ApplicationCommandOptionType.Boolean,
          name: 'reenrich',
          description: 'Recompute commute + heating even if already known (uses API quota)',
          required: false,
        },
      ],
    },
    {
      type: ApplicationCommandOptionType.Subcommand,
      name: 'close',
      description: 'Force this house closed (run inside a house thread)',
    },
    {
      type: ApplicationCommandOptionType.Subcommand,
      name: 'open',
      description: 'Re-open this house if the listing is not sold/closed',
    },
    {
      type: ApplicationCommandOptionType.Subcommand,
      name: 'info',
      description: 'Show everything known about this house without fetching anything',
    },
  ],
} as const;

export const COMMANDS = [HOUSE_COMMAND];
