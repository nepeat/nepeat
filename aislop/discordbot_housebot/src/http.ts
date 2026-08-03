/**
 * workerd throws "Illegal invocation: function called with incorrect `this`
 * reference" when `fetch` is called detached from globalThis -- which is what
 * `this.fetchImpl = opts.fetchImpl ?? fetch` quietly does. Node's fetch tolerates
 * it, so this only ever fails in production. Always route through here.
 */
export function boundFetch(impl?: typeof fetch): typeof fetch {
  return (impl ?? fetch).bind(globalThis);
}

/** Error text plus stack, trimmed for a Discord message. */
export function formatError(err: unknown, limit = 1500): string {
  if (err instanceof Error) {
    const body = err.stack ?? `${err.name}: ${err.message}`;
    return body.length > limit ? `${body.slice(0, limit)}\n… (truncated)` : body;
  }
  const text = String(err);
  return text.length > limit ? `${text.slice(0, limit)}\n… (truncated)` : text;
}
