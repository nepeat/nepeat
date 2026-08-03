/**
 * Register (or print) the guild-scoped `/house` command.
 *
 *   npm run commands:print     # dry run, prints the exact request, no network
 *   npm run commands:register  # PUTs to Discord (needs env below)
 *
 * Env: DISCORD_APPLICATION_ID, DISCORD_GUILD_ID, DISCORD_BOT_TOKEN
 * Read from the process environment or from ./.dev.vars if present.
 */
import { readFileSync } from 'node:fs';
import { COMMANDS } from '../src/discord/commands';

function loadDevVars(): Record<string, string> {
  try {
    const text = readFileSync(new URL('../.dev.vars', import.meta.url), 'utf8');
    const out: Record<string, string> = {};
    for (const line of text.split('\n')) {
      const m = /^\s*([A-Z0-9_]+)\s*=\s*(.*)$/.exec(line);
      if (m && m[1]) out[m[1]] = (m[2] ?? '').trim().replace(/^["']|["']$/g, '');
    }
    return out;
  } catch {
    return {};
  }
}

async function main(): Promise<void> {
  const dryRun = process.argv.includes('--dry-run');
  const vars = { ...loadDevVars(), ...process.env } as Record<string, string | undefined>;

  const appId = vars['DISCORD_APPLICATION_ID'];
  const guildId = vars['DISCORD_GUILD_ID'];
  const token = vars['DISCORD_BOT_TOKEN'];

  const path =
    appId && guildId
      ? `/applications/${appId}/guilds/${guildId}/commands`
      : '/applications/<DISCORD_APPLICATION_ID>/guilds/<DISCORD_GUILD_ID>/commands';

  if (dryRun) {
    console.log(`PUT https://discord.com/api/v10${path}`);
    console.log('Authorization: Bot <DISCORD_BOT_TOKEN>');
    console.log(JSON.stringify(COMMANDS, null, 2));
    console.log(
      `\n(dry run — nothing sent. app id ${appId ? 'found' : 'MISSING'}, guild id ${
        guildId ? 'found' : 'MISSING'
      }, token ${token ? 'found' : 'MISSING'})`,
    );
    return;
  }

  if (!appId || !guildId || !token) {
    console.error(
      'missing DISCORD_APPLICATION_ID / DISCORD_GUILD_ID / DISCORD_BOT_TOKEN (set them in .dev.vars or the environment)',
    );
    process.exitCode = 1;
    return;
  }

  const res = await fetch(`https://discord.com/api/v10${path}`, {
    method: 'PUT',
    headers: {
      Authorization: `Bot ${token}`,
      'Content-Type': 'application/json',
    },
    body: JSON.stringify(COMMANDS),
  });

  const text = await res.text();
  if (!res.ok) {
    console.error(`registration failed: HTTP ${res.status}\n${text}`);
    process.exitCode = 1;
    return;
  }
  console.log(`registered ${COMMANDS.length} command(s) in guild ${guildId}`);
}

void main();
