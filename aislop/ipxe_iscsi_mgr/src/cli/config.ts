import { readFileSync, existsSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

export interface Config {
  mgrUrl: string;
  adminToken: string;
  bootSecret?: string;
  /** ssh destination for the ZFS/LIO host, e.g. root@10.36.75.5. */
  storageSsh?: string;
}

export const projectRoot = resolve(dirname(fileURLToPath(import.meta.url)), '../..');

/** Minimal KEY=VALUE reader for .dev.vars -- same format wrangler uses. */
function readDotVars(path: string): Record<string, string> {
  if (!existsSync(path)) return {};
  const out: Record<string, string> = {};
  for (const line of readFileSync(path, 'utf8').split('\n')) {
    const m = /^\s*([A-Z0-9_]+)\s*=\s*(.*)$/.exec(line);
    if (m) out[m[1]!] = m[2]!.trim().replace(/^["']|["']$/g, '');
  }
  return out;
}

/** Pull `vars.MGR_URL` out of wrangler.jsonc so it is configured in one place. */
function mgrUrlFromWrangler(): string | undefined {
  const p = resolve(projectRoot, 'wrangler.jsonc');
  if (!existsSync(p)) return undefined;
  try {
    const stripped = readFileSync(p, 'utf8').replace(/^\s*\/\/.*$/gm, '');
    return JSON.parse(stripped)?.vars?.MGR_URL;
  } catch {
    return undefined;
  }
}

export function loadConfig(): Config {
  const vars = { ...readDotVars(resolve(projectRoot, '.dev.vars')), ...process.env } as Record<string, string>;
  const mgrUrl = vars.MGR_URL || mgrUrlFromWrangler();
  if (!mgrUrl) throw new Error('MGR_URL not set (put it in .dev.vars or wrangler.jsonc vars)');
  if (!vars.ADMIN_TOKEN) throw new Error('ADMIN_TOKEN not set (put it in .dev.vars or the environment)');
  return {
    mgrUrl: mgrUrl.replace(/\/+$/, ''),
    adminToken: vars.ADMIN_TOKEN,
    bootSecret: vars.BOOT_SECRET,
    storageSsh: vars.STORAGE_SSH,
  };
}
