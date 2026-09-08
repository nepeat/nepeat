import type { Env } from './env.js';
import { now, sha256Hex, timingSafeEqual } from './env.js';

const CACHE_PREFIX = 'sechash:';
const CACHE_TTL = 300;

/**
 * Extract the boot secret. iPXE can put credentials in the URI userinfo, which
 * becomes HTTP Basic -- preferred, since it keeps the secret out of request
 * paths and therefore out of most logs. `X-Boot-Secret` is accepted for curl
 * testing.
 */
export function extractBootSecret(req: Request): string | null {
  const header = req.headers.get('authorization');
  if (header?.toLowerCase().startsWith('basic ')) {
    try {
      const decoded = atob(header.slice(6).trim());
      const idx = decoded.indexOf(':');
      return idx === -1 ? decoded : decoded.slice(idx + 1);
    } catch {
      return null;
    }
  }
  return req.headers.get('x-boot-secret');
}

/** True if the presented secret matches a live row in `secrets`. */
export async function verifyBootSecret(env: Env, secret: string | null): Promise<boolean> {
  if (!secret) return false;
  const hash = await sha256Hex(secret);

  const cached = await env.SECRET_CACHE.get(CACHE_PREFIX + hash);
  if (cached === 'ok') return true;
  if (cached === 'no') return false;

  const row = await env.DB.prepare(
    `SELECT id, hash FROM secrets WHERE hash = ?1 AND disabled_at IS NULL LIMIT 1`,
  )
    .bind(hash)
    .first<{ id: string; hash: string }>();

  const ok = !!row && timingSafeEqual(row.hash, hash);
  await env.SECRET_CACHE.put(CACHE_PREFIX + hash, ok ? 'ok' : 'no', { expirationTtl: CACHE_TTL });
  if (ok) {
    await env.DB.prepare(`UPDATE secrets SET last_used_at = ?1 WHERE id = ?2`).bind(now(), row!.id).run();
  }
  return ok;
}

/** Admin API uses a separate credential from the boot path: different audience. */
export async function verifyAdmin(req: Request, env: Env): Promise<boolean> {
  const header = req.headers.get('authorization');
  if (!header?.toLowerCase().startsWith('bearer ')) return false;
  const token = header.slice(7).trim();
  if (!env.ADMIN_TOKEN || !token) return false;
  // Compare digests so the comparison does not leak token length.
  return timingSafeEqual(await sha256Hex(token), await sha256Hex(env.ADMIN_TOKEN));
}
