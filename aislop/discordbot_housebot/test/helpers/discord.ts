import { DiscordRest } from '../../src/discord/rest';

export interface RecordedCall {
  method: string;
  path: string;
  body: unknown;
}

/**
 * DiscordRest backed by a recording fetch. No network, and every call the bot
 * makes is asserted against in the integration tests.
 */
export function fakeRest(opts: { channelType?: number } = {}): {
  rest: DiscordRest;
  calls: RecordedCall[];
} {
  const calls: RecordedCall[] = [];
  let threadSeq = 0;

  const fetchImpl: typeof fetch = async (input, init) => {
    const url = typeof input === 'string' ? input : (input as Request).url;
    const path = url.replace('https://discord.test', '');
    const method = init?.method ?? 'GET';
    const body = init?.body ? JSON.parse(String(init.body)) : undefined;
    calls.push({ method, path, body });

    if (method === 'POST' && /\/channels\/\d+\/threads$/.test(path)) {
      // Mirror Discord: forum/media channels reject a body with no `message`,
      // text channels reject one that has it.
      const isForum = opts.channelType === 15 || opts.channelType === 16;
      if (isForum !== Boolean(body?.message)) {
        return new Response(
          JSON.stringify({
            message: 'Invalid Form Body',
            code: 50035,
            errors: isForum
              ? { message: { _errors: [{ code: 'BASE_TYPE_REQUIRED' }] } }
              : { type: { _errors: [{ code: 'BASE_TYPE_REQUIRED' }] } },
          }),
          { status: 400, headers: { 'Content-Type': 'application/json' } },
        );
      }
      threadSeq += 1;
      return new Response(
        JSON.stringify({ id: `900000000000000${threadSeq}`, name: body?.name }),
        { status: 200, headers: { 'Content-Type': 'application/json' } },
      );
    }

    if (method === 'GET' && /\/channels\/\d+$/.test(path)) {
      return new Response(JSON.stringify({ id: '1', type: opts.channelType ?? 0 }), {
        status: 200,
        headers: { 'Content-Type': 'application/json' },
      });
    }
    return new Response(JSON.stringify({ id: '1' }), {
      status: 200,
      headers: { 'Content-Type': 'application/json' },
    });
  };

  const rest = new DiscordRest({
    botToken: 'test-token',
    applicationId: '1234',
    fetchImpl,
    apiBase: 'https://discord.test',
  });
  return { rest, calls };
}

export function htmlResponse(html: string, headers: Record<string, string> = {}): Response {
  return new Response(html, {
    status: 200,
    headers: { 'Content-Type': 'text/html', ...headers },
  });
}
