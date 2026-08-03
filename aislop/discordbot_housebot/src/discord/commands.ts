import { ApplicationCommandOptionType } from './types';

/** Guild-scoped `/house` command definition. Registration is a separate script. */
export const HOUSE_COMMAND = {
  name: 'house',
  description: 'Track property listings in per-house threads',
  // Guild install, chat input.
  type: 1,
  options: [
    {
      type: ApplicationCommandOptionType.SUB_COMMAND,
      name: 'add',
      description: 'Track a Zillow or Redfin listing and open a thread for it',
      options: [
        {
          type: ApplicationCommandOptionType.STRING,
          name: 'link',
          description: 'Zillow or Redfin listing URL',
          required: true,
        },
      ],
    },
    {
      type: ApplicationCommandOptionType.SUB_COMMAND,
      name: 'update',
      description: 'Refresh this house from its listing (run inside a house thread)',
      options: [
        {
          type: ApplicationCommandOptionType.STRING,
          name: 'link',
          description: 'Optional explicit listing URL to refresh instead',
          required: false,
        },
      ],
    },
    {
      type: ApplicationCommandOptionType.SUB_COMMAND,
      name: 'close',
      description: 'Force this house closed (run inside a house thread)',
    },
    {
      type: ApplicationCommandOptionType.SUB_COMMAND,
      name: 'open',
      description: 'Re-open this house if the listing is not sold/closed',
    },
    {
      type: ApplicationCommandOptionType.SUB_COMMAND,
      name: 'status',
      description: 'Show stored info for this house without fetching anything',
    },
  ],
} as const;

export const COMMANDS = [HOUSE_COMMAND];
