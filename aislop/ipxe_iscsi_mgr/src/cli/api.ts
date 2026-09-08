import type { Config } from './config.js';

export class ApiError extends Error {
  constructor(readonly status: number, readonly body: string) {
    super(`HTTP ${status}: ${body}`);
  }
}

export async function api<T = any>(
  cfg: Config,
  method: string,
  path: string,
  body?: unknown,
): Promise<T> {
  const res = await fetch(`${cfg.mgrUrl}${path}`, {
    method,
    headers: {
      authorization: `Bearer ${cfg.adminToken}`,
      ...(body === undefined ? {} : { 'content-type': 'application/json' }),
    },
    ...(body === undefined ? {} : { body: JSON.stringify(body) }),
  });
  const text = await res.text();
  if (!res.ok) throw new ApiError(res.status, text);
  const ct = res.headers.get('content-type') ?? '';
  return (ct.includes('json') ? JSON.parse(text) : text) as T;
}
